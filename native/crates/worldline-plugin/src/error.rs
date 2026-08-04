//! Renderer errors.
//!
//! Mirrors the failure modes of the reference implementation:
//! `SynthRequestWrapper.Validate` (`OpenUtau.Core/Render/Worldline.cs`)
//! throws `CutOffExceedDurationError` / `CutOffBeforeOffsetError`, which
//! surface as [`Error::CutoffExceedsDuration`] / [`Error::CutoffBeforeOffset`]
//! here.

/// Errors produced by the worldline plugin.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// `dlopen` / `PhraseSynthNew` failed (see `worldline-sys::Error`).
    #[error("worldline library error: {0}")]
    Worldline(#[from] worldline_sys::Error),
    /// The voicebank wav referenced by an oto entry could not be read.
    #[error("failed to read wav {path}: {source}")]
    Wav {
        path: String,
        #[source]
        source: voicebank::WavError,
    },
    /// The renderer needs `sample_based` (oto + wav samples) and the input
    /// does not carry it.
    #[error(
        "sample_based data missing — the worldline renderer needs oto entries and wav samples"
    )]
    MissingSampleBased,
    /// `sample_based.oto` must have one entry per phoneme.
    #[error("sample_based.oto has {oto} entries but the phrase has {phonemes} phonemes")]
    OtoCountMismatch { oto: usize, phonemes: usize },
    /// `offset + in_length` exceeds the wav duration (`CutOffExceedDurationError`).
    #[error("oto error for phoneme {phoneme}: cutoff exceeds audio duration")]
    CutoffExceedsDuration { phoneme: String },
    /// The input region spans no 10 ms frame (`CutOffBeforeOffsetError`).
    #[error("oto error for phoneme {phoneme}: cutoff before offset")]
    CutoffBeforeOffset { phoneme: String },
}
