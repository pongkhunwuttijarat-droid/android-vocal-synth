// Shared UI state for the OpenUtau-style clone page.
//
// The document below is still mock data, but it is shaped exactly like the
// engine snapshot DTOs (`id`, `name`, `singer_id`, `mute`, `solo`, `volume_db`,
// `pan`, `parts[]`, and notes with `position_ticks`/`duration_ticks`/`tone`/
// `lyric`) and every mutation goes through this one reducer. Swapping the mock
// document for a real `project-session` later therefore touches only this file:
// each action names the engine command it is expected to map onto.
import React, { createContext, useCallback, useContext, useEffect, useMemo, useReducer, useRef, useState } from 'react';
import { BPM, EXPRESSION_LANES, NOTES, TICKS_PER_BEAT, TICKS_PER_MEASURE, TIME_SIG, TRACKS, clampToLane, isBlackKey, toneName } from './mock-data.js';
import { UI_ONLY, createEngine } from './engine.js';
import { importUstxWithMedia } from './ustx-import.js';
import { readActiveProjectName, restoreActiveProject, setActiveProjectName } from './active-project-store.js';
import { createAudioPlayer, msToTick } from './audio-playback.js';
import { requestPlaybackCredential } from './session-stream.js';
import { projectTicks, transportTarget } from './transport-state.js';
import { normalizeTimelineScrollTicks } from './timeline-scroll.js';
export { noteOnsets, projectTicks, skipNoteTarget, skipTarget, transportTarget } from './transport-state.js';
import { buildPeaks, getRenderPool, renderProjectAudio } from './waveform-source.js';
import { DEFAULT_PITCH_SHAPE } from './pitch-curve-input.js';

/**
 * Where the waveform strip sits, in CSS pixels from the bottom of the roll.
 * OpenUtau pins it 60px above the bottom edge at 60px tall (PianoRoll.axaml
 * `Margin="0,0,0,60" Height="60"`); the clone can be dragged, and this is the
 * placement it starts from.
 */
export const WAVEFORM_DEFAULT = { bottom: 60, height: 60 };

/* ------------------------------------------------------------------ *
 * Geometry — shared by the PixiJS drawing code and pointer hit tests
 * so a pixel drawn at X is exactly the pixel that can be clicked.
 *
 * The arrangement timeline's own scale, scroll range and hit tests live in
 * `arrangement-geometry.js`: those are plain functions, and the node test
 * runner cannot import this file because it contains JSX.
 * ------------------------------------------------------------------ */

/** Idle render-preview state, before any session or render exists. */
const IDLE_RENDER = { status: 'idle', trackId: null, peaks: null, source: null, bank: null, error: null, samples: 0, stats: null };
/**
 * How long the document must hold still before the preview re-renders.
 *
 * A drag emits a command per coalesced batch, and each one bumps the revision.
 * Re-planning on every one of them would starve the pool of work that would
 * then be thrown away — and because only the chunks whose hash changed are
 * re-rendered, waiting costs nothing but a moment of lag.
 */
const RENDER_DEBOUNCE_MS = 400;

/** Piano roll: shortcut column + keyboard + note grid, all inside one Pixi app. */
export const ROLL = { shortcutWidth: 26, keyboardWidth: 64, rowHeight: 22, beatWidth: 68, topRuler: 24, maxTone: 107, minTone: 24 };
ROLL.gridLeft = ROLL.shortcutWidth + ROLL.keyboardWidth;
ROLL.rows = ROLL.maxTone - ROLL.minTone + 1;
/** Full musical height of the roll; the canvas is shorter and scrolls inside. */
ROLL.contentHeight = ROLL.topRuler + ROLL.rows * ROLL.rowHeight;
/** Clamp a scroll offset so the roll cannot be scrolled past its last row. */
export const clampScroll = (scrollY, viewportHeight) => Math.max(0, Math.min(ROLL.contentHeight - viewportHeight - ROLL.topRuler, scrollY));
/**
 * Clamp a horizontal scroll offset against the part actually on screen.
 *
 * The roll has no scrollable DOM — it draws only the slice the viewport covers
 * and shifts the world by `scroll.x` — so the legal range has to come from the
 * part's own length. Clamping to a fixed 16-beat canvas width instead made a
 * part that starts at bar 60 impossible to scroll to at all, which is how an
 * imported song ended up showing an empty grid over 319 notes that were present
 * the whole time.
 */
export const clampScrollX = (scrollX, viewportWidth, contentTicks) => {
  const visible = Math.max(0, viewportWidth - ROLL.gridLeft);
  const content = Math.max(0, contentTicks || 0) / TICKS_PER_BEAT * ROLL.beatWidth;
  return Math.max(0, Math.min(Math.max(0, content - visible), scrollX));
};
/** OpenUtau opens the roll around the working vocal range rather than B7. */
export const defaultScroll = rowHeight => (ROLL.maxTone - 84) * rowHeight;
/** Edge band (px) that turns a press into a resize instead of a move. */
export const RESIZE_MARGIN = 8;
/** OpenUtau refuses gestures shorter than 15 ticks. */
export const MIN_NOTE_TICKS = 15;
/** Dead zone before a press becomes a drag. */
export const DRAG_THRESHOLD = 4;

export function rollNoteRect(note) {
  const x = ROLL.gridLeft + note.position_ticks / TICKS_PER_BEAT * ROLL.beatWidth;
  const y = ROLL.topRuler + (ROLL.maxTone - note.tone) * ROLL.rowHeight + 2;
  const w = Math.max(14, note.duration_ticks / TICKS_PER_BEAT * ROLL.beatWidth - 2);
  return { x, y, w, h: ROLL.rowHeight - 4 };
}
/** Pointer → (tick, tone) in the roll grid. */
export function rollReadPointer(x, y) {
  return {
    tickF: (x - ROLL.gridLeft) / ROLL.beatWidth * TICKS_PER_BEAT,
    tone: Math.max(0, Math.min(127, Math.round(ROLL.maxTone - (y - ROLL.topRuler) / ROLL.rowHeight))),
  };
}
/**
 * OpenUtau `HitTestNote`: body test first, then the left/right resize bands.
 * The test uses the drawn rectangle rather than a rounded tick/tone, because a
 * note only fills part of its row and rounding would make its lower half
 * unclickable.
 */
const HIT_TOLERANCE = 2;
export function rollHitNote(notes, x, y) {
  for (const note of notes) {
    const rect = rollNoteRect(note);
    if (x < rect.x - HIT_TOLERANCE || x > rect.x + rect.w + HIT_TOLERANCE) continue;
    if (y < rect.y - HIT_TOLERANCE || y > rect.y + rect.h + HIT_TOLERANCE) continue;
    const hitLeft = x >= rect.x && x < rect.x + RESIZE_MARGIN;
    const hitRight = x <= rect.x + rect.w && x > rect.x + rect.w - RESIZE_MARGIN;
    return { note, rect, resize: hitLeft || hitRight, fromStart: hitLeft && !hitRight };
  }
  return null;
}

/* ------------------------------------------------------------------ *
 * Snapping and helpers
 * ------------------------------------------------------------------ */

export const SNAP_UNITS = { '1/4': TICKS_PER_BEAT, '1/8': TICKS_PER_BEAT / 2, '1/16': TICKS_PER_BEAT / 4, '1/32': TICKS_PER_BEAT / 8, Off: 1 };
export const snapTick = (tick, snap = '1/16', roundUp = false) => {
  const unit = SNAP_UNITS[snap] || 1;
  if (unit <= 1) return Math.max(0, Math.round(tick));
  const floored = Math.floor(Math.max(0, tick) / unit) * unit;
  return roundUp ? floored + unit : floored;
};
/** Vibrato patch shape shared by the roll's Vibrato tool and the properties rows. */
export const VIBRATO_DEFAULTS = { on: false, length: 75, period: 175, depth: 25, fadeIn: 10, fadeOut: 10, shift: 0, drift: 0 };
export const isC = tone => tone % 12 === 0;
export const toneLabel = toneName;
export { isBlackKey };

