// Voicebank `.zip` install: the archives are built here rather than checked in
// as binaries, so every case (wrapper directory, flat archive, stored entry,
// zip-slip path, non-zip file) is readable and reproducible.
import test from 'node:test';
import assert from 'node:assert/strict';
import { deflateRawSync } from 'node:zlib';

import {
  characterNameFrom, commonTopDirectory, findOtoDir, parseZip, readZipEntries, readZipEntry, safeZipPath, writeStoredZip, zipBankRoot,
} from '../../../web/zip.js';

/**
 * Minimal store/deflate zip writer — enough to produce the archives above.
 *
 * A file may carry `nameBytes` instead of `path`, which writes those bytes as
 * the entry name with no UTF-8 flag (bit 11 clear). That is how a Japanese
 * Windows tool writes an archive, and it is what the real banks in
 * `test-ustx-example/voicebank/` contain.
 */
function makeZip(files) {
  const encoder = new TextEncoder();
  const locals = [];
  const centrals = [];
  let offset = 0;
  for (const { path, nameBytes, body, store = false } of files) {
    const name = nameBytes ?? encoder.encode(path);
    const raw = typeof body === 'string' ? encoder.encode(body) : body;
    const data = store ? raw : new Uint8Array(deflateRawSync(raw));
    const crc = crc32(raw);
    const local = new Uint8Array(30 + name.length + data.length);
    const lv = new DataView(local.buffer);
    lv.setUint32(0, 0x04034b50, true);
    lv.setUint16(4, 20, true);
    lv.setUint16(8, store ? 0 : 8, true);
    lv.setUint32(14, crc, true);
    lv.setUint32(18, data.length, true);
    lv.setUint32(22, raw.length, true);
    lv.setUint16(26, name.length, true);
    local.set(name, 30);
    local.set(data, 30 + name.length);
    const central = new Uint8Array(46 + name.length);
    const cv = new DataView(central.buffer);
    cv.setUint32(0, 0x02014b50, true);
    cv.setUint16(4, 20, true);
    cv.setUint16(10, store ? 0 : 8, true);
    cv.setUint32(16, crc, true);
    cv.setUint32(20, data.length, true);
    cv.setUint32(24, raw.length, true);
    cv.setUint16(28, name.length, true);
    cv.setUint32(42, offset, true);
    central.set(name, 46);
    locals.push(local);
    centrals.push(central);
    offset += local.length;
  }
  const directory = centrals.reduce((sum, part) => sum + part.length, 0);
  const localSize = locals.reduce((sum, part) => sum + part.length, 0);
  const eocd = new Uint8Array(22);
  const ev = new DataView(eocd.buffer);
  ev.setUint32(0, 0x06054b50, true);
  ev.setUint16(8, files.length, true);
  ev.setUint16(10, files.length, true);
  ev.setUint32(12, directory, true);
  ev.setUint32(16, localSize, true);
  const parts = [...locals, ...centrals, eocd];
  const total = parts.reduce((sum, part) => sum + part.length, 0);
  const out = new Uint8Array(total);
  let at = 0;
  for (const part of parts) { out.set(part, at); at += part.length; }
  return out;
}

let crcTable = null;
function crc32(bytes) {
  if (!crcTable) {
    crcTable = new Int32Array(256);
    for (let i = 0; i < 256; i += 1) {
      let c = i;
      for (let k = 0; k < 8; k += 1) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
      crcTable[i] = c;
    }
  }
  let crc = -1;
  for (const byte of bytes) crc = crcTable[(crc ^ byte) & 0xff] ^ (crc >>> 8);
  return (crc ^ -1) >>> 0;
}

const decoder = new TextDecoder();
const read = files => Object.fromEntries(files.map(file => [file.path, decoder.decode(file.bytes)]));

