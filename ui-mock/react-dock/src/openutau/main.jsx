// OpenUtau-style UI clone shell: Dockview host with one dockable panel per
// area (Arrangement, Piano Roll, Note Properties). Canvas areas use PixiJS 8
// via @pixi/react, matching the main editor stack so panels can migrate into
// the real app one by one. All three panels read the same document through
// `CloneProvider`, so a part clicked in the arrangement is the part the roll
// and the properties panel are showing.
import React, { useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { DockviewReact, themeLight } from 'dockview-react';
import 'dockview/dist/styles/dockview.css';
import { ArrangementPanel } from './arrangement-panel.jsx';
import { PianoRollPanel } from './piano-roll-panel.jsx';
import { NotePropertiesPanel } from './note-properties-panel.jsx';
import { ExpressionLanesPanel } from './expression-lanes-panel.jsx';
import { DockManager } from './dock-manager.jsx';
import { FileMenu } from './file-menu.jsx';
import { EditMenu, ProjectMenu, ToolsMenu } from './editor-menus.jsx';
import { CloneProvider, TIME_SIG, formatClock, useClone } from './clone-state.jsx';
import { initTheme, setTheme, themeKey, themeList, useTheme } from './palette.js';
import './openutau.css';

const PANEL_NAMES = { arrangement: 'Arrangement', roll: 'Piano Roll', lanes: 'Expression Lanes', properties: 'Note Properties' };
/** Share of the stage height the arrangement gets; the roll takes the rest. */
const ARRANGEMENT_RATIO = 0.3;
/** Width of the Note Properties column, in CSS pixels. */
const PROPERTIES_WIDTH = 300;
/** Share of the stage height the expression lanes get. */
const LANES_RATIO = 0.26;

function layoutPanels() {
  return [
    { id: 'arrangement', component: 'arrangement', title: PANEL_NAMES.arrangement, initialHeight: 300 },
    { id: 'roll', component: 'roll', title: PANEL_NAMES.roll, position: { referencePanel: 'arrangement', direction: 'below' } },
    // OpenUtau keeps the properties column narrow; `initialWidth` keeps the
    // splitter there instead of splitting the roll group vertically.
    // The expression strip used to live inside the roll panel; as its own dock
    // it can be resized, floated or closed independently.
    { id: 'lanes', component: 'lanes', title: PANEL_NAMES.lanes, position: { referencePanel: 'roll', direction: 'below' }, initialHeight: 210 },
    { id: 'properties', component: 'properties', title: PANEL_NAMES.properties, position: { referencePanel: 'roll', direction: 'right' }, initialWidth: 300 },
  ];
}

function HistoryBar() {
  const { doc, connected, undo, redo } = useClone();
  // Ctrl/Cmd+Z and Ctrl/Cmd+Shift+Z go to the engine's history, not the DOM.
  useEffect(() => {
    const onKey = event => {
      if (!(event.ctrlKey || event.metaKey) || event.key.toLowerCase() !== 'z') return;
      event.preventDefault();
      (event.shiftKey ? redo : undo)();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [undo, redo]);
  return <span className="ou-history">
    <button type="button" title="Undo (Ctrl+Z)" disabled={!connected} onClick={() => undo()}>↶</button>
    <button type="button" title="Redo (Ctrl+Shift+Z)" disabled={!connected} onClick={() => redo()}>↷</button>
    <span className="ou-chip">rev {doc.revision}</span>
  </span>;
}

/** Bindings shared by the transport buttons and their keyboard shortcuts, so a
 *  tooltip can never advertise a key the handler stopped listening for. */
const SKIP_KEYS = {
  ArrowRight: ['transport/skip-measure', 1],
  ArrowLeft: ['transport/skip-measure', -1],
  '.': ['transport/skip-note', 1],
  ',': ['transport/skip-note', -1],
};

function TransportBar() {
  // `bpm` comes from the engine snapshot (`project.bpm`), not from the UI
  // constant: the demo project is 240 BPM while the UI default is 120, and a
  // chip that says 120 while the audio plays at 240 makes every timing question
  // (why does the playhead not match the waveform?) look like a bug that is not
  // there. Same number the player and the roll use, from one source.
  const { state, dispatch, connected, waveform, audio, bpm } = useClone();
  // Play renders the entire project mixdown; `waveform` is only the selected
  // track's preview and can fail independently (for example, when that singer
  // has no installed bank). Do not report that as a silent project transport.
  const playTitle = state.playing
    ? (audio?.player?.playing
      ? `Pause (Space)${audio?.warning ? ` — ${audio.warning}` : ''}`
      : audio?.error
        ? `Pause (Space) — ${audio.error}`
        : 'Rendering project mixdown…')
    : 'Play (Space) — render and play the full project mixdown';
  // Stop and restart are separate on purpose: stop is for "freeze and look at
  // this", restart is for "play it again from the top".
  useEffect(() => {
    const onKey = event => {
      // Typing in a field must not scrub the playhead.
      if (event.ctrlKey || event.metaKey || event.altKey) return;
      const target = event.target;
      if (target instanceof HTMLElement && (target.isContentEditable || /^(input|textarea|select)$/i.test(target.tagName))) return;
      const key = SKIP_KEYS[event.key];
      if (key) { event.preventDefault(); dispatch({ type: key[0], direction: key[1] }); return; }
      switch (event.key) {
        case ' ': event.preventDefault(); dispatch({ type: 'transport/toggle' }); break;
        // Home is stop-and-rewind but stays silent; Backspace restarts playing,
        // which is what you want after rewinding during a take.
        case 'Home': event.preventDefault(); dispatch({ type: 'transport/stop' }); dispatch({ type: 'transport/seek', ticks: 0 }); break;
        case 'Backspace': event.preventDefault(); dispatch({ type: 'transport/restart', playing: true }); break;
        default: break;
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [dispatch]);
  const skip = (type, direction) => dispatch({ type, direction });
  return <span className="ou-transport-bar">
    <button type="button" className="ou-transport" aria-pressed={state.playing} title={playTitle} onClick={() => dispatch({ type: 'transport/toggle' })}>{state.playing ? '❚❚' : '▶'}</button>
    <button type="button" className="ou-transport" title="Stop, keeping the playhead where it is (Home)" onClick={() => dispatch({ type: 'transport/stop' })}>■</button>
    <button type="button" className="ou-transport" title="Back to the start and play (Backspace)" onClick={() => dispatch({ type: 'transport/restart', playing: true })}>↺</button>
    <span className="ou-transport-group" role="group" aria-label="Skip">
      <button type="button" className="ou-transport" title="Previous measure (←)" onClick={() => skip('transport/skip-measure', -1)}>⏮</button>
      <button type="button" className="ou-transport" title="Previous note (,)" onClick={() => skip('transport/skip-note', -1)}>◁</button>
      <button type="button" className="ou-transport" title="Next note (.)" onClick={() => skip('transport/skip-note', 1)}>▷</button>
      <button type="button" className="ou-transport" title="Next measure (→)" onClick={() => skip('transport/skip-measure', 1)}>⏭</button>
    </span>
    <HistoryBar />
    <span className="ou-chip">{TIME_SIG}</span>
    <span className="ou-chip" title="Tempo from the engine snapshot">{bpm.toFixed(1)}</span>
    <span className="ou-chip ou-chip-clock">{formatClock(state.playheadTicks, bpm)}</span>
    <span className={`ou-link-state ${connected ? 'live' : 'mock'}`} title={connected ? 'Editing the Rust project session' : 'No engine session: the page is showing the mock document'}>
      {connected ? 'Rust session' : 'Mock only'}
    </span>
  </span>;
}

function MenuBar({ backend, onBackend, onOpenManager, onLayout }) {
  // Subscribing here repaints every canvas panel on a theme switch, because
  // each Pixi draw callback reads the palette through `P()`.
  useTheme();
  const { project } = useClone();
  return <nav className="ou-menubar">
    <FileMenu />
    <EditMenu />
    <ProjectMenu />
    <ToolsMenu onLayout={onLayout} />
    <span className={`ou-project-chip${project.dirty ? ' dirty' : ''}`}
      title={project.documentName && project.documentName !== project.name ? `${project.name} · document name “${project.documentName}”` : `Session ${project.sessionId || 'none'}`}>
      {project.name}{project.dirty && <span className="ou-dirty-dot" aria-label="unsaved changes" />}
    </span>
    <span className="ou-menu-grow" />
    <button type="button" className="ou-menu ou-menu-manager" title="Show, hide and re-open dock panels"
      onClick={() => onOpenManager()}>⚙ Dock Manager</button>
    <label className="ou-backend-switch" title="Colour theme — light, dark, or a preset from src/openutau/themes">
      Theme
      <select className="ou-theme-select" value={themeKey()} onChange={event => setTheme(event.target.value)}>
        {themeList().map(entry => <option key={entry.key} value={entry.key}>{entry.label}</option>)}
      </select>
    </label>
    <label className="ou-backend-switch" title="Which Rust session the panels edit">
      Session
      <select value={backend.kind} onChange={event => onBackend({ ...backend, kind: event.target.value })}>
        <option value="wasm">Direct WASM</option>
        <option value="http">HTTP server</option>
      </select>
    </label>
    <TransportBar />
  </nav>;
}

const COMPONENTS = { arrangement: () => <ArrangementPanel />, roll: () => <PianoRollPanel />, lanes: () => <ExpressionLanesPanel />, properties: () => <NotePropertiesPanel /> };

function OpenUtauClone() {
  const hostRef = useRef(null);
  // `wasm` drives the engine in-process; `http` talks to synth-server. Both go
  // through the same contract, and the page falls back to mock data if neither
  // is reachable so the UI can always be reviewed.
  const [backend, setBackend] = useState(() => {
    const params = new URLSearchParams(window.location.search);
    const kind = params.get('backend') || (params.get('server') ? 'http' : 'wasm');
    return { kind, baseUrl: params.get('server') || 'http://127.0.0.1:8765' };
  });
  const apiRef = useRef(null);
  // Panels closed with their tab's ✕ are otherwise unrecoverable: the layout is
  // fixed at mount, so the manager is the only way back.
  const [managerOpen, setManagerOpen] = useState(false);
  // Dockview only knows the stage is `X` pixels tall once the shell tells it;
  // without `layout` the groups keep their initial sizes and overflow. Sizing is
  // keyed by the top-level group id so the nested roll/properties split keeps
  // its own proportions.
  const relayout = () => {
    const api = apiRef.current, host = hostRef.current;
    if (!api || !host) return;
    const byPanel = Object.fromEntries(api.groups.map(group => [group.panels[0]?.id, group]));
    const width = host.clientWidth, height = host.clientHeight;
    if (!byPanel.arrangement || width <= 0 || height <= 0) return;
    // `api.layout` only sets the overall viewport; the per-axis split comes from
    // each group's own `setSize`, so the arrangement stays a short strip and the
    // properties column stays narrow like OpenUtau's.
    api.layout(width, height, true);
    byPanel.arrangement.api.setSize({ height: Math.round(height * ARRANGEMENT_RATIO) });
    byPanel.properties?.api.setSize({ width: PROPERTIES_WIDTH });
    byPanel.roll?.api.setSize({ width: Math.max(240, width - PROPERTIES_WIDTH) });
    byPanel.lanes?.api.setSize({ height: Math.round(height * LANES_RATIO) });
    // The nested roll/properties split keeps the height it had before the resize
    // until the next frame, so relay the viewport size once more afterwards.
    requestAnimationFrame(() => api.layout(width, height, true));
  };
  const applyLayout = (mode, ratio = 1) => {
    const api = apiRef.current, host = hostRef.current;
    if (!api || !host) return;
    const width = host.clientWidth, height = host.clientHeight;
    if (width <= 0 || height <= 0) return;
    api.layout(width, height, true);
    const groupFor = id => api.groups.find(group => group.panels.some(panel => panel.id === id));
    const arrangement = groupFor('arrangement');
    const properties = groupFor('properties');
    const lanes = groupFor('lanes');
    const arrangementRatio = mode === 'horizontal' ? 1 / (ratio + 1) : ARRANGEMENT_RATIO;
    const propertiesWidth = mode === 'vertical' ? width / (ratio + 1) : PROPERTIES_WIDTH;
    arrangement?.api.setSize({ height: Math.round(height * arrangementRatio) });
    properties?.api.setSize({ width: Math.round(propertiesWidth) });
    lanes?.api.setSize({ height: Math.round(height * LANES_RATIO) });
    requestAnimationFrame(() => api.layout(width, height, true));
  };
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return undefined;
    relayout();
    const observer = new ResizeObserver(relayout);
    observer.observe(host);
    return () => observer.disconnect();
  }, []);
  return <CloneProvider backend={backend.kind} baseUrl={backend.baseUrl}>
    <div className="ou-app">
      <MenuBar backend={backend} onBackend={setBackend} onOpenManager={() => setManagerOpen(true)} onLayout={applyLayout} />
      <div className="ou-dock-stage" ref={hostRef}>
        <DockviewReact
          className="ou-dockview"
          theme={themeLight}
          components={COMPONENTS}
          onReady={event => {
            apiRef.current = event.api;
            layoutPanels().forEach(panel => event.api.addPanel(panel));
            // Panels are added in one pass, so the split only exists afterwards.
            queueMicrotask(relayout);
          }}
          watermarkComponent={() => <span className="ou-watermark">Panels can be dragged, floated and closed — reopen one from ⚙ Dock Manager</span>}
        />
        {managerOpen && <DockManager api={apiRef.current} onClose={() => setManagerOpen(false)} />}
      </div>
      <StatusLine />
    </div>
  </CloneProvider>;
}

/**
 * Says plainly which document is on screen: the engine snapshot when a session
 * is live, the mock fallback otherwise. A silent mock fallback would let the
 * page look correct while nothing reaches Rust.
 */
function formatPreviewElapsed(milliseconds) {
  const seconds = Math.max(0, Math.floor(milliseconds / 1000));
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const remainder = seconds % 60;
  return hours
    ? `${hours}:${String(minutes).padStart(2, '0')}:${String(remainder).padStart(2, '0')}`
    : `${String(minutes).padStart(2, '0')}:${String(remainder).padStart(2, '0')}`;
}

function StatusLine() {
  const { state, activePart, selectedNotes, connected, doc, project, waveform, audio } = useClone();
  const [recoveringWorker, setRecoveringWorker] = useState(false);
  const levelDb = useOutputLevel(audio?.player);
  const noteCount = Object.values(state.notes).flat().length;
  const isRendering = waveform?.status === 'rendering';
  const startedAt = waveform?.startedAt;
  const mixdownJob = project?.renderJob?.kind === 'mixdown' ? project.renderJob : null;
  const isMixdownRendering = Boolean(state.playing && mixdownJob?.status === 'running');
  const mixdownStartedAt = mixdownJob?.startedAt;
  const handoffStartedAt = project?.playbackHandoffStartedAt;
  const isAudioHandoff = Boolean(state.playing && handoffStartedAt && !isMixdownRendering);
  const [clockNow, setClockNow] = useState(() => Date.now());
  useEffect(() => {
    if (!isRendering && !isMixdownRendering && !isAudioHandoff) return undefined;
    const update = () => setClockNow(Date.now());
    update();
    const timer = window.setInterval(update, 1000);
    return () => window.clearInterval(timer);
  }, [isRendering, isMixdownRendering, isAudioHandoff, startedAt, mixdownStartedAt, handoffStartedAt]);
  const render = waveform?.status === 'ready'
    ? `selected-track preview ${(waveform.samples / 44100).toFixed(1)}s from ${waveform.source}`
    : isRendering ? `rendering selected-track preview… elapsed ${formatPreviewElapsed(startedAt ? clockNow - startedAt : 0)}`
      : waveform?.error ? `selected-track preview unavailable: ${waveform.error}`
        : 'no selected-track preview yet';
  const workerFailed = project.workerLifecycle?.status === 'failed' || doc.status === 'worker-failed';
  const needsProjectReopen = doc.status === 'reopen-required';
  return <footer className="ou-status">
    <span className={connected ? 'ou-status-live' : 'ou-status-mock'}>
      {connected ? `Rust session · rev ${doc.revision} · every edit goes through project-session` : 'Mock document — no engine session reachable'}
    </span>
    {workerFailed && <span className="ou-worker-recovery" role="alert">
      Rust Worker stopped. Unsaved changes cannot be restored.
      <button type="button" disabled={recoveringWorker} onClick={async () => {
        setRecoveringWorker(true);
        try { await project.recoverWorker(); } catch { /* the status line keeps the failure visible */ }
        finally { setRecoveringWorker(false); }
      }}>{recoveringWorker ? 'restarting…' : 'Restart Worker'}</button>
    </span>}
    {needsProjectReopen && <span className="ou-worker-recovery" role="status">
      {doc.error
        ? `Worker restarted, but the saved project could not be opened: ${doc.error.message || doc.error}. Choose File → Open to select a project.`
        : 'Worker restarted with no project loaded. Open a saved project from File → Open.'}
    </span>}
    <span className={waveform?.status === 'ready' ? 'ou-status-live' : 'ou-status-mock'} title={render}>{render}</span>
    {isMixdownRendering && <span className="ou-status-mock">project mixdown rendering… elapsed {formatPreviewElapsed(mixdownStartedAt ? clockNow - mixdownStartedAt : 0)}</span>}
    {isAudioHandoff && <span className="ou-status-mock">project mixdown ready · preparing audio output… elapsed {formatPreviewElapsed(handoffStartedAt ? clockNow - handoffStartedAt : 0)}</span>}
    {audio?.error && state.playing && <span className="ou-status-mock" title="The transport is moving, but silently">silent: {audio.error}</span>}
    {audio?.warning && <span className="ou-status-mock" title={audio.warning}>media: relink needed</span>}
    <OutputMeter levelDb={levelDb} />
    <label className="ou-output" title="Where the sound goes. The browser picks this on its own, and a machine with several devices can route it somewhere with no speaker.">
      output
      <select
        value={audio?.outputDevice ?? ''}
        onChange={e => audio?.chooseOutput?.(e.target.value)}
        onPointerDown={() => audio?.refreshOutputs?.()}
      >
        <option value="">default</option>
        {(audio?.outputs ?? []).map(d => <option key={d.id} value={d.id}>{d.label}</option>)}
      </select>
    </label>
    <label className="ou-output" title="Master output level of the transport">
      vol
      <input
        type="range"
        min="0"
        max="1.5"
        step="0.01"
        value={audio?.volume ?? 1}
        onChange={e => audio?.setVolume?.(Number(e.target.value))}
      />
    </label>
    {doc.error && <span className="ou-status-mock" title={String(doc.error)}>last error: {doc.error.message || String(doc.error)}</span>}
    <span>part {activePart?.name || 'none'}</span>
    <span>{state.tracks.length} track{state.tracks.length === 1 ? '' : 's'} · {noteCount} notes</span>
    <span>{selectedNotes.length} selected</span>
  </footer>;
}

/**
 * Poll the master output level, in dBFS, or null when nothing is running.
 *
 * A transport that says "playing" but cannot tell you its own output level is
 * the reason a silent render can sit there looking like a rendering problem: a
 * meter that never moves proves the page produced nothing, whereas a meter that
 * moves while the room stays quiet proves the samples went somewhere else.
 *
 * The poll stops while the player is not playing so an idle page costs nothing.
 */
function useOutputLevel(player) {
  const [level, setLevel] = useState(null);
  useEffect(() => {
    if (!player) return undefined;
    let raf = 0;
    const tick = () => {
      const db = player.playing ? player.levelDb() : null;
      // Only publish a real change: a re-render every frame would repaint the
      // whole footer for a number the reader cannot see move that fast.
      setLevel(prev => {
        if (db === null) return null;
        if (prev !== null && Math.abs(prev - db) < 0.5) return prev;
        return db;
      });
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [player]);
  return level;
}

/** A peak/RMS bar for the transport, -60..0 dBFS, with the number beside it. */
function OutputMeter({ levelDb }) {
  // -60 dBFS is the floor of the scale; anything quieter is silence for our
  // purposes, and a bar creeping at -80 only looks like noise.
  const db = levelDb == null ? null : Math.max(-60, levelDb);
  const pct = db == null ? 0 : Math.min(100, (db + 60) / 60 * 100);
  return <span className="ou-output" title={levelDb == null ? 'No output running' : `Output ${levelDb.toFixed(1)} dBFS`}>
    <span className="ou-meter" aria-hidden="true"><span className="ou-meter-fill" style={{ width: `${pct}%` }} /></span>
    <span className="ou-meter-db">{levelDb == null ? '—' : `${levelDb.toFixed(0)} dB`}</span>
  </span>;
}

// Theme tokens must exist before the first paint, or the shell flashes the
// stylesheet's fallbacks.
initTheme();

createRoot(document.getElementById('root')).render(<OpenUtauClone />);
