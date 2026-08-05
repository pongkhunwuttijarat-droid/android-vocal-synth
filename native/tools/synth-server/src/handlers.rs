//! HTTP handlers for the six API endpoints.
//!
//! `POST /synth-note` and `POST /render` reuse the `synth-cli` pipeline
//! verbatim ([`pipeline::synth_note`] / [`pipeline::render_project`]);
//! this module only maps HTTP ⇄ pipeline. Successful renders return raw
//! WAV bytes (`Content-Type: audio/wav`); failures return JSON
//! `{"error": "..."}` with status 400 (bad request / unmapped phoneme),
//! 404 (unknown voicebank / missing project) or 500 (engine failure).

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use axum::body::Bytes;
use axum::extract::{FromRequest, Request, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use synth_cli::pipeline::{self, SAMPLE_RATE};
use voicebank::Voicebank;
use wavwriter::write_wav_16;

use crate::state::AppState;
use crate::stats::StatsSnapshot;

/// Upper bound for `duration_ms`, to keep a LAN client from requesting
/// absurd renders.
const MAX_DURATION_MS: f64 = 60_000.0;

fn default_duration_ms() -> f64 {
    500.0
}

/// Lenient JSON body extractor: parses the body as JSON regardless of the
/// `Content-Type` header, so plain `curl -d '{...}'` works without
/// `-H 'Content-Type: application/json'`.
pub struct JsonBody<T>(pub T);

impl<S, T> FromRequest<S> for JsonBody<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = (StatusCode, Json<Value>);

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let bytes = Bytes::from_request(req, state)
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": format!("invalid body: {e}") }))))?;
        let value = serde_json::from_slice::<T>(&bytes).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("invalid JSON body: {e}") })),
            )
        })?;
        Ok(JsonBody(value))
    }
}

/// `POST /synth-note` request body.
#[derive(Debug, Deserialize)]
pub struct SynthNoteRequest {
    /// Voicebank name as listed by `GET /voicebanks` (or its dir id, or
    /// an absolute library path).
    pub voicebank: String,
    /// Oto alias to synthesize (e.g. `"A"` for the mock bank).
    pub phoneme: String,
    /// MIDI note number (60 = C4).
    pub tone: i32,
    /// Requested note duration in ms.
    #[serde(default = "default_duration_ms")]
    pub duration_ms: f64,
    /// Accepted for CLI parity; the wav is always returned in the
    /// response body (the server never writes files).
    #[serde(default)]
    pub out: Option<String>,
}

/// `POST /render` request body.
#[derive(Debug, Deserialize)]
pub struct RenderRequest {
    /// Absolute path to the `.ustx` project file.
    pub project: String,
    /// Voicebank name as listed by `GET /voicebanks` (or its dir id, or
    /// an absolute library path).
    pub voicebank: String,
    /// Track index to render (default 0).
    #[serde(default)]
    pub track: Option<i32>,
}

/// `GET /health` — liveness plus a version / renderer summary.
pub async fn health(State(state): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({
        "status": "ok",
        "version": AppState::version(),
        "so_loaded": state.renderer.is_some(),
    }))
}

/// `GET /capabilities` — static worldline renderer capabilities.
pub async fn capabilities(State(_state): State<Arc<AppState>>) -> Json<Value> {
    let caps = worldline_plugin::WorldlineCapabilities::get();
    Json(json!({
        "modes": caps.modes.iter().map(|m| m.as_str()).collect::<Vec<_>>(),
        "needs_wav_samples": caps.needs_wav_samples,
        "needs_oto": caps.needs_oto,
        "needs_frq": caps.needs_frq,
        "expressions": caps.expressions,
        "sample_rate": caps.sample_rate,
        "channels": caps.channels,
    }))
}

/// `GET /voicebanks` — the banks discovered under the configured root.
pub async fn voicebanks(State(state): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({
        "voicebanks": state.entries.iter().map(|e| &e.info).collect::<Vec<_>>(),
    }))
}

/// `GET /stats` — cumulative render counters.
pub async fn stats(State(state): State<Arc<AppState>>) -> Json<StatsSnapshot> {
    Json(state.stats.snapshot())
}

