import initEngine, { Engine } from '../../../../web/pkg/engine_wasm.js';

function post(jobId, type, payload = {}, transfer = []) {
  self.postMessage({ protocol: 'rusted-utau-render-job/1', jobId, type, ...payload }, transfer);
}

function bytesOf(asset) {
  if (asset.bytes instanceof Uint8Array) return asset.bytes;
  if (asset.bytes instanceof ArrayBuffer) return new Uint8Array(asset.bytes);
  throw new Error(`Render asset ${asset.kind}:${asset.key} has no transferred bytes`);
}

self.onmessage = async ({ data }) => {
  if (!data || data.type !== 'render' || typeof data.jobId !== 'string' || !data.snapshot || !Array.isArray(data.assets)) {
    post(data?.jobId ?? null, 'error', { code: 'INVALID_RENDER_REQUEST', message: 'Invalid immutable render request' });
    return;
  }
  const { jobId, snapshot, assets } = data;
  let engine = null;
  let phase = 'booting';
  let wasmMemory = null;
  let assetBytes = 0;
  let knownBytesHighWater = 0;
  const sampleMemory = outputBytes => {
    const wasmLinearMemoryBytes = wasmMemory?.buffer?.byteLength || 0;
    const knownBytes = wasmLinearMemoryBytes + assetBytes + outputBytes;
    knownBytesHighWater = Math.max(knownBytesHighWater, knownBytes);
    return { wasmLinearMemoryBytes, transferredAssetBytes: assetBytes, outputPcmBytes: outputBytes, knownBytes, knownBytesHighWater };
  };
  try {
    post(jobId, 'progress', { phase: 'booting', fraction: 0 });
    const wasmExports = await initEngine();
    wasmMemory = wasmExports.memory;
    assetBytes = assets.reduce((sum, asset) => sum + bytesOf(asset).byteLength, 0);
    engine = new Engine();
    phase = 'opening-session';
    const opened = JSON.parse(engine.session_open_yaml_with_sidecars(
      snapshot.ustx, snapshot.identitySidecar, snapshot.pronunciationSidecar,
    ));
    if (opened?.error) throw Object.assign(new Error(opened.error.message || opened.error.code), { code: opened.error.code });

    post(jobId, 'progress', { phase: 'loading-assets', fraction: 0.1, memory: sampleMemory(0) });
    phase = 'loading-voicebank';
    const oto = assets.find(asset => asset.kind === 'oto');
    const hasVoiceParts = (snapshot.projectSnapshot?.project?.tracks || [])
      .some(track => (track.parts || []).some(part => part.kind === 'voice'));
    if (!oto && hasVoiceParts) throw Object.assign(new Error('Pinned voicebank OTO bytes are missing'), { code: 'MISSING_OTO' });
    // Engine's full-project entrypoint currently requires a Voicebank handle
    // even when the project contains only audio parts. An empty bank is safe in
    // that case because it cannot be consulted for synthesis.
    engine.set_voicebank('render-snapshot', 'Pinned render voicebank', data.otoDir || '', oto ? bytesOf(oto) : new Uint8Array());
    const samples = new Map();
    for (const asset of assets) {
      if (asset.kind === 'sample-wav' || asset.kind === 'sample-frq') {
        const sample = samples.get(asset.key) || { wav: null, frq: null };
        if (asset.kind === 'sample-wav') sample.wav = bytesOf(asset);
        else sample.frq = bytesOf(asset);
        samples.set(asset.key, sample);
      }
    }
    for (const [key, sample] of samples) {
      if (!sample.wav) throw Object.assign(new Error(`Pinned WAV is missing for ${key}`), { code: 'MISSING_SAMPLE_WAV' });
      phase = 'loading-samples';
      if (sample.frq) engine.add_sample_with_frq(key, sample.wav, sample.frq);
      else engine.add_sample(key, sample.wav);
    }
    phase = 'registering-media';
    for (const asset of assets) {
      if (asset.kind === 'project-media') engine.session_register_media(asset.key, bytesOf(asset));
    }

    const voiceTrackIndices = (snapshot.projectSnapshot?.project?.tracks || [])
      .map((track, index) => (track.parts || []).some(part => part.kind === 'voice') ? index : -1)
      .filter(index => index >= 0);
    if (voiceTrackIndices.length) {
      const samplePlan = JSON.parse(engine.session_render_sample_keys_json(JSON.stringify(voiceTrackIndices)));
      if (samplePlan.sessionId !== engine.session_id() || Number(samplePlan.revision) !== Number(engine.session_revision())) {
        throw Object.assign(new Error('Rust sample plan does not match the worker Session snapshot'), { code: 'STALE_RENDER_RESULT' });
      }
      const missing = samplePlan.sampleKeys.filter(key => !samples.has(key));
      if (missing.length) {
        throw Object.assign(new Error(`Pinned render snapshot is missing pronunciation-aware voice samples: ${missing.slice(0, 8).join(', ')}`), { code: 'MISSING_RENDER_SAMPLES' });
      }
    }

    post(jobId, 'progress', { phase: 'rendering', fraction: 0.2, memory: sampleMemory(0) });
    phase = 'mixdown';
    // Full-project WASM mixdown is synchronous. The host can hard-cancel by
    // terminating this dedicated worker; message-based soft cancel cannot be
    // observed until the Rust call returns.
    const pcm = engine.session_render_mixdown();
    const output = pcm instanceof Float32Array ? pcm : Float32Array.from(pcm);
    const memory = sampleMemory(output.byteLength);
    post(jobId, 'complete', {
      sampleRate: 44100,
      frames: output.length / 2,
      pcm: output.buffer,
      assetManifest: data.assetManifest,
      assetFingerprint: data.assetFingerprint,
      memory,
    }, [output.buffer]);
  } catch (error) {
    post(jobId, 'error', { code: error?.code || 'RENDER_FAILED', phase, message: error?.message || String(error), stack: error?.stack || null });
  } finally {
    engine?.free?.();
  }
};
