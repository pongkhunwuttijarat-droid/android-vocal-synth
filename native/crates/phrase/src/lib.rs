//! Phrase pipeline (Sprint 2.2): phoneme → phrase grouping + `RenderInput`
//! building.
//!
//! * [`grouping`] — `RenderPhrase.FromPart` semantics: split a voice part's
//!   derived phonemes into phrases at every gap (`phonemes[i-1].End !=
//!   phonemes[i].position`).
//! * [`builder`] — [`builder::PhraseBuilder`]: one phrase group + part +
//!   project + time axis → [`feed::RenderInput`], wiring the feed crate's
//!   [`feed::PitchComputer`], [`feed::sample_curve`],
//!   [`feed::EnvelopeBuilder`] and [`feed::OtoMapper`] exactly like the
//!   reference `RenderPhrase` constructor.

pub mod builder;
pub mod grouping;

pub use builder::{PhraseBuilder, PhraseError};
pub use grouping::{PhraseGroup, PhraseGrouping};
