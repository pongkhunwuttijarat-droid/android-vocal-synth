// The pitch-curve editing rules, away from the canvas.
//
// The units are the awkward part, so they are pinned here: x is milliseconds
// from the note start (not ticks) and y is tenths of a semitone relative to the
// note's tone (not cents, not semitones). Everything the roll draws and every
// gesture it turns into a command goes through these functions, so a mistake
// here is a mistake in what gets stored in the engine.
import test from 'node:test';
import assert from 'node:assert/strict';
import {
  DEFAULT_PITCH_SHAPE, PITCH_HIT_RADIUS_PX, PITCH_MAX_POINTS, PITCH_MAX_TENTHS, PITCH_MIN_POINTS,
  PITCH_SAMPLE_TICKS, PITCH_SHAPES, clampTenths, cyclePitchShape, flatPitch, isPitchAnchor, movePitchPoint,
  normalizePitch, paintPitchPoints, pitchChanged, pitchForDisplay, pitchOf, pixelsToTenths, removePitchPoint, samplePitch,
  tenthsToPixels, ticksToMs,
} from '../src/openutau/pitch-curve-input.js';

const BPM = 120;
/** One beat of notes at 120 BPM: 480 ticks is 500 ms. */
const BEAT = { duration_ticks: 480 };
const HALF_BEAT_MS = 250;

test('ticks and milliseconds convert both ways at the project tempo', () => {
  assert.equal(ticksToMs(480, 120), 500);
  assert.equal(ticksToMs(480, 60), 1000);
  assert.equal(ticksToMs(240, 120), 250);
});

test('a tenth of a semitone is a tenth of a row in the roll', () => {
  // ROW is 22px, so ten tenths of a semitone is one row.
  assert.equal(tenthsToPixels(10, 2.2), 22);
  assert.equal(pixelsToTenths(22, 2.2), 10);
  assert.equal(pixelsToTenths(2.2, 2.2), 1);
});

test('a note with no curve reads as flat across its whole length', () => {
  const curve = pitchOf({ duration_ticks: 480 }, BPM);
  assert.deepEqual(curve.data, [{ x: 0, y: 0, shape: 'io' }, { x: 500, y: 0, shape: 'io' }]);
  assert.equal(curve.snap_first, true);
  assert.equal(samplePitch(curve, 250), 0);
});

test('a stored curve is read back in the engine units, not converted', () => {
  // The wire form is already ms/tenths, so `pitchOf` only repairs a missing or
  // unknown shape — converting here would double-apply the scale.
  const curve = pitchOf({ duration_ticks: 480, pitch: { data: [{ x: 0, y: -12 }, { x: 300, y: 40, shape: 'bogus' }, { x: 500, y: 0 }], snap_first: false } }, BPM);
  assert.deepEqual(curve.data, [{ x: 0, y: -12, shape: 'io' }, { x: 300, y: 40, shape: 'io' }, { x: 500, y: 0, shape: 'io' }]);
  assert.equal(curve.snap_first, false);
});

test('display pitch retains pre-onset points used by renderer interpolation', () => {
  const note = { duration_ticks: 480, pitch: { snap_first: true, data: [
    { x: -27, y: 50, shape: 'io' }, { x: 20, y: 0, shape: 'io' },
  ] } };
  const display = pitchForDisplay(note, 240);
  assert.deepEqual(display.data.map(point => point.x), [-27, 20]);
  // `feed::PitchComputer` samples the raw curve at the note start; this matches
  // its io interpolation instead of the +50-tenths value from a clamped curve.
  assert.ok(Math.abs(samplePitch(display, 0) - 19.2) < 0.05);
  assert.equal(samplePitch(pitchOf(note, 240), 0), 50);
});

