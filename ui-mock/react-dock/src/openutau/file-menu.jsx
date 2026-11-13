// The File menu: New / Open / Save / Save As / Export, wired to the real
// project lifecycle in `session-backends.js`. Nothing here builds a document —
// every call goes to the engine session and the panels re-read the snapshot it
// answers with. What this file owns is dialog state, which is disposable.
import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useClone } from './clone-state.jsx';
import { MAX_PROJECT_BUNDLE_BYTES, PROJECT_BUNDLE_EXTENSION, readProjectBundle } from './project-bundle.js';
import { importName } from './ustx-import.js';
import { encodeStereoPcm16Wav } from './pcm-wav.js';
import { formatExportElapsed, formatExportProgress, runExportTask } from './export-progress.js';

/**
 * Save frame-aligned engine output as a stereo PCM WAV download. The engine
 * owns the render and supplies both channels; this only packages that result
 * for the browser's file-save adapter.
 */
function downloadWavBytes(name, bytes) {
  if (!(bytes instanceof ArrayBuffer) && !ArrayBuffer.isView(bytes)) {
    throw new Error('the WAV encoder did not return bytes');
  }
  const url = URL.createObjectURL(new Blob([bytes], { type: 'audio/wav' }));
  const link = document.createElement('a');
  link.href = url;
  link.download = `${name}.wav`;
  document.body.appendChild(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 2000);
}

function downloadWav(name, render, reportPhase) {
  reportPhase?.('encoding');
  const bytes = encodeStereoPcm16Wav(render?.channels, render?.sampleRate);
  reportPhase?.('download');
  downloadWavBytes(name, bytes);
}

async function downloadRenderedWav(name, render, encodeTakeWav, reportPhase) {
  if (!render?.takeId) {
    downloadWav(name, render, reportPhase);
    return render;
  }
  if (typeof encodeTakeWav !== 'function') {
    throw new Error('worker-owned audio cannot be encoded as WAV');
  }
  reportPhase?.('encoding');
  const encoded = await encodeTakeWav(render.takeId);
  reportPhase?.('download');
  downloadWavBytes(name, encoded.wavBytes);
  return { ...render, sampleRate: encoded.sampleRate, frames: encoded.frames };
}

/** Save a portable USTX the way a desktop host would: as a download. */
function download(name, text) {
  const url = URL.createObjectURL(new Blob([text], { type: 'text/yaml;charset=utf-8' }));
  const link = document.createElement('a');
  link.href = url;
  link.download = `${name}.ustx`;
  document.body.appendChild(link);
  link.click();
  link.remove();
  // Revoking immediately can cancel the download in some browsers.
  setTimeout(() => URL.revokeObjectURL(url), 2000);
}

