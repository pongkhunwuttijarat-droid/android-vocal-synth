// Arrangement panel: OpenUtau-style track header cards (portrait, name,
// singer, M/S/fx, volume fader) beside a PixiJS-drawn parts timeline.
// Clicking a part selects it and opens it in the Piano Roll panel; clicking an
// empty lane at a snapped measure creates a part through the Rust session.
import React, { useEffect, useRef, useState } from 'react';
import { Application as PixiApplication, extend } from '@pixi/react';
import { Container, Graphics, Text } from 'pixi.js';
import { OpfsBank } from '../../../../web/samples.js';
import { loadSingerChoices } from './singer-choices.js';
import { ARR, arrContentMeasures, arrFirstMeasure, arrHitPart, arrLaneAt, arrMeasureWidth, arrNoteRect, arrPartRect, arrTickAt, arrVisibleMeasures, arrWaveformTimeRange, arrZoomScrollX, clampArrScrollX } from './arrangement-geometry.js';
import { TICKS_PER_MEASURE, projectTicks, useClone, volumePercent } from './clone-state.jsx';
import { useElementSize, useRendererSize, useRenderOnDemand } from './use-element-size.js';
import { P, pixi, trackColor, useTheme } from './palette.js';
import { scrollOffsetToTicks, ticksToScrollOffset } from './timeline-scroll.js';
import { TrackFxDialog } from './track-fx-dialog.jsx';
import { buildPeaks, peaksInRange } from './waveform-source.js';
import { audioWaveformCacheKey } from './audio-waveform-cache.js';

extend({ Container, Graphics, Text });

const { rulerHeight, rowHeight } = ARR;
/**
 * Vertical room left under the lanes for the timeline's own scrollbar. The
 * scroll container is exactly as tall as the canvas it holds, so without this
 * the bar would eat the bottom of the last lane.
 */
const SCROLLBAR_ROOM = 16;

