import test from 'node:test';
import assert from 'node:assert/strict';
import { HttpSessionBackend, DirectWasmBackend } from '../src/session-backends.js';
import { collectSessionTransferables, estimateSessionPayloadBytes, makeSessionFailure, makeSessionResponse, SESSION_RPC_MAX_PAYLOAD_BYTES, SESSION_RPC_PROTOCOL, SESSION_RPC_VERSION } from '../src/openutau/session-worker-rpc.js';

test('native HTTP facade uses v2 Session lifecycle and command envelopes', async t => {
  const calls = [];
  let revision = 0;
  let snapshot = { session_id: 's-1', revision, project: { id: 'p-1', name: 'Tune', tracks: [] }, can_undo: false, can_redo: false };
  const previousFetch = globalThis.fetch;
  t.after(() => { globalThis.fetch = previousFetch; });
  globalThis.fetch = async (url, init = {}) => {
    calls.push({ url: String(url), method: init.method || 'GET', body: init.body ? JSON.parse(init.body) : undefined });
    const path = new URL(String(url)).pathname;
    if (path === '/api/v2/sessions' && init.method === 'POST') return Response.json({ session_id: 's-1', revision, snapshot });
    if (path === '/api/v2/sessions/s-1/commands') { revision += 1; snapshot = { ...snapshot, revision, can_undo: true }; return Response.json({ revision, changed: true, changes: [] }); }
    if (path === '/api/v2/sessions/s-1') return Response.json(snapshot);
    if (path === '/projects') return Response.json({ projects: ['Tune', 'Verse'] });
    if (path === '/api/v2/sessions/s-1/save') return Response.json({ saved: true, revision });
    if (path === '/api/v2/sessions/s-1/export') return Response.json({ revision, ustx: 'name: Tune\n' });
    throw new Error('Unexpected route ' + path);
  };
  const backend = new HttpSessionBackend('http://localhost:8080/');
  await backend.createProject({ name: 'Tune', bpm: 110 });
  assert.equal(backend.state.sessionId, 's-1');
  await backend.apply({ type: 'track.set_mute', track_id: 't-1', mute: true });
  assert.equal(backend.state.revision, 1);
  assert.deepEqual(await backend.listProjects(), [{ name: 'Tune' }, { name: 'Verse' }]);
  await backend.save('Tune');
  assert.equal(await backend.exportUstx(), 'name: Tune\n');
  assert.deepEqual(calls.find(call => call.url.endsWith('/commands')).body, {
    expected_revision: 0,
    command: { type: 'track.set_mute', track_id: 't-1', mute: true },
  });
  assert.equal(calls.find(call => call.url.endsWith('/save')).method, 'PUT');
});

