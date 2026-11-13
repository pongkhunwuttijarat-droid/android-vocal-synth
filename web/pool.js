// RenderPool — the production renderer: N Web Workers, per-chunk sample
// loading, and a chunk cache keyed by the engine's own chunk hash.
//
// Why it is built this way (measured in docs/architecture/wasm-parallel.md):
//
//   * **Per-chunk sample loading.** A worker only receives the wav/frq files
//     its chunks actually reference. With the full Teto bank that is ~1-3
//     samples per chunk out of 558, and worker readiness drops from ~625 ms to
//     ~27 ms while memory per worker drops from ~198 MB to a few MB. On a
//     tablet the full-bank-per-worker variant (~790 MB for 4 workers) is not
//     viable at all.
//   * **Persistent workers.** The pool is created once and reused across
//     renders, so the wasm instantiation and JIT warm-up are paid once.
//   * **Chunk cache by engine hash.** `chunk_meta_json()` reports the Rust
//     chunk hash (phonemes including context + engine salt). After an edit,
//     only the chunks whose hash changed are re-rendered; the rest are reused
//     from the cache, so the mixed output is identical to a full render.
//
// The main thread never loads audio: planning only needs the alias table from
// `oto.ini`, so `chunk_wav_keys` works with no samples installed.

import init, { Engine, Mixer } from "./pkg/engine_wasm.js";

/** One worker connection with promise-based request/response. */
class WorkerLink {
  constructor(samples, projectYaml, track, renderEngine) {
    this.worker = new Worker(new URL("./worker.js", import.meta.url), { type: "module" });
    this.seq = 1;
    this.pending = new Map();
    this.keys = new Set();
    this.failed = false;
    this.timings = { wasmInitMs: 0, voicebankLoadMs: 0 };
    this.ready = new Promise((resolve, reject) => {
      this._resolveReady = resolve;
      this._rejectReady = reject;
    });
    this.worker.onmessage = (ev) => this._onMessage(ev.data);
    this.worker.onerror = (e) => {
      this.failed = true;
      const error = new Error("worker error: " + (e.message || e));
      this.worker.terminate();
      this._rejectReady?.(error);
      for (const pending of this.pending.values()) pending.reject(error);
      this.pending.clear();
    };
    this.worker.postMessage({ type: "init", id: 0, samples, projectYaml, track, renderEngine });
  }

  _onMessage(m) {
    if (m.type === "ready") {
      this.timings.wasmInitMs = m.wasmInitMs;
      this.timings.voicebankLoadMs = m.voicebankLoadMs;
      this.timings.renderEngine = m.renderEngine;
      this.timings.r2LoadMs = m.r2LoadMs || 0;
      this.timings.r2Device = m.r2Device || null;
      this._resolveReady?.(m);
      this._resolveReady = null;
      return;
    }
    if (m.type === "chunk") {
      const p = this.pending.get(m.id);
      if (!p) return;
      if ((m.jobId || undefined) !== p.jobId || m.sessionId !== p.sessionId || m.revision !== p.revision) {
        this.pending.delete(m.id);
        p.reject(new Error("worker chunk identity does not match its render job"));
        return;
      }
      const chunk = { index: m.index, pcm: new Float32Array(m.pcm) };
      p.chunks.push(chunk);
      p.onChunk?.(chunk);
      return;
    }
    if (m.type === "error") {
      const p = this.pending.get(m.id);
      if (p) {
        this.pending.delete(m.id);
        p.reject(new Error(m.message));
      } else {
        this._rejectReady?.(new Error(m.message));
      }
      return;
    }
    const p = this.pending.get(m.id);
    if (!p) return;
    if (p.jobId && (m.jobId !== p.jobId)) {
      this.pending.delete(m.id);
      p.reject(new Error("worker result identity does not match its render job"));
      return;
    }
    this.pending.delete(m.id);
    p.resolve({ ...m, chunks: p.chunks });
  }

  _send(msg, transfer, options = {}) {
    if (this.failed) return Promise.reject(new Error("worker is unavailable"));
    const id = this.seq++;
    return new Promise((resolve, reject) => {
      this.pending.set(id, {
        resolve,
        reject,
        chunks: [],
        onChunk: options.onChunk,
        jobId: options.jobId,
        sessionId: options.sessionId,
        revision: options.revision,
      });
      this.worker.postMessage({ ...msg, id }, transfer || []);
    });
  }

