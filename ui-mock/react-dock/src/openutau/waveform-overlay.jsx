// Waveform overlay — the render preview drawn over the piano roll.
//
// OpenUtau draws it as a fixed strip: a 60px `WaveformImage` pinned to the
// bottom of the notes area, 60px clear of the edge (`Margin="0,0,0,60"`
// Height="60"` in PianoRoll.axaml), scroll-linked through `TickOffset` and
// painted as a per-pixel min/max envelope in 50% grey. Here the same strip is a
// movable overlay instead: it keeps OpenUtau's default placement and rendering,
// adds a grab bar and a resize edge, and remembers where it was put — placement
// is presentation state, so it stays in the frontend.
//
// The peaks are the engine's OWN output (`waveform-source.js` renders the
// session's USTX and builds a min/max envelope). While that render is missing
// or impossible the strip stays empty and says why, instead of drawing a shape
// that is not the audio.
import React, { useCallback, useEffect, useRef } from 'react';
import { ROLL, TICKS_PER_BEAT } from './clone-state.jsx';
import { P, useTheme } from './palette.js';
import { peaksInRange } from './waveform-source.js';

const MIN_HEIGHT = 24;
const MAX_HEIGHT = 320;
/** OpenUtau only shows peaks once the view is zoomed in; keep a floor. */
const MIN_COLUMN_WIDTH = 1;

function useDrag(onDelta) {
  const active = useRef(null);
  const onPointerDown = useCallback(event => {
    event.currentTarget.setPointerCapture?.(event.pointerId);
    active.current = { x: event.clientX, y: event.clientY };
    event.stopPropagation();
    event.preventDefault();
  }, []);
  const onPointerMove = useCallback(event => {
    const start = active.current;
    if (!start) return;
    const dx = event.clientX - start.x;
    const dy = event.clientY - start.y;
    if (dx === 0 && dy === 0) return;
    // Keep the drag anchored: the pointer is a position, not a delta.
    active.current = { x: event.clientX, y: event.clientY };
    onDelta(dx, dy);
    event.stopPropagation();
  }, [onDelta]);
  const onPointerUp = useCallback(event => {
    if (!active.current) return;
    active.current = null;
    event.currentTarget.releasePointerCapture?.(event.pointerId);
    event.stopPropagation();
  }, []);
  return { onPointerDown, onPointerMove, onPointerUp, onPointerCancel: onPointerUp };
}

/**
 * Peak envelope, drawn the way `WaveformImage.Render` does it: for every pixel
 * column take the min and max of the samples it covers and fill between them,
 * centred on the zero line.
 *
 * The columns are ticks; the audio is seconds. `bpm` converts between them, and
 * it comes from the engine's project so the strip stays aligned to the grid
 * even when the tempo is not the UI's default.
 */
function drawPeaks(canvas, { width, height, scrollX, partTicks, tokens, peaks, bpm }) {
  const context = canvas.getContext('2d');
  if (!context || width <= 0 || height <= 0) return;
  const t = tokens || P();
  context.clearRect(0, 0, width, height);
  const right = Math.min(width, ROLL.gridLeft + (partTicks - scrollX) / TICKS_PER_BEAT * ROLL.beatWidth);
  const middle = height / 2;
  context.fillStyle = t.waveSurface;
  context.fillRect(0, 0, width, height);
  context.fillStyle = t.waveZero;
  context.fillRect(0, middle - 0.5, width, 1);
  if (!peaks || !peaks.samples) return;
  context.fillStyle = t.wavePeak;
  const step = Math.max(MIN_COLUMN_WIDTH, Math.round(1));
  const secondsPerTick = 60 / (bpm || 120) / TICKS_PER_BEAT;
  const ticksPerColumn = TICKS_PER_BEAT / ROLL.beatWidth * step;
  for (let x = ROLL.gridLeft; x <= right; x += step) {
    const tick = (x + scrollX - ROLL.gridLeft) / ROLL.beatWidth * TICKS_PER_BEAT;
    const range = peaksInRange(peaks, tick * secondsPerTick, (tick + ticksPerColumn) * secondsPerTick);
    if (!range) continue;
    const [lo, hi] = range;
    // OpenUtau's own mapping, `WaveformImage.DrawPeak`: a sample `s` lands on
    // `(0.5 + s * 0.5) * height` and is clamped to the bitmap, so silence sits
    // on the centre line and a full-scale sample reaches the strip edge. Match
    // it exactly — the strip is meant to be the same picture OpenUtau draws,
    // and an inset scale would make the clone's envelope read as a different
    // take from the same audio.
    const top = Math.min(height - 1, Math.max(0, (0.5 + lo * 0.5) * height));
    const bottom = Math.min(height - 1, Math.max(0, (0.5 + hi * 0.5) * height));
    context.fillRect(x, top, step, Math.max(1, bottom - top));
  }
}

