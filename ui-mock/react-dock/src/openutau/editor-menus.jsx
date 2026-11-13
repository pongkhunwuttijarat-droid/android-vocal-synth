import React, { useCallback, useEffect, useRef, useState } from 'react';
import { OpfsBank } from '../../../../web/samples.js';
import { characterNameFrom, commonTopDirectory, parseZip, readZipEntries, zipBankRoot } from '../../../../web/zip.js';
import { useClone } from './clone-state.jsx';
import { resetPreviewBankScan } from './waveform-source.js';

function MenuPopup({ label, children }) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef(null);
  useEffect(() => {
    if (!open) return undefined;
    const outside = event => { if (!rootRef.current?.contains(event.target)) setOpen(false); };
    const escape = event => { if (event.key === 'Escape') setOpen(false); };
    window.addEventListener('pointerdown', outside);
    window.addEventListener('keydown', escape);
    return () => {
      window.removeEventListener('pointerdown', outside);
      window.removeEventListener('keydown', escape);
    };
  }, [open]);
  return <span className="ou-menu-root" ref={rootRef}>
    <button type="button" className="ou-menu" aria-haspopup="menu" aria-expanded={open}
      onClick={() => setOpen(value => !value)}>{label}</button>
    {open && <div className="ou-dropdown" role="menu" aria-label={label}>
      {children(() => setOpen(false))}
    </div>}
  </span>;
}

function Item({ children, hint, onClick, disabled = false }) {
  return <button type="button" role="menuitem" className="ou-dropdown-item" disabled={disabled} onClick={onClick}>
    <span>{children}</span>{hint && <span className="ou-dropdown-hint">{hint}</span>}
  </button>;
}

