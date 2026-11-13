// The envelope a painted gesture becomes before it crosses into Rust. It must
// be canonical — the same points `commands::canonicalize` would produce — and
// its interval must be the painted span, because that is what decides whether
// the rest of the phrase keeps its shape.
import test from 'node:test';
import assert from 'node:assert/strict';
import { STROKE_TICK_INTERVAL, simplifyTolerance, strokeEnvelope, strokeSummary } from '../src/openutau/curve-stroke-input.js';

const DYN = { abbr: 'dyn', min: -240, max: 120, default: 0 };
const PART = 480 * 8;

test('samples snap to the 5-tick grid the engine stores', () => {
  // Half away from zero, like Rust's `csharp_round`: 7 → 5, 8 → 10, -7 → -5.
  const envelope = strokeEnvelope([{ tick: 7, value: 0 }, { tick: 8, value: 40 }], DYN, PART);
  assert.deepEqual(envelope.points, [{ tick: 5, value: 0 }, { tick: 10, value: 40 }]);
  assert.equal(STROKE_TICK_INTERVAL, 5);
});

test('a drag back over itself keeps the later sample, not the first', () => {
  // Pointer order is not tick order. Rust sorts by (raw tick, arrival index),
  // so 900 then 500 must land as 500 → 900 with both values intact.
  const envelope = strokeEnvelope([{ tick: 900, value: 20 }, { tick: 500, value: -30 }], DYN, PART);
  assert.deepEqual(envelope.points, [{ tick: 500, value: -30 }, { tick: 900, value: 20 }]);
  assert.equal(envelope.startTick, 500);
  assert.equal(envelope.endTick, 900);
});

test('two samples that snap together collapse, last arrival wins', () => {
  // 100 is already on the grid; 103 and 104 both land on 105.
  const envelope = strokeEnvelope([
    { tick: 100, value: 10 }, { tick: 103, value: 60 }, { tick: 104, value: -5 },
  ], DYN, PART);
  assert.deepEqual(envelope.points, [{ tick: 100, value: 10 }, { tick: 105, value: -5 }]);
});

test('values clamp to the descriptor range and round to integers', () => {
  const envelope = strokeEnvelope([
    { tick: 0, value: -900 }, { tick: 480, value: 44.6 }, { tick: 960, value: 900 },
  ], DYN, PART);
  assert.deepEqual(envelope.points, [
    { tick: 0, value: -240 }, { tick: 480, value: 45 }, { tick: 960, value: 120 },
  ]);
});

test('the interval is the painted span, never the whole part', () => {
  // This is the regression that flattened curve ends: a stroke whose interval
  // ran 0..partTicks replaced the entire curve, so `delete_between_exclusive`
  // wiped everything and the boundary anchors came from the default.
  const envelope = strokeEnvelope([{ tick: 1440, value: 30 }, { tick: 1920, value: 60 }], DYN, PART);
  assert.equal(envelope.startTick, 1440);
  assert.equal(envelope.endTick, 1920);
  assert.ok(envelope.endTick < PART);
});

test('a stroke cannot leave the part', () => {
  const envelope = strokeEnvelope([{ tick: -240, value: 10 }, { tick: PART + 960, value: 20 }], DYN, PART);
  assert.equal(envelope.startTick, 0);
  assert.equal(envelope.endTick, PART);
});

test('an empty or absent gesture produces no command', () => {
  assert.equal(strokeEnvelope([], DYN, PART), null);
  assert.equal(strokeEnvelope(null, DYN, PART), null);
  assert.deepEqual(strokeSummary(null), { points: 0, bytes: 2 });
});

test('the reported tolerance is the one Rust simplifies with', () => {
  // min(5, (max - min) * 0.005): dyn's 360-wide range gives 1.8, a wide
  // descriptor like pitd (±1200) is capped at 5.
  assert.equal(simplifyTolerance(DYN), 1.8);
  assert.equal(simplifyTolerance({ min: -1200, max: 1200 }), 5);
  assert.equal(simplifyTolerance({ min: 0, max: 100 }), 0.5);
});