test('drag gestures commit as one group transaction with the exact command envelopes', async t => {
  const calls = [];
  let revision = 5;
  let snapshot = { session_id: 's-2', revision, project: { id: 'p-2', name: 'Gestures', tracks: [] }, can_undo: true, can_redo: false };
  const previousFetch = globalThis.fetch;
  t.after(() => { globalThis.fetch = previousFetch; });
  globalThis.fetch = async (url, init = {}) => {
    calls.push({ url: String(url), method: init.method || 'GET', body: init.body ? JSON.parse(init.body) : undefined });
    const path = new URL(String(url)).pathname;
    if (path === '/api/v2/sessions' && init.method === 'POST') return Response.json({ session_id: 's-2', revision, snapshot });
    if (path === '/api/v2/sessions/s-2/commands') {
      revision += 1;
      snapshot = { ...snapshot, revision, can_undo: true };
      // note.add reports the minted id through its change patch (§6.5).
      return Response.json({ revision, changed: true, group_id: init.body ? JSON.parse(init.body).group_id : undefined, changes: [{ kind: 'note.added', part_id: 'part-1', note_id: 'n-1' }] });
    }
    if (path === '/api/v2/sessions/s-2/groups' && init.method === 'POST') return Response.json({ group_id: 'g-1', revision });
    if (path === '/api/v2/sessions/s-2/groups/g-1/commit') return Response.json({ revision, group_id: 'g-1', changed: true, changes: [] });
    if (path === '/api/v2/sessions/s-2') return Response.json(snapshot);
    throw new Error('Unexpected route ' + path);
  };
  const backend = new HttpSessionBackend('http://localhost:8080/');
  await backend.createProject({ name: 'Gestures' });

  // PixiRoll pen-down: note.add, then read the minted id from the change patch.
  const added = await backend.apply({ type: 'note.add', part_id: 'part-1', position_ticks: 330, tone: 69, duration_ticks: 30, lyric: 'la' });
  assert.equal(added.changes.find(change => change.kind === 'note.added').note_id, 'n-1');

  // A drag is one engine undo transaction: every intermediate delta joins the group.
  await backend.beginGroup('Move notes');
  await backend.applyGrouped('g-1', { type: 'note.move', note_id: 'n-1', delta_position_ticks: 30, delta_tone: 2 });
  await backend.applyGrouped('g-1', { type: 'note.move', note_id: 'n-1', delta_position_ticks: 30, delta_tone: 0 });
  await backend.applyGrouped('g-1', { type: 'note.resize', note_id: 'n-1', delta_duration_ticks: 90 });
  await backend.commitGroup('g-1');

  const commandBodies = calls.filter(call => call.url.endsWith('/commands')).map(call => call.body);
  assert.deepEqual(commandBodies[0], { expected_revision: 5, command: { type: 'note.add', part_id: 'part-1', position_ticks: 330, tone: 69, duration_ticks: 30, lyric: 'la' } });
  for (const body of commandBodies.slice(1)) {
    assert.equal(body.group_id, 'g-1', 'every drag delta must carry the group id');
    assert.equal(typeof body.expected_revision, 'number');
  }
  assert.deepEqual(commandBodies[1].command, { type: 'note.move', note_id: 'n-1', delta_position_ticks: 30, delta_tone: 2 });
  assert.deepEqual(commandBodies[3].command, { type: 'note.resize', note_id: 'n-1', delta_duration_ticks: 90 });
  assert.equal(calls.find(call => call.url.endsWith('/groups/g-1/commit')).method, 'POST');
});

test('drag gesture round-trips against a live Rust server and undoes as one step', { skip: process.env.RUSTED_UTAU_SERVER_URL ? false : 'set RUSTED_UTAU_SERVER_URL to run' }, async t => {
  const backend = new HttpSessionBackend(process.env.RUSTED_UTAU_SERVER_URL);
  await backend.createProject({ name: 'Gesture E2E' });
  const track = backend.state.snapshot.project.tracks[0];
  const created = await backend.apply({ type: 'part.create', track_id: track.id, kind: 'voice', name: 'Voice Part', position_ticks: 0, duration_ticks: 1920 });
  const partId = created.changes.find(change => change.kind === 'part.added')?.part_id
    ?? backend.state.snapshot.project.tracks[0].parts.at(-1).id;
  const added = await backend.apply({ type: 'note.add', part_id: partId, position_ticks: 330, tone: 69, duration_ticks: 30, lyric: 'la' });
  const noteId = added.changes.find(change => change.kind === 'note.added')?.note_id;
  assert.ok(noteId, 'note.add must mint a stable note id');

  const group = await backend.beginGroup('Drag smoke');
  const groupId = group.group_id;
  assert.ok(groupId, 'group begin must return a group id');
  await backend.applyGrouped(groupId, { type: 'note.move', note_id: noteId, delta_position_ticks: 60, delta_tone: 3 });
  await backend.applyGrouped(groupId, { type: 'note.resize', note_id: noteId, delta_duration_ticks: 120 });
  await backend.commitGroup(groupId);

  let notes = (await backend.readNotes(partId)).notes;
  const moved = notes.find(note => note.id === noteId);
  assert.equal(moved.position_ticks, 390);
  assert.equal(moved.tone, 72);
  assert.equal(moved.duration_ticks, 150);

  // The whole drag reverts with ONE undo (the point of the group transaction).
  await backend.undo();
  notes = (await backend.readNotes(partId)).notes;
  const reverted = notes.find(note => note.id === noteId);
  assert.deepEqual(
    { position: reverted.position_ticks, tone: reverted.tone, duration: reverted.duration_ticks },
    { position: 330, tone: 69, duration: 30 },
  );
  await backend.close();
});

