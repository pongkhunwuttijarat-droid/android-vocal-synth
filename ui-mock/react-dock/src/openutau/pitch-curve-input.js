// The OpenUtau pitch-curve model, as the editor needs it.
//
// A pitch curve is PER NOTE and its units are unusual, which is why this is a
// separate module rather than a few lines in the roll:
//
//   x — **milliseconds from the note start**, not ticks (`UNote.PitchPoint.X`)
//   y — **tenths of a semitone relative to the note's tone**, not cents and not
//       semitones (`UNote.PitchPoint.Y`; `UPitch.Sample` multiplies by 10)
//   shape — the interpolation into the NEXT point: `io` sine in-out (the
//       default), `l` linear, `i` sine in, `o` sine out, `sp` spline
//
// `domain::PitchPoint` and the USTX codec already use exactly these units, so
// nothing is converted on the wire — but the roll draws in ticks and rows, so
// the conversion happens here.
//
// **The curve is completed, never partial.** `feed::PitchComputer` reads
// `note.pitch.data` directly and only fills in the two ends (`x: -1` / `x: 1`
// in the fixtures) when a note has no curve at all. A curve that stops short
// of the note end would be sampled as flat after its last point, so every point
// list this module produces starts at 0 ms and ends at the note's duration.
// That is what `UPitch.Sample`'s fallbacks at both ends expect too.
import { TICKS_PER_BEAT } from './mock-data.js';

/** OpenUtau's `PitchPointShape`, in the order the shape picker shows them. */
export const PITCH_SHAPES = [
  { id: 'io', name: 'Sine in-out', hint: 'smooth both ends — the default' },
  { id: 'l', name: 'Linear', hint: 'a straight line between points' },
  { id: 'i', name: 'Sine in', hint: 'ease into the next point' },
  { id: 'o', name: 'Sine out', hint: 'ease out of this point' },
  { id: 'sp', name: 'Spline', hint: 'cubic; needs three or more points' },
];
export const DEFAULT_PITCH_SHAPE = 'io';

const isShape = shape => PITCH_SHAPES.some(entry => entry.id === shape);

/**
 * The grid a freeform pitch stroke is sampled on: 5 ticks, a 1/96 note at 480
 * ticks per beat.
 *
 * This is OpenUtau's own density. `DrawPitchState.Update` (`desktop/OpenUtau/
 * Views/NoteEditStates.cs`) rounds the pointer's tick before it samples anything:
 * `TickToneToPoint((int)Math.Round(tick / 5.0) * 5, …)`. The grid is on the
 * ABSOLUTE tick, so a note that does not start on the grid gets points at a
 * constant offset from it — which is what the stored `x` values mean too, since
 * they are milliseconds from the note start.
 */
export const PITCH_SAMPLE_TICKS = 5;
/** One semitone is ten tenths; the roll draws a whole semitone per row. */
export const PITCH_TENTHS_PER_SEMITONE = 10;
/** Portamento stays inside an octave either way; the roll clamps the gesture. */
export const PITCH_MAX_TENTHS = 120;
/**
 * A curve always spans the note, so its first and last points are anchors at
 * 0 ms and at the note duration. They are created, not drawn: `normalizePitch`
 * puts them there, and the editing helpers below refuse to
 * delete or slide them. Deleting one would store a curve that stops short, which
 * `feed::PitchComputer` silently completes again from `note.pitch.data`.
 */
export const PITCH_MIN_POINTS = 2;
/**
 * How close a press has to be to grab an existing point instead of painting a
 * new one. Tight on purpose: it used to be 7px, and since the points a file
 * brings with it sit only a few pixels in from the note's edges, the first
 * stretch of every note became a grab zone rather than a place to draw.
 */
export const PITCH_HIT_RADIUS_PX = 5;
/**
 * Upper bound on inserted points, so a stroke across a very long note cannot
 * produce an unbounded list. Freeform sampling is already capped by the grid —
 * `PITCH_SAMPLE_TICKS` apart, so one point per 5 ticks of note — which puts the
 * real ceiling at a note a minute long. This is the guard for the rest.
 */
export const PITCH_MAX_POINTS = 2000;
/**
 * The editor works in **f32**, because `domain::PitchPoint.x` is one.
 *
 * `SetNotePitchCommand` refuses a curve whose points are not strictly
 * increasing, and it compares them as f32 — the value it stored. A curve built
 * from f64 arithmetic can contain two points a fraction of an ulp apart that are
 * the SAME f32, so the engine rejects the whole curve with "x values must
 * strictly increase" while both points are visibly distinct on screen and a
 * repaint in the same place reproduces it. Rounding every x to f32 as it is
 * built makes "what the editor sees" and "what the engine compares" the same
 * number, and `sameX` becomes exact equality — the engine's own rule.
 */
