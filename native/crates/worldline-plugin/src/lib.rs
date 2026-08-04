//! Worldline renderer plugin (Sprint 2.1).
//!
//! The bridge from the feed's [`RenderInput`] to the worldline synthesis
//! library (`libworldline.so`): capabilities declaration, the
//! `RenderInput` → `SynthRequest` conversion (per
//! `OpenUtau.Core/Classic/WorldlineRenderer.cs` +
//! `ResamplerItem.cs`) and the [`WorldlineRenderer`] that drives the
//! C++ `PhraseSynth` to produce mono PCM.
//!
//! Pipeline (`docs/architecture/feed-data-flow.md`):
//!
//! ```text
//! RenderInput
//!   ├─ build_requests()  → PhonemeRequest[]  (pure, unit-tested)
//!   ├─ sample_phrase_curves() → PhraseCurves (pure, unit-tested)
//!   ├─ read wavs (voicebank::read_wav) + validate_request()
//!   ├─ PhraseSynth::AddRequest per phoneme (pos/skip/length/fades)
//!   ├─ PhraseSynth::SetCurves (f0/gender/tension/breathiness/voicing)
//!   └─ PhraseSynth::Synth → Vec<f32> (mono 44.1 kHz)
//! ```
//!
//! The `.so` is dlopen'd at runtime (`worldline-sys`) and kept loaded for
//! the renderer's lifetime. Integration tests against the real `.so` are
//! ignored by default; run them with
//! `WORLDLINE_SO=.../libworldline.so cargo test -- --ignored`.

pub mod capabilities;
pub mod convert;
pub mod error;
pub mod renderer;

pub use capabilities::{WorldlineCapabilities, WorldlineMode};
pub use convert::{
    build_requests, expression_value, ms_per_interval, ms_per_tick_at, parse_flags,
    parse_tempo_hint, required_length, sample_phrase_curves, validate_request, Grid,
    PhonemeRequest, PhraseCurves, ResolvedFlags, SynthFlags, DEFAULT_TEMPO_BPM,
};
pub use error::Error;
pub use renderer::{WorldlineRenderer, DEFAULT_FRAME_MS, DEFAULT_SAMPLE_RATE};
