import test from 'node:test';
import assert from 'node:assert/strict';
import { encodeStereoPcm16Wav } from '../src/openutau/pcm-wav.js';

test('encodes stereo float channels as a valid PCM16 RIFF/WAVE', () => {
  const bytes = encodeStereoPcm16Wav([
    Float32Array.from([-1, 0, 0.5, 1]),
    Float32Array.from([1, -0.5, 0, -1]),
  ], 44100);
  const view = new DataView(bytes);
  const tag = offset => String.fromCharCode(...new Uint8Array(bytes, offset, 4));

  assert.equal(tag(0), 'RIFF');
  assert.equal(view.getUint32(4, true), bytes.byteLength - 8);
  assert.equal(tag(8), 'WAVE');
  assert.equal(tag(12), 'fmt ');
  assert.equal(view.getUint16(20, true), 1);
  assert.equal(view.getUint16(22, true), 2);
  assert.equal(view.getUint32(24, true), 44100);
  assert.equal(view.getUint16(34, true), 16);
  assert.equal(tag(36), 'data');
  assert.equal(view.getUint32(40, true), 16);
  assert.deepEqual(
    Array.from({ length: 8 }, (_, index) => view.getInt16(44 + index * 2, true)),
    [-32768, 32767, 0, -16384, 16384, 0, 32767, -32768],
  );
});

test('rejects non-stereo or mismatched render channels', () => {
  assert.throws(() => encodeStereoPcm16Wav([Float32Array.of(0)], 44100), /stereo audio/);
  assert.throws(() => encodeStereoPcm16Wav([Float32Array.of(0), Float32Array.of(0, 1)], 44100), /different frame counts/);
  assert.throws(() => encodeStereoPcm16Wav([Float32Array.of(0), Float32Array.of(0)], 0), /invalid sample rate/);
});