test('a painted stroke is anchored to the note ends', () => {
  const curve = paintPitchPoints(flatPitch(BEAT, BPM), [
    { ms: 100, tenths: 0 }, { ms: 250, tenths: 30 }, { ms: 400, tenths: -10 },
  ], { durationMs: 500, shape: 'l' });
  assert.deepEqual(curve.data.map(p => p.x), [0, 100, 250, 400, 500]);
  // The ends stay flat because the pointer never went there, which is what
  // `feed::PitchComputer` completes them to as well.
  assert.deepEqual(curve.data.map(p => p.y), [0, 0, 30, -10, -10]);
  // The drawn points carry the drawn shape; the anchors keep the shape the
  // renderer completes them with, `PhrasePitchPoint { shape: Io }`.
  assert.deepEqual(curve.data.map(p => p.shape), ['io', 'l', 'l', 'l', 'io']);
});

test('a stroke that doubles back over itself keeps the later sample', () => {
  // Pointer order is not time order, so a curve must be sorted before x can be
  // strictly increasing — `SetNotePitchCommand` rejects a curve that is not.
  const curve = paintPitchPoints(flatPitch(BEAT, BPM), [{ ms: 400, tenths: 20 }, { ms: 100, tenths: -20 }, { ms: 250, tenths: 0 }], { durationMs: 500 });
  // The ends hold the first and last painted value, which is what the renderer
  // completes them to (`feed/src/pitch.rs`: `y: points[0].y` / the last point's
  // y), so the line drawn here is the line that will be rendered.
  assert.deepEqual(curve.data.map(p => [p.x, p.y]), [[0, -20], [100, -20], [250, 0], [400, 20], [500, 20]]);
});

test('a stroke that only starts and ends outside the note leaves it flat', () => {
  const curve = paintPitchPoints(flatPitch(BEAT, BPM), [{ ms: -50, tenths: 5 }, { ms: 900, tenths: -5 }], { durationMs: 500 });
  assert.deepEqual(curve.data.map(p => [p.x, p.y]), [[0, 0], [500, 0]]);
});

test('normalizePitch repairs a curve that stops short of the note end', () => {
  // What the renderer would do with such a curve: complete it. Doing it here
  // means the editor never stores one it would have to draw back.
  const fixed = normalizePitch({ data: [{ x: 0, y: 5, shape: 'l' }, { x: 200, y: 9, shape: 'l' }] }, { durationMs: 500 });
  assert.deepEqual(fixed.data.map(p => [p.x, p.y]), [[0, 5], [200, 9], [500, 9]]);
});

test('normalizePitch drops a duplicate x and sorts an unordered curve', () => {
  const fixed = normalizePitch({ data: [
    { x: 300, y: 2, shape: 'io' }, { x: 0, y: 1, shape: 'io' }, { x: 300, y: 8, shape: 'io' }, { x: 500, y: 0, shape: 'io' },
  ] }, { durationMs: 500 });
  assert.deepEqual(fixed.data.map(p => [p.x, p.y]), [[0, 1], [300, 2], [500, 0]]);
});

test('normalizePitch keeps the portamento range and the snap flag', () => {
  const fixed = normalizePitch({ data: [{ x: 0, y: 9999, shape: 'io' }, { x: 500, y: -9999, shape: 'io' }], snap_first: false }, { durationMs: 500 });
  assert.deepEqual(fixed.data.map(p => p.y), [PITCH_MAX_TENTHS, -PITCH_MAX_TENTHS]);
  assert.equal(fixed.snap_first, false);
});

test('a curve with one point still spans the note by a gap the engine can see', () => {
  // The second point has to be far enough from the first to survive the f32
  // round trip, or the command is refused for "x must strictly increase".
  const fixed = normalizePitch({ data: [{ x: 0, y: 7, shape: 'io' }] }, { durationMs: 500 });
  assert.equal(fixed.data.length, 2);
  assert.notEqual(Math.fround(fixed.data[0].x), Math.fround(fixed.data[1].x));
});

test('painting inserts between the anchors and overwrites the point it lands on', () => {
  const flat = flatPitch(BEAT, BPM);
  const painted = paintPitchPoints(flat, [{ ms: 250, tenths: 20 }], { shape: 'l', durationMs: 500 });
  assert.deepEqual(painted.data, [
    { x: 0, y: 20, shape: 'io' }, { x: 250, y: 20, shape: 'l' }, { x: 500, y: 20, shape: 'io' },
  ]);
  // Dragging back over the same spot moves the point instead of stacking one.
  const again = paintPitchPoints(painted, [{ ms: 250, tenths: -8 }], { shape: 'l', durationMs: 500 });
  assert.equal(again.data.length, 3);
  assert.equal(again.data[1].y, -8);
});

