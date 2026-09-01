//! DiffSinger renderer (MS3.3) — Rust port of the C# OpenUtau DiffSinger
//! plugin (`ref-openutau-clean/core/OpenUtau.Core/DiffSinger/`).
//!
//! DiffSinger = diffusion-based acoustic model: score → phoneme tokens +
//! durations + f0 → acoustic.onnx → mel → vocoder.onnx → PCM.
//! This crate is model-side only (config/table/tensors); the renderer
//! wiring lives in the engine adapters (synth-cli) like r1/r2/classic.
//!
//! Status (plan: `.hermes/plans/2026-08-09-diffsinger.md`):
//! - M1 config + phoneme table — done
//! - M2 tensor build (tokens/durations/f0) — next

pub mod config;
pub mod dict;
pub mod dsdur;
pub mod model;
pub mod phonemes;
pub mod tensors;
pub mod utils;

pub use config::{DsConfig, DsVocoderConfig};
pub use dict::LyricDict;
pub use dsdur::DsDur;
pub use model::DsModel;
