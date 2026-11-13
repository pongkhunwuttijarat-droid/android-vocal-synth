import test from 'node:test';
import assert from 'node:assert/strict';

import { parseZip, readZipEntry, writeStoredZip } from '../../../web/zip.js';
import {
  createProjectBundle, readProjectBundle, restoreProjectBundleMedia,
  PROJECT_BUNDLE_EXTENSION, PROJECT_BUNDLE_FORMAT, PROJECT_BUNDLE_VERSION,
} from '../src/openutau/project-bundle.js';

const ustx = "ustx_version: '0.9'\nname: Song\ntracks: []\n";

test('rustx uses the approved identity and version', async () => {
  assert.equal(PROJECT_BUNDLE_EXTENSION, '.rustx');
  assert.equal(PROJECT_BUNDLE_FORMAT, 'rusted-utau-project');
  assert.equal(PROJECT_BUNDLE_VERSION, 1);
  const bundle = await createProjectBundle({ name: 'Song', ustx, tracks: [] });
  const archive = new Uint8Array(bundle.bytes);
  const { entries } = parseZip(archive);
  const manifestEntry = entries.find(entry => entry.path === 'manifest.json');
  assert.ok(manifestEntry);
  assert.deepEqual(JSON.parse(new TextDecoder().decode(await readZipEntry(manifestEntry, archive))), {
    format: 'rusted-utau-project', version: 1,
    project: { name: 'Song', file: 'project.ustx' }, media: [], unresolvedMedia: 0,
  });
});

test('a project bundle round-trips USTX and one shared media asset', async () => {
  const fileReads = [];
  const bytes = new Uint8Array([0, 1, 2, 3, 254, 255]);
  const bundle = await createProjectBundle({
    name: 'Song',
    ustx,
    tracks: [{ name: 'Audio', parts: [
      { id: 'part-1', kind: 'wave', name: 'Backing.mp3', media_ref: 'asset:media-one' },
      { id: 'part-2', kind: 'wave', name: 'Backing.mp3', media_ref: 'asset:media-one' },
      { id: 'part-missing', kind: 'wave', name: 'External.mp3', media_ref: '../Downloads/External.mp3' },
    ] }],
    loadMedia: async id => { fileReads.push(id); return bytes; },
  });

  assert.equal(bundle.mediaCount, 1);
  assert.equal(bundle.unresolvedMedia, 1);
  assert.deepEqual(fileReads, ['media-one']);
  const parsed = await readProjectBundle(bundle.bytes);
  assert.equal(parsed.name, 'Song');
  assert.equal(parsed.ustx, ustx);
  assert.equal(parsed.media.length, 1);
  assert.equal(parsed.media[0].id, 'media-one');
  assert.equal(parsed.media[0].name, 'Backing.mp3');
  assert.deepEqual([...parsed.media[0].bytes], [...bytes]);
  assert.equal(parsed.unresolvedMedia, 1);
});

test('a project bundle preserves a UTF-8 BOM in the original USTX text', async () => {
  const ustxWithBom = `\uFEFF${ustx}`;
  const bundle = await createProjectBundle({ name: 'Song', ustx: ustxWithBom, tracks: [] });
  const parsed = await readProjectBundle(bundle.bytes);
  assert.equal(parsed.ustx, ustxWithBom);
});

test('a bundle with absent OPFS media stays importable and reports that part', async () => {
  const bundle = await createProjectBundle({
    name: 'Song', ustx,
    tracks: [{ parts: [{ id: 'wave-1', kind: 'wave', name: 'Lost.wav', media_ref: 'asset:not-on-disk' }] }],
    loadMedia: async () => { throw Object.assign(new Error('not found'), { name: 'NotFoundError' }); },
  });
  assert.equal(bundle.mediaCount, 0);
  assert.equal(bundle.unresolvedMedia, 1);
  const parsed = await readProjectBundle(bundle.bytes);
  assert.equal(parsed.media.length, 0);
  assert.equal(parsed.unresolvedMedia, 1);
});

test('bundle reader rejects unsupported versions and corrupt archive contents', async () => {
  const makeArchive = manifest => writeStoredZip([
    { path: 'manifest.json', bytes: new TextEncoder().encode(JSON.stringify(manifest)) },
    { path: 'project.ustx', bytes: new TextEncoder().encode(ustx) },
  ]);
  const unsupported = makeArchive({
    format: 'rusted-utau-project', version: 99,
    project: { name: 'Song', file: 'project.ustx' }, media: [],
  });
  await assert.rejects(readProjectBundle(unsupported), /unsupported project bundle/);

  const valid = makeArchive({
    format: 'rusted-utau-project', version: 1,
    project: { name: 'Song', file: 'project.ustx' }, media: [],
  });
  const corrupted = valid.slice();
  const marker = new TextEncoder().encode('ustx_version');
  let offset = -1;
  for (let at = 0; at <= corrupted.length - marker.length; at += 1) {
    if (marker.every((byte, index) => corrupted[at + index] === byte)) { offset = at; break; }
  }
  assert.notEqual(offset, -1);
  corrupted[offset] ^= 1;
  await assert.rejects(readProjectBundle(corrupted), /CRC check/);
});

test('media restore preserves stable IDs and converts missing refs to relinkable placeholders', async () => {
  const tracks = [{ parts: [
    { id: 'part-ok', kind: 'wave', name: 'Backing.mp3', media_ref: 'asset:known' },
    { id: 'part-missing', kind: 'wave', name: 'Lost.wav', media_ref: 'asset:absent' },
    { id: 'part-external', kind: 'wave', name: 'External.wav', media_ref: '../Downloads/External.wav' },
  ] }];
  const registered = [];
  const applied = [];
  const result = await restoreProjectBundleMedia(tracks, [{
    id: 'known', name: 'Backing.mp3', bytes: new Uint8Array([5, 6]),
  }], {
    registerMedia: async (id, bytes) => {
      registered.push({ id, bytes: [...bytes] });
      return { media_ref: `asset:${id}` };
    },
    apply: async command => { applied.push(command); },
  });

  assert.equal(result.wavePartCount, 3);
  assert.equal(result.linkedMedia, 1);
  assert.equal(result.unresolvedMedia, 2);
  assert.deepEqual(registered, [{ id: 'known', bytes: [5, 6] }]);
  assert.deepEqual(applied.map(({ part_id, media_ref }) => [part_id, media_ref]), [
    ['part-missing', 'missing/part-missing'],
    ['part-external', 'missing/part-external'],
  ]);
});

test('media restore can match a unique filename when an older project ref differs', async () => {
  const registered = [];
  const applied = [];
  const result = await restoreProjectBundleMedia([{ parts: [
    { id: 'part-1', kind: 'wave', name: 'Clip.wav', media_ref: 'old/location/Clip.wav' },
  ] }], [{ id: 'bundle-asset', name: 'Clip.wav', bytes: new Uint8Array([9]) }], {
    registerMedia: async id => { registered.push(id); return { media_ref: `asset:${id}` }; },
    apply: async command => { applied.push(command); },
  });
  assert.equal(result.linkedMedia, 1);
  assert.equal(result.unresolvedMedia, 0);
  assert.deepEqual(registered, ['bundle-asset']);
  assert.deepEqual(applied, [{ type: 'part.set_media_ref', part_id: 'part-1', media_ref: 'asset:bundle-asset' }]);
});
