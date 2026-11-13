import test from 'node:test';
import assert from 'node:assert/strict';
import { WasmSessionRuntime } from '../src/openutau/wasm-session-runtime.js';

function audioOnlyRuntime() {
  const pcm = Float32Array.from([0.25, -0.1, -0.5, 0.75, 0.1, -0.3, 0.8, -0.2]);
  const calls = [];
  const driver = {
    engine: {},
    state: () => ({ snapshot: { project: { tracks: [{ parts: [{ kind: 'wave' }] }] } } }),
    renderTrack: track => { calls.push(track); return pcm; },
  };
  return { runtime: new WasmSessionRuntime(driver), calls, pcm };
}

test('audio-only waveform render skips bank setup and returns peak metadata only', async () => {
  const { runtime, calls, pcm } = audioOnlyRuntime();
  const result = await runtime.renderTrack(0, { waveformOnly: true });
  assert.deepEqual(calls, [0]);
  assert.equal(result.frames, 4);
  assert.equal(result.sampleRate, 44100);
  assert.equal(result.peaks.samples, 4);
  assert.equal(result.peaks.min[0], -0.5);
  assert.ok(Math.abs(result.peaks.max[0] - 0.8) < 1e-6);
  assert.equal(result.samples, undefined);
  assert.equal(result.channels, undefined);
  assert.equal(pcm.length, 8);
});

test('regular track render still returns stereo channels for playback', async () => {
  const { runtime } = audioOnlyRuntime();
  const result = await runtime.renderTrack(0);
  assert.equal(result.samples.length, 4);
  assert.equal(result.channels[0].length, 4);
  assert.equal(result.channels[1].length, 4);
  assert.equal(result.samples[1], 0.125);
});
