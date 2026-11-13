import test from 'node:test';
import assert from 'node:assert/strict';
import { createAudioPlayer, msToTick } from '../src/openutau/audio-playback.js';

class FakeAudioContext {
  constructor({ sampleRate }) {
    this.sampleRate = sampleRate;
    this.currentTime = 0;
    this.state = 'suspended';
    this.destination = {};
    this.audioWorklet = { addModule: async () => {} };
  }
  async resume() { this.state = 'running'; }
  createGain() { return { gain: { value: 1 }, connect() {} }; }
  createAnalyser() { return { connect() {}, getFloatTimeDomainData() {} }; }
  async close() { this.state = 'closed'; }
}

class FakeAudioWorkletNode {
  constructor(context, name, options) {
    FakeAudioWorkletNode.last = this;
    this.context = context;
    this.name = name;
    this.options = options;
    this.messages = [];
    this.port = { onmessage: null, postMessage: message => { this.lastMessage = message; this.messages.push(message); } };
  }
  connect() {}
  disconnect() {}
}

function replaceGlobal(t, name, value) {
  const previous = Object.getOwnPropertyDescriptor(globalThis, name);
  Object.defineProperty(globalThis, name, { configurable: true, writable: true, value });
  t.after(() => {
    if (previous) Object.defineProperty(globalThis, name, previous);
    else delete globalThis[name];
  });
}

test('AudioWorklet clock interpolates past 28 seconds between progress messages', async t => {
  replaceGlobal(t, 'window', { AudioContext: FakeAudioContext });
  replaceGlobal(t, 'AudioWorkletNode', FakeAudioWorkletNode);

  const rate = 44100;
  const player = createAudioPlayer({
    getTake: () => ({ takeId: 'take-1', sampleRate: rate }),
    getTakeAudio: async () => ({ sampleRate: rate, channels: [new Float32Array(rate * 60), new Float32Array(rate * 60)] }),
    getPool: () => null,
    getBpm: () => 120,
  });
  t.after(() => player.dispose());

  await player.play(0);
  const context = player.__dbg;
  assert.equal(player.playing, true);
  assert.ok(player.tickNow() >= 0);

  // Simulate the last worklet progress report landing at 14 seconds. Even if
  // another message is delayed, AudioContext.currentTime must keep the playhead
  // attached to the audio rather than freezing at that stale frame.
  const node = FakeAudioWorkletNode.last;
  assert.ok(node);
  node.port.onmessage({ data: { type: 'progress', frame: rate * 14, contextTime: 14 } });
  const fakeContext = node.context;
  fakeContext.currentTime = 29.5;

  const actual = player.tickNow();
  const expected = msToTick((29.5 - 14) * 1000 + 14 * 1000, 120);
  assert.ok(actual > msToTick(28000, 120), `playhead should pass 28 seconds, got ${actual} ticks`);
  assert.ok(Math.abs(actual - expected) < 1e-6, `expected ${expected} ticks, got ${actual}`);
  assert.equal(player.playing, true);
  assert.equal(context.ctxState, 'running');
});

test('rejected streaming never falls back to a selected-track preview', async t => {
  replaceGlobal(t, 'window', { AudioContext: FakeAudioContext });
  replaceGlobal(t, 'AudioWorkletNode', FakeAudioWorkletNode);
  class DisabledStreamSocket {
    static OPEN = 1;
    constructor() { this.readyState = 0; queueMicrotask(() => { this.readyState = 1; this.onopen?.(); }); }
    send(raw) {
      const request = JSON.parse(raw);
      if (request.type === 'hello') queueMicrotask(() => this.onmessage?.({ data: JSON.stringify({
        type: 'hello', status: 'ready', contract_version: '3.0-draft', capabilities: { feature_groups: [] },
      }) }));
    }
    close() { this.readyState = 3; this.onclose?.(); }
  }
  let previewReads = 0;
  const player = createAudioPlayer({
    getTake: () => ({ takeId: 'selected-track-preview', sampleRate: 44100 }),
    getCompletedTake: () => null,
    getTakeAudio: async () => { previewReads++; return { channels: [new Float32Array(44100), new Float32Array(44100)] }; },
    getSessionStream: () => ({ baseUrl: 'http://127.0.0.1:8765', sessionId: 'session-1', revision: 1, credential: 'a'.repeat(32), WebSocketCtor: DisabledStreamSocket }),
    getPool: () => ({ plan: [] }),
    getBpm: () => 120,
  });
  t.after(() => player.dispose());
  await assert.rejects(player.play(0), /progressive audio playback yet/);
  assert.equal(previewReads, 0);
});