  /** Push samples the worker does not have yet. */
  async addSamples(files) {
    const missing = files.filter((f) => f && !this.keys.has(f.key));
    if (!missing.length) return 0;
    const res = await this._send({ type: "addSamples", files: missing });
    for (const f of missing) this.keys.add(f.key);
    return res.added;
  }

  async replan(projectYaml, track, renderJob = null) {
    return this._send({ type: "replan", projectYaml, track, renderJob });
  }

  async render(indices, { jobId, sessionId, revision, engine, onChunk } = {}) {
    if (!indices.length) return { chunks: [], cancelled: false };
    const res = await this._send(
      { type: "render", indices, jobId, sessionId, revision, engine },
      [],
      { onChunk, jobId, sessionId, revision },
    );
    return { chunks: res.cancelled ? [] : res.chunks, cancelled: Boolean(res.cancelled) };
  }

  async cancelRender(jobId) {
    return this._send({ type: "cancel", jobId });
  }

  terminate() {
    this.failed = true;
    const error = new Error("worker terminated");
    for (const pending of this.pending.values()) pending.reject(error);
    this.pending.clear();
    this.worker.terminate();
  }
}

export class RenderPool {
  /**
   * @param {object} opts
   * @param {number} opts.workerCount   how many workers to keep alive
   * @param {import("./samples.js").BaseBank} opts.bank  sample source
   * @param {string} opts.otoPath       path of `oto.ini` within the bank
   * @param {string} opts.otoDir        directory holding oto.ini + wavs (e.g. "voice")
   * @param {string} opts.projectYaml   the .ustx text
   * @param {number} [opts.track]
   * @param {{volumeDb?: number, pan?: number}} [opts.trackMix]
   *   the track's fader, applied in the MIX stage only. Volume/pan are absent
   *   from the chunk hash, so changing them re-mixes cached PCM instead of
   *   re-rendering — see `Mixer.add_chunk_track`.
   */
  static async create({
    workerCount,
    bank,
    otoPath,
    otoDir = "",
    projectYaml,
    track = 0,
    trackMix = null,
    onProgress,
    renderEngine = "r1-rs",
  }) {
    await init();

    const pool = new RenderPool({ workerCount, bank, otoDir, track, trackMix, onProgress, renderEngine });

    // The planner engine holds the alias table only — no audio.
    await bank.openOto({ otoPath, otoDir });
    pool.otoBytes = bank.otoBytes;
    pool.otoKeys = bank.keys;
    pool.planner = new Engine();
    pool.planner.set_voicebank("pool", pool.id ?? "pool", otoDir, new Uint8Array(bank.otoBytes));

    const emptySamples = {
      id: "pool",
      name: "pool",
      otoDir,
      oto: bank.otoBytes,
      files: [],
    };
    pool.workerSamples = emptySamples;
    pool.links = Array.from(
      { length: workerCount },
      () => new WorkerLink(emptySamples, projectYaml, track, renderEngine),
    );
    try {
      await Promise.all(pool.links.map((l) => l.ready));
    } catch (error) {
      pool.terminate();
      throw error;
    }

    await pool.setProject(projectYaml, { force: true });
    return pool;
  }

  constructor({ workerCount, bank, otoDir, track, trackMix = null, onProgress, renderEngine }) {
    this.workerCount = workerCount;
    this.bank = bank;
    this.otoDir = otoDir;
    this.track = track;
    // Track volume/pan for the mix stage. Not part of the chunk plan or the
    // chunk hash: `mix()` applies it to cached PCM, `setTrackMix()` re-mixes
    // and never re-renders.
    this.trackMix = { volumeDb: trackMix?.volumeDb ?? 0, pan: trackMix?.pan ?? 0 };
    this.onProgress = onProgress;
    this.renderEngine = renderEngine;
    this.committedSnapshot = null;
    this.renderJobs = new Map();
    this.renderListeners = new Set();
    this._renderQueue = Promise.resolve();
    /** hash -> Float32Array (rendered chunk audio). */
    this.cache = new Map();
    /** hash -> {index, position_ms, leading_ms} */
    this.cacheMeta = new Map();
    this.plan = [];
    this.skipped = [];
    this.links = [];
    this.stats = {
      renders: 0,
      chunksRendered: 0,
      chunksReused: 0,
      samplesLoaded: 0,
      lastRenderMs: 0,
      lastLoadMs: 0,
      lastMixMs: 0,
      lastRendered: 0,
      lastReused: 0,
    };
  }