function TimelineCanvas({ width, height, measureWidth, scrollX, tracks, activePartId, waveform, audioWaveforms, notes, bpm, playheadTicks, appRef }) {
  const theme = useTheme();
  const draw = React.useCallback(g => {
    g.clear();
    const t = P();
    const activeTrackIndex = tracks.findIndex(track => (track.parts || []).some(part => part.id === activePartId));
    // `@pixi/react` only applies `background` when the app mounts, so the canvas
    // paints its own backdrop — which also makes a theme switch repaint it. The
    // renderer's own size is used so an unpainted strip after a dock resize can
    // never show the mount-time clear colour.
    const screen = appRef?.current?.getApplication?.()?.screen;
    g.rect(0, 0, Math.max(width, screen?.width ?? 0), Math.max(height, screen?.height ?? 0)).fill(t.surfaceAlt);
    // Only the measures under the viewport are drawn: the timeline is as long
    // as the song, so walking all of them would grow with the document.
    const firstMeasure = arrFirstMeasure(scrollX, measureWidth);
    const lastMeasure = firstMeasure + arrVisibleMeasures(width, measureWidth);
    for (let track = 0; track < tracks.length; track++) {
      const y = rulerHeight + track * rowHeight;
      // The lane holding the open part is tinted, as in OpenUtau's arrangement.
      g.rect(0, y, width, rowHeight).fill({ color: pixi(track === activeTrackIndex ? t.arrRowActive : track % 2 ? t.arrRowB : t.arrRowA) });
      for (let m = firstMeasure; m <= lastMeasure; m++) {
        const x = m * measureWidth - scrollX;
        if (x > width) break;
        g.moveTo(x, y).lineTo(x, y + rowHeight).stroke({ color: pixi(m % 4 === 0 ? t.arrMeasure : t.arrBeat), width: 1 });
      }
    }
    // Ruler
    g.rect(0, 0, width, rulerHeight).fill(t.arrRuler);
    g.rect(0, rulerHeight - 1, width, 1).fill(t.arrRulerLine);
    for (let m = firstMeasure; m < lastMeasure; m++) {
      const x = m * measureWidth - scrollX;
      if (x > width) break;
      g.moveTo(x, 0).lineTo(x, rulerHeight).stroke({ color: pixi(m % 4 === 0 ? t.arrRulerStrong : t.arrRulerWeak), width: 1 });
    }
    // Part blocks. The active track tints its parts, as OpenUtau does.
    tracks.forEach((track, trackIndex) => {
      const trackFill = trackColor(trackIndex).accent;
      (track.parts || []).forEach(part => {
        const { x, y, w, h } = arrPartRect(part, trackIndex, measureWidth, scrollX);
        if (x > width || x + w < 0) return;
        const active = part.id === activePartId;
        const onActiveTrack = trackIndex === activeTrackIndex;
        const color = active ? t.partFillActive : onActiveTrack ? trackFill : t.partFillMuted;
        g.roundRect(x, y, w, h, 6).fill(color).stroke({ color: pixi(active ? t.partStrokeActive : onActiveTrack ? t.partStroke : t.partStroke), width: active ? 1.5 : 1 });
        // Audio parts use a real engine-rendered envelope keyed by track ID;
        // this remains visible even when a synth part is the selected preview.
        const trackWaveform = audioWaveforms?.[track.id]?.status === 'ready'
          ? audioWaveforms[track.id]
          : waveform?.status === 'ready' && waveform.trackId === track.id ? waveform : null;
        if (part.kind === 'wave' && trackWaveform?.peaks && bpm > 0) {
          const left = Math.max(0, Math.ceil(x + 8));
          const right = Math.min(width, Math.floor(x + w - 6));
          const top = y + 24;
          const bottom = y + h - 8;
          const center = (top + bottom) / 2;
          const amplitude = Math.max(1, (bottom - top) / 2 - 1);
          const waveColor = pixi(t.partTextActive);
          if (right > left) {
            g.moveTo(left, center).lineTo(right, center).stroke({ color: waveColor, alpha: 0.22, width: 1 });
            for (let px = left; px < right; px += 2) {
              // Render samples are aligned to project time; sample the envelope
              // at the tick range represented by this screen column.
              const time = arrWaveformTimeRange(px, 2, measureWidth, scrollX, bpm);
              const range = time && peaksInRange(trackWaveform.peaks, time.fromSeconds, time.toSeconds);
              if (!range) continue;
              const lo = Math.max(-1, Math.min(1, range[0]));
              const hi = Math.max(-1, Math.min(1, range[1]));
              g.moveTo(px, center - hi * amplitude).lineTo(px, center - lo * amplitude)
                .stroke({ color: waveColor, alpha: active ? 0.95 : 0.72, width: 1 });
            }
          }
        }
        // Synth blocks get a tiny pitch contour: note time and tone stay
        // authoritative in the Rust note snapshot; this is presentation only.
        if (part.kind === 'voice') {
          const partNotes = notes?.[part.id] || part.notes || [];
          if (partNotes.length) {
            let minTone = Infinity;
            let maxTone = -Infinity;
            for (const note of partNotes) {
              const tone = Number(note.tone);
              if (!Number.isFinite(tone)) continue;
              minTone = Math.min(minTone, tone);
              maxTone = Math.max(maxTone, tone);
            }
            if (Number.isFinite(minTone) && Number.isFinite(maxTone)) {
              const padding = Math.max(2, Math.ceil((maxTone - minTone) * 0.08));
              const pitchRange = { low: Math.max(0, minTone - padding), high: Math.min(127, maxTone + padding) };
              const partOffset = Number(part.position_ticks) || 0;
              const visibleFrom = scrollX / measureWidth * TICKS_PER_MEASURE - partOffset;
              const visibleTo = (scrollX + width) / measureWidth * TICKS_PER_MEASURE - partOffset;
              const noteColor = pixi(active ? t.partTextActive : t.partText);
              for (const note of partNotes) {
                const noteStart = Number(note.position_ticks);
                const noteEnd = noteStart + Number(note.duration_ticks);
                if (!Number.isFinite(noteStart) || !Number.isFinite(noteEnd) || noteEnd <= visibleFrom || noteStart >= visibleTo) continue;
                const noteRect = arrNoteRect(part, note, trackIndex, measureWidth, scrollX, pitchRange);
                if (!noteRect) continue;
                const noteLeft = Math.max(0, x + 4, noteRect.x);
                const noteRight = Math.min(width, x + w - 4, noteRect.x + noteRect.w);
                const noteTop = Math.max(y + 4, noteRect.y);
                const noteBottom = Math.min(y + h - 4, noteRect.y + noteRect.h);
                const noteWidth = noteRight - noteLeft;
                const noteHeight = noteBottom - noteTop;
                if (noteWidth <= 0 || noteHeight <= 0) continue;
                g.roundRect(noteLeft, noteTop, noteWidth, noteHeight, Math.min(2, noteHeight / 2))
                  .fill({ color: noteColor, alpha: active ? 0.94 : 0.78 });
              }
            }
          }
        }
        // OpenUtau draws a small handle bar at the part's left edge.
        g.rect(x + 10, y + h - 10, Math.min(28, w - 20), 5).fill({ color: pixi(t.partShelf), alpha: active ? 0.9 : 0.6 });
      });
    });
    const playheadX = playheadTicks / TICKS_PER_MEASURE * measureWidth - scrollX;
    if (playheadX >= 0 && playheadX <= width) {
      g.moveTo(playheadX, 0).lineTo(playheadX, height).stroke({ color: pixi(t.playhead), width: 2, alpha: 0.9 });
    }
  }, [width, height, measureWidth, scrollX, tracks, activePartId, waveform, audioWaveforms, notes, bpm, playheadTicks, theme, appRef]);
  return <pixiGraphics draw={draw} eventMode="none" />;
}

