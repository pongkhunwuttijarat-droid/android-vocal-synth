// Expression editor — one expression per tab, like OpenUtau's expression strip.
//
// OpenUtau keeps ten `ExpSelector` slots stacked under the notes, each bound to
// one expression descriptor and reachable with Alt+1..Alt+0 (PianoRoll.axaml,
// `PianoRoll.axaml.cs:OnKeyDown`), with the whole strip toggled by
// `ShowExpressions`. Six stacked 30px strips do not fit a dock panel, so this is
// the same idea as tabs: one curve at a time, the same Alt+N shortcuts, and the
// descriptor's tag colour on the tab (OpenUtau colours the selector header with
// the expression's `TagBrush`).
//
// The curve is ENGINE state, not frontend state. CONTRACT-v1 §6.6
// `curve.apply_stroke` takes `{abbr, curve_id, create, mode, start_tick,
// end_tick, points}`, mints the id when the curve does not exist yet, and
// answers with a `curve.changed` patch holding the canonical points (5-tick
// snap, anchors at the descriptor default on both boundaries, clamped to
// min/max). `parts[].curves` in the session snapshot is what the editor draws,
// so the panel never trusts the points it sent. Draw semantics follow
// `NoteEditStates.cs`: a drag paints a transient preview and commits ONCE on
// pointer-up.
import React, { useEffect, useMemo, useRef, useState } from 'react';
import { Application as PixiApplication, extend } from '@pixi/react';
import { Container, Graphics, Text } from 'pixi.js';
import { EXPRESSION_LANES, ROLL, TICKS_PER_BEAT, TICKS_PER_MEASURE, clampScrollX, clampToLane, useClone } from './clone-state.jsx';
import { simplifyTolerance, strokeEnvelope, strokeSummary } from './curve-stroke-input.js';
import { P, pixi, useTheme } from './palette.js';
import { useElementSize, useRendererSize, useRenderOnDemand } from './use-element-size.js';

extend({ Container, Graphics, Text });

/** One point per half measure is dense enough to read, sparse enough to drag. */
const POINT_INTERVAL = TICKS_PER_MEASURE / 2;
const DRAG_THRESHOLD = 3;
/** Below this the drag was a click, so it does not become a stroke. */
const STROKE_MIN_PIXELS = 4;
/**
 * Beats of empty lane kept past the end of the part, so the last measure is not
 * glued to the right edge and there is somewhere to put the boundary anchor.
 */
const LANE_TAIL_BEATS = 4;

/** 0..1 across the descriptor's full range; the y axis is always min..max. */
const valueToY = (lane, value, height) =>
  height - 8 - ((value - lane.min) / (lane.max - lane.min)) * (height - 16);
const yToValue = (lane, y, height) =>
  clampToLane(lane, lane.min + (1 - (y - 8) / (height - 16)) * (lane.max - lane.min));
const formatValue = (lane, value) =>
  `${Math.abs(value) >= 100 ? value.toFixed(0) : value.toFixed(1)}${lane.unit ? ` ${lane.unit}` : ''}`;

/** Points straight from the engine curve, or the descriptor default when absent. */
const curveOf = (part, abbr) => (part?.curves || []).find(curve => curve.abbr === abbr) || null;

/** OpenUtau's base shape for an expression that has never been written. */
function seededPoints(lane, partTicks) {
  const points = [];
  const span = lane.max - lane.min;
  for (let tick = 0; tick <= partTicks; tick += POINT_INTERVAL) {
    // A gentle arc inside the descriptor range so the default curve is not the
    // same line as every other expression.
    const t = partTicks ? tick / partTicks : 0;
    points.push({ tick, value: clampToLane(lane, lane.default + Math.sin(t * Math.PI) * span * 0.18) });
  }
  return points;
}

/**
 * A single edited point is a one-point replace over a minimal interval, not the
 * whole part. `mode: "replace"` edits `(start, end)` exclusively and Rust
 * anchors both ends from the existing curve, so the rest of the phrase keeps
 * its shape.
 */
const pointEnvelope = (tick, value, lane, partTicks) =>
  strokeEnvelope([{ tick, value }, { tick: tick + 5, value }], lane, partTicks);

