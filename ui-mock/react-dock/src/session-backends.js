import { ProjectStore } from '../../../web/app/project-store.js';
import { estimateSessionPayloadBytes, makeSessionRequest, SESSION_RPC_MAX_PAYLOAD_BYTES, SESSION_RPC_MAX_PENDING, SESSION_RPC_MUTATIONS, SESSION_RPC_SESSION_REPLACEMENTS, isSessionRpcEnvelope } from './openutau/session-worker-rpc.js';
const WORKER_STARTUP_TIMEOUT_MS = 15_000;
const WORKER_REQUEST_TIMEOUT_MS = 30 * 60 * 1000;
const WORKER_CLOSE_TIMEOUT_MS = 2_000;
const HTTP_RECOVERY_STORAGE_KEY = 'rusted-utau.httpRecovery.v1';

function httpRecoveryId(baseUrl) {
  const storageKey = `${HTTP_RECOVERY_STORAGE_KEY}:${baseUrl}`;
  try {
    // sessionStorage isolates simultaneous editor tabs while surviving page
    // reloads in the tab that owns the server-side recovery checkpoint.
    const storage = globalThis.sessionStorage ?? globalThis.localStorage;
    let value = storage?.getItem(storageKey);
    if (!value) {
      value = globalThis.crypto?.randomUUID?.()
        || `recovery-${Date.now()}-${Math.random().toString(36).slice(2)}`;
      storage?.setItem(storageKey, value);
    }
    return value;
  } catch {
    return globalThis.crypto?.randomUUID?.()
      || `recovery-${Date.now()}-${Math.random().toString(36).slice(2)}`;
  }
}

export class SessionWorkerError extends Error {
  constructor(code, message, options = {}) {
    super(message, options);
    this.name = 'SessionWorkerError';
    this.code = code;
    this.reopenRequired = true;
    this.unsavedStateRestored = false;
  }
}