function PartLabels({ measureWidth, scrollX, width, tracks, activePartId }) {
  const theme = useTheme();
  return tracks.flatMap((track, trackIndex) => (track.parts || []).map(part => {
    const { x, y, w } = arrPartRect(part, trackIndex, measureWidth, scrollX);
    // A label for a block scrolled off either edge would draw over the wrong
    // lane, or spill past the canvas it is clipped to.
    if (x > width || x + w < 0) return null;
    const active = part.id === activePartId;
    const label = part.kind === 'wave' ? `♫ ${part.name || 'Audio clip'}` : part.name;
    return <pixiText key={part.id} text={label} x={x + 10} y={y + 6} style={{ fontFamily: 'Segoe UI, sans-serif', fontSize: 12, fill: active ? theme.partTextActive : theme.partText, fontWeight: 600 }} eventMode="none" />;
  }));
}

function MeasureNumbers({ measureWidth, scrollX, width }) {
  const theme = useTheme();
  const firstMeasure = arrFirstMeasure(scrollX, measureWidth);
  return Array.from({ length: arrVisibleMeasures(width, measureWidth) }, (_, n) => {
    const m = firstMeasure + n;
    return <pixiText key={m} text={String(m + 1)} x={m * measureWidth - scrollX + 6} y={6} style={{ fontFamily: 'Segoe UI, sans-serif', fontSize: 11, fill: theme.arrNumber }} eventMode="none" />;
  });
}

