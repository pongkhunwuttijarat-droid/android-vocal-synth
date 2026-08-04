//! synth-server (Sprint 2.4.1): a LAN HTTP API over the synth-cli render
//! pipeline.
//!
//! Exposes the same engine the CLI drives — `.ustx` project + voicebank →
//! phonemes → [`feed::RenderInput`] → `libworldline.so` → mono 44.1 kHz
//! PCM → WAV — as an HTTP service (for testing the engine from a tablet
//! or another machine on the LAN).
//!
//! ```text
//! GET  /health        → {status, version, so_loaded}
//! GET  /capabilities  → renderer capabilities (WorldlineCapabilities)
//! GET  /voicebanks    → {voicebanks: [{name, dir, aliases_count, wav_count, samples_rate}]}
//! POST /synth-note    → {voicebank, phoneme, tone, duration_ms?, out?} → audio/wav | {error}
//! POST /render        → {project, voicebank, track?}                  → audio/wav | {error}
//! GET  /stats         → {renders_count, total_ms, cache_hits}
//! ```
//!
//! Design notes:
//!
//! * The render pipeline is **reused from `synth-cli`** (`pipeline::`),
//!   never reimplemented.
//! * `WorldlineRenderer` is `!Send + !Sync` (the C++ `PhraseSynth` must
//!   not cross threads), so it lives on a dedicated worker thread
//!   ([`render_service`]); handlers send jobs over an mpsc channel and
//!   await oneshot replies. Renders are serialized, which the underlying
//!   library requires anyway.
//! * Voicebanks are scanned once at startup from the configured
//!   `--voicebanks` root (each subdirectory = one voicebank, or the root
//!   itself when it is a single bank) and listed by `/voicebanks`.
//! * POST bodies are parsed leniently (any content type), so plain
//!   `curl -d '{...}'` works without a `Content-Type` header.
//! * Successful renders return raw WAV bytes with `Content-Type:
//!   audio/wav`, so `curl -o out.wav` works; failures return JSON
//!   `{"error": "..."}` with 400/404/500 status codes.

pub mod handlers;
pub mod render_service;
pub mod server;
pub mod state;
pub mod stats;
pub mod voicebanks;