  /** Owning worker for chunk `index` (round-robin, stable across re-plans). */
  owner(index) {
    return index % this.workerCount;
  }

  /**
   * Replace the track fader. The chunk plan and the cache are untouched — the
   * next `mix()`, `mixFrom()` or `toWav()` picks the new values up, which is
   * exactly the cache/effect split: volume and pan belong to the MIX stage.
   */
  setTrackMix({ volumeDb = this.trackMix.volumeDb, pan = this.trackMix.pan } = {}) {
    this.trackMix = { volumeDb, pan };
    return this.trackMix;
  }

  /** Add a cached chunk to `mixer` with the track's fader applied. */
  _addChunk(mixer, pcm, meta) {
    const { volumeDb, pan } = this.trackMix;
    mixer.add_chunk_track(pcm, meta.position_ms, meta.leading_ms, volumeDb, pan);
  }

  /** Re-plan after a project edit. Returns `{chunks, dirty}`. */
  async setProject(projectYaml, { force = false, renderJob = null } = {}) {
    this.projectYaml = projectYaml;
    const n = this.planner.plan_project(projectYaml, this.track);
    this.plan = JSON.parse(this.planner.chunk_meta_json());
    this.skipped = this.planner.plan_skipped().split("\n").filter(Boolean);

    // A chunk is dirty when its hash is not cached (or on an explicit force).
    const dirty = [];
    for (let i = 0; i < n; i++) {
      const hash = this.plan[i].hash;
      if (force || !this.cache.has(hash)) dirty.push(i);
    }
    this.dirty = dirty;

    // Every worker must know the current plan (it plans the same text).
    await Promise.all(this.links.map((l) => l.replan(projectYaml, this.track, renderJob)));
    return { chunks: n, dirty: dirty.length, skipped: this.skipped };
  }

  /** Which bank keys a worker needs to render `indices`. */
  keysFor(indices) {
    const keys = new Set();
    for (const i of indices) {
      const text = this.planner.chunk_wav_keys(i);
      for (const k of text.split("\n")) if (k) keys.add(k);
    }
    return [...keys];
  }

  /**
   * Render the dirty chunks (or `only`, if given) and return the mixed PCM.
   * Cached chunks are reused, so an edit that touches one chunk renders one
   * chunk.
   *
   * @param {object} [opts]
   * @param {number[]} [opts.only] render exactly these chunk indices
   * @param {{cancelled:boolean}} [opts.signal] set `cancelled` to drop the
   *   render: worker replies that arrive afterwards are NOT written to the
   *   cache and no PCM is returned, so the caller keeps its previous audio
   *   instead of publishing a half render.
   * @param {(p:{rendered:number,total:number}) => void} [opts.onProgress]
   * @returns {Promise<{samples: Float32Array|null, cancelled?: boolean, stats: object}>}
   */
  async render({ only, signal, onProgress } = {}) {
    const t0 = performance.now();
    // Default to the chunks whose CURRENT hash is not cached — not to a stale
    // dirty list from the last `setProject`. This is what makes "Play" after
    // "Edit" cheap: an unchanged project renders nothing and only re-mixes.
    const renderPlan = this.plan.slice();
    const indices = only ?? renderPlan.filter((m) => !this.cache.has(m.hash)).map((m) => m.index);

    // Group the work by owning worker, and load each worker's missing samples.
    const byWorker = new Map();
    for (const i of indices) {
      const w = this.owner(i);
      if (!byWorker.has(w)) byWorker.set(w, []);
      byWorker.get(w).push(i);
    }

    const tLoad = performance.now();
    let loaded = 0;
    await Promise.all(
      [...byWorker.entries()].map(async ([w, list]) => {
        const keys = this.keysFor(list);
        if (!keys.length) return;
        const files = await this.bank.load(keys);
        loaded += await this.links[w].addSamples(files);
      }),
    );
    this.stats.lastLoadMs = performance.now() - tLoad;
    this.stats.samplesLoaded += loaded;

    if (signal?.cancelled) return this._cancelled(t0);

    // Render, then mix in plan order (the mixer's crossfade depends on order).
    let done = 0;
    const progress = onProgress || this.onProgress;
    // Chunks are written to the cache the moment a worker delivers one, not
    // after the whole worker batch resolves. A worker streams `type:"chunk"`
    // per chunk, so by the time the render promise settles everything is
    // already cached — but caching on delivery is what makes a CANCELLED render
    // keep the chunks it already finished instead of discarding them, and it is
    // what lets a caller start mixing from wherever the cache happens to reach.
    // The cache is keyed by the engine hash, so a cancelled job's completed
    // chunks are exactly the audio a later job would render anyway.
    await Promise.all(
      [...byWorker.entries()].map(async ([w, list]) => {
        await this.links[w].render(list, {
          jobId: signal?.jobId,
          sessionId: signal?.sessionId,
          revision: signal?.revision,
          engine: signal?.engine,
          onChunk: chunk => {
            const meta = renderPlan[chunk.index];
            if (meta) {
              this.cache.set(meta.hash, chunk.pcm);
              this.cacheMeta.set(meta.hash, meta);
            }
            done += 1;
            progress?.({ rendered: done, total: indices.length });
          },
        });
      }),
    );

    if (signal?.cancelled) return this._cancelled(t0, done);

    this.stats.chunksRendered += done;

    const tMix = performance.now();
    const mixed = this.mix();
    this.stats.lastMixMs = performance.now() - tMix;
    this.stats.lastRendered = done;
    this.stats.lastReused = renderPlan.length - done;
    this.stats.chunksReused += this.stats.lastReused;
    this.stats.renders += 1;
    this.stats.lastRenderMs = performance.now() - t0;
    return { samples: mixed, stats: { ...this.stats } };
  }

