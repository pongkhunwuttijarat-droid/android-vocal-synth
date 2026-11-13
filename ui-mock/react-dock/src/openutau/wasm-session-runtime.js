// Lives inside wasm-session.worker.js. Rust remains authoritative; this class
// only adapts the host's OPFS bank/media and PCM buffers to SessionDriver.
import { SessionDriver } from '../../../../web/app/session-driver.js';
import initEngine from '../../../../web/pkg/engine_wasm.js';
import { frqKeyFor, HttpBank, OpfsBank } from '../../../../web/samples.js';
import { FIXTURE_OTO_DIR, FIXTURE_OTO_PATH, fixtureBankBaseUrl } from '../../../../web/fixture-bank.js';
import { captureCommittedRenderSnapshot, fingerprintRenderJob, pinRenderAssetBytes } from '../../../../web/app/session-render-contract.js';
import { buildInterleavedStereoPeaks, buildPeaks } from './pcm-analysis.js';

const SAMPLE_RATE = 44100;

function stereoRender(interleaved, { waveformOnly = false } = {}) {
  const pcm = interleaved instanceof Float32Array ? interleaved : Float32Array.from(interleaved);
  if (pcm.length % 2 !== 0) throw new Error('Rust session render returned an incomplete stereo frame');
  const frames = pcm.length / 2;
  if (waveformOnly) {
    // The Rust result is already interleaved stereo. Reduce it in place rather
    // than allocating two channel copies plus a mono analysis buffer just to
    // draw an envelope (long audio parts can contain millions of frames).
    return { sampleRate: SAMPLE_RATE, frames, peaks: buildInterleavedStereoPeaks(pcm, SAMPLE_RATE) };
  }
  const left = new Float32Array(frames);
  const right = new Float32Array(frames);
  const samples = new Float32Array(frames);
  for (let frame = 0; frame < frames; frame++) {
    left[frame] = pcm[frame * 2];
    right[frame] = pcm[frame * 2 + 1];
    samples[frame] = (left[frame] + right[frame]) * 0.5;
  }
  return {
    sampleRate: SAMPLE_RATE,
    samples,
    channels: [left, right],
    peaks: buildPeaks([left, right], SAMPLE_RATE),
  };
}

