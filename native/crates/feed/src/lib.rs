//! Feed pipeline (Sprint 1.4): project + voicebank → unified [`RenderInput`].
//!
//! The feed builds the superset [`RenderInput`] that every renderer plugin
//! consumes. Plugins read only the fields their capability declares:
//! sample-based renderers (worldline/classic) read `sample_based`; neural
//! renderers (DiffSinger/Vogen) read `neural`. Curves are stored as sparse
//! points+equation in the project and sampled here to dense per-5-tick
//! arrays, then per-frame by the renderer (see docs/architecture/feed-data-flow.md).

pub mod curve;
pub mod envelope;
pub mod error;
pub mod f0;
pub mod music_math;
pub mod oto;
pub mod pitch;
pub mod render_input;

pub use curve::{sample_curve, sample_curve_identity, sample_per_frame};
pub use envelope::EnvelopeBuilder;
pub use error::FeedError;
pub use f0::{compute_f0, F0Result};
pub use music_math::{
    cents_to_freq, decibel_to_linear, freq_to_tone, interpolate_shape, linear, shift_freq,
    sin_easing_in, sin_easing_in_out, sin_easing_out, tone_to_freq,
};
pub use oto::OtoMapper;
pub use pitch::PitchComputer;
pub use render_input::{
    Curves, EnvelopePoint, NamedCurve, NeuralInput, OtoEntry, PhraseInfo, RenderInput, RenderNote,
    RenderPhoneme, SampleBased,
};
