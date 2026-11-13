import test from 'node:test';
import assert from 'node:assert/strict';
import { audioWaveformCacheKey } from '../src/openutau/audio-waveform-cache.js';

const audioTrack = () => ({
  id: 'audio-track',
  volume_db: 0,
  pan: 0,
  parts: [{ id: 'wave-part', kind: 'wave', media_ref: 'asset:take-a', position_ticks: 0, duration_ticks: 960 }],
});

const voiceTrack = () => ({
  id: 'voice-track',
  parts: [{ id: 'voice-part', kind: 'voice', notes: [{ tone: 60, position_ticks: 0 }] }],
});

test('unrelated voice-note edits do not invalidate cached audio waveforms', () => {
  const before = [voiceTrack(), audioTrack()];
  const after = [voiceTrack(), audioTrack()];
  after[0].parts[0].notes[0].tone = 62;
  assert.equal(audioWaveformCacheKey('session-1', before), audioWaveformCacheKey('session-1', after));
});

test('audio media changes and new sessions invalidate cached waveform peaks', () => {
  const baseline = [voiceTrack(), audioTrack()];
  const relinked = [voiceTrack(), audioTrack()];
  relinked[1].parts[0].media_ref = 'asset:take-b';
  assert.notEqual(audioWaveformCacheKey('session-1', baseline), audioWaveformCacheKey('session-1', relinked));
  assert.notEqual(audioWaveformCacheKey('session-1', baseline), audioWaveformCacheKey('session-2', baseline));
});
