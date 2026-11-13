// Piano Roll panel: OpenUtau-style — vertical tool rail, a PixiJS canvas that
// fills the dock (keyboard + note grid scrolled inside it) and expression lanes.
//
// The canvas is sized to its host instead of to the whole musical height, so the
// dock keeps its layout and OpenUtau-style wheel scrolling lives in this file.
// All gesture math lives in clone-state.js so the drawing code and the hit test
// always agree, and so each gesture maps onto exactly one engine command group
// once the mock document is replaced by a real `project-session`.
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Application as PixiApplication, extend } from '@pixi/react';
import { Container, Graphics, Text } from 'pixi.js';
import { phonemeOf } from './mock-data.js';
import { useElementSize, useRendererSize, useRenderOnDemand } from './use-element-size.js';
import { WaveformOverlay } from './waveform-overlay.jsx';
import { P, pixi, useTheme } from './palette.js';
import {
  DEFAULT_PITCH_SHAPE, PITCH_HIT_RADIUS_PX, PITCH_SAMPLE_TICKS, PITCH_SHAPES, PITCH_TENTHS_PER_SEMITONE,
  cyclePitchShape, flatPitch, movePitchPoint, paintPitchPoints, pixelsToTenths, pitchForDisplay, pitchOf, removePitchPoint,
  samplePitch, msToTicks, ticksToMs,
} from './pitch-curve-input.js';
import { scrollOffsetToTicks, ticksToScrollOffset } from './timeline-scroll.js';
import {
  DRAG_THRESHOLD, MIN_NOTE_TICKS, ROLL, TICKS_PER_BEAT, VIBRATO_DEFAULTS, WAVEFORM_DEFAULT,
  clampScroll, clampScrollX, defaultScroll, projectTicks, rollHitNote, rollNoteRect, rollReadPointer, snapTick, toneLabel, useClone,
} from './clone-state.jsx';

extend({ Container, Graphics, Text });

/** One row of the shortcut strip / keyboard / grid shares this height. */
const ROW = ROLL.rowHeight;
/** Keyboard shortcuts OpenUtau prints on the far-left strip. */
const SHORTCUTS = { 36: '1', 38: '2', 40: '3', 43: '4', 45: '5', 48: '6', 50: '7', 52: '8', 55: '9', 57: '10', 60: '20', 65: '22', 72: '43' };

/** Everything a draw pass needs about where the viewport is looking. */
function useViewportGeometry(contentTicks, timelineScrollTicks, dispatch) {
  const [hostRef, size] = useElementSize({ width: 900, height: 420 });
  const [scrollY, setScrollY] = useState(() => defaultScroll(ROW));
  const pixelsPerTick = ROLL.beatWidth / TICKS_PER_BEAT;
  const scroll = {
    x: clampScrollX(ticksToScrollOffset(timelineScrollTicks, pixelsPerTick), size.width, contentTicks),
    y: scrollY,
  };
  const scrollRef = useRef(scroll);
  scrollRef.current = scroll;
  const setScroll = useCallback(update => {
    const current = scrollRef.current;
    const next = typeof update === 'function' ? update(current) : update;
    if (!next) return;
    const x = clampScrollX(next.x ?? current.x, size.width, contentTicks);
    const y = clampScroll(next.y ?? current.y, size.height);
    if (x !== current.x) dispatch({
      type: 'viewport/horizontal-scroll',
      ticks: scrollOffsetToTicks(x, pixelsPerTick),
    });
    if (y !== current.y) setScrollY(y);
  }, [contentTicks, dispatch, pixelsPerTick, size.height, size.width]);
  // Wheel scrolling belongs to the canvas, so the dock never scrolls the page.
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return undefined;
    const onWheel = event => {
      const horizontal = event.shiftKey || Math.abs(event.deltaX) > Math.abs(event.deltaY);
      setScroll(current => horizontal
        ? { ...current, x: clampScrollX(current.x + event.deltaY + event.deltaX, size.width, contentTicks) }
        : { ...current, y: clampScroll(current.y + event.deltaY, size.height) });
      event.preventDefault();
    };
    host.addEventListener('wheel', onWheel, { passive: false });
    return () => host.removeEventListener('wheel', onWheel);
  }, [contentTicks, hostRef, setScroll, size.height, size.width]);
  return { hostRef, size, scroll, setScroll };
}

function KeyboardCanvas({ size, scroll, appRef }) {
  const theme = useTheme();
  const draw = React.useCallback(g => {
    g.clear();
    const t = P();
    const screen = appRef?.current?.getApplication?.()?.screen;
    g.rect(0, 0, ROLL.gridLeft, Math.max(size.height, screen?.height ?? 0)).fill(t.rollKeyBg);
    const first = Math.floor((scroll.y - ROLL.topRuler) / ROW);
    const last = Math.ceil((scroll.y + size.height - ROLL.topRuler) / ROW);
    for (let i = first; i <= last; i++) {
      const tone = ROLL.maxTone - i;
      const y = ROLL.topRuler + i * ROW - scroll.y;
      if (tone < ROLL.minTone || tone > ROLL.maxTone) continue;
      const black = [1, 3, 6, 8, 10].includes(tone % 12);
      g.rect(ROLL.shortcutWidth, y, ROLL.keyboardWidth, ROW).fill({ color: pixi(black ? t.rollKeyBlack : t.rollKeyWhite) }).stroke({ color: t.rollKeyBorder, width: 1 });
      if (tone % 12 === 0) g.rect(ROLL.shortcutWidth, y, ROLL.keyboardWidth, ROW).fill({ color: pixi(t.rollCHighlight), alpha: 0.28 });
    }
    g.rect(0, 0, ROLL.shortcutWidth, size.height).fill(t.rollShortcut);
    g.moveTo(ROLL.gridLeft, 0).lineTo(ROLL.gridLeft, size.height).stroke({ color: pixi(t.rulerLine), width: 1 });
  }, [size, scroll.y, theme, appRef]);
  return <pixiGraphics draw={draw} eventMode="none" />;
}

