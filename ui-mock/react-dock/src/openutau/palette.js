// Role-based colour tokens for the clone shell, modelled on OpenUtau's
// `ThemeManager` (ref-openutau-clean/desktop-ref/OpenUtau/ThemeManager.cs):
// nothing in the UI names a raw colour, every surface asks for a ROLE, so a
// theme is one token map instead of ~150 edits.
//
// OpenUtau keeps those roles as static fields (`ForegroundBrush`,
// `TickLineBrushLow`, `WhiteKeyBrush`, `ExpActiveBrush`, …) and its XAML asks
// for them by name (`{DynamicResource BackgroundBrushSemi}`). Here the same
// idea in two layers:
//
//   * CSS gets the tokens as custom properties on `<html>` (`--ou-<kebab>`), so
//     `openutau.css` never contains a hex literal either.
//   * PixiJS cannot read CSS variables, so the canvas panels read the resolved
//     token object through `useTheme()` / `P()` and a theme change re-runs the
//     draw pass.
//
// Theme keys mirror OpenUtau's `GetAvailableThemes()`: "light", "dark", plus
// anything registered from `themes/*.json` (OpenUtau ships installable theme
// packages through its ThemeLoader; ours are bundled JSON presets with the
// same shape, so dropping a file in adds a theme with no code change).
import { useSyncExternalStore } from 'react';

// Bundled presets. Vite turns this into an import map at build time.
const PRESET_MODULES = import.meta.glob('./themes/*.json', { eager: true });

/** `#rrggbb` -> the integer PixiJS wants. Memoised: the draw pass calls it a lot. */
const pixiCache = new Map();
export const pixi = hex => {
  if (typeof hex === 'number') return hex;
  const cached = pixiCache.get(hex);
  if (cached !== undefined) return cached;
  const value = parseInt(String(hex).replace('#', ''), 16);
  pixiCache.set(hex, value);
  return value;
};

/**
 * The two shipped themes. Values are the clone shell's current light palette
 * and a dark counterpart derived from OpenUtau's dark keyboard/expression
 * inversion (ExpBrush follows the black keys in light mode and the white keys
 * in dark mode — see `ThemeManager.SetKeyboardBrush`).
 */
