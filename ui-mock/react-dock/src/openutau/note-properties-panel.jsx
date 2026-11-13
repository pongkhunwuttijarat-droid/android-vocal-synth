// Note Properties panel: OpenUtau-style accordion — Basic, Portamento,
// Vibrato, Expressions. Reads and writes the selected notes through the shared
// clone state, so editing a lyric here repaints the Piano Roll immediately.
// Against a real session these fields become `note.set_lyric` / `note.set_tone`
// command groups rather than reducer actions.
import React, { useState } from 'react';
import { VIBRATO_DEFAULTS, toneLabel, useClone } from './clone-state.jsx';

const SECTIONS = ['Basic', 'Portamento', 'Vibrato', 'Expressions'];

function Row({ label, children }) {
  return <div className="ou-prop-row"><label>{label}</label>{children}</div>;
}

function NumberSlider({ value, min = 0, max = 100, step = 1, onChange, disabled }) {
  return <span className="ou-prop-slider">
    <input type="number" value={value} min={min} max={max} step={step} disabled={disabled} onChange={event => onChange(Number(event.target.value))} />
    <input type="range" value={value} min={min} max={max} step={step} disabled={disabled} onChange={event => onChange(Number(event.target.value))} />
  </span>;
}


export function NotePropertiesPanel() {
  const { selectedNotes, dispatch, connected } = useClone();
  const [open, setOpen] = useState(() => new Set(['Basic']));
  const [portamento, setPortamento] = useState({ preset: '', length: 80, start: -40, curve: '' });
  const [expressions, setExpressions] = useState({ voiceColor: '', velocity: 100, volume: 100, modulation: 0, alt: false, direct: false, toneShift: 0 });
  const toggle = section => setOpen(current => {
    const next = new Set(current);
    if (next.has(section)) next.delete(section); else next.add(section);
    return next;
  });
  const disabled = selectedNotes.length === 0;
  const note = selectedNotes[0] || null;
  // The vibrato rows edit the selected note's patch; with no note they show
  // the shared defaults. Both this panel and the roll's Vibrato tool write the
  // same `note/vibrato` action, so the two stay in sync.
  const noteVibrato = note?.vibrato ?? VIBRATO_DEFAULTS;
  const setVibrato = patch => dispatch({ type: 'note/vibrato', ids: selectedNotes.map(value => value.id), patch: { ...noteVibrato, ...patch } });
  const vibratoDisabled = disabled;
  // Editing one lyric applies to the whole selection, like OpenUtau.
  const setLyric = lyric => dispatch({ type: 'note/lyric', ids: selectedNotes.map(value => value.id), lyric });
  const setTone = tone => dispatch({ type: 'note/tone', updates: selectedNotes.map(value => ({ id: value.id, tone })) });
  return <div className="ou-properties">
    <div className="ou-prop-title">Note Properties ({selectedNotes.length} note{selectedNotes.length === 1 ? '' : 's'})</div>
    {SECTIONS.map(section => (
      <section className="ou-prop-section" key={section}>
        <button type="button" className="ou-prop-header" aria-expanded={open.has(section)} onClick={() => toggle(section)}>
          <span>{section}</span><span className="ou-caret">{open.has(section) ? '⌃' : '⌄'}</span>
        </button>
        {open.has(section) && <div className={`ou-prop-body ${disabled ? 'disabled' : ''}`}>
          {connected && section !== 'Basic' && <p className="ou-prop-hint">This section is not connected to Project Session yet.</p>}
          {section === 'Basic' && <>
            <Row label="Lyric"><input value={note?.lyric ?? ''} placeholder={disabled ? 'No selection' : ''} disabled={disabled} onChange={event => setLyric(event.target.value)} /></Row>
            <Row label="Phonemizer"><input value="Defaults (JA VCV & CVVC)" disabled /></Row>
            <Row label="Tone">
              <span className="ou-prop-slider">
                <input type="number" value={note?.tone ?? 0} min={0} max={127} disabled={disabled} onChange={event => setTone(Number(event.target.value))} />
                <input type="range" value={note?.tone ?? 0} min={0} max={127} disabled={disabled} onChange={event => setTone(Number(event.target.value))} />
                <span className="ou-prop-tone">{note ? toneLabel(note.tone) : '—'}</span>
              </span>
            </Row>
            <Row label="Tuning"><NumberSlider value={0} min={-100} max={100} disabled onChange={() => {}} /></Row>
          </>}
          {section === 'Portamento' && <>
            <Row label="Preset">
              <span className="ou-prop-buttons">
                <select value={portamento.preset} disabled={disabled || connected} onChange={event => setPortamento({ ...portamento, preset: event.target.value })}>
                  <option value="">—</option>
                  <option value="smooth">Smooth</option>
                  <option value="sharp">Sharp</option>
                </select>
                <button type="button" disabled={disabled || connected}>Save</button>
                <button type="button" disabled={disabled || connected}>Remove</button>
              </span>
            </Row>
            <Row label="Length"><NumberSlider value={portamento.length} disabled={disabled || connected} onChange={value => setPortamento({ ...portamento, length: value })} /></Row>
            <Row label="Start"><NumberSlider value={portamento.start} min={-100} max={100} disabled={disabled || connected} onChange={value => setPortamento({ ...portamento, start: value })} /></Row>
            <Row label="Curve shape">
              <select value={portamento.curve} disabled={disabled || connected} onChange={event => setPortamento({ ...portamento, curve: event.target.value })}>
                <option value="">—</option>
                <option value="linear">Linear</option>
                <option value="ease-in">Ease in</option>
                <option value="ease-out">Ease out</option>
              </select>
            </Row>
          </>}
          {section === 'Vibrato' && <>
            <Row label="Enabled"><input type="checkbox" checked={noteVibrato.on} disabled={vibratoDisabled} onChange={event => setVibrato({ on: event.target.checked, length: event.target.checked && noteVibrato.length === 0 ? 75 : noteVibrato.length })} /></Row>
            <Row label="Preset">
              <span className="ou-prop-buttons">
                <select value="" disabled title="Vibrato presets are user preferences, not project-session data"><option>—</option></select>
                <button type="button" disabled title="Preset management is not connected">Save</button>
                <button type="button" disabled title="Preset management is not connected">Remove</button>
              </span>
            </Row>
            <Row label="Length"><NumberSlider value={noteVibrato.length} disabled={vibratoDisabled} onChange={value => setVibrato({ length: value })} /></Row>
            <Row label="Period"><NumberSlider value={noteVibrato.period} min={5} max={500} disabled={vibratoDisabled} onChange={value => setVibrato({ period: value })} /></Row>
            <Row label="Depth"><NumberSlider value={noteVibrato.depth} min={5} max={200} disabled={vibratoDisabled} onChange={value => setVibrato({ depth: value })} /></Row>
            <Row label="Fade In"><NumberSlider value={noteVibrato.fadeIn} disabled={vibratoDisabled} onChange={value => setVibrato({ fadeIn: value })} /></Row>
            <Row label="Fade Out"><NumberSlider value={noteVibrato.fadeOut} disabled={vibratoDisabled} onChange={value => setVibrato({ fadeOut: value })} /></Row>
            <Row label="Shift"><NumberSlider value={noteVibrato.shift} min={0} max={100} disabled={vibratoDisabled} onChange={value => setVibrato({ shift: value })} /></Row>
            <Row label="Drift"><NumberSlider value={noteVibrato.drift} min={-100} max={100} disabled={vibratoDisabled} onChange={value => setVibrato({ drift: value })} /></Row>
            <Row label="Volume link"><NumberSlider value={noteVibrato.volLink ?? 0} min={-100} max={100} disabled={vibratoDisabled} onChange={value => setVibrato({ volLink: value })} /></Row>
            <Row label="Set only on long note"><input type="checkbox" disabled title="Auto-vibrato defaults are not part of per-note session data yet" /></Row>
            <Row label="Minimum length"><NumberSlider value={110} min={0} max={1000} disabled onChange={() => {}} /></Row>
            {connected && <p className="ou-prop-hint">Vibrato is stored by the project session and applied by Rust rendering.</p>}
          </>}
          {section === 'Expressions' && <>
            <Row label="voice color">
              <select value={expressions.voiceColor} disabled={disabled || connected} onChange={event => setExpressions({ ...expressions, voiceColor: event.target.value })}>
                <option value="">—</option>
                <option value="bright">Bright</option>
                <option value="dark">Dark</option>
              </select>
            </Row>
            <Row label="velocity"><NumberSlider value={expressions.velocity} disabled={disabled || connected} onChange={value => setExpressions({ ...expressions, velocity: value })} /></Row>
            <Row label="volume"><NumberSlider value={expressions.volume} disabled={disabled || connected} onChange={value => setExpressions({ ...expressions, volume: value })} /></Row>
            <Row label="modulation"><NumberSlider value={expressions.modulation} min={-100} max={100} disabled={disabled || connected} onChange={value => setExpressions({ ...expressions, modulation: value })} /></Row>
            <Row label="modulation plus"><input type="checkbox" checked={expressions.alt} disabled={disabled || connected} onChange={event => setExpressions({ ...expressions, alt: event.target.checked })} /></Row>
            <Row label="alternate"><input type="checkbox" disabled={disabled || connected} /></Row>
            <Row label="direct off"><input type="checkbox" checked={expressions.direct} disabled={disabled || connected} onChange={event => setExpressions({ ...expressions, direct: event.target.checked })} /></Row>
            <Row label="tone shift"><NumberSlider value={expressions.toneShift} min={-36} max={36} disabled={disabled || connected} onChange={value => setExpressions({ ...expressions, toneShift: value })} /></Row>
            <Row label="voice color y"><NumberSlider value={0} min={-100} max={100} disabled onChange={() => {}} /></Row>
          </>}
        </div>}
      </section>
    ))}
  </div>;
}
