import { WasmSessionRuntime } from './wasm-session-runtime.js';
import { encodeStereoPcm16Wav } from './pcm-wav.js';
import { collectSessionTransferables, estimateSessionPayloadBytes, isSessionRpcEnvelope, makeSessionFailure, makeSessionResponse, SESSION_RPC_MAX_PAYLOAD_BYTES, SESSION_RPC_MUTATIONS } from './session-worker-rpc.js';

let runtime = null;
const takes = new Map();
const lastTakeByKind = new Map();
let takeSequence = 0;
let mutationTail = Promise.resolve();
const cancelledRequests = new Set();
const requestPayloadBytes = new Map();
const requestPayloadLimits = new Map();
const requestMethods = new Map();
let outstandingRequestPayloadBytes = 0;
let peakOutstandingRequestPayloadBytes = 0;
let peakKnownMemoryBytes = 0;

function sampleKnownMemoryPeak(additional = null) {
  const buffers = [];
  const seen = new Set();
  if (additional) collectSessionTransferables(additional, buffers, seen);
  for (const take of takes.values()) collectSessionTransferables(take.audio, buffers, seen);
  const wasmBytes = runtime?.memoryStats().wasmLinearMemoryBytes || 0;
  const knownBytes = wasmBytes + outstandingRequestPayloadBytes + buffers.reduce((sum, buffer) => sum + buffer.byteLength, 0);
  peakKnownMemoryBytes = Math.max(peakKnownMemoryBytes, knownBytes);
}

function respond(id, result, revision, transfer = []) {
  if (cancelledRequests.delete(id)) return;
  const resultSize = estimateSessionPayloadBytes(makeSessionResponse(id, result, revision));
  const limit = requestPayloadLimits.get(id) ?? SESSION_RPC_MAX_PAYLOAD_BYTES;
  if (resultSize > limit) {
    const error = Object.assign(new Error(`Response payload ${resultSize} exceeds ${limit} byte limit`), { code: 'RESPONSE_TOO_LARGE' });
    self.postMessage(makeSessionFailure(id, error, revision));
    return;
  }
  self.postMessage(makeSessionResponse(id, result, revision), transfer);
}

function respondError(id, error, revision) {
  if (cancelledRequests.delete(id)) return;
  self.postMessage(makeSessionFailure(id, error, revision));
}

async function handleRequest(request) {
  const { id, method, args = [] } = request;
  try {
    if (cancelledRequests.delete(id)) return;
    if (runtime && Object.prototype.hasOwnProperty.call(request, 'baseSessionId') && request.baseSessionId !== runtime.state.sessionId) {
      const error = new Error(`Request was based on session ${request.baseSessionId ?? '(none)'}; current session is ${runtime.state.sessionId ?? '(none)'}`);
      error.code = 'STALE_SESSION';
      throw error;
    }
    if (runtime && Number.isInteger(request.baseRevision) && request.baseRevision !== runtime.state.revision) {
      const error = new Error(`Request was based on revision ${request.baseRevision}; current revision is ${runtime.state.revision}`);
      error.code = 'STALE_REVISION';
      throw error;
    }
    if (method === 'boot') {
      runtime = await WasmSessionRuntime.boot();
      runtime.onChange(event => self.postMessage({ protocol: request.protocol, version: request.version, type: 'event', event: 'session.change', revision: event?.state?.revision ?? runtime?.state?.revision ?? 0, payload: event }));
      runtime.onRenderProgress = payload => self.postMessage({ protocol: request.protocol, version: request.version, type: 'event', event: 'render.progress', revision: runtime?.state?.revision ?? 0, payload });
      const state = runtime.state;
      sampleKnownMemoryPeak();
      respond(id, state, state.revision);
      return;
    }
    if (!runtime) throw new Error('WASM session worker is not booted');
    if (method === 'state') {
      respond(id, runtime.state, runtime.state.revision);
      return;
    }
    if (method === 'partRoutes') {
      respond(id, runtime.partRoutes(), runtime.state.revision);
      return;
    }
    if (method === 'memoryStats') {
      respond(id, {
        ...runtime.memoryStats(),
        outstandingRequestPayloadBytes,
        peakOutstandingRequestPayloadBytes,
        peakKnownMemoryBytes,
        retainedPcmBytes: [...takes.values()].reduce((total, take) => total + take.audio.channels.reduce((sum, channel) => sum + channel.byteLength, 0), 0),
      }, runtime.state.revision);
      return;
    }
    if (method === 'dispose') {
      const finalRevision = runtime.state.revision;
      await runtime.close();
      runtime = null;
      respond(id, null, finalRevision);
      self.close();
      return;
    }
    if (method === 'getPlaybackTake') {
      const take = takes.get(args[0]);
      if (!take) throw new Error('The rendered take is no longer available');
      const result = { sampleRate: take.audio.sampleRate, channels: take.audio.channels.map(channel => channel.slice()) };
      sampleKnownMemoryPeak(result);
      respond(id, result, runtime.state.revision, collectSessionTransferables(result));
      return;
    }
    if (method === 'encodeTakeWav') {
      const take = takes.get(args[0]);
      if (!take) throw new Error('The rendered take is no longer available');
      const wavBytes = encodeStereoPcm16Wav(take.audio.channels, take.audio.sampleRate);
      const result = { sampleRate: take.audio.sampleRate, frames: take.audio.channels[0].length, wavBytes };
      sampleKnownMemoryPeak(result);
      respond(id, result, runtime.state.revision, collectSessionTransferables(result));
      return;
    }
    const fn = runtime[method];
    if (typeof fn !== 'function' || method.startsWith('_')) throw new Error(`Unknown WASM session operation: ${method}`);
    const result = await fn.apply(runtime, args);
    sampleKnownMemoryPeak(result);
    // Arrangement only needs the envelope. Keep the selected preview's worker
    // take alive and avoid retaining another full-length PCM buffer for it.
    if (method === 'renderTrack' && args[1]?.waveformOnly === true) {
      respond(id, {
        sampleRate: result.sampleRate, frames: result.frames ?? result.samples?.length ?? 0,
        peaks: result.peaks,
      }, runtime.state.revision);
      return;
    }
    if (method === 'renderTrack' || method === 'renderMixdown') {
      const takeId = `take-${++takeSequence}`;
      const kind = method === 'renderMixdown' ? 'mixdown' : 'track';
      const previous = lastTakeByKind.get(kind);
      if (previous) takes.delete(previous);
      // Playback only needs the two PCM channels. Drop the mono analysis buffer
      // and preview peaks from the worker-owned cache so a long stereo mixdown
      // does not retain a third full-length PCM copy.
      takes.set(takeId, { kind, audio: { sampleRate: result.sampleRate, channels: result.channels } });
      lastTakeByKind.set(kind, takeId);
      sampleKnownMemoryPeak(result);
      respond(id, {
        takeId, sampleRate: result.sampleRate, frames: result.samples.length,
        peaks: result.peaks,
      }, runtime.state.revision);
    } else respond(id, result, runtime.state.revision, collectSessionTransferables(result));
  } catch (error) {
    respondError(id, error, runtime?.state?.revision ?? null);
  } finally {
    cancelledRequests.delete(id);
    outstandingRequestPayloadBytes = Math.max(0, outstandingRequestPayloadBytes - (requestPayloadBytes.get(id) || 0));
    requestPayloadBytes.delete(id);
    requestPayloadLimits.delete(id);
    requestMethods.delete(id);
  }
}

