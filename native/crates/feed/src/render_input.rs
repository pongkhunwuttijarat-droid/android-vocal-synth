//! The `RenderInput` superset data model (Sprint 1.4).
//!
//! Every renderer plugin consumes a [`RenderInput`]; a plugin reads only
//! the sections its capability declares:
//!
//! * worldline / classic (sample-based): [`RenderInput::sample_based`] —
//!   per-phoneme oto entries, wav paths, envelope and resampler flags;
//! * neural renderers (DiffSinger-style): [`RenderInput::neural`] —
//!   tokens, per-frame durations, f0 and key-shifted f0;
//! * every renderer: [`RenderInput::phrase`], [`RenderInput::notes`],
//!   [`RenderInput::phonemes`], [`RenderInput::pitches_cents`] and
//!   [`RenderInput::curves`].
//!
//! The shape mirrors `native/test-data/render-input.example.json`; the
//! sample-based section additionally carries per-entry envelope/flags so
//! multi-phoneme phrases stay lossless.

/// One fully prepared phrase, ready for any renderer plugin.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderInput {
    /// Phrase-level timing in milliseconds.
    pub phrase: PhraseInfo,
    /// Notes of the phrase (project order).
    pub notes: Vec<RenderNote>,
    /// Derived phonemes with oto timing (project order).
    pub phonemes: Vec<RenderPhoneme>,
    /// Pitch in cents, sampled every 5 ticks over the pitch grid
    /// (starts `leading` ticks before the first phoneme).
    pub pitches_cents: Vec<i32>,
    /// Sampled expression curves, per-5-tick aligned with `pitches_cents`.
    pub curves: Curves,
    /// Sample-based (worldline/classic) inputs; filled when the renderer
    /// capability declares `needs_oto` / `needs_wav`.
    pub sample_based: Option<SampleBased>,
    /// Neural (DiffSinger-style) inputs; filled when the renderer
    /// capability declares `needs_neural`.
    pub neural: Option<NeuralInput>,
}

/// Phrase-level timing (`render-input.example.json` → `phrase`).
#[derive(Debug, Clone, PartialEq)]
pub struct PhraseInfo {
    /// Position of the first phoneme, relative to the project start (ms).
    pub position_ms: f64,
    /// Duration from the first phoneme start to the last phoneme end (ms).
    pub duration_ms: f64,
    /// Leading (preutter) time of the first phoneme (ms).
    pub leading_ms: f64,
    /// Leading time in ticks (used by the pitch grid start).
    pub leading_ticks: i32,
    /// Human-readable tempo/time-signature hint, e.g. `"120bpm 4/4"`.
    pub time_axis_hint: Option<String>,
}

/// A note as the feed sees it (`render-input.example.json` → `notes[]`).
#[derive(Debug, Clone, PartialEq)]
pub struct RenderNote {
    pub lyric: String,
    /// MIDI note number (C4 = 60).
    pub tone: i32,
    pub position_ms: f64,
    pub duration_ms: f64,
}

/// A derived phoneme (`render-input.example.json` → `phonemes[]`).
#[derive(Debug, Clone, PartialEq)]
pub struct RenderPhoneme {
    /// Resolved phoneme / oto alias (e.g. `"3 h3"`).
    pub phoneme: String,
    /// Position relative to the project start (ms).
    pub position_ms: f64,
    pub duration_ms: f64,
    /// Preutter time (ms).
    pub leading_ms: f64,
    /// Overlap with the previous phoneme (ms).
    pub overlap_ms: f64,
    /// MIDI tone of the parent note.
    pub tone: i32,
    /// Per-phoneme tone shift in semitones (`shft` expression).
    pub tone_shift: i32,
    /// Index of the phoneme within its note.
    pub index: i32,
    /// Index of the parent note in [`RenderInput::notes`].
    pub parent_note: usize,
}

/// Sampled expression curves, aligned with the pitch grid.
///
/// `render-input.example.json` → `curves`. Defaults follow the expression
/// descriptors: dynamics 1.0 (0 dB), gender/breathiness/tension 0.0,
/// voicing 100.0. Renderer-specific curves land in [`Curves::extra`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Curves {
    /// Volume curve: 0.0 at the descriptor minimum, else dB → linear
    /// (`10^(x/200)`), matching `RenderPhrase.SampleCurve`.
    pub dynamics: Vec<f32>,
    pub gender: Vec<f32>,
    pub breathiness: Vec<f32>,
    pub tension: Vec<f32>,
    /// 0..100; default 100.
    pub voicing: Vec<f32>,
    /// Any other curve-type expressions (e.g. `shfc`, `velc`), sampled the
    /// same way.
    pub extra: Vec<NamedCurve>,
}

/// One sampled extra curve.
#[derive(Debug, Clone, PartialEq)]
pub struct NamedCurve {
    pub abbr: String,
    pub values: Vec<f32>,
}

/// Sample-based renderer inputs (`render-input.example.json` →
/// `sample_based`).
#[derive(Debug, Clone, PartialEq)]
pub struct SampleBased {
    /// One entry per phoneme that mapped to an oto, in phoneme order.
    pub oto: Vec<OtoEntry>,
    /// Absolute wav path of the first mapped phoneme (example-schema
    /// convenience mirror of the first entry's `wav_path`).
    pub wav_path: Option<String>,
    /// 5-point envelope of the first mapped phoneme, in ms space with
    /// amplitude normalized to 0..1 (example-schema mirror).
    pub envelope: Vec<EnvelopePoint>,
    /// Resampler flags string of the first mapped phoneme, e.g.
    /// `"g0B0H0P86"` (example-schema mirror).
    pub flags: String,
}

/// The oto data of one mapped phoneme.
#[derive(Debug, Clone, PartialEq)]
pub struct OtoEntry {
    /// Mapped alias as found in the voicebank, e.g. `"3 h3"`.
    pub alias: String,
    /// Wav file name, relative to the oto.ini directory.
    pub file: String,
    /// Absolute path of the wav file.
    pub wav_path: String,
    pub offset: f64,
    pub consonant: f64,
    pub cutoff: f64,
    pub preutter: f64,
    pub overlap: f64,
    /// 5-point envelope in ms space, amplitude normalized to 0..1.
    pub envelope: Vec<EnvelopePoint>,
    /// Resampler flags string, e.g. `"g0B0H0P86"`.
    pub flags: String,
}

/// One envelope point: `x_ms` relative to the phoneme start (may be
/// negative — the preutter region), `y` amplitude in 0..=1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnvelopePoint {
    pub x_ms: f32,
    pub y: f32,
}

impl EnvelopePoint {
    pub fn new(x_ms: f32, y: f32) -> Self {
        EnvelopePoint { x_ms, y }
    }
}

/// Neural renderer inputs (`render-input.example.json` → `neural`).
///
/// All arrays are aligned: `durations_frames` sums to `f0_hz.len()`.
#[derive(Debug, Clone, PartialEq)]
pub struct NeuralInput {
    /// `SP` + phoneme tokens + `SP` (DiffSinger token convention).
    pub tokens: Vec<i64>,
    /// `[head] + per-phoneme frames + [tail]`, head/tail = 8 frames
    /// (`DurationsMsToFrames` accumulated rounding, like the reference).
    pub durations_frames: Vec<i64>,
    /// Per-frame f0 in Hz, sampled from `pitches_cents` starting
    /// `head_frames × frame_ms` before the first phoneme.
    pub f0_hz: Vec<f64>,
    /// `f0_hz` shifted by the per-frame `shft` expression
    /// (`f0 * 2^(shift/12)`).
    pub shifted_f0_hz: Vec<f64>,
}
