import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash, webcrypto } from 'node:crypto';
import { captureCommittedRenderSnapshot, fingerprintRenderJob, pinRenderAssetBytes } from '../../../web/app/session-render-contract.js';

test('committed render snapshot includes canonical project and both Rust sidecars', () => {
  const engine = {
    session_id: () => 'session-1',
    session_revision: () => 7n,
    project_snapshot: () => 'ustx-from-rust',
    session_snapshot_json: () => JSON.stringify({ project: { tracks: [] } }),
    session_save_sidecar: () => 'ids-from-rust',
    session_save_pronunciation_sidecar: () => 'pronunciation-from-rust',
  };
  const snapshot = captureCommittedRenderSnapshot(engine, 7);
  assert.deepEqual(snapshot, {
    sessionId: 'session-1', revision: 7, ustx: 'ustx-from-rust',
    identitySidecar: 'ids-from-rust', pronunciationSidecar: 'pronunciation-from-rust',
    projectSnapshot: { project: { tracks: [] } },
  });
  assert.equal(Object.isFrozen(snapshot), true);
  assert.equal(Object.isFrozen(snapshot.projectSnapshot.project.tracks), true);
});

test('render assets are copied, sorted, and pinned by SHA-256 of worker-owned bytes', async () => {
  const wav = Uint8Array.from([0, 10, 255]);
  const pinned = await pinRenderAssetBytes([
    { kind: 'sample-wav', key: 'voice/a.wav', bytes: wav },
    { kind: 'oto', key: 'voice/oto.ini', bytes: new TextEncoder().encode('oto') },
    { kind: 'project-media', key: 'asset:clip', bytes: new Uint8Array([4, 5]) },
  ], webcrypto);
  wav[0] = 99;
  assert.deepEqual(pinned.manifest.map(({ kind }) => kind), ['oto', 'project-media', 'sample-wav']);
  const sample = pinned.assets.find(asset => asset.kind === 'sample-wav');
  assert.deepEqual([...sample.bytes], [0, 10, 255]);
  assert.equal(sample.sha256, createHash('sha256').update(Buffer.from([0, 10, 255])).digest('hex'));
  assert.equal(pinned.transfer.length, 3);
  assert.equal(Object.isFrozen(pinned.manifest[0]), true);
});

test('render asset pinning rejects unsupported assets and missing WebCrypto', async () => {
  await assert.rejects(() => pinRenderAssetBytes([{ kind: 'path', key: 'x', bytes: new Uint8Array() }], webcrypto), /supported kind/);
  await assert.rejects(() => pinRenderAssetBytes([], null), /SHA-256 WebCrypto/);
});

test('render job fingerprint binds document revision, settings, and asset digests', async () => {
  const snapshot = { sessionId: 's', revision: 4, ustx: 'project', identitySidecar: 'ids', pronunciationSidecar: 'pron' };
  const assets = [{ kind: 'project-media', key: 'asset:a', byteLength: 2, sha256: 'a'.repeat(64) }];
  const first = await fingerprintRenderJob(snapshot, assets, { sampleRate: 44100 }, webcrypto);
  assert.equal(first, await fingerprintRenderJob(snapshot, assets, { sampleRate: 44100 }, webcrypto));
  assert.equal(first, await fingerprintRenderJob(snapshot, [...assets].reverse(), { sampleRate: 44100 }, webcrypto));
  assert.notEqual(first, await fingerprintRenderJob({ ...snapshot, revision: 5 }, assets, { sampleRate: 44100 }, webcrypto));
  assert.notEqual(first, await fingerprintRenderJob(snapshot, [{ ...assets[0], sha256: 'b'.repeat(64) }], { sampleRate: 44100 }, webcrypto));
});