function CurveCanvas({ lane, points, stroke, width, height, partTicks, scrollX, playheadTicks, partStartTicks, selection, onSelection, onStrokeEnd, appRef }) {
  const [hover, setHover] = useState(null);
  const theme = useTheme();
  const drag = useRef(null);
  const gridLeft = ROLL.gridLeft;
  // Same scale as the roll — the two strips are meant to line up — but the lane
  // scrolls on its own bar, so the offset is added back before pixels become
  // ticks and subtracted before ticks become pixels.
  const tickToX = tick => gridLeft + tick / TICKS_PER_BEAT * ROLL.beatWidth - scrollX;
  const xToTick = x => Math.max(0, (x + scrollX - gridLeft) / ROLL.beatWidth * TICKS_PER_BEAT);

  const hitPoint = (x, y) => {
    let best = null;
    let bestDistance = DRAG_THRESHOLD * 2;
    for (const point of points) {
      const distance = Math.hypot(tickToX(point.tick) - x, valueToY(lane, point.value, height) - y);
      if (distance < bestDistance) { best = point; bestDistance = distance; }
    }
    return best;
  };

  const point = event => {
    const rect = event.currentTarget.getBoundingClientRect();
    return { x: event.clientX - rect.left, y: event.clientY - rect.top };
  };

  const onPointerDown = event => {
    const { x, y } = point(event);
    event.currentTarget.setPointerCapture?.(event.pointerId);
    const hit = hitPoint(x, y);
    if (hit) {
      // Dragging an existing point edits it; a freeform drag paints a stroke.
      drag.current = { kind: 'point', tick: hit.tick, value: hit.value, origin: hit.tick, moved: false };
      onSelection([hit.tick]);
      return;
    }
    const tick = Math.round(xToTick(x) / POINT_INTERVAL) * POINT_INTERVAL;
    drag.current = { kind: 'stroke', origin: hit?.tick ?? tick, start: { x, y }, moved: false, samples: [] };
    onSelection([tick]);
  };

  const onPointerMove = event => {
    const { x, y } = point(event);
    setHover({ tick: xToTick(x), value: yToValue(lane, y, height) });
    if (!drag.current) return;
    if (drag.current.kind === 'stroke') {
      const travelled = Math.hypot(x - drag.current.start.x, y - drag.current.start.y);
      if (!drag.current.moved && travelled < STROKE_MIN_PIXELS) return;
      drag.current.moved = true;
      drag.current.samples.push({ tick: Math.round(xToTick(x)), value: yToValue(lane, y, height) });
      onStrokeEnd(null, drag.current.samples);
      return;
    }
    // Vertical drag edits the value; horizontal drag moves the point in time.
    if (Math.abs(x - tickToX(drag.current.tick)) < DRAG_THRESHOLD) {
      drag.current.value = yToValue(lane, y, height);
      onSelection([drag.current.tick], drag.current.value);
    } else {
      const tick = Math.round(xToTick(x) / POINT_INTERVAL) * POINT_INTERVAL;
      if (tick !== drag.current.tick) onSelection([drag.current.tick], drag.current.value, tick);
      drag.current.tick = tick;
    }
  };

  const onPointerUp = event => {
    const dragState = drag.current;
    drag.current = null;
    event.currentTarget.releasePointerCapture?.(event.pointerId);
    // OpenUtau commits the whole gesture as one command on release.
    if (dragState?.kind === 'stroke' && dragState.moved) onStrokeEnd(dragState.samples, null);
    else onStrokeEnd(null, null);
  };

  const draw = React.useCallback(g => {
    g.clear();
    const t = P();
    const colour = pixi(lane.color);
    // `@pixi/react` only applies `background` on mount, so the canvas paints its
    // own backdrop — which is also what makes a theme switch repaint it. The
    // renderer's own size is used because the buffer can be taller than the
    // measured host for a frame after a dock resize, and an unpainted strip
    // would otherwise show the mount-time clear colour.
    const screen = appRef?.current?.getApplication?.()?.screen;
    const fillW = Math.max(width, screen?.width ?? width);
    const fillH = Math.max(height, screen?.height ?? height);
    g.rect(0, 0, fillW, fillH).fill(t.surfaceRaised);
    const right = Math.min(width, tickToX(partTicks));
    // Beat and measure grid, matching the roll's spacing so the two line up, and
    // only for the beats the viewport covers: the lane is as long as the part.
    for (let b = 0; tickToX(b * TICKS_PER_BEAT) <= right; b++) {
      const x = tickToX(b * TICKS_PER_BEAT);
      if (x > width) break;
      const measure = b % 4 === 0;
      g.moveTo(x, 0).lineTo(x, height).stroke({ color: pixi(measure ? t.expGridMeasure : t.expGrid), width: measure ? 1.2 : 1 });
    }
    for (const fraction of [0.25, 0.5, 0.75]) {
      const y = valueToY(lane, lane.min + (lane.max - lane.min) * fraction, height);
      g.moveTo(gridLeft, y).lineTo(right, y).stroke({ color: pixi(t.expHLine), width: 1 });
    }
    // The descriptor default, so it is obvious where a value sits.
    const defaultY = valueToY(lane, lane.default, height);
    if (defaultY > 8 && defaultY < height - 8) {
      g.moveTo(gridLeft, defaultY).lineTo(right, defaultY).stroke({ color: pixi(t.expGridMeasure), width: 1 });
    }
    const path = points.filter(point => point.tick <= partTicks);
    if (path.length > 1) {
      g.moveTo(tickToX(path[0].tick), height);
      for (const point of path) g.lineTo(tickToX(point.tick), valueToY(lane, point.value, height));
      g.lineTo(tickToX(path[path.length - 1].tick), height).fill({ color: colour, alpha: 0.12 });
      g.moveTo(tickToX(path[0].tick), valueToY(lane, path[0].value, height));
      for (const point of path.slice(1)) g.lineTo(tickToX(point.tick), valueToY(lane, point.value, height));
      g.stroke({ color: colour, width: 1.8 });
      for (const point of path) {
        const selected = selection.includes(point.tick);
        g.circle(tickToX(point.tick), valueToY(lane, point.value, height), selected ? 4.5 : 3)
          .fill(t.expFill).stroke({ color: colour, width: selected ? 2 : 1.3 });
      }
    }
    // Transient freeform preview: `previewLine` in NoteEditStates.cs.
    if (stroke?.length > 1) {
      g.moveTo(tickToX(stroke[0].tick), valueToY(lane, stroke[0].value, height));
      for (const point of stroke.slice(1)) g.lineTo(tickToX(point.tick), valueToY(lane, point.value, height));
      g.stroke({ color: colour, width: 2, alpha: 0.9 });
    }
    const localPlayhead = playheadTicks - partStartTicks;
    if (localPlayhead >= 0 && localPlayhead <= partTicks) {
      const x = tickToX(localPlayhead);
      if (x >= 0 && x <= width) g.moveTo(x, 0).lineTo(x, height).stroke({ color: pixi(t.playhead), width: 2, alpha: 0.9 });
    }
  }, [lane, points, stroke, width, height, partTicks, scrollX, playheadTicks, partStartTicks, selection, theme, appRef]);

  return <div className="ou-exp-canvas" role="application" aria-label={`${lane.name} curve`}
    style={{ width, height }}
    onPointerDown={event => onPointerDown(event)} onPointerMove={onPointerMove} onPointerUp={onPointerUp} onPointerCancel={onPointerUp}>
    {/* The ref is what `useRendererSize` resizes through: @pixi/react only sizes
       the renderer when it mounts, so without it the 900x150 default buffer stays
       and gets stretched over the whole strip. */}
    <PixiApplication ref={appRef} width={width} height={height} background={theme.surfaceRaised} antialias preference="webgl">
      <pixiGraphics draw={draw} eventMode="none" />
    </PixiApplication>
    {hover && <span className="ou-exp-readout" style={{ left: Math.min(width - 150, Math.max(2, tickToX(hover.tick) + 8)), top: 4 }}>
      {formatValue(lane, hover.value)}
    </span>}
  </div>;
}