/** Minimal 16-bit mono PCM wav, so the decode path is exercised on real bytes. */
function pcm16Wav(samples, sampleRate = 44100) {
  const data = Buffer.alloc(samples.length * 2);
  samples.forEach((value, index) => data.writeInt16LE(Math.max(-32768, Math.min(32767, Math.round(value * 32767))), index * 2));
  const header = Buffer.alloc(44);
  header.write('RIFF', 0, 'ascii');
  header.writeUInt32LE(36 + data.length, 4);
  header.write('WAVE', 8, 'ascii');
  header.write('fmt ', 12, 'ascii');
  header.writeUInt32LE(16, 16);
  header.writeUInt16LE(1, 20);
  header.writeUInt16LE(1, 22);
  header.writeUInt32LE(sampleRate, 24);
  header.writeUInt32LE(sampleRate * 2, 28);
  header.writeUInt16LE(2, 32);
  header.writeUInt16LE(16, 34);
  header.write('data', 36, 'ascii');
  header.writeUInt32LE(data.length, 40);
  return Buffer.concat([header, data]);
}

test('render follows the §6.13 job flow and returns decoded samples', async t => {
  const calls = [];
  const wav = pcm16Wav([0, 0.5, -0.5, 1]);
  const previousFetch = globalThis.fetch;
  t.after(() => { globalThis.fetch = previousFetch; });
  let polls = 0;
  globalThis.fetch = async (url, init = {}) => {
    const path = new URL(String(url)).pathname;
    calls.push({ path, method: init.method || 'GET', body: init.body ? JSON.parse(init.body) : undefined });
    if (path === '/api/v2/sessions') return Response.json({ session_id: 's-r', revision: 3, snapshot: { revision: 3 } });
    if (path === '/api/v2/sessions/s-r/render') return Response.json({ render_id: 'r-1', status: 'queued' }, { status: 202 });
    if (path === '/api/v2/render/r-1') {
      polls += 1;
      // One non-terminal poll first, so the loop is proven to wait rather than
      // assuming the first response is final.
      return polls === 1
        ? Response.json({ status: 'running', progress: 0.4 })
        : Response.json({ status: 'done', revision: 3 });
    }
    if (path === '/api/v2/render/r-1/audio') return new Response(wav, { headers: { 'content-type': 'audio/wav' } });
    throw new Error('Unexpected route ' + path);
  };

  const backend = new HttpSessionBackend('http://localhost:8765');
  await backend.createProject({ name: 'Tune' });
  const result = await backend.renderTrack(2, { voicebank: 'Teto' });

  assert.equal(result.sampleRate, 44100);
  // 16-bit PCM quantises, so compare within one LSB rather than exactly.
  const expected = [0, 0.5, -0.5, 1];
  assert.equal(result.samples.length, expected.length);
  result.samples.forEach((value, index) => {
    assert.ok(Math.abs(value - expected[index]) <= 1 / 32768, `sample ${index}: ${value} vs ${expected[index]}`);
  });
  assert.equal(result.renderId, 'r-1');

  const start = calls.find(call => call.path.endsWith('/render'));
  assert.equal(start.method, 'POST');
  // expected_revision is mandatory: the job snapshots the document there.
  assert.equal(start.body.expected_revision, 3);
  assert.equal(start.body.voicebank, 'Teto');
  assert.equal(start.body.track, 2);
  assert.equal(start.body.engine, 'r1');
  assert.equal(polls, 2, 'must poll until terminal, not read one response and stop');
});