self.onmessage = ({ data }) => {
  if (!isSessionRpcEnvelope(data)) {
    self.postMessage(makeSessionFailure(data?.id ?? -1, Object.assign(new Error('Invalid Session Worker RPC envelope'), { code: 'INVALID_RPC_ENVELOPE' }), runtime?.state?.revision ?? null));
    return;
  }
  if (data.type === 'cancel') {
    if (Number.isSafeInteger(data.id) && data.id > 0 && requestPayloadBytes.has(data.id)) {
      cancelledRequests.add(data.id);
      if (requestMethods.get(data.id) === 'renderMixdown') runtime?.cancelRenderMixdown();
    }
    return;
  }
  if (data.type !== 'request' || !Number.isSafeInteger(data.id) || data.id < 1 || typeof data.method !== 'string' || !Array.isArray(data.args) || (data.maxPayloadBytes !== undefined && (!Number.isSafeInteger(data.maxPayloadBytes) || data.maxPayloadBytes < 1 || data.maxPayloadBytes > SESSION_RPC_MAX_PAYLOAD_BYTES))) {
    self.postMessage(makeSessionFailure(data.id ?? -1, Object.assign(new Error('Malformed Session Worker request'), { code: 'INVALID_RPC_REQUEST' }), runtime?.state?.revision ?? null));
    return;
  }
  const size = estimateSessionPayloadBytes(data);
  const requestLimit = data.maxPayloadBytes ?? SESSION_RPC_MAX_PAYLOAD_BYTES;
  if (size > requestLimit) {
    self.postMessage(makeSessionFailure(data.id, Object.assign(new Error(`Request payload ${size} exceeds ${requestLimit} byte limit`), { code: 'PAYLOAD_TOO_LARGE' }), runtime?.state?.revision ?? null));
    return;
  }
  if (requestPayloadBytes.has(data.id)) {
    self.postMessage(makeSessionFailure(data.id, Object.assign(new Error(`Request id ${data.id} is already active`), { code: 'DUPLICATE_REQUEST_ID' }), runtime?.state?.revision ?? null));
    return;
  }
  requestPayloadBytes.set(data.id, size);
  requestPayloadLimits.set(data.id, data.maxPayloadBytes ?? SESSION_RPC_MAX_PAYLOAD_BYTES);
  requestMethods.set(data.id, data.method);
  outstandingRequestPayloadBytes += size;
  peakOutstandingRequestPayloadBytes = Math.max(peakOutstandingRequestPayloadBytes, outstandingRequestPayloadBytes);
  if (SESSION_RPC_MUTATIONS.has(data.method)) {
    mutationTail = mutationTail.then(() => handleRequest(data), () => handleRequest(data));
  } else void handleRequest(data);
};