function ToneLabels({ size, scroll }) {
  const theme = useTheme();
  const first = Math.floor((scroll.y - ROLL.topRuler) / ROW);
  const last = Math.ceil((scroll.y + size.height - ROLL.topRuler) / ROW);
  return Array.from({ length: Math.max(0, last - first + 1) }, (_, n) => first + n)
    .filter(i => { const tone = ROLL.maxTone - i; return tone % 12 === 0 && tone >= ROLL.minTone && tone <= ROLL.maxTone; })
    .map(i => <pixiText key={`tone-${ROLL.maxTone - i}`} text={toneLabel(ROLL.maxTone - i)} x={ROLL.shortcutWidth + 6} y={ROLL.topRuler + i * ROW - scroll.y + 4} style={{ fontFamily: 'Segoe UI, sans-serif', fontSize: 10, fill: theme.rollKeyName }} eventMode="none" />);
}

function ShortcutColumn({ size, scroll }) {
  const theme = useTheme();
  return Object.entries(SHORTCUTS).map(([tone, label]) => (
    <pixiText key={`sc-${tone}`} text={label} x={4} y={ROLL.topRuler + (ROLL.maxTone - Number(tone)) * ROW - scroll.y + 5} style={{ fontFamily: 'Segoe UI, sans-serif', fontSize: 10, fill: theme.rollShortcutText }} eventMode="none" />
  ));
}

function RollCanvas({ notes, selection, partId, playheadTicks, partTicks, size, scroll, pitchCurves, bpm, appRef }) {
  const theme = useTheme();
  const draw = React.useCallback(g => {
    g.clear();
    const t = P();
    // Paint the whole grid area, not just the measured size: an unpainted strip
    // would show the clear colour `@pixi/react` fixed at mount. The keyboard
    // column is left to KeyboardCanvas, which draws first.
    const screen = appRef?.current?.getApplication?.()?.screen;
    g.rect(ROLL.gridLeft, 0, Math.max(size.width, screen?.width ?? 0) - ROLL.gridLeft, Math.max(size.height, screen?.height ?? 0)).fill(t.rollBg);
    const left = ROLL.gridLeft - scroll.x;
    const first = Math.floor((scroll.y - ROLL.topRuler) / ROW);
    const last = Math.ceil((scroll.y + size.height - ROLL.topRuler) / ROW);
    for (let i = first; i <= last; i++) {
      const tone = ROLL.maxTone - i;
      if (tone < ROLL.minTone || tone > ROLL.maxTone) continue;
      const y = ROLL.topRuler + i * ROW - scroll.y;
      g.rect(left, y, size.width - ROLL.gridLeft + scroll.x, ROW).fill({ color: pixi(tone % 12 === 1 || tone % 12 === 6 ? t.rollRowBlack : t.rollRow) });
      g.moveTo(left, y + ROW).lineTo(size.width, y + ROW).stroke({ color: pixi(t.gridLine), width: 1 });
    }
    // Beat and measure lines, with a measure-number header strip pinned on top.
    // `b` counts quarter-beats. The range comes from the viewport, not from a
    // fixed count: a hard-coded 0..128 stops drawing at beat 128, which on a real
    // song (the Hatsune golden's `harm` part is 1036 beats) is 13% of the way
    // through — past that the roll keeps its notes but loses every grid line and
    // every measure number, so scrolling right reads as "the piano roll broke".
    const quarterPerBeat = 4;
    const firstQuarter = Math.max(0, Math.floor((scroll.x - ROLL.gridLeft) / ROLL.beatWidth * quarterPerBeat) - 1);
    const lastQuarter = Math.ceil((scroll.x + size.width - ROLL.gridLeft) / ROLL.beatWidth * quarterPerBeat) + 1;
    for (let b = firstQuarter; b <= lastQuarter; b++) {
      const x = ROLL.gridLeft + b * ROLL.beatWidth / 4 - scroll.x;
      if (x < ROLL.gridLeft - 2 || x > size.width) continue;
      const measure = b % (4 * quarterPerBeat) === 0;
      g.moveTo(x, 0).lineTo(x, size.height).stroke({ color: pixi(measure ? t.gridLineMeasure : t.gridLineSoft), width: measure ? 1.4 : 1 });
      if (measure) {
        g.rect(x - 1, 0, Math.min(28, size.width - x + 1), ROLL.topRuler - 2).fill(t.rulerBg);
        g.moveTo(x, 0).lineTo(x, ROLL.topRuler - 2).stroke({ color: pixi(t.rulerLine), width: 1.4 });
      }
    }
    g.rect(ROLL.gridLeft, 0, size.width - ROLL.gridLeft, ROLL.topRuler - 2).fill(t.rulerBg);
    g.rect(ROLL.gridLeft, ROLL.topRuler - 2, size.width - ROLL.gridLeft, 1).fill(t.rulerLine);
    // Waveform used to be painted into the grid here. OpenUtau draws it as its
    // own strip (WaveformImage in PianoRoll.axaml) so it can sit above the
    // notes and move independently, so it lives in WaveformOverlay now.
    // Stored pitch curves are drawn above note bodies below, matching OpenUtau.
    // Notes
    for (const note of notes) {
      const rect = rollNoteRect(note);
      const x = rect.x - scroll.x, y = rect.y - scroll.y;
      if (x > size.width || x + rect.w < ROLL.gridLeft) continue;
      const selected = selection.has(note.id);
      const foreign = partId && note.part_id !== partId;
      g.roundRect(x, y, rect.w, rect.h, 4).fill({ color: pixi(foreign ? t.noteFillForeign : selected ? t.noteFillSelected : t.noteFill) })
        .stroke({ color: pixi(selected ? t.noteStrokeSelected : t.noteStroke), width: selected ? 1.4 : 1 });
      if (rect.w > 26) g.rect(x + 3, y + rect.h - 4, Math.min(20, rect.w - 8), 2).fill({ color: pixi(t.noteShelf), alpha: 0.5 });
    }
    // OpenUtau draws every stored note curve independently of selection. Map
    // each note-relative millisecond point through the project time axis; in
    // particular, negative-X snap points stay before the note instead of being
    // clamped to its left edge. The project model currently exposes one BPM.
    for (const note of notes) {
      const curve = pitchCurves?.get(note.id);
      if (!curve?.data || curve.data.length < 2) continue;
      const noteX = ms => ROLL.gridLeft
        + (note.position_ticks + msToTicks(ms, bpm)) / TICKS_PER_BEAT * ROLL.beatWidth
        - scroll.x;
      const zero = ROLL.topRuler + (ROLL.maxTone - note.tone) * ROW + ROW / 2 - scroll.y;
      const pitchY = tenths => zero - tenths / PITCH_TENTHS_PER_SEMITONE * ROW;
      const firstMs = curve.data[0].x;
      const lastMs = curve.data[curve.data.length - 1].x;
      const firstX = noteX(firstMs), lastX = noteX(lastMs);
      if (Math.max(firstX, lastX) < ROLL.gridLeft || Math.min(firstX, lastX) > size.width) continue;
      const steps = Math.max(1, Math.min(240, Math.ceil(Math.abs(lastX - firstX) / 4)));
      let drawing = false;
      for (let step = 0; step <= steps; step += 1) {
        const ms = firstMs + (lastMs - firstMs) * step / steps;
        const x = noteX(ms), y = pitchY(samplePitch(curve, ms));
        if (x < ROLL.gridLeft || x > size.width || y < ROLL.topRuler || y > size.height) {
          drawing = false;
          continue;
        }
        if (drawing) g.lineTo(x, y);
        else { g.moveTo(x, y); drawing = true; }
      }
      g.stroke({ color: pixi(t.accent), width: 1.6, alpha: 0.9 });
    }
    // Playhead
    g.rect(ROLL.gridLeft + playheadTicks / TICKS_PER_BEAT * ROLL.beatWidth - scroll.x, 0, 2, size.height).fill({ color: pixi(t.playhead), alpha: 0.7 });
  }, [notes, selection, partId, playheadTicks, partTicks, size, scroll, pitchCurves, bpm, theme, appRef]);
  return <pixiGraphics draw={draw} eventMode="none" />;
}