/** Transparent hit layer: maps canvas pixels to parts / lanes. */
function TimelineInteraction({ measureWidth, scrollX }) {
  const { tracks, state, dispatch } = useClone();
  const onPointerDown = (event, { x, y }) => {
    const hit = arrHitPart(tracks, x, y, measureWidth, scrollX);
    if (hit) {
      dispatch({ type: 'part/select', partId: hit.part.id, trackId: tracks[hit.trackIndex].id });
      return;
    }
    const lane = arrLaneAt(y);
    if (lane < 0 || lane >= tracks.length) return;
    dispatch({ type: 'part/add', trackId: tracks[lane].id, tick: arrTickAt(x, measureWidth, scrollX), durationTicks: TICKS_PER_MEASURE * 4 });
  };
  const onPointerMove = (event, { x, y }) => {
    const hit = arrHitPart(tracks, x, y, measureWidth, scrollX);
    const overLane = arrLaneAt(y) >= 0 && arrLaneAt(y) < tracks.length;
    event.currentTarget.cursor = hit || overLane ? 'pointer' : 'default';
  };
  // Plain DOM overlay on top of the Pixi canvas: the gesture math lives in
  // clone-state, so the layer only converts client coordinates to canvas pixels.
  const point = event => {
    const rect = event.currentTarget.getBoundingClientRect();
    return { x: event.clientX - rect.left, y: event.clientY - rect.top };
  };
  return <div className="ou-arr-input" role="application" aria-label="Arrangement timeline"
    onPointerDown={event => onPointerDown(event, point(event))}
    onPointerMove={event => onPointerMove(event, point(event))} />;
}

/**
 * Per-track singer, sent as `track.set_singer`.
 *
 * The choices are host-installed voicebanks (not project state). Keep an
 * imported singer name as a disabled/unavailable choice when its bank is not
 * installed, so the UI still displays the project's value without pretending it
 * can render with that bank.
 */
function SingerPicker({ track }) {
  const { dispatch, ui, backendKind, baseUrl } = useClone();
  const [choices, setChoices] = useState(null);
  useEffect(() => {
    let cancelled = false;
    loadSingerChoices({
      backendKind,
      baseUrl,
      getInstalledRoots: () => OpfsBank.installedRoots(),
    })
      .then(nextChoices => { if (!cancelled) setChoices(nextChoices); })
      .catch(() => { if (!cancelled) setChoices([]); });
    return () => { cancelled = true; };
  }, [backendKind, baseUrl, ui.bankEpoch]);

  const current = track.singer_id ?? '';
  const available = choices || [];
  const currentIsInstalled = available.some(choice => choice.id.toLowerCase() === current.toLowerCase());
  return <span className="ou-singer-select">
    <select aria-label={`Singer for ${track.name}`} title="Singer for this track"
      value={current} onChange={event => dispatch({
        type: 'track/singer', trackId: track.id, singerId: event.target.value || null,
      })}>
      <option value="">Unassigned singer</option>
      {current && !currentIsInstalled && <option value={current} disabled>{current} (not installed)</option>}
      {available.map(({ id, label }) => <option key={id} value={id}>{label}</option>)}
    </select>
  </span>;
}