const f32 = x => Math.fround(x);
/** Two points at the same f32 x are one point, not a zero-width span. */
const sameX = (a, b) => a === b;
/**
 * The smallest gap the engine can still see between two points: one f32 ulp,
 * with a floor so it stays meaningful near zero.
 */
const xGap = x => Math.max(1e-4, Math.abs(x) * 1.2e-7);
/** `music_math.rs`'s `EP`: a span this short renders as its end value. */
const SPAN_EP = 0.001;

/** ticks → milliseconds, using the project's tempo. */
export const ticksToMs = (ticks, bpm) => (ticks / TICKS_PER_BEAT) * (60_000 / bpm);

/** milliseconds → ticks, the inverse of `ticksToMs`. */
export const msToTicks = (ms, bpm) => (ms * bpm / 60_000) * TICKS_PER_BEAT;

/**
 * One row of the roll is `ROW` px per semitone, so a tenth of a semitone is a
 * tenth of that. Callers pass `pixelsPerTenth` because the row height is a
 * drawing decision that lives in the roll.
 */
export const tenthsToPixels = (tenths, pixelsPerTenth) => tenths * pixelsPerTenth;
export const pixelsToTenths = (pixels, pixelsPerTenth) => pixels / pixelsPerTenth;

/**
 * The curve a note should show when it has none.
 *
 * Two points at 0 and the note duration, both flat, which is what the engine
 * renders for an empty `pitch.data` and what the two-point fixtures contain.
 */
export function flatPitch(note, bpm = 120) {
  const ms = ticksToMs(note?.duration_ticks ?? 480, bpm);
  return { data: [{ x: 0, y: 0, shape: DEFAULT_PITCH_SHAPE }, { x: ms, y: 0, shape: DEFAULT_PITCH_SHAPE }], snap_first: true };
}

/** The stored curve, or the flat one when the note has never been drawn on. */
export function pitchOf(note, bpm = 120) {
  const durationMs = ticksToMs(note?.duration_ticks ?? 480, bpm);
  const stored = note?.pitch?.data;
  if (!Array.isArray(stored) || !stored.length) return flatPitch(note, bpm);
  // Editing needs note-bounded anchors because the engine's note-edit command
  // validates the curve written by a gesture. Display uses pitchForDisplay()
  // instead: imported pre-onset points affect rendered pitch at note start.
  return normalizePitch({
    data: stored.map(point => ({
      x: Number(point.x) || 0,
      y: Number(point.y) || 0,
      shape: isShape(point.shape) ? point.shape : DEFAULT_PITCH_SHAPE,
    })),
    snap_first: note?.pitch?.snap_first !== false,
  }, { durationMs, clampY: false });
}

/**
 * Read an imported curve for the piano-roll overlay without moving its x points.
 * The renderer interpolates from note-relative negative-x points through ms=0;
 * clamping them here changes the visible pitch at onset even though the stored
 * USTX and rendered pitch are unchanged. Out-of-note handles are hidden by the
 * canvas, while sampling the retained points shows the renderer's value.
 */
export function pitchForDisplay(note, bpm = 120) {
  const stored = note?.pitch?.data;
  if (!Array.isArray(stored) || !stored.length) return flatPitch(note, bpm);
  return {
    data: stored.map(point => ({
      x: Number(point.x) || 0,
      y: Number(point.y) || 0,
      shape: isShape(point.shape) ? point.shape : DEFAULT_PITCH_SHAPE,
    })),
    snap_first: note?.pitch?.snap_first !== false,
  };
}

/**
 * Sample a curve at `ms`, for drawing only.
 *
 * The four non-spline shapes are `feed::music_math`'s easing functions, term
 * for term, so the line the roll draws is the line the renderer will produce —
 * including `EP`, which makes a very short span jump to its end value. The one
 * deliberate difference is `sp`: the engine takes a true cubic spline segment
 * through four knots whenever the span is not auto-completed, and a Catmull-Rom
 * through the same knots is the closest thing that can be written without the
 * solver; it agrees at every point and bends the same way between them.
 */