// Direct WASM session is isolated from React and Pixi on its own worker thread.
// The main-thread state is only the latest disposable Rust snapshot.
export class DirectWasmBackend {
  constructor(worker, { requestTimeoutMs = WORKER_REQUEST_TIMEOUT_MS, closeTimeoutMs = WORKER_CLOSE_TIMEOUT_MS, maxPendingRequests = SESSION_RPC_MAX_PENDING, maxPayloadBytes = SESSION_RPC_MAX_PAYLOAD_BYTES, initialStatus = 'ready', workerFactory = null, onLifecycle = null } = {}) {
    this.worker = null;
    this.kind = 'wasm';
    this.store = new ProjectStore();
    this._state = { sessionId: null, revision: 0, snapshot: null };
    this._listeners = new Set();
    this._lifecycleListeners = new Set(onLifecycle ? [onLifecycle] : []);
    this._pending = new Map();
    this._sequence = 0;
    this._mutationTail = Promise.resolve();
    this._queuedMutations = 0;
    this._closing = false;
    this._requestTimeoutMs = requestTimeoutMs;
    this._closeTimeoutMs = closeTimeoutMs;
    this._maxPendingRequests = Math.min(SESSION_RPC_MAX_PENDING, Math.max(1, Number(maxPendingRequests) || SESSION_RPC_MAX_PENDING));
    this._maxPayloadBytes = Math.min(SESSION_RPC_MAX_PAYLOAD_BYTES, Math.max(1, Number(maxPayloadBytes) || SESSION_RPC_MAX_PAYLOAD_BYTES));
    this._lifecycle = { status: initialStatus, error: null, recoverable: initialStatus === 'failed', unsavedStateRestored: false };
    this._workerFactory = workerFactory || (() => new Worker(new URL('./openutau/wasm-session.worker.js', import.meta.url), { type: 'module' }));
    this._workerHandlers = null;
    this._attachWorker(worker);
    for (const listener of this._lifecycleListeners) listener(this._lifecycle);
  }
  static async create({ startupTimeoutMs = WORKER_STARTUP_TIMEOUT_MS, requestTimeoutMs = WORKER_REQUEST_TIMEOUT_MS, maxPendingRequests = SESSION_RPC_MAX_PENDING, maxPayloadBytes = SESSION_RPC_MAX_PAYLOAD_BYTES, workerFactory = null, onLifecycle = null } = {}) {
    const createWorker = workerFactory || (() => new Worker(new URL('./openutau/wasm-session.worker.js', import.meta.url), { type: 'module' }));
    let worker;
    try { worker = createWorker(); }
    catch (error) {
      const failure = new SessionWorkerError('WORKER_CREATE_FAILED', 'Could not create the WASM session worker', { cause: error });
      onLifecycle?.({ status: 'failed', error: failure, recoverable: true, unsavedStateRestored: false });
      throw failure;
    }
    const backend = new DirectWasmBackend(worker, { requestTimeoutMs, maxPendingRequests, maxPayloadBytes, initialStatus: 'initializing', workerFactory: createWorker, onLifecycle });
    try {
      backend._state = await backend._call('boot', [], startupTimeoutMs);
      backend._setLifecycle('ready');
      return backend;
    } catch (error) {
      throw backend._fail(error);
    }
  }
  get state() { return this._state; }
  get lifecycle() { return this._lifecycle; }
  get partRoutes() {
    return (this._state.snapshot?.project?.tracks || []).flatMap(track =>
      (track.parts || []).filter(part => part.kind === 'voice').map(part => ({
        trackId: track.id, partId: part.id, trackName: track.name, partName: part.name,
      })),
    );
  }
  _attachWorker(worker) {
    this.worker = worker;
    const handlers = {
      message: event => this._receive(event.data),
      error: event => this._fail(new SessionWorkerError('WORKER_ERROR', event.message || 'WASM session worker failed')),
      messageerror: () => this._fail(new SessionWorkerError('WORKER_MESSAGE_ERROR', 'WASM session worker sent an unreadable message')),
    };
    this._workerHandlers = handlers;
    for (const [type, listener] of Object.entries(handlers)) worker.addEventListener(type, listener);
  }
  _detachWorker() {
    if (this.worker && this._workerHandlers) {
      for (const [type, listener] of Object.entries(this._workerHandlers)) this.worker.removeEventListener?.(type, listener);
    }
    this._workerHandlers = null;
  }
  _setLifecycle(status, error = null) {
    this._lifecycle = { status, error, recoverable: status === 'failed', unsavedStateRestored: false };
    for (const listener of this._lifecycleListeners) listener(this._lifecycle);
  }
  async recover({ startupTimeoutMs = WORKER_STARTUP_TIMEOUT_MS } = {}) {
    if (this._lifecycle.status !== 'failed') {
      throw new SessionWorkerError('RECOVERY_NOT_AVAILABLE', `Cannot recover a WASM session worker from ${this._lifecycle.status}`);
    }
    this._setLifecycle('recovering');
    this._state = { sessionId: null, revision: 0, snapshot: null };
    try {
      this._attachWorker(this._workerFactory());
      this._setLifecycle('initializing');
      this._state = await this._call('boot', [], startupTimeoutMs);
      this._setLifecycle('ready');
      return { state: this._state, restored: false, requiresReopen: true };
    } catch (error) {
      throw this._fail(error);
    }
  }
  _fail(error) {
    if (this._lifecycle.status === 'failed' || this._lifecycle.status === 'closed') return this._lifecycle.error || error;
    const failure = error instanceof SessionWorkerError
      ? error
      : new SessionWorkerError('WORKER_FAILURE', error?.message || String(error));
    this._setLifecycle('failed', failure);
    this._failAll(failure);
    this._detachWorker();
    this.worker?.terminate();
    return failure;
  }
  _call(method, args = [], timeoutMs = this._requestTimeoutMs) {
    if (method !== 'boot' && this._lifecycle.status !== 'ready') {
      const code = this._lifecycle.status === 'closed' ? 'WORKER_CLOSED' : 'WORKER_UNAVAILABLE';
      return Promise.reject(new SessionWorkerError(code, `WASM session worker is ${this._lifecycle.status}; reopen the project to continue`));
    }
    const call = () => this._dispatch(method, args, timeoutMs);
    if (SESSION_RPC_MUTATIONS.has(method)) {
      if (this._pending.size + this._queuedMutations >= this._maxPendingRequests) {
        return Promise.reject(new SessionWorkerError('WORKER_QUEUE_FULL', `WASM session worker reached its ${this._maxPendingRequests}-request in-flight/queued limit`));
      }
      this._queuedMutations += 1;
      const orderedCall = () => {
        this._queuedMutations = Math.max(0, this._queuedMutations - 1);
        return call();
      };
      const ordered = this._mutationTail.then(orderedCall, orderedCall);
      this._mutationTail = ordered.catch(() => {});
      return ordered;
    }
    return call();
  }
  _dispatch(method, args, timeoutMs, { signal = null, transfer = [] } = {}) {
    if (this._closing && method !== 'dispose') {
      return Promise.reject(new SessionWorkerError('WORKER_CLOSED', 'WASM session worker is closing'));
    }
    if (method !== 'boot' && this._lifecycle.status !== 'ready') {
      const code = this._lifecycle.status === 'closed' ? 'WORKER_CLOSED' : 'WORKER_UNAVAILABLE';
      return Promise.reject(new SessionWorkerError(code, `WASM session worker is ${this._lifecycle.status}; reopen the project to continue`));
    }
    if (this._pending.size + this._queuedMutations >= this._maxPendingRequests) {
      return Promise.reject(new SessionWorkerError('WORKER_QUEUE_FULL', `WASM session worker reached its ${this._maxPendingRequests}-request in-flight/queued limit`));
    }
    const id = ++this._sequence;
    const baseRevision = this._state.revision;
    const baseSessionId = this._state.sessionId;
    const request = makeSessionRequest(id, method, args, baseRevision, this._maxPayloadBytes, baseSessionId);
    const payloadBytes = estimateSessionPayloadBytes(request);
    if (payloadBytes > this._maxPayloadBytes) {
      return Promise.reject(new SessionWorkerError('WORKER_PAYLOAD_TOO_LARGE', `WASM session request is ${payloadBytes} bytes; limit is ${this._maxPayloadBytes}`));
    }
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        const code = method === 'boot' ? 'WORKER_STARTUP_TIMEOUT' : 'WORKER_REQUEST_TIMEOUT';
        this._fail(new SessionWorkerError(code, `WASM session operation '${method}' did not respond within ${timeoutMs} ms`));
      }, timeoutMs);
      const cancel = () => {
        if (!this._pending.has(id)) return;
        clearTimeout(timer);
        this._pending.delete(id);
        signal?.removeEventListener('abort', cancel);
        this.worker?.postMessage({ protocol: request.protocol, version: request.version, type: 'cancel', id });
        reject(new SessionWorkerError('REQUEST_CANCELLED', `WASM session operation '${method}' was cancelled`));
      };
      this._pending.set(id, { resolve, reject, timer, cancel, method, baseSessionId });
      if (signal?.aborted) {
        clearTimeout(timer);
        this._pending.delete(id);
        reject(new SessionWorkerError('REQUEST_CANCELLED', `WASM session operation '${method}' was cancelled`));
        return;
      }
      signal?.addEventListener('abort', cancel, { once: true });
      this._pending.get(id).signal = signal;
      try {
        this.worker.postMessage(request, transfer);
      } catch (error) {
        this._fail(new SessionWorkerError('WORKER_POST_FAILED', `Could not send '${method}' to the WASM session worker`, { cause: error }));
      }
    });
  }
  _receive(message) {
    if (!isSessionRpcEnvelope(message)) {
      this._fail(new SessionWorkerError('WORKER_PROTOCOL_ERROR', 'WASM session worker sent an invalid RPC envelope'));
      return;
    }
    if (message.type === 'event' && message.event === 'session.change') {
      const nextState = message.payload?.state;
      const sessionChanged = Boolean(nextState?.sessionId && nextState.sessionId !== this._state.sessionId);
      const replacementPending = [...this._pending.values()].some(({ method }) => SESSION_RPC_SESSION_REPLACEMENTS.has(method));
      if (sessionChanged && !replacementPending) return;
      if (!sessionChanged && Number.isInteger(message.revision) && message.revision < this._state.revision) return;
      this._state = nextState || this._state;
      for (const listener of this._listeners) listener({ ...message.payload, state: this._state });
      return;
    }
    if (message.type === 'event' && message.event === 'render.progress') {
      for (const listener of this._listeners) listener({ reason: 'render-progress', result: message.payload, state: this._state });
      return;
    }
    const pending = this._pending.get(message?.id);
    if (!pending) return;
    this._pending.delete(message.id);
    clearTimeout(pending.timer);
    pending.signal?.removeEventListener('abort', pending.cancel);
    if (message.type === 'error') {
      const error = Object.assign(new Error(message.error?.message || 'WASM session request failed'), message.error);
      pending.reject(error);
    } else if (message.type === 'response') {
      const replacement = SESSION_RPC_SESSION_REPLACEMENTS.has(pending.method);
      if (pending.baseSessionId !== this._state.sessionId && !replacement) {
        pending.reject(new SessionWorkerError('STALE_SESSION', `Ignored response from session ${pending.baseSessionId ?? '(none)'}; current session is ${this._state.sessionId ?? '(none)'}`));
        return;
      }
      if (Number.isInteger(message.revision) && message.revision < this._state.revision) {
        pending.reject(new SessionWorkerError('STALE_REVISION', `Ignored stale response revision ${message.revision}; current revision is ${this._state.revision}`));
        return;
      }
      if (message.result?.snapshot !== undefined && 'revision' in message.result) this._state = message.result;
      pending.resolve(message.result);
    } else pending.reject(new SessionWorkerError('WORKER_PROTOCOL_ERROR', `Unexpected worker message type: ${message.type}`));
  }
  _failAll(error) {
    for (const pending of this._pending.values()) {
      clearTimeout(pending.timer);
      pending.signal?.removeEventListener('abort', pending.cancel);
      pending.reject(error);
    }
    this._pending.clear();
  }
  onLifecycle(fn) { this._lifecycleListeners.add(fn); return () => this._lifecycleListeners.delete(fn); }
  onChange(fn) { this._listeners.add(fn); return () => this._listeners.delete(fn); }
  readNotes(partId) { return this._call('readNotes', [partId]); }
  readTracks() { return this._call('readTracks'); }
  readCurveStrokeContext(partId, curveId) { return this._call('readCurveStrokeContext', [partId, curveId]); }
  createProject(options) { return this._call('createProject', [options]); }
  openStored(name) { return this._call('openStored', [name]); }
  importUstx(ustx, _name) { return this._call('importUstx', [ustx]); }
  apply(command) { return this._call('apply', [command]); }
  undo() { return this._call('undo'); }
  redo() { return this._call('redo'); }
  beginGroup(name) { return this._call('beginGroup', [name]); }
  applyGrouped(groupId, command) { return this._call('applyGrouped', [groupId, command]); }
  commitGroup(groupId) { return this._call('commitGroup', [groupId]); }
  cancelGroup(groupId) { return this._call('cancelGroup', [groupId]); }
  async save(name) { const result = await this._call('save', [name]); return result; }
  listProjects() { return this._call('listProjects'); }
  renderTrack(track, options = {}) { const { signal, ...renderOptions } = options; return this._dispatch('renderTrack', [track, renderOptions], this._requestTimeoutMs, { signal }); }
  renderMixdown({ signal } = {}) { return this._dispatch('renderMixdown', [], this._requestTimeoutMs, { signal }); }
  getPlaybackTake(takeId) { return this._call('getPlaybackTake', [takeId]); }
  encodeTakeWav(takeId) { return this._call('encodeTakeWav', [takeId]); }
  async registerMedia(mediaId, bytes) {
    await this.store.saveMedia(mediaId, bytes);
    const ownedBytes = bytes instanceof Uint8Array ? bytes.slice() : new Uint8Array(bytes).slice();
    return this._dispatch('registerMedia', [mediaId, ownedBytes], this._requestTimeoutMs, { transfer: [ownedBytes.buffer] });
  }
  saveAs(name) { return this._call('saveAs', [name]); }
  exportUstx() { return this._call('exportUstx'); }
  async close() {
    if (this._lifecycle.status === 'closed') return;
    const worker = this.worker;
    this._closing = true;
    try {
      if (this._lifecycle.status === 'ready') await this._call('dispose', [], this._closeTimeoutMs);
    } finally {
      this._detachWorker();
      worker?.terminate();
      this._failAll(new SessionWorkerError('WORKER_CLOSED', 'WASM session worker closed'));
      this._setLifecycle('closed');
    }
  }
}

