//! Router assembly and per-request logging.

use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use axum::extract::Request;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{delete, get, post, put};
use axum::Router;

use crate::handlers;
use crate::handlers_v2;
use crate::state::AppState;
use crate::stream_ws;

/// Request-body ceiling. axum's default is 2 MiB; it is stated explicitly here
/// so the number a client sees in `/api/v2/capabilities` is the number the
/// server enforces (1 MiB commands in, 2 MiB total body).
pub const BODY_LIMIT_BYTES: usize = 2 * 1024 * 1024;
/// Audio uploads are raw media assets, not project-session JSON commands.
pub const MEDIA_LIMIT_BYTES: usize = 128 * 1024 * 1024;

/// Build the API router over `state`.
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(handlers::health))
        .route(stream_ws::ROUTE, get(stream_ws::upgrade))
        .route(
            stream_ws::CREDENTIAL_ROUTE,
            post(stream_ws::playback_credential),
        )
        .route("/capabilities", get(handlers::capabilities))
        .route("/voicebanks", get(handlers::voicebanks))
        .route(
            "/api/v2/media/{id}",
            put(handlers_v2::upload_media)
                .layer(axum::extract::DefaultBodyLimit::max(MEDIA_LIMIT_BYTES)),
        )
        .route("/synth-note", post(handlers::synth_note))
        .route("/render", post(handlers::render))
        .route("/cancel", post(handlers::cancel))
        .route("/mixer-params", post(handlers::mixer_params))
        .route("/post-fx", post(handlers::post_fx))
        .route("/phonemize", post(handlers::phonemize))
        .route("/project", post(handlers::project_save))
        .route("/projects", get(handlers::projects_list))
        .route("/project/{name}", get(handlers::project_load))
        .route("/project/{name}", delete(handlers::project_delete))
        .route("/stats", get(handlers::stats))
        // ---- v2 project sessions (CONTRACT-v1 §6.7) ----
        // Additive: the legacy routes above keep their behavior.
        .route("/api/v2/capabilities", get(handlers_v2::capabilities))
        .route("/api/v2/sessions", post(handlers_v2::create_session))
        .route(
            "/api/v2/recovery/{recovery_id}",
            post(handlers_v2::restore_recovery),
        )
        .route("/api/v2/sessions/{id}", get(handlers_v2::read_session))
        .route("/api/v2/sessions/{id}", delete(handlers_v2::close_session))
        .route(
            "/api/v2/sessions/{id}/project",
            get(handlers_v2::read_project),
        )
        .route(
            "/api/v2/sessions/{id}/commands",
            post(handlers_v2::session_commands),
        )
        .route(
            "/api/v2/sessions/{id}/undo",
            post(handlers_v2::session_undo),
        )
        .route(
            "/api/v2/sessions/{id}/redo",
            post(handlers_v2::session_redo),
        )
        .route(
            "/api/v2/sessions/{id}/groups",
            post(handlers_v2::group_begin),
        )
        .route(
            "/api/v2/sessions/{id}/groups/{group_id}/commit",
            post(handlers_v2::group_commit),
        )
        .route(
            "/api/v2/sessions/{id}/groups/{group_id}/cancel",
            post(handlers_v2::group_cancel),
        )
        .route(
            "/api/v2/sessions/{id}/save",
            axum::routing::put(handlers_v2::session_save),
        )
        .route(
            "/api/v2/sessions/{id}/save-as",
            axum::routing::put(handlers_v2::session_save_as),
        )
        .route(
            "/api/v2/sessions/{id}/export",
            get(handlers_v2::session_export),
        )
        .route(
            "/api/v2/sessions/{id}/parts/{part_id}",
            get(handlers_v2::read_part),
        )
        .route(
            "/api/v2/sessions/{id}/phonemes/compose",
            post(handlers_v2::phoneme_compose),
        )
        .route(
            "/api/v2/sessions/{id}/parts/{part_id}/notes",
            get(handlers_v2::part_notes),
        )
        .route(
            "/api/v2/sessions/{id}/parts/{part_id}/curves/{curve_id}",
            get(handlers_v2::read_curve),
        )
        .route(
            "/api/v2/sessions/{id}/descriptors/{abbr}",
            get(handlers_v2::expression_descriptor),
        )
        .route(
            "/api/v2/sessions/{id}/parts/{part_id}/stroke-context",
            get(handlers_v2::stroke_context),
        )
        .route("/api/v2/sessions/{id}/tracks", get(handlers_v2::tracks))
        // ---- v2 render jobs (CONTRACT-v1 §6.13) ----
        // Jobs are addressed by `render_id`, not by session: a session can have
        // several takes in flight, and the id is what makes a result
        // attributable to the revision it came from.
        .route(
            "/api/v2/sessions/{id}/render",
            post(handlers_v2::render_start),
        )
        .route(
            "/api/v2/sessions/{id}/mixdown",
            post(handlers_v2::mixdown_start),
        )
        .route(
            "/api/v2/render/{render_id}",
            get(handlers_v2::render_status),
        )
        .route(
            "/api/v2/render/{render_id}/audio",
            get(handlers_v2::render_audio),
        )
        .route(
            "/api/v2/render/{render_id}/cancel",
            post(handlers_v2::render_cancel),
        )
        .layer(axum::extract::DefaultBodyLimit::max(BODY_LIMIT_BYTES))
        .with_state(state)
        .layer(middleware::from_fn(log_requests))
        // Outermost: even an error response carries the headers a browser needs
        // to read it, and OPTIONS preflight never reaches a route handler.
        .layer(middleware::from_fn(cors))
}

/// Permissive CORS for browser clients served from another origin (a static
/// host, a dev server, or a local page). This echoes any caller-supplied
/// Origin; it is not an authentication or origin allowlist.
///
/// The CLI currently defaults to `0.0.0.0`, so the API may be reachable from
/// the LAN. Do not expose credential-issuing endpoints through this middleware
/// without first adding a trusted-origin policy and a loopback-only or
/// otherwise authenticated deployment boundary.
async fn cors(req: Request, next: Next) -> Response {
    let origin = req
        .headers()
        .get(axum::http::header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string());
    let is_preflight = req.method() == axum::http::Method::OPTIONS;
    let mut response = if is_preflight {
        Response::new(axum::body::Body::empty())
    } else {
        next.run(req).await
    };
    let headers = response.headers_mut();
    if let Some(origin) = origin {
        if let Ok(value) = axum::http::HeaderValue::from_str(&origin) {
            headers.insert(axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN, value);
            headers.insert(
                axum::http::header::VARY,
                axum::http::HeaderValue::from_static("origin"),
            );
        }
    }
    headers.insert(
        axum::http::header::ACCESS_CONTROL_ALLOW_METHODS,
        axum::http::HeaderValue::from_static("GET, POST, PUT, DELETE, OPTIONS"),
    );
    headers.insert(
        axum::http::header::ACCESS_CONTROL_ALLOW_HEADERS,
        axum::http::HeaderValue::from_static("content-type, x-mixer-params"),
    );
    headers.insert(
        axum::http::header::ACCESS_CONTROL_MAX_AGE,
        axum::http::HeaderValue::from_static("600"),
    );
    if is_preflight {
        *response.status_mut() = axum::http::StatusCode::NO_CONTENT;
    }
    response
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
