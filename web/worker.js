// One render worker: owns a private wasm engine instance, plans the same
// project as the main thread (the plan is deterministic, so the chunk list
// matches), renders the chunk indices it is assigned, and posts the PCM back
// as transferable buffers.
//
// Samples are loaded **incrementally**: the worker starts with no audio at
// all, and `addSamples` pushes only the samples the chunks it owns actually
// need (`engine.chunk_wav_keys`). That is what keeps memory per worker in the
// low MB instead of the whole bank — see docs/architecture/wasm-parallel.md.
//
// No shared memory: each worker has its own wasm instance and its own copy of
// the samples it needs. That is what makes this work without
// SharedArrayBuffer / COOP-COEP headers.
//
// Protocol (all messages carry `id` so replies can be matched):
//   init       {samples: {id,name,otoDir,oto:ArrayBuffer}, projectYaml, track}
//                -> {ready, chunks, wasmInitMs, voicebankLoadMs}
//   addSamples {files: [{key, wav: ArrayBuffer, frq: ArrayBuffer|null}]}
//                -> {added, loadedKeys}      (already-loaded keys are skipped)
//   replan     {projectYaml, track}
//                -> {chunks}
//   render     {indices: [..]}
//                -> {done, chunks: [{index, pcm: ArrayBuffer}]}
//
// `init` also accepts `samples.files` (the older shape) so the existing
// harness pages keep working.

import init, { Engine } from "./pkg/engine_wasm.js";

let engine = null;
let projectYaml = null;
let track = 0;
let engineInitMs = 0;
let sampleLoadMs = 0;
let renderEngine = "r1-rs";
let r2Runtime = null;
let plannedRenderJob = null;
/** Keys currently installed in this worker's engine. */
const loadedKeys = new Set();
const cancelledJobs = new Set();
const activeJobs = new Set();

function yieldToMessages() {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

function installSamples(files) {
  let added = 0;
  for (const f of files) {
    if (!f || loadedKeys.has(f.key)) continue;
    const wav = new Uint8Array(f.wav);
    if (f.frq) engine.add_sample_with_frq(f.key, wav, new Uint8Array(f.frq));
    else engine.add_sample(f.key, wav);
    loadedKeys.add(f.key);
    added += 1;
  }
  return added;
}

async function ensureEngine(samples) {
  if (engine) return engine;
  const t0 = performance.now();
  await init();
  engineInitMs = performance.now() - t0;
  const t1 = performance.now();
  engine = new Engine();
  engine.set_voicebank(samples.id, samples.name, samples.otoDir, new Uint8Array(samples.oto));
  if (samples.files && samples.files.length) installSamples(samples.files);
  sampleLoadMs = performance.now() - t1;
  return engine;
}

self.onmessage = async (ev) => {
  const msg = ev.data;
  try {
    switch (msg.type) {
      case "init": {
        await ensureEngine(msg.samples);
        renderEngine = msg.renderEngine || "r1-rs";
        let r2LoadMs = 0;
        if (renderEngine === "r2-webgpu") {
          const tR2 = performance.now();
          const { R2Runtime } = await import("./app/r2-runtime.js");
          r2Runtime = await R2Runtime.load({ ep: "webgpu" });
          r2LoadMs = performance.now() - tR2;
        }
        projectYaml = msg.projectYaml;
        track = msg.track ?? 0;
        const chunks = engine.plan_project(projectYaml, track);
        self.postMessage({
          type: "ready",
          id: msg.id,
          chunks,
          loadedKeys: loadedKeys.size,
          wasmInitMs: engineInitMs,
          voicebankLoadMs: sampleLoadMs,
          renderEngine,
          r2LoadMs,
          r2Device: r2Runtime?.info.device || null,
        });
        return;
      }

      case "addSamples": {
        const added = installSamples(msg.files || []);
        self.postMessage({ type: "added", id: msg.id, added, loadedKeys: loadedKeys.size });
        return;
      }

      case "replan": {
        if (msg.projectYaml) projectYaml = msg.projectYaml;
        if (msg.track !== undefined) track = msg.track;
        const chunks = engine.plan_project(projectYaml, track);
        plannedRenderJob = msg.renderJob || null;
        self.postMessage({ type: "replanned", id: msg.id, chunks });
        return;
      }

      case "render": {
        if (!engine) {
          if (!msg.samples) throw new Error("worker has no engine (send init first)");
          await ensureEngine(msg.samples);
        }
        if (msg.samples && msg.samples.files) installSamples(msg.samples.files);
        if (msg.jobId && (
          !plannedRenderJob ||
          plannedRenderJob.renderId !== msg.jobId ||
          plannedRenderJob.sessionId !== msg.sessionId ||
          plannedRenderJob.revision !== msg.revision ||
          plannedRenderJob.engine !== msg.engine
        )) {
          throw new Error("render job does not match the worker's committed snapshot plan");
        }
        if (msg.projectYaml && msg.projectYaml !== projectYaml) {
          projectYaml = msg.projectYaml;
          engine.plan_project(projectYaml, msg.track ?? track);
        }
        const out = [];
        const jobId = msg.jobId || null;
        if (jobId) activeJobs.add(jobId);
        for (const index of msg.indices || []) {
          if (jobId && cancelledJobs.has(jobId)) break;
          let pcm;
          if (renderEngine === "r2-webgpu") {
            if (!r2Runtime) throw new Error("R2 WebGPU runtime is not initialized");
            const packet = engine.render_r2_features_chunk(index);
            pcm = (await r2Runtime.renderFeaturePacket(packet)).pcm;
          } else {
            // `slice()` copies out of wasm memory so the buffer is transferable.
            pcm = engine.render_chunk(index).slice();
          }
          const chunk = { index, pcm: pcm.buffer };
          out.push(chunk);
          // Stream each completed chunk so the host can report contract-level
          // progress and cancellation can be observed before the next chunk.
          self.postMessage({
            type: "chunk",
            id: msg.id,
            jobId,
            sessionId: msg.sessionId,
            revision: msg.revision,
            index,
            pcm: chunk.pcm,
          }, [chunk.pcm]);
          await yieldToMessages();
        }
        const cancelled = Boolean(jobId && cancelledJobs.has(jobId));
        if (jobId) {
          activeJobs.delete(jobId);
          cancelledJobs.delete(jobId);
        }
        self.postMessage({ type: "done", id: msg.id, jobId, cancelled, chunkCount: out.length });
        return;
      }

      case "cancel": {
        if (msg.jobId && activeJobs.has(msg.jobId)) cancelledJobs.add(msg.jobId);
        self.postMessage({ type: "cancelled", id: msg.id, jobId: msg.jobId });
        return;
      }

      default:
        throw new Error(`unknown message type: ${msg.type}`);
    }
  } catch (err) {
    if (msg?.jobId) {
      activeJobs.delete(msg.jobId);
      cancelledJobs.delete(msg.jobId);
    }
    self.postMessage({ type: "error", id: msg?.id, message: String(err) });
  }
};
