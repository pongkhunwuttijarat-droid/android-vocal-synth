import React, { useState } from 'react';

const DEFAULTS = {
  enabled: false,
  eq_preset: 'vocal_air', comp_preset: 'gentle', reverb_preset: 'small_room',
  eq_low_db: 0, eq_mid_freq: 3000, eq_mid_db: 1.5, eq_high_db: 3,
  comp_threshold_db: -18, comp_ratio: 2, comp_makeup_db: 2.5,
  reverb_size: 0.3, reverb_damp: 0.7, reverb_wet: 1, reverb_pre_delay_ms: 0,
};

const EQ = {
  off: [0, 1000, 0, 0], vocal_air: [0, 3000, 1.5, 3], warm: [2, 500, 0, -1],
  demud: [0, 200, -3, 0], telephone: [-6, 1500, 4, -6],
};
const COMP = { off: [0, 1, 0], gentle: [-18, 2, 2.5], pop: [-14, 3, 4], limit: [-3, 10, 0] };
const REVERB = {
  off: [0, 0, 1, 0], small_room: [0.3, 0.7, 1, 12], vocal_plate: [0.55, 0.3, 1, 25],
  hall: [0.85, 0.4, 1, 40], ambient: [0.92, 0.2, 1, 60],
};

const NUMBER_FIELDS = [
  ['eq_low_db', 'Low EQ (dB)', -24, 24, 0.1], ['eq_mid_freq', 'Mid EQ (Hz)', 20, 20000, 1],
  ['eq_mid_db', 'Mid EQ gain (dB)', -24, 24, 0.1], ['eq_high_db', 'High EQ (dB)', -24, 24, 0.1],
  ['comp_threshold_db', 'Compressor threshold (dB)', -60, 0, 0.1], ['comp_ratio', 'Compressor ratio', 1, 20, 0.1],
  ['comp_makeup_db', 'Compressor makeup (dB)', -24, 24, 0.1], ['reverb_size', 'Reverb size', 0, 1, 0.01],
  ['reverb_damp', 'Reverb damping', 0, 1, 0.01], ['reverb_wet', 'Reverb wet', 0, 2, 0.01],
  ['reverb_pre_delay_ms', 'Reverb pre-delay (ms)', 0, 200, 1],
];

function Field({ label, children }) {
  return <label style={{ display: 'grid', gridTemplateColumns: '1fr 150px', gap: 12, alignItems: 'center', margin: '8px 0' }}>
    <span>{label}</span>{children}
  </label>;
}

export function TrackFxDialog({ track, onClose, onApply }) {
  const [draft, setDraft] = useState({ ...DEFAULTS, ...(track.mix_fx || {}) });
  const set = (key, value) => setDraft(current => ({ ...current, [key]: value }));
  const setPreset = (key, value, table, fields) => {
    const values = table[value];
    setDraft(current => ({ ...current, [key]: value, ...Object.fromEntries(fields.map((name, i) => [name, values[i]])) }));
  };
  return <div role="presentation" onMouseDown={event => { if (event.target === event.currentTarget) onClose(); }}
    style={{ position: 'fixed', inset: 0, zIndex: 1000, background: '#0009', display: 'grid', placeItems: 'center', padding: 24 }}>
    <section role="dialog" aria-modal="true" aria-label={`${track.name} track effects`}
      style={{ width: 'min(620px, 96vw)', maxHeight: '90vh', overflow: 'auto', background: 'var(--ou-panel, #202028)', color: 'var(--ou-text, #eee)', border: '1px solid #666', borderRadius: 12, padding: 20, boxShadow: '0 16px 48px #0008' }}>
      <header style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
        <h2 style={{ margin: 0 }}>{track.name} · Track FX</h2>
        <button type="button" onClick={onClose} aria-label="Close">×</button>
      </header>
      <Field label="Enabled"><input type="checkbox" checked={draft.enabled} onChange={event => set('enabled', event.target.checked)} /></Field>
      <h3>Equalizer</h3>
      <Field label="Preset"><select value={draft.eq_preset} onChange={event => setPreset('eq_preset', event.target.value, EQ, ['eq_low_db', 'eq_mid_freq', 'eq_mid_db', 'eq_high_db'])}>{Object.keys(EQ).map(key => <option key={key}>{key}</option>)}</select></Field>
      <h3>Compressor</h3>
      <Field label="Preset"><select value={draft.comp_preset} onChange={event => setPreset('comp_preset', event.target.value, COMP, ['comp_threshold_db', 'comp_ratio', 'comp_makeup_db'])}>{Object.keys(COMP).map(key => <option key={key}>{key}</option>)}</select></Field>
      <h3>Reverb</h3>
      <Field label="Preset"><select value={draft.reverb_preset} onChange={event => setPreset('reverb_preset', event.target.value, REVERB, ['reverb_size', 'reverb_damp', 'reverb_wet', 'reverb_pre_delay_ms'])}>{Object.keys(REVERB).map(key => <option key={key}>{key}</option>)}</select></Field>
      <details style={{ marginTop: 12 }}>
        <summary>Effect parameters</summary>
        {NUMBER_FIELDS.map(([key, label, min, max, step]) => <Field key={key} label={label}>
          <input type="number" min={min} max={max} step={step} value={draft[key]} onChange={event => set(key, Number(event.target.value))} />
        </Field>)}
      </details>
      <footer style={{ display: 'flex', justifyContent: 'space-between', gap: 8, marginTop: 18 }}>
        <button type="button" onClick={() => onApply(null)}>Clear FX</button>
        <span style={{ display: 'flex', gap: 8 }}>
          <button type="button" onClick={onClose}>Cancel</button>
          <button type="button" onClick={() => onApply(draft)}>Apply</button>
        </span>
      </footer>
    </section>
  </div>;
}