test('a failed render job rejects instead of returning silence', async t => {
  const previousFetch = globalThis.fetch;
  t.after(() => { globalThis.fetch = previousFetch; });
  globalThis.fetch = async (url, init = {}) => {
    const path = new URL(String(url)).pathname;
    if (path === '/api/v2/sessions') return Response.json({ session_id: 's-f', revision: 1, snapshot: { revision: 1 } });
    if (path.endsWith('/render')) return Response.json({ render_id: 'r-f', status: 'queued' }, { status: 202 });
    if (path === '/api/v2/render/r-f') return Response.json({ status: 'failed', error: { code: 'render_error', message: 'no voicebank found' } });
    throw new Error('Unexpected route ' + path);
  };
  const backend = new HttpSessionBackend('http://localhost:8765');
  await backend.createProject({ name: 'Tune' });
  await assert.rejects(() => backend.renderTrack(0, { voicebank: 'Missing' }), /no voicebank found/);
});

test('with no voicebank given, the render picks an installed bank that has samples', async t => {
  const calls = [];
  const wav = pcm16Wav([0, 0.5, -0.5, 1]);
  const previousFetch = globalThis.fetch;
  t.after(() => { globalThis.fetch = previousFetch; });
  globalThis.fetch = async (url, init = {}) => {
    const path = new URL(String(url)).pathname;
    calls.push({ path, method: init.method || 'GET', body: init.body ? JSON.parse(init.body) : undefined });
    if (path === '/api/v2/sessions') return Response.json({ session_id: 's-v', revision: 2, snapshot: { revision: 2 } });
    if (path === '/voicebanks') return Response.json({ voicebanks: [{ name: 'Empty', dir: 'Empty', wav_count: 0 }, { name: 'Teto', dir: 'Teto', wav_count: 857 }] });
    if (path.endsWith('/render')) return Response.json({ render_id: 'r-v', status: 'queued' }, { status: 202 });
    if (path === '/api/v2/render/r-v') return Response.json({ status: 'done', revision: 2 });
    if (path === '/api/v2/render/r-v/audio') return new Response(wav);
    throw new Error('Unexpected route ' + path);
  };
  const backend = new HttpSessionBackend('http://localhost:8765');
  await backend.createProject({ name: 'Tune' });
  const result = await backend.renderTrack(0);
  assert.equal(calls.find(call => call.path.endsWith('/render')).body.voicebank, 'Teto');
  assert.equal(result.samples.length, 4);
});

test('rendering with no usable voicebank rejects instead of starting a job', async t => {
  const previousFetch = globalThis.fetch;
  t.after(() => { globalThis.fetch = previousFetch; });
  const calls = [];
  globalThis.fetch = async (url, init = {}) => {
    const path = new URL(String(url)).pathname;
    calls.push(path);
    if (path === '/api/v2/sessions') return Response.json({ session_id: 's-n', revision: 0, snapshot: { revision: 0 } });
    if (path === '/voicebanks') return Response.json({ voicebanks: [] });
    throw new Error('Unexpected route ' + path);
  };
  const backend = new HttpSessionBackend('http://localhost:8765');
  await backend.createProject({ name: 'Tune' });
  await assert.rejects(() => backend.renderTrack(0), /no voicebank with samples/);
  assert.equal(calls.filter(path => path.endsWith('/render')).length, 0, 'no render job may be started');
});

