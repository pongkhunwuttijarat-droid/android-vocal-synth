import test from 'node:test';
import assert from 'node:assert/strict';
import { TICKS_PER_MEASURE, TRACKS } from '../src/openutau/mock-data.js';
import { projectTicks, transportTarget } from '../src/openutau/transport-state.js';

test('live project transport is not clamped to the mock 14-measure timeline', () => {
  const projectTracks = [{ parts: [{ position_ticks: 0, duration_ticks: 553712 }] }];
  const project = { tracks: projectTracks, notes: {} };
  const uiState = { tracks: TRACKS, notes: {}, playheadTicks: TICKS_PER_MEASURE * 14 };
  const totalTicks = projectTicks(projectTracks);
  const beyond28s = 29.5 * 240 / 60 * 480;

  assert.equal(projectTicks(TRACKS), TICKS_PER_MEASURE * 14);
  assert.equal(totalTicks, 553712);

  const target = transportTarget(uiState, { type: 'transport/seek', ticks: beyond28s }, project);
  assert.equal(target, beyond28s);

  // The reducer receives only the resolved seek and bounds; it must preserve the
  // live target even though its own presentation state still contains mock tracks.
  assert.equal(transportTarget(uiState, {
    type: 'transport/seek',
    ticks: beyond28s,
    totalTicks,
    targetTicks: target,
  }), beyond28s);
});