export function samplePitch(curve, ms) {
  const data = curve?.data ?? [];
  if (!data.length) return 0;
  if (ms <= data[0].x) return data[0].y;
  const last = data[data.length - 1];
  if (ms >= last.x) return last.y;
  for (let index = 0; index < data.length - 1; index += 1) {
    const a = data[index], b = data[index + 1];
    if (ms < a.x || ms > b.x) continue;
    const span = b.x - a.x;
    if (span < SPAN_EP) return b.y;
    const t = (ms - a.x) / span;
    const rise = b.y - a.y;
    switch (a.shape) {
      case 'l': return a.y + rise * t;
      case 'i': return a.y + rise * (1 - Math.cos(t * Math.PI / 2));
      case 'o': return a.y + rise * Math.sin(t * Math.PI / 2);
      case 'sp': {
        // Catmull-Rom through the neighbours, guarded at the ends where there
        // is no knot on one side.
        const before = data[index - 1] ?? a;
        const after = data[index + 2] ?? b;
        const t2 = t * t, t3 = t2 * t;
        return 0.5 * (
          2 * a.y
          + (-before.y + b.y) * t
          + (2 * before.y - 5 * a.y + 4 * b.y - after.y) * t2
          + (-before.y + 3 * a.y - 3 * b.y + after.y) * t3
        );
      }
      case 'io':
      default:
        return a.y + rise * (1 - Math.cos(t * Math.PI)) / 2;
    }
  }
  return last.y;
}

/** How far off two curves are, for deciding whether a gesture changed anything. */
export const pitchChanged = (before, after) =>
  JSON.stringify(before?.data ?? []) !== JSON.stringify(after?.data ?? [])
  || (before?.snap_first ?? true) !== (after?.snap_first ?? true);

/* ------------------------------------------------------------------ *
 * Point editing
 *
 * The three helpers below are what the roll's gestures are made of. They all
 * return a NEW curve and never touch their argument, so the gesture can hold
 * the pre-edit curve for `pitchChanged` and so a cancelled drag has nothing to
 * undo.
 * ------------------------------------------------------------------ */

/** Clamp a y into the portamento range the gesture allows. */
export const clampTenths = tenths => Math.max(-PITCH_MAX_TENTHS, Math.min(PITCH_MAX_TENTHS, tenths));

const copyCurve = curve => ({
  data: (curve?.data ?? []).map(point => ({ x: point.x, y: point.y, shape: isShape(point.shape) ? point.shape : DEFAULT_PITCH_SHAPE })),
  snap_first: curve?.snap_first !== false,
});

/** True for the two points that span the note, which the editors leave alone. */
export const isPitchAnchor = (curve, index) => index === 0 || index === curve.data.length - 1;

/**
 * Make a curve safe to hand to the engine: sorted, strictly increasing in x,
 * y inside the portamento range, and spanning the whole note.
 *
 * This is the one rule every editing helper runs its result through, because a
 * curve that breaks it — a
 * shared x, an out-of-order point or a curve that stops short of the note end
 * are all things `SetNotePitchCommand` would reject after the user had already
 * seen the stroke on screen.
 *
 * `clampY` is off when only drawing: a curve read back from a file is drawn as
 * it is stored, even if it reaches further than the editor would let a gesture
 * put it.
 */
export function normalizePitch(curve, { durationMs, shape = DEFAULT_PITCH_SHAPE, clampY = true } = {}) {
  const source = curve?.data?.length ? curve : { data: [], snap_first: true };
  // The note's end is an f32 too, so the anchor lands on the same value the
  // engine will compare it against.
  const end = f32(Math.max(0, Number(durationMs ?? source.data[source.data.length - 1].x)));
  const points = source.data
    .map(point => ({
      x: Number(point.x) || 0,
      y: clampY ? clampTenths(Number(point.y) || 0) : Number(point.y) || 0,
      shape: isShape(point.shape) ? point.shape : shape,
    }))
    .sort((a, b) => a.x - b.x);
  const data = [];
  for (const point of points) {
    const x = f32(Math.max(0, Math.min(end, point.x)));
    const last = data[data.length - 1];
    if (last && sameX(last.x, x)) continue;
    data.push({ x, y: point.y, shape: point.shape });
  }
  if (!data.length) data.push({ x: 0, y: 0, shape });
  else if (data[0].x > 0) data.unshift({ x: 0, y: data[0].y, shape });
  const tail = data[data.length - 1];
  if (tail.x < end) data.push({ x: end, y: tail.y, shape });
  else if (data.length < PITCH_MIN_POINTS) {
    // A one-point curve has no span, and the renderer samples it flat. Give it a
    // second anchor just after the first so there is something to interpolate —
    // by an f32-visible gap, or the engine would collapse the pair back to one
    // point and refuse the curve.
    data.push({ x: f32(Math.min(end, data[0].x + xGap(data[0].x))), y: data[0].y, shape });
  }
  return { data, snap_first: source.snap_first !== false };
}

/**
 * Freeform editing: paint samples into a curve.
 *
 * A sample that lands on an existing point overwrites its value — dragging back
 * and forth across one spot must move the point, not stack duplicates — and a
 * sample between two points is inserted between them. Samples are pushed, not
 * summed, so the result depends only on the curve plus this batch.
 */
