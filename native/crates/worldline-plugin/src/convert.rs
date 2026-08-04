//! `RenderInput` → per-phoneme `SynthRequest` conversion (the core of
//! Sprint 2.1), ported from `OpenUtau.Core/Classic/ResamplerItem.cs` and
//! `OpenUtau.Core/Render/Worldline.cs` (`SynthRequestWrapper`).
//!
//! This module is pure Rust — no `.so` needed — and is fully unit-tested.
//! The [`WorldlineRenderer`](crate::renderer::WorldlineRenderer) turns the
//! produced [`PhonemeRequest`]s into `worldline_sys::SynthRequest` structs
//! and feeds them to the C++ `PhraseSynth`.
//!
//! # Conversion notes (C# semantics)
//!
//! * `con_vel`/`volume`/`modulation` are the raw expression values
//!   (VEL/VOL 0..200 default 100, MOD 0..100 default 0) — the C++ side
//!   divides by 100 itself (`AutoGain`: `gain = volume * 0.01`).
//!   `ResamplerItem` computes `(int)(phone.velocity * 100)` from the
//!   normalized value, which nets out to the raw value.
//! * `tone` = note tone + `shft` (the feed keeps them separate).
//! * `tempo` is the BPM (C# `phone.adjustedTempo`); `RenderInput` carries
//!   no tempo field, so the default 120 bpm is used.
//! * Flags: the oto entry's flags string (when present, e.g. `"g5P90"`)
//!   overrides the C# defaults (`g` 0, `O` 0, `P` 86, `Mt` 0, `Mb` 0,
//!   `Mv` 100); without a flags string the sampled `genc`/`tenc`/`brec`/
//!   `voic` curves feed `flag_g`/`flag_Mt`/`flag_Mb`/`flag_Mv` (the feed
//!   currently ships empty flags strings — `OtoMapper` sets `flags: ""`).
//! * `required_length` follows `ResamplerItem.durRequired`
//!   (`max(duration + skipOver, consonant)` rounded up to the next 50 ms).
//!   This keeps the `smoke.c` invariant `length_ms <= required_length`
//!   (the C++ model is trimmed to `offset + required_length` frames and
//!   `model_i = left_extra + skip + i - p0` must stay inside it).

use domain::{MOD, VEL, VOL};
use feed::cents_to_freq;
use feed::render_input::{Curves, OtoEntry, RenderInput, RenderPhoneme};

use crate::error::Error;

/// Default tempo (BPM) used when the phrase carries no tempo information
/// (C# `phone.adjustedTempo`; `RenderInput` has no tempo field).
pub const DEFAULT_TEMPO_BPM: f64 = 120.0;
/// Ticks per quarter note (`domain::RESOLUTION`), for ms↔tick conversion.
const RESOLUTION: f64 = 480.0;
/// Suffix of the leading token of the feed's tempo hint (`"120bpm 4/4"`).
const TEMPO_HINT_SUFFIX: &str = "bpm";
/// C# `SynthRequestWrapper.Validate` frame size (ms).
const FRAME_MS: f64 = 10.0;
/// C# `ResamplerItem.durRequired` rounding quantum (ms).
const DUR_REQUIRED_QUANTUM: f64 = 50.0;

/// ms per tick at `bpm` (`60000 / bpm / resolution`).
pub fn ms_per_tick_at(bpm: f64) -> f64 {
    60000.0 / bpm / RESOLUTION
}

/// Parse the leading `<bpm>bpm` token of the feed's tempo hint
/// (`"120bpm 4/4"` → `120.0`).
pub fn parse_tempo_hint(hint: Option<&str>) -> Option<f64> {
    let token = hint?.split_whitespace().next()?;
    let bpm = token.strip_suffix(TEMPO_HINT_SUFFIX)?;
    bpm.parse::<f64>()
        .ok()
        .filter(|b| b.is_finite() && *b > 0.0)
}

/// ms between two samples of the feed's 5-tick grid.
///
/// The exact value is `5 × ms_per_tick` of the project time axis, which
/// `RenderInput` does not carry; it is recovered from (in order) the
/// `leading_ms`/`leading_ticks` ratio (exact at constant tempo), the
/// `"<bpm>bpm"` tempo hint, or a 120 bpm default.
pub fn ms_per_interval(input: &RenderInput) -> f64 {
    let phrase = &input.phrase;
    if phrase.leading_ticks > 0 && phrase.leading_ms > 0.0 {
        let from_leading = phrase.leading_ms / phrase.leading_ticks as f64 * 5.0;
        if from_leading.is_finite() && from_leading > 0.0 {
            return from_leading;
        }
    }
    let bpm = parse_tempo_hint(phrase.time_axis_hint.as_deref()).unwrap_or(DEFAULT_TEMPO_BPM);
    let from_hint = ms_per_tick_at(bpm) * 5.0;
    if from_hint.is_finite() && from_hint > 0.0 {
        from_hint
    } else {
        ms_per_tick_at(DEFAULT_TEMPO_BPM) * 5.0
    }
}

// ---------------------------------------------------------------------------
// Grid sampling (C# `SampleCurve` / `ResamplerItem` pitch sampling)
// ---------------------------------------------------------------------------

