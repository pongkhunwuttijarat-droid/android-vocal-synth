// Mock project data shaped like the Rust session snapshot DTOs, so every
// component here can later be re-mounted on the real backend without changing
// its props. Field names deliberately match engine snapshot fields.
export const TICKS_PER_BEAT = 480;
export const BEATS_PER_MEASURE = 4;
export const TICKS_PER_MEASURE = TICKS_PER_BEAT * BEATS_PER_MEASURE;
export const BPM = 120;
export const TIME_SIG = '4/4';

export const TRACKS = [
  {
    id: 'track-1',
    name: 'Track1',
    singer_id: '重音テト (かさねてと) OU 用',
    singer_detail: 'JA VCV & CVVC · WORLDLINE-R',
    mute: false,
    solo: false,
    volume_db: 0,
    pan: 0,
    parts: [
      { id: 'part-1', track_id: 'track-1', kind: 'voice', name: 'New Part', position_ticks: 0, duration_ticks: TICKS_PER_MEASURE * 14 },
    ],
  },
];

export const NOTES = [
  { id: 'note-1', part_id: 'part-1', position_ticks: TICKS_PER_BEAT * 1, duration_ticks: TICKS_PER_BEAT, tone: 66, lyric: 'あ' },
  { id: 'note-2', part_id: 'part-1', position_ticks: TICKS_PER_BEAT * 2, duration_ticks: TICKS_PER_BEAT, tone: 66, lyric: 'い' },
  { id: 'note-3', part_id: 'part-1', position_ticks: TICKS_PER_BEAT * 3, duration_ticks: TICKS_PER_BEAT, tone: 68, lyric: 'う' },
  { id: 'note-4', part_id: 'part-1', position_ticks: TICKS_PER_BEAT * 4, duration_ticks: TICKS_PER_BEAT, tone: 70, lyric: 'え' },
];

/**
 * The expression descriptors the Rust engine actually accepts, from
 * CONTRACT-v1 §6.6.1. The previous list here invented OpenUtau-fake abbrs
 * (DYN/PTD/CLR/ENG/VEL/VOL) with invented ranges; §6 `curve.apply_stroke`
 * validates every point against the descriptor's own min/max/default, so a tab
 * that shows the wrong bounds draws a curve the engine would clamp or reject.
 *
 *   abbr  min    max   default  engine slot
 *   dyn   -240   120   0        curves.dynamics (dB)
 *   pitd  -1200  1200  0        pitch computer (cents)
 *   genc  -100   100   0        curves.gender
 *   brec  -100   100   0        curves.breathiness
 *   shfc  -1200  1200  0        curves.extra
 *   tenc  -100   100   0        curves.tension
 *   voic  0      100   100      curves.voicing
 */
export const EXPRESSION_LANES = [
  { abbr: 'dyn', name: 'Dynamics', min: -240, max: 120, default: 0, unit: 'dB', color: '#e8628f' },
  { abbr: 'pitd', name: 'Pitch deviation', min: -1200, max: 1200, default: 0, unit: 'cents', color: '#b07fd4' },
  { abbr: 'genc', name: 'Voice color', min: -100, max: 100, default: 0, unit: '', color: '#64a8d8' },
  { abbr: 'brec', name: 'Breathiness', min: -100, max: 100, default: 0, unit: '', color: '#7dd4a8' },
  { abbr: 'shfc', name: 'Shift flag', min: -1200, max: 1200, default: 0, unit: 'cents', color: '#d8a754' },
  { abbr: 'tenc', name: 'Tension', min: -100, max: 100, default: 0, unit: '', color: '#a88fe8' },
  { abbr: 'voic', name: 'Voicing', min: 0, max: 100, default: 100, unit: '', color: '#8f9bb3' },
];

/** Clamp to the descriptor, the same rule `curve.apply_stroke` applies. */
export const clampToLane = (lane, value) => Math.max(lane.min, Math.min(lane.max, value));

/** Tone names for the piano keyboard, MIDI 0-127. */
export const TONE_NAMES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];
export const toneName = tone => `${TONE_NAMES[tone % 12]}${Math.floor(tone / 12) - 1}`;
export const isBlackKey = tone => [1, 3, 6, 8, 10].includes(tone % 12);

/**
 * Phoneme text shown in place of the lyric when the roll's label mode is
 * "phoneme", the way OpenUtau switches to phonemes in its piano roll. These
 * are mock stand-ins: with a real session the strings come from the engine's
 * phonemizer result for the note, not from this table.
 */
export const PHONEMES = {
  '\u3042': 'a', '\u3044': 'i', '\u3046': 'u', '\u3048': 'e', '\u304a': 'o',
  '\u304b': 'ka', '\u304d': 'ki', '\u3051': 'ke', '\u3053': 'ko',
  '\u3055': 'sa', '\u3057': 'shi', '\u3059': 'su', '\u305b': 'se', '\u305d': 'so',
  '\u305f': 'ta', '\u3061': 'chi', '\u3064': 'tsu', '\u3066': 'te', '\u3068': 'to',
  '\u306a': 'na', '\u306b': 'ni', '\u306c': 'nu', '\u306d': 'ne', '\u306e': 'no',
  '\u306f': 'ha', '\u3072': 'hi', '\u3075': 'hu', '\u3078': 'he', '\u307b': 'ho',
  '\u307e': 'ma', '\u307f': 'mi', '\u3080': 'mu', '\u3081': 'me', '\u3082': 'mo',
  '\u3084': 'ya', '\u3086': 'yu', '\u3088': 'yo',
  '\u3089': 'ra', '\u308a': 'ri', '\u308b': 'ru', '\u308c': 're', '\u308d': 'ro',
  '\u3093': 'n', '\u309d': 'N',
};
export const phonemeOf = lyric => PHONEMES[lyric] ?? lyric ?? '';
