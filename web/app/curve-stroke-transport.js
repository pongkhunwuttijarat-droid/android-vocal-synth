/**
 * Reduce a curve replacement stroke against the latest Rust curve before it
 * crosses an adapter boundary. This is transport-only: Rust still validates,
 * canonicalizes, merges and simplifies the committed command.
 *
 * `readLatest` must return a revision-tagged Rust read plus the descriptor's
 * integer clamp range, default value and precomputed tolerance. `apply` must
 * submit one `curve.apply_stroke` envelope with the supplied expected revision.
 */

export class CurveStrokeTransportError extends Error {
  constructor(message, code = "validation_error") {
    super(message);
    this.name = "CurveStrokeTransportError";
    this.code = code;
  }
}

// Keep transport reduction fail-closed until browser/native shared vectors
// have verified the JavaScript floating-point decisions against Rust.
export const CURVE_STROKE_REDUCER_PARITY_VERIFIED = false;

/** Describe every tolerance-approved chord as an overlapping tick/value pair. */
export function curveStrokeIntervals(points) {
  if (!Array.isArray(points)) throw new CurveStrokeTransportError("Reduced stroke points must be an array");
  for (let index = 0; index < points.length; index += 1) {
    const point = points[index];
    if (!Number.isInteger(point?.tick) || !Number.isInteger(point?.value) ||
        (index > 0 && point.tick <= points[index - 1].tick)) {
      throw new CurveStrokeTransportError("Reduced stroke points must have integer values and increasing ticks");
    }
  }
  return points.slice(1).map((end, index) => {
    const start = points[index];
    return {
      start_tick: start.tick,
      end_tick: end.tick,
      start_value: start.value,
      end_value: end.value,
    };
  });
}

/** Prepare a reduced request from a fresh authoritative Rust curve read. */
export async function prepareSimplifiedCurveStroke({
  command,
  gestureRevision,
  readLatest,
  maxStrokePoints = 20_000,
}) {
  if (command?.type !== "curve.apply_stroke" || command.mode !== "replace") {
    throw new CurveStrokeTransportError("Only curve.apply_stroke replace commands can be reduced");
  }
  if (!CURVE_STROKE_REDUCER_PARITY_VERIFIED) {
    throw new CurveStrokeTransportError("Curve reduction is disabled until client/Rust parity is verified", "unsupported_operation");
  }
  if (!Number.isSafeInteger(gestureRevision) || gestureRevision < 0) {
    throw new CurveStrokeTransportError("gestureRevision must be a non-negative safe integer");
  }
  const latest = await readLatest(command.part_id, command.curve_id);
  validateLatest(latest, command, gestureRevision);
  const points = reduceCurveStroke({
    basePoints: latest.points,
    startTick: command.start_tick,
    endTick: command.end_tick,
    rawPoints: command.points,
    minValue: latest.descriptor.min_value,
    maxValue: latest.descriptor.max_value,
    defaultValue: latest.descriptor.default_value,
    tolerance: latest.descriptor.simplify_tolerance,
  });
  if (points.length > maxStrokePoints) {
    throw new CurveStrokeTransportError("Simplified stroke exceeds the advertised point limit", "payload_too_large");
  }
  return {
    request: {
      expected_revision: latest.revision,
      command: { ...command, points },
    },
    context: latest,
  };
}

/**
 * Submit a simplified replace stroke. Each attempt re-reads Rust and prepares
 * a new reduced payload. Raw samples are never sent; revision conflicts retry.
 */
export async function commitSimplifiedCurveStroke({
  command,
  gestureRevision,
  readLatest,
  apply,
  maxAttempts = 4,
  maxStrokePoints = 20_000,
}) {
  if (!Number.isInteger(maxAttempts) || maxAttempts < 1) {
    throw new CurveStrokeTransportError("maxAttempts must be a positive integer");
  }
  for (let attempt = 0; attempt < maxAttempts; attempt += 1) {
    const { request } = await prepareSimplifiedCurveStroke({ command, gestureRevision, readLatest, maxStrokePoints });
    try {
      return await apply(request);
    } catch (error) {
      if (!isRevisionConflict(error) || attempt + 1 === maxAttempts) throw error;
    }
  }
  throw new CurveStrokeTransportError("Could not commit curve stroke after rebasing", "revision_conflict");
}