test('painting keeps the curve sorted no matter which way the pointer moves', () => {
  const flat = flatPitch(BEAT, BPM);
  const painted = paintPitchPoints(flat, [{ ms: 400, tenths: 30 }, { ms: 100, tenths: -30 }, { ms: 250, tenths: 0 }], { durationMs: 500 });
  assert.deepEqual(painted.data.map(p => p.x), [0, 100, 250, 400, 500]);
  assert.deepEqual(painted.data.map(p => p.y), [-30, -30, 0, 30, 30]);
});

test('painting never writes outside the two anchors', () => {
  const flat = flatPitch(BEAT, BPM);
  const painted = paintPitchPoints(flat, [{ ms: -40, tenths: 5 }, { ms: 900, tenths: 7 }, { ms: 250, tenths: 1 }], { durationMs: 500 });
  // A sample outside the note is dropped rather than clamped onto the nearest
  // anchor: a stroke that wandered off used to flatten the note's end.
  assert.deepEqual(painted.data.map(p => p.x), [0, 250, 500]);
  assert.deepEqual(painted.data.map(p => p.y), [1, 1, 1]);
});

test('a point exactly on a note end still moves that end', () => {
  const flat = flatPitch(BEAT, BPM);
  // Painting on an anchor is the user's own value for that end, so the
  // end-follows-the-stroke rule does not then overwrite it with its neighbour.
  const painted = paintPitchPoints(flat, [{ ms: 0, tenths: 6 }, { ms: 250, tenths: 2 }, { ms: 500, tenths: -6 }], { durationMs: 500 });
  assert.deepEqual(painted.data.map(p => [p.x, p.y]), [[0, 6], [250, 2], [500, -6]]);
});

test('painting stops inserting at the point ceiling but still moves points', () => {
  // A cap that a normal stroke can never reach is decoration; this one has to be
  // demonstrably reachable, so fill until it stops growing.
  let curve = flatPitch(BEAT, BPM);
  for (let ms = 0.2; ms < 500; ms += 0.2) curve = paintPitchPoints(curve, [{ ms, tenths: 1 }], { durationMs: 500 });
  assert.equal(curve.data.length, PITCH_MAX_POINTS);
  const moved = paintPitchPoints(curve, [{ ms: 250, tenths: -40 }], { durationMs: 500 });
  assert.equal(moved.data.length, PITCH_MAX_POINTS);
  assert.equal(moved.data.find(p => Math.abs(p.x - 250) < 0.01).y, -40);
});

test('a point drag is clamped between its neighbours so x stays increasing', () => {
  const curve = { data: [{ x: 0, y: 0, shape: 'io' }, { x: 200, y: 10, shape: 'io' }, { x: 300, y: 20, shape: 'io' }, { x: 500, y: 0, shape: 'io' }] };
  assert.equal(movePitchPoint(curve, 1, { ms: 250, tenths: 15 }, { durationMs: 500 }).data[1].x, 250);
  // Dragged left past its left neighbour and right past its right one: it stops
  // just inside them, because a shared x is not a valid span.
  const left = movePitchPoint(curve, 1, { ms: -900, tenths: 15 }, { durationMs: 500 }).data[1].x;
  assert.ok(left > 0 && left < 300, `clamped to ${left}`);
  const right = movePitchPoint(curve, 1, { ms: 900, tenths: 15 }, { durationMs: 500 }).data[1].x;
  assert.ok(right < 300 && right > 299.9, `clamped to ${right}`);
  assert.equal(movePitchPoint(curve, 1, { ms: 250, tenths: 9999 }, { durationMs: 500 }).data[1].y, PITCH_MAX_TENTHS);
});