test('the direct WASM backend forwards waveform-only render requests through worker RPC', async () => {
  const listeners = {};
  const messages = [];
  const worker = {
    addEventListener: (type, listener) => { listeners[type] = listener; },
    postMessage: message => { messages.push(message); },
  };
  const backend = new DirectWasmBackend(worker);
  const pending = backend.renderTrack(2, { waveformOnly: true });
  assert.deepEqual(messages, [{ protocol: SESSION_RPC_PROTOCOL, version: SESSION_RPC_VERSION, type: 'request', id: 1, method: 'renderTrack', args: [2, { waveformOnly: true }], baseRevision: 0, maxPayloadBytes: SESSION_RPC_MAX_PAYLOAD_BYTES }]);
  const result = { sampleRate: 44100, frames: 3, peaks: [[-0.25, 0.25]] };
  listeners.message({ data: makeSessionResponse(1, result, 0) });
  assert.deepEqual(await pending, result);
});

test('the direct WASM backend encodes a worker-owned take as WAV without fetching its PCM', async () => {
  const listeners = {};
  const messages = [];
  const worker = {
    addEventListener: (type, listener) => { listeners[type] = listener; },
    postMessage: message => { messages.push(message); },
  };
  const backend = new DirectWasmBackend(worker);
  const pending = backend.encodeTakeWav('take-7');
  assert.deepEqual(messages, [{ protocol: SESSION_RPC_PROTOCOL, version: SESSION_RPC_VERSION, type: 'request', id: 1, method: 'encodeTakeWav', args: ['take-7'], baseRevision: 0, maxPayloadBytes: SESSION_RPC_MAX_PAYLOAD_BYTES }]);
  const result = { sampleRate: 44100, frames: 2, wavBytes: new ArrayBuffer(52) };
  listeners.message({ data: makeSessionResponse(1, result, 0) });
  assert.deepEqual(await pending, result);
});

test('Direct WASM mutation requests are sent in revision order', async () => {
  const listeners = {};
  const messages = [];
  const worker = {
    addEventListener: (type, listener) => { listeners[type] = listener; },
    postMessage: message => { messages.push(message); },
  };
  const backend = new DirectWasmBackend(worker);
  backend._state = { sessionId: 's', revision: 0, snapshot: { project: {} } };
  const first = backend.apply({ type: 'track.set_mute', mute: true });
  const second = backend.apply({ type: 'track.set_solo', solo: true });
  await Promise.resolve();
  assert.equal(messages.length, 1);
  listeners.message({ data: { protocol: SESSION_RPC_PROTOCOL, version: SESSION_RPC_VERSION, type: 'event', event: 'session.change', revision: 1, payload: { state: { sessionId: 's', revision: 1, snapshot: { project: {} } } } } });
  listeners.message({ data: makeSessionResponse(1, { revision: 1, changed: true }, 1) });
  await first;
  await Promise.resolve();
  assert.equal(messages[1].baseRevision, 1);
  listeners.message({ data: { protocol: SESSION_RPC_PROTOCOL, version: SESSION_RPC_VERSION, type: 'event', event: 'session.change', revision: 2, payload: { state: { sessionId: 's', revision: 2, snapshot: { project: {} } } } } });
  listeners.message({ data: makeSessionResponse(2, { revision: 2, changed: true }, 2) });
  await second;
});

test('Direct WASM transport rejects overload and reports explicit request cancellation', async () => {
  const listeners = {};
  const messages = [];
  const worker = { addEventListener: (type, listener) => { listeners[type] = listener; }, postMessage: message => messages.push(message), terminate() {} };
  const backend = new DirectWasmBackend(worker);
  const pending = Array.from({ length: 32 }, () => backend.readTracks());
  await assert.rejects(backend.readTracks(), error => error.code === 'WORKER_QUEUE_FULL');
  for (const request of pending) request.catch(() => {});
  backend._fail(new Error('test cleanup'));
  await Promise.allSettled(pending);

  const cancelListeners = {};
  const cancelMessages = [];
  const cancelWorker = { addEventListener: (type, listener) => { cancelListeners[type] = listener; }, postMessage: message => cancelMessages.push(message) };
  const cancelBackend = new DirectWasmBackend(cancelWorker);
  const controller = new AbortController();
  const render = cancelBackend.renderMixdown({ signal: controller.signal });
  controller.abort();
  await assert.rejects(render, error => error.code === 'REQUEST_CANCELLED');
  assert.ok(cancelMessages.some(message => message.type === 'cancel' && message.id === 1));
  assert.equal(cancelBackend._pending.size, 0);
});

