/** Convert a pane's pixel scale to a shared, project-absolute timeline tick. */
export function scrollOffsetToTicks(scrollX, pixelsPerTick) {
  const offset = Number(scrollX);
  const scale = Number(pixelsPerTick);
  if (!Number.isFinite(offset) || !Number.isFinite(scale) || scale <= 0) return 0;
  return Math.max(0, Math.round(offset / scale));
}

/** Convert shared project ticks back to a pane's pixel scale. */
export function ticksToScrollOffset(ticks, pixelsPerTick) {
  const position = Number(ticks);
  const scale = Number(pixelsPerTick);
  if (!Number.isFinite(position) || !Number.isFinite(scale) || scale <= 0) return 0;
  return Math.max(0, Math.round(position * scale));
}

/** Sanitize the ephemeral shared viewport value before storing it in UI state. */
export function normalizeTimelineScrollTicks(ticks) {
  const value = Number(ticks);
  return Number.isFinite(value) ? Math.max(0, Math.round(value)) : 0;
}
