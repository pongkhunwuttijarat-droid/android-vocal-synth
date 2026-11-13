// The name an imported USTX carries. The engine mints the identities (§6.11
// `imported_ustx`), so this only decides what the title bar says — and it must
// prefer what the file declares over what the file happens to be called.
import test from 'node:test';
import assert from 'node:assert/strict';
import { importName, importUstxWithMedia, importWaveMediaFiles, matchWaveMediaFiles } from '../src/openutau/ustx-import.js';

test("the file's own name: field wins", () => {
  assert.equal(importName('whatever.ustx', "ustx_version: '0.9'\nname: midnight_demo\ntracks: []\n"), 'midnight_demo');
  assert.equal(importName('a.ustx', 'name: "quoted name"\n'), 'quoted name');
  assert.equal(importName('a.ustx', 'name: tight_value # comment\n'), 'tight_value');
});

test('the file stem is the fallback, and a nameless file still gets one', () => {
  assert.equal(importName('my-song.ustx', 'tracks: []\n'), 'my-song');
  assert.equal(importName('', ''), 'Imported');
});

test('an empty or commented name: does not produce a blank title', () => {
  // `name:` with nothing after it, or only a comment, must fall through.
  assert.equal(importName('fallback.ustx', 'name:\ntracks: []\n'), 'fallback');
  assert.equal(importName('fallback.ustx', 'name: # nothing\n'), 'fallback');
});

test('companion audio matches by basename, case, and Unicode normalization', () => {
  const part = {
    id: 'wave-1', kind: 'wave', name: 'Café backing.WAV',
    media_ref: 'C:\\\\source\\\\audio\\\\Café backing.wav',
  };
  const file = { name: 'Café backing.wav' };
  const result = matchWaveMediaFiles([{ name: 'Backing', parts: [part] }], [file]);
  assert.equal(result.matches.length, 1);
  assert.equal(result.matches[0].part.id, 'wave-1');
  assert.equal(result.matches[0].file, file);
  assert.deepEqual(result.unresolved, []);
});

test('missing and duplicate companion filenames stay unresolved', () => {
  const tracks = [{ parts: [
    { id: 'missing', kind: 'wave', name: 'Missing.mp3', media_ref: 'Missing.mp3' },
    { id: 'ambiguous', kind: 'wave', name: 'Backing.wav', media_ref: 'Backing.wav' },
    { id: 'already-linked', kind: 'wave', name: 'Existing.wav', media_ref: 'asset:stored-audio' },
  ] }];
  const result = matchWaveMediaFiles(tracks, [{ name: 'Backing.wav' }, { name: 'backing.WAV' }]);
  assert.deepEqual(result.matches, []);
  assert.deepEqual(result.unresolved.map(({ part, reason }) => [part.id, reason]), [
    ['missing', 'missing'],
    ['ambiguous', 'ambiguous'],
  ]);
});

test('the lifecycle adapter forwards selected companion files to the engine', () => {
  const mediaFiles = [{ name: 'backing.wav' }];
  let received;
  const engine = { importUstx: (...args) => { received = args; return 'imported'; } };
  assert.equal(importUstxWithMedia(engine, 'Song', 'tracks: []', mediaFiles), 'imported');
  assert.deepEqual(received, ['Song', 'tracks: []', mediaFiles]);
});

test('matched companion media is registered once and linked to every matching wave part', async () => {
  const file = { name: 'backing.wav', bytes: new Uint8Array([1, 2, 3]) };
  const tracks = [{ name: 'Audio', parts: [
    { id: 'wave-1', kind: 'wave', name: 'backing.wav', media_ref: '/old/backing.wav' },
    { id: 'wave-2', kind: 'wave', name: 'backing.wav', media_ref: '/old/backing.wav' },
  ] }];
  const registered = [];
  const commands = [];
  const result = await importWaveMediaFiles(tracks, [file], {
    registerMedia: async (mediaId, bytes) => {
      registered.push({ mediaId, bytes: [...bytes] });
      return { media_ref: `asset:${mediaId}` };
    },
    apply: async command => { commands.push(command); },
  });
  assert.equal(result.wavePartCount, 2);
  assert.equal(result.linkedMedia, 2);
  assert.equal(result.unresolvedMedia, 0);
  assert.equal(registered.length, 1);
  assert.deepEqual(registered[0].bytes, [1, 2, 3]);
  assert.deepEqual(commands.map(({ part_id, media_ref }) => [part_id, media_ref]), [
    ['wave-1', `asset:${registered[0].mediaId}`],
    ['wave-2', `asset:${registered[0].mediaId}`],
  ]);
});

test('missing and failed media links are reported without hiding the import', async () => {
  const tracks = [{ parts: [
    { id: 'available', kind: 'wave', name: 'backing.wav', media_ref: '/old/backing.wav' },
    { id: 'missing', kind: 'wave', name: 'missing.wav', media_ref: '/old/missing.wav' },
  ] }];
  const result = await importWaveMediaFiles(tracks, [{ name: 'backing.wav', bytes: new Uint8Array([9]) }], {
    registerMedia: async () => { throw new Error('storage unavailable'); },
    apply: async () => assert.fail('must not apply an asset ref when registration fails'),
  });
  assert.equal(result.wavePartCount, 2);
  assert.equal(result.linkedMedia, 0);
  assert.equal(result.unresolvedMedia, 2);
  assert.equal(result.mediaError, 'storage unavailable');
});