export class HttpSessionBackend {
  constructor(baseUrl = '') { this.kind = 'http'; this.baseUrl = baseUrl.replace(/\/$/, ''); this.recoveryId = httpRecoveryId(this.baseUrl); this.sessionId = null; this.store = new ProjectStore(); this._state = { sessionId: null, revision: 0, snapshot: null }; this._listeners = new Set(); }
  get state() { return this._state; }
  onChange(fn) { this._listeners.add(fn); return () => this._listeners.delete(fn); }
  async request(path, { method = 'GET', body } = {}) {
    const response = await fetch(this.baseUrl + path, { method, headers: body === undefined ? {} : { 'content-type': 'application/json' }, body: body === undefined ? undefined : JSON.stringify(body) });
    const text = await response.text(); let value = null;
    try { value = text ? JSON.parse(text) : null; } catch { value = text; }
    if (value?.recovery_warning) {
      for (const fn of this._listeners) {
        fn({ reason: 'recovery-warning', warning: value.recovery_warning, state: this._state });
      }
    }
    if (!response.ok || value?.error) { const error = new Error(value?.error?.message || value?.message || response.statusText || 'Session request failed'); error.status = response.status; error.code = value?.error?.code || value?.code; throw error; }
    return value;
  }
  async refresh(reason = 'refresh', result = null) {
    if (!this.sessionId) this._state = { sessionId: null, revision: 0, snapshot: null };
    else { const snapshot = await this.request('/api/v2/sessions/' + encodeURIComponent(this.sessionId)); this._state = { sessionId: this.sessionId, revision: snapshot.revision, snapshot }; }
    for (const fn of this._listeners) fn({ reason, result, state: this._state }); return this._state;
  }
  get partRoutes() { return (this.state.snapshot?.project?.tracks || []).flatMap(t => (t.parts || []).filter(p => p.kind === 'voice').map(p => ({ trackId: t.id, partId: p.id, trackName: t.name, partName: p.name }))); }
  readNotes(partId) { return this.request('/api/v2/sessions/' + encodeURIComponent(this.sessionId) + '/parts/' + encodeURIComponent(partId) + '/notes'); }
  readTracks() { return this.request('/api/v2/sessions/' + encodeURIComponent(this.sessionId) + '/tracks'); }
  readCurveStrokeContext() { throw new Error('Native HTTP v2 does not expose curve stroke context yet'); }
  async open(source, reason) {
    const opened = await this.request('/api/v2/sessions', { method: 'POST', body: { source, recovery_id: this.recoveryId } });
    this.sessionId = opened.session_id; this._state = { sessionId: opened.session_id, revision: opened.revision, snapshot: opened.snapshot };
    for (const fn of this._listeners) fn({ reason, result: opened, state: this._state }); return opened.snapshot;
  }
  async restoreRecovery() {
    let opened;
    try {
      opened = await this.request(`/api/v2/recovery/${encodeURIComponent(this.recoveryId)}`, { method: 'POST' });
    } catch (error) {
      if (error?.status === 404) return null;
      throw error;
    }
    this.sessionId = opened.session_id;
    this._state = { sessionId: opened.session_id, revision: opened.revision, snapshot: opened.snapshot };
    for (const fn of this._listeners) fn({ reason: 'recovery', result: opened, state: this._state });
    return opened;
  }
  createProject({ name = 'Untitled', bpm = null } = {}) { return this.open({ type: 'new', name, bpm }, 'new'); }
  openStored(name) { return this.open({ type: 'stored_project', name }, 'open'); }
  importUstx(ustx, name = 'Imported') { return this.open({ type: 'imported_ustx', name, ustx }, 'import'); }
  async apply(command) { const result = await this.request('/api/v2/sessions/' + encodeURIComponent(this.sessionId) + '/commands', { method: 'POST', body: { expected_revision: this.state.revision, command } }); await this.refresh('edit', result); return result; }
  async applyGrouped(groupId, command) { const result = await this.request('/api/v2/sessions/' + encodeURIComponent(this.sessionId) + '/commands', { method: 'POST', body: { expected_revision: this.state.revision, group_id: groupId, command } }); await this.refresh('edit', result); return result; }
  history(action) { return this._history(action); }
  async _history(action) { const result = await this.request('/api/v2/sessions/' + encodeURIComponent(this.sessionId) + '/' + action, { method: 'POST', body: { expected_revision: this.state.revision } }); await this.refresh(action, result); return result; }
  undo() { return this.history('undo'); }
  redo() { return this.history('redo'); }
  async beginGroup(name = 'Edit') { return this.request('/api/v2/sessions/' + encodeURIComponent(this.sessionId) + '/groups', { method: 'POST', body: { expected_revision: this.state.revision, name } }); }
  async commitGroup(id) { const result = await this.request('/api/v2/sessions/' + encodeURIComponent(this.sessionId) + '/groups/' + encodeURIComponent(id) + '/commit', { method: 'POST', body: { expected_revision: this.state.revision } }); await this.refresh('group-commit', result); return result; }
  async cancelGroup(id) { const result = await this.request('/api/v2/sessions/' + encodeURIComponent(this.sessionId) + '/groups/' + encodeURIComponent(id) + '/cancel', { method: 'POST', body: { expected_revision: this.state.revision } }); await this.refresh('group-cancel', result); return result; }
  async save(name) { const result = await this.request('/api/v2/sessions/' + encodeURIComponent(this.sessionId) + '/save', { method: 'PUT', body: { name, expected_revision: this.state.revision } }); for (const fn of this._listeners) fn({ reason: 'save', result, state: this.state }); return result; }
  async saveAs(name) { const result = await this.request('/api/v2/sessions/' + encodeURIComponent(this.sessionId) + '/save-as', { method: 'PUT', body: { name, expected_revision: this.state.revision } }); await this.refresh('save-as', result); return result; }
  async exportUstx() { return (await this.request('/api/v2/sessions/' + encodeURIComponent(this.sessionId) + '/export')).ustx; }
  async close() { if (!this.sessionId) return; const id = this.sessionId; await this.request('/api/v2/sessions/' + encodeURIComponent(id), { method: 'DELETE' }); this.sessionId = null; return this.refresh('close'); }
  async listProjects() { const result = await this.request('/projects'); return (result.projects || []).map(name => ({ name })); }
  /** Installed voicebanks. A NAME, never a path — that is all §6.13 accepts. */
  async listVoicebanks() {
    const result = await this.request('/voicebanks');
    return (result.voicebanks || []).map(entry => ({ name: entry.name, dir: entry.dir, wavCount: entry.wav_count ?? 0 }));
  }
  /**
   * Render one track through the §6.13 job flow: POST the job (202), poll
   * `GET /api/v2/render/{id}` until terminal, then fetch the wav artifact.
   *
   * `expected_revision` is sent because the contract requires it — the job
   * snapshots the document at that revision, so an edit made while the render
   * is in flight cannot change what comes out.
   */
  async renderTrack(track, { voicebank, engine = 'r1', onProgress } = {}) {
    // The logical API takes a bank NAME. With none given, fall back to the one
    // installed bank that actually has samples — picking an empty one would
    // only produce silence and a confusing empty job. This is a lookup of a
    // host resource the user installed, not a fact about the project.
    const bank = voicebank ?? (await this.listVoicebanks()).find(entry => entry.wavCount > 0)?.name;
    if (!bank) throw new Error('no voicebank with samples is installed (the render route takes a bank name, not a path)');
    const started = await this.request('/api/v2/sessions/' + encodeURIComponent(this.sessionId) + '/render', {
      method: 'POST',
      body: { expected_revision: this.state.revision, voicebank: bank, track: Number(track), engine },
    });
    const status = await awaitRenderJob(path => this.request(path), started.render_id, { onProgress });
    const response = await fetch(this.baseUrl + '/api/v2/render/' + encodeURIComponent(started.render_id) + '/audio');
    if (!response.ok) {
      const text = await response.text();
      let parsed = null;
      try { parsed = text ? JSON.parse(text) : null; } catch { parsed = { message: text }; }
      const error = new Error(parsed?.error?.message || parsed?.message || response.statusText || 'render audio unavailable');
      error.code = parsed?.error?.code;
      throw error;
    }
    return { ...decodePcm16Wav(new Uint8Array(await response.arrayBuffer())), renderId: started.render_id, revision: status.revision };
  }
  async renderMixdown({ voicebank, engine = 'r1', onProgress } = {}) {
    const bank = voicebank ?? (await this.listVoicebanks()).find(entry => entry.wavCount > 0)?.name;
    if (!bank) throw new Error('no voicebank with samples is installed (the mixdown route takes a bank name, not a path)');
    const started = await this.request('/api/v2/sessions/' + encodeURIComponent(this.sessionId) + '/mixdown', {
      method: 'POST',
      body: { expected_revision: this.state.revision, voicebank: bank, engine },
    });
    const status = await awaitRenderJob(path => this.request(path), started.render_id, { onProgress });
    const response = await fetch(this.baseUrl + '/api/v2/render/' + encodeURIComponent(started.render_id) + '/audio');
    if (!response.ok) {
      const text = await response.text();
      let parsed = null;
      try { parsed = text ? JSON.parse(text) : null; } catch { parsed = { message: text }; }
      const error = new Error(parsed?.error?.message || parsed?.message || response.statusText || 'mixdown audio unavailable');
      error.code = parsed?.error?.code;
      throw error;
    }
    return { ...decodePcm16Wav(new Uint8Array(await response.arrayBuffer())), renderId: started.render_id, revision: status.revision };
  }
  async registerMedia(mediaId, bytes) {
    await this.store.saveMedia(mediaId, bytes);
    const response = await fetch(this.baseUrl + '/api/v2/media/' + encodeURIComponent(mediaId), {
      method: 'PUT',
      headers: { 'content-type': 'application/octet-stream' },
      body: bytes,
    });
    const value = await response.json().catch(() => null);
    if (!response.ok || value?.error) {
      throw new Error(value?.error?.message || value?.message || response.statusText || 'audio upload failed');
    }
    return value;
  }
}

