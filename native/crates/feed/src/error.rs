//! Feed pipeline error type.

use std::fmt;

/// Errors produced while building a [`crate::RenderInput`].
#[derive(Debug, Clone, PartialEq)]
pub enum FeedError {
    /// `part_index` does not address a voice part in `project.parts`.
    NoVoicePart(usize),
    /// The part's `track_no` is out of range.
    TrackNotFound(i32),
    /// The phonemizer produced no phonemes (empty phrase).
    EmptyPhrase,
    /// A phoneme could not be tokenized (unknown symbol for this tokenizer).
    UnknownToken(String),
    /// No oto entry exists for a phoneme while the renderer requires oto.
    OtoNotFound(String),
    /// A required expression descriptor is missing from the project.
    MissingExpression(String),
    /// Time axis was not built (call `project.after_load`/`validate` first).
    TimeAxisNotBuilt,
}

impl fmt::Display for FeedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FeedError::NoVoicePart(i) => write!(f, "no voice part at index {i}"),
            FeedError::TrackNotFound(t) => write!(f, "track {t} not found"),
            FeedError::EmptyPhrase => write!(f, "phonemizer produced no phonemes"),
            FeedError::UnknownToken(p) => write!(f, "phoneme {p:?} has no token"),
            FeedError::OtoNotFound(p) => write!(f, "no oto entry for phoneme {p:?}"),
            FeedError::MissingExpression(a) => write!(f, "expression {a:?} not registered"),
            FeedError::TimeAxisNotBuilt => {
                write!(
                    f,
                    "project time axis is empty; run after_load/validate first"
                )
            }
        }
    }
}

impl std::error::Error for FeedError {}