function MeasureNumbers({ size, scroll }) {
  const theme = useTheme();
  // Counted from the first measure the viewport can see, not from measure 1:
  // numbering always started at the origin, so on a long song the ruler went
  // blank after the first screenful of measures even with the grid still drawn.
  const perMeasure = 4 * ROLL.beatWidth;
  const first = Math.max(0, Math.floor((scroll.x - ROLL.gridLeft) / perMeasure));
  const count = Math.ceil(size.width / perMeasure) + 2;
  return Array.from({ length: count }, (_, i) => {
    const m = first + i;
    const x = ROLL.gridLeft + m * perMeasure - scroll.x;
    if (x < ROLL.gridLeft || x > size.width) return null;
    return <pixiText key={`m-${m}`} text={String(m + 1)} x={x + 5} y={4} style={{ fontFamily: 'Segoe UI, sans-serif', fontSize: 11, fill: theme.measureNumber }} eventMode="none" />;
  });
}

/**
 * Note labels. OpenUtau can show either the lyric or the phonemizer output, so
 * the label mode is shared state and the roll re-renders on toggle.
 */
function NoteLyrics({ notes, scroll, mode }) {
  const theme = useTheme();
  return notes.map(note => {
    const rect = rollNoteRect(note);
    const x = rect.x - scroll.x;
    if (rect.w < 16) return null;
    const text = mode === 'phoneme' ? phonemeOf(note.lyric) : note.lyric;
    const vibrato = note.vibrato ? 4 : 0;
    return <pixiText key={`lyric-${note.id}`} text={`${text}${vibrato ? ' 〜' : ''}`} x={x + 5} y={rect.y - scroll.y + 3}
      style={{ fontFamily: 'Segoe UI, sans-serif', fontSize: 11, fill: note.vibrato ? theme.noteLabelVibrato : theme.noteLabel }} eventMode="none" />;
  });
}

/**
 * Follows the active part. Switching parts (or opening a fresh engine session,
 * whose part ids the roll has never seen) scrolls the grid to that part's notes
 * instead of leaving the viewport wherever the last one happened to be.
 *
 * Both axes matter. A part imported from a real song rarely starts at bar 1 —
 * `harm` in the Hatsune golden begins at tick 205200 — so a roll left at the
 * previous project's horizontal offset shows an empty grid over a header that
 * still reads "319 notes". Centring the tones alone hid that completely.
 */
function useFollowPart(partId, notes, size, setScroll, contentTicks, partStartTicks) {
  const followed = useRef(null);
  useEffect(() => {
    if (!partId || size.height <= 0) return;
    // Only remember the part once it actually had something to centre on: a
    // session can hand over the part before its notes arrive, and marking it
    // followed there would leave the roll scrolled past the phrase.
    if (followed.current !== partId) {
      followed.current = partId;
      if (!notes.length) {
        // An audio part has no notes to follow, but its project position is still
        // the shared timeline origin for both Arrangement and Piano Roll.
        // There is no tone range to centre vertically, so leave `y` untouched.
        const x = clampScrollX(
          ticksToScrollOffset(partStartTicks ?? 0, ROLL.beatWidth / TICKS_PER_BEAT),
          size.width,
          contentTicks,
        );
        setScroll(current => (current.x === x ? current : { ...current, x }));
        return;
      }
      const tones = notes.map(note => note.tone);
      const middle = (Math.min(...tones) + Math.max(...tones)) / 2;
      const y = clampScroll(ROLL.topRuler + (ROLL.maxTone - middle) * ROW - size.height / 2, size.height);
      // Horizontally, only move when the phrase is not already on screen. A
      // part that starts at bar 60 (`harm` in the Hatsune golden begins at tick
      // 205200) is invisible from bar 1 however many notes it has, while a
      // phrase that already fits must not be nudged off bar 1 just because its
      // first note is one beat in.
      const firstPx = Math.min(...notes.map(note => note.position_ticks)) / TICKS_PER_BEAT * ROLL.beatWidth;
      const lastPx = Math.max(...notes.map(note => note.position_ticks + note.duration_ticks)) / TICKS_PER_BEAT * ROLL.beatWidth;
      const visible = Math.max(0, size.width - ROLL.gridLeft);
      setScroll(current => {
        const onScreen = current.x <= firstPx && lastPx <= current.x + visible;
        const x = onScreen ? current.x : clampScrollX(firstPx, size.width, contentTicks);
        return { ...current, x, y };
      });
      return;
    }
    // The dock changed size: keep the offset legal without throwing away the
    // user's own scrolling. Returning `current` when nothing has to move is
    // load-bearing: a fresh object here would re-render the panel, re-run this
    // effect, and spin.
    setScroll(current => {
      const y = clampScroll(current.y, size.height);
      const x = clampScrollX(current.x, size.width, contentTicks);
      return y === current.y && x === current.x ? current : { ...current, x, y };
    });
  }, [partId, notes, size.height, size.width, contentTicks, partStartTicks, setScroll]);
}