test('the anchors cannot be moved sideways, only up and down', () => {
  const curve = { data: [{ x: 0, y: 0, shape: 'io' }, { x: 500, y: 0, shape: 'io' }] };
  const first = movePitchPoint(curve, 0, { ms: 200, tenths: 12 }, { durationMs: 500 });
  assert.deepEqual(first.data.map(p => [p.x, p.y]), [[0, 12], [500, 0]]);
  const last = movePitchPoint(curve, 1, { ms: 200, tenths: -12 }, { durationMs: 500 });
  assert.deepEqual(last.data.map(p => [p.x, p.y]), [[0, 0], [500, -12]]);
});

test('a shorter note keeps the curve spanning it after an anchor drag', () => {
  // The stored curve is in ms, so a resize changes the last point's x. Dragging
  // an anchor is what re-pins it.
  const curve = { data: [{ x: 0, y: 0, shape: 'io' }, { x: 200, y: 10, shape: 'io' }, { x: 500, y: 5, shape: 'io' }] };
  const resized = movePitchPoint(curve, 2, { ms: 500, tenths: 5 }, { durationMs: 250 });
  assert.deepEqual(resized.data.map(p => p.x), [0, 200, 250]);
});

test('deleting refuses the anchors and the last pair', () => {
  const curve = { data: [{ x: 0, y: 0, shape: 'io' }, { x: 200, y: 10, shape: 'io' }, { x: 500, y: 5, shape: 'io' }] };
  assert.equal(removePitchPoint(curve, 0), curve);
  assert.equal(removePitchPoint(curve, 2), curve);
  assert.deepEqual(removePitchPoint(curve, 1).data.map(p => p.x), [0, 500]);
  const pair = { data: [{ x: 0, y: 0, shape: 'io' }, { x: 500, y: 0, shape: 'io' }] };
  // A one-span curve has nothing left to interpolate; that is what clear is for.
  assert.equal(removePitchPoint(pair, 0).data.length, PITCH_MIN_POINTS);
  assert.equal(removePitchPoint(pair, 1).data.length, PITCH_MIN_POINTS);
});

test('isPitchAnchor covers exactly the two ends', () => {
  const curve = { data: [{ x: 0 }, { x: 1 }, { x: 2 }] };
  assert.deepEqual([0, 1, 2].map(i => isPitchAnchor(curve, i)), [true, false, true]);
});

test('right-click cycles one point through every shape', () => {
  let curve = { data: [{ x: 0, y: 0, shape: 'io' }, { x: 250, y: 5, shape: 'io' }, { x: 500, y: 0, shape: 'io' }] };
  const seen = [curve.data[1].shape];
  for (let step = 0; step < PITCH_SHAPES.length; step += 1) {
    curve = cyclePitchShape(curve, 1);
    seen.push(curve.data[1].shape);
  }
  assert.deepEqual(seen, PITCH_SHAPES.map(entry => entry.id).concat(DEFAULT_PITCH_SHAPE));
  // The anchors are not points the user owns, so they keep their shape.
  assert.equal(cyclePitchShape(curve, 0).data[0].shape, 'io');
});

test('the freeform sampling grid is OpenUtau\'s 5 ticks, in ms from the note start', () => {
  // `DrawPitchState.Update` rounds the ABSOLUTE tick to a multiple of 5, and
  // the point it stores is ms from the NOTE start — so the grid and the stored
  // units are two different things and the conversion has to hold.
  assert.equal(PITCH_SAMPLE_TICKS, 5);
  const note = { position_ticks: 480, duration_ticks: 480 };
  assert.equal(Math.round(605 / 5) * 5, 605);
  // 125 ticks is 125/480 of a beat, and a beat at 120 BPM is 500 ms.
  assert.equal(ticksToMs(605 - note.position_ticks, 120), 125 / 480 * 500);
  // A note that does not start on the grid still gets points a whole number of
  // grid slots apart, because the rounding is on the absolute tick.
  const offGrid = { position_ticks: 483, duration_ticks: 480 };
  assert.equal(Math.round(600 / 5) * 5 - offGrid.position_ticks, 117);
  assert.equal(ticksToMs(117, 120), 117 / 480 * 500);
});