/** Convenience adapter for the shared in-process WASM Engine API. */
export function commitCurveStrokeInWasm(engine, { command, gestureRevision, maxAttempts = 4 }) {
  if (typeof engine?.session_curve_stroke_context_json !== "function" ||
      typeof engine?.session_apply_v2 !== "function") {
    throw new CurveStrokeTransportError("Engine does not expose the Session curve stroke APIs", "unsupported_operation");
  }
  return commitSimplifiedCurveStroke({
    command,
    gestureRevision,
    maxAttempts,
    readLatest: (partId, curveId) => JSON.parse(engine.session_curve_stroke_context_json(partId, curveId)),
    apply: request => JSON.parse(engine.session_apply_v2(JSON.stringify(request))),
  });
}

/**
 * Return only canonical stroke points that survive simplifying the full
 * post-replacement curve. The returned points are safe to replay over the
 * same base with Rust's normal `curve.apply_stroke` semantics.
 */
export function reduceCurveStroke({
  basePoints,
  startTick,
  endTick,
  rawPoints,
  minValue,
  maxValue,
  defaultValue,
  tolerance,
}) {
  for (const [name, value] of Object.entries({ startTick, endTick, minValue, maxValue, defaultValue })) {
    if (!Number.isInteger(value)) throw new CurveStrokeTransportError(`${name} must be an integer`);
  }
  if (startTick < 0 || startTick >= endTick) throw new CurveStrokeTransportError("Invalid stroke interval");
  if (minValue > maxValue || !Number.isFinite(tolerance) || tolerance < 0) {
    throw new CurveStrokeTransportError("Invalid curve descriptor reduction context");
  }
  if (!Array.isArray(basePoints) || !Array.isArray(rawPoints) || rawPoints.length === 0) {
    throw new CurveStrokeTransportError("A replace stroke needs base and raw points");
  }

  const stroke = canonicalize(rawPoints, startTick, endTick, minValue, maxValue);
  const base = basePoints.map((point, index) => {
    if (!Number.isInteger(point.tick) || !Number.isInteger(point.value)) {
      throw new CurveStrokeTransportError(`Invalid base point at index ${index}`);
    }
    if (index > 0 && point.tick <= basePoints[index - 1].tick) {
      throw new CurveStrokeTransportError("Base curve ticks must be strictly increasing");
    }
    return { tick: point.tick, value: point.value, fromStroke: false };
  });

  const leftValue = sample(base, startTick, defaultValue);
  const rightValue = sample(base, endTick, defaultValue);
  const merged = base.filter(point => point.tick <= startTick || point.tick >= endTick);
  insert(merged, { tick: startTick, value: leftValue, fromStroke: false });
  insert(merged, { tick: endTick, value: rightValue, fromStroke: false });
  for (const point of stroke) insert(merged, { ...point, fromStroke: true });

  const keep = simplify(merged, tolerance);
  const retained = new Map();
  for (let index = 0; index < merged.length; index += 1) {
    if (keep[index] && merged[index].fromStroke) retained.set(merged[index].tick, merged[index].value);
  }
  let reduced = stroke
    .filter(point => retained.get(point.tick) === point.value)
    .map(({ tick, value }) => ({ tick, value }));

  // The frozen command schema requires at least one point for `replace`. A
  // point discarded by RDP remains within tolerance of the retained polyline,
  // and Rust's commit-time simplifier removes it again.
  if (reduced.length === 0) reduced = [{ tick: stroke[0].tick, value: stroke[0].value }];
  return reduced;
}