  /** Install an immutable Rust-owned snapshot as the source for future jobs. */
  setCommittedSnapshot(snapshot) {
    if (!snapshot || typeof snapshot.sessionId !== "string" || !snapshot.sessionId ||
        !Number.isSafeInteger(snapshot.revision) || snapshot.revision < 0 ||
        typeof snapshot.ustx !== "string" || !snapshot.ustx) {
      throw new TypeError("expected committed snapshot {sessionId, revision, ustx}");
    }
    if (this.committedSnapshot?.sessionId === snapshot.sessionId &&
        snapshot.revision < this.committedSnapshot.revision) {
      const error = new Error("cannot replace a committed render snapshot with an older revision");
      error.code = "revision_conflict";
      error.details = {
        current_revision: this.committedSnapshot.revision,
        received_revision: snapshot.revision,
      };
      throw error;
    }
    this.committedSnapshot = Object.freeze({
      sessionId: snapshot.sessionId,
      revision: snapshot.revision,
      ustx: snapshot.ustx,
    });
  }

  onRenderEvent(listener) {
    this.renderListeners.add(listener);
    return () => this.renderListeners.delete(listener);
  }

  async _recoverFailedWorkers(projectYaml) {
    const failed = this.links.map((link, index) => link.failed ? index : -1).filter((index) => index >= 0);
    if (!failed.length) return;
    await Promise.all(failed.map(async (index) => {
      const replacement = new WorkerLink(this.workerSamples, projectYaml, this.track, this.renderEngine);
      await replacement.ready;
      this.links[index] = replacement;
    }));
  }

  _emitRenderEvent(type, job, fields = {}) {
    const event = {
      type,
      sessionId: job.sessionId,
      renderId: job.renderId,
      revision: job.revision,
      ...fields,
    };
    for (const listener of this.renderListeners) {
      try {
        listener(event);
      } catch (error) {
        console.error("RenderPool listener failed", error);
      }
    }
  }

  _jobDescriptor(job) {
    return {
      sessionId: job.sessionId,
      renderId: job.renderId,
      revision: job.revision,
      status: job.status,
      engine: job.engine,
    };
  }