test('bounded mutation queue counts work before it reaches the Worker', async () => {
  const worker = { addEventListener() {}, postMessage() {}, terminate() {} };
  const backend = new DirectWasmBackend(worker, { maxPendingRequests: 2 });
  const first = backend.apply({ type: 'track.set_mute', mute: true });
  const second = backend.apply({ type: 'track.set_solo', solo: true });
  first.catch(() => {});
  second.catch(() => {});
  await assert.rejects(backend.apply({ type: 'track.set_gain', gain: 1 }), error => error.code === 'WORKER_QUEUE_FULL');
  await Promise.resolve();
  assert.equal(backend._pending.size + backend._queuedMutations, 2);
  backend._fail(new Error('test cleanup'));
  await Promise.allSettled([first, second]);
  assert.equal(backend._queuedMutations, 0);
});

test('stale worker events cannot replace newer snapshots and transfer lists deduplicate buffers', async () => {
  const listeners = {};
  const worker = { addEventListener: (type, listener) => { listeners[type] = listener; }, postMessage: () => {} };
  const backend = new DirectWasmBackend(worker);
  const current = { sessionId: 's', revision: 8, snapshot: { project: { id: 'new' } } };
  backend._state = current;
  listeners.message({ data: { protocol: SESSION_RPC_PROTOCOL, version: SESSION_RPC_VERSION, type: 'event', event: 'session.change', revision: 7, payload: { state: { sessionId: 's', revision: 7, snapshot: { project: { id: 'old' } } } } } });
  assert.equal(backend.state.snapshot.project.id, 'new');
  const shared = new Float32Array(8);
  assert.deepEqual(collectSessionTransferables({ left: shared.subarray(0, 4), right: shared.subarray(4) }), [shared.buffer]);
  assert.equal(estimateSessionPayloadBytes({ pcm: shared }), shared.byteLength + 38);
  assert.equal(estimateSessionPayloadBytes({ left: shared.subarray(0, 4), right: shared.subarray(4) }), shared.byteLength + 50);
});

test('stale responses reject without replacing the current Rust snapshot', async () => {
  const listeners = {};
  const worker = { addEventListener: (type, listener) => { listeners[type] = listener; }, postMessage: () => {} };
  const backend = new DirectWasmBackend(worker);
  backend._state = { sessionId: 's', revision: 8, snapshot: { project: { id: 'current' } } };
  const pending = backend.readTracks();
  listeners.message({ data: makeSessionResponse(1, ['old'], 7) });
  await assert.rejects(pending, error => error.code === 'STALE_REVISION');
  assert.equal(backend.state.snapshot.project.id, 'current');
});