test('the browser ZIP writer produces readable UTF-8 archives and CRC rejects corruption', async () => {
  const zip = writeStoredZip([
    { path: 'manifest.json', bytes: new TextEncoder().encode('{"format":"rusted-utau"}') },
    { path: 'media/重音テト/audio.mp3', bytes: new Uint8Array([0, 1, 2, 255]) },
  ]);
  const { entries } = parseZip(zip);
  assert.deepEqual(entries.map(({ path, method }) => [path, method]), [
    ['manifest.json', 0],
    ['media/重音テト/audio.mp3', 0],
  ]);
  assert.deepEqual([...await readZipEntry(entries[1], zip)], [0, 1, 2, 255]);

  const corrupted = zip.slice();
  const entry = entries[1];
  const view = new DataView(corrupted.buffer, corrupted.byteOffset, corrupted.byteLength);
  const dataOffset = entry.localOffset + 30 + view.getUint16(entry.localOffset + 26, true) + view.getUint16(entry.localOffset + 28, true);
  corrupted[dataOffset] ^= 1;
  await assert.rejects(readZipEntry(entry, corrupted), /CRC check/);
});

test('a wrapped voicebank unpacks with the wrapper directory peeled off', async () => {
  const zip = makeZip([
    { path: 'Teto Bank/', body: '' },
    { path: 'Teto Bank/character.txt', body: 'name=重音テト\n' },
    { path: 'Teto Bank/voice/oto.ini', body: '[oto]\na.wav alias\n' },
    { path: 'Teto Bank/voice/a.wav', body: 'RIFF'.repeat(500) },
  ]);
  const { entries } = parseZip(zip);
  assert.deepEqual(entries.map(entry => entry.path), [
    'Teto Bank/character.txt', 'Teto Bank/voice/oto.ini', 'Teto Bank/voice/a.wav',
  ], 'the directory entry is not installed as a file');

  const prefix = commonTopDirectory(entries);
  assert.equal(prefix, 'Teto Bank');
  const files = await readZipEntries(zip, { prefix });
  assert.deepEqual(Object.keys(read(files)).sort(), ['character.txt', 'voice/a.wav', 'voice/oto.ini']);
  // Deflate really decompressed: the deflated wav round-trips to 2000 bytes.
  assert.equal(files.find(file => file.path === 'voice/a.wav').bytes.length, 2000);
});

test('a bank already at the archive root is installed as-is', async () => {
  const zip = makeZip([
    { path: 'character.txt', body: 'name=Flat Singer\n' },
    { path: 'voice/oto.ini', body: '[oto]\n' },
    { path: 'voice/a.wav', body: 'RIFF', store: true },
  ]);
  const { entries } = parseZip(zip);
  assert.equal(commonTopDirectory(entries), null, 'no wrapper to peel');
  const files = await readZipEntries(zip, { prefix: commonTopDirectory(entries) });
  assert.equal(decoder.decode(files.find(f => f.path === 'voice/a.wav').bytes), 'RIFF', 'stored entry reads back');
});

test('the bank root prefers character.txt, then the wrapper, then the file stem', async () => {
  const wrapped = makeZip([
    { path: 'archive-folder/character.txt', body: 'name=Teto Singer\n' },
    { path: 'archive-folder/voice/oto.ini', body: '[oto]\n' },
  ]);
  const { entries } = parseZip(wrapped);
  const files = await readZipEntries(wrapped, { prefix: commonTopDirectory(entries) });
  const character = files.find(file => file.path === 'character.txt');
  assert.equal(characterNameFrom(character.bytes), 'Teto Singer');
  assert.equal(zipBankRoot('download.zip', entries, characterNameFrom(character.bytes)), 'Teto Singer');
  assert.equal(zipBankRoot('download.zip', entries, null), 'archive-folder');

  // The file stem is the last resort: no `character.txt`, and nothing to peel.
  // A bank that is only `voice/…` must NOT have `voice` peeled as a wrapper —
  // that would lift `oto.ini` to the root and leave a bank with no aliases.
  const bare = makeZip([{ path: 'voice/oto.ini', body: '[oto]\n', store: true }]);
  const bareEntries = parseZip(bare).entries;
  assert.equal(commonTopDirectory(bareEntries), null);
  assert.equal(zipBankRoot('Teto-v1.2.zip', bareEntries, null), 'Teto-v1.2');
  assert.equal(zipBankRoot('', bareEntries, null), 'rusted-utau-voicebank');
});

test('a path that would escape the bank root is dropped, not installed', async () => {
  const zip = makeZip([
    { path: '../../evil.txt', body: 'pwned', store: true },
    { path: 'voice/oto.ini', body: '[oto]\n', store: true },
  ]);
  const { entries } = parseZip(zip);
  assert.deepEqual(entries.map(entry => entry.path), ['voice/oto.ini'], 'zip-slip entry never reaches OPFS');
  assert.equal(safeZipPath('../../evil.txt'), null);
  assert.equal(safeZipPath('/etc/passwd'), 'etc/passwd', 'a leading slash is rooted, not absolute');
  assert.equal(safeZipPath('voice/./oto.ini'), 'voice/oto.ini');
});