/* ------------------------------------------------------------------ *
 * UI state (frontend-owned) and the mock fallback document
 * ------------------------------------------------------------------ */

const clone = value => JSON.parse(JSON.stringify(value));

/** Mock document used only when no engine session could be opened. */
function mockDocument() {
  const tracks = clone(TRACKS);
  const notes = {};
  for (const note of clone(NOTES)) (notes[note.part_id] ||= []).push(note);
  return { tracks, notes };
}

function initialState() {
  const { tracks, notes } = mockDocument();
  return {
    // Mock-only document; replaced by the engine snapshot once a session opens.
    tracks,
    notes,
    activePartId: tracks[0]?.parts?.[0]?.id ?? null,
    selection: new Set(),
    activeNoteId: null,
    tool: 'Select',
    snap: '1/16',
    noteLabels: 'lyric',
    // Pitch-curve tool settings. Presentation state, like the tool itself: the
    // shape is which one the next drawn point gets and `snapFirst` is the
    // `snap_first` field the engine stores with the curve.
    pitchEdit: { shape: DEFAULT_PITCH_SHAPE, snapFirst: true },
    playing: false,
    playheadTicks: 0,
    // Shared, project-absolute viewport position for Arrangement and Piano Roll.
    // This is presentation state only and never enters the Rust project model.
    timelineScrollTicks: 0,
    enabledLanes: {},
    // Waveform overlay placement, in CSS pixels from the bottom of the roll.
    waveform: { ...WAVEFORM_DEFAULT },
    showWaveform: true,
    expEdits: {}, // mock-only curve cache; a live session reads parts[].curves
    edit: { kind: null, anchorTicks: 0, anchorTone: 0, startX: 0, moved: false },
    // Bumped when the host's installed voicebanks change. Voicebanks are host
    // storage, not project state, so the preview has to be told to re-resolve
    // its source; this is the only thing the change touches.
    bankEpoch: 0,
  };
}

/** Every part in the document with its owning track, in draw order. */
export function allParts(tracks) {
  return tracks.flatMap(track => (track.parts || []).map(part => ({ part, track })));
}

// Pure transport helpers live in a JavaScript module so the node test runner
// can exercise project-aware clamping without loading this JSX file.

/** Transport intents that move the playhead (so, when audio is playing, the audio). */
const MOVES_PLAYHEAD = new Set(['transport/seek', 'transport/skip-measure', 'transport/skip-note', 'transport/restart']);

function nextId(prefix, taken) {
  let n = 1;
  while (taken.has(`${prefix}-${n}`)) n += 1;
  return `${prefix}-${n}`;
}

function mapNotes(notes, fn) {
  const out = {};
  for (const [partId, list] of Object.entries(notes)) out[partId] = list.map(fn);
  return out;
}

/**
 * Applies an intent to the mock document. The intent shapes are the same ones
 * `engine.js` turns into Rust commands, so both paths stay comparable; delta
 * based gestures therefore behave identically with and without an engine.
 */