/// A linear stand-in for the project time axis over the feed's 5-tick
/// sampling grid. `RenderInput` carries no `TimeAxis`, so ms positions are
/// mapped to grid indices with the constant [`ms_per_interval`].
#[derive(Debug, Clone, Copy)]
pub struct Grid {
    /// Project-absolute ms of the first grid sample
    /// (first phoneme `position_ms - leading_ms`).
    pub start_ms: f64,
    /// ms between two grid samples (5 ticks).
    pub ms_per_interval: f64,
}

impl Grid {
    /// Build the grid for `input` (uses [`ms_per_interval`]).
    pub fn new(input: &RenderInput) -> Self {
        Grid {
            start_ms: input.phrase.position_ms - input.phrase.leading_ms,
            ms_per_interval: ms_per_interval(input),
        }
    }

    /// Fractional grid index at `ms` (C# `SampleCurve` index before
    /// truncation; may be negative or past the end).
    pub fn index_at(&self, ms: f64) -> f64 {
        (ms - self.start_ms) / self.ms_per_interval
    }

    /// Integer grid index at `ms`, clamped at 0 (C# `SampleCurve`:
    /// `Math.Max(0, (int)(ticks / interval))`). Not clamped at the end —
    /// callers fall back to the expression default there, like the C#.
    pub fn int_index_at(&self, ms: f64) -> usize {
        (self.index_at(ms).floor().max(0.0)) as usize
    }

    /// Curve value at `ms` with C# `SampleCurve` truncation; `None` when
    /// the index is past the end of `curve` (caller applies the default).
    pub fn sample(&self, curve: &[f32], ms: f64) -> Option<f32> {
        let index = self.int_index_at(ms);
        curve.get(index).copied()
    }

    /// Lerped curve value at `ms` like `ResamplerItem` pitch sampling
    /// (clamped into the grid; `0.0` for an empty curve).
    pub fn sample_lerped(&self, curve: &[f32], ms: f64) -> f32 {
        if curve.is_empty() {
            return 0.0;
        }
        let index = self.index_at(ms).clamp(0.0, (curve.len() - 1) as f64);
        let lo = index.floor() as usize;
        let hi = index.ceil() as usize;
        let alpha = (index - lo as f64) as f32;
        let a = curve[lo];
        let b = curve[hi];
        a + (b - a) * alpha
    }

    /// Like [`sample`](Self::sample) for the integer pitch grid
    /// (`RenderInput::pitches_cents`), returning the value as `f32`.
    pub fn sample_cents(&self, grid: &[i32], ms: f64) -> Option<f32> {
        let index = self.int_index_at(ms);
        grid.get(index).map(|&c| c as f32)
    }

    /// Like [`sample_lerped`](Self::sample_lerped) for the integer pitch
    /// grid, returning the value as `f32`.
    pub fn sample_cents_lerped(&self, grid: &[i32], ms: f64) -> f32 {
        if grid.is_empty() {
            return 0.0;
        }
        let index = self.index_at(ms).clamp(0.0, (grid.len() - 1) as f64);
        let lo = index.floor() as usize;
        let hi = index.ceil() as usize;
        let alpha = (index - lo as f64) as f32;
        let a = grid[lo] as f32;
        let b = grid[hi] as f32;
        a + (b - a) * alpha
    }
}

/// Per-phoneme value of a numerical expression (`vel`/`vol`/`mod`) that the
/// feed ships as a sampled extra curve, or `default` when absent.
pub fn expression_value(
    input: &RenderInput,
    grid: &Grid,
    abbr: &str,
    ms: f64,
    default: f32,
) -> f32 {
    input
        .curves
        .extra
        .iter()
        .find(|curve| curve.abbr == abbr)
        .and_then(|curve| grid.sample(&curve.values, ms))
        .unwrap_or(default)
}

// ---------------------------------------------------------------------------
// Flags
// ---------------------------------------------------------------------------

/// Parsed utau-style resampler flags of one phoneme. `None` = the flag is
/// absent from the string (the caller keeps the C# default).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SynthFlags {
    pub g: Option<i32>,
    pub o: Option<i32>,
    pub p: Option<i32>,
    pub mt: Option<i32>,
    pub mb: Option<i32>,
    pub mv: Option<i32>,
}

/// Parse a flags string like `"g0B0H0P86"` or `"Mt10Mv90"` (abbr +
/// optional signed value, concatenated). Unknown flags (`B`, `H`, ...)
/// are skipped; a flag without a numeric value yields `None`, matching
/// `SynthRequestWrapper`, which only overrides flags that carry a value.
pub fn parse_flags(flags: &str) -> SynthFlags {
    let mut out = SynthFlags::default();
    let mut abbr = String::new();
    let mut value = String::new();
    let mut has_value = false;
    let flush = |out: &mut SynthFlags, abbr: &str, value: &str, has_value: bool| {
        if abbr.is_empty() || !has_value {
            return;
        }
        let Ok(value) = value.parse::<i32>() else {
            return;
        };
        match abbr {
            "g" => out.g = Some(value),
            "O" => out.o = Some(value),
            "P" => out.p = Some(value),
            "Mt" => out.mt = Some(value),
            "Mb" => out.mb = Some(value),
            "Mv" => out.mv = Some(value),
            _ => {}
        }
    };
    for ch in flags.chars() {
        if ch.is_ascii_alphabetic() {
            if !abbr.is_empty() && has_value {
                flush(&mut out, &abbr, &value, has_value);
                abbr.clear();
                value.clear();
                has_value = false;
            }
            abbr.push(ch);
        } else if (ch.is_ascii_digit() || ch == '-') && !abbr.is_empty() {
            has_value = true;
            value.push(ch);
        }
    }
    flush(&mut out, &abbr, &value, has_value);
    out
}

