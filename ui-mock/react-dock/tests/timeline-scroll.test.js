import test from 'node:test';
import assert from 'node:assert/strict';
import {
  normalizeTimelineScrollTicks,
  scrollOffsetToTicks,
  ticksToScrollOffset,
} from '../src/openutau/timeline-scroll.js';

const TICKS_PER_MEASURE = 1920;
const TICKS_PER_BEAT = 480;

test('Arrangement and Piano Roll pixels map to the same project tick', () => {
  const arrangementPixelsPerTick = 72 / TICKS_PER_MEASURE;
  const pianoRollPixelsPerTick = 68 / TICKS_PER_BEAT;
  const projectTick = 205200;

  const arrangementX = ticksToScrollOffset(projectTick, arrangementPixelsPerTick);
  const rollX = ticksToScrollOffset(projectTick, pianoRollPixelsPerTick);
  assert.equal(scrollOffsetToTicks(arrangementX, arrangementPixelsPerTick), projectTick);
  assert.equal(scrollOffsetToTicks(rollX, pianoRollPixelsPerTick), projectTick);
  assert.notEqual(arrangementX, rollX);
});

test('the same timeline tick stays fixed when a pane zooms', () => {
  const tick = TICKS_PER_MEASURE * 37 + 240;
  const before = ticksToScrollOffset(tick, 72 / TICKS_PER_MEASURE);
  const after = ticksToScrollOffset(tick, 144 / TICKS_PER_MEASURE);
  assert.equal(scrollOffsetToTicks(before, 72 / TICKS_PER_MEASURE), tick);
  assert.equal(scrollOffsetToTicks(after, 144 / TICKS_PER_MEASURE), tick);
  assert.equal(after, before * 2);
});

test('invalid, negative and fractional shared positions are normalized', () => {
  assert.equal(normalizeTimelineScrollTicks(-1), 0);
  assert.equal(normalizeTimelineScrollTicks(12.6), 13);
  assert.equal(normalizeTimelineScrollTicks(Number.NaN), 0);
  assert.equal(scrollOffsetToTicks(-10, 0.1), 0);
  assert.equal(scrollOffsetToTicks(10, 0), 0);
  assert.equal(ticksToScrollOffset(Number.POSITIVE_INFINITY, 1), 0);
});