/// `POST /synth-note` — render one phoneme/note and return the wav.
pub async fn synth_note(
    State(state): State<Arc<AppState>>,
    JsonBody(req): JsonBody<SynthNoteRequest>,
) -> Response {
    let voicebank = match resolve_voicebank(&state, &req.voicebank) {
        Ok(vb) => vb,
        Err(e) => return error_response(e.status(), e.message()),
    };
    if let Err(e) = validate_synth_note(&req) {
        return error_response(StatusCode::BAD_REQUEST, e);
    }
    // Oto gate: the phoneme must map to an oto alias in this voicebank
    // (cheap, no renderer needed — mirrors the CLI's error path).
    if let Err(e) = pipeline::synth_note_validate(&voicebank, &req.phoneme, req.tone, req.duration_ms)
    {
        return error_response(StatusCode::BAD_REQUEST, e);
    }
    let Some(renderer) = state.renderer.as_ref() else {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "renderer not loaded (start the server with --so <path>)".into(),
        );
    };

    let started = Instant::now();
    match renderer
        .synth_note(voicebank, req.phoneme.clone(), req.tone, req.duration_ms)
        .await
    {
        Ok(samples) if !samples.is_empty() => {
            state
                .stats
                .record_render(started.elapsed().as_millis() as u64);
            wav_response(write_wav_16(&samples, SAMPLE_RATE))
        }
        Ok(_) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "renderer produced no samples".into(),
        ),
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("render failed: {e}"),
        ),
    }
}

/// `POST /render` — render a `.ustx` project track and return the wav.
pub async fn render(
    State(state): State<Arc<AppState>>,
    JsonBody(req): JsonBody<RenderRequest>,
) -> Response {
    let project_path = Path::new(&req.project);
    if !project_path.is_file() {
        return error_response(
            StatusCode::NOT_FOUND,
            format!("project {} not found", req.project),
        );
    }
    let voicebank = match resolve_voicebank(&state, &req.voicebank) {
        Ok(vb) => vb,
        Err(e) => return error_response(e.status(), e.message()),
    };
    let track = req.track.unwrap_or(0);
    if track < 0 {
        return error_response(
            StatusCode::BAD_REQUEST,
            format!("track {track} must be >= 0"),
        );
    }
    let Some(renderer) = state.renderer.as_ref() else {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "renderer not loaded (start the server with --so <path>)".into(),
        );
    };

    let started = Instant::now();
    match renderer
        .render_project(project_path.to_path_buf(), voicebank, track)
        .await
    {
        Ok(report) if !report.samples.is_empty() => {
            state
                .stats
                .record_render(started.elapsed().as_millis() as u64);
            wav_response(write_wav_16(&report.samples, SAMPLE_RATE))
        }
        Ok(report) => {
            let total = report.phrases_rendered + report.skipped.len();
            let reasons = if report.skipped.is_empty() {
                "unknown".to_string()
            } else {
                report.skipped.join("; ")
            };
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!(
                    "no audio produced ({}/{} phrases rendered): {reasons}",
                    report.phrases_rendered, total
                ),
            )
        }
        Err(e) => error_response(render_error_status(&e), e),
    }
}

/// `POST /cancel` — hard Stop: abort the in-flight render. The worker
/// sets the shared cancel flag; the pipeline bails between chunks and
/// the render reply comes back as "render cancelled" (500).
pub async fn cancel(State(state): State<Arc<AppState>>) -> Response {
    let Some(renderer) = state.renderer.as_ref() else {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "renderer not loaded (start the server with --so <path>)".into(),
        );
    };
    match renderer.cancel() {
        Ok(()) => Json(json!({ "ok": true, "cancelled": true })).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

/// `POST /mixer-params` — hot-swap the mixer FX params (fader/EQ/comp
/// changes from the app). Body: the params JSON string, e.g.
/// `{"gain":0.8,"low_gain":3}`.
pub async fn mixer_params(
    State(state): State<Arc<AppState>>,
    JsonBody(params): JsonBody<String>,
) -> Response {
    let Some(renderer) = state.renderer.as_ref() else {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "renderer not loaded (start the server with --so <path>)".into(),
        );
    };
    match renderer.set_mixer_params(params) {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

/// `POST /post-fx` — post-synth mixer FX: apply the mixer chain to an
/// ALREADY rendered wav (no re-synthesis — the fast fader/EQ path).
/// Request body: raw wav bytes; header `x-mixer-params`: the params JSON
/// (e.g. `{"gain":0.8,"low_gain":3}`). Response: the FX'd wav.
pub async fn post_fx(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Response {
    let Some(renderer) = state.renderer.as_ref() else {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "renderer not loaded (start the server with --so <path>)".into(),
        );
    };
    let params = match headers.get("x-mixer-params") {
        Some(v) => v.to_str().unwrap_or("{}").to_string(),
        None => "{}".to_string(),
    };
    // Decode the raw wav → f32 samples.
    let wav = match voicebank::parse_wav(&body) {
        Ok(w) => w,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                format!("post-fx: not a wav body: {e}"),
            );
        }
    };
    let samples = wav.samples;
    if samples.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "post-fx: empty wav".into(),
        );
    }
    let started = Instant::now();
    match renderer.post_fx(params, samples).await {
        Ok(out) if !out.is_empty() => {
            state
                .stats
                .record_render(started.elapsed().as_millis() as u64);
            wav_response(write_wav_16(&out, SAMPLE_RATE))
        }
        Ok(_) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "post-fx produced no samples".into(),
        ),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