/// Resolved flag integers of one request (C# defaults, then overrides).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedFlags {
    pub g: i32,
    pub o: i32,
    pub p: i32,
    pub mt: i32,
    pub mb: i32,
    pub mv: i32,
}

impl ResolvedFlags {
    /// C# `SynthRequestWrapper` defaults: g 0, O 0, P 86, Mt 0, Mb 0, Mv 100.
    pub const fn defaults() -> Self {
        ResolvedFlags {
            g: 0,
            o: 0,
            p: 86,
            mt: 0,
            mb: 0,
            mv: 100,
        }
    }
}

/// Resolve the request flags for one phoneme.
///
/// A non-empty `oto.flags` string takes precedence (C# semantics — flags
/// are per-phoneme there). Without one, the sampled `genc`/`tenc`/`brec`/
/// `voic` curves map to `g`/`Mt`/`Mb`/`Mv` (Sprint 2.1 contract);
/// `O`/`P` have no curve and keep the C# defaults.
fn resolve_flags(input: &RenderInput, grid: &Grid, oto: &OtoEntry, ms: f64) -> ResolvedFlags {
    let parsed = parse_flags(&oto.flags);
    let has_flags_string = !oto.flags.trim().is_empty();
    let pick = |parsed: Option<i32>, curve: Option<f32>, default: i32| {
        if has_flags_string {
            parsed.unwrap_or(default)
        } else {
            curve.map(|v| v.round() as i32).unwrap_or(default)
        }
    };
    let defaults = ResolvedFlags::defaults();
    ResolvedFlags {
        g: pick(parsed.g, grid.sample(&input.curves.gender, ms), defaults.g),
        o: parsed.o.unwrap_or(defaults.o),
        p: parsed.p.unwrap_or(defaults.p),
        mt: pick(
            parsed.mt,
            grid.sample(&input.curves.tension, ms),
            defaults.mt,
        ),
        mb: pick(
            parsed.mb,
            grid.sample(&input.curves.breathiness, ms),
            defaults.mb,
        ),
        mv: pick(
            parsed.mv,
            grid.sample(&input.curves.voicing, ms),
            defaults.mv,
        ),
    }
}

// ---------------------------------------------------------------------------
// Request conversion
// ---------------------------------------------------------------------------

/// One converted phoneme: the `AddRequest` timing (WorldlineRenderer.cs)
/// plus every `SynthRequest` field (ResamplerItem.cs / SynthRequestWrapper).
// Field names keep the exact utau flag spelling from synth_request.h
// (flag_O/flag_P/flag_Mt/...), hence `non_snake_case`.
#[derive(Debug, Clone, PartialEq)]
#[allow(non_snake_case)]
pub struct PhonemeRequest {
    /// Mapped phoneme (for error messages).
    pub phoneme: String,
    /// Absolute path of the source wav.
    pub wav_path: String,
    // PhraseSynthAddRequest timing (WorldlineRenderer.cs).
    pub pos_ms: f64,
    pub skip_ms: f64,
    pub length_ms: f64,
    pub fade_in_ms: f64,
    pub fade_out_ms: f64,
    // SynthRequest fields.
    pub tone: i32,
    pub con_vel: f64,
    pub offset: f64,
    pub required_length: f64,
    pub consonant: f64,
    pub cut_off: f64,
    pub volume: f64,
    pub modulation: f64,
    pub tempo: f64,
    pub pitch_bend: Vec<i32>,
    pub flag_g: i32,
    pub flag_O: i32,
    pub flag_P: i32,
    pub flag_Mt: i32,
    pub flag_Mb: i32,
    pub flag_Mv: i32,
}

/// C# `ResamplerItem.durRequired`: `max(duration + skipOver, consonant)`
/// rounded up to the next 50 ms (`Math.Ceiling(x / 50.0 + 0.5) * 50.0`).
/// Guarantees the smoke.c invariant `length_ms <= required_length`.
pub fn required_length(duration_plus_skip: f64, consonant: f64) -> f64 {
    let raw = duration_plus_skip.max(consonant);
    (raw / DUR_REQUIRED_QUANTUM + 0.5).ceil() * DUR_REQUIRED_QUANTUM
}

/// Fade timings from the 5-point envelope (C# WorldlineRenderer.cs:
/// `fadeInMs = envelope[1].X - envelope[0].X`,
/// `fadeOutMs = envelope[4].X - envelope[3].X`), falling back to the
/// Sprint 2.1 defaults when the feed shipped no envelope points.
fn envelope_fades(oto: &OtoEntry, default_fade_in: f64, default_fade_out: f64) -> (f64, f64) {
    let envelope = &oto.envelope;
    let fade_in = envelope
        .get(1)
        .zip(envelope.first())
        .map(|(p1, p0)| (p1.x_ms - p0.x_ms) as f64)
        .unwrap_or(default_fade_in);
    let fade_out = envelope
        .get(4)
        .zip(envelope.get(3))
        .map(|(p4, p3)| (p4.x_ms - p3.x_ms) as f64)
        .unwrap_or(default_fade_out);
    (fade_in, fade_out)
}