const LIGHT = {
  surface: '#f2f2f4',
  surfaceAlt: '#f7f7f9',
  surfaceSunken: '#f0f0f3',
  surfaceRaised: '#fbfbfd',
  surfaceOverlay: '#ffffffdd',
  border: '#d5d5de',
  borderStrong: '#d0d0da',
  divider: '#e2e2ea',

  text: '#28303c',
  textStrong: '#3c4450',
  textMuted: '#6f6f7c',
  textFaint: '#8a8a98',
  textOnAccent: '#ffffff',

  accent: '#e8628f',
  accentHover: '#fdeef4',
  accentText: '#c23e70',
  accentSoft: '#fdeef4',
  accent2: '#7db8ef',
  accent2Text: '#3d7fc4',
  accent3: '#e8b45d',

  okBg: '#e6f6ec',
  okText: '#1f7a45',
  warnBg: '#fdeef4',
  warnText: '#a03a6a',

  trackCard: '#ffffff',
  trackCardBorder: '#d0d0da',
  trackSideBtn: '#f6f6f9',
  trackSideBtnBorder: '#c5c5d2',
  trackMuted: '#9aa0ae',
  sliderTrack: '#e2e2ea',
  sliderKnob: '#ffffff',
  sliderKnobBorder: '#b0b0c0',

  // Interactive controls (menu-bar buttons, selects, chips). Kept apart from
  // the slider roles on purpose: OpenUtau's slider knob inverts with the theme
  // (light on dark), so reusing it for a button face paints light chrome on a
  // dark bar.
  controlBg: '#f6f6f9',
  controlBgHover: '#ececf2',
  controlBorder: '#d5d5de',
  controlText: '#3c4450',

  arrRowA: '#fafafb',
  arrRowB: '#f2f2f4',
  arrRowActive: '#fdf0f5',
  arrMeasure: '#c9c9d2',
  arrBeat: '#e3e3ea',
  arrRuler: '#eceef2',
  arrRulerLine: '#c9c9d2',
  arrRulerStrong: '#a9a9b6',
  arrRulerWeak: '#d5d5de',
  arrNumber: '#6f6f7c',
  marqueeFill: '#7db8ef26',
  marqueeStroke: '#3d7fc4',

  partFill: '#e8558a',
  partFillActive: '#ff77a8',
  partFillMuted: '#f0c3d5',
  partStroke: '#d9a3b8',
  partStrokeActive: '#c23e70',
  partText: '#ffffff',
  partTextActive: '#7a1f42',
  partShelf: '#ffffff',

  rollBg: '#ffffff',
  rollShortcut: '#f7f7f9',
  rollKeyBg: '#f4f4f6',
  rollKeyWhite: '#fbfbfd',
  rollKeyBlack: '#3c3c46',
  rollKeyBorder: '#d8d8e0',
  rollKeyName: '#8a3a5a',
  rollShortcutText: '#8a8a98',
  rollRow: '#ffffff',
  rollRowBlack: '#f6f4f9',
  rollCHighlight: '#e8628f',
  gridLine: '#ececf2',
  gridLineMeasure: '#b9b9c6',
  gridLineSoft: '#e6e6ee',
  rulerBg: '#eceef2',
  rulerLine: '#c9c9d2',
  rulerText: '#6f6f7c',
  measureNumber: '#6f6f7c',

  noteFill: '#b7d9f5',
  noteFillSelected: '#7db8ef',
  noteFillForeign: '#dfe7ee',
  noteStroke: '#8fbede',
  noteStrokeSelected: '#3d7fc4',
  noteLabel: '#28303c',
  noteLabelVibrato: '#a03a6a',
  noteShelf: '#5a90bd',
  playhead: '#e8628f',

  expGrid: '#ececf2',
  expGridMeasure: '#d3d3de',
  expHLine: '#f0f0f5',
  expFill: '#ffffff',
  expReadoutBg: '#fffffff0',
  expReadoutText: '#3c4450',
  expTabText: '#5a6070',
  expTabActiveText: '#3c4450',

  waveBg: '#ffffffb0',
  waveBorder: '#dcdce4',
  waveHandle: '#ffffffcc',
  waveHandleLine: '#e6e6ee',
  waveGrip: '#b6b6c4',
  waveText: '#8a8a98',
  waveStroke: '#8a8a98',
  // The 2D peak envelope uses translucent values; OpenUtau paints it in 50%
  // grey (`WaveformImage.Render`).
  waveSurface: 'rgba(120,120,132,0.10)',
  waveZero: 'rgba(96,96,110,0.16)',
  wavePeak: 'rgba(96,96,110,0.55)',

  shadow: '#00000014',
  shadowSoft: '#00000010',
  shadowText: '#00000018',
};

