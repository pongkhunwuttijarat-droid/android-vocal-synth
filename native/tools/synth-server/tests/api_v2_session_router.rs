//! Router-level coverage for project import/export routes used by host file
//! pickers. This does not claim Android SAF or voicebank ingestion support.

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use synth_server::server::{router, BODY_LIMIT_BYTES};
use synth_server::state::AppState;
use tower::ServiceExt;

const DEMO_USTX: &str = include_str!("../../synth-cli/tests/data/demo-song.ustx");

fn test_state() -> Arc<AppState> {
    let root = std::env::temp_dir().join(format!(
        "rusted-utau-router-import-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    Arc::new(AppState::new(root.clone(), root, Vec::new(), None))
}

async fn json_response(response: axum::response::Response) -> (StatusCode, Value) {
    let status = response.status();
    let bytes = to_bytes(response.into_body(), BODY_LIMIT_BYTES + 1024)
        .await
        .expect("read response body");
    (
        status,
        serde_json::from_slice(&bytes).expect("JSON response"),
    )
}

#[tokio::test]
async fn router_imports_ustx_into_rust_session_and_exports_canonical_document() {
    let state = test_state();
    let app = router(state);
    let request = Request::builder()
        .method("POST")
        .uri("/api/v2/sessions")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"source":{"type":"imported_ustx","name":"imported-demo","ustx":DEMO_USTX}})
                .to_string(),
        ))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let (status, created) = json_response(response).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let session_id = created["session_id"].as_str().expect("engine session ID");
    assert_eq!(created["snapshot"]["project"]["name"], "Demo Song");
    assert_eq!(created["revision"], 0);

    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/v2/sessions/{session_id}/export"))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let (status, exported) = json_response(response).await;
    assert_eq!(status, StatusCode::OK, "{exported}");
    assert_eq!(exported["revision"], 0);
    assert_eq!(exported["sidecar_included"], false);
    assert!(exported["ustx"]
        .as_str()
        .unwrap()
        .contains("name: Demo Song"));
}

#[tokio::test]
async fn router_enforces_the_documented_json_body_limit() {
    let app = router(test_state());
    let oversized = " ".repeat(BODY_LIMIT_BYTES + 1);
    let request = Request::builder()
        .method("POST")
        .uri("/api/v2/sessions")
        .header("content-type", "application/json")
        .body(Body::from(oversized))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}