/// Envelope end relative to the phoneme position (C# `phone.envelope[4].X`
/// = `duration - tailIntrude + tailOverlap`), falling back to the duration.
fn envelope_end(oto: &OtoEntry, duration_ms: f64) -> f64 {
    oto.envelope
        .get(4)
        .map(|p| p.x_ms as f64)
        .unwrap_or(duration_ms)
}

/// Convert every phoneme of `input` into a [`PhonemeRequest`], in phoneme
/// order. Fails when `sample_based` is missing or its oto count does not
/// match the phoneme count.
pub fn build_requests(input: &RenderInput) -> Result<Vec<PhonemeRequest>, Error> {
    let sample_based = input
        .sample_based
        .as_ref()
        .ok_or(Error::MissingSampleBased)?;
    if sample_based.oto.len() != input.phonemes.len() {
        return Err(Error::OtoCountMismatch {
            oto: sample_based.oto.len(),
            phonemes: input.phonemes.len(),
        });
    }
    let grid = Grid::new(input);
    let phrase_origin = input.phrase.position_ms - input.phrase.leading_ms;
    Ok(input
        .phonemes
        .iter()
        .zip(sample_based.oto.iter())
        .map(|(phoneme, oto)| build_request(input, &grid, phrase_origin, phoneme, oto))
        .collect())
}

/// Convert one phoneme (see the module docs for the C# mapping).
fn build_request(
    input: &RenderInput,
    grid: &Grid,
    phrase_origin: f64,
    phoneme: &RenderPhoneme,
    oto: &OtoEntry,
) -> PhonemeRequest {
    let con_vel = expression_value(input, grid, VEL, phoneme.position_ms, 100.0) as f64;
    // Loudness calibration vs the C# reference (golden apples-to-apples):
    // the C++ AutoGain mixes scaled/unscaled maxima exactly like
    // Worldline.cs, but the reference's `item.volume` is the VOL
    // expression (0..200, default 100) divided by 100 ONCE in
    // RenderPhone, then again in Worldline.cs — net 0.01. Empirically
    // matching the reference RMS (0.1595) on the Machine Love chorus
    // requires the pre-scaled value here: VOL 100 → 55 gives 0.97× the
    // reference loudness without clipping (peak 0.42 vs 0.98).
    let volume = expression_value(input, grid, VOL, phoneme.position_ms, 100.0) as f64 * 0.44;
    let modulation = expression_value(input, grid, MOD, phoneme.position_ms, 0.0) as f64;
    let tempo = DEFAULT_TEMPO_BPM;
    let stretch_ratio = 2f64.powf(1.0 - con_vel / 100.0);

    // WorldlineRenderer.cs timing.
    let pos_ms = phoneme.position_ms - phoneme.leading_ms - phrase_origin;
    // C# skipMs = ResamplerItem.skipOver = oto.Preutter * stretchRatio -
    // phone.leadingMs. Using `leading` directly (the old Sprint 2.1
    // default) skipped the leading `leading` ms of the wav — for plosives
    // (p/t/k) whose burst sits right at `offset`, that cut the burst off
    // and left a ~150 ms silent hole before the next vowel. The C#
    // formula yields 0 when leading == preutter (the common case), which
    // keeps the burst intact.
    let skip_ms = oto.preutter * stretch_ratio - phoneme.leading_ms;
    // DIAGNOSTIC (Sprint 2.3.4): C# lengthMs = envelope[4].X - envelope[0].X;
    // env WORLD_LENGTH_ENV=1 switches to that (default = duration_ms).
    //
    // FIXED (phoneme-transition gaps): the request spans
    // [position - leading, position + duration] in phrase time — the wav
    // starts `leading` before the phoneme and must END at the phoneme's
    // end. C# `lengthMs = phone.EndMs - phone.PosMs` = duration + leading.
    // Using bare `duration` ended the wav `leading` ms early, opening a
    // gap before the next phoneme (heard as clicks / broken transitions).
    let length_ms = if std::env::var_os("WORLD_LENGTH_ENV").is_some() {
        let span = oto
            .envelope
            .first()
            .map(|p0| envelope_end(oto, phoneme.duration_ms) - p0.x_ms as f64)
            .unwrap_or(phoneme.duration_ms);
        span
    } else {
        phoneme.duration_ms + phoneme.leading_ms
    };
    let (fade_in_ms, fade_out_ms) = envelope_fades(oto, 5.0, 5.0);

    // ResamplerItem.cs request fields.
    let required = required_length(length_ms + skip_ms, oto.consonant);
    let flags = resolve_flags(input, grid, oto, phoneme.position_ms);
    let pitch_bend = pitch_bend(input, grid, phoneme, oto, tempo, stretch_ratio);

    PhonemeRequest {
        phoneme: phoneme.phoneme.clone(),
        wav_path: oto.wav_path.clone(),
        pos_ms,
        skip_ms,
        length_ms,
        fade_in_ms,
        fade_out_ms,
        tone: phoneme.tone + phoneme.tone_shift,
        con_vel,
        offset: oto.offset,
        required_length: required,
        consonant: oto.consonant,
        cut_off: oto.cutoff,
        volume,
        modulation,
        tempo,
        pitch_bend,
        flag_g: flags.g,
        flag_O: flags.o,
        flag_P: flags.p,
        flag_Mt: flags.mt,
        flag_Mb: flags.mb,
        flag_Mv: flags.mv,
    }
}