function applyToMock(state, intent) {
  switch (intent.type) {
    case 'track/add': {
      // Mock-only: the contract has no `track.create` command yet (§6).
      const id = nextId('track', new Set(state.tracks.map(t => t.id)));
      const partId = nextId('part', new Set(allParts(state.tracks).map(({ part }) => part.id)));
      return { ...state, tracks: [...state.tracks, { id, name: `Track${state.tracks.length + 1}`, singer_id: 'Unassigned singer', singer_detail: 'No voicebank selected', mute: false, solo: false, volume_db: 0, pan: 0, parts: [{ id: partId, track_id: id, kind: 'voice', name: 'New Part', position_ticks: 0, duration_ticks: TICKS_PER_MEASURE * 8 }] }], notes: { ...state.notes, [partId]: [] } };
    }
    case 'track/mute':
      return { ...state, tracks: state.tracks.map(t => t.id === intent.trackId ? { ...t, mute: intent.mute ?? !t.mute } : t) };
    case 'track/solo':
      return { ...state, tracks: state.tracks.map(t => t.id === intent.trackId ? { ...t, solo: intent.solo ?? !t.solo } : t) };
    case 'track/volume':
      return { ...state, tracks: state.tracks.map(t => t.id === intent.trackId ? { ...t, volume_db: intent.volumeDb } : t) };
    case 'track/mix_fx':
      return { ...state, tracks: state.tracks.map(t => t.id === intent.trackId ? { ...t, mix_fx: intent.mixFx } : t) };
    // Mirrors the engine's normalisation so the disconnected preview behaves the
    // same way the connected one does.
    case 'track/singer':
      return { ...state, tracks: state.tracks.map(t => t.id === intent.trackId ? { ...t, singer_id: intent.singerId?.trim() || null } : t) };
    case 'part/add': {
      const track = state.tracks.find(t => t.id === intent.trackId);
      if (!track) return state;
      const partId = nextId('part', new Set(allParts(state.tracks).map(({ part }) => part.id)));
      const part = { id: partId, track_id: track.id, kind: 'voice', name: 'New Part', position_ticks: intent.tick ?? 0, duration_ticks: intent.durationTicks ?? TICKS_PER_MEASURE * 4 };
      return { ...state, tracks: state.tracks.map(t => t.id === track.id ? { ...t, parts: [...(t.parts || []), part] } : t), notes: { ...state.notes, [partId]: [] }, activePartId: partId, selection: new Set(), activeNoteId: null };
    }
    case 'part/select':
      return { ...state, activePartId: intent.partId, selection: new Set(), activeNoteId: null };
    case 'project/loaded': {
      // The document behind the panels was swapped, so every id the selection
      // held belongs to a project that is no longer on screen — unless the
      // engine sent the *same* document back. A refresh after a render replays
      // the same ids, and dropping the open part then silently threw the user
      // back to the first phrase every time a preview finished.
      const live = new Set(allParts(state.tracks).map(({ part }) => part.id));
      return { ...state, activePartId: live.has(state.activePartId) ? state.activePartId : null, selection: new Set(), activeNoteId: null };
    }
    case 'notes/select': {
      const selection = intent.additive ? new Set([...state.selection, ...intent.ids]) : new Set(intent.ids);
      const activeNoteId = intent.ids.length ? intent.ids[intent.ids.length - 1] : [...selection].pop() ?? null;
      return { ...state, selection, activeNoteId };
    }
    case 'note/add': {
      const partId = intent.partId || state.activePartId;
      if (!partId) return state;
      const taken = new Set(allParts(state.tracks).flatMap(({ part }) => (state.notes[part.id] || []).map(n => n.id)));
      const note = { id: nextId('note', taken), part_id: partId, position_ticks: intent.tick, tone: intent.tone, duration_ticks: intent.durationTicks ?? TICKS_PER_BEAT, lyric: intent.lyric || 'あ' };
      return { ...state, notes: { ...state.notes, [partId]: [...(state.notes[partId] || []), note].sort((a, b) => a.position_ticks - b.position_ticks) }, selection: new Set([note.id]), activeNoteId: note.id, tool: 'Select' };
    }
    case 'note/remove': {
      const selection = new Set([...state.selection].filter(id => !intent.ids.includes(id)));
      return { ...state, notes: mapNotes(state.notes, list => list.filter(n => !intent.ids.includes(n.id))), selection, activeNoteId: selection.has(state.activeNoteId) ? state.activeNoteId : null };
    }
    case 'note/move':
      // Gestures carry the requested position (see engine.js): a coalesced
      // drag must land where the pointer is, not one frame behind.
      return { ...state, notes: mapNotes(state.notes, list => list.map(n => {
        const update = intent.updates.find(u => u.id === n.id);
        if (!update) return n;
        return { ...n, position_ticks: Math.max(0, update.position_ticks ?? n.position_ticks), tone: Math.max(0, Math.min(127, update.tone ?? n.tone)) };
      }).sort((a, b) => a.position_ticks - b.position_ticks)) };
    case 'note/resize':
      return { ...state, notes: mapNotes(state.notes, list => list.map(n => {
        const update = intent.updates.find(u => u.id === n.id);
        if (!update) return n;
        return { ...n, position_ticks: Math.max(0, update.position_ticks ?? n.position_ticks), duration_ticks: Math.max(MIN_NOTE_TICKS, update.duration_ticks ?? n.duration_ticks) };
      }).sort((a, b) => a.position_ticks - b.position_ticks)) };
    case 'note/tone':
      return { ...state, notes: mapNotes(state.notes, n => {
        const update = intent.updates.find(u => u.id === n.id);
        return update ? { ...n, tone: Math.max(0, Math.min(127, update.tone)) } : n;
      }) };
    case 'note/pitch':
      // Mock path only; with a live session this intent goes to `note.set_pitch`
      // and the drawn points come back from the snapshot's `notes[].pitch`.
      return { ...state, notes: mapNotes(state.notes, n => n.id === intent.id ? { ...n, pitch: intent.pitch } : n) };
    case 'note/lyric':
      return { ...state, notes: mapNotes(state.notes, n => intent.ids.includes(n.id) ? { ...n, lyric: intent.lyric } : n) };
    case 'note/split': {
      const { note, tick } = intent;
      const taken = new Set((state.notes[note.part_id] || []).flatMap(n => [n.id, `${n.id}-b`]));
      const tail = { ...note, id: nextId('note', taken) + '-b', position_ticks: tick, duration_ticks: note.duration_ticks - (tick - note.position_ticks) };
      if (tail.duration_ticks < MIN_NOTE_TICKS) return state;
      return { ...state, notes: { ...state.notes, [note.part_id]: [...(state.notes[note.part_id] || []).map(n => n.id === note.id ? { ...n, duration_ticks: tick - note.position_ticks } : n), tail].sort((a, b) => a.position_ticks - b.position_ticks) }, selection: new Set([tail.id]), activeNoteId: tail.id };
    }
    case 'note/vibrato': {
      // Mock-only fallback. Connected sessions handle this intent through
      // commandsFor() and the Rust snapshot remains authoritative.
      const ids = intent.ids?.length ? intent.ids : [...state.selection];
      const patches = { ...(state.vibratoPatches || {}) };
      for (const id of ids) patches[id] = intent.patch;
      return { ...state, vibratoPatches: patches };
    }
    case 'lane/toggle': {
      const partLanes = { ...(state.enabledLanes[intent.partId] || {}) };
      partLanes[intent.abbr] = !(partLanes[intent.abbr] ?? true);
      return { ...state, enabledLanes: { ...state.enabledLanes, [intent.partId]: partLanes } };
    }
    case 'viewport/horizontal-scroll': {
      const timelineScrollTicks = normalizeTimelineScrollTicks(intent.ticks);
      return timelineScrollTicks === state.timelineScrollTicks ? state : { ...state, timelineScrollTicks };
    }
    case 'ui/banks-changed':
      // A bank was installed or removed: nothing about the project changed, but
      // the preview must re-resolve which samples it reads.
      return { ...state, bankEpoch: (state.bankEpoch || 0) + 1 };
    case 'waveform/toggle':
      return { ...state, showWaveform: intent.visible ?? !state.showWaveform };
    case 'waveform/placement': {
      // Where the waveform strip sits is presentation state, not project state.
      // `patch` may be a function of the current placement: a drag fires one
      // intent per pointermove and React batches them, so a value computed from
      // the last render would drop every move but the final one.
      const base = { ...WAVEFORM_DEFAULT, ...state.waveform };
      const patch = intent.patch;
      return { ...state, waveform: typeof patch === 'function' ? patch(base) : { ...base, ...patch } };
    }
    case 'exp/stroke': {
      // Mock path only: with a live session this intent goes to
      // `curve.apply_stroke` and the drawn points come back from the snapshot's
      // `parts[].curves`. Here we mirror what the engine would answer so the
      // mock document behaves the same (canonical ids + the points we sent).
      const parts = {};
      for (const track of state.doc.tracks) {
        for (const part of track.parts || []) {
          if (part.id !== intent.partId) { parts[part.id] = part; continue; }
          const others = (part.curves || []).filter(curve => curve.abbr !== intent.abbr);
          parts[part.id] = {
            ...part,
            curves: [...others, { id: intent.curveId || `curve-${intent.abbr}`, abbr: intent.abbr, points: intent.points }],
          };
        }
      }
      return {
        ...state,
        doc: { ...state.doc, tracks: state.doc.tracks.map(track => ({ ...track, parts: track.parts?.map(part => parts[part.id]).filter(Boolean) })) },
      };
    }
    case 'edit/begin':
      return { ...state, edit: { ...intent.edit, moved: false, startX: intent.x } };
    case 'edit/move':
      return { ...state, edit: { ...state.edit, moved: true } };
    case 'edit/end':
      return { ...state, edit: { kind: null, anchorTicks: 0, anchorTone: 0, startX: 0, moved: false } };
    case 'ui/tool':
      return { ...state, tool: intent.tool };
    case 'ui/snap':
      return { ...state, snap: intent.snap };
    case 'ui/labels':
      return { ...state, noteLabels: intent.labels };
    case 'ui/pitch-shape':
      return { ...state, pitchEdit: { ...state.pitchEdit, shape: intent.shape } };
    case 'ui/pitch-snap-first':
      return { ...state, pitchEdit: { ...state.pitchEdit, snapFirst: intent.snapFirst } };
    case 'transport/toggle':
      return { ...state, playing: intent.playing ?? !state.playing };
    case 'transport/stop':
      // Stops the clock but KEEPS the playhead where it was, so stopping to
      // look at something does not lose your place. Restart is separate.
      return { ...state, playing: false };
    case 'transport/restart':
      return { ...state, playing: intent.playing ?? true, playheadTicks: transportTarget(state, intent) };
    case 'transport/seek':
    case 'transport/skip-measure':
    case 'transport/skip-note':
      return { ...state, playheadTicks: transportTarget(state, intent) };
    default:
      return state;
  }
}

/* ------------------------------------------------------------------ *
 * Context
 * ------------------------------------------------------------------ */

const CloneContext = createContext(null);

/** Gestures that describe where something should be: only the newest one matters. */
const GESTURE_TARGETS = new Set(['note/move', 'note/resize', 'note/tone', 'note/pitch']);

/**
 * Owns the document. With an engine session the document is the engine
 * snapshot (authoritative); intents are translated into Rust commands. Without
 * one — no WASM package, no server — the same intents fall back to the mock
 * document so the page still demonstrates the UI, with the status line saying so.
 */
