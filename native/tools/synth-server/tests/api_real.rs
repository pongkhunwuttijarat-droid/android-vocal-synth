//! Real-.so handler tests: the success paths of `POST /synth-note` and
//! `POST /render` against the built `libworldline.so` (ignored by
//! default — they need the prebuilt library and the golden Teto English
//! voicebank):
//!
//! ```sh
//! WORLDLINE_SO=.../native/build/build-linux/libworldline.so \
//!     cargo test -p synth-server --test api_real -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::State;
use axum::http::{header, StatusCode};
use synth_server::handlers::{self, JsonBody, RenderRequest, SynthNoteRequest};
use synth_server::render_service::RenderService;
use synth_server::state::AppState;
use synth_server::voicebanks;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn so_path() -> PathBuf {
    if let Ok(path) = std::env::var("WORLDLINE_SO") {
        return PathBuf::from(path);
    }
    let path = repo_root().join("native/build/build-linux/libworldline.so");
    assert!(
        path.exists(),
        "built libworldline.so not found at {path:?}; set WORLDLINE_SO"
    );
    path
}

fn mock_bank_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../test-data/mock-voicebank")
}

fn teto_bank_path() -> PathBuf {
    repo_root().join("test/golden/teto-english/library")
}

fn demo_song_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../synth-cli/tests/data/demo-song.ustx")
}

fn mock_song_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../test-data/mock-song.ustx")
}

/// AppState with the real renderer and the mock bank scanned.
fn real_state() -> Arc<AppState> {
    let scan = voicebanks::scan_voicebanks(&mock_bank_path()).expect("scan mock bank");
    Arc::new(AppState::new(
        mock_bank_path(),
        scan.entries,
        Some(RenderService::spawn(so_path()).expect("spawn render service")),
    ))
}

async fn wav_body(resp: axum::response::Response) -> Vec<u8> {
    assert_eq!(resp.headers()[header::CONTENT_TYPE], "audio/wav");
    axum::body::to_bytes(resp.into_body(), 32 * 1024 * 1024)
        .await
        .expect("read body")
        .to_vec()
}

#[tokio::test]
#[ignore = "requires libworldline.so; WORLDLINE_SO=... cargo test -p synth-server --test api_real -- --ignored --nocapture"]
async fn synth_note_returns_wav_bytes_and_records_stats() {
    let state = real_state();
    let resp = handlers::synth_note(
        State(state.clone()),
        JsonBody(SynthNoteRequest {
            voicebank: "mock-voicebank".into(),
            phoneme: "A".into(),
            tone: 60,
            duration_ms: 500.0,
            out: None,
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = wav_body(resp).await;
    assert_eq!(&body[..4], b"RIFF", "wav header");
    assert!(body.len() > 44, "wav body");
    // ~500 ms of 44.1 kHz mono 16-bit PCM
    let expected = 44 + 500 * 44100 * 2 / 1000;
    assert!(
        (body.len() as i64 - expected as i64).abs() < 2000,
        "wav length {} ~= {expected}",
        body.len()
    );
    let snapshot = state.stats.snapshot();
    assert_eq!(snapshot.renders_count, 1);
    assert!(snapshot.total_ms > 0);
}

#[tokio::test]
#[ignore = "requires libworldline.so; WORLDLINE_SO=... cargo test -p synth-server --test api_real -- --ignored --nocapture"]
async fn render_demo_song_with_teto_bank_returns_wav_bytes() {
    let state = real_state();
    let resp = handlers::render(
        State(state.clone()),
        JsonBody(RenderRequest {
            project: demo_song_path().display().to_string(),
            // The teto library is not scanned into the state; the path
            // fallback must load it on the fly.
            voicebank: teto_bank_path().display().to_string(),
            track: None,
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = wav_body(resp).await;
    assert_eq!(&body[..4], b"RIFF");
    // demo-song is ~3 s: 44 + ~3000 ms * 44100 * 2 / 1000 bytes
    assert!(body.len() > 44 + 1_000 * 44100 * 2 / 1000, "> 1 s of audio");
    assert_eq!(state.stats.snapshot().renders_count, 1);
}

#[tokio::test]
#[ignore = "requires libworldline.so; WORLDLINE_SO=... cargo test -p synth-server --test api_real -- --ignored --nocapture"]
async fn render_unmapped_project_returns_json_error() {
    // mock-song's ARPABET hints do not map in the teto bank → every
    // phrase is skipped → 500 with a JSON error body.
    let state = real_state();
    let resp = handlers::render(
        State(state),
        JsonBody(RenderRequest {
            project: mock_song_path().display().to_string(),
            voicebank: teto_bank_path().display().to_string(),
            track: None,
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .expect("read body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("error json");
    assert!(
        json["error"].as_str().unwrap().contains("no audio produced"),
        "error body: {json}"
    );
}