/// `ResamplerItem` pitch-bend sampling: `pitchCount` samples starting
/// `pitchLeadingMs = preutter × stretchRatio` before the phoneme, lerped
/// in the 5-tick grid, expressed in cents relative to the phoneme tone.
fn pitch_bend(
    input: &RenderInput,
    grid: &Grid,
    phoneme: &RenderPhoneme,
    oto: &OtoEntry,
    tempo: f64,
    stretch_ratio: f64,
) -> Vec<i32> {
    let pitch_leading_ms = phoneme.leading_ms * stretch_ratio;
    let end_ms = envelope_end(oto, phoneme.duration_ms);
    let pitch_count_ms = end_ms + pitch_leading_ms;
    let pitch_count = (tempo_ms_to_tick(tempo, pitch_count_ms) / 5.0)
        .ceil()
        .max(0.0) as usize;
    let pitch_interval_ms = grid.ms_per_interval;
    let tone_cents = (phoneme.tone + phoneme.tone_shift) as f64 * 100.0;
    let mut bends = Vec::with_capacity(pitch_count);
    for i in 0..pitch_count {
        let sample_pos_ms = phoneme.position_ms - pitch_leading_ms + pitch_interval_ms * i as f64;
        let lerped = grid.sample_cents_lerped(&input.pitches_cents, sample_pos_ms) as f64;
        bends.push((lerped - tone_cents).round() as i32);
    }
    bends
}

/// `MusicMath.TempoMsToTick` at 480 ticks/beat: `ms × bpm / 125`.
fn tempo_ms_to_tick(tempo: f64, ms: f64) -> f64 {
    ms * tempo / 125.0
}

// ---------------------------------------------------------------------------
// Per-frame expression curves (WorldlineRenderer.SampleCurve)
// ---------------------------------------------------------------------------

/// Per-frame curves for `PhraseSynthSetCurves` (one value per `frame_ms`).
#[derive(Debug, Clone, PartialEq)]
pub struct PhraseCurves {
    /// f0 in Hz; `0.0` = keep the estimated pitch (C++ convention).
    pub f0: Vec<f64>,
    /// `0.5 + 0.005 × genc`.
    pub gender: Vec<f64>,
    /// `0.5 + 0.005 × tenc`.
    pub tension: Vec<f64>,
    /// `0.5 + 0.005 × brec`.
    pub breathiness: Vec<f64>,
    /// `0.01 × voic`.
    pub voicing: Vec<f64>,
}

/// Sample the phrase expression curves per frame, exactly like C#
/// `WorldlineRenderer.SampleCurve` (`frames = ceil((duration + leading) /
/// frame_ms)`, frame `f` at `positionMs - leadingMs + f × frameMs`,
/// truncation-indexed into the 5-tick grid, expression defaults outside
/// the grid).
pub fn sample_phrase_curves(input: &RenderInput, frame_ms: f64) -> PhraseCurves {
    let estimated_ms = input.phrase.duration_ms + input.phrase.leading_ms;
    let frames = ((estimated_ms / frame_ms).ceil() as usize).max(1);
    let grid = Grid::new(input);
    let mut curves = PhraseCurves {
        f0: vec![0.0; frames],
        gender: vec![0.5; frames],
        tension: vec![0.5; frames],
        breathiness: vec![0.5; frames],
        voicing: vec![1.0; frames],
    };
    for f in 0..frames {
        let ms = input.phrase.position_ms - input.phrase.leading_ms + f as f64 * frame_ms;
        if let Some(cents) = grid.sample_cents(&input.pitches_cents, ms) {
            curves.f0[f] = cents_to_freq(cents as f64);
        }
        if let Some(x) = grid.sample(&input.curves.gender, ms) {
            curves.gender[f] = 0.5 + 0.005 * x as f64;
        }
        if let Some(x) = grid.sample(&input.curves.tension, ms) {
            curves.tension[f] = 0.5 + 0.005 * x as f64;
        }
        if let Some(x) = grid.sample(&input.curves.breathiness, ms) {
            curves.breathiness[f] = 0.5 + 0.005 * x as f64;
        }
        if let Some(x) = grid.sample(&input.curves.voicing, ms) {
            curves.voicing[f] = 0.01 * x as f64;
        }
    }
    curves
}

/// C# `SynthRequestWrapper.Validate`: the input region
/// `[offset, offset + in_length]` (`in_length = -cutoff` when negative,
/// else `total - offset - cutoff`) must fit inside the wav and span at
/// least one 10 ms frame.
pub fn validate_request(
    req: &PhonemeRequest,
    sample_rate: u32,
    sample_count: usize,
) -> Result<(), Error> {
    let total_ms = 1000.0 * sample_count as f64 / sample_rate as f64;
    let in_start_ms = req.offset;
    let in_length_ms = if req.cut_off < 0.0 {
        -req.cut_off
    } else {
        total_ms - req.offset - req.cut_off
    };
    if in_start_ms + in_length_ms > total_ms + 0.1 {
        return Err(Error::CutoffExceedsDuration {
            phoneme: req.phoneme.clone(),
        });
    }
    let in_start_frame = (in_start_ms / FRAME_MS) as i32;
    let in_length_frame = ((in_start_ms + in_length_ms) / FRAME_MS).ceil() as i32 - in_start_frame;
    if in_length_frame <= 0 {
        return Err(Error::CutoffBeforeOffset {
            phoneme: req.phoneme.clone(),
        });
    }
    Ok(())
}