/** One gesture = one engine command group; `edit` in clone-state tracks it. */
function RollInteraction({ size, scroll }) {
  const { state, dispatch, dispatchGesture, beginGesture, endGesture, activeNotes, bpm } = useClone();
  const { tool, snap, selection } = state;
  const pitchEdit = state.pitchEdit ?? { shape: DEFAULT_PITCH_SHAPE, snapFirst: true };
  const notesRef = useRef(activeNotes); notesRef.current = activeNotes;
  const [marquee, setMarquee] = useState(null);
  const gesture = useRef(null);
  // OpenUtau gives the note edge its own cursor (`PianoRoll.axaml.cs`
  // `NotesCanvasPointerMoved` → `cursorSizeWE` on the resize band, a move cursor
  // on the body). Without it the roll only ever shows the active tool's cursor,
  // the 8px stretch handle stays invisible, and stretching reads as impossible.
  const [hoverCursor, setHoverCursor] = useState(null);
  const setHover = next => setHoverCursor(previous => (previous === next ? previous : next));

  /* ---------------------------------------------------------------- *
   * Pitch curve geometry.
   *
   * The note rectangle IS the curve's box — x is the note's own duration in
   * ms, y is tenths of a semitone around the note's tone — so there is no second
   * scale to keep in sync, and the drawing code in RollCanvas uses the same two
   * conversions. One row is one semitone, so one tenth of a row is one tenth of
   * a semitone.
   * ---------------------------------------------------------------- */
  const pitchMs = note => ticksToMs(note.duration_ticks, bpm);
  // World coordinates, like `rollHitNote` and `rollReadPointer`: the pointer
  // handlers add the scroll offset themselves, and the drawing pass in
  // RollCanvas does its own screen conversion. Comparing a screen point against
  // a world one is what made the hit test miss every point on a scrolled roll.
  const pitchPointPx = (note, point) => {
    const rect = rollNoteRect(note);
    const span = pitchMs(note) || 1;
    return {
      x: rect.x + Math.max(0, Math.min(1, point.x / span)) * rect.w,
      y: rect.y + rect.h / 2 - point.y / PITCH_TENTHS_PER_SEMITONE * ROW,
    };
  };
  const pitchSampleAt = (note, worldX, worldY) => {
    const rect = rollNoteRect(note);
    // Sampled on OpenUtau's 5-tick grid, on the ABSOLUTE tick, exactly as
    // `DrawPitchState` does — so the stored curve's density matches a file
    // authored in OpenUtau rather than depending on how fast the pointer moved.
    const absTick = (worldX - ROLL.gridLeft) / ROLL.beatWidth * TICKS_PER_BEAT;
    const snapped = Math.round(absTick / PITCH_SAMPLE_TICKS) * PITCH_SAMPLE_TICKS;
    const noteTick = Math.max(0, Math.min(note.duration_ticks, snapped - note.position_ticks));
    return {
      ms: ticksToMs(noteTick, bpm),
      tenths: pixelsToTenths(rect.y + rect.h / 2 - worldY, ROW / PITCH_TENTHS_PER_SEMITONE),
      tick: snapped,
    };
  };
  /**
   * Which note a pitch gesture press belongs to.
   *
   * By x span only, not by the note rectangle. The rectangle is one row tall
   * (18px), but a drawn pitch curve leaves the note — a rising stroke starts
   * ABOVE the note body — so requiring the press to land inside the rectangle
   * made the most natural gesture do nothing at all. When several notes share a
   * time span (a chord) the one whose row is vertically nearest wins, so the
   * press still goes to the note the pointer is aiming at.
   */
  const pitchHitNote = (worldX, worldY) => {
    let best = null, nearest = Infinity;
    for (const note of notesRef.current) {
      const rect = rollNoteRect(note);
      if (worldX < rect.x || worldX > rect.x + rect.w) continue;
      const distance = worldY < rect.y ? rect.y - worldY : worldY > rect.y + rect.h ? worldY - (rect.y + rect.h) : 0;
      if (distance < nearest) { nearest = distance; best = note; }
    }
    return best;
  };
  /**
   * The editable point under the pointer, or -1. Index 0 and the last point are
   * the anchors and are skipped: they are drawn hollow and `removePitchPoint`
   * and `movePitchPoint` both refuse them.
   */
  const hitPitchPoint = (note, worldX, worldY) => {
    const curve = pitchOf(note, bpm);
    let best = -1, nearest = PITCH_HIT_RADIUS_PX;
    for (let index = 1; index < curve.data.length - 1; index += 1) {
      const px = pitchPointPx(note, curve.data[index]);
      const distance = Math.hypot(px.x - worldX, px.y - worldY);
      if (distance <= nearest) { nearest = distance; best = index; }
    }
    return best;
  };
  /**
   * One `note/pitch` per move, absolute like `note/move`: the whole curve goes
   * out each time because the engine owns the stored points, so the gesture
   * never has to reconstruct a delta from the snapshot it just replaced.
   */
  const commitPitch = gesture => {
    const pitch = { data: gesture.curve.data, snap_first: pitchEdit.snapFirst };
    gesture.curve = { ...gesture.curve, snap_first: pitchEdit.snapFirst };
    dispatchGesture({ type: 'note/pitch', id: gesture.noteId, pitch });
    dispatch({ type: 'edit/move' });
  };

  const onPointerDown = (event, { x, y }) => {
    if (y < ROLL.topRuler) {
      dispatch({ type: 'transport/seek', ticks: Math.max(0, (x - ROLL.gridLeft + scroll.x) / ROLL.beatWidth * TICKS_PER_BEAT) });
      return;
    }
    const worldX = x + scroll.x;
    const hit = rollHitNote(notesRef.current, worldX, y + scroll.y);
      if (tool === 'Eraser') {
      gesture.current = { kind: 'erase' };
      if (hit) { beginGesture('Erase notes'); dispatchGesture({ type: 'note/remove', ids: [hit.note.id] }); }
      return;
    }
    if (tool === 'Pen') {
      const { tickF, tone } = rollReadPointer(worldX, y + scroll.y);
      // A pen drag is one undo entry, like an OpenUtau draw gesture.
      beginGesture('Draw notes');
      dispatchGesture({ type: 'note/add', partId: state.activePartId, tick: snapTick(tickF, snap), tone, durationTicks: TICKS_PER_BEAT, lyric: 'あ' });
      gesture.current = { kind: 'draw' };
      return;
    }
    if (tool === 'Split') {
      const { tickF } = rollReadPointer(worldX, y + scroll.y);
      if (hit) {
        // Resize + add in one transaction: a split must undo as a single cut.
        beginGesture('Split note');
        gesture.current = { kind: 'split' };
        dispatch({ type: 'note/split', group: true, id: hit.note.id, note: hit.note, tick: snapTick(tickF, snap) });
      }
      return;
    }
    if (tool === 'Vibrato') {
      // Toggle the note's engine-owned UVibrato value.
      if (hit) dispatch({ type: 'note/vibrato', ids: [hit.note.id], patch: hit.note.vibrato?.on ? null : { ...VIBRATO_DEFAULTS, on: true } });
      return;
    }
    if (tool === 'Pitch') {
      // Retune in semitones without moving the note in time; dragging empty
      // space retunes the whole selection.
      const targets = hit ? [hit.note] : notesRef.current.filter(n => selection.has(n.id));
      if (!targets.length) return;
      gesture.current = { kind: 'pitch', anchors: targets.map(n => ({ id: n.id, tone: n.tone })), fromY: y + scroll.y, moved: false };
      beginGesture('Retune notes');
      return;
    }
    if (tool === 'PitchCurve') {
      // Freeform pitch editing, as OpenUtau's pitch editor: a press on empty
      // space paints points, a press on a point grabs it, alt-click deletes it,
      // and ctrl/cmd-click paints even when it starts on a point. One of these
      // is one `note.set_pitch`.
      const worldY = y + scroll.y;
      const note = pitchHitNote(worldX, worldY);
      if (!note) return;
      dispatch({ type: 'notes/select', ids: [note.id], additive: false });
      // `under` is the point the press landed on. Alt deletes it; ctrl/cmd paints
      // even when the press is on a point, so a point can always be added where
      // one already is.
      const under = hitPitchPoint(note, worldX, worldY);
      const grabbed = event.ctrlKey || event.metaKey ? -1 : under;
      beginGesture('Edit pitch curve');
      const base = pitchOf(note, bpm);
      if (under >= 0 && event.altKey) {
        gesture.current = { kind: 'pitchCurve', noteId: note.id, note, index: -1, painting: false, curve: removePitchPoint(base, under) };
        commitPitch(gesture.current);
        return;
      }
      const painting = grabbed < 0;
      gesture.current = { kind: 'pitchCurve', noteId: note.id, note, index: grabbed, painting, lastTick: null, curve: base };
      if (painting) {
        const sample = pitchSampleAt(note, worldX, worldY);
        gesture.current.curve = paintPitchPoints(base, [sample], { shape: pitchEdit.shape, durationMs: pitchMs(note) });
        gesture.current.lastTick = sample.tick;
        commitPitch(gesture.current);
      }
      return;
    }
    if (hit) {
      const additive = event.shiftKey || event.metaKey || event.ctrlKey;
      if (hit.resize) {
        dispatch({ type: 'notes/select', ids: [hit.note.id], additive: false });
        gesture.current = { kind: 'resize', noteId: hit.note.id, fromStart: hit.fromStart, anchorTicks: hit.note.position_ticks, anchorDuration: hit.note.duration_ticks, fromX: worldX, moved: false };
        beginGesture('Resize note');
      } else {
        let ids;
        if (additive) {
          const next = new Set(selection);
          if (next.has(hit.note.id)) next.delete(hit.note.id); else next.add(hit.note.id);
          ids = [...next];
        } else if (selection.has(hit.note.id)) {
          ids = [...selection];
        } else {
          ids = [hit.note.id];
        }
        dispatch({ type: 'notes/select', ids, additive });
        const anchors = notesRef.current.filter(n => ids.includes(n.id)).map(n => ({ id: n.id, ticks: n.position_ticks, tone: n.tone }));
        gesture.current = { kind: 'move', anchors, fromX: worldX, fromY: y + scroll.y, moved: false };
        beginGesture('Move notes');
      }
      return;
    }
    // Empty space: marquee select, matching OpenUtau's drag selection.
    gesture.current = { kind: 'marquee', x, y };
    setMarquee({ x, y, current: x, currentY: y });
    dispatch({ type: 'notes/select', ids: [], additive: false });
  };

  const onPointerMove = (event, { x, y }) => {
    const active = gesture.current;
    if (!active) {
      // Hover affordance only. OpenUtau reserves it for the cursor tool, so a
      // Pen/Eraser/Pitch gesture keeps its own cursor.
      if (tool !== 'Select') {
        // The pitch editor still needs to say when a point is under the cursor,
        // otherwise a 3px circle is impossible to tell from the curve line.
        if (tool === 'PitchCurve') {
          const note = pitchHitNote(x + scroll.x, y + scroll.y);
          setHover(note && hitPitchPoint(note, x + scroll.x, y + scroll.y) >= 0 ? 'move' : null);
          return;
        }
        setHover(null);
        return;
      }
      const hit = rollHitNote(notesRef.current, x + scroll.x, y + scroll.y);
      setHover(hit ? (hit.resize ? 'ew-resize' : 'move') : null);
      return;
    }
    const worldX = x + scroll.x, worldY = y + scroll.y;
    if (active.kind === 'marquee') {
      setMarquee(current => current && { ...current, current: x, currentY: y });
      return;
    }
    if (active.kind === 'erase') {
      const hit = rollHitNote(notesRef.current, worldX, worldY);
      if (hit) dispatchGesture({ type: 'note/remove', ids: [hit.note.id] });
      return;
    }
    if ((active.kind === 'move' || active.kind === 'resize') && !active.moved) {
      if (Math.abs(x - active.fromX + scroll.x) < DRAG_THRESHOLD && Math.abs(worldY - active.fromY) < DRAG_THRESHOLD) return;
      active.moved = true;
    }
    if (active.kind === 'move') {
      // Snap the gesture target once and apply the same shift to every note in
      // the selection, so a group drag keeps its internal rhythm. Positions are
      // absolute: coalescing may drop frames, and a dropped frame must not
      // leave the note behind (engine.js turns these into §6 deltas).
      const dTicks = snapTick(active.anchors[0].ticks + (worldX - active.fromX) / ROLL.beatWidth * TICKS_PER_BEAT, snap) - active.anchors[0].ticks;
      const dTone = Math.round(ROLL.maxTone - (worldY - ROLL.topRuler) / ROW) - active.anchors[0].tone;
      dispatchGesture({
        type: 'note/move',
        updates: active.anchors.map(a => ({ id: a.id, position_ticks: Math.max(0, a.ticks + dTicks), tone: Math.max(0, Math.min(127, a.tone + dTone)) })),
      });
      dispatch({ type: 'edit/move' });
      return;
    }
    if (active.kind === 'pitch') {
      if (!active.moved && Math.abs(worldY - active.fromY) < DRAG_THRESHOLD) return;
      active.moved = true;
      // One row is one semitone, so the retune stays musical while dragging.
      const dTone = Math.round((active.fromY - worldY) / ROW);
      dispatchGesture({ type: 'note/tone', updates: active.anchors.map(a => ({ id: a.id, tone: Math.max(0, Math.min(127, a.tone + dTone)) })) });
      dispatch({ type: 'edit/move' });
      return;
    }
    if (active.kind === 'pitchCurve') {
      const note = active.note;
      const durationMs = pitchMs(note);
      if (active.painting) {
        const sample = pitchSampleAt(note, worldX, worldY);
        // One point per grid slot, so the density is the project's and not the
        // pointer's speed. Two moves inside the same 5-tick slot add nothing.
        if (active.lastTick !== null && sample.tick === active.lastTick) return;
        active.curve = paintPitchPoints(active.curve, [sample], { shape: pitchEdit.shape, durationMs });
        active.lastTick = sample.tick;
        commitPitch(active);
        return;
      }
      if (active.index < 0) return;
      active.curve = movePitchPoint(active.curve, active.index, pitchSampleAt(note, worldX, worldY), { durationMs });
      commitPitch(active);
      return;
    }
    if (active.kind === 'resize') {
      const dTicks = (worldX - active.fromX) / ROLL.beatWidth * TICKS_PER_BEAT;
      if (active.fromStart) {
        const start = Math.min(snapTick(active.anchorTicks + dTicks, snap, dTicks < 0), active.anchorTicks + active.anchorDuration - MIN_NOTE_TICKS);
        dispatchGesture({ type: 'note/resize', updates: [{ id: active.noteId, position_ticks: Math.max(0, start), duration_ticks: active.anchorTicks + active.anchorDuration - Math.max(0, start) }] });
      } else {
        const end = snapTick(active.anchorTicks + active.anchorDuration + dTicks, snap, dTicks > 0);
        dispatchGesture({ type: 'note/resize', updates: [{ id: active.noteId, position_ticks: active.anchorTicks, duration_ticks: Math.max(MIN_NOTE_TICKS, end - active.anchorTicks) }] });
      }
      dispatch({ type: 'edit/move' });
    }
  };

  const onPointerUp = (event, { x, y }) => {
    const active = gesture.current;
    gesture.current = null;
    setMarquee(null);
    if (!active) return;
    if (active.kind === 'marquee') {
      const left = Math.min(active.x, x), right = Math.max(active.x, x);
      const top = Math.min(active.y, y), bottom = Math.max(active.y, y);
      const ids = notesRef.current.filter(note => {
        const rect = rollNoteRect(note);
        return rect.x - scroll.x < right && rect.x + rect.w - scroll.x > left && rect.y - scroll.y < bottom && rect.y + rect.h - scroll.y > top;
      }).map(note => note.id);
      dispatch({ type: 'notes/select', ids, additive: event.shiftKey || event.metaKey || event.ctrlKey });
      return;
    }
    // One release closes the gesture's engine transaction.
    if (['erase', 'move', 'resize', 'pitch', 'pitchCurve', 'draw', 'split'].includes(active.kind)) endGesture(true);
  };

  /**
   * Right-click changes one point's shape, which is OpenUtau's per-point shape
   * menu reduced to a cycle — the shapes are a closed set of five, so a cycle
   * reaches all of them without a menu that would need its own hit testing on
   * top of a canvas that cannot host one.
   */
  const onContextMenu = event => {
    if (tool !== 'PitchCurve') return;
    event.preventDefault();
    const at = point(event);
    const hit = rollHitNote(notesRef.current, at.x + scroll.x, at.y + scroll.y);
    if (!hit) return;
    const index = hitPitchPoint(hit.note, at.x + scroll.x, at.y + scroll.y);
    if (index < 0) return;
    beginGesture('Change pitch point shape');
    commitPitch({ noteId: hit.note.id, curve: cyclePitchShape(pitchOf(hit.note, bpm), index) });
    endGesture(true);
  };

  const toolCursor = tool === 'Pen' || tool === 'PitchCurve' ? 'crosshair' : tool === 'Eraser' ? 'not-allowed' : tool === 'Pitch' ? 'ns-resize' : tool === 'Split' ? 'col-resize' : 'default';
  const cursor = hoverCursor || toolCursor;
  // The gesture layer is a plain DOM overlay on top of the canvas: Pixi's own
  // hit testing depends on the canvas being the event target, which breaks as
  // soon as the dock resizes or a panel overlaps it.
  const point = event => {
    const rect = event.currentTarget.getBoundingClientRect();
    return { x: event.clientX - rect.left, y: event.clientY - rect.top };
  };
  const box = marquee && {
    left: Math.min(marquee.x, marquee.current), top: Math.min(marquee.y, marquee.currentY),
    width: Math.abs(marquee.current - marquee.x), height: Math.abs(marquee.currentY - marquee.y),
  };
  return <div className="ou-roll-input" style={{ cursor }} role="application" aria-label="Piano roll grid"
    onPointerDown={event => { event.currentTarget.setPointerCapture?.(event.pointerId); onPointerDown(event, point(event)); }}
    onPointerMove={event => onPointerMove(event, point(event))}
    onPointerUp={event => onPointerUp(event, point(event))}
    onPointerCancel={event => onPointerUp(event, point(event))}
    onContextMenu={onContextMenu}
    onPointerLeave={() => setHover(null)}>
    {box && <span className="ou-marquee" style={box} />}
  </div>;
}