test('stopping during native stream startup cannot fall through to completed-take playback', async t => {
  replaceGlobal(t, 'window', { AudioContext: FakeAudioContext });
  replaceGlobal(t, 'AudioWorkletNode', FakeAudioWorkletNode);

  let resolveBeginSent;
  const beginSent = new Promise(resolve => { resolveBeginSent = resolve; });
  class PendingBeginSocket {
    static OPEN = 1;
    constructor() {
      this.readyState = PendingBeginSocket.OPEN;
      queueMicrotask(() => this.onopen?.());
    }
    send(raw) {
      const request = JSON.parse(raw);
      if (request.type === 'hello') {
        queueMicrotask(() => this.onmessage?.({ data: JSON.stringify({
          type: 'hello', status: 'ready', contract_version: '3.0-draft',
          capabilities: { feature_groups: ['render.stream'] },
        }) }));
      } else if (request.method === 'session_render_stream_begin') {
        resolveBeginSent();
      }
    }
    close() {
      this.readyState = 3;
      queueMicrotask(() => this.onclose?.());
    }
  }

  let fallbackAudioReads = 0;
  const player = createAudioPlayer({
    getTake: () => ({ takeId: 'completed-fallback', sampleRate: 44100 }),
    getTakeAudio: async () => {
      fallbackAudioReads++;
      return { sampleRate: 44100, channels: [new Float32Array(64), new Float32Array(64)] };
    },
    getSessionStream: () => ({
      baseUrl: 'http://127.0.0.1:8765', sessionId: 's-1', revision: 4,
      credential: 'x'.repeat(32), WebSocketCtor: PendingBeginSocket,
    }),
    getPool: () => null,
    getBpm: () => 120,
  });
  t.after(() => player.dispose());

  const playback = player.play(0);
  await beginSent;
  player.pause();
  await assert.rejects(playback, error => error.code === 'PLAYBACK_CANCELLED');
  assert.equal(fallbackAudioReads, 0, 'a manual stop must not start the completed-take fallback');
  assert.equal(player.playing, false);
});

test('stopping while session credentials are pending prevents playback startup', async t => {
  replaceGlobal(t, 'window', { AudioContext: FakeAudioContext });
  replaceGlobal(t, 'AudioWorkletNode', FakeAudioWorkletNode);

  let resolveSession;
  const session = new Promise(resolve => { resolveSession = resolve; });
  let socketCreations = 0;
  class NeverOpenedSocket {
    constructor() { socketCreations++; }
  }
  let fallbackAudioReads = 0;
  const player = createAudioPlayer({
    getTake: () => ({ takeId: 'completed-fallback', sampleRate: 44100 }),
    getTakeAudio: async () => { fallbackAudioReads++; return { channels: [new Float32Array(8), new Float32Array(8)] }; },
    getSessionStream: () => session,
    getPool: () => null,
    getBpm: () => 120,
  });
  t.after(() => player.dispose());

  const playback = player.play(0);
  player.pause();
  resolveSession({
    baseUrl: 'http://127.0.0.1:8765', sessionId: 's-1', revision: 4,
    credential: 'x'.repeat(32), WebSocketCtor: NeverOpenedSocket,
  });
  await assert.rejects(playback, error => error.code === 'PLAYBACK_CANCELLED');
  assert.equal(socketCreations, 0);
  assert.equal(fallbackAudioReads, 0);
});

test('stopping while the stream worklet loads prevents socket startup', async t => {
  let resolveWorklet;
  let workletRequested;
  const requested = new Promise(resolve => { workletRequested = resolve; });
  const workletReady = new Promise(resolve => { resolveWorklet = resolve; });
  class PendingWorkletContext extends FakeAudioContext {
    constructor(options) {
      super(options);
      this.audioWorklet = { addModule: () => { workletRequested(); return workletReady; } };
    }
  }
  replaceGlobal(t, 'window', { AudioContext: PendingWorkletContext });
  replaceGlobal(t, 'AudioWorkletNode', FakeAudioWorkletNode);

  let socketCreations = 0;
  class NeverOpenedSocket {
    constructor() { socketCreations++; }
  }
  let fallbackAudioReads = 0;
  const player = createAudioPlayer({
    getTake: () => ({ takeId: 'completed-fallback', sampleRate: 44100 }),
    getTakeAudio: async () => { fallbackAudioReads++; return { channels: [new Float32Array(8), new Float32Array(8)] }; },
    getSessionStream: () => ({
      baseUrl: 'http://127.0.0.1:8765', sessionId: 's-1', revision: 4,
      credential: 'x'.repeat(32), WebSocketCtor: NeverOpenedSocket,
    }),
    getPool: () => null,
    getBpm: () => 120,
  });
  t.after(() => player.dispose());

  const playback = player.play(0);
  await requested;
  player.pause();
  resolveWorklet();
  await assert.rejects(playback, error => error.code === 'PLAYBACK_CANCELLED');
  assert.equal(socketCreations, 0);
  assert.equal(fallbackAudioReads, 0);
});

