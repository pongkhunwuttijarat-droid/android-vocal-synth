// Engine driver: one object the UI talks to.
//
// Wraps the wasm `Engine` (Rust owns the project document) and turns UI
// intents into JSON commands. Nothing here reimplements musical rules — tick
// arithmetic, bounds checks and curve semantics stay in Rust.

import init, { Engine } from "../pkg/engine_wasm.js";

export class Driver {
  constructor(engine) {
    this.engine = engine;
    /** Bumped on every document change, so views know to re-read. */
    this.revision = 0;
    this._listeners = new Set();
  }

  /** Load the module and create an engine. */
  static async boot() {
    await init();
    return new Driver(new Engine());
  }

  on(fn) {
    this._listeners.add(fn);
    return () => this._listeners.delete(fn);
  }

  _changed(what) {
    this.revision += 1;
    for (const fn of this._listeners) fn(what, this.revision);
  }

  // ---- voicebank --------------------------------------------------------

  /**
   * Install a voicebank from `oto.ini` bytes.
   *
   * `otoBytes` is an ArrayBuffer; wasm-bindgen's `&[u8]` needs a *view*, so it
   * MUST be wrapped — passing a raw ArrayBuffer silently yields an empty
   * slice and the bank ends up with zero aliases.
   */
  setVoicebank(id, name, otoDir, otoBytes) {
    const view = otoBytes instanceof Uint8Array ? otoBytes : new Uint8Array(otoBytes);
    this.engine.set_voicebank(id, name, otoDir, view);
    return this.engine.alias_count();
  }

  wavKeys() {
    return this.engine.wav_keys().split("\n").filter(Boolean);
  }

  /** Add one sample (ArrayBuffer or Uint8Array; both are wrapped). */
  addSample(key, wav, frq) {
    const w = wav instanceof Uint8Array ? wav : new Uint8Array(wav);
    if (frq) {
      const f = frq instanceof Uint8Array ? frq : new Uint8Array(frq);
      return this.engine.add_sample_with_frq(key, w, f);
    }
    return this.engine.add_sample(key, w);
  }

  sampleCount() {
    return this.engine.sample_count();
  }

  aliasCount() {
    return this.engine.alias_count();
  }

  fftBackend() {
    return this.engine.fft_backend();
  }

  // ---- project ----------------------------------------------------------

  loadYaml(yaml) {
    const normalized = this.engine.project_load_yaml(yaml);
    this._changed("load");
    return normalized;
  }

  newProject() {
    const yaml = this.engine.project_new();
    this._changed("new");
    return yaml;
  }

  yaml() {
    return this.engine.project_yaml();
  }

  snapshot() {
    return this.engine.project_snapshot();
  }

  meta() {
    return JSON.parse(this.engine.meta_json());
  }

  tracks() {
    return JSON.parse(this.engine.tracks_json());
  }

  /** Notes of a track, flattened with their part index attached. */
  notes(track = 0) {
    const model = JSON.parse(this.engine.notes_json(track));
    const out = [];
    for (const part of model.parts) {
      for (const note of part.notes) {
        out.push({ ...note, part: part.part, partName: part.name });
      }
    }
    out.sort((a, b) => a.position - b.position || a.tone - b.tone);
    return out;
  }

  curves(track = 0) {
    return JSON.parse(this.engine.curves_json(track));
  }

  // ---- editing (all through reversible commands) ------------------------

  /** Apply one command; throws on rejection with the Rust error message. */
  apply(cmd) {
    const name = this.engine.apply_command(JSON.stringify(cmd));
    this._changed("edit");
    return name;
  }

  /** Apply several commands as ONE undo step. */
  applyGroup(name, cmds) {
    this.engine.apply_command(JSON.stringify({ cmd: "group_begin", name }));
    try {
      for (const c of cmds) this.engine.apply_command(JSON.stringify(c));
    } finally {
      this.engine.apply_command(JSON.stringify({ cmd: "group_end" }));
    }
    this._changed("edit");
  }

  undo() {
    const name = this.engine.undo();
    this._changed("undo");
    return name;
  }

  redo() {
    const name = this.engine.redo();
    this._changed("redo");
    return name;
  }

  canUndo() {
    return this.engine.can_undo();
  }
  canRedo() {
    return this.engine.can_redo();
  }

  // ---- rendering --------------------------------------------------------

  /** Plan chunks from the current document. Returns the chunk metadata. */
  plan(track = 0) {
    this.engine.plan_document(track);
    return this.chunkMeta();
  }

  chunkMeta() {
    return JSON.parse(this.engine.chunk_meta_json());
  }

  chunkWavKeys(index) {
    return this.engine.chunk_wav_keys(index).split("\n").filter(Boolean);
  }

  planSkipped() {
    return this.engine.plan_skipped().split("\n").filter(Boolean);
  }

  /** Render one chunk (worker-side call). */
  renderChunk(index) {
    return this.engine.render_chunk(index);
  }

  /** Render every planned chunk on this thread. */
  renderAll() {
    return this.engine.render_planned_sequential();
  }
}
