import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import { SessionStreamSocket, decodeRup3, requestPlaybackCredential, websocketUrl } from '../src/openutau/session-stream.js';

function rup3(header, left = [], right = []) {
  const json = new TextEncoder().encode(JSON.stringify({ ...header, frame_count: left.length }));
  const buffer = new ArrayBuffer(8 + json.length + (left.length + right.length) * 4);
  const bytes = new Uint8Array(buffer);
  bytes.set(new TextEncoder().encode('RUP3'));
  new DataView(buffer).setUint32(4, json.length, true);
  bytes.set(json, 8);
  const view = new DataView(buffer);
  let offset = 8 + json.length;
  for (const sample of [...left, ...right]) { view.setFloat32(offset, sample, true); offset += 4; }
  return buffer;
}

class MockWebSocket {
  static OPEN = 1;
  static instances = [];
  constructor(url, protocol) {
    this.url = url;
    this.protocol = protocol;
    this.readyState = MockWebSocket.OPEN;
    this.sent = [];
    MockWebSocket.instances.push(this);
    queueMicrotask(() => this.onopen?.());
  }
  send(raw) {
    const request = JSON.parse(raw);
    this.sent.push(request);
    if (request.type === 'hello') {
      queueMicrotask(() => this.onmessage?.({ data: JSON.stringify({ type: 'hello', status: 'ready', contract_version: '3.0-draft', capabilities: { feature_groups: ['render.stream'] } }) }));
      return;
    }
    this.onRequest?.(request);
  }
  close() { this.readyState = 3; this.onclose?.(); }
  reply(value) { queueMicrotask(() => this.onmessage?.({ data: JSON.stringify(value) })); }
  replyBinary(value) { queueMicrotask(() => this.onmessage?.({ data: value })); }
}

const config = { baseUrl: 'http://127.0.0.1:8765', sessionId: 's-1', revision: 4, credential: 'x'.repeat(32), WebSocketCtor: MockWebSocket };

test('browser bootstrap requests a no-store, credential-free, one-use playback grant', async () => {
  let requested;
  const credential = 'a'.repeat(32);
  const result = await requestPlaybackCredential(config.baseUrl, async (url, options) => {
    requested = { url, options };
    return { ok: true, json: async () => ({ credential, expires_in_ms: 30000, single_use: true }) };
  });
  assert.equal(result, credential);
  assert.equal(requested.url, 'http://127.0.0.1:8765/api/v3/credential');
  assert.equal(requested.options.method, 'POST');
  assert.equal(requested.options.credentials, 'omit');
  assert.equal(requested.options.cache, 'no-store');
});

test('browser bootstrap rejects malformed grants and returns null on host errors', async () => {
  assert.equal(await requestPlaybackCredential(config.baseUrl, async () => ({ ok: true, json: async () => ({ credential: 'short', expires_in_ms: 30000, single_use: true }) })), null);
  assert.equal(await requestPlaybackCredential(config.baseUrl, async () => ({ ok: false })), null);
  assert.equal(await requestPlaybackCredential(config.baseUrl, async () => { throw new Error('offline'); }), null);
});

test('WebSocket URL uses the native server and secure scheme when needed', () => {
  assert.equal(websocketUrl('http://127.0.0.1:8765'), 'ws://127.0.0.1:8765/api/v3/ws');
  assert.equal(websocketUrl('https://engine.example:9443'), 'wss://engine.example:9443/api/v3/ws');
});

test('RUP3 decoder reads bounded planar little-endian float32 channels', () => {
  const decoded = decodeRup3(rup3({ request_id: 7, status: 'data', frame_start: 20, session_id: 's-1', revision: 4 }, [0.25, -0.5], [0.75, -1]));
  assert.equal(decoded.header.frame_start, 20);
  assert.deepEqual([...decoded.left], [0.25, -0.5]);
  assert.deepEqual([...decoded.right], [0.75, -1]);
  assert.throws(() => decodeRup3(rup3({ frame_count: 2 }, [NaN, 0], [0, 0])), /non-finite/);
  assert.throws(() => decodeRup3(new ArrayBuffer(9)), /Unsupported audio frame/);
});

test('begin negotiates revision-pinned stream and pending next retries with a fresh request ID', async () => {
  MockWebSocket.instances.length = 0;
  const socket = new SessionStreamSocket(config);
  const ws = MockWebSocket.instances.at(-1);
  ws.onRequest = request => {
    if (request.method === 'session_render_stream_begin') {
      ws.reply({ request_id: request.request_id, status: 'preparing', stream_id: 'stream-1', session_id: 's-1', revision: 4, sample_rate: 44100, limits: { max_block_frames: 4096 } });
    } else if (request.method === 'session_render_stream_next' && request.request_id === '2') {
      ws.reply({ request_id: request.request_id, status: 'pending', retry_after_ms: 1 });
    } else if (request.method === 'session_render_stream_next') {
      ws.replyBinary(rup3({ request_id: request.request_id, status: 'data', stream_id: 'stream-1', session_id: 's-1', revision: 4, sequence: 0, frame_start: 0 }, [0.1], [-0.1]));
    }
  };
  const begun = await socket.begin();
  assert.equal(begun.limits.max_block_frames, 4096);
  const reply = await socket.next(begun.limits.max_block_frames);
  assert.equal(reply.status, 'data');
  assert.ok(Math.abs(reply.pcm.left[0] - 0.1) < 1e-7);
  const nextRequests = ws.sent.filter(item => item.method === 'session_render_stream_next');
  assert.deepEqual(nextRequests.map(item => item.request_id), ['2', '3']);
  assert.ok(nextRequests.every(item => typeof item.request_id === 'string'));
  assert.ok(nextRequests.every(item => item.params.max_frames === 4096 && item.params.wait_ms === 1000));
  await socket.close();
});

