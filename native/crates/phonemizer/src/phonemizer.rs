//! The [`Phonemizer`] trait and output phoneme construction.
//!
//! Mirrors `OpenUtau.Core/Api/Phonemizer.cs`: a phonemizer converts a
//! consecutive sequence of notes into a flat phoneme list. Positions are
//! in ticks, **relative to the parent note** (OpenUtau plugin semantics);
//! [`crate::TimingEngine`] converts them to part-relative ticks and
//! milliseconds afterwards.

use domain::{UNote, UPhoneme};
use voicebank::Voicebank;

/// Output phoneme type of every phonemizer: the domain `UPhoneme`.
///
/// The phonemizer fills the tick-based fields (`phoneme`, `raw_phoneme`,
/// `position`, `raw_position`, `duration`, `index`, `tone`, `parent`);
/// millisecond fields (`position_ms`, `duration_ms`, `leading_ms`,
/// `overlap_ms`, oto fields) are computed by [`crate::TimingEngine`].
pub type Phoneme = UPhoneme;

/// A phonemizer plugin: turns notes into phonemes.
///
/// * `notes` — a consecutive sequence of notes. `parent` on every output
///   phoneme is the index of its note within this slice.
/// * `singer` — the voicebank used for oto alias resolution (may be
///   `None`, in which case phonemes are emitted unresolved).
///
/// `phoneme` holds the *resolved* alias when the implementation matched
/// the oto table (e.g. `"a き"` for the head of a note following `か`);
/// `raw_phoneme` always holds the pre-resolution symbol (e.g. `"き"`).
pub trait Phonemizer {
    fn process(&self, notes: &[UNote], singer: Option<&Voicebank>) -> Vec<UPhoneme>;
}

/// Build a phoneme with a note-relative tick position/duration.
///
/// `phoneme` is both the raw and the resolved symbol (implementations
/// that resolve aliases update `phoneme` afterwards).
pub fn make_phoneme(
    phoneme: impl Into<String>,
    position: i32,
    duration: i32,
    index: i32,
    tone: i32,
    parent: usize,
) -> UPhoneme {
    let phoneme = phoneme.into();
    UPhoneme {
        raw_phoneme: phoneme.clone(),
        phoneme,
        position,
        raw_position: position,
        duration,
        index,
        tone,
        parent: Some(parent),
        ..Default::default()
    }
}

/// Split `note.duration` evenly across `count` phonemes.
///
/// Returns the `count` phoneme durations (integer ticks; the last phoneme
/// receives the remainder, mirroring how OpenUtau plugins divide note
/// durations).
pub(crate) fn split_duration(total: i32, count: usize) -> Vec<i32> {
    let count = count.max(1) as i32;
    let base = total / count;
    let rem = total % count;
    (0..count)
        .map(|i| if i < rem { base + 1 } else { base })
        .collect()
}

/// Cumulative positions from durations: `[0, d0, d0+d1, ...]`.
pub(crate) fn positions_from_durations(durations: &[i32]) -> Vec<i32> {
    let mut positions = Vec::with_capacity(durations.len());
    let mut acc = 0;
    for d in durations {
        positions.push(acc);
        acc += d;
    }
    positions
}

/// Whether `lyric` denotes a rest note (no phoneme content).
pub(crate) fn is_rest(lyric: &str) -> bool {
    matches!(lyric, "" | "-" | "R" | "…" | "･･･" | "rest")
}
