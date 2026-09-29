//! synth-cli (Sprint 2.2): the first-sound milestone.
//!
//! A thin CLI over the render pipeline: `.ustx` project + voicebank →
//! phonemes → phrase groups → [`feed::RenderInput`] → `libworldline.so`
//! → mono 44.1 kHz PCM → WAV.
//!
//! * [`pipeline`] — the shared pipeline logic (also used by the
//!   integration tests).
//! * `main` — the `render` / `synth-note` commands.

pub mod chunking;
pub mod engine;
#[cfg(feature = "r2")]
pub mod moe;
#[cfg(feature = "r2")]
pub mod diffusion;
pub mod pipeline;