const DARK = {
  surface: '#22242b',
  surfaceAlt: '#1d1f25',
  surfaceSunken: '#262932',
  surfaceRaised: '#2a2d36',
  surfaceOverlay: '#2a2d36dd',
  border: '#3c4049',
  borderStrong: '#464b56',
  divider: '#33363f',

  text: '#e4e7ee',
  textStrong: '#d2d6df',
  textMuted: '#9aa1af',
  textFaint: '#7d8493',
  textOnAccent: '#ffffff',

  accent: '#ff77a8',
  accentHover: '#4a2634',
  accentText: '#ff9ec1',
  accentSoft: '#3a2029',
  accent2: '#64a8d8',
  accent2Text: '#8cc2e8',
  accent3: '#e8b45d',

  okBg: '#1d3327',
  okText: '#7fd3a2',
  warnBg: '#3a2029',
  warnText: '#ff9ec1',

  trackCard: '#2a2d36',
  trackCardBorder: '#3c4049',
  trackSideBtn: '#31343e',
  trackSideBtnBorder: '#464b56',
  trackMuted: '#79808f',
  sliderTrack: '#3c4049',
  sliderKnob: '#e4e7ee',
  sliderKnobBorder: '#565b68',

  controlBg: '#31343e',
  controlBgHover: '#3d414d',
  controlBorder: '#464b56',
  controlText: '#d2d6df',

  arrRowA: '#22242b',
  arrRowB: '#262932',
  arrRowActive: '#33222b',
  arrMeasure: '#4a4f5a',
  arrBeat: '#33363f',
  arrRuler: '#262932',
  arrRulerLine: '#464b56',
  arrRulerStrong: '#6b7180',
  arrRulerWeak: '#3c4049',
  arrNumber: '#9aa1af',
  marqueeFill: '#64a8d826',
  marqueeStroke: '#64a8d8',

  partFill: '#b8527a',
  partFillActive: '#ff77a8',
  partFillMuted: '#6b4453',
  partStroke: '#8a4b64',
  partStrokeActive: '#ff9ec1',
  partText: '#ffffff',
  partTextActive: '#2a0f1c',
  partShelf: '#e4e7ee',

  rollBg: '#1f2128',
  rollShortcut: '#262932',
  rollKeyBg: '#262932',
  rollKeyWhite: '#33363f',
  rollKeyBlack: '#15161b',
  rollKeyBorder: '#3c4049',
  rollKeyName: '#ffc0d8',
  rollShortcutText: '#9aa1af',
  rollRow: '#1f2128',
  rollRowBlack: '#232630',
  rollCHighlight: '#ff77a8',
  gridLine: '#2b2e37',
  gridLineMeasure: '#4a4f5a',
  gridLineSoft: '#262932',
  rulerBg: '#262932',
  rulerLine: '#464b56',
  rulerText: '#9aa1af',
  measureNumber: '#9aa1af',

  noteFill: '#3a5f80',
  noteFillSelected: '#4d86b8',
  noteFillForeign: '#2f3540',
  noteStroke: '#5a7f9e',
  noteStrokeSelected: '#8cc2e8',
  noteLabel: '#e4e7ee',
  noteLabelVibrato: '#ff9ec1',
  noteShelf: '#8cc2e8',
  playhead: '#ff77a8',

  expGrid: '#262932',
  expGridMeasure: '#3c4049',
  expHLine: '#2b2e37',
  expFill: '#1f2128',
  expReadoutBg: '#2a2d36f0',
  expReadoutText: '#d2d6df',
  expTabText: '#9aa1af',
  expTabActiveText: '#e4e7ee',

  waveBg: '#1f2128b0',
  waveBorder: '#3c4049',
  waveHandle: '#2a2d36cc',
  waveHandleLine: '#33363f',
  waveGrip: '#4a4f5a',
  waveText: '#7d8493',
  waveStroke: '#9aa1af',
  waveSurface: 'rgba(158,166,186,0.12)',
  waveZero: 'rgba(180,188,206,0.20)',
  wavePeak: 'rgba(200,208,226,0.55)',

  shadow: '#00000040',
  shadowSoft: '#00000030',
  shadowText: '#00000050',
};

/**
 * OpenUtau's 18 track palettes, verbatim from `ThemeManager.TrackColors`
 * (name, accent, dark, light, semi). Our arrangement and roll use `accent` for
 * a track's colour and the roll keeps the per-track CSS variable so a selected
 * track tints the keyboard and notes like OpenUtau's `SelectedTrackAccent*`.
 */