test('a curve that arrived from a file is completed before it is drawn', () => {
  // The demo song stores `x: -40 … 40` as a placeholder curve. `feed` completes
  // both ends of whatever it is handed, so the roll must show the completed
  // curve too — and, more importantly, painting has to have anchors at 0 and at
  // the note duration to insert between.
  const stored = { duration_ticks: 480, pitch: { data: [{ x: -40, y: 0, shape: 'io' }, { x: 40, y: 0, shape: 'io' }] } };
  const read = pitchOf(stored, BPM);
  assert.deepEqual(read.data.map(p => [p.x, p.y]), [[0, 0], [40, 0], [500, 0]]);
  const painted = paintPitchPoints(read, [{ ms: 250, tenths: 20 }], { shape: 'l', durationMs: 500 });
  // The end follows the stroke; the start still holds the placeholder point at
  // 40ms, which is nearer to it than the painted one.
  assert.deepEqual(painted.data.map(p => [p.x, p.y]), [[0, 0], [40, 0], [250, 20], [500, 20]]);
});

test('painting onto a curve with no room still records the stroke', () => {
  // The failure this pins: every sample landed outside the two anchors and was
  // dropped, so the gesture sent the same curve it started with and the stroke
  // did nothing at all.
  const degenerate = { data: [{ x: -40, y: 0, shape: 'io' }, { x: 40, y: 0, shape: 'io' }] };
  const painted = paintPitchPoints(degenerate, [{ ms: 100, tenths: 8 }, { ms: 400, tenths: -8 }], { shape: 'io', durationMs: 500 });
  assert.deepEqual(painted.data.map(p => [p.x, p.y]), [[0, 0], [40, 0], [100, 8], [400, -8], [500, -8]]);
  assert.equal(pitchChanged(degenerate, painted), true);
});

test('a drawn point is never before the note start', () => {
  // The command refuses a negative x, so nothing the gesture builds may contain
  // one even when the curve it started from did.
  const painted = paintPitchPoints({ data: [{ x: -40, y: 0, shape: 'io' }, { x: 40, y: 0, shape: 'io' }] },
    [{ ms: -999, tenths: 4 }, { ms: 200, tenths: 2 }], { durationMs: 500 });
  assert.ok(painted.data.every(p => p.x >= 0), JSON.stringify(painted.data));
  assert.ok(painted.data.every((p, i) => i === 0 || p.x > painted.data[i - 1].x), JSON.stringify(painted.data));
});

test('every x this module builds is an f32, exactly as the engine stores it', () => {
  // `domain::PitchPoint.x` is an f32 and `SetNotePitchCommand` compares the
  // stored values. A curve assembled from f64 arithmetic can hold two points a
  // fraction of an ulp apart that are the SAME f32, and the engine then refuses
  // the whole curve with "x values must strictly increase" — while both points
  // are visibly separate and a repaint in the same place reproduces it. Working
  // in f32 makes the two sides the same number, so equality IS the engine's rule.
  const painted = paintPitchPoints(flatPitch(BEAT, BPM), [
    { ms: 100, tenths: 1 }, { ms: 100.0000005, tenths: 2 }, { ms: 400, tenths: 3 },
  ], { durationMs: 500 });
  for (const point of painted.data) assert.equal(point.x, Math.fround(point.x), `${point.x} is not an f32`);
  const xs = painted.data.map(p => p.x);
  for (let i = 1; i < xs.length; i += 1) {
    assert.ok(xs[i] > xs[i - 1], `${xs[i - 1]} then ${xs[i]} is not strictly increasing as f32`);
  }
  // Two samples that ARE the same f32 are one point, not a zero-width span.
  const merged = paintPitchPoints(flatPitch(BEAT, BPM), [{ ms: 250, tenths: 5 }, { ms: Math.fround(250) + 1e-9, tenths: 9 }], { durationMs: 500 });
  assert.equal(merged.data.length, 3);
  assert.equal(merged.data.find(p => Math.abs(p.x - 250) < 0.01).y, 9);
});

