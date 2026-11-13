import test from 'node:test';
import assert from 'node:assert/strict';
import { DirectWasmBackend, SessionWorkerError } from '../src/session-backends.js';
import { makeSessionResponse } from '../src/openutau/session-worker-rpc.js';

class FakeWorker {
  listeners = new Map();
  messages = [];
  terminated = false;
  constructor({ respond = null } = {}) { this.respond = respond; }
  addEventListener(type, listener) {
    const entries = this.listeners.get(type) || new Set();
    entries.add(listener);
    this.listeners.set(type, entries);
  }
  removeEventListener(type, listener) { this.listeners.get(type)?.delete(listener); }
  postMessage(message) {
    this.messages.push(message);
    this.respond?.(message, this);
  }
  terminate() { this.terminated = true; }
  emit(type, event = {}) { for (const listener of this.listeners.get(type) || []) listener(event); }
  reply(message, value = makeSessionResponse(message.id, { sessionId: null, revision: 0, snapshot: null }, 0)) {
    this.emit('message', { data: value });
  }
}

test('boot timeout terminates worker and reports a reopen-required startup error', async () => {
  const worker = new FakeWorker();
  await assert.rejects(
    DirectWasmBackend.create({ startupTimeoutMs: 5, workerFactory: () => worker }),
    error => error instanceof SessionWorkerError && error.code === 'WORKER_STARTUP_TIMEOUT' && error.reopenRequired,
  );
  assert.equal(worker.terminated, true);
  assert.equal(worker.listeners.get('message')?.size || 0, 0, 'startup failure removes worker handlers');
});

test('worker construction failure publishes a recoverable lifecycle error', async () => {
  const statuses = [];
  await assert.rejects(
    DirectWasmBackend.create({ workerFactory: () => { throw new Error('worker constructor denied'); }, onLifecycle: state => statuses.push(state.status) }),
    error => error instanceof SessionWorkerError && error.code === 'WORKER_CREATE_FAILED',
  );
  assert.deepEqual(statuses, ['failed']);
});

test('successful boot publishes initializing then ready lifecycle states', async () => {
  const worker = new FakeWorker({ respond: (message, target) => {
    if (message.method === 'boot') queueMicrotask(() => target.reply(message));
    if (message.method === 'dispose') queueMicrotask(() => target.reply(message, makeSessionResponse(message.id, null, 0)));
  } });
  const statuses = [];
  const backend = await DirectWasmBackend.create({ workerFactory: () => worker, onLifecycle: state => statuses.push(state.status) });
  assert.deepEqual(statuses, ['initializing', 'ready']);
  assert.equal(backend.lifecycle.status, 'ready');
  await backend.close();
});

test('worker error rejects every pending request and enters a terminal failed state', async () => {
  const worker = new FakeWorker();
  const backend = new DirectWasmBackend(worker, { requestTimeoutMs: 1000 });
  const first = backend.readTracks();
  const second = backend.readNotes('part-1');
  worker.emit('error', { message: 'worker crashed' });
  await assert.rejects(first, error => error.code === 'WORKER_ERROR' && error.reopenRequired);
  await assert.rejects(second, error => error.code === 'WORKER_ERROR');
  assert.equal(backend.lifecycle.status, 'failed');
  assert.equal(backend.lifecycle.unsavedStateRestored, false);
  assert.equal(worker.terminated, true);
  await assert.rejects(backend.readTracks(), error => error.code === 'WORKER_UNAVAILABLE');
});

test('messageerror fails pending callers and removes all registered listeners', async () => {
  const worker = new FakeWorker();
  const backend = new DirectWasmBackend(worker);
  const pending = backend.readTracks();
  worker.emit('messageerror');
  await assert.rejects(pending, error => error.code === 'WORKER_MESSAGE_ERROR');
  assert.equal(backend.lifecycle.status, 'failed');
  for (const listeners of worker.listeners.values()) assert.equal(listeners.size, 0);
});

test('recover starts a fresh worker without claiming in-memory project restoration', async () => {
  const first = new FakeWorker();
  const makeReadyWorker = () => new FakeWorker({ respond: (message, worker) => {
    if (message.method === 'boot') queueMicrotask(() => worker.reply(message, makeSessionResponse(message.id, { sessionId: null, revision: 0, snapshot: null }, 0)));
    if (message.method === 'dispose') queueMicrotask(() => worker.reply(message, makeSessionResponse(message.id, null, 0)));
  } });
  const replacements = [makeReadyWorker(), makeReadyWorker()];
  const backend = new DirectWasmBackend(first, { workerFactory: () => replacements.shift() });
  first.emit('error', { message: 'lost' });
  await assert.rejects(backend.readTracks());
  const recovered = await backend.recover({ startupTimeoutMs: 100 });
  assert.equal(recovered.restored, false);
  assert.equal(recovered.requiresReopen, true);
  assert.equal(backend.lifecycle.status, 'ready');
  assert.equal(backend.state.snapshot, null);
  assert.equal(first.terminated, true);
  const second = backend.worker;
  second.emit('messageerror');
  assert.equal(second.terminated, true);
  for (const listeners of second.listeners.values()) assert.equal(listeners.size, 0);
  const recoveredAgain = await backend.recover({ startupTimeoutMs: 100 });
  assert.equal(recoveredAgain.restored, false);
  assert.equal(backend.lifecycle.status, 'ready');
  await backend.close();
  assert.equal(backend.lifecycle.status, 'closed');
  for (const listeners of backend.worker.listeners.values()) assert.equal(listeners.size, 0);
});

test('close timeout always terminates the worker and settles pending callers', async () => {
  const worker = new FakeWorker();
  const backend = new DirectWasmBackend(worker, { closeTimeoutMs: 5 });
  const closing = backend.close();
  await assert.rejects(closing, error => error.code === 'WORKER_REQUEST_TIMEOUT');
  assert.equal(worker.terminated, true);
  assert.equal(backend.lifecycle.status, 'closed');
  assert.equal(backend._pending.size, 0);
});

test('graceful shutdown waits for dispose and rejects other pending RPCs', async () => {
  const worker = new FakeWorker();
  const backend = new DirectWasmBackend(worker, { closeTimeoutMs: 100 });
  const pending = backend.readTracks();
  pending.catch(() => {});
  const closing = backend.close();
  await Promise.resolve();
  const dispose = worker.messages.find(message => message.method === 'dispose');
  assert.ok(dispose);
  worker.reply(dispose, makeSessionResponse(dispose.id, null, 0));
  await closing;
  await assert.rejects(pending, error => error.code === 'WORKER_CLOSED');
  assert.equal(worker.terminated, true);
  assert.equal(backend._pending.size, 0);
});
