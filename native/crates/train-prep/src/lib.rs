//! train-prep — Phase 0 POC: build a distillation training set from a
//! classic voicebank using the R2 engine as the teacher.
//!
//! For each phrase spec (one note per oto alias, plus a few connected
//! multi-note phrases), we render through the R2 path (WORLD features →
//! mel.onnx → pc-nsf-hifigan) while dumping the teacher signal
//! (score-overridden f0 + mel, frame-aligned) via the env-gated
//! `WORLD_DUMP_MEL_FILE` hook in `worldline-plugin::r2_engine`.
//!
//! Output: `train-set.jsonl` — one line per phrase:
//!   { alias, tone, duration_ms, phonemes:[{phoneme,position_ms,duration_ms}],
//!     frames, f0_hz:[...], mel:[[128]×frames] }
//!
//! POC scope (2026-08-09): single-alias phrases + N-note connected
//! phrases from the bank's own alias set. Additive-only: original bank
//! files are never touched.

pub mod extract;
pub mod filter;
pub mod phrase_gen;
pub mod project_extract;

use serde::Serialize;

/// One phrase to render: 1..n notes, each note's lyric = its alias
/// given as a phonetic hint (so the phonemizer emits it verbatim).
#[derive(Debug, Clone, Serialize)]
pub struct PhraseSpec {
    pub aliases: Vec<String>,
    pub tone: i32,
    pub duration_ms: f64,
}

impl PhraseSpec {
    pub fn single(alias: &str, tone: i32, duration_ms: f64) -> Self {
        PhraseSpec {
            aliases: vec![alias.to_string()],
            tone,
            duration_ms,
        }
    }
}

/// Per-phoneme timing as the student needs it (frame alignment is
/// derived from position_ms/duration_ms on the 11.61 ms grid).
#[derive(Debug, Clone, Serialize)]
pub struct PhonemeTiming {
    pub phoneme: String,
    pub position_ms: f64,
    pub duration_ms: f64,
}

/// One training example: teacher (f0/mel) + the score-side inputs
/// (phonemes/timing) the student will learn from.
#[derive(Debug, Clone, Serialize)]
pub struct TrainEntry {
    pub aliases: Vec<String>,
    pub tone: i32,
    pub duration_ms: f64,
    pub phonemes: Vec<PhonemeTiming>,
    pub frames: usize,
    /// Per-frame f0 (Hz, 0 = unvoiced) — score-overridden (the curve
    /// the teacher actually used).
    pub f0_hz: Vec<f32>,
    /// Per-frame mel spectrogram (frames × 128 bins).
    pub mel: Vec<Vec<f32>>,
}
