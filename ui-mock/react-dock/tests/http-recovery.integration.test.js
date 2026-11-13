import test from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { createServer } from 'node:net';
import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { HttpSessionBackend } from '../src/session-backends.js';
import { createEngine } from '../src/openutau/engine.js';

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, '../../..');
const serverBinary = path.join(repo, 'native/target/debug/synth-server');
const demoPath = path.join(repo, 'native/tools/synth-cli/tests/data/demo-song.ustx');

async function unusedPort() {
  const server = createServer();
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  const port = server.address().port;
  await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
  return port;
}

async function startServer(root, { blockRecovery = false, port: requestedPort = null } = {}) {
  const projects = path.join(root, 'projects');
  const banks = path.join(root, 'banks');
  await mkdir(projects, { recursive: true });
  await mkdir(banks, { recursive: true });
  if (blockRecovery) await writeFile(path.join(projects, '.recovery'), 'intentional storage blocker');
  const port = requestedPort ?? await unusedPort();
  const child = spawn(serverBinary, [
    '--voicebanks', banks,
    '--projects', projects,
    '--bind', '127.0.0.1',
    '--port', String(port),
  ], { stdio: ['ignore', 'ignore', 'pipe'] });
  let stderr = '';
  child.stderr.setEncoding('utf8');
  child.stderr.on('data', chunk => { stderr += chunk; });
  const baseUrl = `http://127.0.0.1:${port}`;
  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) throw new Error(`synth-server exited (${child.exitCode}): ${stderr}`);
    try {
      const response = await fetch(`${baseUrl}/health`);
      if (response.ok) return { child, baseUrl, projects, stderr: () => stderr };
    } catch { /* server is still starting */ }
    await new Promise(resolve => setTimeout(resolve, 50));
  }
  child.kill('SIGKILL');
  throw new Error(`synth-server did not become healthy: ${stderr}`);
}

async function stopServer(child) {
  if (child.exitCode !== null) return;
  child.kill('SIGTERM');
  await Promise.race([
    once(child, 'exit'),
    new Promise(resolve => setTimeout(resolve, 3_000)),
  ]);
  if (child.exitCode === null) child.kill('SIGKILL');
}

function installTabStorage(baseUrl, recoveryId) {
  const previous = Object.getOwnPropertyDescriptor(globalThis, 'sessionStorage');
  const values = new Map([[`rusted-utau.httpRecovery.v1:${baseUrl}`, recoveryId]]);
  Object.defineProperty(globalThis, 'sessionStorage', {
    configurable: true,
    value: {
      getItem: key => values.get(key) ?? null,
      setItem: (key, value) => values.set(key, value),
    },
  });
  return () => {
    if (previous) Object.defineProperty(globalThis, 'sessionStorage', previous);
    else delete globalThis.sessionStorage;
  };
}

test('HTTP tab reload after server restart restores the latest edit without overwriting its named project', async t => {
  const root = await mkdtemp(path.join(os.tmpdir(), 'rusted-utau-http-recovery-restart-'));
  let server;
  try {
    server = await startServer(root);
    const recoveryId = 'recovery-restart-tab-0001';
    const restoreStorage = installTabStorage(server.baseUrl, recoveryId);
    t.after(restoreStorage);
    const projectName = 'Protected Recovery Project';
    const backend = new HttpSessionBackend(server.baseUrl);
    const demo = await readFile(demoPath, 'utf8');
    await backend.importUstx(demo, 'Recovery integration fixture');
    await backend.saveAs(projectName);
    const savedProject = await readFile(path.join(server.projects, `${projectName}.ustx`));
    const note = backend.state.snapshot.project.tracks[0].parts[0].notes[0];
    await backend.apply({ type: 'note.set_lyric', note_id: note.id, lyric: 'recovered after restart' });
    const oldSessionId = backend.state.sessionId;
    assert.equal(backend.state.revision, 1);

    const port = Number(new URL(server.baseUrl).port);
    await stopServer(server.child);
    server = await startServer(root, { port });
    const afterReload = new HttpSessionBackend(server.baseUrl);
    const recovered = await afterReload.restoreRecovery();

    assert.ok(recovered, 'the same tab recovery key should reopen its committed document');
    assert.notEqual(afterReload.state.sessionId, oldSessionId, 'restart creates a fresh session');
    assert.equal(afterReload.state.revision, 0, 'undo history/revision restart with the new session');
    assert.equal(recovered.store_name, projectName);
    assert.equal(afterReload.state.snapshot.project.tracks[0].parts[0].notes[0].id, note.id);
    assert.equal(afterReload.state.snapshot.project.tracks[0].parts[0].notes[0].lyric, 'recovered after restart');
    assert.deepEqual(await readFile(path.join(server.projects, `${projectName}.ustx`)), savedProject,
      'automatic recovery must not overwrite the explicitly saved named project');
  } finally {
    if (server) await stopServer(server.child);
    await rm(root, { recursive: true, force: true });
  }
});

test('HTTP editor exposes a failed recovery checkpoint while keeping the session usable', async t => {
  const root = await mkdtemp(path.join(os.tmpdir(), 'rusted-utau-http-recovery-failure-'));
  let server;
  let restoreStorage;
  try {
    server = await startServer(root, { blockRecovery: true });
    restoreStorage = installTabStorage(server.baseUrl, 'recovery-warning-tab-0001');
    const states = [];
    const engine = createEngine({ kind: 'http', baseUrl: server.baseUrl, onChange: state => states.push(state) });
    await engine.boot({ name: 'Recovery warning fixture', seed: false });

    assert.equal(engine.state.status, 'ready', 'a checkpoint failure does not discard the live session');
    assert.match(engine.state.error?.message || '', /automatic recovery checkpoint failed/i);
    assert.ok(states.some(state => /automatic recovery checkpoint failed/i.test(state.error?.message || '')),
      'the warning must reach the UI state rendered by the editor');
  } finally {
    restoreStorage?.();
    if (server) await stopServer(server.child);
    await rm(root, { recursive: true, force: true });
  }
});