function TrackHeaderCard({ track, index, active }) {
  const { dispatch, tracks } = useClone();
  const [fxOpen, setFxOpen] = useState(false);
  const color = trackColor(index).accent;
  const db = track.volume_db ?? 0;
  const pct = volumePercent(db);
  // Clicking the card selects the track: its first part becomes active, which
  // is what makes the transport and the waveform strip follow THAT track. An
  // audio track's part is often one short rectangle near the origin, which is
  // a miserable click target — and without a card click there was no other way
  // to select it, so an audio track could never be previewed or heard.
  const firstPart = (track.parts || [])[0];
  const selectTrack = () => {
    if (!firstPart) return;
    dispatch({ type: 'part/select', partId: firstPart.id, trackId: track.id });
  };
  return <>
  <div
    className={`ou-track-card ${active ? 'active' : ''}`}
    style={{ '--track-color': color }}
    onClick={selectTrack}
    title={firstPart ? `Select ${firstPart.name || 'part'}` : 'This track has no parts yet'}
  >
    <span className="ou-track-badge">{index + 1}</span>
    <span className="ou-track-portrait" title={track.singer_id || 'Unassigned singer'}>{track.singer_id?.slice(0, 1) || '♪'}</span>
    <div className="ou-track-info">
      <strong>{track.name}</strong>
      {/* The engine keeps the real value (null when unset); the placeholder is
          the panel's own, so a missing voicebank never looks like a real one. */}
      <SingerPicker track={track} />
      <small className="ou-track-detail">{track.singer_detail}</small>
    </div>
    <div className="ou-track-side">
      <button type="button" className="ou-side-btn" disabled={index === 0} title="Move track up"
        onClick={event => { event.stopPropagation(); dispatch({ type: 'track/reorder', trackId: track.id, toIndex: index - 1 }); }}>↑</button>
      <button type="button" className="ou-side-btn" disabled={index === tracks.length - 1} title="Move track down"
        onClick={event => { event.stopPropagation(); dispatch({ type: 'track/reorder', trackId: track.id, toIndex: index + 1 }); }}>↓</button>
      <button type="button" className="ou-side-btn ou-track-delete" disabled={tracks.length <= 1} title="Delete track and its parts (undo to restore)"
        onClick={event => { event.stopPropagation(); dispatch({ type: 'track/remove', trackId: track.id }); }}>×</button>
      {/* The engine command is `track.set_mute` / `track.set_solo` and it REQUIRES
          the new boolean — an omitted value is a validation error that kills the
          session, which looks like the document failed to load. These are
          toggles, so they send the inverted current value from the snapshot. */}
      <button type="button" className={`ou-side-btn ${track.mute ? 'on' : ''}`} aria-pressed={track.mute} title="Mute" onClick={event => { event.stopPropagation(); dispatch({ type: 'track/mute', trackId: track.id, mute: !track.mute }); }}>M</button>
      <button type="button" className={`ou-side-btn ${track.solo ? 'on' : ''}`} aria-pressed={track.solo} title="Solo" onClick={event => { event.stopPropagation(); dispatch({ type: 'track/solo', trackId: track.id, solo: !track.solo }); }}>S</button>
      <button type="button" className={`ou-side-btn ${track.mix_fx?.enabled ? 'on' : ''}`} aria-pressed={fxOpen} title="Track effects" onClick={event => { event.stopPropagation(); setFxOpen(true); }}>fx</button>
    </div>
    <div className="ou-track-volume">
      <span className="ou-vol-slider" style={{ '--fill': `${pct * 100}%` }}>
        <span className="ou-vol-fill" />
        <span className="ou-vol-knob" style={{ left: `calc(${pct * 100}% - 5px)` }} />
        <input type="range" min={-24} max={6} step={0.5} value={db} aria-label={`${track.name} volume`}
          onClick={event => event.stopPropagation()}
          onChange={event => dispatch({ type: 'track/volume', trackId: track.id, volumeDb: Number(event.target.value) })} />
      </span>
      <span className="ou-vol-db">{db >= 0 ? '+' : ''}{db.toFixed(1)}</span>
      <label className="ou-pan-control" title="Track pan">
        <span>PAN</span>
        <input type="range" min={-1} max={1} step={0.01} value={track.pan ?? 0} aria-label={`${track.name} pan`}
          onClick={event => event.stopPropagation()}
          onChange={event => dispatch({ type: 'track/pan', trackId: track.id, pan: Number(event.target.value) })} />
        <span>{Math.round((track.pan ?? 0) * 100)}</span>
      </label>
      <span className="ou-vol-badge" title="Compress">C</span>
      <button type="button" className="ou-side-btn" disabled title="Track settings are not connected to Project Session yet">⚙</button>
    </div>
  </div>
  {fxOpen && <TrackFxDialog track={track} onClose={() => setFxOpen(false)} onApply={mixFx => {
    dispatch({ type: 'track/mix_fx', trackId: track.id, mixFx });
    setFxOpen(false);
  }} />}
  </>;
}

/**
 * Keeps the scrollbar where the drawing is.
 *
 * The offset is React state and the scrollbar is a DOM element, and only the
 * user moves the second one. Without this the two drift apart: following a part
 * scrolls the canvas while the bar stays where it was, so the next wheel or
 * scrollbar drag snaps the timeline somewhere else entirely. Writing the value
 * back is a no-op for a scroll the user just made.
 */