test('handshake refuses progressive playback when the feature group is absent', async () => {
  MockWebSocket.instances.length = 0;
  const socket = new SessionStreamSocket(config);
  const ws = MockWebSocket.instances.at(-1);
  ws.onopen = () => ws.send(JSON.stringify({ type: 'hello', session_id: null, params: { credential: config.credential, contract_versions: ['3.0-draft'], required_feature_groups: [] } }));
  const original = ws.send.bind(ws);
  // Replace the ready response with the truthful currently deployed capability set.
  ws.send = raw => {
    const message = JSON.parse(raw);
    if (message.type === 'hello') {
      ws.sent.push(message);
      queueMicrotask(() => ws.onmessage?.({ data: JSON.stringify({ type: 'hello', status: 'ready', contract_version: '3.0-draft', capabilities: { feature_groups: [] } }) }));
    } else original(raw);
  };
  await assert.rejects(socket.begin(), /not enabled progressive audio playback yet/);
  await socket.close();
});

test('AudioWorklet reports contiguous starvation as one underrun interval', async () => {
  let Processor;
  const messages = [];
  class WorkletBase {
    constructor() {
      this.port = {
        onmessage: null,
        postMessage: message => messages.push(message),
      };
    }
  }
  const source = await readFile(new URL('../src/openutau/session-stream.worklet.js', import.meta.url), 'utf8');
  vm.runInNewContext(source, {
    AudioWorkletProcessor: WorkletBase,
    currentTime: 1,
    sampleRate: 44100,
    registerProcessor: (_name, processor) => { Processor = processor; },
  });

  const processor = new Processor();
  const output = [[new Float32Array(128), new Float32Array(128)]];
  processor.port.onmessage({ data: { type: 'start', frame: 0, startAt: 0 } });
  processor.process([], output);
  processor.process([], output);
  assert.equal(processor.underrunFrames, 256);

  processor.port.onmessage({ data: {
    type: 'push', frameStart: 0,
    left: new Float32Array(256).fill(0.25),
    right: new Float32Array(256).fill(-0.25),
  } });
  processor.process([], output);
  const report = messages.find(message => message.type === 'underrun');
  assert.deepEqual({
    frameStart: report.frameStart,
    missingFrames: report.missingFrames,
    totalUnderrunFrames: report.totalUnderrunFrames,
    totalUnderrunEvents: report.totalUnderrunEvents,
  }, { frameStart: 0, missingFrames: 256, totalUnderrunFrames: 256, totalUnderrunEvents: 1 });
  assert.deepEqual([...output[0][0]], Array(128).fill(0.25));
  assert.deepEqual([...output[0][1]], Array(128).fill(-0.25));

  processor.port.onmessage({ data: { type: 'finish' } });
  processor.process([], output);
  assert.equal(messages.filter(message => message.type === 'underrun').length, 1);
});

test('AudioWorklet rearms need-data after a completed underfilled pump', async () => {
  let Processor;
  const messages = [];
  class WorkletBase {
    constructor() {
      this.port = {
        onmessage: null,
        postMessage: message => messages.push(message),
      };
    }
  }
  const source = await readFile(new URL('../src/openutau/session-stream.worklet.js', import.meta.url), 'utf8');
  vm.runInNewContext(source, {
    AudioWorkletProcessor: WorkletBase,
    currentTime: 1,
    sampleRate: 44100,
    registerProcessor: (_name, processor) => { Processor = processor; },
  });

  const processor = new Processor();
  const output = [[new Float32Array(128), new Float32Array(128)]];
  processor.port.onmessage({ data: { type: 'start', frame: 0, startAt: 0 } });
  processor.process([], output);
  assert.equal(messages.filter(message => message.type === 'need-data').length, 1);

  // If the prior demand arrived while the main-thread pump was already active,
  // the worklet must make another demand after that pump completes if it is
  // still below the low-water mark.
  processor.port.onmessage({ data: { type: 'pump-complete' } });
  processor.process([], output);
  assert.equal(messages.filter(message => message.type === 'need-data').length, 2);
});

test('AudioWorklet asks for more PCM while fourteen seconds remain in its queue', async () => {
  let Processor;
  const messages = [];
  class WorkletBase {
    constructor() {
      this.port = {
        onmessage: null,
        postMessage: message => messages.push(message),
      };
    }
  }
  const source = await readFile(new URL('../src/openutau/session-stream.worklet.js', import.meta.url), 'utf8');
  vm.runInNewContext(source, {
    AudioWorkletProcessor: WorkletBase,
    currentTime: 1,
    sampleRate: 1000,
    registerProcessor: (_name, processor) => { Processor = processor; },
  });

  const processor = new Processor();
  const output = [[new Float32Array(128), new Float32Array(128)]];
  processor.port.onmessage({ data: { type: 'start', frame: 0, startAt: 0 } });
  processor.port.onmessage({ data: {
    type: 'push', frameStart: 0,
    left: new Float32Array(14140).fill(0.25),
    right: new Float32Array(14140).fill(-0.25),
  } });
  processor.process([], output);
  assert.equal(messages.filter(message => message.type === 'need-data').length, 0,
    'refill should not start while more than fourteen seconds remain');
  processor.process([], output);
  assert.equal(messages.filter(message => message.type === 'need-data').length, 1,
    'the stream must prefetch before its queue reaches the old sub-second threshold');
});
