// Native Session WebSocket adapter. Project state and rendering stay in Rust;
// this module only negotiates transport and forwards bounded PCM blocks.
export const SESSION_STREAM_PROTOCOL = 'rusted-utau.v3';
const CONTRACT_VERSION = '3.0-draft';

export function websocketUrl(baseUrl) {
  const url = new URL('/api/v3/ws', baseUrl || window.location.href);
  url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
  return url.href;
}

/** Request a fresh one-playback grant from the loopback Rust host. */
export async function requestPlaybackCredential(baseUrl, fetchImpl = globalThis.fetch) {
  if (typeof fetchImpl !== 'function') return null;
  try {
    const url = new URL('/api/v3/credential', baseUrl || window.location.href);
    const response = await fetchImpl(url.href, {
      method: 'POST', mode: 'cors', credentials: 'omit', cache: 'no-store', redirect: 'error',
    });
    if (!response.ok) return null;
    const payload = await response.json();
    if (payload?.single_use !== true || payload?.expires_in_ms !== 30000
      || typeof payload?.credential !== 'string' || !/^[0-9a-f]{32}$/.test(payload.credential)) return null;
    return payload.credential;
  } catch {
    return null;
  }
}

export function decodeRup3(buffer) {
  if (!(buffer instanceof ArrayBuffer) || buffer.byteLength < 8) throw new Error('Invalid RUP3 audio frame');
  const bytes = new Uint8Array(buffer);
  if (String.fromCharCode(...bytes.subarray(0, 4)) !== 'RUP3') throw new Error('Unsupported audio frame');
  const headerLength = new DataView(buffer).getUint32(4, true);
  const payloadAt = 8 + headerLength;
  if (headerLength > 16 * 1024 || payloadAt > buffer.byteLength) throw new Error('Invalid RUP3 header length');
  const header = JSON.parse(new TextDecoder().decode(bytes.subarray(8, payloadAt)));
  const frames = Number(header.frame_count);
  if (!Number.isSafeInteger(frames) || frames < 0 || buffer.byteLength - payloadAt !== frames * 2 * 4) {
    throw new Error('RUP3 PCM length does not match its frame count');
  }
  const left = new Float32Array(frames);
  const right = new Float32Array(frames);
  const view = new DataView(buffer, payloadAt);
  for (let i = 0; i < frames; i++) {
    left[i] = view.getFloat32(i * 4, true);
    right[i] = view.getFloat32((frames + i) * 4, true);
    if (!Number.isFinite(left[i]) || !Number.isFinite(right[i])) throw new Error('RUP3 contains non-finite PCM');
  }
  return { header, left, right };
}

export class SessionStreamSocket {
  constructor({ baseUrl, sessionId, revision, credential, WebSocketCtor = WebSocket }) {
    if (!sessionId || !Number.isInteger(revision) || !credential) throw new Error('Native stream session credentials are unavailable');
    this.sessionId = sessionId;
    this.revision = revision;
    this.socket = new WebSocketCtor(websocketUrl(baseUrl), SESSION_STREAM_PROTOCOL);
    this.socket.binaryType = 'arraybuffer';
    this.nextRequestId = 0;
    this.pending = new Map();
    this.retryTimers = new Set();
    this.streamId = null;
    this.closed = false;
    this.credential = credential;
    this.socket.onmessage = event => this.receive(event.data);
    this.ready = new Promise((resolve, reject) => {
      this.resolveReady = resolve;
      this.rejectReady = reject;
      this.socket.onopen = () => this.socket.send(JSON.stringify({
        type: 'hello', session_id: null,
        params: { credential, contract_versions: [CONTRACT_VERSION], required_feature_groups: [] },
      }));
      this.socket.onerror = () => reject(new Error('Native audio WebSocket connection failed'));
      this.socket.onclose = () => {
        if (!this.closed) reject(new Error('Native audio WebSocket closed'));
        for (const pending of this.pending.values()) pending.reject(new Error('Native audio WebSocket closed'));
        this.pending.clear();
      };
    }).then(reply => {
      if (reply.status !== 'ready' || reply.contract_version !== CONTRACT_VERSION) throw new Error(reply.error?.message || 'Native audio stream is unavailable');
      this.featureGroups = reply.capabilities?.feature_groups || [];
      if (!this.featureGroups.includes('render.stream')) throw new Error('The native engine has not enabled progressive audio playback yet');
    });
  }