function Dialog({ title, onClose, children, onApply, applyLabel = 'Apply (coming later)', applyDisabled = true, applyHelp }) {
  useEffect(() => {
    const onKey = event => { if (event.key === 'Escape') onClose(); };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);
  return <div className="ou-dialog-backdrop" onPointerDown={event => { if (event.target === event.currentTarget) onClose(); }}>
    <section className="ou-dialog ou-project-dialog" role="dialog" aria-modal="true" aria-label={title}>
      <h2 className="ou-dialog-title">{title}</h2>
      {children}
      <div className="ou-dialog-actions">
        <button type="button" className="ou-button" onClick={onClose}>Close</button>
        <button type="button" className="ou-button ou-button-primary" disabled={applyDisabled} onClick={onApply}
          title={applyDisabled ? (applyHelp || 'Choose an installed voicebank and an active track') : undefined}>{applyLabel}</button>
      </div>
    </section>
  </div>;
}

export function EditMenu() {
  const { undo, redo, connected } = useClone();
  return <MenuPopup label="Edit">{close => <>
    <Item hint="Ctrl+Z" disabled={!connected} onClick={() => { close(); void undo(); }}>Undo</Item>
    <Item hint="Ctrl+Shift+Z" disabled={!connected} onClick={() => { close(); void redo(); }}>Redo</Item>
  </>}</MenuPopup>;
}

export function ProjectMenu() {
  const { bpm } = useClone();
  const [dialog, setDialog] = useState(null);
  return <>
    <MenuPopup label="Project">{close => <>
      <Item onClick={() => { close(); setDialog('expressions'); }}>Expressions…</Item>
      <Item onClick={() => { close(); setDialog('tempo'); }}>Adjust Tempo (Preserve Timing)…</Item>
    </>}</MenuPopup>
    {dialog === 'expressions' && <Dialog title="Project Expressions" onClose={() => setDialog(null)}>
      <p className="ou-dialog-message">Expression definitions apply to the project. This UI is a draft; the Rust session exposes no descriptor read or write yet, so these rows are the engine's own §6.6.1 curve defaults shown for reference — they are not read from or saved to the session.</p>
      <div className="ou-expression-table" role="table" aria-label="§6.6.1 curve descriptors">
        <div className="head" role="row"><span>Abbr.</span><span>Name</span><span>Default</span><span>Range</span></div>
        <div role="row"><span>dyn</span><span>Dynamics</span><span>0</span><span>−240–120</span></div>
        <div role="row"><span>pitd</span><span>Pitch deviation</span><span>0</span><span>−1200–1200</span></div>
        <div role="row"><span>genc</span><span>Gender (curve)</span><span>0</span><span>−100–100</span></div>
        <div role="row"><span>brec</span><span>Breathiness (curve)</span><span>0</span><span>−100–100</span></div>
        <div role="row"><span>shfc</span><span>Tone shift (curve)</span><span>0</span><span>−1200–1200</span></div>
        <div role="row"><span>tenc</span><span>Tension (curve)</span><span>0</span><span>−100–100</span></div>
        <div role="row"><span>voic</span><span>Voicing</span><span>100</span><span>0–100</span></div>
      </div>
      <button type="button" className="ou-button" disabled title="Descriptor editing is not in the frozen v2 command scope">Add expression</button>
    </Dialog>}
    {dialog === 'tempo' && <Dialog title="Adjust Tempo (Preserve Timing)" onClose={() => setDialog(null)}>
      <p className="ou-dialog-message">Choose a new base tempo. Applying this operation must remap musical positions in Rust so the notes keep their current real-time timing.</p>
      <label className="ou-dialog-label ou-tempo-field">New BPM
        <input className="ou-dialog-input" type="number" min="1" max="1000" step="0.1" defaultValue={Number(bpm).toFixed(1)} />
      </label>
      <p className="ou-project-draft-note">Preview only · changes are not applied or saved.</p>
    </Dialog>}
  </>;
}

/**
 * Tools → Singers… / Install Singer…
 *
 * A voicebank is **host storage**, not project state: installing one writes
 * files the browser can read, and then the render preview re-resolves which
 * samples it uses. Which track uses which singer is Rust-owned. Applying an
 * installed bank sends `track.set_singer` for the active track; installation
 * alone does not mutate the project.
 */
function SingersDialog({ autoInstall = false, onClose }) {
  const { waveform, dispatch, activeTrack, connected } = useClone();
  const [roots, setRoots] = useState(null);
  const [selectedRoot, setSelectedRoot] = useState('');
  const [status, setStatus] = useState(null);
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const pickRef = useRef(null);
  const zipRef = useRef(null);

  // A bank is a folder (an `oto.ini` plus wavs), not a single file. `voice/` is
  // the convention but not the rule — see `findOtoDir`.
  // `webkitdirectory` is not in React's attribute allow-list, so set it on the
  // node itself rather than waiting for a browser to honour a synthetic prop.
  useEffect(() => {
    const input = pickRef.current;
    if (!input) return;
    input.setAttribute('webkitdirectory', '');
    input.setAttribute('directory', '');
    if (autoInstall) input.click();
  }, [autoInstall]);

  const refresh = useCallback(async () => {
    try {
      if (!(await OpfsBank.available())) {
        setRoots([]);
        setError('This browser has no OPFS, so a voicebank cannot be stored here.');
        return;
      }
      setRoots(await OpfsBank.installedRoots());
    } catch (scanError) { setError(scanError.message); }
  }, []);
  useEffect(() => { void refresh(); }, [refresh]);
  useEffect(() => {
    if (!roots) return;
    setSelectedRoot(current => roots.includes(activeTrack?.singer_id)
      ? activeTrack.singer_id
      : roots.includes(current) ? current : roots[0] || '');
  }, [roots, activeTrack?.id, activeTrack?.singer_id]);

  /** Tell the preview to re-resolve, then let it depend on the new set. */
  const announceChange = () => { resetPreviewBankScan(); dispatch({ type: 'ui/banks-changed' }); };

  const install = async event => {
    const files = [...(event.target.files || [])];
    event.target.value = '';
    if (!files.length) return;
    setBusy(true); setError(null); setStatus(null);
    // The folder the user picked names the bank, which is how the preview's
    // singer match finds it later; stripping the top dir moves its contents up.
    const root = (files[0].webkitRelativePath || '').split('/')[0] || 'rusted-utau-voicebank';
    try {
      const written = await new OpfsBank({ root }).importFiles(files);
      if (!written) throw new Error('no files were selected');
      const bank = new OpfsBank({ root });
      const info = await bank.openAllOtos();
      if (!info.aliasCount) {
        await OpfsBank.removeRoot(root);
        throw new Error('the selected folder has no usable oto aliases, so the import was removed');
      }
      announceChange();
      await refresh();
      setSelectedRoot(root);
      setStatus(`Installed “${root}” — ${written} files, ${info.otoSets} oto sets, ${info.aliasCount} aliases, ${info.sampleCount} samples`);
    } catch (installError) {
      setError(installError.message);
    } finally { setBusy(false); }
  };

  /**
   * Install from a `.zip`, the other way a voicebank is distributed.
   *
   * Archives come both ways round — some wrap the bank in one folder, some put
   * `character.txt` and `voice/` at the top level — so peel a shared wrapper
   * first and name the root the way OpenUtau lists a singer (`character.txt`'s
   * `name=`) so a project that references that singer finds it.
   */
  const installZip = async event => {
    const picked = event.target.files?.[0];
    event.target.value = '';
    if (!picked) return;
    setBusy(true); setError(null); setStatus(null);
    try {
      const buffer = await picked.arrayBuffer();
      const { entries } = parseZip(buffer);
      if (!entries.length) throw new Error('that zip archive holds no files');
      const prefix = commonTopDirectory(entries);
      const files = await readZipEntries(buffer, { prefix });
      const character = files.find(file => file.path === 'character.txt');
      const root = zipBankRoot(picked.name, entries, character ? characterNameFrom(character.bytes) : null);
      const written = await new OpfsBank({ root }).importFiles(files, { stripTopDir: false });
      if (!written) throw new Error('that zip archive held no installable files');
      const bank = new OpfsBank({ root });
      const info = await bank.openAllOtos();
      if (!info.aliasCount) {
        await OpfsBank.removeRoot(root);
        throw new Error('that zip archive has no usable oto aliases, so the import was removed');
      }
      announceChange();
      await refresh();
      setSelectedRoot(root);
      setStatus(`Installed “${root}” from ${picked.name} — ${written} files, ${info.otoSets} oto sets, ${info.aliasCount} aliases, ${info.sampleCount} samples`);
    } catch (installError) {
      setError(installError.message);
    } finally { setBusy(false); }
  };

  const remove = async root => {
    setBusy(true); setError(null); setStatus(null);
    try {
      await OpfsBank.removeRoot(root);
      announceChange();
      await refresh();
      setSelectedRoot(current => current === root ? '' : current);
      setStatus(`Removed “${root}”`);
    } catch (removeError) { setError(removeError.message); }
    finally { setBusy(false); }
  };

  const inUse = waveform?.bank?.name ?? null;
  const alreadyApplied = Boolean(activeTrack && selectedRoot && selectedRoot === activeTrack.singer_id);
  const applySinger = async () => {
    if (!activeTrack || !selectedRoot || !connected) return;
    setBusy(true); setError(null); setStatus(null);
    try {
      const result = await dispatch({ type: 'track/singer', trackId: activeTrack.id, singerId: selectedRoot });
      if (result?.error) throw new Error(result.error.message || String(result.error));
      if (result?.unsupported?.includes('track/singer')) throw new Error('The active session does not support track.set_singer.');
      setStatus(`Applied “${selectedRoot}” to ${activeTrack.name}.`);
    } catch (applyError) { setError(applyError.message); }
    finally { setBusy(false); }
  };
  return <Dialog title="Singers" onClose={onClose} onApply={applySinger}
    applyLabel={`Apply to ${activeTrack?.name || 'track'}`}
    applyDisabled={!connected || !activeTrack || !selectedRoot || alreadyApplied || busy}
    applyHelp={alreadyApplied ? `“${selectedRoot}” is already applied to ${activeTrack.name}` : undefined}>
    <p className="ou-dialog-message">Voicebanks are stored by this browser (OPFS), not in the project. Choose a bank below and Apply it to the active track to update the Rust project.</p>
    <label className="ou-dialog-label">Apply to {activeTrack?.name || 'no active track'}
      <select className="ou-dialog-input" aria-label="Voicebank for active track" value={selectedRoot}
        onChange={event => setSelectedRoot(event.target.value)} disabled={!roots?.length || busy}>
        {(roots || []).map(root => <option key={root} value={root}>{root}</option>)}
      </select>
    </label>
    <div className="ou-singer-list" role="list">
      {roots === null && <p className="ou-dialog-empty">Reading installed voicebanks…</p>}
      {roots?.length === 0 && <p className="ou-dialog-empty">No voicebank is installed; the render preview falls back to the bundled dev bank.</p>}
      {(roots || []).map(root => <div className="ou-singer-row" role="listitem" key={root}>
        <span className="ou-singer-name">{root}</span>
        {root === inUse && <span className="ou-singer-tag">preview</span>}
        <button type="button" className="ou-button" disabled={busy} onClick={() => void remove(root)}>Remove</button>
      </div>)}
    </div>
    {status && <p className="ou-import-selected" role="status">{status}</p>}
    {!status && alreadyApplied && <p className="ou-import-selected" role="status">“{selectedRoot}” is already applied to {activeTrack.name}.</p>}
    {error && <p className="ou-import-selected error" role="alert">{error}</p>}
    <input ref={pickRef} type="file" multiple className="ou-singer-pick" aria-label="Voicebank folder" onChange={install} />
    <input ref={zipRef} type="file" accept=".zip,application/zip" className="ou-singer-pick" aria-label="Voicebank zip archive" data-testid="singer-zip" onChange={installZip} />
    <div className="ou-dialog-actions">
      <button type="button" className="ou-button" disabled={busy} onClick={() => zipRef.current?.click()}>Install from .zip…</button>
      <button type="button" className="ou-button ou-button-primary" disabled={busy} onClick={() => pickRef.current?.click()}>Install Singer…</button>
    </div>
  </Dialog>;
}

export function ToolsMenu({ onLayout }) {
  const [fullscreen, setFullscreen] = useState(Boolean(document.fullscreenElement));
  const [dialog, setDialog] = useState(null);
  useEffect(() => {
    const update = () => setFullscreen(Boolean(document.fullscreenElement));
    document.addEventListener('fullscreenchange', update);
    return () => document.removeEventListener('fullscreenchange', update);
  }, []);
  const toggleFullscreen = useCallback(async () => {
    try {
      if (document.fullscreenElement) await document.exitFullscreen();
      else await document.documentElement.requestFullscreen();
    } catch { /* Browser policy may reject fullscreen; keep the menu usable. */ }
  }, []);
  useEffect(() => {
    const onKeyDown = event => {
      if (event.key !== 'F11') return;
      event.preventDefault();
      void toggleFullscreen();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [toggleFullscreen]);
  return <>
    <MenuPopup label="Tools">{close => <>
    <div className="ou-dropdown-heading">Layout</div>
    <Item onClick={() => { close(); onLayout('reset'); }}>Reset layout</Item>
    <div className="ou-dropdown-heading">Arrangement : Editor</div>
    {[['1:1', 1], ['1:2', 2], ['1:3', 3]].map(([label, ratio]) =>
      <Item key={`h-${label}`} onClick={() => { close(); onLayout('horizontal', ratio); }}>{label}</Item>)}
    <div className="ou-dropdown-heading">Piano Roll : Properties</div>
    {[['1:1', 1], ['2:1', 2], ['3:1', 3]].map(([label, ratio]) =>
      <Item key={`v-${label}`} onClick={() => { close(); onLayout('vertical', ratio); }}>{label}</Item>)}
    <span className="ou-dropdown-sep" />
    <div className="ou-dropdown-heading">Singers</div>
    <Item onClick={() => { close(); setDialog('singers'); }}>Singers…</Item>
    <Item onClick={() => { close(); setDialog('install'); }}>Install Singer…</Item>
    <span className="ou-dropdown-sep" />
    <Item hint="F11" onClick={() => { close(); void toggleFullscreen(); }}>{fullscreen ? 'Exit Fullscreen' : 'Fullscreen'}</Item>
    </>}</MenuPopup>
    {dialog && <SingersDialog autoInstall={dialog === 'install'} onClose={() => setDialog(null)} />}
  </>;
}