/// Which status a pipeline error from `/render` maps to: parameter /
/// project-content problems are client errors, everything else is a
/// server-side engine failure.
fn render_error_status(e: &str) -> StatusCode {
    if e.starts_with("track ") || e.starts_with("no voice part") || e.starts_with("parse ")
        || e.starts_with("after_load")
    {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    }
}

/// Request-level validation for `/synth-note`.
fn validate_synth_note(req: &SynthNoteRequest) -> Result<(), String> {
    if !(0..=127).contains(&req.tone) {
        return Err(format!("tone {} out of range (0..=127)", req.tone));
    }
    if !req.duration_ms.is_finite()
        || req.duration_ms <= 0.0
        || req.duration_ms > MAX_DURATION_MS
    {
        return Err(format!(
            "duration_ms {} out of range (0..={MAX_DURATION_MS})",
            req.duration_ms
        ));
    }
    if req.phoneme.is_empty() {
        return Err("phoneme must not be empty".into());
    }
    Ok(())
}

/// Where a request's `voicebank` field may point.
enum ResolveError {
    /// No configured bank matched and the value is not a directory.
    NotFound(String),
    /// The value is a directory but does not load as a voicebank.
    LoadFailed(String),
}

impl ResolveError {
    fn status(&self) -> StatusCode {
        match self {
            ResolveError::NotFound(_) => StatusCode::NOT_FOUND,
            ResolveError::LoadFailed(_) => StatusCode::BAD_REQUEST,
        }
    }
    fn message(&self) -> String {
        match self {
            ResolveError::NotFound(m) | ResolveError::LoadFailed(m) => m.clone(),
        }
    }
}

/// Resolve the `voicebank` request field: first against the banks
/// scanned at startup (by name / dir id / absolute path), then — so a
/// client can point at any library dir — by loading the value directly
/// when it is an existing directory.
fn resolve_voicebank(state: &AppState, name: &str) -> Result<Arc<Voicebank>, ResolveError> {
    if let Some(bank) = state.find_voicebank(name) {
        return Ok(bank);
    }
    let path = Path::new(name);
    if path.is_dir() {
        let vb = pipeline::load_voicebank(path).map_err(|e| {
            ResolveError::LoadFailed(format!(
                "cannot load voicebank at {}: {e}",
                path.display()
            ))
        })?;
        return Ok(Arc::new(vb));
    }
    Err(ResolveError::NotFound(format!(
        "voicebank '{}' not found (GET /voicebanks lists the configured banks)",
        name
    )))
}

/// 200 with the wav bytes.
fn wav_response(wav: Vec<u8>) -> Response {
    (
        [
            (
                header::CONTENT_TYPE,
                header::HeaderValue::from_static("audio/wav"),
            ),
            (
                header::CONTENT_DISPOSITION,
                header::HeaderValue::from_static("attachment; filename=\"synth.wav\""),
            ),
        ],
        wav,
    )
        .into_response()
}