export function ExpressionLanesPanel() {
  const { state, dispatch, activePart, connected } = useClone();
  const partId = state.activePartId;
  const partTicks = activePart?.duration_ticks ?? TICKS_PER_BEAT * 4;
  const [active, setActive] = useState(0);
  const [hostRef, size] = useElementSize({ width: 900, height: 150 });
  const pixiRef = useRef(null);
  const [stroke, setStroke] = useState(null);
  const [sent, setSent] = useState(null);
  const [selection, setSelection] = useState([]);
  // The strip is as long as the part — a 25-bar phrase is 3800px of curve at the
  // roll's 68px/beat, and anything past the dock used to be drawn off the
  // canvas with no way to reach it. Same treatment as the arrangement: a real
  // scroll container over a spacer, with the canvas pinned by `position:sticky`.
  const [scrollX, setScrollX] = useState(0);
  useEffect(() => {
    if (!state.playing) return;
    const localTick = state.playheadTicks - (activePart?.position_ticks ?? 0);
    const x = ROLL.gridLeft + localTick / TICKS_PER_BEAT * ROLL.beatWidth;
    const margin = Math.min(96, size.width * 0.15);
    if (x - scrollX > size.width - margin) {
      const next = clampScrollX(x - margin, size.width, partTicks + LANE_TAIL_BEATS * TICKS_PER_BEAT);
      setScrollX(next);
      if (hostRef.current) hostRef.current.scrollLeft = next;
    }
  }, [state.playing, state.playheadTicks, activePart?.position_ticks, partTicks, size.width, scrollX, hostRef]);
  const contentWidth = ROLL.gridLeft + (partTicks / TICKS_PER_BEAT + LANE_TAIL_BEATS) * ROLL.beatWidth;
  useRendererSize(pixiRef, size.width, Math.max(60, size.height));
  useRenderOnDemand(pixiRef);
  const lane = EXPRESSION_LANES[active] ?? EXPRESSION_LANES[0];

  // Scrolling is the browser's (scrollbar, trackpad, shift+wheel); the offset
  // comes back as state so the drawing follows.
  const onScroll = event => {
    const next = Math.round(event.currentTarget.scrollLeft);
    setScrollX(current => current === next ? current : next);
  };
  // A new part, a shorter phrase or a narrower dock narrows the range. Returning
  // `current` when nothing has to move keeps this from looping.
  useEffect(() => {
    setScrollX(current => {
      const next = clampScrollX(current, size.width, partTicks + LANE_TAIL_BEATS * TICKS_PER_BEAT);
      if (next !== current && hostRef.current) hostRef.current.scrollLeft = next;
      return next === current ? current : next;
    });
  }, [partTicks, size.width, hostRef]);

  // The engine curve is the source of truth; the seeded shape is only what a
  // part with no such curve draws until the first stroke replaces it.
  const engineCurve = curveOf(activePart, lane.abbr);
  const enginePoints = useMemo(
    () => (engineCurve?.points || []).map(point => ({ tick: point.tick, value: point.value })),
    [engineCurve],
  );
  const points = enginePoints.length ? enginePoints : seededPoints(lane, partTicks);

  const commit = envelope => {
    if (!envelope) return;
    dispatch({
      type: 'exp/stroke',
      partId,
      abbr: lane.abbr,
      curveId: engineCurve?.id ?? null,
      create: !engineCurve,
      startTick: envelope.startTick,
      endTick: envelope.endTick,
      points: envelope.points,
    });
  };

  /** Point drag: absolute target, one command on release. */
  const onSelection = (ticks, value, moveToTick) => {
    setSelection(ticks);
    if (value === undefined) return;
    const tick = moveToTick ?? ticks[0];
    commit(pointEnvelope(tick, clampToLane(lane, value), lane, partTicks));
  };

  // Alt+1..Alt+7 pick the lane, the way OpenUtau's ExpSelector slots do.
  useEffect(() => {
    const onKey = event => {
      if (!event.altKey || event.ctrlKey || event.metaKey) return;
      const index = '1234567'.indexOf(event.key);
      if (index < 0 || index >= EXPRESSION_LANES.length) return;
      event.preventDefault();
      setActive(index);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  useEffect(() => { setSelection([]); setStroke(null); setSent(null); }, [active, partId]);

  // Dev-only viewport handle, like the roll's and the arrangement's.
  if (import.meta.env?.DEV) window.__ouExp = { scrollX, width: size.width, partTicks, contentWidth, sent, stored: enginePoints.length };

  const freeform = samples => {
    if (!samples) {
      // Release without movement: keep whatever the click selected.
      setStroke(null);
      return;
    }
    // The painted samples ARE the stroke: they go out canonicalized (5-tick
    // snap, clamp, duplicates collapsed) and Rust simplifies the committed
    // curve with the descriptor tolerance. Regridding them onto the
    // half-measure draw spacing first is what used to throw the gesture away.
    setStroke(samples);
    const envelope = strokeEnvelope(samples, lane, partTicks);
    setSent(strokeSummary(envelope));
    commit(envelope);
    setStroke(null);
  };

  return <div className="ou-lanes">
    <div className="ou-lanes-head">
      <strong>Expressions</strong>
      <span>{activePart?.name || 'No part'}</span>
      <span className="ou-lanes-hint">Alt+1..7</span>
    </div>
    {/* One tab per expression descriptor: abbr and name, in its tag colour. */}
    <div className="ou-exp-tabs" role="tablist">
      {EXPRESSION_LANES.map((entry, index) => {
        const written = !!curveOf(activePart, entry.abbr);
        return <button key={entry.abbr} type="button" role="tab" aria-selected={index === active}
          className={`ou-exp-tab ${index === active ? 'on' : ''} ${written ? '' : 'off'}`} style={{ '--lane-color': entry.color }}
          title={`${entry.name} (${entry.abbr}) — ${entry.min}…${entry.max}${entry.unit ? ` ${entry.unit}` : ''}, default ${entry.default} (Alt+${index + 1})`}
          onClick={() => setActive(index)}>
          <span className="ou-exp-tab-abbr">{entry.abbr}</span>
          <span className="ou-exp-tab-name">{entry.name}</span>
          <span className="ou-exp-tab-check" aria-hidden="true">{written ? '✓' : '·'}</span>
        </button>;
      })}
    </div>
    <div className="ou-exp-body" ref={hostRef} onScroll={onScroll}>
      {/* Wider than the viewport: this element is what the scrollbar travels. */}
      <div className="ou-exp-track" style={{ width: contentWidth }}>
        <CurveCanvas key={lane.abbr} lane={lane} points={points} stroke={stroke}
          width={size.width} height={Math.max(60, size.height)} scrollX={scrollX}
          partTicks={partTicks} playheadTicks={state.playheadTicks} partStartTicks={activePart?.position_ticks ?? 0}
          selection={selection} onSelection={onSelection} onStrokeEnd={freeform} appRef={pixiRef} />
      </div>
    </div>
    <div className="ou-exp-foot">
      <span className="ou-lane-mode">{lane.name} · {lane.min}…{lane.max}{lane.unit ? ` ${lane.unit}` : ''} · default {formatValue(lane, lane.default)}</span>
      <button type="button" className="ou-exp-clear" disabled={!engineCurve} title="Remove this curve (writes the descriptor default back)"
        onClick={() => commit(strokeEnvelope([{ tick: 0, value: lane.default }, { tick: partTicks, value: lane.default }], lane, partTicks))}>clear</button>
      <span className="ou-exp-note">
        {connected
          ? `curve.apply_stroke → ${engineCurve ? `curve ${engineCurve.id.slice(0, 8)}` : 'engine mints a new curve id'} · ${engineCurve ? `${enginePoints.length} points read back from the session` : 'no curve yet — the flat line is the descriptor default'}${sent ? ` · last stroke sent ${sent.points} points (${sent.bytes} B), engine stored ${enginePoints.length}` : ''} · tolerance ${simplifyTolerance(lane).toFixed(2)}`
          : 'mock document — strokes are not sent anywhere'}
      </span>
    </div>
  </div>;
}
