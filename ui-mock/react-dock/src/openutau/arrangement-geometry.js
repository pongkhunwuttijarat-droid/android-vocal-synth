// Arrangement timeline geometry, as plain data-in/data-out functions.
//
// This lives outside `clone-state.jsx` so `node --test` can import it: the
// panel's whole correctness argument — a pixel drawn at X is exactly the pixel
// that can be clicked — rests on these numbers agreeing across the Pixi draw
// passes, the DOM hit layer and the scrollbar, and that has to be checkable
// without a browser.
//
// The document's own length decides how far the timeline can scroll (see
// `projectTicks` in clone-state); the viewport only decides how much of it is
// on screen. Nothing here reads project state directly.
import { TICKS_PER_BEAT, TICKS_PER_MEASURE } from './mock-data.js';

/** Arrangement: fixed track-header column + Pixi-drawn measure timeline. */
export const ARR = { headerWidth: 220, minHeaderWidth: 160, maxHeaderWidth: 420, rowHeight: 136, rulerHeight: 28, measureWidth: 72, visibleMeasures: 16, minMeasureWidth: 10, maxMeasureWidth: 480, trailingMeasures: 4 };

/**
 * Pixels per measure for a dock of `viewportWidth`.
 *
 * The opening view is OpenUtau's 16 measures across the dock rather than a fixed
 * column: the canvas is exactly the dock wide and the zoom level is whatever
 * makes that many measures fit. Everything horizontal then follows from one
 * `measureWidth`, so zooming, scrolling, drawing and hit testing cannot drift
 * apart.
 */
export const arrMeasureWidth = (viewportWidth, visibleMeasures = ARR.visibleMeasures) =>
  Math.max(ARR.minMeasureWidth, Math.floor(viewportWidth / Math.max(1, visibleMeasures)));

/**
 * Measures the timeline has to cover: as many as the document needs, plus a
 * tail so the last bar is not glued to the right edge, and never fewer than the
 * opening view. `contentTicks` is the engine's own document length.
 */
export const arrContentMeasures = (contentTicks, visibleMeasures = ARR.visibleMeasures) =>
  Math.max(visibleMeasures, Math.ceil((contentTicks || 0) / TICKS_PER_MEASURE) + ARR.trailingMeasures);

/** Horizontal scroll offset, clamped to the scrollable range. */
export const clampArrScrollX = (scrollX, viewportWidth, contentWidth) =>
  Math.max(0, Math.min(Math.max(0, contentWidth - viewportWidth), scrollX || 0));

/** First measure whose column can still touch the viewport at this offset. */
export const arrFirstMeasure = (scrollX, measureWidth) => Math.max(0, Math.floor((scrollX || 0) / measureWidth));

/** Measure columns covering the viewport, plus the partial one at the edge. */
export const arrVisibleMeasures = (viewportWidth, measureWidth) => Math.ceil(viewportWidth / measureWidth) + 1;

/**
 * Scroll offset that keeps the measure under the viewport's centre where it was
 * across a zoom change. Zooming around the left edge instead would drag the
 * song out from under the pointer on every step.
 */
export const arrZoomScrollX = (scrollX, viewportWidth, fromWidth, toWidth, contentWidth) =>
  clampArrScrollX(((scrollX || 0) + viewportWidth / 2) / fromWidth * toWidth - viewportWidth / 2, viewportWidth, contentWidth);

/**
 * Part block rectangle inside the arrangement timeline, in canvas pixels.
 * `measureWidth` follows the dock's real width and `scrollX` is the timeline's
 * horizontal offset, so the drawing and the DOM hit layer agree without CSS
 * stretching one of them.
 */
export function arrPartRect(part, trackIndex, measureWidth = ARR.measureWidth, scrollX = 0) {
  const x = part.position_ticks / TICKS_PER_MEASURE * measureWidth - scrollX;
  const y = ARR.rulerHeight + trackIndex * ARR.rowHeight + 8;
  const w = Math.max(24, part.duration_ticks / TICKS_PER_MEASURE * measureWidth);
  return { x, y, w, h: ARR.rowHeight - 16 };
}

/** Miniature note rectangle inside its owning synth part's arrangement block. */
export function arrNoteRect(part, note, trackIndex, measureWidth = ARR.measureWidth, scrollX = 0, pitchRange = { low: 48, high: 84 }) {
  const partPosition = Number(part?.position_ticks);
  const partDuration = Number(part?.duration_ticks);
  const notePosition = Number(note?.position_ticks);
  const noteDuration = Number(note?.duration_ticks);
  const tone = Number(note?.tone);
  if (![partPosition, partDuration, notePosition, noteDuration, tone].every(Number.isFinite) || noteDuration <= 0) return null;

  // Notes are local to their part in the piano roll; the arrangement adds the
  // part's project offset before mapping them through the shared measure scale.
  const localStart = Math.max(0, notePosition);
  const localEnd = Math.min(partDuration, notePosition + noteDuration);
  if (localEnd <= localStart) return null;
  const bounds = arrPartRect(part, trackIndex, measureWidth, scrollX);
  const x = (partPosition + localStart) / TICKS_PER_MEASURE * measureWidth - scrollX;
  const w = Math.max(2, (localEnd - localStart) / TICKS_PER_MEASURE * measureWidth);
  const innerTop = bounds.y + 8;
  const innerBottom = bounds.y + bounds.h - 8;
  const low = Number.isFinite(Number(pitchRange?.low)) ? Number(pitchRange.low) : 48;
  const high = Number.isFinite(Number(pitchRange?.high)) ? Number(pitchRange.high) : 84;
  const span = Math.max(1, high - low);
  const normalized = Math.max(0, Math.min(1, (tone - low) / span));
  const centerY = innerBottom - normalized * (innerBottom - innerTop);
  const h = Math.max(2, Math.min(6, (innerBottom - innerTop) / span * 0.72));
  return { x, y: centerY - h / 2, w, h };
}

/** Project-time window represented by a horizontal waveform pixel span. */
export function arrWaveformTimeRange(x, pixelSpan, measureWidth, scrollX, bpm) {
  if (!(measureWidth > 0) || !(bpm > 0)) return null;
  const secondsPerTick = 60 / (bpm * TICKS_PER_BEAT);
  const tickAt = px => (px + (scrollX || 0)) / measureWidth * TICKS_PER_MEASURE;
  return {
    fromSeconds: tickAt(x) * secondsPerTick,
    toSeconds: tickAt(x + pixelSpan) * secondsPerTick,
  };
}

/** Which lane (track row) a timeline y coordinate falls in, -1 for the ruler. */
export function arrLaneAt(y) {
  if (y < ARR.rulerHeight) return -1;
  return Math.floor((y - ARR.rulerHeight) / ARR.rowHeight);
}

/** Timeline x → musical tick, including the scroll offset. */
export function arrTickAt(x, measureWidth = ARR.measureWidth, scrollX = 0) {
  return Math.max(0, Math.round((x + scrollX) / measureWidth * TICKS_PER_MEASURE / TICKS_PER_BEAT) * TICKS_PER_BEAT);
}

/** Topmost part under a timeline point, mirroring OpenUtau's row-first hit test. */
export function arrHitPart(tracks, x, y, measureWidth = ARR.measureWidth, scrollX = 0) {
  const lane = arrLaneAt(y);
  if (lane < 0 || lane >= tracks.length) return null;
  for (const part of tracks[lane].parts || []) {
    const r = arrPartRect(part, lane, measureWidth, scrollX);
    if (x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h) return { part, trackIndex: lane, rect: r };
  }
  return null;
}