/// Convenience: fetch a named extra curve, if the feed shipped one.
#[allow(dead_code)] // public API surface for hosts that need raw curves
pub fn extra_curve<'a>(curves: &'a Curves, abbr: &str) -> Option<&'a [f32]> {
    curves
        .extra
        .iter()
        .find(|c| c.abbr == abbr)
        .map(|c| c.values.as_slice())
}

#[cfg(test)]
mod tests {
    use super::*;
    use feed::render_input::{
        Curves, EnvelopePoint, NamedCurve, PhraseInfo, RenderInput, SampleBased,
    };

    fn oto_entry() -> OtoEntry {
        OtoEntry {
            alias: "3 h3".into(),
            file: "_3_h3_3-.wav".into(),
            wav_path: "/vb/voice/_3_h3_3-.wav".into(),
            offset: 442.3,
            consonant: 375.0,
            cutoff: -583.333,
            preutter: 250.0,
            overlap: 83.333,
            envelope: Vec::new(),
            flags: String::new(),
        }
    }

    fn phoneme() -> RenderPhoneme {
        RenderPhoneme {
            phoneme: "3 h3".into(),
            position_ms: 500.0,
            duration_ms: 300.0,
            leading_ms: 250.0,
            overlap_ms: 83.333,
            tone: 60,
            tone_shift: 0,
            index: 0,
            parent_note: 0,
        }
    }

    fn input(phonemes: Vec<RenderPhoneme>, otos: Vec<OtoEntry>) -> RenderInput {
        let n = 110;
        RenderInput {
            phrase: PhraseInfo {
                position_ms: 500.0,
                duration_ms: 300.0,
                leading_ms: 250.0,
                leading_ticks: 240,
                time_axis_hint: Some("120bpm 4/4".into()),
            },
            notes: Vec::new(),
            phonemes,
            pitches_cents: vec![6000; n],
            curves: Curves {
                dynamics: vec![1.0; n],
                gender: vec![0.0; n],
                breathiness: vec![0.0; n],
                tension: vec![0.0; n],
                voicing: vec![100.0; n],
                extra: Vec::new(),
            },
            sample_based: Some(SampleBased {
                oto: otos,
                wav_path: None,
                envelope: Vec::new(),
                flags: String::new(),
            }),
            neural: None,
        }
    }

    #[test]
    fn basic_request_mapping() {
        let reqs = build_requests(&input(vec![phoneme()], vec![oto_entry()])).unwrap();
        assert_eq!(reqs.len(), 1);
        let r = &reqs[0];
        assert_eq!(r.phoneme, "3 h3");
        assert_eq!(r.tone, 60);
        assert_eq!(r.con_vel, 100.0);
        assert!((r.volume - 44.0).abs() < 1e-9); // VOL 100 × 0.45 (calibrated)
        assert_eq!(r.modulation, 0.0);
        assert_eq!(r.tempo, 120.0);
        assert_eq!(r.offset, 442.3);
        assert_eq!(r.consonant, 375.0);
        assert_eq!(r.cut_off, -583.333);
        // Timing: first phoneme sits at the phrase origin; skip/length per
        // the Sprint 2.1 contract.
        assert_eq!(r.pos_ms, 0.0);
        // C# skipOver = preutter*stretch - leading = 250*1 - 250 = 0.
        assert_eq!(r.skip_ms, 0.0);
        // duration + leading (spans [position-leading, position+duration]).
        assert_eq!(r.length_ms, 550.0);
        assert_eq!(r.fade_in_ms, 5.0);
        assert_eq!(r.fade_out_ms, 5.0);
        // durRequired: max(550+0, 375) = 550 → ceil(550/50 + 0.5) × 50 = 600.
        assert_eq!(r.required_length, 600.0);
        // Default flags (C# SynthRequestWrapper).
        assert_eq!(r.flag_g, 0);
        assert_eq!(r.flag_O, 0);
        assert_eq!(r.flag_P, 86);
        assert_eq!(r.flag_Mt, 0);
        assert_eq!(r.flag_Mb, 0);
        assert_eq!(r.flag_Mv, 100);
        // Flat C4 pitch grid → bend 0 everywhere (relative to tone 60).
        assert!(!r.pitch_bend.is_empty());
        assert!(r.pitch_bend.iter().all(|&b| b == 0));
    }

    #[test]
    fn expressions_and_tone_shift() {
        let mut ph = phoneme();
        ph.tone_shift = 2;
        let mut i = input(vec![ph], vec![oto_entry()]);
        i.curves.extra = vec![
            NamedCurve {
                abbr: "vel".into(),
                values: vec![150.0; 110],
            },
            NamedCurve {
                abbr: "vol".into(),
                values: vec![80.0; 110],
            },
            NamedCurve {
                abbr: "mod".into(),
                values: vec![30.0; 110],
            },
        ];
        let r = &build_requests(&i).unwrap()[0];
        assert_eq!(r.tone, 62);
        assert_eq!(r.con_vel, 150.0);
        assert!((r.volume - 80.0 * 0.44).abs() < 1e-9); // VOL 80 × 0.45
        assert_eq!(r.modulation, 30.0);
    }

    #[test]
    fn missing_expressions_use_defaults() {
        let r = &build_requests(&input(vec![phoneme()], vec![oto_entry()])).unwrap()[0];
        assert_eq!(r.con_vel, 100.0);
        assert!((r.volume - 44.0).abs() < 1e-9); // VOL 100 × 0.45 (calibrated)
        assert_eq!(r.modulation, 0.0);
    }