export function paintPitchPoints(curve, samples, { shape = DEFAULT_PITCH_SHAPE, durationMs } = {}) {
  const end = Math.max(0, Number(durationMs ?? curve?.data?.[curve.data.length - 1]?.x ?? 0));
  // Normalized first, so a curve that arrived from a file without anchors at 0
  // and the note's duration can still be painted on. Painting into a curve
  // whose points all sit outside the note would drop every sample.
  const next = normalizePitch(curve, { durationMs: end, shape });
  let paintedStart = false;
  let paintedEnd = false;
  for (const sample of samples) {
    const ms = Number(sample?.ms), raw = Number(sample?.tenths);
    if (!Number.isFinite(ms) || !Number.isFinite(raw)) continue;
    // A sample outside the note is dropped, not clamped to its end. A stroke
    // that wanders off to the side used to smear the note's endpoint flat,
    // which looked like the drawing had failed.
    if (ms < 0 || ms > end) continue;
    const tenths = clampTenths(raw);
    const x = f32(ms);
    if (x === 0) paintedStart = true;
    if (x === end) paintedEnd = true;
    // First point strictly to the right of the sample: the insertion slot, and
    // the neighbour of the point the sample lands on.
    const slot = next.data.findIndex(point => point.x > x);
    const existing = slot < 0 ? next.data.length - 1 : slot - 1;
    if (existing >= 0 && sameX(next.data[existing].x, x)) {
      next.data[existing].y = tenths;
      if (!isPitchAnchor(next, existing)) next.data[existing].shape = shape;
      continue;
    }
    // Outside the anchors there is nothing to insert before or after.
    if (slot <= 0 || slot >= next.data.length) continue;
    if (next.data.length >= PITCH_MAX_POINTS) continue;
    next.data.splice(slot, 0, { x, y: tenths, shape });
  }
  // The ends follow the stroke. `feed::PitchComputer` completes a curve that
  // does not reach the note's ends by HOLDING the nearest value flat out to
  // them, so a stroke starting at +20 tenths renders at +20 all the way to the
  // note's start. The editor draws what the renderer will produce, so the
  // anchors have to move with the stroke — unless the pointer actually painted
  // on them, in which case that value is the user's own.
  if (next.data.length > PITCH_MIN_POINTS) {
    if (!paintedStart) next.data[0].y = next.data[1].y;
    if (!paintedEnd) next.data[next.data.length - 1].y = next.data[next.data.length - 2].y;
  }
  return next;
}

/**
 * Move one point. The anchors keep their x and follow the note length, so their
 * x is not editable; every other point is clamped between its neighbours, which
 * is what keeps `x` strictly increasing.
 */
export function movePitchPoint(curve, index, { ms, tenths }, { durationMs } = {}) {
  const next = normalizePitch(curve, { durationMs });
  const point = next.data[index];
  if (!point) return next;
  point.y = clampTenths(Number.isFinite(Number(tenths)) ? Number(tenths) : point.y);
  const end = Math.max(0, Number(durationMs ?? next.data[next.data.length - 1]?.x ?? 0));
  if (isPitchAnchor(next, index)) {
    // Both ends are pinned to the note, so a resize re-pins whichever one the
    // gesture grabbed; the other is already at 0 or `end`.
    next.data[0].x = 0;
    next.data[next.data.length - 1].x = end;
    return next;
  }
  const before = next.data[index - 1].x;
  const after = next.data[index + 1].x;
  // Clamped into the gap, never onto a neighbour: the engine collapses x at f32
  // precision and refuses a curve whose points are not strictly increasing.
  point.x = f32(Math.max(before + xGap(before), Math.min(after - xGap(after), Number(ms) || point.x)));
  return next;
}

/**
 * Delete one point, refusing the anchors and the last pair.
 *
 * A curve with fewer than two points has no span at all, so the engine would
 * read it as "no curve" and render flat — which is what the clear button is
 * for. Returns the same curve when the delete is not allowed, so a caller can
 * dispatch unconditionally and let `pitchChanged` decide.
 */
export function removePitchPoint(curve, index) {
  const next = normalizePitch(curve);
  if (isPitchAnchor(next, index) || next.data.length <= PITCH_MIN_POINTS) return curve;
  return { ...next, data: next.data.filter((_, i) => i !== index) };
}

/** OpenUtau's right-click shape menu, as a cycle over `PITCH_SHAPES`. */
export function cyclePitchShape(curve, index) {
  const next = copyCurve(curve);
  if (isPitchAnchor(next, index)) return curve;
  const at = PITCH_SHAPES.findIndex(entry => entry.id === next.data[index].shape);
  next.data[index].shape = PITCH_SHAPES[(at + 1) % PITCH_SHAPES.length].id;
  return next;
}