test('new Session identities may reset revisions, while old-session replies stay stale', async () => {
  const listeners = {};
  const messages = [];
  const worker = { addEventListener: (type, listener) => { listeners[type] = listener; }, postMessage: message => messages.push(message), terminate() {} };
  const backend = new DirectWasmBackend(worker);
  backend._state = { sessionId: 'session-old', revision: 5, snapshot: { project: { id: 'old' } } };

  const oldRead = backend.readTracks();
  const imported = backend.importUstx('name: New project\n');
  await Promise.resolve();
  assert.equal(messages[0].baseSessionId, 'session-old');
  assert.equal(messages[1].baseSessionId, 'session-old');

  listeners.message({ data: {
    protocol: SESSION_RPC_PROTOCOL, version: SESSION_RPC_VERSION, type: 'event',
    event: 'session.change', revision: 0,
    payload: { state: { sessionId: 'session-new', revision: 0, snapshot: { project: { id: 'new' } } } },
  } });
  assert.equal(backend.state.sessionId, 'session-new');
  assert.equal(backend.state.revision, 0);

  listeners.message({ data: makeSessionResponse(1, ['old tracks'], 5) });
  await assert.rejects(oldRead, error => error.code === 'STALE_SESSION');
  listeners.message({ data: makeSessionResponse(2, { revision: 0 }, 0) });
  assert.deepEqual(await imported, { revision: 0 });

  listeners.message({ data: {
    protocol: SESSION_RPC_PROTOCOL, version: SESSION_RPC_VERSION, type: 'event',
    event: 'session.change', revision: 6,
    payload: { state: { sessionId: 'session-old', revision: 6, snapshot: { project: { id: 'old-late' } } } },
  } });
  assert.equal(backend.state.sessionId, 'session-new');
  assert.equal(backend.state.snapshot.project.id, 'new');
  backend._fail(new Error('test cleanup'));
});

test('media bytes are stored from the caller buffer and transferred as an owned copy', async () => {
  const listeners = {};
  const sends = [];
  const worker = { addEventListener: (type, listener) => { listeners[type] = listener; }, postMessage: (message, transfer) => sends.push({ message, transfer }) };
  const backend = new DirectWasmBackend(worker);
  backend.store.saveMedia = async () => {};
  const original = new Uint8Array([1, 2, 3]);
  const pending = backend.registerMedia('audio-1', original);
  await Promise.resolve();
  assert.equal(sends[0].message.method, 'registerMedia');
  assert.notEqual(sends[0].message.args[1].buffer, original.buffer);
  assert.deepEqual(sends[0].transfer, [sends[0].message.args[1].buffer]);
  assert.deepEqual([...original], [1, 2, 3], 'ownership transfer must not detach the caller buffer');
  listeners.message({ data: makeSessionResponse(1, { registered: true }, 0) });
  assert.deepEqual(await pending, { registered: true });
});

test('configured payload ceiling rejects before a worker post', async () => {
  const messages = [];
  const worker = { addEventListener() {}, postMessage: message => messages.push(message) };
  const backend = new DirectWasmBackend(worker, { maxPayloadBytes: 48 });
  await assert.rejects(backend.importUstx('name: a project with a payload over the configured budget'), error => error.code === 'WORKER_PAYLOAD_TOO_LARGE');
  assert.equal(messages.length, 0);
});

test('typed failure envelopes distinguish domain, stale revision and transport limits', () => {
  assert.equal(makeSessionFailure(1, Object.assign(new Error('bad command'), { code: 'INVALID_COMMAND' }), 4).error.category, 'domain');
  assert.equal(makeSessionFailure(2, Object.assign(new Error('stale'), { code: 'STALE_REVISION' }), 5).error.category, 'stale_revision');
  assert.equal(makeSessionFailure(3, Object.assign(new Error('large response'), { code: 'RESPONSE_TOO_LARGE' }), 5).error.category, 'transport');
});

test('Direct WASM backend forwards render-worker progress without changing project revision', () => {
  const listeners = new Map();
  const worker = {
    addEventListener(type, listener) { listeners.set(type, listener); },
    removeEventListener() {},
    postMessage() {},
  };
  const backend = new DirectWasmBackend(worker);
  const events = [];
  backend.onChange(event => events.push(event));
  listeners.get('message')({ data: {
    protocol: SESSION_RPC_PROTOCOL, version: SESSION_RPC_VERSION, type: 'event',
    event: 'render.progress', revision: 9, payload: { jobId: 'render-a', phase: 'rendering', fraction: 0.2 },
  } });
  assert.equal(events.length, 1);
  assert.equal(events[0].reason, 'render-progress');
  assert.equal(events[0].result.jobId, 'render-a');
  assert.equal(backend.state.revision, 0, 'progress must not replace the Rust project snapshot');
});