function downloadProjectBundle(name, bytes) {
  const safeName = String(name || 'Untitled').replace(/[\\/:*?"<>|\u0000-\u001f]/g, '_').trim() || 'Untitled';
  const url = URL.createObjectURL(new Blob([bytes], { type: 'application/zip' }));
  const link = document.createElement('a');
  link.href = url;
  link.download = `${safeName}${PROJECT_BUNDLE_EXTENSION}`;
  document.body.appendChild(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 2000);
}

/** Shared modal frame: backdrop, Escape to dismiss, focus moved into the body. */
function Dialog({ title, children, onClose, width = 360 }) {
  const boxRef = useRef(null);
  useEffect(() => {
    const onKey = event => { if (event.key === 'Escape') { event.stopPropagation(); onClose(); } };
    window.addEventListener('keydown', onKey);
    // Focus the first control so Enter submits and Escape works without a click.
    const first = boxRef.current?.querySelector('input, button');
    first?.focus();
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);
  return <div className="ou-dialog-backdrop" onPointerDown={event => { if (event.target === event.currentTarget) onClose(); }}>
    <div className="ou-dialog" style={{ width }} role="dialog" aria-modal="true" aria-label={title} ref={boxRef}>
      <h2 className="ou-dialog-title">{title}</h2>
      {children}
    </div>
  </div>;
}

/** Name prompt, used by New and Save As. */
function NameDialog({ title, label, initial, confirmLabel, onConfirm, onClose }) {
  const [value, setValue] = useState(initial ?? '');
  const submit = () => { const name = value.trim(); if (name) onConfirm(name); };
  return <Dialog title={title} onClose={onClose}>
    <label className="ou-dialog-label">{label}
      <input className="ou-dialog-input" value={value} autoFocus
        onChange={event => setValue(event.target.value)}
        onKeyDown={event => { if (event.key === 'Enter') { event.preventDefault(); submit(); } }} />
    </label>
    <div className="ou-dialog-actions">
      <button type="button" className="ou-button" onClick={onClose}>Cancel</button>
      <button type="button" className="ou-button ou-button-primary" disabled={!value.trim()} onClick={submit}>{confirmLabel}</button>
    </div>
  </Dialog>;
}

/** Project picker: the names the host store lists, newest first. */
function OpenDialog({ names, current, onPick, onClose }) {
  return <Dialog title="Open project" onClose={onClose} width={420}>
    <div className="ou-project-list">
      {names.length === 0 && <p className="ou-dialog-empty">The host store has no saved projects yet.</p>}
      {names.map(name => <button key={name} type="button" className={`ou-project-item${name === current ? ' active' : ''}`}
        onClick={() => onPick(name)}>
        <span className="ou-project-name">{name}</span>
        {name === current && <span className="ou-project-tag">open</span>}
      </button>)}
    </div>
    <div className="ou-dialog-actions">
      <button type="button" className="ou-button" onClick={onClose}>Cancel</button>
    </div>
  </Dialog>;
}

/** Guard rail: opening something else throws unsaved edits away. */
function ConfirmDialog({ title, message, confirmLabel, onConfirm, onClose }) {
  return <Dialog title={title} onClose={onClose}>
    <p className="ou-dialog-message">{message}</p>
    <div className="ou-dialog-actions">
      <button type="button" className="ou-button" onClick={onClose}>Cancel</button>
      <button type="button" className="ou-button ou-button-primary" onClick={onConfirm}>{confirmLabel}</button>
    </div>
  </Dialog>;
}

/**
 * Import USTX. The file is read here but only ever handed to the engine:
 * parsing, validation and ID minting are all `imported_ustx` semantics in Rust
 * (§6.11), so a malformed file fails with the engine's own error instead of a
 * half-imported document.
 */
function ImportDialog({ onClose, onImport, busy }) {
  const [file, setFile] = useState(null);
  const [mediaFiles, setMediaFiles] = useState([]);
  const [error, setError] = useState(null);
  const accept = '.ustx,.yaml,.yml';
  const pick = async event => {
    const picked = event.target.files?.[0];
    if (!picked) { setFile(null); setMediaFiles([]); return; }
    try {
      const text = await picked.text();
      // A USTX is YAML with a `tracks:` list. Checking the shape here is a
      // courtesy, not validation — the engine is still the authority.
      if (!/^[\s\S]*\btracks:/m.test(text)) throw new Error('that file has no `tracks:` list, so it is not USTX');
      setError(null);
      setMediaFiles([]);
      setFile({ fileName: picked.name, name: importName(picked.name, text), text });
    } catch (readError) {
      setFile(null);
      setMediaFiles([]);
      setError(readError.message);
    }
  };
  const pickMedia = event => setMediaFiles(Array.from(event.target.files || []));
  return <Dialog title="Import USTX" onClose={onClose} width={520}>
    <p className="ou-dialog-message">Importing opens the file as a <strong>new project</strong>. Every note gets a new identity, so it can never collide with the project it came from.</p>
    <label className="ou-import-picker">USTX project
      <input type="file" accept={accept} data-testid="ustx-file" onChange={pick} />
    </label>
    <label className="ou-import-picker">Companion audio files (optional)
      <input type="file" multiple accept="audio/*,.mp3,.wav,.flac,.ogg,.m4a" data-testid="ustx-media-files" onChange={pickMedia} />
    </label>
    <p className="ou-dialog-message">Select referenced audio now to store and link it during import. Files are matched by filename. If unavailable or ambiguous, the project still imports and shows which parts need Relink Audio; voicebank ZIPs are separate.</p>
    {error && <p className="ou-import-selected error" role="alert">{error}</p>}
    {file && <p className="ou-import-selected" role="status">Selected: {file.fileName} → project “{file.name}”</p>}
    {mediaFiles.length > 0 && <p className="ou-import-selected" role="status">Companion audio: {mediaFiles.map(media => media.name).join(', ')}</p>}
    <div className="ou-dialog-actions">
      <button type="button" className="ou-button" onClick={onClose}>Cancel</button>
      <button type="button" className="ou-button ou-button-primary" disabled={!file || busy}
        data-testid="ustx-import" onClick={() => onImport(file.name, file.text, mediaFiles)}>Import</button>
    </div>
  </Dialog>;
}

function ProjectBundleImportDialog({ busy, onClose, onImport }) {
  const [bundle, setBundle] = useState(null);
  const [error, setError] = useState(null);
  const pick = async event => {
    const picked = event.target.files?.[0];
    if (!picked) { setBundle(null); return; }
    try {
      if (picked.size > MAX_PROJECT_BUNDLE_BYTES) throw new Error('project bundle is larger than 256 MiB');
      const parsed = await readProjectBundle(await picked.arrayBuffer());
      setBundle({ ...parsed, fileName: picked.name });
      setError(null);
    } catch (readError) {
      setBundle(null);
      setError(readError.message);
    }
  };
  return <Dialog title="Import project bundle" onClose={onClose} width={480}>
    <p className="ou-dialog-message">A rusted Utau project bundle contains the USTX project plus audio assets available when it was exported. This imports as a new project; unresolved source files remain available for Relink Audio.</p>
    <label className="ou-import-picker">Project bundle
      <input type="file" accept={PROJECT_BUNDLE_EXTENSION} data-testid="project-bundle-file" onChange={pick} />
    </label>
    {error && <p className="ou-import-selected error" role="alert">{error}</p>}
    {bundle && <p className="ou-import-selected" role="status">Selected: {bundle.fileName} → “{bundle.name}” · {bundle.media.length} audio asset(s){bundle.unresolvedMedia ? ` · ${bundle.unresolvedMedia} unresolved` : ''}</p>}
    <div className="ou-dialog-actions">
      <button type="button" className="ou-button" onClick={onClose}>Cancel</button>
      <button type="button" className="ou-button ou-button-primary" disabled={!bundle || busy}
        data-testid="project-bundle-import" onClick={() => onImport(bundle)}>Import project</button>
    </div>
  </Dialog>;
}

function ImportTracksDialog({ busy, onClose, onImport }) {
  const [file, setFile] = useState(null);
  const [error, setError] = useState(null);
  const pick = async event => {
    const picked = event.target.files?.[0];
    if (!picked) { setFile(null); return; }
    try {
      const text = await picked.text();
      if (!/^[\s\S]*\btracks:/m.test(text)) throw new Error('that file has no `tracks:` list, so it is not USTX');
      setFile({ name: picked.name, text });
      setError(null);
    } catch (readError) {
      setFile(null);
      setError(readError.message);
    }
  };
  return <Dialog title="Import Tracks" onClose={onClose} width={440}>
    <p className="ou-dialog-message">Append tracks and their parts to the current project. Rust parses the USTX and applies this as one undoable project change. Audio files referenced by imported wave parts must already be available to this host.</p>
    <label className="ou-import-picker">USTX project
      <input type="file" accept=".ustx,.yaml,.yml" onChange={pick} />
    </label>
    {error && <p className="ou-import-selected error" role="alert">{error}</p>}
    {file && <p className="ou-import-selected" role="status">Selected: {file.name}</p>}
    <div className="ou-dialog-actions">
      <button type="button" className="ou-button" onClick={onClose}>Cancel</button>
      <button type="button" className="ou-button ou-button-primary" disabled={!file || busy}
        onClick={() => onImport(file.text, file.name)}>Import tracks</button>
    </div>
  </Dialog>;
}

function AudioImportDialog({ tracks, selectedTrackId, playheadTicks, busy, onClose, onImport }) {
  const [file, setFile] = useState(null);
  const [trackId, setTrackId] = useState(selectedTrackId ?? tracks[0]?.id ?? '');
  const [positionTicks, setPositionTicks] = useState(Math.max(0, Math.round(playheadTicks ?? 0)));
  const [error, setError] = useState(null);
  const pick = async event => {
    const picked = event.target.files?.[0];
    if (!picked) { setFile(null); return; }
    try {
      const bytes = new Uint8Array(await picked.arrayBuffer());
      if (!bytes.length) throw new Error('audio file is empty');
      setFile({ name: picked.name.replace(/\.[^.]+$/, ''), bytes });
      setError(null);
    } catch (readError) {
      setFile(null);
      setError(readError.message);
    }
  };
  return <Dialog title="Import Audio" onClose={onClose} width={440}>
    <p className="ou-dialog-message">Audio is stored as a host media asset; Rust probes its duration, creates the wave part, and renders it.</p>
    <label className="ou-import-picker">Audio file
      <input type="file" accept="audio/*,.wav,.flac,.mp3,.ogg" onChange={pick} />
    </label>
    <label className="ou-dialog-label">Track
      <select className="ou-dialog-input" value={trackId} onChange={event => setTrackId(event.target.value)}>
        {tracks.map((track, index) => <option key={track.id} value={track.id}>{track.name || `Track ${index + 1}`}</option>)}
      </select>
    </label>
    <label className="ou-dialog-label">Position (ticks)
      <input className="ou-dialog-input" type="number" min="0" step="1" value={positionTicks}
        onChange={event => setPositionTicks(Number(event.target.value) || 0)} />
    </label>
    {error && <p className="ou-import-selected error" role="alert">{error}</p>}
    {file && <p className="ou-import-selected" role="status">Selected: {file.name}</p>}
    <div className="ou-dialog-actions">
      <button type="button" className="ou-button" onClick={onClose}>Cancel</button>
      <button type="button" className="ou-button ou-button-primary" disabled={!file || !trackId || busy}
        onClick={() => onImport({ ...file, trackId, positionTicks })}>Import</button>
    </div>
  </Dialog>;
}

function AudioRelinkDialog({ tracks, busy, onClose, onRelink }) {
  const waveParts = tracks.flatMap(track => (track.parts || [])
    .filter(part => part.kind === 'wave')
    .map(part => ({ ...part, trackName: track.name })));
  const [partId, setPartId] = useState(waveParts[0]?.id ?? '');
  const [file, setFile] = useState(null);
  const [error, setError] = useState(null);
  const pick = async event => {
    const picked = event.target.files?.[0];
    if (!picked) { setFile(null); return; }
    try {
      const bytes = new Uint8Array(await picked.arrayBuffer());
      if (!bytes.length) throw new Error('audio file is empty');
      setFile({ name: picked.name, bytes });
      setError(null);
    } catch (readError) {
      setFile(null);
      setError(readError.message);
    }
  };
  const relink = async () => {
    try {
      const mediaId = `relink-${globalThis.crypto?.randomUUID?.() || `${Date.now()}-${Math.random().toString(36).slice(2)}`}`;
      await onRelink({ partId, mediaId, bytes: file.bytes });
      onClose();
    } catch (relinkError) {
      setError(relinkError.message);
    }
  };
  return <Dialog title="Relink Audio" onClose={onClose} width={460}>
    <p className="ou-dialog-message">Choose the wave part from the imported USTX and its source audio file. The file is stored in this host and the Rust project session updates the part reference.</p>
    <label className="ou-dialog-label">Wave part
      <select className="ou-dialog-input" value={partId} onChange={event => setPartId(event.target.value)}>
        {waveParts.map(part => <option key={part.id} value={part.id}>{part.trackName} · {part.name || part.media_ref}</option>)}
      </select>
    </label>
    <label className="ou-import-picker">Audio file
      <input type="file" accept="audio/*,.wav,.flac,.mp3,.ogg" onChange={pick} />
    </label>
    {file && <p className="ou-import-selected" role="status">Selected: {file.name}</p>}
    {error && <p className="ou-import-selected error" role="alert">{error}</p>}
    <div className="ou-dialog-actions">
      <button type="button" className="ou-button" onClick={onClose}>Cancel</button>
      <button type="button" className="ou-button ou-button-primary" disabled={!partId || !file || busy} onClick={relink}>Relink audio</button>
    </div>
  </Dialog>;
}

/**
 * An import OpenUtau has that the Rust session cannot finish yet.
 *
 * These are the File menu's own entries (`menu.file.importtracks`,
 * `menu.file.importaudio`), not inventions — but the project state they would
 * change has no v2 command, so they say exactly what is missing instead of
 * reading bytes and half-applying something. Both are recorded in T014.
 */
function UnavailableImportDialog({ format, accept, why, needs, onClose }) {
  const [fileName, setFileName] = useState('');
  return <Dialog title={`Import ${format}`} onClose={onClose} width={420}>
    <p className="ou-dialog-message">Importing {format.toLowerCase()} is not available yet.</p>
    <p className="ou-dialog-message">{why}</p>
    <label className="ou-import-picker">File
      <input type="file" accept={accept} onChange={event => setFileName(event.target.files?.[0]?.name || '')} />
    </label>
    {fileName && <p className="ou-import-selected" role="status">Selected: {fileName} (not read)</p>}
    <div className="ou-dialog-actions">
      <button type="button" className="ou-button" onClick={onClose}>Close</button>
      <button type="button" className="ou-button ou-button-primary" disabled title={`Waiting on ${needs}`}>Import (unavailable)</button>
    </div>
  </Dialog>;
}

export function FileMenu() {
  // `renderTrack` lives on `project`: it is a document-level engine call, not
  // transient UI state, so the context groups it with the other project actions.
  const { project, activeTrack, tracks, playheadTicks } = useClone();
  // The engine addresses tracks by NUMBER (§6.13 `track`), and the snapshot
  // lists them in the engine's own order — so the index in `tracks` is the
  // number, and the active track's id says which one the user is looking at.
  const trackNumber = Math.max(0, (activeTrack ? tracks.indexOf(activeTrack) : 0));
  const [open, setOpen] = useState(false);
  const [dialog, setDialog] = useState(null);
  const [names, setNames] = useState([]);
  const [toast, setToast] = useState(null);
  const [exportStatus, setExportStatus] = useState(null);
  const [exportClock, setExportClock] = useState(0);
  const [importFormat, setImportFormat] = useState(null);
  const rootRef = useRef(null);
  const toastTimer = useRef(null);
  const exportSequenceRef = useRef(0);
  const exportActiveRef = useRef(false);

  const say = useCallback((text, kind = 'info') => {
    setToast({ text, kind, id: Date.now() });
    clearTimeout(toastTimer.current);
    toastTimer.current = setTimeout(() => setToast(null), 5000);
  }, []);

  useEffect(() => () => clearTimeout(toastTimer.current), []);

  useEffect(() => {
    if (!exportStatus || exportStatus.finishedAt != null) return undefined;
    const timer = setInterval(() => setExportClock(performance.now()), 1000);
    return () => clearInterval(timer);
  }, [exportStatus?.id, exportStatus?.finishedAt]);

  // A menu is a popup: one click elsewhere or Escape closes it.
  useEffect(() => {
    if (!open) return undefined;
    const onPointer = event => { if (!rootRef.current?.contains(event.target)) setOpen(false); };
    const onKey = event => { if (event.key === 'Escape') setOpen(false); };
    window.addEventListener('pointerdown', onPointer);
    window.addEventListener('keydown', onKey);
    return () => { window.removeEventListener('pointerdown', onPointer); window.removeEventListener('keydown', onKey); };
  }, [open]);

  // `success` receives whatever the action resolved with, so a message can
  // report the result it just produced instead of re-reading state that the
  // action has only just invalidated.
  const run = useCallback(async (action, success, failure) => {
    setOpen(false);
    try { say(success(await action())); }
    catch (error) { say(failure(error), 'error'); }
  }, [say]);

  const runExport = useCallback(async (label, action, success, failure) => {
    setOpen(false);
    if (exportActiveRef.current) {
      say('An export is already in progress', 'error');
      return;
    }
    exportActiveRef.current = true;
    clearTimeout(toastTimer.current);
    setToast(null);
    const id = ++exportSequenceRef.current;
    const startedAt = performance.now();
    setExportClock(startedAt);
    setExportStatus({ id, label, phase: 'engine', startedAt });
    try {
      await runExportTask({
        label,
        action,
        startedAt,
        onStatus: status => setExportStatus(current => (
          current?.id === id ? { ...current, ...status, id } : current
        )),
        successMessage: (result, elapsedMs) => `${success(result)} · ${formatExportElapsed(elapsedMs)} total`,
        failureMessage: (error, elapsedMs) => `${failure(error)} · ${formatExportElapsed(elapsedMs)} elapsed`,
      });
    } catch {
      // runExportTask has already published a persistent error status.
    } finally {
      exportActiveRef.current = false;
    }
  }, [say]);

  const newProject = name => run(
    () => project.newProject(name),
    () => `New project “${name}”`,
    error => error.message,
  );
  const openProject = name => run(
    () => project.openProject(name),
    () => `Opened “${name}”`,
    error => error.message,
  );
  const save = () => run(
    () => project.saveProject(),
    () => `Saved “${project.name}”`,
    error => error.message,
  );
  const saveAs = name => run(
    () => project.saveProjectAs(name),
    () => `Saved as “${name}”`,
    error => error.message,
  );
  const exportUstx = () => run(
    async () => {
      const ustx = await project.exportUstx();
      download(project.name, ustx);
    },
    () => `Exported “${project.name}.ustx”`,
    error => error.message,
  );
  const exportProjectBundle = () => run(
    async () => {
      const bundle = await project.exportProjectBundle();
      downloadProjectBundle(project.name, bundle.bytes);
      return bundle;
    },
    result => `Exported “${project.name}${PROJECT_BUNDLE_EXTENSION}” — ${result.mediaCount} audio asset(s)${result.unresolvedMedia ? `; ${result.unresolvedMedia} part(s) still need source files` : ''}`,
    error => error.message,
  );
  const importProjectBundle = bundle => run(
    () => project.importProjectBundle(bundle.name, bundle),
    result => {
      setImportFormat(null);
      const linked = result?.linkedMedia || 0;
      const unresolved = result?.unresolvedMedia || 0;
      const audioSummary = unresolved
        ? ` — linked ${linked}; ${unresolved} audio part(s) still need source files`
        : linked ? ` — linked ${linked} audio part(s)` : '';
      const mediaError = result?.mediaError ? ` (${result.mediaError})` : '';
      return `Imported “${bundle.name}”${audioSummary}${mediaError} — use Save As to keep it`;
    },
    error => error.message,
  );
  /** An imported document is not in the host store, so it saves via Save As. */
  const importUstx = (name, text, mediaFiles = []) => run(
    () => project.importUstx(name, text, mediaFiles),
    result => {
      setImportFormat(null);
      const linked = result?.linkedMedia || 0;
      const unresolved = result?.unresolvedMedia || 0;
      const audioSummary = unresolved
        ? ` — linked ${linked}; ${unresolved} audio part(s) still need source files`
        : linked ? ` — linked ${linked} audio part(s)` : '';
      const mediaError = result?.mediaError ? ` (${result.mediaError})` : '';
      return `Imported “${name}”${audioSummary}${mediaError} — use Save As to keep it`;
    },
    error => error.message,
  );
  const importAudio = ({ name, bytes, trackId, positionTicks }) => {
    setImportFormat(null);
    const mediaId = globalThis.crypto?.randomUUID?.() || `${Date.now()}-${Math.random().toString(16).slice(2)}`;
    return run(
      () => project.importAudio({ mediaId, bytes, trackId, name, positionTicks }),
      () => `Imported audio “${name}”`,
      error => error.message,
    );
  };
  const importTracks = (yaml, fileName) => {
    setImportFormat(null);
    return run(
      () => project.importTracks(yaml),
      () => `Imported tracks from “${fileName}”`,
      error => error.message,
    );
  };

  /**
   * Render the track the piano roll is showing, through the engine (§6.13).
   *
   * The track NUMBER comes from the engine-ordered track list, and the file
   * menu deliberately does not pick a voicebank: the native route wants a bank
   * NAME, and guessing one would make the result depend on host state the user
   * never chose. Without one the backend says so and the toast shows the
   * engine's own reason.
   */
  // The sample count comes from the render's OWN result, not from the state
  // update it triggers: reading `renderJob` inside the success message would
  // read the value from before this render and could report the previous run's
  // length (or none at all).
  const render = () => runExport(
    `Track ${trackNumber + 1}`,
    async reportPhase => {
      const result = await project.renderTrack(trackNumber);
      return downloadRenderedWav(`${project.name}-Track-${trackNumber + 1}`, result, project.encodeTakeWav, reportPhase);
    },
    result => `WAV ready · browser download started · ${(result.frames ?? result.channels?.[0]?.length ?? result.samples?.length ?? 0).toLocaleString()} frames at ${result.sampleRate.toLocaleString()} Hz`,
    error => error.message,
  );
  const mixdown = () => runExport(
    'Mixdown',
    async reportPhase => {
      const result = await project.renderMixdown();
      return downloadRenderedWav(`${project.name}-Mixdown`, result, project.encodeTakeWav, reportPhase);
    },
    result => `WAV ready · browser download started · ${(result.frames ?? result.channels?.[0]?.length ?? result.samples?.length ?? 0).toLocaleString()} frames at ${result.sampleRate.toLocaleString()} Hz`,
    error => error.message,
  );

  /** Unsaved edits are only ever discarded after an explicit confirmation. */
  const guarded = action => { setOpen(false); setImportFormat(null); if (project.dirty) setDialog({ kind: 'confirm', action }); else action(); };
  /** Save As is itself a save, so it never stands behind the unsaved guard. */
  const openDialog = kind => { setOpen(false); setDialog({ kind }); };

  const startOpen = async () => {
    setOpen(false);
    try { setNames((await project.listProjects()).map(entry => entry.name ?? String(entry))); }
    catch (error) { say(error.message, 'error'); }
    setDialog({ kind: 'open' });
  };

  // No dependency list on purpose: the handlers read the current project name
  // and dirty flag, and re-registering one listener per render is cheaper than
  // the stale-closure bugs a `useCallback` web would hide here.
  useEffect(() => {
    const onKey = event => {
      if (!(event.ctrlKey || event.metaKey)) return;
      const key = event.key.toLowerCase();
      if (key === 's') {
        event.preventDefault();
        if (event.shiftKey) openDialog('save-as');
        else save();
      }
      else if (key === 'n') { event.preventDefault(); guarded(() => openDialog('new')); }
      else if (key === 'o') { event.preventDefault(); guarded(startOpen); }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });

  const item = (label, hint, onClick, disabled = false) => <button type="button" role="menuitem" className="ou-dropdown-item"
    onClick={onClick} disabled={disabled || project.busy}>
    <span>{label}</span>{hint && <span className="ou-dropdown-hint">{hint}</span>}
  </button>;

  return <span className="ou-menu-root" ref={rootRef}>
    <button type="button" className="ou-menu" aria-haspopup="menu" aria-expanded={open} onClick={() => setOpen(value => !value)}>File</button>
    {open && <div className="ou-dropdown" role="menu" aria-label="File">
      {item('New project…', 'Ctrl+N', () => guarded(() => openDialog('new')))}
      {item('Open…', 'Ctrl+O', startOpen)}
      <span className="ou-dropdown-sep" />
      {item('Import USTX…', '', () => { setOpen(false); setImportFormat('USTX'); })}
      {item('Import Project Bundle…', '.rustx', () => { setOpen(false); setImportFormat('Bundle'); })}
      {item('Import Tracks…', '', () => { setOpen(false); setImportFormat('Tracks'); })}
      {item('Import Audio…', '', () => { setOpen(false); setImportFormat('Audio'); })}
      {tracks.some(track => (track.parts || []).some(part => part.kind === 'wave'))
        && item('Relink Audio…', '', () => { setOpen(false); setImportFormat('RelinkAudio'); })}
      <span className="ou-dropdown-sep" />
      {item('Save', 'Ctrl+S', save)}
      {item('Save As…', 'Ctrl+Shift+S', () => openDialog('save-as'))}
      <span className="ou-dropdown-sep" />
      {item(`Export track ${trackNumber + 1} as WAV…`, 'engine · WAV', render)}
      {item('Export mixdown…', 'all tracks · stereo WAV', mixdown)}
      {item('Export USTX…', 'text only', exportUstx)}
      {item('Export Project Bundle…', '.rustx · USTX + audio', exportProjectBundle)}
    </div>}
    {toast && <span key={toast.id} className={`ou-toast ${toast.kind}`} role="status" onClick={() => setToast(null)}>{toast.text}</span>}
    {exportStatus && <span
      key={exportStatus.id}
      className={`ou-toast ${exportStatus.kind || 'progress'}`}
      role="status"
      aria-live={exportStatus.finishedAt == null ? 'off' : 'polite'}
      aria-busy={exportStatus.finishedAt == null}
    >
      {exportStatus.finishedAt == null
        ? formatExportProgress({ label: `${exportStatus.label} export`, phase: exportStatus.phase, elapsedMs: exportClock - exportStatus.startedAt })
        : exportStatus.message}
    </span>}
    {dialog?.kind === 'new' && <NameDialog title="New project" label="Project name" initial="Untitled" confirmLabel="Create"
      onConfirm={name => { setDialog(null); newProject(name); }} onClose={() => setDialog(null)} />}
    {dialog?.kind === 'save-as' && <NameDialog title="Save project as" label="Store this project as" initial={project.name} confirmLabel="Save"
      onConfirm={name => { setDialog(null); saveAs(name); }} onClose={() => setDialog(null)} />}
    {dialog?.kind === 'open' && <OpenDialog names={names} current={project.name}
      onPick={name => { setDialog(null); openProject(name); }} onClose={() => setDialog(null)} />}
    {dialog?.kind === 'confirm' && <ConfirmDialog title="Unsaved changes" confirmLabel="Discard changes"
      message={`“${project.name}” has changes that are not saved. Replacing it discards them.`}
      onConfirm={() => { const action = dialog.action; setDialog(null); action(); }}
      onClose={() => setDialog(null)} />}
    {importFormat === 'USTX' && <ImportDialog busy={project.busy}
      onImport={(name, text, mediaFiles) => guarded(() => importUstx(name, text, mediaFiles))}
      onClose={() => setImportFormat(null)} />}
    {importFormat === 'Bundle' && <ProjectBundleImportDialog busy={project.busy}
      onImport={bundle => guarded(() => importProjectBundle(bundle))}
      onClose={() => setImportFormat(null)} />}
    {importFormat === 'Tracks' && <ImportTracksDialog busy={project.busy} onImport={importTracks}
      onClose={() => setImportFormat(null)} />}
    {importFormat === 'Audio' && <AudioImportDialog tracks={tracks} selectedTrackId={activeTrack?.id}
      playheadTicks={playheadTicks} busy={project.busy} onImport={importAudio}
      onClose={() => setImportFormat(null)} />}
    {importFormat === 'RelinkAudio' && <AudioRelinkDialog tracks={tracks} busy={project.busy}
      onRelink={project.relinkAudio} onClose={() => setImportFormat(null)} />}
  </span>;
}
