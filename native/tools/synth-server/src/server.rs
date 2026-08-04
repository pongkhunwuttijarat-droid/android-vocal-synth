//! Router assembly and per-request logging.

use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use axum::extract::Request;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;

use crate::handlers;
use crate::state::AppState;

/// Build the API router over `state`.
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(handlers::health))
        .route("/capabilities", get(handlers::capabilities))
        .route("/voicebanks", get(handlers::voicebanks))
        .route("/synth-note", post(handlers::synth_note))
        .route("/render", post(handlers::render))
        .route("/stats", get(handlers::stats))
        .with_state(state)
        .layer(middleware::from_fn(log_requests))
}

/// Log every request: method, path, status and duration.
async fn log_requests(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let started = Instant::now();
    let response = next.run(req).await;
    println!(
        "[{}] {} {} -> {} ({:.1} ms)",
        timestamp(),
        method,
        uri,
        response.status(),
        started.elapsed().as_secs_f64() * 1000.0
    );
    response
}

/// `HH:MM:SS` UTC for the request log.
fn timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!(
        "{:02}:{:02}:{:02} UTC",
        (secs / 3600) % 24,
        (secs / 60) % 60,
        secs % 60
    )
}