export const TRACK_COLORS = [
  ['Pink', '#F06292', '#EC407A', '#F48FB1', '#FAC7D8'],
  ['Red', '#EF5350', '#E53935', '#E57373', '#F2B9B9'],
  ['Orange', '#FF8A65', '#FF7043', '#FFAB91', '#FFD5C8'],
  ['Yellow', '#FBC02D', '#F9A825', '#FDD835', '#FEF1B6'],
  ['Light Green', '#CDDC39', '#C0CA33', '#DCE775', '#F2F7CE'],
  ['Green', '#66BB6A', '#43A047', '#A5D6A7', '#D2EBD3'],
  ['Light Blue', '#4FC3F7', '#29B6F6', '#81D4FA', '#C0EAFD'],
  ['Blue', '#4EA6EA', '#1E88E5', '#90CAF9', '#C8E5FC'],
  ['Purple', '#BA68C8', '#AB47BC', '#CE93D8', '#E7C9EC'],
  ['Pink2', '#E91E63', '#C2185B', '#F06292', '#F8B1C9'],
  ['Red2', '#D32F2F', '#B71C1C', '#EF5350', '#F7A9A8'],
  ['Orange2', '#FF5722', '#E64A19', '#FF7043', '#FFB8A1'],
  ['Yellow2', '#FF8F00', '#FF7F00', '#FFB300', '#FFE097'],
  ['Light Green2', '#AFB42B', '#9E9D24', '#CDDC39', '#E6EE9C'],
  ['Green2', '#2E7D32', '#1B5E20', '#43A047', '#A1D0A3'],
  ['Light Blue2', '#1976D2', '#0D47A1', '#2196F3', '#90CBF9'],
  ['Blue2', '#3949AB', '#283593', '#5C6BC0', '#AEB5E0'],
  ['Purple2', '#7B1FA2', '#4A148C', '#AB47BC', '#D5A3DE'],
].map(([name, accent, accentDark, accentLight, accentSemi]) => ({
  name, accent, accentDark, accentLight, accentSemi,
}));

/** Stable track colour: OpenUtau assigns by track index, so replans keep it. */
export const trackColor = index => TRACK_COLORS[((index % TRACK_COLORS.length) + TRACK_COLORS.length) % TRACK_COLORS.length];

const themes = new Map([
  ['light', { label: 'Light', tokens: LIGHT }],
  ['dark', { label: 'Dark', tokens: DARK }],
]);

// A preset only overrides the tokens it names, so a small JSON file can retint
// one role without restating the other ~90.
for (const [path, module] of Object.entries(PRESET_MODULES)) {
  const preset = module?.default ?? module;
  const key = path.replace(/^.*themes\//, '').replace(/\.json$/, '');
  themes.set(key, { label: preset.label || key, tokens: { ...LIGHT, ...(preset.tokens || {}) }, darkTokens: preset.darkTokens });
}

const STORAGE_KEY = 'rusted-utau.ou-theme';

let currentKey = null;
let current = themes.get('light').tokens;
const listeners = new Set();

const kebab = key => key.replace(/[A-Z]/g, ch => `-${ch.toLowerCase()}`);

/** Writes the resolved tokens onto `<html>` as `--ou-*` custom properties. */
const writeCssVariables = tokens => {
  const root = document.documentElement;
  for (const [key, value] of Object.entries(tokens)) root.style.setProperty(`--ou-${kebab(key)}`, value);
  root.dataset.ouTheme = currentKey || 'light';
};

const notify = () => { for (const listener of listeners) listener(); };

/**
 * Switch theme. `key` may be a shipped theme or a preset key; unknown keys are
 * ignored rather than blanking the UI. The choice persists in localStorage
 * because it is presentation state, like the dock layout.
 */
export function setTheme(key) {
  const entry = themes.get(key);
  if (!entry || entry.tokens === current) return current;
  currentKey = key;
  current = entry.tokens;
  writeCssVariables(current);
  try { localStorage.setItem(STORAGE_KEY, key); } catch { /* private mode */ }
  notify();
  return current;
}

export const themeKey = () => currentKey || 'light';
export const themeList = () => [...themes.entries()].map(([key, entry]) => ({ key, label: entry.label }));

/**
 * The active token map. Canvas draw code calls this directly (`P().noteFill`)
 * so it does not need theme props threaded through every callback.
 */
export const P = () => current;

/** Subscribe a Pixi panel to theme changes so it repaints on switch. */
export function useTheme() {
  useSyncExternalStore(
    listener => { listeners.add(listener); return () => listeners.delete(listener); },
    () => current,
    () => current,
  );
  return current;
}

/** Apply the persisted (or default) theme. Called once when the shell mounts. */
export function initTheme() {
  let stored = null;
  try { stored = localStorage.getItem(STORAGE_KEY); } catch { /* private mode */ }
  currentKey = null;
  return setTheme(stored && themes.has(stored) ? stored : 'light');
}