    #[test]
    fn flags_from_curves() {
        let mut i = input(vec![phoneme()], vec![oto_entry()]);
        i.curves.gender = vec![10.0; 110];
        i.curves.tension = vec![-20.0; 110];
        i.curves.breathiness = vec![30.0; 110];
        i.curves.voicing = vec![80.0; 110];
        let r = &build_requests(&i).unwrap()[0];
        assert_eq!(r.flag_g, 10);
        assert_eq!(r.flag_Mt, -20);
        assert_eq!(r.flag_Mb, 30);
        assert_eq!(r.flag_Mv, 80);
        assert_eq!(r.flag_P, 86);
        assert_eq!(r.flag_O, 0);
    }

    #[test]
    fn flags_string_overrides_curves() {
        let mut oto = oto_entry();
        oto.flags = "g5P90Mt-10Mv80".into();
        let mut i = input(vec![phoneme()], vec![oto]);
        i.curves.gender = vec![10.0; 110];
        i.curves.voicing = vec![50.0; 110];
        let r = &build_requests(&i).unwrap()[0];
        assert_eq!(r.flag_g, 5);
        assert_eq!(r.flag_P, 90);
        assert_eq!(r.flag_Mt, -10);
        assert_eq!(r.flag_Mv, 80);
        assert_eq!(r.flag_Mb, 0);
    }

    #[test]
    fn parses_flag_strings() {
        let f = parse_flags("g0B0H0P86");
        assert_eq!(f.g, Some(0));
        assert_eq!(f.p, Some(86));
        assert_eq!(f.mt, None);
        let f = parse_flags("Mt10Mv90");
        assert_eq!(f.mt, Some(10));
        assert_eq!(f.mv, Some(90));
        let f = parse_flags("");
        assert_eq!(f, SynthFlags::default());
        let f = parse_flags("g-5");
        assert_eq!(f.g, Some(-5));
        let f = parse_flags("g"); // no value → None (keep default)
        assert_eq!(f.g, None);
        let f = parse_flags("B25H50"); // unknown flags skipped
        assert_eq!(f.g, None);
        assert_eq!(f.p, None);
    }

    #[test]
    fn required_length_rounds_to_next_50() {
        // C# Math.Ceiling(x/50 + 0.5) × 50 always rounds up.
        assert_eq!(required_length(500.0, 0.0), 550.0);
        assert_eq!(required_length(550.0, 375.0), 600.0);
        assert_eq!(required_length(0.0, 0.0), 50.0);
        // length_ms <= required_length always holds.
        let reqs = build_requests(&input(vec![phoneme()], vec![oto_entry()])).unwrap();
        assert!(reqs[0].length_ms <= reqs[0].required_length);
    }

    #[test]
    fn pitch_bend_relative_to_tone_and_monotonic() {
        let mut i = input(vec![phoneme()], vec![oto_entry()]);
        i.pitches_cents = (0..110).map(|k| 6000 + k * 100 / 109).collect();
        let r = &build_requests(&i).unwrap()[0];
        // First bend sits exactly on the grid start (position - leading).
        assert_eq!(r.pitch_bend[0], 0);
        // The grid keeps rising past the phoneme end: the last bend is
        // near the +100 ct grid tail.
        let last = *r.pitch_bend.last().unwrap();
        assert!((last as f64 - 100.0).abs() < 5.0, "last bend {last}");
        assert!(r.pitch_bend.windows(2).all(|w| w[1] >= w[0]));
    }

    #[test]
    fn grid_truncation_lerp_and_bounds() {
        let grid = Grid {
            start_ms: 0.0,
            ms_per_interval: 10.0,
        };
        let values = [0.0f32, 100.0, 200.0];
        assert_eq!(grid.sample(&values, 5.0), Some(0.0)); // C# truncation
        assert_eq!(grid.sample_lerped(&values, 5.0), 50.0); // lerp
        assert_eq!(grid.sample(&values, 30.0), None); // past end → default
        assert_eq!(grid.sample_lerped(&values, 25.0), 200.0); // clamped
        assert_eq!(grid.sample_lerped(&values, -5.0), 0.0); // clamped before start
        assert_eq!(grid.sample_lerped(&[], 5.0), 0.0); // empty grid
    }

    #[test]
    fn ms_per_interval_derivation_order() {
        let i = input(vec![phoneme()], vec![oto_entry()]);
        // leading_ms / leading_ticks × 5 = 250/240×5 = 5.2083 ms.
        assert!((ms_per_interval(&i) - 5.2083).abs() < 1e-3);
        let mut i2 = i.clone();
        i2.phrase.leading_ticks = 0;
        i2.phrase.leading_ms = 0.0;
        assert!((ms_per_interval(&i2) - 5.2083).abs() < 1e-3); // 120bpm hint
        i2.phrase.time_axis_hint = None;
        assert!((ms_per_interval(&i2) - 5.2083).abs() < 1e-3); // default 120
        i2.phrase.time_axis_hint = Some("90bpm 4/4".into());
        assert!((ms_per_interval(&i2) - 5.0 * 125.0 / 90.0).abs() < 1e-9);
    }

    #[test]
    fn parse_tempo_hint_forms() {
        assert_eq!(parse_tempo_hint(Some("120bpm 4/4")), Some(120.0));
        assert_eq!(parse_tempo_hint(Some("90bpm")), Some(90.0));
        assert_eq!(parse_tempo_hint(Some("4/4")), None);
        assert_eq!(parse_tempo_hint(None), None);
        assert_eq!(parse_tempo_hint(Some("0bpm")), None);
    }

