//! worldline-rs: pure-Rust port of the Worldline renderer.
//!
//! `crates/world-rs` ports the WORLD DSP kernel (CheapTrick / D4C /
//! Synthesis). This crate ports the *orchestration* around it — the C++
//! `worldline` plugin's `Model`, `PhraseSynth`, `classic/timing` and
//! `model/effects` — so a renderer can run with no dlopen'd
//! `libworldline.so` at all.
//!
//! Consumed by the `r1-rs` engine (`WORLDLINE-R`, Rust kernel), which is
//! input-compatible with the C++ `.so` `r1` engine: both take the same
//! `PhonemeRequest` / `PhraseCurves` from `worldline-plugin::convert`.

pub mod effects;
pub mod f0;
pub mod model;
pub mod phrase;
pub mod prepared_cache;
pub mod pyin;
pub mod resampler;
pub mod source;
pub mod spline;
pub mod timing;

pub use model::Model;
pub use phrase::{PhraseSynthRs, StreamStatsRs};
pub use prepared_cache::PreparedModelCache;
pub use resampler::ResamplerRs;
pub use source::{FileSource, MemorySource, ResolvedSample, SampleSource};
pub use timing::ModelTiming;

/// The C++ `FrameMs()` compiled default for the v2 artifact
/// (`WORLDLINE_FRAME_MS_DEFAULT` = `512.0 * 1000.0 / 44100.0`).
///
/// This is the grid the reference uses for the model frames and for the
/// p0..p4 timing — NOT the rounded `11.6` the Rust plugin uses for the
/// curve sampling density. The two differ by ~0.09%, which is enough to
/// flip a `round()` at a frame boundary and change the rendered length.
pub const V2_MODEL_FRAME_MS: f64 = 512.0 * 1000.0 / 44100.0;

/// The C++ `FrameMs()` compiled default for the v1 artifact (10 ms).
pub const V1_MODEL_FRAME_MS: f64 = 10.0;

/// Frame size the `.so` uses for the model grid: `WORLD_FRAME_MS` when set
/// (the C++ `FrameMs()` prefers it), else the v2 compiled default.
pub fn model_frame_ms() -> f64 {
    std::env::var_os("WORLD_FRAME_MS")
        .and_then(|v| v.to_str().and_then(|s| s.parse::<f64>().ok()))
        .filter(|ms| *ms > 0.0)
        .unwrap_or(V2_MODEL_FRAME_MS)
}

/// Render one phrase to mono f32 PCM at 44.1 kHz.
///
/// Mirrors `worldline_plugin::WorldlineRenderer::render_phrase`: the same
/// request conversion and curve sampling, with the Rust kernel instead of
/// the `.so`.
pub fn render_phrase(
    input: &feed::render_input::RenderInput,
    curve_frame_ms: f64,
) -> Result<Vec<f32>, String> {
    render_phrase_with_frames(input, curve_frame_ms, model_frame_ms())
}

/// `render_phrase` with the model grid made explicit.
///
/// `curve_frame_ms` is the density used to sample the expression curves —
/// the reference passes the plugin's `DEFAULT_FRAME_MS` (11.6) — while
/// `model_frame_ms` is the C++ `FrameMs()` grid that drives the model
/// frames, the trim and the p0..p4 timing.
pub fn render_phrase_with_frames(
    input: &feed::render_input::RenderInput,
    curve_frame_ms: f64,
    model_frame_ms: f64,
) -> Result<Vec<f32>, String> {
    render_phrase_with_source(input, curve_frame_ms, model_frame_ms, &FileSource)
}

/// Render one phrase resolving voicebank samples through `source`.
///
/// This is the filesystem-free entry point: a browser host passes a
/// [`MemorySource`] (samples decoded from OPFS) and never touches
/// `std::fs`. Native callers use [`render_phrase_with_frames`], which is
/// this function with [`FileSource`].
pub fn render_phrase_with_source<S: SampleSource + ?Sized>(
    input: &feed::render_input::RenderInput,
    curve_frame_ms: f64,
    model_frame_ms: f64,
    source: &S,
) -> Result<Vec<f32>, String> {
    render_phrase_with_source_and_cache(input, curve_frame_ms, model_frame_ms, source, None)
}