test('stream pump keeps a bounded sixteen-second lookahead and reports underruns', async t => {
  replaceGlobal(t, 'window', { AudioContext: FakeAudioContext });
  replaceGlobal(t, 'AudioWorkletNode', FakeAudioWorkletNode);

  function pcmFrame(header, left, right) {
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

  class StreamingSocket {
    static OPEN = 1;
    constructor() {
      this.readyState = 1;
      this.frame = 0;
      this.sequence = 0;
      queueMicrotask(() => this.onopen?.());
    }
    send(raw) {
      const request = JSON.parse(raw);
      if (request.type === 'hello') {
        queueMicrotask(() => this.onmessage?.({ data: JSON.stringify({
          type: 'hello', status: 'ready', contract_version: '3.0-draft',
          capabilities: { feature_groups: ['render.stream'] },
        }) }));
      } else if (request.method === 'session_render_stream_begin') {
        queueMicrotask(() => this.onmessage?.({ data: JSON.stringify({
          request_id: request.request_id, status: 'preparing', stream_id: 'stream-1',
          session_id: 's-1', revision: 4, sample_rate: 44100,
          limits: { max_block_frames: 4096 },
        }) }));
      } else if (request.method === 'session_render_stream_next' && request.request_id === '2') {
        this.frame = 2;
        this.sequence = 1;
        queueMicrotask(() => this.onmessage?.({ data: pcmFrame({
          request_id: request.request_id, status: 'data', stream_id: 'stream-1',
          session_id: 's-1', revision: 4, sequence: 0, frame_start: 0,
        }, [0.1, 0.2], [-0.1, -0.2]) }));
      } else if (request.method === 'session_render_stream_next') {
        const frameStart = this.frame;
        const sequence = this.sequence++;
        const frames = 4096;
        this.frame += frames;
        queueMicrotask(() => this.onmessage?.({ data: pcmFrame({
          request_id: request.request_id, status: 'data', stream_id: 'stream-1',
          session_id: 's-1', revision: 4, sequence, frame_start: frameStart,
        }, new Float32Array(frames).fill(0.1), new Float32Array(frames).fill(-0.1)) }));
      } else {
        queueMicrotask(() => this.onmessage?.({ data: JSON.stringify({ request_id: request.request_id, status: 'closed' }) }));
      }
    }
    close() { this.readyState = 3; queueMicrotask(() => this.onclose?.()); }
  }

  const reports = [];
  const player = createAudioPlayer({
    getSessionStream: () => ({
      baseUrl: 'http://127.0.0.1:8765', sessionId: 's-1', revision: 4,
      credential: 'x'.repeat(32), WebSocketCtor: StreamingSocket,
    }),
    getPool: () => null,
    getBpm: () => 120,
    onStreamUnderrun: report => reports.push(report),
  });
  t.after(() => player.dispose());

  await player.play(0);
  const node = FakeAudioWorkletNode.last;
  await new Promise(resolve => setTimeout(resolve, 0));
  assert.ok(node.messages.some(message => message.type === 'pump-complete'),
    'the player must let the worklet re-arm a low-water demand after each pump');
  const pushedFrames = node.messages
    .filter(message => message.type === 'push')
    .reduce((total, message) => total + message.left.length, 0);
  assert.ok(pushedFrames >= 44100 * 16,
    `the stream should keep a sixteen-second lookahead for phrase synthesis, got ${pushedFrames} frames`);
  assert.ok(pushedFrames <= 44100 * 16 + 4096,
    `the bounded lookahead may overshoot by at most one negotiated block, got ${pushedFrames} frames`);
  node.port.onmessage({ data: { type: 'underrun', frameStart: 2, missingFrames: 128 } });
  node.port.onmessage({ data: { type: 'underrun', frameStart: 2, missingFrames: 64 } });
  assert.deepEqual(reports, [
    { events: 1, frames: 128, last: { frameStart: 2, missingFrames: 128 } },
    { events: 2, frames: 192, last: { frameStart: 2, missingFrames: 64 } },
  ]);
  assert.deepEqual(player.__dbg.streamUnderruns, reports[1]);
  assert.equal(player.playing, true, 'diagnostic messages must not stop the transport');
  const context = FakeAudioWorkletNode.last.context;
  context.currentTime = 0.05;
  const expectedMs = Math.max(0, (context.currentTime - 0.02) * 44100 - 192) / 44100 * 1000;
  assert.ok(Math.abs(player.tickNow() - msToTick(expectedMs, 120)) < 1e-6,
    'the playhead should exclude AudioContext time spent outputting silence');
  player.pause();
});