export async function createSessionBackend(kind, options = {}) {
  if (kind === 'wasm') return DirectWasmBackend.create(options);
  if (kind === 'http') return new HttpSessionBackend(options.baseUrl || '');
  throw new Error('Unknown Session backend: ' + kind);
}

/**
 * Decode 16-bit PCM RIFF/WAVE into frame-aligned Float32 channels.
 *
 * The native render route answers `GET /api/v2/render/{id}/audio` with the wav
 * bytes the engine wrote (`WaveFileWriter.CreateWaveFile16`), so this only
 * unwraps a container — it derives no musical state. `samples` remains a mono
 * waveform projection while `channels` preserves the signal for playback.
 */
function decodePcm16Wav(bytes) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const tag = offset => String.fromCharCode(view.getUint8(offset), view.getUint8(offset + 1), view.getUint8(offset + 2), view.getUint8(offset + 3));
  if (tag(0) !== 'RIFF' || tag(8) !== 'WAVE') throw new Error('render audio is not a RIFF/WAVE payload');
  let sampleRate = 0;
  let channels = 0;
  let bitsPerSample = 0;
  let dataOffset = -1;
  let dataLength = 0;
  for (let offset = 12; offset + 8 <= view.byteLength;) {
    const id = tag(offset);
    const size = view.getUint32(offset + 4, true);
    const body = offset + 8;
    if (id === 'fmt ') {
      channels = view.getUint16(body + 2, true);
      sampleRate = view.getUint32(body + 4, true);
      bitsPerSample = view.getUint16(body + 14, true);
    } else if (id === 'data') {
      dataOffset = body;
      dataLength = size;
    }
    offset = body + size + (size % 2); // RIFF chunks are word-aligned
  }
  if (dataOffset < 0) throw new Error('render audio has no data chunk');
  if (bitsPerSample !== 16) throw new Error(`render audio is ${bitsPerSample}-bit; only 16-bit PCM is handled`);
  if (channels < 1) throw new Error('render audio has an invalid channel count');
  const frameCount = Math.floor(dataLength / (2 * channels));
  const samples = new Float32Array(frameCount);
  const channelData = Array.from({ length: channels }, () => new Float32Array(frameCount));
  for (let frame = 0; frame < frameCount; frame++) {
    let sum = 0;
    for (let channel = 0; channel < channels; channel++) {
      const sample = view.getInt16(dataOffset + (frame * channels + channel) * 2, true) / 32768;
      channelData[channel][frame] = sample;
      sum += sample;
    }
    samples[frame] = sum / channels;
  }
  return { sampleRate, samples, channels: channelData };
}

/** Poll a §6.13 render job until it reaches a terminal status. */
async function awaitRenderJob(request, renderId, { onProgress, intervalMs = 500, timeoutMs = 30 * 60 * 1000 } = {}) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const status = await request('/api/v2/render/' + encodeURIComponent(renderId));
    onProgress?.(status);
    // The contract's terminal success is `done`; `failed` and `cancelled` are
    // terminal and carry no audio artifact (§6.13). Anything else keeps polling.
    if (status.status === 'done') return status;
    if (status.status === 'failed' || status.status === 'cancelled') {
      const error = new Error(status.error?.message || `render ${status.status}`);
      error.code = status.error?.code || status.status;
      throw error;
    }
    if (Date.now() > deadline) throw new Error(`render ${renderId} did not finish within ${timeoutMs} ms`);
    await new Promise(resolve => setTimeout(resolve, intervalMs));
  }
}
