// Turning a painted freeform gesture into a §6.6.2 `curve.apply_stroke`
// envelope — and nothing else.
//
// This is deliberately NOT a second simplifier. CONTRACT-v1 §6.7.2 makes the
// committed curve the output of ONE commit-time Ramer-Douglas-Peucker pass in
// Rust (`commands::simplify_curve`, tolerance `min(5, (max-min)*0.005)`), so a
// host may only prepare the envelope: snap the samples the engine would snap,
// drop duplicates the engine would collapse, and clamp to the descriptor range
// it would clamp anyway. Every point that survives here is a point Rust keeps
// or removes by its own rule, so the drawn result is identical either way.
//
// The client-side reducer in `web/app/curve-stroke-transport.js` is the
// optional transport-compression path (T009) and is still gated off pending
// JS/Rust parity vectors. This module is the always-available path: send the
// canonical samples, let Rust simplify. That is why the panel used to look
// "unsimplified" — it was not sending what the user painted at all.
import { clampToLane } from './mock-data.js';

/** `UCurve::INTERVAL` / `commands::STROKE_INTERVAL` — Rust snaps to this too. */
export const STROKE_TICK_INTERVAL = 5;

/**
 * The tolerance Rust will simplify the committed curve with
 * (`min(5, (max - min) * 0.005)`). Shown in the panel footer so the stored
 * point count is explainable; it is never used to filter points here.
 */
export const simplifyTolerance = lane =>
  Math.min(5, (lane.max - lane.min) * 0.005);

/** Half away from zero, matching Rust's `csharp_round`. */
const roundHalfAway = value => {
  const floor = Math.floor(value);
  const fraction = value - floor;
  if (fraction < 0.5) return floor;
  if (fraction > 0.5) return floor + 1;
  return floor % 2 === 0 ? floor : floor + 1;
};
const snapTick = tick => roundHalfAway(tick / STROKE_TICK_INTERVAL) * STROKE_TICK_INTERVAL;

/**
 * Canonicalize painted samples into the points of one `curve.apply_stroke`.
 *
 * Mirrors `commands::canonicalize` (sort by tick, snap to the 5-tick grid,
 * clamp to the descriptor range, later sample wins a shared tick) so the
 * payload is already canonical. `samples` arrive in pointer order and are NOT
 * monotonic — a drag back and forth must overwrite, not accumulate.
 *
 * Returns `{ startTick, endTick, points }`. The interval is the painted span,
 * NOT the whole part: `mode: "replace"` edits `(start, end)` exclusively, so a
 * stroke covering the whole part is what flattened the curve's ends. Rust
 * anchors both ends from the existing curve before applying the stroke, so the
 * ends outside the gesture are the user's own curve, not the default.
 */
export function strokeEnvelope(samples, lane, partTicks) {
  const limit = Math.max(0, Math.round(partTicks || 0));
  // Sort by RAW tick with the arrival index as the tie-break, exactly as
  // `canonicalize` does. Snapping afterwards is what makes two nearby samples
  // collapse onto one tick, and the later sample must win regardless of the
  // order the pointer produced them.
  const ordered = (samples || [])
    .map((sample, index) => ({ sample, index }))
    .sort((a, b) => a.sample.tick - b.sample.tick || a.index - b.index);
  const canonical = [];
  for (const { sample } of ordered) {
    const tick = Math.min(limit, Math.max(0, snapTick(sample.tick)));
    const value = Math.round(clampToLane(lane, sample.value));
    const last = canonical[canonical.length - 1];
    if (last && last.tick === tick) last.value = value;
    else canonical.push({ tick, value });
  }
  if (!canonical.length) return null;
  return { startTick: canonical[0].tick, endTick: canonical[canonical.length - 1].tick, points: canonical };
}

/**
 * The payload's byte size and point count, for the footer. Not a limit check:
 * the advertised `max_stroke_points` is enforced by the engine, which answers
 * `payload_too_large` rather than truncating what the user painted.
 */
export const strokeSummary = envelope => ({
  points: envelope?.points.length ?? 0,
  bytes: JSON.stringify(envelope?.points ?? []).length,
});