export function CloneProvider({ backend = 'wasm', baseUrl = '', children }) {
  const [ui, dispatchUi] = useReducer((state, intent) => intent.type === '@reset' ? state : applyToMock(state, intent), undefined, initialState);
  const [doc, setDoc] = useState(() => ({ tracks: [], notes: {}, revision: 0, status: 'mock' }));
  const engineRef = useRef(null);
  const loopRef = useRef(null);
  const uiRef = useRef(ui);
  uiRef.current = ui;
  const docRef = useRef(doc);
  docRef.current = doc;
  const backendRef = useRef(backend);
  backendRef.current = backend;
  const baseUrlRef = useRef(baseUrl);
  baseUrlRef.current = baseUrl;
  // Render-preview audio: real PCM from the engine, reduced to a min/max
  // envelope. `idle` until a session exists, then `rendering` / `ready` /
  // `empty` / `error`. This is engine OUTPUT, never project state.
  const [render, setRender] = useState(IDLE_RENDER);
  // A user-triggered render is a different thing from the debounced preview
  // above: the preview re-renders track 0 whenever the document changes, while
  // this is one explicit §6.13 job the user asked for, on a track they chose.
  const [renderJob, setRenderJob] = useState(null);
  const [playbackHandoffStartedAt, setPlaybackHandoffStartedAt] = useState(null);
  // Why the transport is silent, when it is: no pool yet, cache behind the
  // playhead, a browser without Web Audio. Null while audio is running.
  const [audioError, setAudioError] = useState(null);
  // Where the sound goes. A browser chooses its own default output and this
  // machine has three sinks, so the page shows the choice and lets it be made
  // here rather than in a system menu nobody thinks to open.
  const [outputs, setOutputs] = useState([]);
  const [outputDevice, setOutputDevice] = useState('');
  const [volume, setVolumeState] = useState(1);
  // One player for the page. It reads the pool and the tempo lazily, so it
  // survives a backend switch or a new session without being rebuilt.
  const playerRef = useRef(null);
  const bankEpochRef = useRef(ui.bankEpoch);
  bankEpochRef.current = ui.bankEpoch;
  /**
   * The last finished engine render, whole-track, kept out of React state.
   *
   * This is what the transport plays when the preview came from the synth-server
   * rather than the in-browser pool — and the only source that carries a wave
   * part at all, since the pool has no media store. A ref, not state: the samples
   * are megabytes and nothing renders from them, so re-rendering the tree on every
   * new take would be pure cost.
   */
  const takeRef = useRef(null);
  // A project mixdown is the transport source. Keep it separate from the
  // selected-track take used by the waveform strip and cache one revision at a
  // time so replay does not rerender hundreds of seconds of audio.
  const mixdownTakeRef = useRef(null);
  const pendingSessionStreamRef = useRef(null);
  const mixdownPromiseRef = useRef(null);
  const playbackPreparingRef = useRef(false);
  const playbackSessionKeyRef = useRef(null);
  useEffect(() => {
    const key = `${doc.sessionId ?? ''}:${doc.revision ?? 0}:${ui.bankEpoch}`;
    if (mixdownTakeRef.current?.key !== key) mixdownTakeRef.current = null;
  }, [doc.sessionId, doc.revision, ui.bankEpoch]);
  /** Last audio-clock reading, to notice a context whose clock stopped. */
  const stallRef = useRef({ tick: null, at: 0 });
  /**
   * Whether audio currently owns the playhead.
   *
   * Once a take is playing, the playhead comes from the AudioContext and from
   * nowhere else. Letting the silent clock run alongside it would make the two
   * fight: the cursor advances while the audio clock stands still, and every
   * frame then looks like a seek. One owner at a time.
   */
  const audioArmedRef = useRef(false);
  if (!playerRef.current) {
    playerRef.current = createAudioPlayer({
      getTake: () => {
        const cached = mixdownTakeRef.current;
        const current = docRef.current;
        const key = `${current?.sessionId ?? ''}:${current?.revision ?? 0}:${bankEpochRef.current}`;
        return cached?.key === key ? cached.take : takeRef.current;
      },
      getCompletedTake: () => {
        const cached = mixdownTakeRef.current;
        const current = docRef.current;
        const key = `${current?.sessionId ?? ''}:${current?.revision ?? 0}:${bankEpochRef.current}`;
        return cached?.key === key ? cached.take : null;
      },
      getTakeAudio: takeId => engineRef.current?.getPlaybackTake(takeId),
      getSessionStream: () => {
        const config = pendingSessionStreamRef.current;
        pendingSessionStreamRef.current = null;
        return config;
      },
      getPool: () => getRenderPool(),
      getBpm: () => docRef.current?.bpm ?? BPM,
      // The take ran out: stop the transport rather than leaving a silent clock
      // running past the end of the audio.
      onEnded: ms => {
        const project = docRef.current?.status === 'ready' ? docRef.current : uiRef.current;
        dispatchUi({
          type: 'transport/seek',
          ticks: msToTick(ms, docRef.current?.bpm ?? BPM),
          totalTicks: projectTicks(project.tracks),
        });
        dispatchUi({ type: 'transport/stop' });
      },
      onError: message => setAudioError(message),
    });
  }

  useEffect(() => {
    let cancelled = false;
    const engine = createEngine({
      kind: backend,
      baseUrl,
      onChange: next => { if (!cancelled) setDoc(next); },
      onWorkerLifecycle: lifecycle => {
        if (cancelled) return;
        setDoc(current => ({
          ...current,
          workerLifecycle: lifecycle,
          ...(lifecycle.status === 'failed' ? { status: 'worker-failed', error: lifecycle.error } : {}),
        }));
      },
      onError: () => { if (!cancelled) setDoc(current => ({ ...current, status: 'mock' })); },
    });
    engineRef.current = engine;
    setDoc({ tracks: [], notes: {}, revision: 0, status: 'connecting' });
    engine.boot({ name: 'OpenUtau Clone' })
      .then(async () => {
        if (cancelled) return;
        if (backend === 'wasm') await restoreActiveProject(engine);
        if (!cancelled) setDoc(current => ({ ...current, status: 'ready' }));
      })
      .catch(() => { if (!cancelled) setDoc({ tracks: uiRef.current.tracks, notes: uiRef.current.notes, revision: 0, status: 'mock' }); });
    return () => { cancelled = true; engine.close(); };
  }, [backend, baseUrl]);

  const live = doc.status === 'ready';
  const tracks = live ? doc.tracks : ui.tracks;
  const notes = live ? doc.notes : ui.notes;
  // USTX stores a path to external audio, not its bytes. Surface missing
  // browser-local assets as soon as the project loads, not only after Play.
  const audioWarning = useMemo(() => {
    if (backend !== 'wasm') return null;
    const part = tracks.flatMap(track => track.parts || [])
      .find(candidate => candidate.kind === 'wave' && (!candidate.media_ref || !candidate.media_ref.startsWith('asset:')));
    return part
      ? `Audio part “${part.name || part.media_ref || 'Audio part'}” still points to the source computer. Use File → Relink Audio to add it to this browser project.`
      : null;
  }, [backend, tracks]);
  // Mock-only vibrato edits live beside the mock document. Connected edits go
  // through note.set_vibrato and are read back from the session snapshot.
  // Not `|| {}`: a fresh object literal here would be a new identity on every
  // render, which would rebuild `activeNotes` on every render, which would
  // re-run every effect that depends on the notes — a render loop that only
  // shows up while something (the transport) updates state every frame.
  const vibratoPatches = ui.vibratoPatches;

  /**
   * The drag preview: what the open gesture has already asked the engine for.
   *
   * A grouped command produces no snapshot change until the group commits, so
   * without this the note sat still under the pointer for the whole drag and
   * jumped on release — it read as if dragging did nothing. The engine publishes
   * this overlay as each batch is accepted and clears it on commit, cancel,
   * undo and redo, so it is a projection of an in-flight request and never a
   * second source of truth: it is never persisted, exported or rendered.
   *
   * `doc.pendingNotes` is a counter that changes on every overlay update, so it
   * is a real dependency — the overlay array identity alone would not be.
   */
  const pendingById = useMemo(
    () => new Map((doc.pending || []).map(note => [note.id, note])),
    [doc.pendingNotes],
  );

  // `allParts` yields { part, track } wrappers; consumers want the part DTO and
  // its owning track, so unwrap once here instead of at every call site. The
  // engine mints its own part ids, so a selection made in the mock document
  // cannot be reused after a session opens: fall back to the first voice part.
  const { part: activePart, track: activeTrack } = useMemo(() => {
    const parts = allParts(tracks);
    return parts.find(({ part }) => part.id === ui.activePartId)
      ?? parts.find(({ part }) => part.kind === 'voice')
      ?? { part: null, track: null };
  }, [tracks, ui.activePartId]);
  const effectivePartId = activePart?.id ?? ui.activePartId;
  const activeNotes = useMemo(
    () => (notes[effectivePartId] || []).map(note => {
      const pending = pendingById.get(note.id);
      const vibrato = vibratoPatches?.[note.id];
      if (!pending && !vibrato) return note;
      return { ...note, ...(pending || {}), ...(vibrato ? { vibrato } : {}) };
    }),
    [notes, effectivePartId, vibratoPatches, pendingById],
  );
  const selectedNotes = useMemo(() => activeNotes.filter(n => ui.selection.has(n.id)), [activeNotes, ui.selection]);

  /**
   * One entry point for panels: UI-only intents stay local, document intents go
   * to the engine when one is connected and to the mock document otherwise.
   */
  const applyIntents = useCallback((intents, group = false, gesture = null) => {
    if (!live || !engineRef.current) { intents.forEach(intent => dispatchUi(intent)); return Promise.resolve({ unsupported: [] }); }
    return engineRef.current.send(intents, { group, gesture })
      .then(({ unsupported = [], changes = [] }) => {
        // The engine mints the ids, so a drawn note can only be selected once
        // the change patch comes back — the frontend never invents them.
        const minted = changes.filter(change => change.kind === 'note.added').map(change => change.note_id).filter(Boolean);
        if (minted.length) dispatchUi({ type: 'notes/select', ids: minted, additive: false });
        return { unsupported };
      })
      .catch(error => ({ unsupported: intents.map(intent => intent.type), error }));
  }, [live]);

  const dispatch = useCallback(intent => {
    if (intent.type.startsWith('transport/')) {
      // Unlocking the AudioContext has to happen inside the click that started
      // the transport; a browser refuses to resume it from a later effect.
      const player = playerRef.current;
      player?.unlock();
      const moving = MOVES_PLAYHEAD.has(intent.type);
      // UI state is only presentation state. Resolve bounds and note onsets from
      // the current Rust snapshot so a long live project is not clamped to the
      // mock document's 14-measure demo part.
      const project = docRef.current?.status === 'ready' ? docRef.current : uiRef.current;
      const totalTicks = projectTicks(project.tracks);
      const target = moving ? transportTarget(uiRef.current, intent, project) : null;
      dispatchUi(moving ? { ...intent, totalTicks, targetTicks: target } : intent);
      // A seek while audio is playing has to move the AUDIO, not just the
      // cursor. Doing it here — where the intent is known — keeps the
      // per-frame clock out of the business of guessing what a jump meant.
      if (moving && player?.playing) {
        stallRef.current.tick = null;
        player.play(target).catch(error => {
          if (error?.code !== 'PLAYBACK_CANCELLED') setAudioError(String(error?.message || error));
        });
      }
      return Promise.resolve({ unsupported: [] });
    }
    if (UI_ONLY.has(intent.type) || intent.type.startsWith('edit/')) { dispatchUi(intent); return Promise.resolve({ unsupported: [] }); }
    // A grouped one-shot (note split) belongs to the gesture that is open, so it
    // shares that gesture's single undo entry.
    return applyIntents([intent], intent.group === true, gestureRef.current?.id ?? null);
  }, [applyIntents]);

  /**
   * Gesture updates are coalesced: a drag fires pointermove far faster than a
   * session can answer, and queueing every frame buries the UI in round trips
   * (each one re-reads the document and repaints, which is what made a drag
   * white out the canvas). One batch stays in flight and only the newest target
   * is kept — safe because move/resize/tone carry absolute positions, so the
   * note lands where the pointer is rather than one frame behind. Notes being
   * drawn or erased accumulate instead: dropping one would lose an edit.
   */
  // The gesture currently open, if any: its transaction id and the promise for
  // the engine's group-open round trip, so a batch can name and wait for it.
  const gestureRef = useRef(null);
  const gestureSeq = useRef(0);
  /** Resolves when the previous gesture's transaction has closed. */
  const closeTailRef = useRef(Promise.resolve());
  const flightRef = useRef({ busy: false, pending: [], promise: null });
  const applyRef = useRef(applyIntents);
  applyRef.current = applyIntents;
  const dispatchGesture = useCallback(intent => {
    const flight = flightRef.current;
    const last = flight.pending[flight.pending.length - 1];
    if (GESTURE_TARGETS.has(intent.type)) flight.pending = [...flight.pending.filter(entry => entry.type !== intent.type), intent];
    else if (intent.type === 'note/remove' && last?.type === 'note/remove') flight.pending = [...flight.pending.slice(0, -1), { ...intent, ids: [...new Set([...last.ids, ...intent.ids])] }];
    else flight.pending = [...flight.pending, intent];
    if (flight.busy) return flight.promise;
    flight.busy = true;
    flight.promise = (async () => {
      try {
        while (flight.pending.length) {
          const batch = flight.pending;
          flight.pending = [];
          // Wait for the gesture's group to exist before sending the batch: the
          // first pointermove can beat the press's `beginGroup` round trip.
          if (gestureRef.current?.promise) { try { await gestureRef.current.promise; } catch { /* send falls back */ } }
          await applyRef.current(batch, true, gestureRef.current?.id ?? null);
        }
      } finally { flight.busy = false; flight.promise = null; }
    })();
    return flight.promise;
  }, []);
  /** Waits for the last coalesced update so a commit cannot cut the gesture short. */
  const settleGesture = useCallback(async () => { if (flightRef.current.promise) await flightRef.current.promise.catch(() => {}); }, []);

  // Gesture transactions: open on press, commit on release, cancel on failure.
  const beginGesture = useCallback(name => {
    // Every gesture gets its own transaction id up front. Sharing "the open
    // group" is what let a press arrive while the previous gesture was still
    // committing and adopt its transaction — which then closed on the new
    // gesture, splitting one drag into several undo entries.
    const id = gestureSeq.current += 1;
    const previous = closeTailRef.current;
    gestureRef.current = { id, promise: null };
    if (!live || !engineRef.current) { dispatchUi({ type: 'edit/begin', edit: { kind: name }, x: 0 }); return Promise.resolve(); }
    // A session allows one open group at a time, so this gesture's open has to
    // wait for the previous gesture's close. Chaining here (rather than inside
    // the engine) is what keeps the order right when a press arrives while the
    // previous release is still draining its last batch.
    gestureRef.current.promise = previous.then(() => engineRef.current.beginGroup(name, id));
    return gestureRef.current.promise;
  }, [live]);
  const endGesture = useCallback(async commit => {
    if (!live || !engineRef.current) { dispatchUi({ type: 'edit/end' }); return; }
    const gesture = gestureRef.current;
    gestureRef.current = null;
    // Registered synchronously: the close must take its place in the chain now,
    // even though the batches still in flight have to drain first.
    const closing = settleGesture().then(() => (commit
      ? engineRef.current.commitGroup(gesture?.id ?? null)
      : engineRef.current.cancelGroup(gesture?.id ?? null)));
    closeTailRef.current = closing.catch(() => {});
    // Notes drawn or split inside a group only get their engine id in the
    // commit patch, so the selection follows from there.
    const changes = await closing;
    const minted = (changes || []).filter(change => change.kind === 'note.added').map(change => change.note_id).filter(Boolean);
    if (minted.length) dispatchUi({ type: 'notes/select', ids: minted, additive: false });
  }, [live]);

  const undo = useCallback(() => (live ? engineRef.current.undo() : Promise.resolve()), [live]);
  const redo = useCallback(() => (live ? engineRef.current.redo() : Promise.resolve()), [live]);

  /**
   * Project lifecycle for the File menu.
   *
   * Every call goes to the engine, which owns the session; the panel state is
   * only cleared afterwards because the ids it holds stop meaning anything when
   * the document is replaced. Without a session there is nothing to save, so the
   * caller gets a readable failure instead of a silent no-op.
   */
  const noSession = action => Promise.reject(new Error(`${action} needs a live Rust session — none is reachable`));
  const lifecycleCall = useCallback((action, run) => {
    if (!live || !engineRef.current) return noSession(action);
    return Promise.resolve(run(engineRef.current)).finally(() => dispatchUi({ type: 'project/loaded' }));
  }, [live]);
  const newProject = useCallback(name => lifecycleCall('New project', engine => engine.newProject({ name: name || 'Untitled' })).then(result => {
    if (backend === 'wasm') setActiveProjectName(null);
    return result;
  }), [backend, lifecycleCall]);
  const openProject = useCallback(name => lifecycleCall(`Open “${name}”`, engine => engine.openProject(name)).then(result => {
    if (backend === 'wasm') setActiveProjectName(name);
    return result;
  }), [backend, lifecycleCall]);
  const importUstx = useCallback((name, ustx, mediaFiles = []) => lifecycleCall('Import USTX', engine => importUstxWithMedia(engine, name, ustx, mediaFiles)).then(result => {
    if (backend === 'wasm') setActiveProjectName(null);
    return result;
  }), [backend, lifecycleCall]);
  const importProjectBundle = useCallback((name, bundle) => lifecycleCall('Import project bundle', engine => engine.importProjectBundle(name, bundle)).then(result => {
    if (backend === 'wasm') setActiveProjectName(null);
    return result;
  }), [backend, lifecycleCall]);
  const saveProject = useCallback(() => lifecycleCall('Save', engine => engine.save()).then(result => {
    if (backend === 'wasm') setActiveProjectName(engineRef.current?.state?.storeName ?? null);
    return result;
  }), [backend, lifecycleCall]);
  const saveProjectAs = useCallback(name => lifecycleCall('Save As', engine => engine.saveAs(name)).then(result => {
    if (backend === 'wasm') setActiveProjectName(engineRef.current?.state?.storeName ?? name);
    return result;
  }), [backend, lifecycleCall]);
  const recoverWorker = useCallback(async () => {
    if (backend !== 'wasm' || !engineRef.current) throw new Error('Direct WASM Worker recovery is unavailable');
    if (!window.confirm('The Rust Worker was lost. Unsaved changes cannot be restored. Restart it and reopen the last saved project, if one is available?')) {
      return { cancelled: true };
    }
    try {
      return await engineRef.current.recover({ reopenStoreName: readActiveProjectName() });
    } catch (error) {
      setDoc(current => ({ ...current, status: 'worker-failed', error }));
      throw error;
    }
  }, [backend]);
  const exportUstx = useCallback(() => (live && engineRef.current ? engineRef.current.exportUstx() : noSession('Export')), [live]);
  const exportProjectBundle = useCallback(() => (live && engineRef.current ? engineRef.current.exportProjectBundle() : noSession('Export bundle')), [live]);
  const listProjects = useCallback(() => (live && engineRef.current ? engineRef.current.listProjects() : Promise.resolve([])), [live]);

  /**
   * Render one track through the engine (§6.13).
   *
   * `track` is the track NUMBER the engine addresses, not a UI list index: the
   * native route rejects negative values and the WASM host passes the value
   * straight into the session's own track ordering. The caller supplies it so
   * this hook never has to guess which entry of `tracks` the UI considers
   * "current" — the engine owns that mapping.
   *
   * The native HTTP backend needs a voicebank NAME (never a filesystem path in
   * the logical API), so it is threaded through rather than guessed here.
   */
  const renderTrack = useCallback(async (track, options = {}) => {
    if (!live || !engineRef.current) return noSession('Render');
    setRenderJob({ status: 'running', track });
    try {
      const result = await engineRef.current.renderTrack(track, options);
      setRenderJob({ status: 'done', track, sampleRate: result.sampleRate, samples: result.frames ?? result.samples?.length ?? 0 });
      return result;
    } catch (error) {
      // A failed render carries a reason from the engine; surfacing it beats
      // handing the player an empty buffer that looks like silence.
      setRenderJob({ status: 'failed', track, error: error.message });
      throw error;
    }
  }, [live]);
  const renderMixdown = useCallback(async (options = {}) => {
    if (!live || !engineRef.current) return noSession('Mixdown export');
    const cacheKey = `${doc.sessionId ?? ''}:${doc.revision ?? 0}:${ui.bankEpoch}`;
    setRenderJob({ status: 'running', kind: 'mixdown', startedAt: Date.now() });
    try {
      const result = await engineRef.current.renderMixdown(options);
      const take = result.takeId
        ? { takeId: result.takeId, sampleRate: result.sampleRate, frames: result.frames }
        : { samples: result.samples, channels: result.channels, sampleRate: result.sampleRate };
      // A late render from an older revision is still a valid export, but must
      // never replace the transport cache for the current document.
      const currentKey = `${docRef.current?.sessionId ?? ''}:${docRef.current?.revision ?? 0}:${bankEpochRef.current}`;
      if (currentKey === cacheKey) mixdownTakeRef.current = { key: cacheKey, take };
      setRenderJob({ status: 'done', kind: 'mixdown', sampleRate: result.sampleRate, samples: result.frames ?? result.samples?.length ?? 0 });
      return result;
    } catch (error) {
      setRenderJob({ status: 'failed', kind: 'mixdown', error: error.message });
      throw error;
    }
  }, [live, doc.sessionId, doc.revision, ui.bankEpoch]);
  const encodeTakeWav = useCallback(takeId => {
    if (!live || !engineRef.current) return noSession('WAV export');
    return engineRef.current.encodeTakeWav(takeId);
  }, [live]);
  const importAudio = useCallback(options => {
    if (!live || !engineRef.current) return noSession('Import audio');
    return engineRef.current.importAudio(options);
  }, [live]);
  const relinkAudio = useCallback(async options => {
    if (!live || !engineRef.current) return noSession('Relink audio');
    return engineRef.current.relinkAudio(options);
  }, [live]);
  const importTracks = useCallback(ustxYaml => {
    if (!live || !engineRef.current) return noSession('Import tracks');
    return engineRef.current.importTracks(ustxYaml);
  }, [live]);

  // A new session id means the document behind every panel was replaced.
  const sessionId = doc.sessionId ?? null;
  const loadedRef = useRef(sessionId);
  useEffect(() => {
    if (loadedRef.current === sessionId) return;
    loadedRef.current = sessionId;
    dispatchUi({ type: 'project/loaded' });
  }, [sessionId]);

  // The preview follows the session revision. The direct-WASM route currently
  // renders the complete active track as one take; its 400 ms debounce delays
  // the start but does not make the render progressive.
  const tracksRef = useRef(tracks);
  tracksRef.current = tracks;
  // Which track the strip draws. The engine numbers tracks in its own snapshot
  // order, so the index in `tracks` IS that number. Following the active track
  // (rather than always track 0) is what makes an AUDIO track drawable: a wave
  // part has no phrases, so only its own track's render carries its waveform.
  const activeTrackNo = Math.max(0, activeTrack ? tracks.indexOf(activeTrack) : 0);
  useEffect(() => {
    if (!live || !engineRef.current) { setRender(IDLE_RENDER); takeRef.current = null; return undefined; }
    let stale = false;
    const signal = { cancelled: false };
    const timer = setTimeout(async () => {
      // The take on hand belongs to whatever the last render was — often the
      // previous track. Drop it the moment a new render starts, so pressing
      // play mid-render can never play audio that is not the audio on screen.
      takeRef.current = null;
      const previewTrack = tracksRef.current[activeTrackNo];
      const renderTrackId = previewTrack?.id ?? null;
      // Keep waveform provenance explicit: Arrange may only project peaks onto
      // parts belonging to the track that produced those samples.
      setRender({ ...IDLE_RENDER, status: 'rendering', trackId: renderTrackId, startedAt: Date.now() });
      try {
        const engine = engineRef.current;
        let audio;
        if (backend === 'wasm') {
          // The WASM session render owns vibrato, track fader/pan and Mix FX.
          // Rendering an exported USTX through RenderPool skipped that chain
          // and could show a waveform different from what the UI would play.
          audio = await engine.renderTrack(activeTrackNo);
          audio.source = 'Rust WASM session';
        } else {
          const ustx = await Promise.resolve(engine.exportUstx());
          if (stale || signal.cancelled) return;
          const track = tracksRef.current[activeTrackNo];
          audio = await renderProjectAudio({
            kind: backend,
            baseUrl,
            sessionId: doc.sessionId ?? null,
            revision: doc.revision,
            ustx,
            track: activeTrackNo,
            trackMix: { volumeDb: track?.volume_db ?? 0, pan: track?.pan ?? 0 },
            singerId: track?.singer_id ?? null,
            signal,
          });
        }
        if (stale) return;
        takeRef.current = audio.takeId
          ? { takeId: audio.takeId, sampleRate: audio.sampleRate, frames: audio.frames }
          : { samples: audio.samples, channels: audio.channels, sampleRate: audio.sampleRate };
        setRender({ status: 'ready', trackId: renderTrackId, peaks: audio.peaks || buildPeaks(audio.channels || audio.samples, audio.sampleRate), source: audio.source, bank: audio.bank || null, error: null, samples: audio.frames ?? audio.samples.length, stats: audio.stats || null });
      } catch (error) {
        if (stale || signal.cancelled) return;
        // The take this track had is now stale: the render that replaced it
        // failed, so playing the old audio would be a lie about the document.
        takeRef.current = null;
        setRender({ ...IDLE_RENDER, status: 'error', error: String(error?.message || error) });
      }
    }, RENDER_DEBOUNCE_MS);
    return () => { stale = true; signal.cancelled = true; clearTimeout(timer); };
    // `backend`/`baseUrl` identify the session; the revision is the document;
    // `bankEpoch` is a voicebank install/uninstall, which changes the samples
    // without touching the project at all.
  }, [live, backend, baseUrl, doc.sessionId, doc.revision, ui.bankEpoch, activeTrackNo]);

  const project = useMemo(() => ({
    name: doc.storeName || doc.documentName || 'Untitled',
    documentName: doc.documentName,
    sessionId: doc.sessionId ?? null,
    dirty: !!doc.dirty,
    busy: !!doc.busy,
    error: doc.error,
    workerLifecycle: doc.workerLifecycle ?? null,
    newProject, openProject, importUstx, importProjectBundle, saveProject, saveProjectAs, recoverWorker, exportUstx, exportProjectBundle, listProjects,
    renderTrack, renderMixdown, encodeTakeWav, importAudio, relinkAudio, importTracks, renderJob, playbackHandoffStartedAt,
  }), [doc.storeName, doc.documentName, doc.sessionId, doc.dirty, doc.busy, doc.error, doc.workerLifecycle, newProject, openProject, importUstx, importProjectBundle, saveProject, saveProjectAs, recoverWorker, exportUstx, exportProjectBundle, listProjects, renderTrack, renderMixdown, encodeTakeWav, importAudio, relinkAudio, importTracks, renderJob, playbackHandoffStartedAt]);

  // A clock advances the playhead while transport is running; it stops at the
  // end of the last part instead of wrapping so the UI never lies about length.
  //
  // While audio is playing the clock is only a VIEW: the position comes from the
  // AudioContext, so the playhead cannot drift away from the sound. The
  // delta-based silent clock stays as the fallback for when there is no rendered
  // audio to play (no bank, no pool) — the transport still moves, it just does
  // not pretend there is a take playing.
  useEffect(() => {
    if (!ui.playing) return undefined;
    const total = projectTicks(tracks);
    const player = playerRef.current;
    let last = performance.now();
    loopRef.current = requestAnimationFrame(function tick(now) {
      let audioTick = audioArmedRef.current && player?.playing ? player.tickNow() : null;
      if (audioTick !== null) {
        // A context that exists but whose clock never runs (no output device,
        // a headless browser) would freeze the playhead. A silent transport
        // that still moves is better than a stuck one, so watch for the stall.
        const stall = stallRef.current;
        if (stall.tick === null || audioTick > stall.tick + 1e-6) { stall.tick = audioTick; stall.at = now; }
        else if (now - stall.at > 600) {
          player.pause();
          audioArmedRef.current = false;
          setAudioError('the browser audio clock is not running');
          audioTick = null;
        }
      }
      let next;
      if (audioTick !== null) {
        next = audioTick;
      } else if (playbackPreparingRef.current) {
        // Rendering the full project can take time; keep the cursor parked
        // until the matching mixdown is ready, then let audio frames own it.
        last = now;
        next = uiRef.current.playheadTicks;
      } else {
        const deltaTicks = (now - last) / 1000 * TICKS_PER_BEAT * ((doc.bpm ?? BPM) / 60);
        last = now;
        next = uiRef.current.playheadTicks + deltaTicks;
      }
      if (next >= total) {
        dispatchUi({ type: 'transport/stop' });
        dispatchUi({ type: 'transport/seek', ticks: total, totalTicks: total });
        player?.pause();
        return;
      }
      dispatchUi({ type: 'transport/seek', ticks: next, totalTicks: total });
      loopRef.current = requestAnimationFrame(tick);
    });
    return () => cancelAnimationFrame(loopRef.current);
  }, [ui.playing, tracks, doc.bpm]);

  // Starting and stopping the audio follows `playing`, not the click: the click
  // is where the AudioContext is unlocked, the effect is where the take starts.
  // A document change restarts it too — the pool re-renders, and playback has to
  // follow the audio that is actually in the cache.
  useEffect(() => {
    const player = playerRef.current;
    if (!player) return;
    if (!ui.playing) {
      setPlaybackHandoffStartedAt(null);
      playbackPreparingRef.current = false;
      audioArmedRef.current = false;
      playbackSessionKeyRef.current = null;
      player.pause();
      return;
    }
    const cacheKey = `${doc.sessionId ?? ''}:${doc.revision ?? 0}:${ui.bankEpoch}`;
    if (playbackSessionKeyRef.current && playbackSessionKeyRef.current !== cacheKey) {
      playbackSessionKeyRef.current = null;
      playbackPreparingRef.current = false;
      audioArmedRef.current = false;
      player.pause();
      setPlaybackHandoffStartedAt(null);
      setAudioError('Project audio changed. Press Play to start the updated version.');
      dispatchUi({ type: 'transport/stop' });
      return;
    }
    let cancelled = false;
    playbackSessionKeyRef.current = cacheKey;
    audioArmedRef.current = true;
    playbackPreparingRef.current = true;
    const trace = import.meta.env?.DEV ? (() => {
      const startedAt = performance.now();
      const traceId = `play-${Math.round(startedAt)}`;
      return {
        firstPcmReceived: false,
        firstPcmEnqueued: false,
        firstAudibleMeter: false,
        mark(stage, fields = {}) {
          console.info('[TTFS]', {
            traceId,
            stage,
            elapsedMs: Number((performance.now() - startedAt).toFixed(1)),
            ...fields,
          });
        },
      };
    })() : null;
    trace?.mark('play_effect_started');
    setPlaybackHandoffStartedAt(null);
    stallRef.current.tick = null;
    (async () => {
      try {
        const ensureMixdown = async () => {
          let cached = mixdownTakeRef.current;
          if (cached?.key !== cacheKey) {
            let pending = mixdownPromiseRef.current;
            if (pending?.key !== cacheKey) {
              trace?.mark('mixdown_render_started');
              pending = { key: cacheKey, promise: renderMixdown() };
              mixdownPromiseRef.current = pending;
            } else trace?.mark('mixdown_waiting_for_existing_render');
            try { await pending.promise; }
            finally { if (mixdownPromiseRef.current === pending) mixdownPromiseRef.current = null; }
            if (cancelled) return;
            cached = mixdownTakeRef.current;
            trace?.mark('mixdown_render_ready', {
              sampleRate: cached?.take?.sampleRate ?? null,
              frames: cached?.take?.frames ?? cached?.take?.samples?.length ?? null,
            });
          } else {
            trace?.mark('mixdown_cache_hit', {
              sampleRate: cached.take?.sampleRate ?? null,
              frames: cached.take?.frames ?? cached.take?.samples?.length ?? null,
            });
          }
          if (cached?.key !== cacheKey) throw new Error('The full-project mixdown did not produce a playable take');
        };
        let streamConfig = null;
        if (backend === 'http' && docRef.current?.sessionId) {
          trace?.mark('credential_request_started');
          const credential = await requestPlaybackCredential(baseUrlRef.current);
          if (cancelled) return;
          trace?.mark(credential ? 'credential_ready' : 'credential_unavailable');
          if (credential) {
            streamConfig = {
              baseUrl: baseUrlRef.current,
              sessionId: docRef.current.sessionId,
              revision: docRef.current.revision,
              credential,
            };
            pendingSessionStreamRef.current = streamConfig;
          }
        }
        if (!streamConfig) await ensureMixdown();
        if (cancelled) return;
        setPlaybackHandoffStartedAt(Date.now());
        let tick;
        trace?.mark('player_play_called');
        try {
          tick = await player.play(uiRef.current.playheadTicks, trace);
        } catch (error) {
          pendingSessionStreamRef.current = null;
          if (!streamConfig || cancelled || error?.code === 'PLAYBACK_CANCELLED') throw error;
          // A grant may be available before the server enables render.stream,
          // or this browser may not support the streaming transport. Build the
          // normal completed mixdown and retry playback through the fallback.
          await ensureMixdown();
          if (cancelled) return;
          trace?.mark('mixdown_fallback_retry');
          tick = await player.play(uiRef.current.playheadTicks, trace);
        }
        if (!cancelled) {
          playbackPreparingRef.current = false;
          setPlaybackHandoffStartedAt(null);
          setAudioError(null);
          dispatchUi({ type: 'transport/seek', ticks: tick, totalTicks: projectTicks(tracks) });
        }
      } catch (error) {
        if (cancelled) return;
        pendingSessionStreamRef.current = null;
        playbackPreparingRef.current = false;
        audioArmedRef.current = false;
        setPlaybackHandoffStartedAt(null);
        setAudioError(String(error?.message || error));
      }
    })();
    return () => { cancelled = true; playbackPreparingRef.current = false; };
  }, [ui.playing, doc.sessionId, doc.revision, ui.bankEpoch, backend, baseUrl, tracks, renderMixdown]);

  // Dev-only handle so the probe can read what the transport is really doing
  // (is audio running, where does the audio clock say we are) without
  // instrumenting the player.
  if (import.meta.env?.DEV) window.__ouAudio = playerRef.current;

  // The AudioContext belongs to the page, so the page gives it back.
  useEffect(() => () => playerRef.current?.dispose(), []);

  /**
   * Output-device controls.
   *
   * Both need a live `AudioContext`, and a context needs a user gesture, so
   * they are callbacks the transport calls from its click rather than effects.
   * A failed selection surfaces as `audioError` rather than being swallowed: a
   * browser that cannot honour the request is exactly the kind of thing that
   * looks like "the app is silent".
   */
  const chooseOutput = useCallback(async id => {
    try {
      const applied = await playerRef.current?.setOutputDevice(id);
      setOutputDevice(applied ?? '');
      setAudioError(null);
    } catch (error) {
      setAudioError(String(error?.message || error));
    }
  }, []);

  const refreshOutputs = useCallback(async () => {
    setOutputs(await (playerRef.current?.listOutputs() ?? []));
  }, []);

  const setVolume = useCallback(value => {
    setVolumeState(playerRef.current?.setVolume(value) ?? 1);
  }, []);

  const value = useMemo(() => ({
    // `state` keeps its old name for panels; `document` is the engine view.
    backendKind: backend,
    baseUrl,
    state: { ...ui, tracks, notes, activePartId: effectivePartId },
    ui,
    doc,
    connected: live,
    dispatch,
    dispatchGesture,
    beginGesture,
    endGesture,
    undo,
    redo,
    project,
    // The engine's tempo, not a UI constant: the preview aligns audio seconds
    // to the roll's ticks with it.
    bpm: doc.bpm ?? BPM,
    waveform: render,
    // Non-null when the transport is moving without audio and says why.
    audio: { error: audioError, warning: audioWarning, outputs, outputDevice, volume, player: playerRef.current, chooseOutput, refreshOutputs, setVolume },
    activePart,
    activeTrack,
    playheadTicks: ui.playheadTicks,
    activeNotes,
    selectedNotes,
    tracks,
    notes,
    snap: ui.snap,
    tool: ui.tool,
  }), [backend, baseUrl, ui, tracks, notes, effectivePartId, doc, live, dispatch, dispatchGesture, beginGesture, endGesture, undo, redo, project, render, audioError, audioWarning, activePart, activeTrack, activeNotes, selectedNotes, outputs, outputDevice, volume, chooseOutput, refreshOutputs, setVolume]);

  return <CloneContext.Provider value={value}>{children}</CloneContext.Provider>;
}

export function useClone() {
  const value = useContext(CloneContext);
  if (!value) throw new Error('useClone must be used inside <CloneProvider>');
  return value;
}

/** Beats → `mm:ss.mmm`, matching the transport chip in the menu bar. */
export function formatClock(ticks, bpm = BPM) {
  const seconds = ticks / TICKS_PER_BEAT * 60 / (Number(bpm) > 0 ? Number(bpm) : BPM);
  const mm = String(Math.floor(seconds / 60)).padStart(2, '0');
  const ss = String(Math.floor(seconds % 60)).padStart(2, '0');
  const ms = String(Math.floor((seconds % 1) * 1000)).padStart(3, '0');
  return `${mm}:${ss}.${ms}`;
}

/** Screenshot-faithful rendering of the track volume fader position. */
export const volumePercent = volumeDb => Math.max(0, Math.min(1, (volumeDb + 40) / 46));
export { EXPRESSION_LANES, clampToLane, TICKS_PER_BEAT, TICKS_PER_MEASURE, BPM, TIME_SIG };