function useScrollbarSync(hostRef, scrollX, contentWidth) {
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const next = clampArrScrollX(scrollX, host.clientWidth, contentWidth);
    if (Math.abs(host.scrollLeft - next) > 1) host.scrollLeft = next;
  }, [hostRef, scrollX, contentWidth]);
}

export function ArrangementPanel() {
  const { tracks, state, dispatch, connected, activePart, waveform, bpm, doc, project } = useClone();
  // The timeline draws in its own pixels, so it measures the dock and divides
  // the visible measures across it: canvas and hit layer then share one scale.
  const [hostRef, size] = useElementSize({ width: 1152, height: 160 });
  const pixiRef = useRef(null);
  const [audioWaveforms, setAudioWaveforms] = useState({});
  const audioWaveformsRef = useRef({ key: null, values: new Map() });
  const audioTrackSpecs = tracks.flatMap((track, trackIndex) =>
    (track.parts || []).some(part => part.kind === 'wave') ? [{ trackId: track.id, trackIndex }] : [],
  );
  const audioTrackKey = audioTrackSpecs.map(({ trackId, trackIndex }) => `${trackIndex}:${trackId}`).join('|');
  const audioWaveformKey = audioWaveformCacheKey(doc.sessionId, tracks);
  useEffect(() => {
    let stale = false;
    if (audioWaveformsRef.current.key !== audioWaveformKey) {
      audioWaveformsRef.current = { key: audioWaveformKey, values: new Map() };
      setAudioWaveforms({});
    }
    const cache = audioWaveformsRef.current;
    if (!connected || !audioTrackSpecs.length || typeof project?.renderTrack !== 'function') {
      if (!connected) cache.values.clear();
      setAudioWaveforms({});
      return () => { stale = true; };
    }
    // The regular selected-track render also produces peaks. Wait for it to
    // finish before asking the worker for audio-only lanes, and reuse its result
    // when the selected track is itself an audio track.
    if (waveform?.status === 'idle' || waveform?.status === 'rendering') return () => { stale = true; };
    if (waveform?.status === 'ready' && waveform.trackId && waveform.peaks
      && audioTrackSpecs.some(spec => spec.trackId === waveform.trackId)) {
      cache.values.set(waveform.trackId, waveform);
    }
    setAudioWaveforms(Object.fromEntries(cache.values));
    // A transient render failure is not a valid cache entry. Retry it the next
    // time the waveform dependencies change, but keep successful audio peaks.
    const pending = audioTrackSpecs.filter(spec => cache.values.get(spec.trackId)?.status !== 'ready');
    if (!pending.length) return () => { stale = true; };
    (async () => {
      for (const spec of pending) {
        try {
          // The WASM host returns peaks without retaining or replacing a PCM
          // playback take; the HTTP host may return PCM which is reduced here.
          const audio = await project.renderTrack(spec.trackIndex, { waveformOnly: true });
          if (stale) return;
          const peaks = audio?.peaks || ((audio?.channels || audio?.samples)
            ? buildPeaks(audio.channels || audio.samples, audio.sampleRate) : null);
          if (!peaks) throw new Error('Audio track render returned no waveform peaks');
          cache.values.set(spec.trackId, { status: 'ready', trackId: spec.trackId, peaks });
        } catch (error) {
          if (stale) return;
          cache.values.set(spec.trackId, { status: 'error', trackId: spec.trackId, error: String(error?.message || error) });
        }
        setAudioWaveforms(Object.fromEntries(cache.values));
      }
    })();
    return () => { stale = true; };
  }, [connected, audioTrackKey, audioWaveformKey, project?.renderTrack, waveform?.status, waveform?.trackId, waveform?.peaks]);
  const [headerWidth, setHeaderWidth] = useState(() => {
    try {
      const saved = Number(localStorage.getItem('rusted-utau.arrangement.headerWidth'));
      return Number.isFinite(saved) && saved > 0
        ? Math.max(ARR.minHeaderWidth, Math.min(ARR.maxHeaderWidth, saved))
        : ARR.headerWidth;
    } catch { return ARR.headerWidth; }
  });
  const headerRef = useRef(null);
  const scrollSyncRef = useRef(false);
  // `null` means "follow the dock", OpenUtau's 16-measure opening view; a number
  // is an explicit zoom level the user stepped to, kept across dock resizes.
  const [zoom, setZoom] = useState(null);
  const width = Math.max(320, size.width);
  const measure = zoom ?? arrMeasureWidth(width);
  const height = rulerHeight + tracks.length * rowHeight + 40;
  const contentMeasures = arrContentMeasures(projectTicks(tracks));
  const contentWidth = contentMeasures * measure;
  const timelineScrollTicks = state.timelineScrollTicks ?? 0;
  const scrollX = clampArrScrollX(
    ticksToScrollOffset(timelineScrollTicks, measure / TICKS_PER_MEASURE), width, contentWidth,
  );
  useScrollbarSync(hostRef, scrollX, contentWidth);
  // Width follows the dock; height stays the height of the lanes, so a short
  // dock shows empty space below them instead of a stretched canvas.
  useRendererSize(pixiRef, width, height);
  useRenderOnDemand(pixiRef);
  const activeTrackId = tracks.find(track => (track.parts || []).some(part => part.id === state.activePartId))?.id;
  const theme = useTheme();
  // Scrolling is the browser's: a real scrollbar, a trackpad's sideways swipe
  // and shift+wheel all arrive here already, and the canvas stays pinned to the
  // viewport by `position: sticky` while the drawn offset follows `scrollX`.
  const syncVerticalScroll = (source, target) => {
    if (scrollSyncRef.current) return;
    scrollSyncRef.current = true;
    target.scrollTop = source.scrollTop;
    requestAnimationFrame(() => { scrollSyncRef.current = false; });
  };
  const onScroll = event => {
    const next = Math.round(event.currentTarget.scrollLeft);
    // Programmatic scrollbar alignment already reflects shared state. Only a
    // real user offset publishes a new timeline position (avoids round-trip
    // pixel quantization nudging the other canvas by a few ticks).
    if (next !== Math.round(scrollX)) dispatch({
      type: 'viewport/horizontal-scroll',
      ticks: scrollOffsetToTicks(next, measure / TICKS_PER_MEASURE),
    });
    if (headerRef.current) syncVerticalScroll(event.currentTarget, headerRef.current);
  };
  const onHeaderScroll = event => {
    if (hostRef.current) syncVerticalScroll(event.currentTarget, hostRef.current);
  };
  const resizeHeader = event => {
    if (event.type === 'pointerdown') {
      event.preventDefault();
      event.currentTarget.setPointerCapture(event.pointerId);
      event.currentTarget.dataset.startX = String(event.clientX);
      event.currentTarget.dataset.startWidth = String(headerWidth);
      return;
    }
    if (event.type === 'pointermove' && event.buttons) {
      const next = Math.max(ARR.minHeaderWidth, Math.min(ARR.maxHeaderWidth,
        Number(event.currentTarget.dataset.startWidth) + event.clientX - Number(event.currentTarget.dataset.startX)));
      setHeaderWidth(next);
      return;
    }
    if (event.type === 'pointerup' || event.type === 'pointercancel') {
      try { localStorage.setItem('rusted-utau.arrangement.headerWidth', String(headerWidth)); } catch { /* layout persistence is optional */ }
    }
  };
  const resizeHeaderByKey = event => {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault();
    const next = event.key === 'Home' ? ARR.minHeaderWidth
      : event.key === 'End' ? ARR.maxHeaderWidth
        : headerWidth + (event.key === 'ArrowRight' ? 12 : -12);
    const width = Math.max(ARR.minHeaderWidth, Math.min(ARR.maxHeaderWidth, next));
    setHeaderWidth(width);
    try { localStorage.setItem('rusted-utau.arrangement.headerWidth', String(width)); } catch { /* layout persistence is optional */ }
  };
  const zoomTo = next => {
    const clamped = Math.max(ARR.minMeasureWidth, Math.min(ARR.maxMeasureWidth, Math.round(next)));
    if (clamped === measure) return;
    // Zoom around the middle of the view so the song does not slide sideways
    // under the pointer on every step.
    const nextScrollX = arrZoomScrollX(scrollX, width, measure, clamped, contentMeasures * clamped);
    dispatch({
      type: 'viewport/horizontal-scroll',
      ticks: scrollOffsetToTicks(nextScrollX, clamped / TICKS_PER_MEASURE),
    });
    setZoom(clamped);
  };
  // Dev-only viewport handle, like the piano roll's: the scrollbar is a DOM
  // element the probe can measure, but what is actually *drawn* is a canvas it
  // cannot see into.
  if (import.meta.env?.DEV) window.__ouArr = { scrollX, width, height, measure, contentMeasures, contentWidth };
  return <div className="ou-arrangement" style={{ '--arr-header-width': `${headerWidth}px` }}>
    <div className="ou-arr-headers" ref={headerRef} onScroll={onHeaderScroll}>
      <div className="ou-arr-header-ruler" aria-hidden="true" />
      {tracks.map((track, index) => <TrackHeaderCard key={track.id} track={track} index={index} active={track.id === activeTrackId} />)}
      <button type="button" className="ou-add-track" disabled={!connected}
        title={connected ? 'Add a track' : 'Connect to Project Session to add a track'}
        onClick={() => dispatch({ type: 'track/add', name: `Track ${tracks.length + 1}` })}>＋</button>
    </div>
    <div className="ou-arr-resizer" role="separator" aria-orientation="vertical" aria-label="Resize arrangement track mixer" aria-valuemin={ARR.minHeaderWidth} aria-valuemax={ARR.maxHeaderWidth} aria-valuenow={Math.round(headerWidth)} tabIndex={0}
      onPointerDown={resizeHeader} onPointerMove={resizeHeader} onPointerUp={resizeHeader} onPointerCancel={resizeHeader} onKeyDown={resizeHeaderByKey} />
    <div className="ou-arr-timeline" ref={hostRef} onScroll={onScroll}>
      {/* Wider than the viewport: this element is what the scrollbar travels. */}
      <div className="ou-arr-track" style={{ width: contentWidth, height: height + SCROLLBAR_ROOM }}>
        <div className="ou-arr-canvas" style={{ width, height }}>
          <PixiApplication ref={pixiRef} width={width} height={height} background={theme.surfaceAlt} antialias preference="webgl">
            <TimelineCanvas width={width} height={height} measureWidth={measure} scrollX={scrollX} tracks={tracks} activePartId={state.activePartId} waveform={waveform} audioWaveforms={audioWaveforms} notes={state.notes} bpm={bpm} playheadTicks={state.playheadTicks} appRef={pixiRef} />
            <MeasureNumbers measureWidth={measure} scrollX={scrollX} width={width} />
            <PartLabels measureWidth={measure} scrollX={scrollX} width={width} tracks={tracks} activePartId={state.activePartId} />
          </PixiApplication>
          <TimelineInteraction measureWidth={measure} scrollX={scrollX} />
          <div className="ou-arr-zoom">
            <button type="button" aria-label="Zoom the timeline out" title="Zoom out" onClick={() => zoomTo(measure / 1.25)}>−</button>
            <button type="button" className="ou-arr-zoom-fit" title="Fit 16 measures across the dock"
              onClick={() => zoomTo(arrMeasureWidth(width))}>{Math.round(measure)}</button>
            <button type="button" aria-label="Zoom the timeline in" title="Zoom in" onClick={() => zoomTo(measure * 1.25)}>＋</button>
          </div>
        </div>
      </div>
    </div>
  </div>;
}