function validateLatest(latest, command, gestureRevision) {
  if (!latest || !Number.isSafeInteger(latest.revision) || latest.revision < gestureRevision) {
    throw new CurveStrokeTransportError("Latest curve read is invalid or older than the gesture base", "revision_conflict");
  }
  if (latest.part_id !== command.part_id || latest.curve_id !== command.curve_id || latest.abbr !== command.abbr) {
    throw new CurveStrokeTransportError("Latest curve identity does not match the stroke target", "revision_conflict");
  }
  if (!latest.descriptor) throw new CurveStrokeTransportError("Latest curve read has no authoritative descriptor context");
}

function canonicalize(rawPoints, startTick, endTick, minValue, maxValue) {
  const ordered = rawPoints.map((point, index) => ({ ...point, index }));
  for (const point of ordered) {
    if (!Number.isInteger(point.tick) || !Number.isInteger(point.value) || point.tick < startTick || point.tick > endTick) {
      throw new CurveStrokeTransportError("Stroke points must be integer values inside the replacement interval");
    }
  }
  ordered.sort((a, b) => a.tick - b.tick || a.index - b.index);
  const points = [];
  for (const point of ordered) {
    const tick = Math.max(startTick, Math.min(endTick, roundHalfEven(point.tick / 5) * 5));
    const value = Math.max(minValue, Math.min(maxValue, point.value));
    const last = points[points.length - 1];
    if (last?.tick === tick) last.value = value;
    else points.push({ tick, value });
  }
  return points;
}

function sample(points, tick, fallback) {
  const index = points.findIndex(point => point.tick >= tick);
  if (index >= 0 && points[index].tick === tick) return points[index].value;
  if (index <= 0 || index === -1) return fallback;
  const left = points[index - 1], right = points[index];
  const ratio = (tick - left.tick) / (right.tick - left.tick);
  return roundCsharp(left.value + (right.value - left.value) * ratio);
}

function insert(points, value) {
  let index = points.findIndex(point => point.tick >= value.tick);
  if (index < 0) index = points.length;
  if (points[index]?.tick === value.tick) points[index] = value;
  else points.splice(index, 0, value);
}

function simplify(points, tolerance) {
  const keep = Array(points.length).fill(false);
  if (points.length < 3) {
    keep.fill(true);
    return keep;
  }
  keep[0] = true;
  keep[points.length - 1] = true;
  simplifySpan(points, 0, points.length - 1, tolerance, keep);
  return keep;
}

function simplifySpan(points, first, last, tolerance, keep) {
  if (last <= first + 1) return;
  const p1 = points[first], p2 = points[last];
  const dx = p2.tick - p1.tick, dy = p2.value - p1.value;
  const length = Math.hypot(dx, dy);
  let best = first + 1, maxDistance = -1;
  for (let index = first + 1; index < last; index += 1) {
    const point = points[index];
    const distance = length === 0
      ? Math.hypot(point.tick - p1.tick, point.value - p1.value)
      : Math.abs(dy * point.tick - dx * point.value + p2.tick * p1.value - p2.value * p1.tick) / length;
    if (distance > maxDistance) {
      maxDistance = distance;
      best = index;
    }
  }
  if (maxDistance > tolerance) {
    keep[best] = true;
    simplifySpan(points, first, best, tolerance, keep);
    simplifySpan(points, best, last, tolerance, keep);
  }
}

function roundHalfEven(value) {
  const floor = Math.floor(value), fraction = value - floor;
  if (fraction < 0.5) return floor;
  if (fraction > 0.5) return floor + 1;
  return floor % 2 === 0 ? floor : floor + 1;
}

function roundCsharp(value) {
  if (value < 0) return -roundHalfEven(-value);
  return roundHalfEven(value);
}

function isRevisionConflict(error) {
  if (error?.code === "revision_conflict") return true;
  const message = typeof error?.message === "string" ? error.message : "";
  try { return JSON.parse(message).code === "revision_conflict"; }
  catch { return message.includes("revision_conflict"); }
}