const TOOLS = [
  { id: 'Select', icon: '⌖', title: 'Selection — click, shift-click, drag' },
  { id: 'Pen', icon: '✎', title: 'Pen — click to draw a note' },
  { id: 'Eraser', icon: '⌫', title: 'Eraser — drag to erase notes' },
  { id: 'Split', icon: '⌁', title: 'Split — click a note to cut it in two' },
  { id: 'Pitch', icon: '∿', title: 'Pitch — drag up/down to retune in semitones' },
  {
    id: 'PitchCurve', icon: '⌇',
    title: 'Pitch curve — drag across a note to draw pitch points, drag a point to shape it, alt-click to delete',
  },
  { id: 'Vibrato', icon: '〜', title: 'Vibrato — click a note to toggle its vibrato patch' },
];

/**
 * The roll's horizontal scrollbar, OpenUtau's strip under the grid.
 *
 * The roll cannot use a DOM scroll container: the keyboard and shortcut columns
 * have to stay pinned while the grid moves, so the offset lives in state and is
 * driven by the wheel. That leaves the offset with no visible affordance — this
 * is it, and dragging it is the other way to move along the part.
 */
function RollScrollbar({ size, scroll, setScroll, contentTicks }) {
  const trackRef = useRef(null);
  const drag = useRef(null);
  const visible = Math.max(0, size.width - ROLL.gridLeft);
  const content = contentTicks / TICKS_PER_BEAT * ROLL.beatWidth;
  // The scrollable range, not a clamped offset: `clampScrollX` answers “is this
  // offset legal”, the bar needs “how far can it go”.
  const maxScroll = Math.max(0, content - visible);
  const trackWidth = visible;
  const thumbWidth = content > visible ? Math.max(28, trackWidth * visible / content) : trackWidth;
  const travel = Math.max(1, trackWidth - thumbWidth);
  const thumbLeft = maxScroll > 0 ? (scroll.x / maxScroll) * travel : 0;
  const scrollable = maxScroll > 0;
  const point = event => {
    const rect = trackRef.current.getBoundingClientRect();
    return { x: event.clientX - rect.left - ROLL.gridLeft, y: event.clientY - rect.top };
  };
  const onPointerDown = event => {
    if (!scrollable) return;
    event.currentTarget.setPointerCapture?.(event.pointerId);
    const { x } = point(event);
    // A press on the track jumps the thumb under the pointer; a press on the
    // thumb itself keeps its grab offset, as a scrollbar does.
    const grab = x >= thumbLeft && x <= thumbLeft + thumbWidth ? x - thumbLeft : thumbWidth / 2;
    drag.current = { grab };
    // `scroll` is the roll's `{ x, y }` viewport, not a bare offset: handing
    // `setScroll` a number replaces the object and every drawing computation
    // downstream goes NaN.
    const toOffset = px => clampScrollX(((px - grab) / travel) * maxScroll, size.width, contentTicks);
    setScroll(current => (current.x === toOffset(x) ? current : { ...current, x: toOffset(x) }));
  };
  const onPointerMove = event => {
    if (!drag.current) return;
    const { x } = point(event);
    const next = clampScrollX(((x - drag.current.grab) / travel) * maxScroll, size.width, contentTicks);
    setScroll(current => (current.x === next ? current : { ...current, x: next }));
  };
  const onPointerUp = event => {
    event.currentTarget.releasePointerCapture?.(event.pointerId);
    drag.current = null;
  };
  return <div className={`ou-roll-hbar ${scrollable ? '' : 'idle'}`} ref={trackRef} role="scrollbar"
    aria-label="Scroll the piano roll sideways" aria-valuemin={0} aria-valuemax={maxScroll} aria-valuenow={Math.round(scroll.x)}
    onPointerDown={onPointerDown} onPointerMove={onPointerMove} onPointerUp={onPointerUp} onPointerCancel={onPointerUp}>
    <span className="ou-roll-thumb" style={{ width: thumbWidth, left: ROLL.gridLeft + thumbLeft }} />
  </div>;
}

