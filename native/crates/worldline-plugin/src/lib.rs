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
//!
//! # Feature `ffi`
//!
//! The dlopen'd renderer is behind the default `ffi` feature. Building
//! `--no-default-features` drops `worldline-sys` (and therefore
//! `libloading`) and leaves the pure conversion + curve sampling code,
//! which compiles to `wasm32-unknown-unknown` for the browser engine
//! (`worldline-rs` consumes exactly that).

pub mod capabilities;
pub mod convert;
pub mod error;
#[cfg(feature = "r2")]
pub mod r2;
#[cfg(feature = "ffi")]
pub mod renderer;

#[cfg(feature = "r2")]
pub mod r2_engine;

pub use capabilities::{WorldlineCapabilities, WorldlineMode};
pub use convert::{
    build_requests, expression_value, ms_per_interval, ms_per_tick_at, parse_flags,
    parse_tempo_hint, required_length, sample_phrase_curves, validate_request, Grid,
    PhonemeRequest, PhraseCurves, ResolvedFlags, SynthFlags, DEFAULT_TEMPO_BPM,
};
pub use error::Error;
#[cfg(feature = "ffi")]
pub use renderer::WorldlineRenderer;
#[cfg(feature = "ffi")]
pub use worldline_sys::WorldlineStreamStats;

/// Preferred output sample rate of the worldline library (mono 44.1 kHz).
///
/// Kept outside the `ffi` gate: capabilities and the pure-Rust phrase path
/// both need it on wasm.
pub const DEFAULT_SAMPLE_RATE: u32 = 44100;

/// Frame size of the per-frame expression curves, in ms.
///
/// Available without the FFI feature: the pure-Rust phrases uses it for
/// curve sampling density. (The C++ side stretches the curves onto its own
/// model grid, so this only affects sampling density.)
pub const DEFAULT_FRAME_MS: f64 = 10.0;

/// Free a C++-allocated buffer (WorldSynthesis/F0 output).
#[cfg(feature = "ffi")]
pub use worldline_sys::libc_free;
/// The dlopen'd WORLD analysis API handle (F0 / analysis / synthesis).
#[cfg(feature = "ffi")]
pub use worldline_sys::Analysis;
/// Raw C++ `WorldSynthesis` (f0/sp/ap + curves → PCM; allocates *y,
/// free with [`libc_free`]). Re-exported for the classic engine.
#[cfg(feature = "ffi")]
pub use worldline_sys::WorldSynthesis;

/// Open the dlopen'd WORLD analysis API (F0 / WORLD analysis) from
/// `worldline_so`. Both the R2 engine and tooling use this.
#[cfg(feature = "ffi")]
pub fn analysis(so: impl AsRef<std::path::Path>) -> Result<worldline_sys::Analysis, Error> {
    Ok(worldline_sys::Analysis::open(so)?)
}
