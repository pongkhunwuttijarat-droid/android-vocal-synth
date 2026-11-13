import test from 'node:test';
import assert from 'node:assert/strict';
import { loadSingerChoices } from '../src/openutau/singer-choices.js';

test('HTTP singer choices come from renderable server voicebanks', async () => {
  const requested = [];
  const choices = await loadSingerChoices({
    backendKind: 'http',
    baseUrl: 'http://127.0.0.1:8765/',
    getInstalledRoots: async () => { throw new Error('HTTP should not read browser OPFS'); },
    fetchImpl: async url => {
      requested.push(url);
      return new Response(JSON.stringify({
        voicebanks: [
          { dir: 'Teto-root', name: '重音テト', wav_count: 857 },
          { dir: 'Empty-root', name: 'Empty bank', wav_count: 0 },
          { name: 'Missing root' },
          null,
        ],
      }), { status: 200, headers: { 'content-type': 'application/json' } });
    },
  });

  assert.deepEqual(choices, [{ id: 'Teto-root', label: '重音テト' }]);
  assert.deepEqual(requested, ['http://127.0.0.1:8765/voicebanks']);
});

test('Direct WASM singer choices continue to use browser-installed roots', async () => {
  const requested = [];
  const choices = await loadSingerChoices({
    backendKind: 'wasm',
    baseUrl: 'http://127.0.0.1:8765',
    getInstalledRoots: async () => ['Teto-root', 'Haru-root'],
    fetchImpl: async url => { requested.push(url); throw new Error('WASM should not request HTTP'); },
  });

  assert.deepEqual(choices, [
    { id: 'Teto-root', label: 'Teto-root' },
    { id: 'Haru-root', label: 'Haru-root' },
  ]);
  assert.deepEqual(requested, []);
});
