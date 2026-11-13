import test from 'node:test';
import assert from 'node:assert/strict';
import { buildInterleavedStereoPeaks, buildPeaks } from '../src/openutau/pcm-analysis.js';

test('interleaved stereo peak analysis matches channel-based envelope without channel copies', () => {
  const left = Float32Array.from([0.25, -0.5, 0.1, 0.8, -0.2]);
  const right = Float32Array.from([-0.1, 0.75, -0.3, -0.2, 0.4]);
  const interleaved = new Float32Array(left.length * 2);
  for (let frame = 0; frame < left.length; frame++) {
    interleaved[frame * 2] = left[frame];
    interleaved[frame * 2 + 1] = right[frame];
  }

  const actual = buildInterleavedStereoPeaks(interleaved, 48000, 2);
  const expected = buildPeaks([left, right], 48000, 2);
  assert.deepEqual(Array.from(actual.min), Array.from(expected.min));
  assert.deepEqual(Array.from(actual.max), Array.from(expected.max));
  assert.equal(actual.samples, left.length);
  assert.equal(actual.sampleRate, 48000);
  assert.equal(actual.bucket, 2);
});

test('interleaved peak analysis rejects incomplete stereo frames', () => {
  assert.throws(() => buildInterleavedStereoPeaks(new Float32Array([0.25]), 44100), /complete frames/);
});