  /** Start a §6.13 render job from the currently installed committed snapshot. */
  startRender({ expectedRevision, engine = this.renderEngine } = {}) {
    const snapshot = this.committedSnapshot;
    if (!snapshot) throw new Error("no committed session snapshot has been installed");
    if (expectedRevision !== snapshot.revision) {
      const error = new Error(`expected revision ${expectedRevision}, current revision is ${snapshot.revision}`);
      error.code = "revision_conflict";
      error.details = { expected_revision: expectedRevision, current_revision: snapshot.revision };
      throw error;
    }
    if (engine !== this.renderEngine) {
      const error = new Error(`render engine ${engine} is not available in this pool`);
      error.code = "unsupported_operation";
      throw error;
    }
    const bytes = new Uint8Array(16);
    if (!globalThis.crypto?.getRandomValues) throw new Error("secure random source is unavailable");
    globalThis.crypto.getRandomValues(bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    const hex = [...bytes].map((v) => v.toString(16).padStart(2, "0")).join("");
    const renderId = `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
    const job = {
      sessionId: snapshot.sessionId,
      revision: snapshot.revision,
      snapshot,
      renderId,
      engine,
      status: "queued",
      signal: {
        cancelled: false,
        jobId: renderId,
        sessionId: snapshot.sessionId,
        revision: snapshot.revision,
        engine,
      },
      startedAt: performance.now(),
      result: null,
      error: null,
    };
    this.renderJobs.set(renderId, job);
    this._emitRenderEvent("queued", job);

    const run = async () => {
      if (job.status === "cancelled") return;
      if (job.signal.cancelled) {
        job.status = "cancelled";
        job.error = { code: "render_cancelled", message: "render cancelled before start" };
        this._emitRenderEvent("cancelled", job, { error: job.error });
        return;
      }
      try {
        // Re-plan workers from this job's captured snapshot. Jobs share worker
        // engines, so the FIFO queue prevents one job changing another's plan.
        await this._recoverFailedWorkers(job.snapshot.ustx);
        await this.setProject(job.snapshot.ustx, {
          renderJob: {
            sessionId: job.sessionId,
            renderId: job.renderId,
            revision: job.revision,
            engine: job.engine,
          },
        });
        if (job.signal.cancelled) throw Object.assign(new Error("render cancelled"), { code: "render_cancelled" });
        job.status = "running";
        job.startedAt = performance.now();
        this._emitRenderEvent("running", job);
        const rendered = await this.render({
          signal: job.signal,
          onProgress: ({ rendered: doneChunks, total: totalChunks }) => {
            this._emitRenderEvent("progress", job, {
              doneChunks,
              totalChunks,
              elapsedMs: performance.now() - job.startedAt,
            });
          },
        });
        if (job.signal.cancelled || rendered.cancelled) throw Object.assign(new Error("render cancelled"), { code: "render_cancelled" });
        job.status = "done";
        job.result = {
          sessionId: job.sessionId,
          renderId: job.renderId,
          revision: job.revision,
          sampleRate: 44100,
          frames: rendered.samples.length,
          engine: job.engine,
          samples: rendered.samples,
          stats: rendered.stats,
        };
        this._emitRenderEvent("complete", job, { result: job.result });
      } catch (error) {
        if (job.status === "cancelled") return;
        const cancelled = job.signal.cancelled || error?.code === "render_cancelled";
        job.status = cancelled ? "cancelled" : "failed";
        job.error = {
          code: cancelled ? "render_cancelled" : "render_error",
          message: String(error?.message || error),
        };
        this._emitRenderEvent(cancelled ? "cancelled" : "failed", job, { error: job.error });
      }
    };
    const queued = this._renderQueue.then(run, run);
    this._renderQueue = queued.catch(() => {});
    return this._jobDescriptor(job);
  }

  getRender(renderId) {
    const job = this.renderJobs.get(renderId);
    if (!job) return null;
    return { ...this._jobDescriptor(job), result: job.result, error: job.error };
  }

  async cancelRender(renderId) {
    const job = this.renderJobs.get(renderId);
    if (!job) {
      const error = new Error(`render ${renderId} was not found`);
      error.code = "not_found";
      throw error;
    }
    if (job.status === "done" || job.status === "failed" || job.status === "cancelled") return this.getRender(renderId);
    job.signal.cancelled = true;
    if (job.status === "queued") {
      job.status = "cancelled";
      job.error = { code: "render_cancelled", message: "render cancelled before start" };
      this._emitRenderEvent("cancelled", job, { error: job.error });
      return this.getRender(renderId);
    }
    if (job.status === "running") {
      await Promise.all(this.links.map((link) => link.cancelRender(job.renderId).catch(() => {})));
    }
    return this.getRender(renderId);
  }

  /** Outcome of a render that was cancelled before it could be mixed. */
  _cancelled(t0, done = 0) {
    this.stats.renders += 1;
    this.stats.chunksRendered += done;
    this.stats.lastRendered = 0;
    this.stats.lastReused = this.plan.length;
    this.stats.lastRenderMs = performance.now() - t0;
    return { samples: null, cancelled: true, stats: { ...this.stats } };
  }

  /** True when the chunk carrying this hash is already rendered. */
  has(hash) {
    return this.cache.has(hash);
  }

  /**
   * Overlay state per planned chunk, straight from the cache:
   * `done` = rendered, `stale` = its hash is not cached yet.
   */
  states() {
    return this.plan.map((m) => (this.cache.has(m.hash) ? "done" : "stale"));
  }

  /** Mix every planned chunk from the cache, in plan order. */
  mix() {
    const mixer = new Mixer();
    for (const meta of this.plan) {
      const pcm = this.cache.get(meta.hash);
      if (!pcm) throw new Error(`chunk ${meta.index} not rendered`);
      this._addChunk(mixer, pcm, meta);
    }
    return mixer.finish();
  }

  /**
   * How far the cache reaches, as a chunk index — for streaming playback.
   *
   * Chunks render in parallel across workers, so the cached set is rarely a
   * prefix: a player that waits for `index === 0` waits for the whole song.
   * What it must respect is ORDER, because `mix_from` is a sequential
   * crossfade-overwrite: the first missing index is where playback has to stop
   * and wait, and everything before it can already be heard.
   */
  readyThrough() {
    for (let i = 0; i < this.plan.length; i++) {
      // `-1` means nothing is playable yet: chunk 0 missing is NOT the same as
      // "chunk 0 ready", and reporting 0 there would let a caller believe it can
      // start playing silence from the top.
      if (!this.cache.has(this.plan[i].hash)) return i - 1;
    }
    return this.plan.length - 1;
  }

  /**
   * The mixable audio from chunk `startIndex` onward, given that everything up
   * to `startIndex` is already rendered.
   *
   * Returns `null` when the cache does not reach `startIndex`, so a caller can
   * tell "not ready yet" from "ready and empty" — the difference between
   * waiting and having reached the end of the plan. Uses `Mixer.finish_from`
   * (Rust `mixer::mix_from`), which is bit-identical to the tail of a full
   * mix, so appending consecutive ranges reproduces exactly one full mix.
   *
   * Chunks before `startIndex` are added too: they still feed the crossfade at
   * its leading zone, and `mix_from` walks back over as many of them as
   * actually overlap.
   */
  mixFrom(startIndex) {
    if (startIndex < 0 || startIndex >= this.plan.length) return null;
    if (this.readyThrough() < startIndex) return null;
    const mixer = new Mixer();
    for (const meta of this.plan) {
      const pcm = this.cache.get(meta.hash);
      if (!pcm) break; // only reachable past startIndex; the tail is what we mix
      this._addChunk(mixer, pcm, meta);
    }
    return mixer.finish_from(startIndex);
  }

  /** The current mix as a 16-bit PCM WAV, from the cache (no re-render). */
  toWav() {
    const mixer = new Mixer();
    for (const meta of this.plan) {
      const pcm = this.cache.get(meta.hash);
      if (!pcm) throw new Error(`chunk ${meta.index} not rendered`);
      this._addChunk(mixer, pcm, meta);
    }
    return mixer.finish_wav();
  }

  /** Total bytes of rendered audio currently cached. */
  get cachedBytes() {
    let bytes = 0;
    for (const pcm of this.cache.values()) bytes += pcm.byteLength;
    return bytes;
  }

  /** Samples each worker currently holds (for diagnostics). */
  get perWorkerKeys() {
    return this.links.map((l) => l.keys.size);
  }

  terminate() {
    for (const job of this.renderJobs.values()) {
      if (job.status !== "queued" && job.status !== "running") continue;
      job.signal.cancelled = true;
      job.status = "cancelled";
      job.error = { code: "render_cancelled", message: "render pool closed" };
      this._emitRenderEvent("cancelled", job, { error: job.error });
    }
    for (const l of this.links) l.terminate();
    this.links = [];
  }
}
