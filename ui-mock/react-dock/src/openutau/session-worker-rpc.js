// Internal host protocol for the Direct-WASM Session Worker. This is not the
// public Project Session API (CONTRACT-v1 §6).
export const SESSION_RPC_PROTOCOL = 'rusted-utau.session-worker';
export const SESSION_RPC_VERSION = 1;
export const SESSION_RPC_MAX_PENDING = 32;
export const SESSION_RPC_MAX_PAYLOAD_BYTES = 256 * 1024 * 1024;

export const SESSION_RPC_MUTATIONS = new Set([
  'createProject', 'openStored', 'importUstx', 'apply', 'undo', 'redo',
  'beginGroup', 'applyGrouped', 'commitGroup', 'cancelGroup', 'save', 'saveAs', 'dispose',
]);
export const SESSION_RPC_SESSION_REPLACEMENTS = new Set(['createProject', 'openStored', 'importUstx']);
const TRANSPORT_ERROR_CODES = new Set([
  'INVALID_RPC_ENVELOPE', 'INVALID_RPC_REQUEST', 'DUPLICATE_REQUEST_ID',
  'PAYLOAD_TOO_LARGE', 'RESPONSE_TOO_LARGE',
]);

export function makeSessionRequest(id, method, args, baseRevision, maxPayloadBytes = SESSION_RPC_MAX_PAYLOAD_BYTES, baseSessionId = null) {
  return {
    protocol: SESSION_RPC_PROTOCOL, version: SESSION_RPC_VERSION, type: 'request', id, method, args,
    baseRevision, maxPayloadBytes, ...(baseSessionId ? { baseSessionId } : {}),
  };
}

export function makeSessionResponse(id, result, revision) {
  return { protocol: SESSION_RPC_PROTOCOL, version: SESSION_RPC_VERSION, type: 'response', id, result, revision };
}

export function makeSessionFailure(id, error, revision) {
  const category = error?.code === 'STALE_REVISION' ? 'stale_revision' : TRANSPORT_ERROR_CODES.has(error?.code) ? 'transport' : 'domain';
  return {
    protocol: SESSION_RPC_PROTOCOL, version: SESSION_RPC_VERSION, type: 'error', id, revision,
    error: { category, name: error?.name || 'Error', code: error?.code || 'SESSION_OPERATION_FAILED', message: String(error?.message || error) },
  };
}

export function isSessionRpcEnvelope(value) {
  return value?.protocol === SESSION_RPC_PROTOCOL && value?.version === SESSION_RPC_VERSION;
}

// Approximate the structured-clone payload size without serializing or copying
// PCM/typed-array contents. Cycles are legal structured-clone values.
export function estimateSessionPayloadBytes(value, seen = new Set()) {
  if (value == null || typeof value === 'boolean' || typeof value === 'number') return 8;
  if (typeof value === 'string') return value.length * 2;
  if (typeof value !== 'object' || seen.has(value)) return 0;
  if (value instanceof ArrayBuffer) {
    if (seen.has(value)) return 0;
    seen.add(value);
    return value.byteLength;
  }
  if (ArrayBuffer.isView(value)) {
    const buffer = value.buffer;
    if (buffer instanceof ArrayBuffer) {
      if (seen.has(buffer)) return 0;
      seen.add(buffer);
      return buffer.byteLength;
    }
    return value.byteLength;
  }
  seen.add(value);
  if (Array.isArray(value)) return value.reduce((total, item) => total + estimateSessionPayloadBytes(item, seen), 16);
  return Object.entries(value).reduce((total, [key, item]) => total + key.length * 2 + estimateSessionPayloadBytes(item, seen), 32);
}

export function collectSessionTransferables(value, buffers = [], seen = new Set()) {
  if (!value || typeof value !== 'object' || seen.has(value)) return buffers;
  seen.add(value);
  if (value instanceof ArrayBuffer) { buffers.push(value); return buffers; }
  if (ArrayBuffer.isView(value)) {
    if (value.buffer instanceof ArrayBuffer && !seen.has(value.buffer)) { seen.add(value.buffer); buffers.push(value.buffer); }
    return buffers;
  }
  for (const child of Object.values(value)) collectSessionTransferables(child, buffers, seen);
  return buffers;
}