/// JSON `{"error": ...}` response.
fn error_response(status: StatusCode, message: String) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::state::AppState;
    use crate::voicebanks;

    fn test_data_dir() -> PathBuf {
        // tools/synth-server → native/test-data (two levels up)
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data")
    }

    /// AppState without a renderer (validation/error paths only).
    fn test_state() -> Arc<AppState> {
        let scan = voicebanks::scan_voicebanks(&test_data_dir()).expect("scan test-data");
        Arc::new(AppState::new(test_data_dir(), scan.entries, None))
    }

    fn synth_req(voicebank: &str, phoneme: &str, tone: i32, duration_ms: f64) -> SynthNoteRequest {
        SynthNoteRequest {
            voicebank: voicebank.into(),
            phoneme: phoneme.into(),
            tone,
            duration_ms,
            out: None,
        }
    }

    #[tokio::test]
    async fn health_reports_ok_without_so() {
        let resp = health(State(test_state())).await;
        assert_eq!(resp.0["status"], "ok");
        assert_eq!(resp.0["so_loaded"], false);
        assert_eq!(resp.0["version"], env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn capabilities_are_static() {
        let resp = capabilities(State(test_state())).await;
        assert_eq!(resp.0["sample_rate"], 44100);
        assert_eq!(resp.0["channels"], 1);
        assert_eq!(resp.0["needs_oto"], true);
        assert_eq!(resp.0["needs_frq"], false);
        assert_eq!(resp.0["modes"][0], "classic");
        assert!(resp.0["expressions"].as_array().unwrap().len() >= 12);
    }

    #[tokio::test]
    async fn voicebanks_lists_scanned_banks() {
        let resp = voicebanks(State(test_state())).await;
        let banks = resp.0["voicebanks"].as_array().expect("voicebanks array");
        let mock = banks
            .iter()
            .find(|b| b["dir"] == "mock-voicebank")
            .expect("mock-voicebank listed");
        assert_eq!(mock["name"], "Teto Mock");
        assert_eq!(mock["aliases_count"], 19);
        assert_eq!(mock["wav_count"], 3);
        assert_eq!(mock["samples_rate"], 44100);
    }

    #[tokio::test]
    async fn stats_zero_by_default() {
        let state = test_state();
        let resp = stats(State(state.clone())).await;
        assert_eq!(resp.renders_count, 0);
        assert_eq!(resp.total_ms, 0);
        assert_eq!(resp.cache_hits, 0);
    }

    #[tokio::test]
    async fn synth_note_unknown_voicebank_is_404() {
        let resp = synth_note(
            State(test_state()),
            JsonBody(synth_req("no-such-bank", "A", 60, 500.0)),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn synth_note_bad_tone_is_400() {
        let resp = synth_note(
            State(test_state()),
            JsonBody(synth_req("mock-voicebank", "A", 999, 500.0)),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn synth_note_bad_duration_is_400() {
        let resp = synth_note(
            State(test_state()),
            JsonBody(synth_req("mock-voicebank", "A", 60, 0.0)),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn synth_note_unmapped_phoneme_is_400() {
        // The mock bank has no `r` alias; the oto gate must reject it
        // before any renderer work.
        let resp = synth_note(
            State(test_state()),
            JsonBody(synth_req("mock-voicebank", "r", 60, 500.0)),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .expect("read body");
        let json: Value = serde_json::from_slice(&body).expect("error json");
        assert!(json["error"].as_str().unwrap().contains("no oto entry"));
    }

    #[tokio::test]
    async fn synth_note_without_renderer_is_500() {
        let resp = synth_note(
            State(test_state()),
            JsonBody(synth_req("mock-voicebank", "A", 60, 500.0)),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn render_missing_project_is_404() {
        let resp = render(
            State(test_state()),
            JsonBody(RenderRequest {
                project: "/nonexistent/song.ustx".into(),
                voicebank: "mock-voicebank".into(),
                track: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn render_unknown_voicebank_is_404() {
        let resp = render(
            State(test_state()),
            JsonBody(RenderRequest {
                project: test_data_dir().join("mock-song.ustx").display().to_string(),
                voicebank: "no-such-bank".into(),
                track: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn render_without_renderer_is_500() {
        let resp = render(
            State(test_state()),
            JsonBody(RenderRequest {
                project: test_data_dir().join("mock-song.ustx").display().to_string(),
                voicebank: "mock-voicebank".into(),
                track: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