export class WasmSessionRuntime {
  constructor(driver, wasmExports = null) {
    this.driver = driver;
    this._wasmExports = wasmExports;
    this._bankKey = null;
    this._loadedSampleKeys = new Set();
    this._bank = null;
    this._renderJob = null;
    this._renderPreparing = false;
    this._renderCancelled = false;
    this.onRenderProgress = null;
  }
  static async boot() {
    // Retain wasm-bindgen's exports for host diagnostics. Project state remains
    // owned by the Rust Engine accessed through SessionDriver.
    const wasmExports = await initEngine();
    return new WasmSessionRuntime(await SessionDriver.boot(), wasmExports);
  }
  get state() { return this.driver.state(); }
  memoryStats() { return { wasmLinearMemoryBytes: this._wasmExports?.memory?.buffer?.byteLength ?? null }; }
  onChange(fn) { return this.driver.onChange(fn); }
  partRoutes() {
    return (this.state.snapshot?.project?.tracks || []).flatMap(track =>
      (track.parts || []).filter(part => part.kind === 'voice').map(part => ({
        trackId: track.id, partId: part.id, trackName: track.name, partName: part.name,
      })),
    );
  }
  readNotes(partId) { return JSON.parse(this.driver.engine.session_part_notes_json(partId)); }
  readTracks() { return JSON.parse(this.driver.engine.session_tracks_json()); }
  readCurveStrokeContext(partId, curveId) {
    return JSON.parse(this.driver.engine.session_curve_stroke_context_json(partId, curveId));
  }
  createProject(options) { return this.driver.create(options); }
  openStored(name) { return this.driver.openStored(name); }
  importUstx(ustx) { return this.driver.importUstx(ustx); }
  apply(command) { return this.driver.apply(command); }
  undo() { return this.driver.undo(); }
  redo() { return this.driver.redo(); }
  beginGroup(name) { return this.driver.beginGroup(name); }
  applyGrouped(groupId, command) { return this.driver.apply({ group_id: groupId, command }); }
  commitGroup(groupId) { return this.driver.commitGroup(groupId); }
  cancelGroup(groupId) { return this.driver.cancelGroup(groupId); }
  async save(name) {
    const result = await this.driver.save(name);
    this.driver._notify('save', result);
    return result;
  }
  listProjects() { return this.driver.store.list(); }
  async renderTrack(track, options = {}) {
    // A waveform preview only renders one track. Don't plan and fetch samples
    // for every other voice track before that selected-track render can start.
    await this.prepareRenderBank([track]);
    return stereoRender(this.driver.renderTrack(track), options);
  }
  async renderMixdown() {
    if (this._renderJob || this._renderPreparing) throw Object.assign(new Error('A full-project render is already running'), { code: 'RENDER_BUSY' });
    this._renderPreparing = true;
    this._renderCancelled = false;
    try { return await this.executeMixdownJob(); }
    finally { this._renderPreparing = false; this._renderCancelled = false; }
  }
  async executeMixdownJob() {
    const engine = this.driver.engine;
    const snapshot = captureCommittedRenderSnapshot(engine, this.driver.revision);
    const project = snapshot.projectSnapshot.project;
    const projectTracks = project.tracks || [];
    const renderInputs = await this.prepareRenderBank(null, project, snapshot.ustx, snapshot);
    this.assertSnapshotCurrent(snapshot);

    const assets = [];
    if (renderInputs.otoBytes) assets.push({ kind: 'oto', key: renderInputs.bankKey, bytes: renderInputs.otoBytes });
    for (const sample of renderInputs.samples) {
      assets.push({ kind: 'sample-wav', key: sample.key, bytes: sample.wav });
      if (sample.frq) assets.push({ kind: 'sample-frq', key: sample.key, bytes: sample.frq });
    }
    const mediaRefs = new Set(projectTracks.flatMap(track => (track.parts || [])
      .filter(part => part.kind === 'wave' && typeof part.media_ref === 'string' && part.media_ref)
      .map(part => part.media_ref)));
    for (const mediaRef of mediaRefs) {
      if (!mediaRef.startsWith('asset:')) {
        throw Object.assign(new Error(`Render media reference cannot be resolved from project storage: ${mediaRef}`), { code: 'UNRESOLVED_PROJECT_MEDIA' });
      }
      const mediaId = mediaRef.slice('asset:'.length);
      const bytes = await this.driver.store.loadMedia(mediaId);
      assets.push({ kind: 'project-media', key: mediaRef, bytes });
    }
    const pinned = await pinRenderAssetBytes(assets);
    const renderConfig = { sampleRate: SAMPLE_RATE, mixdown: 'rust-session-stereo-v1', voicebankOtoDir: renderInputs.otoDir || '' };
    const assetFingerprint = await fingerprintRenderJob(snapshot, pinned.manifest, renderConfig);
    this.assertSnapshotCurrent(snapshot);
    const nonce = globalThis.crypto?.randomUUID?.() || `${Date.now()}-${Math.random().toString(16).slice(2)}`;
    const jobId = `render-${assetFingerprint.slice(0, 16)}-${nonce}`;
    const worker = new Worker(new URL('./wasm-render.worker.js', import.meta.url), { type: 'module' });
    const job = { jobId, snapshot, assetFingerprint, worker };
    this._renderJob = job;
    this.onRenderProgress?.({ jobId, sessionId: snapshot.sessionId, revision: snapshot.revision, phase: 'queued', fraction: 0 });
    try {
      const rendered = await new Promise((resolve, reject) => {
        job.reject = reject;
        worker.onmessage = ({ data }) => {
          if (data?.protocol !== 'rusted-utau-render-job/1' || data.jobId !== jobId) return;
          if (data.type === 'progress') {
            this.onRenderProgress?.({ jobId, sessionId: snapshot.sessionId, revision: snapshot.revision, ...data });
          } else if (data.type === 'error') {
            reject(Object.assign(new Error(data.message || 'Render worker failed'), { code: data.code || 'RENDER_FAILED', phase: data.phase }));
          } else if (data.type === 'complete') {
            if (data.assetFingerprint !== assetFingerprint || JSON.stringify(data.assetManifest) !== JSON.stringify(pinned.manifest)) {
              reject(Object.assign(new Error('Render worker returned a result for a different pinned asset manifest'), { code: 'RENDER_MANIFEST_MISMATCH' }));
            } else if (!(data.pcm instanceof ArrayBuffer) || data.pcm.byteLength % 8 !== 0) {
              reject(Object.assign(new Error('Render worker returned malformed stereo PCM'), { code: 'INVALID_RENDER_PCM' }));
            } else resolve(data);
          }
        };
        worker.onerror = event => reject(Object.assign(new Error(event.message || 'Render worker crashed'), { code: 'RENDER_WORKER_CRASHED' }));
        worker.onmessageerror = () => reject(Object.assign(new Error('Render worker response could not be decoded'), { code: 'RENDER_WORKER_MESSAGE_ERROR' }));
        worker.postMessage({
          type: 'render', jobId, snapshot, assets: pinned.assets,
          assetManifest: pinned.manifest, assetFingerprint, renderConfig, otoDir: renderInputs.otoDir || '',
        }, pinned.transfer);
      });
      this.assertSnapshotCurrent(snapshot);
      await this.assertRenderAssetsCurrent(pinned.manifest, renderInputs.bankKey);
      this.assertSnapshotCurrent(snapshot);
      const audio = stereoRender(new Float32Array(rendered.pcm));
      return {
        ...audio,
        renderJob: Object.freeze({
          jobId, sessionId: snapshot.sessionId, revision: snapshot.revision,
          assetFingerprint, assetManifest: pinned.manifest, memory: rendered.memory, stale: false,
        }),
      };
    } finally {
      worker.terminate();
      if (this._renderJob?.jobId === jobId) this._renderJob = null;
    }
  }
  cancelRenderMixdown(jobId = null) {
    const job = this._renderJob;
    if (!job) {
      if (this._renderPreparing) { this._renderCancelled = true; return true; }
      return false;
    }
    if (jobId && job.jobId !== jobId) return false;
    job.reject?.(Object.assign(new Error('The render was cancelled by terminating its render worker'), { code: 'RENDER_CANCELLED' }));
    job.worker.terminate();
    this._renderJob = null;
    return true;
  }
  assertSnapshotCurrent(snapshot) {
    if (this._renderCancelled) throw Object.assign(new Error('The render was cancelled before dispatch'), { code: 'RENDER_CANCELLED' });
    if (this.driver.sessionId !== snapshot.sessionId || this.driver.revision !== snapshot.revision) {
      throw Object.assign(new Error('The project changed while its render snapshot was being prepared or rendered'), {
        code: 'STALE_RENDER_RESULT', expected_revision: snapshot.revision, current_revision: this.driver.revision,
      });
    }
  }
  async assertRenderAssetsCurrent(expectedManifest, bankKey) {
    let verifyBank = null;
    const otoExpected = expectedManifest.find(asset => asset.kind === 'oto');
    if (otoExpected) {
      const root = bankKey?.startsWith('opfs:') ? bankKey.slice('opfs:'.length) : null;
      verifyBank = root ? new OpfsBank({ root }) : new HttpBank({ baseUrl: fixtureBankBaseUrl() });
      if (root) await verifyBank.openAllOtos();
      else await verifyBank.openOto({ otoPath: FIXTURE_OTO_PATH, otoDir: FIXTURE_OTO_DIR });
    }
    const expectedKeys = new Set(expectedManifest.map(asset => `${asset.kind}\0${asset.key}`));
    for (const expected of expectedManifest) {
      let bytes;
      if (expected.kind === 'oto') bytes = verifyBank?.otoBytes;
      else if (expected.kind === 'sample-wav' || expected.kind === 'sample-frq') {
        if (!verifyBank) throw Object.assign(new Error('Pinned voicebank is no longer available'), { code: 'STALE_RENDER_ASSETS' });
        const key = expected.kind === 'sample-frq' ? expected.key.replace(/\.wav$/i, '_wav.frq') : expected.key;
        const storageKey = key.startsWith('/vb/') ? key.slice(4) : key;
        try { bytes = await verifyBank.raw(storageKey); }
        catch (error) { throw Object.assign(new Error(`Render sample changed or disappeared: ${storageKey}`), { code: 'STALE_RENDER_ASSETS', cause: error }); }
      } else if (expected.kind === 'project-media') {
        try { bytes = await this.driver.store.loadMedia(expected.key.slice('asset:'.length)); }
        catch (error) { throw Object.assign(new Error(`Project media changed or disappeared: ${expected.key}`), { code: 'STALE_RENDER_ASSETS', cause: error }); }
      }
      const [actual] = (await pinRenderAssetBytes([{ kind: expected.kind, key: expected.key, bytes }])).manifest;
      if (actual.sha256 !== expected.sha256 || actual.byteLength !== expected.byteLength) {
        throw Object.assign(new Error(`A render input changed while the job was running: ${expected.kind}:${expected.key}`), { code: 'STALE_RENDER_ASSETS' });
      }
    }
    // A newly added FRQ also changes how the same WAV would render. Detect
    // appearance of a companion that was absent from the pinned job.
    for (const wav of expectedManifest.filter(asset => asset.kind === 'sample-wav')) {
      const frqKey = frqKeyFor(wav.key);
      if (expectedKeys.has(`sample-frq\0${wav.key}`)) continue;
      const storageKey = frqKey.startsWith('/vb/') ? frqKey.slice(4) : frqKey;
      try {
        await verifyBank.raw(storageKey);
        throw Object.assign(new Error(`A sample FRQ appeared during render: ${storageKey}`), { code: 'STALE_RENDER_ASSETS' });
      } catch (error) {
        if (error?.code === 'STALE_RENDER_ASSETS') throw error;
        // Missing companion remains the same exact render configuration.
      }
    }
  }
  async prepareRenderBank(onlyTracks = null, projectOverride = null, ustxOverride = null, snapshotOverride = null) {
    const engine = this.driver.engine;
    const project = projectOverride || this.state.snapshot?.project;
    if (!project) throw new Error('No project session is open');
    const projectTracks = project.tracks || [];
    const requestedTracks = onlyTracks == null
      ? projectTracks.map((_, index) => index)
      : (Array.isArray(onlyTracks) ? onlyTracks : [onlyTracks]);
    const tracksToPlan = [...new Set(requestedTracks.map(Number))];
    if (tracksToPlan.some(track => !Number.isInteger(track) || track < 0 || track >= projectTracks.length)) {
      throw new RangeError('Selected render track is outside the project');
    }
    const voiceTracksToPlan = tracksToPlan.filter(track =>
      (projectTracks[track].parts || []).some(part => part.kind === 'voice'),
    );
    // Audio-only renders need the media already registered with the session, but
    // they do not need a voicebank or sample planning at all.
    if (!voiceTracksToPlan.length) return { bankKey: null, otoDir: '', otoBytes: null, samples: [] };
    const sessionFence = snapshotOverride || { sessionId: engine.session_id(), revision: Number(engine.session_revision()) };
    const assertFence = () => {
      const currentRevision = Number(engine.session_revision());
      if (engine.session_id() !== sessionFence.sessionId || currentRevision !== sessionFence.revision) {
        throw Object.assign(new Error('The project changed while render samples were being prepared'), {
          code: 'STALE_RENDER_RESULT', expected_revision: sessionFence.revision,
          current_revision: currentRevision,
        });
      }
    };

    const roots = await OpfsBank.installedRoots();
    assertFence();
    const requestedSinger = voiceTracksToPlan.map(track => projectTracks[track].singer_id)
      .find(id => typeof id === 'string' && id.trim() && !/^unassigned/i.test(id));
    const requestedRoot = requestedSinger?.split('/')[0];
    const root = roots.find(name => name === requestedRoot)
      || roots.find(name => name.toLowerCase() === requestedRoot?.toLowerCase())
      || roots[0]
      || null;
    const bankKey = root ? `opfs:${root}` : (import.meta.env.DEV ? 'fixture' : null);
    if (!bankKey) throw new Error('Install a voicebank before rendering this project');
    if (bankKey !== this._bankKey) {
      const bank = root
        ? new OpfsBank({ root })
        : new HttpBank({ baseUrl: fixtureBankBaseUrl() });
      let engineOtoDir = FIXTURE_OTO_DIR;
      if (root) {
        await bank.openAllOtos();
        engineOtoDir = '';
      } else {
        await bank.openOto({ otoPath: FIXTURE_OTO_PATH, otoDir: FIXTURE_OTO_DIR });
      }
      assertFence();
      if (!bank.keys.length) throw new Error(`Voicebank “${root || 'dev fixture'}” has no usable aliases`);
      engine.set_voicebank(`session:${bankKey}`, root || 'Dev Fixture', engineOtoDir, new Uint8Array(bank.otoBytes));
      this._bank = bank;
      this._bankKey = bankKey;
      this._loadedSampleKeys.clear();
    }

    const samplePlan = JSON.parse(engine.session_render_sample_keys_json(JSON.stringify(voiceTracksToPlan)));
    assertFence();
    if (samplePlan.sessionId !== sessionFence.sessionId || samplePlan.revision !== sessionFence.revision) {
      throw Object.assign(new Error('Rust returned sample keys for a different Session revision'), { code: 'STALE_RENDER_RESULT' });
    }
    if (!Array.isArray(samplePlan.sampleKeys) || samplePlan.sampleKeys.some(key => typeof key !== 'string')) {
      throw new Error('Rust returned an invalid render sample key list');
    }
    // Keep chunk planning as a conservative compatibility superset for
    // legacy projects whose phrase renderer has partial OTO data. The
    // sidecar-aware list adds aliases selected by committed pronunciations.
    const needed = new Set(samplePlan.sampleKeys);
    const ustx = ustxOverride ?? this.driver.exportUstx();
    for (const track of voiceTracksToPlan) {
      const count = engine.plan_project(ustx, track);
      for (let index = 0; index < count; index++) {
        for (const key of engine.chunk_wav_keys(index).split('\n').filter(Boolean)) needed.add(key);
      }
    }
    const sampleKeys = [...needed].sort();
    const fresh = sampleKeys.filter(key => !this._loadedSampleKeys.has(key));
    const freshSet = new Set(fresh);
    const storageKeys = sampleKeys.map(key => key.startsWith('/vb/') ? key.slice(4) : key);
    const entries = await this._bank.load(storageKeys);
    assertFence();
    for (let index = 0; index < entries.length; index++) {
      const entry = entries[index];
      const engineKey = sampleKeys[index];
      if (freshSet.has(engineKey)) {
        const wav = new Uint8Array(entry.wav);
        if (entry.frq) engine.add_sample_with_frq(engineKey, wav, new Uint8Array(entry.frq));
        else engine.add_sample(engineKey, wav);
        this._loadedSampleKeys.add(engineKey);
      }
    }
    return {
      bankKey, otoDir: this._bank.otoDir || '', otoBytes: new Uint8Array(this._bank.otoBytes),
      samples: entries.map((entry, index) => ({
        key: sampleKeys[index], wav: new Uint8Array(entry.wav),
        frq: entry.frq ? new Uint8Array(entry.frq) : null,
      })),
    };
  }
  async registerMedia(mediaId, bytes) {
    await this.driver.store.saveMedia(mediaId, bytes);
    return this.driver.registerMedia(`asset:${mediaId}`, bytes);
  }
  saveAs(name) { return this.driver.saveAs(name); }
  exportUstx() { return this.driver.exportUstx(); }
  close() {
    this.cancelRenderMixdown();
    return this.driver.close();
  }
}
