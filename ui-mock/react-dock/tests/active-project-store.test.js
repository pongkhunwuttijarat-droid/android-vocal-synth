import test from 'node:test';
import assert from 'node:assert/strict';
import { readActiveProjectName, restoreActiveProject, setActiveProjectName } from '../src/openutau/active-project-store.js';

function memoryStorage(initial = {}) {
  const values = new Map(Object.entries(initial));
  return {
    getItem: key => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, String(value)),
    removeItem: key => values.delete(key),
  };
}

const key = 'openutau.activeProject.v1';

test('restores the remembered project only when it is in the stored-project list', async () => {
  const storage = memoryStorage({ [key]: 'Saved Song' });
  const opened = [];
  const engine = {
    async listProjects() { return [{ name: 'Other Song' }, { name: 'Saved Song' }]; },
    async openProject(name) { opened.push(name); },
  };

  assert.equal(await restoreActiveProject(engine, storage), true);
  assert.deepEqual(opened, ['Saved Song']);
  assert.equal(readActiveProjectName(storage), 'Saved Song');
});

test('clears a remembered project that is no longer stored', async () => {
  const storage = memoryStorage({ [key]: 'Removed Song' });
  let opened = false;
  const engine = {
    async listProjects() { return [{ name: 'Still Here' }]; },
    async openProject() { opened = true; },
  };

  assert.equal(await restoreActiveProject(engine, storage), false);
  assert.equal(opened, false);
  assert.equal(readActiveProjectName(storage), null);
});

test('accepts string entries from a project store', async () => {
  const storage = memoryStorage({ [key]: 'Saved Song' });
  let opened = false;
  const engine = {
    async listProjects() { return ['Saved Song']; },
    async openProject(name) { opened = name === 'Saved Song'; },
  };
  assert.equal(await restoreActiveProject(engine, storage), true);
  assert.equal(opened, true);
});

test('keeps the remembered name when listing fails so a later boot can retry', async () => {
  const storage = memoryStorage({ [key]: 'Saved Song' });
  const engine = { async listProjects() { throw new Error('temporary store failure'); } };
  assert.equal(await restoreActiveProject(engine, storage), false);
  assert.equal(readActiveProjectName(storage), 'Saved Song');
});

test('storage errors are non-fatal and empty names clear the key', () => {
  const brokenStorage = { getItem() { throw new Error('blocked'); }, setItem() { throw new Error('blocked'); }, removeItem() { throw new Error('blocked'); } };
  assert.equal(readActiveProjectName(brokenStorage), null);
  assert.doesNotThrow(() => setActiveProjectName('Saved Song', brokenStorage));
  const storage = memoryStorage({ [key]: 'Saved Song' });
  setActiveProjectName('  ', storage);
  assert.equal(readActiveProjectName(storage), null);
});