  receive(data) {
    let reply;
    let pcm = null;
    try {
      if (data instanceof ArrayBuffer) { pcm = decodeRup3(data); reply = pcm.header; }
      else reply = JSON.parse(String(data));
    }
    catch (error) { this.fail(error); return; }
    if (reply.type === 'hello') {
      if (reply.status === 'ready') this.resolveReady?.(reply);
      else this.rejectReady?.(new Error(reply.error?.message || 'Native audio stream handshake rejected'));
      return;
    }
    const pending = this.pending.get(reply.request_id);
    if (!pending) return;
    if (reply.status === 'data' && data instanceof ArrayBuffer) {
      pending.resolve({ ...reply, pcm });
      this.pending.delete(reply.request_id);
    } else if (reply.status === 'pending') {
      this.pending.delete(reply.request_id);
      clearTimeout(pending.timer);
      pending.timer = setTimeout(() => {
        this.retryTimers.delete(pending.timer);
        if (!this.closed) this.request(pending.method, pending.params).then(pending.resolve, pending.reject);
        else pending.reject(new Error('Audio stream stopped'));
      }, Math.max(1, Number(reply.retry_after_ms) || 10));
      this.retryTimers.add(pending.timer);
    } else {
      this.pending.delete(reply.request_id);
      if (reply.error) pending.reject(Object.assign(new Error(reply.error.message || 'Native stream request failed'), { code: reply.error.code }));
      else pending.resolve(reply);
    }
  }

  fail(error) {
    for (const pending of this.pending.values()) { clearTimeout(pending.timer); pending.reject(error); }
    this.pending.clear();
    this.rejectReady?.(error);
    this.socket.close();
  }

  async request(method, params = {}) {
    await this.ready;
    if (this.nextRequestId >= 4096) throw new Error('Native stream request limit reached; restart playback to continue');
    // The v3 server validates request_id as a JSON string. Keep the local
    // counter numeric for its bounded-request guard, but send a string on wire.
    const requestId = String(++this.nextRequestId);
    const pending = { requestId, method, params, resolve: null, reject: null, timer: null };
    const result = new Promise((resolve, reject) => { pending.resolve = resolve; pending.reject = reject; });
    this.pending.set(requestId, pending);
    this.sendPending(pending);
    return result;
  }

  sendPending(pending) {
    if (this.closed || this.socket.readyState !== 1) return;
    pending.timer = null;
    this.socket.send(JSON.stringify({
      request_id: pending.requestId, method: pending.method, session_id: this.sessionId, params: pending.params,
    }));
  }

  async begin() {
    const reply = await this.request('session_render_stream_begin', {
      expected_revision: this.revision,
      engine: 'r1',
      options: { mode: 'playback', sample_rate: 44100, layout: 'stereo', range: { start_frame: 0, end_frame: null } },
    });
    if (reply.status !== 'preparing' || !reply.stream_id || reply.revision !== this.revision) {
      throw new Error(reply.error?.message || 'Native stream did not accept the current project revision');
    }
    this.streamId = reply.stream_id;
    return reply;
  }

  async next(maxFrames) {
    const reply = await this.request('session_render_stream_next', {
      stream_id: this.streamId, max_frames: maxFrames, wait_ms: 1000,
    });
    if (reply.revision !== this.revision) throw new Error('Project changed while audio was streaming; press Play to restart');
    return reply;
  }

  async cancel() {
    if (!this.streamId || this.closed) return;
    try { await this.request('session_render_stream_cancel', { stream_id: this.streamId }); } catch { /* connection is closing */ }
  }

  async close() {
    if (this.closed) return;
    this.closed = true;
    for (const timer of this.retryTimers) clearTimeout(timer);
    this.retryTimers.clear();
    for (const pending of this.pending.values()) { clearTimeout(pending.timer); pending.reject(new Error('Audio stream stopped')); }
    this.pending.clear();
    this.socket.close();
  }
}