test('a file that is not a zip is reported, not half-installed', () => {
  const junk = new TextEncoder().encode('this is not a zip archive at all, not even close');
  assert.throws(() => parseZip(junk), /not a zip archive|no end-of-central-directory/);
});

// The two cases below are the real archives in `test-ustx-example/voicebank/`,
// reproduced by hand. Both installed successfully and produced no samples,
// which is the hardest kind of bug to notice: nothing errored.

test('a Shift-JIS bank name survives the round trip (TETO-English-150401)', () => {
  // These are the real Shift-JIS bytes for `重音テト音声ライブラリー`, taken from
  // the archive in `test-ustx-example/voicebank/`. That archive sets NO
  // general-purpose flags, so decoding its names as UTF-8 turned the directory
  // into U+FFFD replacement characters and the bank installed under a mojibake
  // path that no alias in `oto.ini` could ever resolve against.
  const bankDir = [143, 100, 137, 185, 131, 101, 131, 103, 137, 185, 144, 186, 131, 137, 131, 67, 131, 117, 131, 137, 131, 138, 129, 91];
  const zip = makeZip([
    { path: 'install.txt', body: 'install me', store: true },
    { nameBytes: new Uint8Array([...bankDir, ...Buffer.from('/character.txt', 'utf-8')]), body: 'name=Teto\n', store: true },
    { nameBytes: new Uint8Array([...bankDir, ...Buffer.from('/oto.ini', 'utf-8')]), body: '[oto]\n', store: true },
  ]);
  const { entries } = parseZip(zip);
  assert.ok(entries.every(entry => !entry.path.includes('\uFFFD')),
    `no replacement characters in ${JSON.stringify(entries.map(e => e.path))}`);
  assert.ok(entries.some(entry => entry.path === '重音テト音声ライブラリー/character.txt'),
    `expected the decoded bank directory, got ${JSON.stringify(entries.map(e => e.path))}`);
});

test('a stray top-level file does not hide the wrapper directory', () => {
  // `install.txt` sits at the archive root BESIDE the bank folder in both real
  // archives, so counting distinct top-level names gave 2, no wrapper was
  // peeled, the bank installed one level too deep and `oto.ini` was never found.
  const zip = makeZip([
    { path: 'install.txt', body: 'x', store: true },
    { path: '重音テト音声ライブラリー/character.txt', body: 'name=Teto\n', store: true },
    { path: '重音テト音声ライブラリー/重音テト英語音源/oto.ini', body: '[oto]\n', store: true },
  ]);
  const { entries } = parseZip(zip);
  assert.equal(commonTopDirectory(entries), '重音テト音声ライブラリー',
    'the stray root file must not count as a second top-level directory');
});

test('oto.ini is found where the bank puts it, not assumed to be in voice/', () => {
  // `TETO-English-150401.zip` keeps its samples in `重音テト英語音源/`. The
  // loader asked for `voice/oto.ini`, found nothing, and the bank rendered
  // silence while looking installed.
  const real = findOtoDir(['character.txt', '重音テト英語音源/oto.ini', '重音テト英語音源/a.wav']);
  assert.deepEqual(real, { otoPath: '重音テト英語音源/oto.ini', otoDir: '重音テト英語音源' });

  // The conventional layout still resolves the same way.
  assert.deepEqual(findOtoDir(['voice/oto.ini', 'voice/a.wav']),
    { otoPath: 'voice/oto.ini', otoDir: 'voice' });

  // A bank at the root has no directory at all, so aliases resolve bare.
  assert.deepEqual(findOtoDir(['oto.ini', 'a.wav']), { otoPath: 'oto.ini', otoDir: '' });

  // Shallowest wins, so a bank with a root oto.ini and per-colour ones is not
  // reduced to one colour's samples.
  assert.deepEqual(findOtoDir(['voice/colorA/oto.ini', 'voice/oto.ini']),
    { otoPath: 'voice/oto.ini', otoDir: 'voice' });

  assert.equal(findOtoDir(['character.txt', 'readme.txt']), null, 'no oto.ini at all');
});