/// Render one phrase with an optional bounded cache of exact per-phoneme
/// WORLD analysis models. The cache affects work performed, never timing,
/// curves, random state, or the final synthesis path.
pub fn render_phrase_with_source_and_cache<S: SampleSource + ?Sized>(
    input: &feed::render_input::RenderInput,
    curve_frame_ms: f64,
    model_frame_ms: f64,
    source: &S,
    mut cache: Option<&mut PreparedModelCache>,
) -> Result<Vec<f32>, String> {
    let requests =
        worldline_plugin::build_requests(input).map_err(|e| format!("build_requests: {e}"))?;
    if requests.is_empty() {
        return Ok(Vec::new());
    }
    let curves = worldline_plugin::sample_phrase_curves(input, curve_frame_ms);
    let mut synth = PhraseSynthRs::new(model_frame_ms);
    for request in &requests {
        let resolved = source.resolve(&request.wav_path)?;
        worldline_plugin::validate_request(
            request,
            resolved.wav.sample_rate,
            resolved.wav.samples.len(),
        )
        .map_err(|e| format!("validate {}: {e}", request.phoneme))?;
        let mono: Vec<f64> = resolved
            .wav
            .to_mono()
            .into_iter()
            .map(|s| s as f64)
            .collect();
        synth.add_request_from_samples_cached(
            request,
            mono,
            resolved.wav.sample_rate as i32,
            resolved.frq.as_deref(),
            cache.as_deref_mut(),
        )?;
    }
    synth.set_curves(&curves);
    synth.synth()
}

/// Shared-mutex variant of [`render_phrase_with_source_and_cache`] so concurrent
/// callers share prepared models without holding the cache lock across WORLD analysis.
pub fn render_phrase_with_source_and_shared_cache<S: SampleSource + ?Sized>(
    input: &feed::render_input::RenderInput,
    curve_frame_ms: f64,
    model_frame_ms: f64,
    source: &S,
    cache: Option<&std::sync::Mutex<PreparedModelCache>>,
) -> Result<Vec<f32>, String> {
    let requests =
        worldline_plugin::build_requests(input).map_err(|e| format!("build_requests: {e}"))?;
    if requests.is_empty() {
        return Ok(Vec::new());
    }
    let curves = worldline_plugin::sample_phrase_curves(input, curve_frame_ms);
    let mut synth = PhraseSynthRs::new(model_frame_ms);
    for request in &requests {
        let resolved = source.resolve(&request.wav_path)?;
        worldline_plugin::validate_request(
            request,
            resolved.wav.sample_rate,
            resolved.wav.samples.len(),
        )
        .map_err(|e| format!("validate {}: {e}", request.phoneme))?;
        let mono: Vec<f64> = resolved
            .wav
            .to_mono()
            .into_iter()
            .map(|s| s as f64)
            .collect();
        synth.add_request_from_samples_shared_cache(
            request,
            mono,
            resolved.wav.sample_rate as i32,
            resolved.frq.as_deref(),
            cache,
        )?;
    }
    synth.set_curves(&curves);
    synth.synth()
}

/// Stream one phrase to mono f32 PCM at 44.1 kHz via incremental phoneme
/// preparation and stateful WORLD synthesis.
pub fn render_phrase_stream<F>(
    input: &feed::render_input::RenderInput,
    curve_frame_ms: f64,
    block_samples: usize,
    callback: F,
) -> Result<(usize, StreamStatsRs), String>
where
    F: FnMut(&[f32]) -> Result<(), String>,
{
    render_phrase_stream_with_frames(input, curve_frame_ms, model_frame_ms(), block_samples, callback)
}

/// [`render_phrase_stream`] with an explicit synthesis frame grid.
pub fn render_phrase_stream_with_frames<F>(
    input: &feed::render_input::RenderInput,
    curve_frame_ms: f64,
    model_frame_ms: f64,
    block_samples: usize,
    callback: F,
) -> Result<(usize, StreamStatsRs), String>
where
    F: FnMut(&[f32]) -> Result<(), String>,
{
    render_phrase_stream_with_source(input, curve_frame_ms, model_frame_ms, &FileSource, block_samples, callback)
}

/// Stream one phrase resolving voicebank samples through `source`.
pub fn render_phrase_stream_with_source<S, F>(
    input: &feed::render_input::RenderInput,
    curve_frame_ms: f64,
    model_frame_ms: f64,
    source: &S,
    block_samples: usize,
    callback: F,
) -> Result<(usize, StreamStatsRs), String>
where
    S: SampleSource + Sync + ?Sized,
    F: FnMut(&[f32]) -> Result<(), String>,
{
    render_phrase_stream_with_source_and_cache(
        input,
        curve_frame_ms,
        model_frame_ms,
        source,
        None,
        block_samples,
        callback,
    )
}

/// Stream one phrase with an optional thread-safe cache of prepared WORLD
/// analysis models.
pub fn render_phrase_stream_with_source_and_cache<S, F>(
    input: &feed::render_input::RenderInput,
    curve_frame_ms: f64,
    model_frame_ms: f64,
    source: &S,
    cache: Option<&std::sync::Mutex<PreparedModelCache>>,
    block_samples: usize,
    callback: F,
) -> Result<(usize, StreamStatsRs), String>
where
    S: SampleSource + Sync + ?Sized,
    F: FnMut(&[f32]) -> Result<(), String>,
{
    phrase::render_phrase_stream_with_source_and_cache(
        input,
        curve_frame_ms,
        model_frame_ms,
        source,
        cache,
        block_samples,
        callback,
    )
}
