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
  constructor(samples, projectYaml, track) {
    this.worker = new Worker(new URL("./worker.js", import.meta.url), { type: "module" });
    this.seq = 1;
    this.pending = new Map();
    this.keys = new Set();
    this.timings = { wasmInitMs: 0, voicebankLoadMs: 0 };
    this.ready = new Promise((resolve, reject) => {
      this._resolveReady = resolve;
      this._rejectReady = reject;
    });
    this.worker.onmessage = (ev) => this._onMessage(ev.data);
    this.worker.onerror = (e) => this._rejectReady?.(new Error("worker error: " + (e.message || e)));
    this.worker.postMessage({ type: "init", id: 0, samples, projectYaml, track });
  }

  _onMessage(m) {
    if (m.type === "ready") {
      this.timings.wasmInitMs = m.wasmInitMs;
      this.timings.voicebankLoadMs = m.voicebankLoadMs;
      this._resolveReady?.(m);
      this._resolveReady = null;
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
    this.pending.delete(m.id);
    p.resolve(m);
  }

  _send(msg, transfer) {
    const id = this.seq++;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
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

  async replan(projectYaml, track) {
    return this._send({ type: "replan", projectYaml, track });
  }

  async render(indices) {
    if (!indices.length) return [];
    const res = await this._send({ type: "render", indices });
    return res.chunks.map((c) => ({ index: c.index, pcm: new Float32Array(c.pcm) }));
  }

  terminate() {
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
   */
  static async create({
    workerCount,
    bank,
    otoPath,
    otoDir = "",
    projectYaml,
    track = 0,
    onProgress,
  }) {
    await init();

    const pool = new RenderPool({ workerCount, bank, otoDir, track, onProgress });

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
    pool.links = Array.from(
      { length: workerCount },
      () => new WorkerLink(emptySamples, projectYaml, track),
    );
    await Promise.all(pool.links.map((l) => l.ready));

    await pool.setProject(projectYaml, { force: true });
    return pool;
  }

  constructor({ workerCount, bank, otoDir, track, onProgress }) {
    this.workerCount = workerCount;
    this.bank = bank;
    this.otoDir = otoDir;
    this.track = track;
    this.onProgress = onProgress;
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

  /** Re-plan after a project edit. Returns `{chunks, dirty}`. */
  async setProject(projectYaml, { force = false } = {}) {
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
    await Promise.all(this.links.map((l) => l.replan(projectYaml, this.track)));
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
   * @returns {Promise<{samples: Float32Array, stats: object}>}
   */
  async render({ only } = {}) {
    const t0 = performance.now();
    // Default to the chunks whose CURRENT hash is not cached — not to a stale
    // dirty list from the last `setProject`. This is what makes "Play" after
    // "Edit" cheap: an unchanged project renders nothing and only re-mixes.
    const indices = only ?? this.plan.filter((m) => !this.cache.has(m.hash)).map((m) => m.index);

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

    // Render, then mix in plan order (the mixer's crossfade depends on order).
    let done = 0;
    await Promise.all(
      [...byWorker.entries()].map(async ([w, list]) => {
        const out = await this.links[w].render(list);
        for (const c of out) {
          const hash = this.plan[c.index].hash;
          this.cache.set(hash, c.pcm);
          this.cacheMeta.set(hash, this.plan[c.index]);
        }
        done += out.length;
        this.onProgress?.({ rendered: done, total: indices.length });
      }),
    );

    this.stats.chunksRendered += done;

    const tMix = performance.now();
    const mixed = this.mix();
    this.stats.lastMixMs = performance.now() - tMix;
    this.stats.lastRendered = done;
    this.stats.lastReused = this.plan.length - done;
    this.stats.chunksReused += this.stats.lastReused;
    this.stats.renders += 1;
    this.stats.lastRenderMs = performance.now() - t0;
    return { samples: mixed, stats: { ...this.stats } };
  }

  /** Mix every planned chunk from the cache, in plan order. */
  mix() {
    const mixer = new Mixer();
    for (const meta of this.plan) {
      const pcm = this.cache.get(meta.hash);
      if (!pcm) throw new Error(`chunk ${meta.index} not rendered`);
      mixer.add_chunk(pcm, meta.position_ms, meta.leading_ms);
    }
    return mixer.finish();
  }

  /** The current mix as a 16-bit PCM WAV, from the cache (no re-render). */
  toWav() {
    const mixer = new Mixer();
    for (const meta of this.plan) {
      const pcm = this.cache.get(meta.hash);
      if (!pcm) throw new Error(`chunk ${meta.index} not rendered`);
      mixer.add_chunk(pcm, meta.position_ms, meta.leading_ms);
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
    for (const l of this.links) l.terminate();
    this.links = [];
  }
}
