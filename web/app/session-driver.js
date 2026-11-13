// Stable-ID browser Session adapter. Rust owns project state, revisions,
// validation, history and serialization; this class only sequences host I/O.
import init, { Engine } from "../pkg/engine_wasm.js";
import { ProjectStore } from "./project-store.js";

function revisionOf(value) {
  const revision = typeof value === "bigint" ? Number(value) : value;
  if (!Number.isSafeInteger(revision) || revision < 0) throw new Error("Session revision is outside the safe browser range");
  return revision;
}

/**
 * Rust exposes revisions as `u64`, which wasm-bindgen marshals as BigInt.
 * Passing a JS number throws "Cannot convert <n> to a BigInt", so every
 * revision argument must be widened here.
 */
function bigRevision(revision) {
  return BigInt(revisionOf(revision));
}

function resultOf(raw) {
  const value = JSON.parse(raw);
  if (value?.error) throw Object.assign(new Error(value.error.message || value.error.code), value.error);
  return value;
}

export class SessionDriver {
  constructor(engine, store = new ProjectStore()) {
    this.engine = engine;
    this.store = store;
    this._listeners = new Set();
    this._lifecycleWrite = false;
    this._lastWarnings = [];
  }

  static async boot({ store } = {}) {
    await init();
    return new SessionDriver(new Engine(), store);
  }

  get sessionId() { return this.engine.session_id() || null; }
  get revision() { return revisionOf(this.engine.session_revision()); }
  get isOpen() { return Boolean(this.sessionId); }
  get warnings() { return this._lastWarnings; }

  onChange(listener) { this._listeners.add(listener); return () => this._listeners.delete(listener); }
  _notify(reason, result = null) {
    const state = this.state();
    for (const listener of this._listeners) listener({ reason, result, state });
    return state;
  }

  state() {
    if (!this.isOpen) return Object.freeze({ sessionId: null, revision: 0, snapshot: null });
    return Object.freeze({
      sessionId: this.sessionId,
      revision: this.revision,
      snapshot: JSON.parse(this.engine.session_snapshot_json()),
    });
  }

  openStored(name) {
    if (this._lifecycleWrite) throw new Error("A project save or lifecycle change is in progress");
    this._lifecycleWrite = true;
    return this.store.loadBundle(name).then(({ ustx, sidecar, pronunciationSidecar }) => {
      if (!ustx) throw new Error(`Project “${name}” was not found or is empty`);
      const snapshot = resultOf(this.engine.session_open_yaml_with_sidecars(
        ustx, sidecar, pronunciationSidecar,
      ));
      this._lastWarnings = Object.freeze(JSON.parse(this.engine.session_warnings_json()));
      this._notify("open", snapshot);
      return snapshot;
    }).finally(() => { this._lifecycleWrite = false; });
  }

  /** Portable USTX import intentionally omits identity; Rust assigns fresh IDs. */
  importUstx(ustx) {
    if (this._lifecycleWrite) throw new Error("A project save or lifecycle change is in progress");
    const snapshot = resultOf(this.engine.session_open_yaml(String(ustx), ""));
    this._lastWarnings = Object.freeze(JSON.parse(this.engine.session_warnings_json()));
    this._notify("import", snapshot);
    return snapshot;
  }

  create({ name = null, bpm = null } = {}) {
    if (this._lifecycleWrite) throw new Error("A project save or lifecycle change is in progress");
    const snapshot = resultOf(this.engine.session_new_json(JSON.stringify({ name, bpm })));
    this._lastWarnings = Object.freeze(JSON.parse(this.engine.session_warnings_json()));
    this._notify("new", snapshot);
    return snapshot;
  }

  close() {
    if (this._lifecycleWrite) throw new Error("A project save or lifecycle change is in progress");
    this.engine.session_close();
    this._lastWarnings = [];
    this._notify("close");
  }

  apply(command) {
    if (this._lifecycleWrite) throw new Error("A project save or lifecycle change is in progress");
    const expected = this.revision;
    const request = command?.command
      ? { ...command, expected_revision: command.expected_revision ?? expected }
      : { expected_revision: expected, command };
    const result = resultOf(this.engine.session_apply_v2(JSON.stringify(request)));
    this._notify("edit", result);
    return result;
  }

  /** Revision-tagged Rust candidate read for one stable note identity. */
  composePhoneme({ projectId, partId, noteId, expectedRevision = this.revision, ...request }) {
    if (!this.isOpen) throw new Error("No project session is open");
    const raw = this.engine.session_phoneme_compose_v2(JSON.stringify({
      project_id: String(projectId),
      part_id: String(partId),
      note_id: String(noteId),
      expected_revision: revisionOf(expectedRevision),
      ...request,
    }));
    return resultOf(raw);
  }