/** What the strip is waiting for, in one line. */
function statusText(status, error) {
  if (status === 'rendering') return 'rendering…';
  if (status === 'ready') return null;
  if (status === 'error') return error || 'render failed';
  return 'waiting for a Rust session';
}

/**
 * Hover text for the strip, including the cache split when the pool reports it:
 * `0 rendered / 4 reused` is the point — the fader changed the mix, not the
 * rendered chunks.
 */
function canvasTitle(status, source, peaks, stats, bank) {
  if (status !== 'ready') return statusText(status, null) || '';
  const parts = [`Render preview from ${source || 'the engine'}${bank ? ` (${bank.name})` : ''}`];
  if (peaks?.samples) parts.push(`${(peaks.samples / 44100).toFixed(1)}s`);
  if (stats) parts.push(`${stats.lastRendered} chunk${stats.lastRendered === 1 ? '' : 's'} rendered / ${stats.lastReused} reused`);
  if (bank?.reason) parts.push(bank.reason);
  return parts.join(' · ');
}

/**
 * The bank the strip is looking at, shown rather than only in the hover text.
 *
 * A render preview that silently uses the bundled dev fixture is exactly what
 * makes it look like the engine disagrees with OpenUtau, so say which bank the
 * samples came from — and flag the fixture as a stand-in.
 */
function bankLabel(bank) {
  if (!bank) return null;
  return bank.kind === 'fixture' ? `demo bank · ${bank.name} (not the project's)` : `bank · ${bank.name}`;
}

export function WaveformOverlay({ size, scroll, partTicks, placement, onPlacement, onClose, peaks, bpm = 120, status = 'idle', error = null, source = null, stats = null, bank = null }) {
  const canvasRef = useRef(null);
  const height = Math.round(placement.height);
  const theme = useTheme();
  const bankText = bankLabel(bank);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    // The 2D canvas is sized in CSS pixels; the roll has no device-pixel ratio
    // scaling of its own, so keep them 1:1 like Pixi's default renderer.
    canvas.width = size.width;
    canvas.height = height;
    drawPeaks(canvas, { width: size.width, height, scrollX: scroll.x, partTicks, tokens: theme, peaks, bpm });
  }, [size.width, height, scroll.x, partTicks, theme, peaks, bpm]);

  // Dragging the grab bar moves the strip; dragging the bottom edge resizes it.
  // Each move sends a FUNCTION of the current placement, not a value computed
  // from the last render: React batches the intents, so a computed value would be
  // derived from a stale placement and a fast drag would lose most of its travel.
  const dragMove = useDrag((_dx, dy) => onPlacement(current => ({ ...current, bottom: Math.max(0, current.bottom - dy) })));
  const dragResize = useDrag((_dx, _dy) => onPlacement(current => ({
    ...current, height: Math.max(MIN_HEIGHT, Math.min(MAX_HEIGHT, current.height + _dy)),
  })));

  const note = statusText(status, error);
  return <div className="ou-wave-overlay" style={{ bottom: Math.round(placement.bottom), height }}>
    {/* Grab bar: OpenUtau's strip cannot be moved; here the whole overlay is. */}
    <div className="ou-wave-handle" title="Drag to move the waveform overlay" {...dragMove}><span className="ou-wave-grip" /></div>
    <canvas ref={canvasRef} style={{ width: size.width, height }} title={canvasTitle(status, source, peaks, stats, bank)} />
    {note && <span className="ou-wave-status" title={note}>{note}</span>}
    {!note && bankText && <span className="ou-wave-bank" title={canvasTitle(status, source, peaks, stats, bank)}>{bankText}</span>}
    <button type="button" className="ou-wave-close" title="Hide waveform" onClick={onClose}>×</button>
    <div className="ou-wave-resize" title="Drag to resize" {...dragResize} />
  </div>;
}