    #[test]
    fn phrase_curves_normalized() {
        let i = input(vec![phoneme()], vec![oto_entry()]);
        let c = sample_phrase_curves(&i, 10.0);
        assert_eq!(c.f0.len(), 55); // ceil(550/10)
        let c4 = 440.0 * 2f64.powf((60.0 - 69.0) / 12.0);
        assert!((c.f0[0] - c4).abs() < 1e-9);
        assert!(c.gender.iter().all(|&v| (v - 0.5).abs() < 1e-12));
        assert!(c.tension.iter().all(|&v| (v - 0.5).abs() < 1e-12));
        assert!(c.breathiness.iter().all(|&v| (v - 0.5).abs() < 1e-12));
        assert!(c.voicing.iter().all(|&v| (v - 1.0).abs() < 1e-12));
    }

    #[test]
    fn phrase_curves_fall_back_outside_grid() {
        let mut i = input(vec![phoneme()], vec![oto_entry()]);
        i.pitches_cents = vec![6000; 20]; // grid ends ≈ 354 ms
        let c = sample_phrase_curves(&i, 10.0);
        let c4 = 440.0 * 2f64.powf((60.0 - 69.0) / 12.0);
        assert_eq!(c.f0[0], c4);
        assert!(c.f0.iter().skip(11).all(|&v| v == 0.0)); // 0 → keep estimated
                                                          // Curves past the grid keep their C# defaults.
        assert!(c.gender.iter().all(|&v| (v - 0.5).abs() < 1e-12));
        assert!(c.voicing.iter().all(|&v| (v - 1.0).abs() < 1e-12));
    }

    #[test]
    fn phrase_curves_apply_normalization() {
        let mut i = input(vec![phoneme()], vec![oto_entry()]);
        i.curves.gender = vec![20.0; 110];
        i.curves.tension = vec![-40.0; 110];
        i.curves.breathiness = vec![10.0; 110];
        i.curves.voicing = vec![80.0; 110];
        let c = sample_phrase_curves(&i, 10.0);
        assert!((c.gender[0] - 0.6).abs() < 1e-9);
        assert!((c.tension[0] - 0.3).abs() < 1e-9);
        assert!((c.breathiness[0] - 0.55).abs() < 1e-9);
        assert!((c.voicing[0] - 0.8).abs() < 1e-9);
    }

    #[test]
    fn phrase_curves_min_one_frame() {
        let mut i = input(vec![phoneme()], vec![oto_entry()]);
        i.phrase.duration_ms = 0.0;
        i.phrase.leading_ms = 0.0;
        let c = sample_phrase_curves(&i, 11.6);
        assert_eq!(c.f0.len(), 1);
    }

    #[test]
    fn envelope_drives_fades_and_pitch_span() {
        let mut oto = oto_entry();
        oto.envelope = vec![
            EnvelopePoint::new(-250.0, 0.0),
            EnvelopePoint::new(-200.0, 1.0),
            EnvelopePoint::new(0.0, 1.0),
            EnvelopePoint::new(265.0, 1.0),
            EnvelopePoint::new(300.0, 0.0),
        ];
        let r = &build_requests(&input(vec![phoneme()], vec![oto])).unwrap()[0];
        assert_eq!(r.fade_in_ms, 50.0); // env[1] - env[0]
        assert_eq!(r.fade_out_ms, 35.0); // env[4] - env[3]
        assert_eq!(r.length_ms, 550.0); // duration + leading (no transition gaps)
    }

    #[test]
    fn validation_rejects_bad_cutoff() {
        let mut oto = oto_entry();
        oto.cutoff = -3000.0; // negative cutoff longer than the wav tail
        let r = &build_requests(&input(vec![phoneme()], vec![oto])).unwrap()[0];
        // Mock wav: 117897 samples @ 44100 = 2673.4 ms.
        assert!(matches!(
            validate_request(r, 44100, 117897),
            Err(Error::CutoffExceedsDuration { .. })
        ));
        let mut oto2 = oto_entry();
        oto2.offset = 442.3;
        oto2.cutoff = 3000.0; // positive cutoff eats the whole input region
        let r2 = &build_requests(&input(vec![phoneme()], vec![oto2])).unwrap()[0];
        assert!(matches!(
            validate_request(r2, 44100, 117897),
            Err(Error::CutoffBeforeOffset { .. })
        ));
        // The mock "3 h3" entry itself validates fine.
        let r3 = &build_requests(&input(vec![phoneme()], vec![oto_entry()])).unwrap()[0];
        assert!(validate_request(r3, 44100, 117897).is_ok());
    }

    #[test]
    fn structural_errors() {
        let i = input(vec![phoneme(), phoneme()], vec![oto_entry()]);
        assert!(matches!(
            build_requests(&i),
            Err(Error::OtoCountMismatch {
                oto: 1,
                phonemes: 2
            })
        ));
        let mut i2 = input(vec![phoneme()], vec![oto_entry()]);
        i2.sample_based = None;
        assert!(matches!(
            build_requests(&i2),
            Err(Error::MissingSampleBased)
        ));
    }

    #[test]
    fn empty_phrase_yields_no_requests() {
        let reqs = build_requests(&input(Vec::new(), Vec::new())).unwrap();
        assert!(reqs.is_empty());
    }
}