export function PianoRollPanel() {
  const { state, dispatch, beginGesture, endGesture, activePart, activeNotes, tracks, bpm, waveform, connected } = useClone();
  const pitchEdit = state.pitchEdit ?? { shape: DEFAULT_PITCH_SHAPE, snapFirst: true };
  // The pitch curve of every note that has one, keyed by id. Read from the
  // engine snapshot, so this is the stored curve and never a local edit.
  const pitchCurves = useMemo(
    () => new Map(activeNotes.filter(note => note.pitch?.data?.length).map(note => [note.id, pitchForDisplay(note, bpm)])), 
    [activeNotes, bpm],
  );
  const waveformState = state.waveform ?? WAVEFORM_DEFAULT;
  const showWaveform = state.showWaveform !== false;
  const { tool, snap, selection, edit } = state;
  const partTicks = activePart?.duration_ticks ?? TICKS_PER_BEAT * 4;
  // Scroll across the whole project's timeline (not only this part) so the Roll
  // can follow Arrangement even when another track extends farther.
  const contentTicks = Math.max(projectTicks(tracks), (activePart?.position_ticks ?? 0) + partTicks);
  const timelineScrollTicks = state.timelineScrollTicks ?? 0;
  const { hostRef, size, scroll, setScroll } = useViewportGeometry(contentTicks, timelineScrollTicks, dispatch);
  useFollowPart(activePart?.id, activeNotes, size, setScroll, contentTicks, activePart?.position_ticks ?? 0);
  // Keep this as the single auto-follow authority while playing; Arrangement
  // derives its viewport from the same project tick and therefore follows it.
  useEffect(() => {
    if (!state.playing) return;
    const x = ROLL.gridLeft + state.playheadTicks / TICKS_PER_BEAT * ROLL.beatWidth;
    const margin = Math.min(96, size.width * 0.15);
    if (x - scroll.x > size.width - margin) {
      const next = clampScrollX(x - margin, size.width, contentTicks);
      setScroll(current => current.x === next ? current : { ...current, x: next });
    } else if (x - scroll.x < ROLL.gridLeft + margin) {
      const next = clampScrollX(x - ROLL.gridLeft - margin, size.width, contentTicks);
      setScroll(current => current.x === next ? current : { ...current, x: next });
    }
  }, [state.playing, state.playheadTicks, size.width, scroll.x, contentTicks, setScroll]);
  const pixiRef = useRef(null);
  useRendererSize(pixiRef, size.width, size.height);
  useRenderOnDemand(pixiRef);
  // Dev-only viewport handle, like the engine's. The viewport decides whether an
  // imported part's notes are on screen at all, so the probe reads it rather than
  // inferring it from a canvas it cannot see into.
  if (import.meta.env?.DEV) window.__ouRoll = { get scroll() { return scroll; }, size, partTicks, setScroll };
  const editHint = edit.kind === 'move' ? 'Move notes — one undo step on release'
    : edit.kind === 'resize' ? 'Resize note — one undo step on release'
      : edit.kind === 'Edit pitch curve' ? 'Draw pitch curve — one undo step on release'
        : null;
  /**
   * Clear the pitch curve of every selected note: the flat two-point curve the
   * engine renders when a note has none. One group, so a five-note clear is one
   * undo step rather than five.
   */
  const clearPitchCurves = () => {
    const targets = activeNotes.filter(note => selection.has(note.id));
    if (!targets.length) return;
    beginGesture('Clear pitch curves');
    for (const note of targets) {
      dispatch({ type: 'note/pitch', group: true, id: note.id, pitch: flatPitch(note, bpm) });
    }
    endGesture(true);
  };
  const theme = useTheme();
  return <div className="ou-roll">
    <div className="ou-roll-main">
      <div className="ou-tool-rail">
        {TOOLS.map(entry => (
          <button key={entry.id} type="button" className={`ou-tool-btn ${tool === entry.id ? 'on' : ''}`}
            title={entry.title}
            aria-pressed={tool === entry.id}
            onClick={() => dispatch({ type: 'ui/tool', tool: entry.id })}>{entry.icon}</button>
        ))}
        <select className="ou-snap-select" value={snap} aria-label="Snap" onChange={event => dispatch({ type: 'ui/snap', snap: event.target.value })}>
          {Object.keys({ '1/4': 1, '1/8': 1, '1/16': 1, '1/32': 1, Off: 1 }).map(unit => <option key={unit} value={unit}>{unit}</option>)}
        </select>
        {/* OpenUtau toggles note labels between lyric and phoneme text. */}
        <button type="button" className={`ou-label-toggle ${state.noteLabels}`} title="Show lyric or phoneme text on notes"
          onClick={() => dispatch({ type: 'ui/labels', labels: state.noteLabels === 'lyric' ? 'phoneme' : 'lyric' })}>
          {state.noteLabels === 'lyric' ? 'あ' : 'a'}
        </button>
      </div>
      <div className="ou-roll-canvas" ref={hostRef}>
        {/* Status sits over the canvas instead of taking a column of dock width. */}
        <div className="ou-roll-status">
          <strong>{activePart?.name || 'No part'}</strong>
          <span>{activeNotes.length} note{activeNotes.length === 1 ? '' : 's'}{selection.size ? ` · ${selection.size} selected` : ''}</span>
          <button type="button" className={`ou-wave-toggle ${showWaveform ? 'on' : ''}`} aria-pressed={showWaveform}
            title={showWaveform ? 'Hide the waveform overlay' : 'Show the waveform overlay'}
            onClick={() => dispatch({ type: 'waveform/toggle', visible: !showWaveform })}>〜 wave</button>
          {editHint && <span className="ou-roll-edit">{editHint}</span>}
        </div>
        {/* The pitch editor's options. OpenUtau keeps these next to the curve
            it is drawing rather than in a dialog, and so does this: the shape
            picker decides the shape of the next point painted. */}
        {tool === 'PitchCurve' && (
          <div className="ou-pitch-bar" role="toolbar" aria-label="Pitch curve options">
            <span className="ou-pitch-label">Pitch curve</span>
            {PITCH_SHAPES.map(entry => (
              <button key={entry.id} type="button" className={`ou-pitch-shape ${pitchEdit.shape === entry.id ? 'on' : ''}`}
                title={`${entry.name} — ${entry.hint}`} aria-pressed={pitchEdit.shape === entry.id}
                onClick={() => dispatch({ type: 'ui/pitch-shape', shape: entry.id })}>{entry.id}</button>
            ))}
            <label className="ou-pitch-snap" title="Store snap_first with the curve, so the first point is pulled toward the note tone">
              <input type="checkbox" checked={pitchEdit.snapFirst}
                onChange={event => dispatch({ type: 'ui/pitch-snap-first', snapFirst: event.target.checked })} />
              snap first
            </label>
            <button type="button" className="ou-pitch-clear" disabled={!selection.size}
              title="Reset the selected notes to a flat pitch curve"
              onClick={clearPitchCurves}>clear</button>
          </div>
        )}
        <PixiApplication ref={pixiRef} width={size.width} height={size.height} background={theme.rollBg} antialias preference="webgl">
          <KeyboardCanvas size={size} scroll={scroll} appRef={pixiRef} />
          <RollCanvas notes={activeNotes} selection={selection} partId={state.activePartId} playheadTicks={state.playheadTicks} partTicks={partTicks} size={size} scroll={scroll} pitchCurves={pitchCurves} bpm={bpm} appRef={pixiRef} />
          <NoteLyrics notes={activeNotes} scroll={scroll} mode={state.noteLabels} />
          <ToneLabels size={size} scroll={scroll} />
          <ShortcutColumn size={size} scroll={scroll} />
          <MeasureNumbers size={size} scroll={scroll} />
        </PixiApplication>
        <RollInteraction size={size} scroll={scroll} />
        {showWaveform && (
          <WaveformOverlay
            size={size}
            scroll={scroll}
            partTicks={partTicks}
            placement={waveformState}
            peaks={waveform?.peaks}
            bpm={bpm}
            status={waveform?.status}
            error={waveform?.error}
            source={waveform?.source}
            stats={waveform?.stats}
            bank={waveform?.bank}
            onPlacement={patch => dispatch({ type: 'waveform/placement', patch })}
            onClose={() => dispatch({ type: 'waveform/toggle', visible: false })}
          />
        )}
        <RollScrollbar size={size} scroll={scroll} setScroll={setScroll} contentTicks={contentTicks} />
      </div>
    </div>
  </div>;
}