test('a point dragged onto its neighbour still leaves the engine a gap', () => {
  const curve = { data: [{ x: 0, y: 0, shape: 'io' }, { x: 200, y: 10, shape: 'io' }, { x: 500, y: 0, shape: 'io' }] };
  for (const target of [200, 200.0000001, 200.001, 1e9, -1e9, 0, 500]) {
    const moved = movePitchPoint(curve, 1, { ms: target, tenths: 4 }, { durationMs: 500 });
    const [before, after] = [moved.data[0].x, moved.data[2].x];
    const x = moved.data[1].x;
    assert.equal(x, Math.fround(x));
    assert.ok(x > before && x < after, `dragged to ${target} → ${x}, outside (${before}, ${after})`);
  }
});

test('the preview sampler matches feed::music_math easing', () => {
  // sin_easing_in_out / in / out / linear from `feed/src/music_math.rs`, with
  // a span of 10 and the sample at x = 3: y0 = 0, y1 = 100.
  const span = shape => ({ data: [{ x: 0, y: 0, shape }, { x: 10, y: 100, shape }] });
  const close = (actual, expected) => assert.ok(Math.abs(actual - expected) < 1e-9, `${actual} != ${expected}`);
  close(samplePitch(span('io'), 3), 100 * (1 - Math.cos(0.3 * Math.PI)) / 2);
  close(samplePitch(span('i'), 3), 100 * (1 - Math.cos(0.3 * Math.PI / 2)));
  close(samplePitch(span('o'), 3), 100 * Math.sin(0.3 * Math.PI / 2));
  close(samplePitch(span('l'), 3), 30);
  // Outside the curve the end values hold, which is the fallback the renderer
  // relies on for a note whose curve does not cover its whole length.
  assert.equal(samplePitch(span('l'), -5), 0);
  assert.equal(samplePitch(span('l'), 50), 100);
  // A span shorter than music_math's `EP` renders as its end value.
  assert.equal(samplePitch({ data: [{ x: 0, y: 0, shape: 'l' }, { x: 0.0005, y: 100, shape: 'l' }] }, 0.0002), 100);
});

test('the spline preview passes through its own points', () => {
  const curve = { data: [
    { x: 0, y: 0, shape: 'sp' }, { x: 100, y: 40, shape: 'sp' }, { x: 200, y: -20, shape: 'sp' }, { x: 300, y: 10, shape: 'sp' },
  ] };
  for (const point of curve.data) assert.ok(Math.abs(samplePitch(curve, point.x) - point.y) < 1e-9);
});

test('clampTenths holds the portamento range', () => {
  assert.equal(clampTenths(1000), PITCH_MAX_TENTHS);
  assert.equal(clampTenths(-1000), -PITCH_MAX_TENTHS);
  assert.equal(clampTenths(12), 12);
  // The grab radius has to stay near the drawn circle, or a press meant to paint
  // a point grabs whatever a file left near the note's edge instead.
  assert.ok(PITCH_HIT_RADIUS_PX <= 5, `grab radius ${PITCH_HIT_RADIUS_PX}px is too greedy to paint`);
});

test('pitchChanged compares points and the snap flag, not object identity', () => {
  const curve = { data: [{ x: 0, y: 0, shape: 'io' }, { x: 500, y: 0, shape: 'io' }], snap_first: true };
  assert.equal(pitchChanged(curve, { data: [{ x: 0, y: 0, shape: 'io' }, { x: 500, y: 0, shape: 'io' }], snap_first: true }), false);
  assert.equal(pitchChanged(curve, { ...curve, snap_first: false }), true);
  assert.equal(pitchChanged(curve, { ...curve, data: [{ x: 0, y: 1, shape: 'io' }, { x: 500, y: 0, shape: 'io' }] }), true);
  assert.equal(pitchChanged(null, curve), true);
});

test('half a beat of note is the reference length the roll draws against', () => {
  // Guards the constant the drawing code and the gesture math both assume: the
  // stored x is the note's full duration in ms, not a fraction of a beat.
  assert.equal(ticksToMs(BEAT.duration_ticks, BPM), 500);
  assert.equal(HALF_BEAT_MS, 250);
});