  /** Shared Rust command semantics; caller may wrap lyric + choice in a group. */
  setPronunciation(noteId, pronunciation) {
    return this.apply({ type: "note.set_pronunciation", note_id: String(noteId), pronunciation });
  }

  setLyric(noteId, lyric) {
    return this.apply({ type: "note.set_lyric", note_id: String(noteId), lyric: String(lyric) });
  }

  undo() {
    if (this._lifecycleWrite) throw new Error("A project save or lifecycle change is in progress");
    const result = resultOf(this.engine.session_undo_v2(bigRevision(this.revision)));
    this._notify("undo", result);
    return result;
  }

  redo() {
    if (this._lifecycleWrite) throw new Error("A project save or lifecycle change is in progress");
    const result = resultOf(this.engine.session_redo_v2(bigRevision(this.revision)));
    this._notify("redo", result);
    return result;
  }

  beginGroup(name) {
    if (this._lifecycleWrite) throw new Error("A project save or lifecycle change is in progress");
    return resultOf(this.engine.session_group_begin(bigRevision(this.revision), String(name)));
  }

  commitGroup(groupId) {
    if (this._lifecycleWrite) throw new Error("A project save or lifecycle change is in progress");
    const result = resultOf(this.engine.session_group_commit(String(groupId), bigRevision(this.revision)));
    this._notify("group-commit", result);
    return result;
  }

  cancelGroup(groupId) {
    if (this._lifecycleWrite) throw new Error("A project save or lifecycle change is in progress");
    const result = resultOf(this.engine.session_group_cancel(String(groupId)));
    this._notify("group-cancel", result);
    return result;
  }

  /** Save the current identity and stable-ID table to OPFS. */
  async save(name) {
    if (this._lifecycleWrite) throw new Error("A project save or lifecycle change is in progress");
    this._lifecycleWrite = true;
    try {
    const expected = this.revision;
    const sessionId = this.sessionId;
    const ustx = this.engine.session_export_ustx();
    const sidecar = this.engine.session_save_sidecar();
    // The WASM binding returns the sidecar *bytes* (`Result<String, String>`:
    // wasm-bindgen already throws the `Err`), so it is handed to the store as it
    // is. `resultOf` would JSON-parse it into an object, and `saveBundle` writes
    // the three artifacts verbatim — an object here failed every plain `Save`
    // with "A Session save requires canonical USTX and both Session sidecars".
    const pronunciationSidecar = this.engine.session_save_pronunciation_sidecar();
    if (this.revision !== expected || this.sessionId !== sessionId) throw new Error("Session changed while preparing save");
    const savedName = await this.store.saveBundle(name, { ustx, sidecar, pronunciationSidecar });
    await this.store.remember(savedName);
    return { name: savedName, sessionId, revision: expected };
    } finally { this._lifecycleWrite = false; }
  }

  /** Save a portable USTX without the private identity sidecar. */
  exportUstx() {
    if (!this.isOpen) throw new Error("No project session is open");
    return this.engine.session_export_ustx();
  }

  /** Render a track from the authoritative Rust session snapshot. */
  renderTrack(track) {
    if (!this.isOpen) throw new Error("No project session is open");
    return this.engine.session_render_project(Number(track));
  }

  /** Render the whole authoritative session through the Rust stereo mixer. */
  renderMixdown() {
    if (!this.isOpen) throw new Error("No project session is open");
    return this.engine.session_render_mixdown();
  }

  registerMedia(mediaRef, bytes) {
    if (!this.isOpen) throw new Error("No project session is open");
    const key = mediaRef.startsWith('asset:') ? mediaRef : `asset:${mediaRef}`;
    const duration_ms = this.engine.session_register_media(key, bytes);
    return { media_ref: key, duration_ms };
  }

  /** Write the Save As bundle first, then adopt its forked project identity. */
  async saveAs(name) {
    if (this._lifecycleWrite) throw new Error("A project save or lifecycle change is in progress");
    this._lifecycleWrite = true;
    try {
    const expected = this.revision;
    const sessionId = this.sessionId;
    const prepared = resultOf(this.engine.session_prepare_save_as_json());
    if (this.revision !== expected || this.sessionId !== sessionId) throw new Error("Session changed while preparing Save As");
    const savedName = await this.store.saveBundle(name, {
      ustx: prepared.yaml,
      sidecar: prepared.sidecar,
      pronunciationSidecar: prepared.pronunciation_sidecar,
    });
    if (this.revision !== expected || this.sessionId !== sessionId) {
      throw new Error("Session changed before Save As could adopt its new identity");
    }
    const committed = resultOf(this.engine.session_commit_save_as());
    if (committed.project_id !== prepared.project_id) {
      throw new Error("Save As identity did not match the persisted sidecar");
    }
    await this.store.remember(savedName);
    this._notify("save-as", committed);
    return { name: savedName, ...committed };
    } finally { this._lifecycleWrite = false; }
  }
}
