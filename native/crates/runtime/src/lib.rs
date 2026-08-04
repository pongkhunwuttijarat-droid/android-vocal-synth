//! `runtime` — orchestration layer (Sprint 1.5): jobs, chunking, caching,
//! mixing.
//!
//! This crate is **generic over its render-input type** (`T`), so it
//! compiles and tests standalone while the `feed` crate is still being
//! built. When `feed` exposes its `RenderInput` type, enable the
//! `feed-integration` feature to get the ready-made alias:
//!
//! ```toml
//! runtime = { path = "crates/runtime", features = ["feed-integration"] }
//! ```
//!
//! Pipeline shape (see `docs/architecture/runtime.md`):
//!
//! ```text
//! phones: Vec<T>
//!   -> Chunker::split        -> Vec<RenderChunk<T>>   (overlap + XXH64 hash)
//!   -> Scheduler             -> jobs, cancel, progress, retry
//!   -> RenderCache           -> get_or_compute(hash)  (LRU bytes + disk)
//!   -> Mixer::mix            -> FinalAudio            (align + sum + FX)
//! ```
//!
//! Components:
//!
//! * [`scheduler`] — FIFO job queue with cancellation, per-job progress
//!   callbacks and bounded retry.
//! * [`chunker`] — batch splitting with phrase-boundary overlap and
//!   deterministic XXH64 chunk hashes.
//! * [`cache`] — byte-capped LRU over audio chunks with disk persistence
//!   through the `storage` crate (`res-{hash}.bin`, OpenUtau-style).
//! * [`mixer`] — time alignment (`position_ms - leading_ms`), per-track
//!   volume/pan summing, dynamics envelopes and fades, mono 44.1 kHz.
//! * [`hash`] — XXH64 wrapper (streaming + `io::Write` adapter).

pub mod cache;
pub mod chunker;
pub mod hash;
pub mod mixer;
pub mod scheduler;

pub use cache::RenderCache;
pub use chunker::{Chunker, HashKey, RenderChunk};
pub use mixer::{AudioChunk, FinalAudio, MixInput, TrackSpec, SAMPLE_RATE};
pub use scheduler::{Job, JobId, JobStatus, Scheduler};

/// Ready-made aliases once `feed` lands. `feed` is an optional dependency
/// (off by default) so this crate never blocks on the parallel feed work.
#[cfg(feature = "feed-integration")]
pub mod feed_compat {
    /// The render input type produced by the feed crate.
    pub use feed::RenderInput as FeedRenderInput;
    /// A scheduler job over feed inputs.
    pub type FeedJob = crate::Job<FeedRenderInput>;
    /// Chunks of feed inputs.
    pub type FeedChunk = crate::RenderChunk<FeedRenderInput>;
}

/// `HashKey` implementations for feed's render input types (the chunker and
/// render cache key chunks by the full input). Enabled with the same
/// `feed-integration` feature so the runtime compiles standalone.
#[cfg(feature = "feed-integration")]
pub mod feed_hash;
