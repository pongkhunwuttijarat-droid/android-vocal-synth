//! MK3 `/api/v3/ws` origin/authentication and hello negotiation.
//!
//! This is deliberately a separate route from the legacy v2 HTTP API. A
//! trusted native host may provide a launch credential; the loopback Web host
//! can instead obtain a short-lived single-use credential from the bootstrap
//! route after an exact-origin check.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures_util::SinkExt;
use serde_json::{json, Value};
use tokio::sync::watch;

use crate::render_service::{ProgressCounters, StreamRenderEvent};
use crate::state::AppState;

pub const ROUTE: &str = "/api/v3/ws";
pub const CREDENTIAL_ROUTE: &str = "/api/v3/credential";
pub const SUBPROTOCOL: &str = "rusted-utau.v3";
const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CONTROL_BYTES: usize = 16 * 1024;
const MAX_WS_MESSAGE_BYTES: usize = 64 * 1024;
const CONTRACT_VERSION: &str = "3.0-draft";
const RENDER_STREAM_FEATURE: &str = "render.stream";
const MAX_STREAM_WAIT_MS: u64 = 1000;
// 4096 stereo f32 frames is 32 KiB of PCM (about 93 ms at 44.1 kHz),
// leaving room for the RUP3 header inside the bounded 64 KiB WS message.
const MAX_STREAM_BLOCK_FRAMES: usize = 4096;
const MAX_STREAM_TOMBSTONES: usize = 512;
// At most 32 deferred control messages, each limited by MAX_CONTROL_BYTES.
const MAX_DEFERRED_REQUESTS: usize = 32;
const STREAM_TOMBSTONE_TTL: Duration = Duration::from_secs(60);
pub const PLAYBACK_CREDENTIAL_TTL: Duration = Duration::from_secs(30);
const MAX_PLAYBACK_CREDENTIALS: usize = 64;
static NEXT_STREAM_ID: AtomicU64 = AtomicU64::new(1);
type EventReceiver = Arc<StdMutex<Receiver<StreamRenderEvent>>>;

/// Host-local stream secret/grants and exact allowed browser origins.
#[derive(Clone)]
pub struct StreamAuth {
    credential: Option<Arc<str>>,
    allowed_origins: Arc<HashSet<String>>,
    playback_credentials: Arc<StdMutex<HashMap<String, Instant>>>,
}

impl StreamAuth {
    pub fn new(
        credential: impl Into<String>,
        allowed_origins: impl IntoIterator<Item = String>,
    ) -> Result<Self, String> {
        let credential = credential.into();
        if credential.len() < 32 || !credential.is_ascii() {
            return Err("WebSocket launch credential must be at least 32 ASCII bytes".into());
        }
        let allowed_origins = allowed_origins
            .into_iter()
            .map(|origin| origin.trim().to_string())
            .collect::<HashSet<_>>();
        if allowed_origins.is_empty()
            || allowed_origins
                .iter()
                .any(|origin| !is_exact_origin(origin))
        {
            return Err(
                "WebSocket auth requires exact http(s) origins; wildcards and paths are invalid"
                    .into(),
            );
        }
        Ok(Self {
            credential: Some(Arc::from(credential)),
            allowed_origins: Arc::new(allowed_origins),
            playback_credentials: Arc::new(StdMutex::new(HashMap::new())),
        })
    }

    /// Configure one-use browser grants for a loopback-only local server.
    pub fn browser_bootstrap(
        allowed_origins: impl IntoIterator<Item = String>,
    ) -> Result<Self, String> {
        let allowed_origins = allowed_origins
            .into_iter()
            .map(|origin| origin.trim().to_string())
            .collect::<HashSet<_>>();
        if allowed_origins.is_empty()
            || allowed_origins
                .iter()
                .any(|origin| !is_exact_origin(origin))
        {
            return Err(
                "WebSocket auth requires exact http(s) origins; wildcards and paths are invalid"
                    .into(),
            );
        }
        Ok(Self {
            credential: None,
            allowed_origins: Arc::new(allowed_origins),
            playback_credentials: Arc::new(StdMutex::new(HashMap::new())),
        })
    }

    fn allows_origin(&self, origin: &str) -> bool {
        self.allowed_origins.contains(origin)
    }

    fn authenticates(&self, candidate: &str) -> bool {
        if self
            .credential
            .as_ref()
            .is_some_and(|credential| constant_time_eq(credential.as_bytes(), candidate.as_bytes()))
        {
            return true;
        }
        let now = Instant::now();
        let mut credentials = self
            .playback_credentials
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        credentials.retain(|_, expires| *expires > now);
        credentials.remove(candidate).is_some()
    }

    pub fn issue_playback_credential(&self, origin: &str) -> Result<String, CredentialIssueError> {
        if !self.allows_origin(origin) {
            return Err(CredentialIssueError::OriginNotAllowed);
        }
        if self.credential.is_some() {
            return Err(CredentialIssueError::BootstrapDisabled);
        }
        let now = Instant::now();
        let mut credentials = self
            .playback_credentials
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        credentials.retain(|_, expires| *expires > now);
        if credentials.len() >= MAX_PLAYBACK_CREDENTIALS {
            return Err(CredentialIssueError::Capacity);
        }
        let credential = uuid::Uuid::new_v4().simple().to_string();
        credentials.insert(credential.clone(), now + PLAYBACK_CREDENTIAL_TTL);
        Ok(credential)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialIssueError {
    OriginNotAllowed,
    BootstrapDisabled,
    Capacity,
}

fn is_exact_origin(origin: &str) -> bool {
    if origin.contains('*') || origin.contains('?') || origin.contains('#') {
        return false;
    }
    let Some((scheme, authority)) = origin.split_once("://") else {
        return false;
    };
    matches!(scheme, "http" | "https")
        && !authority.is_empty()
        && !authority.contains('/')
        && !authority.contains('@')
        && !authority.chars().any(char::is_whitespace)
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |diff, (a, b)| diff | (a ^ b))
        == 0
}

struct LiveStream {
    id: String,
    generation: String,
    session_id: String,
    revision: u64,
    cancel: Arc<AtomicBool>,
    cancel_signal: watch::Sender<bool>,
    cancel_events: watch::Receiver<bool>,
    events: EventReceiver,
    sequence: u64,
    next_frame: u64,
    received_frame: u64,
    pending_samples: VecDeque<f32>,
    terminal: Option<Value>,
    pending_terminal: Option<Value>,
    slot_active: bool,
    state: Arc<AppState>,
}

impl LiveStream {
    fn release_session_slot(&mut self) {
        if !self.slot_active {
            return;
        }
        self.slot_active = false;
        release_session_slot(&self.state, &self.session_id);
    }

    /// Stop generation and drain the bounded producer queue in a blocking
    /// worker before releasing the Session's active-render exemption.
    fn retire_job(&mut self) {
        self.cancel.store(true, Ordering::Release);
        let _ = self.cancel_signal.send(true);
        if !self.slot_active {
            return;
        }
        self.slot_active = false;
        let events = self.events.clone();
        let state = self.state.clone();
        let session_id = self.session_id.clone();
        tokio::task::spawn_blocking(move || {
            loop {
                let result = events
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .recv_timeout(Duration::from_secs(35));
                match result {
                    Ok(StreamRenderEvent::Finished(_)) | Err(RecvTimeoutError::Disconnected) => {
                        break;
                    }
                    Ok(StreamRenderEvent::Data { .. }) => continue,
                    Err(RecvTimeoutError::Timeout) => continue,
                }
            }
            release_session_slot(&state, &session_id);
        });
    }
}

fn release_session_slot(state: &AppState, session_id: &str) {
    let mut registry = state
        .sessions
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(entry) = registry.get_mut(session_id) {
        entry.end_render_job();
    }
}

enum EventRead {
    Event(StreamRenderEvent),
    Pending,
    Closed,
    Cancelled,
}

async fn read_event(stream: &LiveStream, wait_ms: u64) -> Result<EventRead, String> {
    let events = stream.events.clone();
    tokio::task::spawn_blocking(move || {
        let receiver = events.lock().unwrap_or_else(|error| error.into_inner());
        if wait_ms == 0 {
            return match receiver.try_recv() {
                Ok(event) => EventRead::Event(event),
                Err(TryRecvError::Empty) => EventRead::Pending,
                Err(TryRecvError::Disconnected) => EventRead::Closed,
            };
        }
        match receiver.recv_timeout(Duration::from_millis(wait_ms)) {
            Ok(event) => EventRead::Event(event),
            Err(RecvTimeoutError::Timeout) => EventRead::Pending,
            Err(RecvTimeoutError::Disconnected) => EventRead::Closed,
        }
    })
    .await
    .map_err(|error| format!("stream event waiter failed: {error}"))
}

async fn read_event_or_cancel(stream: &LiveStream, wait_ms: u64) -> Result<EventRead, String> {
    let mut cancel_events = stream.cancel_events.clone();
    if *cancel_events.borrow() {
        return Ok(EventRead::Cancelled);
    }
    tokio::select! {
        biased;
        changed = cancel_events.changed() => {
            if changed.is_ok() && *cancel_events.borrow() {
                Ok(EventRead::Cancelled)
            } else {
                Ok(EventRead::Pending)
            }
        }
        result = read_event(stream, wait_ms) => result,
    }
}

fn start_stream(
    state: &Arc<AppState>,
    session_id: &str,
    expected_revision: u64,
    params: &Value,
) -> Result<LiveStream, (String, String, Option<u64>)> {
    validate_stream_options(params)?;
    let renderer = state.renderer.clone().ok_or_else(|| {
        (
            "backend_unavailable".to_string(),
            "The native renderer is unavailable".to_string(),
            None,
        )
    })?;
    let engine = params
        .get("engine")
        .and_then(Value::as_str)
        .unwrap_or("r1")
        .to_owned();

    let now = std::time::Instant::now();
    let (yaml, voicebanks, pronunciations, revision) = {
        let mut registry = state
            .sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        registry.sweep(now);
        let Some(entry) = registry.get_mut(session_id) else {
            return Err((
                "not_found".into(),
                format!("session '{session_id}' was not found"),
                None,
            ));
        };
        entry.touch(now);
        let session = &entry.session;
        let revision = session.revision();
        if revision != expected_revision {
            return Err((
                "revision_conflict".into(),
                format!("expected revision {expected_revision}; current revision is {revision}"),
                Some(revision),
            ));
        }
        let project = session.project();
        let voicebanks = state
            .voicebanks_for_project(project)
            .map_err(|message| ("asset_unavailable".into(), message, Some(revision)))?;
        let pronunciations = (0..project.tracks.len())
            .map(|track| session.render_pronunciations_for_track(track))
            .collect();
        let yaml = session.yaml();
        entry.begin_render_job();
        (yaml, voicebanks, pronunciations, revision)
    };

    let cancel = Arc::new(AtomicBool::new(false));
    let (cancel_signal, cancel_events) = watch::channel(false);
    let progress = Arc::new(ProgressCounters::default());
    let events = match renderer.render_mixdown_stream_snapshot(
        yaml,
        voicebanks,
        &engine,
        pronunciations,
        cancel.clone(),
        progress,
    ) {
        Ok(events) => Arc::new(StdMutex::new(events)),
        Err(message) => {
            release_session_slot(state, session_id);
            return Err(("backend_render_failed".into(), message, Some(revision)));
        }
    };
    let id = NEXT_STREAM_ID.fetch_add(1, Ordering::Relaxed);
    Ok(LiveStream {
        id: format!("stream-{id}"),
        generation: format!("generation-{id}"),
        session_id: session_id.to_owned(),
        revision,
        cancel,
        cancel_signal,
        cancel_events,
        events,
        sequence: 0,
        next_frame: 0,
        received_frame: 0,
        pending_samples: VecDeque::new(),
        terminal: None,
        pending_terminal: None,
        slot_active: true,
        state: state.clone(),
    })
}

fn validate_stream_options(params: &Value) -> Result<(), (String, String, Option<u64>)> {
    let options = params.get("options").ok_or_else(|| {
        (
            "validation_error".into(),
            "params.options is required".into(),
            None,
        )
    })?;
    let mode = options.get("mode").and_then(Value::as_str);
    if !matches!(mode, Some("playback" | "export")) {
        return Err((
            "validation_error".into(),
            "options.mode must be playback or export".into(),
            None,
        ));
    }
    let sample_rate = options
        .get("sample_rate")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            (
                "validation_error".into(),
                "options.sample_rate must be an unsigned integer".into(),
                None,
            )
        })?;
    let layout = options
        .get("layout")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            (
                "validation_error".into(),
                "options.layout must be a string".into(),
                None,
            )
        })?;
    if sample_rate != 44100 || layout != "stereo" {
        return Err((
            "unsupported_format".into(),
            "This stream backend currently supports 44100 Hz stereo only".into(),
            None,
        ));
    }
    let range = options.get("range").ok_or_else(|| {
        (
            "validation_error".into(),
            "options.range is required".into(),
            None,
        )
    })?;
    let start_frame = range
        .get("start_frame")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            (
                "validation_error".into(),
                "options.range.start_frame must be an unsigned integer".into(),
                None,
            )
        })?;
    let end_frame = range.get("end_frame").ok_or_else(|| {
        (
            "validation_error".into(),
            "options.range.end_frame is required (or null for an open end)".into(),
            None,
        )
    })?;
    if !end_frame.is_null() && end_frame.as_u64().is_none() {
        return Err((
            "validation_error".into(),
            "options.range.end_frame must be an unsigned integer or null".into(),
            None,
        ));
    }
    if start_frame != 0 || !end_frame.is_null() {
        return Err((
            "unsupported_range".into(),
            "The initial sequential stream supports range [0, end) only".into(),
            None,
        ));
    }
    Ok(())
}

pub async fn upgrade(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let Some(auth) = state.stream_auth.clone() else {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({
            "error": { "code": "stream_auth_unconfigured", "message": "The native host has not configured WebSocket credentials" }
        }))).into_response();
    };
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    if origin.is_none_or(|origin| !auth.allows_origin(origin)) {
        return (StatusCode::FORBIDDEN, Json(json!({
            "error": { "code": "origin_not_allowed", "message": "This origin is not allowed to connect" }
        }))).into_response();
    }
    let offered = headers
        .get(header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .any(|protocol| protocol == SUBPROTOCOL);
    if !offered {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": { "code": "subprotocol_required", "message": "rusted-utau.v3 is required" }
            })),
        )
            .into_response();
    }
    let app_state = state.clone();
    ws.protocols([SUBPROTOCOL])
        .max_message_size(MAX_WS_MESSAGE_BYTES)
        .max_frame_size(MAX_WS_MESSAGE_BYTES)
        .on_upgrade(move |socket| serve_authenticated(socket, auth, app_state))
}

/// Issue one short-lived, single-use credential for one browser playback.
pub async fn playback_credential(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let Some(auth) = state.stream_auth.as_ref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":{"code":"stream_auth_unconfigured"}})),
        )
            .into_response();
    };
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    let Some(origin) = origin else {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":{"code":"origin_required"}})),
        )
            .into_response();
    };
    match auth.issue_playback_credential(origin) {
        Ok(credential) => {
            let mut response = Json(json!({
                "credential": credential,
                "expires_in_ms": PLAYBACK_CREDENTIAL_TTL.as_millis(),
                "single_use": true
            }))
            .into_response();
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                axum::http::HeaderValue::from_static("no-store, private"),
            );
            response.headers_mut().insert(
                header::PRAGMA,
                axum::http::HeaderValue::from_static("no-cache"),
            );
            response
        }
        Err(CredentialIssueError::OriginNotAllowed) => (
            StatusCode::FORBIDDEN,
            Json(json!({"error":{"code":"origin_not_allowed"}})),
        )
            .into_response(),
        Err(CredentialIssueError::BootstrapDisabled) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":{"code":"credential_bootstrap_disabled"}})),
        )
            .into_response(),
        Err(CredentialIssueError::Capacity) => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({"error":{"code":"credential_capacity"}})),
        )
            .into_response(),
    }
}

async fn serve_authenticated(socket: WebSocket, auth: StreamAuth, state: Arc<AppState>) {
    serve_authenticated_with_streams(socket, auth, state, HashMap::new()).await;
}

async fn serve_authenticated_with_streams(
    mut socket: WebSocket,
    auth: StreamAuth,
    state: Arc<AppState>,
    mut streams: HashMap<String, LiveStream>,
) {
    let first = tokio::time::timeout(HELLO_TIMEOUT, socket.recv()).await;
    let hello = match first {
        Ok(Some(Ok(Message::Text(text)))) if text.len() <= MAX_CONTROL_BYTES => {
            serde_json::from_str::<Value>(&text).ok()
        }
        _ => None,
    };
    let Some(hello) = hello else {
        let _ = send_json(&mut socket, json!({
            "type": "error", "error": { "code": "hello_required", "message": "The first message must be a valid hello JSON message" }
        })).await;
        let _ = socket.close().await;
        return;
    };

    let params = hello.get("params");
    let credential = params
        .and_then(|params| params.get("credential"))
        .and_then(Value::as_str);
    let versions = params
        .and_then(|params| params.get("contract_versions"))
        .and_then(Value::as_array);
    let required = params
        .and_then(|params| params.get("required_feature_groups"))
        .and_then(Value::as_array);
    let hello_valid = hello.get("type").and_then(Value::as_str) == Some("hello")
        && hello.get("session_id").is_some_and(Value::is_null)
        && versions.is_some_and(|items| {
            items
                .iter()
                .any(|item| item.as_str() == Some(CONTRACT_VERSION))
        })
        && required.is_some_and(|items| items.iter().all(|item| item.as_str().is_some()));
    if !hello_valid || !credential.is_some_and(|value| auth.authenticates(value)) {
        let _ = send_json(&mut socket, json!({
            "type": "error", "error": { "code": "hello_rejected", "message": "Hello is invalid, unsupported, or unauthenticated" }
        })).await;
        let _ = socket.close().await;
        return;
    }
    let required = required.map(Vec::as_slice).unwrap_or(&[]);
    if required
        .iter()
        .any(|item| item.as_str() != Some(RENDER_STREAM_FEATURE))
    {
        let _ = send_json(&mut socket, json!({
            "type": "hello", "status": "rejected", "contract_version": CONTRACT_VERSION,
            "error": { "code": "feature_unavailable", "message": "A requested MK3 WebSocket feature group is unavailable" }
        })).await;
        let _ = socket.close().await;
        return;
    }
    if send_json(
        &mut socket,
        json!({
            "type": "hello", "status": "ready", "contract_version": CONTRACT_VERSION,
            "capabilities": { "feature_groups": [RENDER_STREAM_FEATURE] }
        }),
    )
    .await
    .is_err()
    {
        return;
    }

    let mut closed_streams = HashMap::<String, std::time::Instant>::new();
    let mut request_ids = HashSet::new();
    let mut deferred_requests = VecDeque::<String>::new();
    let mut playback_started = false;
    'connection: loop {
        let incoming = if let Some(text) = deferred_requests.pop_front() {
            Some(Ok(Message::Text(text.into())))
        } else {
            socket.recv().await
        };
        let Some(message) = incoming else { break };
        match message {
            Ok(Message::Text(text)) => {
                if text.len() > MAX_CONTROL_BYTES {
                    if send_json(&mut socket, json!({
                        "error": { "code": "control_message_too_large", "message": format!("control messages are limited to {MAX_CONTROL_BYTES} bytes") }
                    })).await.is_err() { break; }
                    continue;
                }
                let request = match serde_json::from_str::<Value>(&text) {
                    Ok(request) if request.is_object() => request,
                    _ => {
                        if send_json(&mut socket, json!({ "error": { "code": "invalid_json", "message": "Request must be a JSON object" } })).await.is_err() { break; }
                        continue;
                    }
                };
                let request_id = request.get("request_id").cloned().unwrap_or(Value::Null);
                let request_key = request_id.as_str().unwrap_or("").to_owned();
                if request_key.is_empty() || request_key.len() > 128 {
                    if send_json(&mut socket, json!({ "request_id": request_id, "error": { "code": "validation_error", "message": "request_id must be a non-empty string of at most 128 bytes" } })).await.is_err() { break; }
                    continue;
                }
                if request_ids.contains(&request_key) {
                    if send_json(&mut socket, json!({ "request_id": request_id, "error": { "code": "duplicate_request", "message": "request_id was already used on this connection" } })).await.is_err() { break; }
                    continue;
                }
                if request_ids.len() >= 4096 {
                    if send_json(&mut socket, json!({ "request_id": request_id, "error": { "code": "request_limit_reached", "message": "connection request ID limit reached; reconnect to continue" } })).await.is_err() { break; }
                    continue;
                }
                let now = std::time::Instant::now();
                closed_streams.retain(|_, expires| *expires > now);
                let requested_method = request.get("method").and_then(Value::as_str);
                if requested_method == Some("session_render_stream_begin") && playback_started {
                    if send_json(&mut socket, json!({
                        "request_id": request_id,
                        "error": { "code": "playback_grant_consumed", "message": "one playback is allowed per credential; request a fresh credential to play again" }
                    })).await.is_err() { break 'connection; }
                    continue;
                }
                if requested_method == Some("session_render_stream_begin") {
                    playback_started = true;
                }
                let active_stream_id = request
                    .get("params")
                    .and_then(|params| params.get("stream_id"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                let cancel_signal = (requested_method == Some("session_render_stream_next"))
                    .then(|| {
                        streams
                            .get(&active_stream_id)
                            .map(|stream| stream.cancel_signal.clone())
                    })
                    .flatten();
                if let Some(cancel_signal) = cancel_signal {
                    request_ids.insert(request_key);
                    let active_session_id = streams
                        .get(&active_stream_id)
                        .map(|stream| stream.session_id.clone())
                        .unwrap_or_default();
                    let mut next = Box::pin(dispatch_stream_request(
                        &request,
                        request_id.clone(),
                        &mut streams,
                        &mut closed_streams,
                        &state,
                    ));
                    'wait_next: loop {
                        tokio::select! {
                            response = &mut next => {
                                if send_stream_response(&mut socket, response).await.is_err() {
                                    break 'connection;
                                }
                                break 'wait_next;
                            }
                            incoming = socket.recv() => {
                                match incoming {
                                    Some(Ok(Message::Text(text))) => {
                                        let request = match serde_json::from_str::<Value>(&text) {
                                            Ok(request) if request.is_object() => request,
                                            _ => {
                                                if send_json(&mut socket, json!({ "error": { "code": "invalid_json", "message": "Request must be a JSON object" } })).await.is_err() { break 'connection; }
                                                continue 'wait_next;
                                            }
                                        };
                                        let request_id = request.get("request_id").cloned().unwrap_or(Value::Null);
                                        let request_key = request_id.as_str().unwrap_or("").to_owned();
                                        if request_key.is_empty() || request_key.len() > 128 {
                                            if send_json(&mut socket, json!({ "request_id": request_id, "error": { "code": "validation_error", "message": "request_id must be a non-empty string of at most 128 bytes" } })).await.is_err() { break 'connection; }
                                            continue 'wait_next;
                                        }
                                        if request_ids.contains(&request_key) {
                                            if send_json(&mut socket, json!({ "request_id": request_id, "error": { "code": "duplicate_request", "message": "request_id was already used on this connection" } })).await.is_err() { break 'connection; }
                                            continue 'wait_next;
                                        }
                                        if request_ids.len() >= 4096 {
                                            if send_json(&mut socket, json!({ "request_id": request_id, "error": { "code": "request_limit_reached", "message": "connection request ID limit reached; reconnect to continue" } })).await.is_err() { break 'connection; }
                                            continue 'wait_next;
                                        }
                                        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
                                        let params = request.get("params").unwrap_or(&Value::Null);
                                        let target_id = params.get("stream_id").and_then(Value::as_str).unwrap_or("");
                                        let is_cancel_or_close = matches!(method, "session_render_stream_cancel" | "session_render_stream_close");
                                        if is_cancel_or_close && target_id == active_stream_id {
                                            request_ids.insert(request_key);
                                            let requested_session = request.get("session_id").and_then(Value::as_str).unwrap_or("");
                                            if requested_session != active_session_id {
                                                if send_json(&mut socket, json!({ "request_id": request_id, "error": { "code": "session_mismatch", "message": "stream handle belongs to a different session" } })).await.is_err() { break 'connection; }
                                                continue 'wait_next;
                                            }
                                            let _ = cancel_signal.send(true);
                                            let next_response = (&mut next).await;
                                            if send_stream_response(&mut socket, next_response).await.is_err() {
                                                break 'connection;
                                            }
                                            drop(next);
                                            let cancel_response = dispatch_stream_request(
                                                &request,
                                                request_id,
                                                &mut streams,
                                                &mut closed_streams,
                                                &state,
                                            ).await;
                                            if send_stream_response(&mut socket, cancel_response).await.is_err() {
                                                break 'connection;
                                            }
                                            break 'wait_next;
                                        }
                                        if method == "session_render_stream_next" && target_id == active_stream_id {
                                            request_ids.insert(request_key);
                                            if send_json(&mut socket, json!({
                                                "request_id": request_id,
                                                "error": { "code": "stream_busy", "message": "only one next request may be outstanding for a stream" }
                                            })).await.is_err() {
                                                break 'connection;
                                            }
                                            continue 'wait_next;
                                        }
                                        if deferred_requests.len() >= MAX_DEFERRED_REQUESTS {
                                            request_ids.insert(request_key);
                                            if send_json(&mut socket, json!({
                                                "request_id": request_id,
                                                "error": { "code": "stream_busy", "message": "connection request queue is full while a next request is waiting" }
                                            })).await.is_err() {
                                                break 'connection;
                                            }
                                            continue 'wait_next;
                                        }
                                        deferred_requests.push_back(text.to_string());
                                    }
                                    Some(Ok(Message::Ping(bytes))) => {
                                        if socket.send(Message::Pong(bytes)).await.is_err() { break 'connection; }
                                    }
                                    Some(Ok(Message::Pong(_))) => {}
                                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => {
                                        let _ = cancel_signal.send(true);
                                        let _ = (&mut next).await;
                                        break 'connection;
                                    }
                                    Some(Ok(_)) => {
                                        let _ = cancel_signal.send(true);
                                        let _ = (&mut next).await;
                                        let _ = socket.close().await;
                                        break 'connection;
                                    }
                                }
                            }
                        }
                    }
                    continue;
                }
                request_ids.insert(request_key);
                let response = dispatch_stream_request(
                    &request,
                    request_id,
                    &mut streams,
                    &mut closed_streams,
                    &state,
                )
                .await;
                if send_stream_response(&mut socket, response).await.is_err() {
                    break;
                }
            }
            Ok(Message::Ping(bytes)) => {
                if socket.send(Message::Pong(bytes)).await.is_err() {
                    break;
                }
            }
            Ok(Message::Close(_)) | Err(_) => break,
            _ => {
                let _ = socket.close().await;
                break;
            }
        }
    }
    for stream in streams.values_mut() {
        stream.retire_job();
    }
}

enum StreamResponse {
    Json(Value),
    Binary(Vec<u8>),
}

async fn send_stream_response(
    socket: &mut WebSocket,
    response: StreamResponse,
) -> Result<(), axum::Error> {
    match response {
        StreamResponse::Json(value) => send_json(socket, value).await,
        StreamResponse::Binary(bytes) => socket.send(Message::Binary(bytes.into())).await,
    }
}

async fn dispatch_stream_request(
    request: &Value,
    request_id: Value,
    streams: &mut HashMap<String, LiveStream>,
    closed_streams: &mut HashMap<String, std::time::Instant>,
    state: &Arc<AppState>,
) -> StreamResponse {
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let params = request.get("params").unwrap_or(&Value::Null);
    let session_id = request
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    let error = |code: &str, message: String, revision: Option<u64>| {
        StreamResponse::Json(json!({
            "request_id": request_id, "error": { "code": code, "message": message }, "revision": revision
        }))
    };
    match method {
        "session_render_stream_begin" => {
            if session_id.is_empty() {
                return error(
                    "validation_error",
                    "session_id must be a non-empty string".into(),
                    None,
                );
            }
            let Some(expected_revision) = params.get("expected_revision").and_then(Value::as_u64)
            else {
                return error(
                    "validation_error",
                    "params.expected_revision must be an unsigned integer".into(),
                    None,
                );
            };
            match start_stream(state, session_id, expected_revision, params) {
                Ok(stream) => {
                    let response = json!({
                        "request_id": request_id, "status": "preparing", "stream_id": stream.id,
                        "generation": stream.generation, "session_id": stream.session_id, "revision": stream.revision,
                        "sample_rate": 44100, "channels": 2, "layout": "stereo", "pcm_format": "f32le_planar",
                        "range": {"start_frame": 0, "end_frame": null}, "timing_profiles": [],
                        "limits": {"max_block_frames": MAX_STREAM_BLOCK_FRAMES, "max_wait_ms": MAX_STREAM_WAIT_MS}
                    });
                    streams.insert(stream.id.clone(), stream);
                    StreamResponse::Json(response)
                }
                Err((code, message, revision)) => error(&code, message, revision),
            }
        }
        "session_render_stream_next" => {
            let id = params
                .get("stream_id")
                .and_then(Value::as_str)
                .unwrap_or("");
            let Some(stream) = streams.get_mut(id) else {
                return error(
                    if closed_streams.contains_key(id) {
                        "stream_closed"
                    } else {
                        "stream_not_found"
                    },
                    "stream handle was not found".into(),
                    None,
                );
            };
            if session_id != stream.session_id {
                return error(
                    "session_mismatch",
                    "stream handle belongs to a different session".into(),
                    Some(stream.revision),
                );
            }
            let max_frames = params
                .get("max_frames")
                .and_then(Value::as_u64)
                .and_then(|frames| usize::try_from(frames).ok())
                .unwrap_or(0);
            let wait_ms = params
                .get("wait_ms")
                .and_then(Value::as_u64)
                .unwrap_or(u64::MAX);
            if max_frames == 0
                || max_frames > MAX_STREAM_BLOCK_FRAMES
                || wait_ms > MAX_STREAM_WAIT_MS
            {
                return error(
                    "validation_error",
                    format!(
                        "max_frames must be 1..={MAX_STREAM_BLOCK_FRAMES} and wait_ms 0..={MAX_STREAM_WAIT_MS}"
                    ),
                    Some(stream.revision),
                );
            }
            if let Some(terminal) = &stream.terminal {
                let mut reply = terminal.clone();
                reply["request_id"] = request_id;
                return StreamResponse::Json(reply);
            }
            let deadline = tokio::time::Instant::now() + Duration::from_millis(wait_ms);
            loop {
                let available_frames = stream.pending_samples.len() / 2;
                if available_frames >= max_frames
                    || (stream.pending_terminal.is_some() && available_frames > 0)
                {
                    return stream_data_response(
                        stream,
                        request_id,
                        available_frames.min(max_frames),
                    );
                }
                if available_frames == 0 {
                    if let Some(mut terminal) = stream.pending_terminal.take() {
                        terminal["next_sequence"] = json!(stream.sequence);
                        terminal["final_frame_exclusive"] = json!(stream.next_frame);
                        terminal["request_id"] = request_id;
                        stream.terminal = Some(terminal.clone());
                        return StreamResponse::Json(terminal);
                    }
                }
                let remaining = deadline
                    .saturating_duration_since(tokio::time::Instant::now())
                    .as_millis() as u64;
                match read_event_or_cancel(stream, remaining).await {
                    Ok(EventRead::Cancelled) => {
                        let terminal = terminal_reply(stream, "cancelled", Value::Null, json!([]));
                        stream.terminal = Some(terminal.clone());
                        stream.retire_job();
                        let mut terminal = terminal;
                        terminal["request_id"] = request_id.clone();
                        return StreamResponse::Json(terminal);
                    }
                    Ok(EventRead::Pending) => {
                        if available_frames > 0 {
                            return stream_data_response(
                                stream,
                                request_id,
                                available_frames.min(max_frames),
                            );
                        }
                        return StreamResponse::Json(json!({
                            "request_id": request_id, "status": "pending", "stream_id": stream.id, "generation": stream.generation,
                            "session_id": stream.session_id, "revision": stream.revision, "next_sequence": stream.sequence,
                            "next_frame": stream.next_frame, "retry_after_ms": 10
                        }));
                    }
                    Ok(EventRead::Closed) => {
                        let terminal = terminal_reply(
                            stream,
                            "failed",
                            json!({"code":"backend_render_failed","message":"render event stream closed unexpectedly"}),
                            json!([]),
                        );
                        stream.pending_terminal = Some(terminal);
                        stream.retire_job();
                    }
                    Ok(EventRead::Event(StreamRenderEvent::Data {
                        frame_start,
                        samples,
                    })) => {
                        if frame_start != stream.received_frame
                            || samples.len() % 2 != 0
                            || samples.iter().any(|sample| !sample.is_finite())
                        {
                            let terminal = terminal_reply(
                                stream,
                                "failed",
                                json!({"code":"backend_render_failed","message":"renderer emitted invalid, non-finite, or out-of-order PCM"}),
                                json!([]),
                            );
                            stream.terminal = Some(terminal.clone());
                            stream.retire_job();
                            let mut terminal = terminal;
                            terminal["request_id"] = request_id;
                            return StreamResponse::Json(terminal);
                        }
                        stream.received_frame += (samples.len() / 2) as u64;
                        stream.pending_samples.extend(samples);
                    }
                    Ok(EventRead::Event(StreamRenderEvent::Finished(Ok(report)))) => {
                        if report.channels != 2
                            || report.sample_rate != 44100
                            || report.frames != stream.received_frame
                        {
                            let terminal = terminal_reply(
                                stream,
                                "failed",
                                json!({"code":"backend_render_failed","message":"renderer completion does not match the emitted PCM format or frame prefix"}),
                                json!([]),
                            );
                            stream.pending_terminal = Some(terminal);
                            stream.release_session_slot();
                            continue;
                        }
                        let warnings = Value::Array(report.tracks.into_iter().filter(|track| !track.skipped.is_empty()).map(|track| json!({ "track_no": track.track_no, "skipped": track.skipped })).collect());
                        let terminal = terminal_reply(stream, "end", Value::Null, warnings);
                        stream.pending_terminal = Some(terminal);
                        stream.release_session_slot();
                        continue;
                    }
                    Ok(EventRead::Event(StreamRenderEvent::Finished(Err(message)))) => {
                        let terminal = terminal_reply(
                            stream,
                            "failed",
                            json!({"code":"backend_render_failed","message":message}),
                            json!([]),
                        );
                        stream.pending_terminal = Some(terminal);
                        stream.release_session_slot();
                        continue;
                    }
                    Err(message) => {
                        return error("backend_render_failed", message, Some(stream.revision));
                    }
                }
                if tokio::time::Instant::now() >= deadline {
                    return StreamResponse::Json(json!({
                        "request_id": request_id, "status": "pending", "stream_id": stream.id, "generation": stream.generation,
                        "session_id": stream.session_id, "revision": stream.revision, "next_sequence": stream.sequence,
                        "next_frame": stream.next_frame, "retry_after_ms": 10
                    }));
                }
            }
        }
        "session_render_stream_cancel" | "session_render_stream_close" => {
            let id = params
                .get("stream_id")
                .and_then(Value::as_str)
                .unwrap_or("");
            if method.ends_with("cancel") {
                let Some(stream) = streams.get_mut(id) else {
                    return error(
                        if closed_streams.contains_key(id) {
                            "stream_closed"
                        } else {
                            "stream_not_found"
                        },
                        "stream handle was not found or is already closed".into(),
                        None,
                    );
                };
                if session_id != stream.session_id {
                    return error(
                        "session_mismatch",
                        "stream handle belongs to a different session".into(),
                        Some(stream.revision),
                    );
                }
                if stream.terminal.is_none() {
                    stream.terminal =
                        Some(terminal_reply(stream, "cancelled", Value::Null, json!([])));
                    stream.retire_job();
                }
                let mut reply = stream.terminal.clone().unwrap_or(Value::Null);
                reply["request_id"] = request_id;
                StreamResponse::Json(reply)
            } else {
                if let Some(stream) = streams.get(id) {
                    if session_id != stream.session_id {
                        return error(
                            "session_mismatch",
                            "stream handle belongs to a different session".into(),
                            Some(stream.revision),
                        );
                    }
                } else if closed_streams.contains_key(id) {
                    return StreamResponse::Json(
                        json!({"request_id":request_id,"status":"closed","stream_id":id}),
                    );
                } else {
                    return error(
                        "stream_not_found",
                        "stream handle was not found".into(),
                        None,
                    );
                }
                if closed_streams.len() >= MAX_STREAM_TOMBSTONES {
                    if let Some(oldest) = closed_streams
                        .iter()
                        .min_by_key(|(_, expires)| *expires)
                        .map(|(id, _)| id.clone())
                    {
                        closed_streams.remove(&oldest);
                    }
                }
                if let Some(mut stream) = streams.remove(id) {
                    stream.retire_job();
                }
                closed_streams.insert(
                    id.to_owned(),
                    std::time::Instant::now() + STREAM_TOMBSTONE_TTL,
                );
                StreamResponse::Json(
                    json!({"request_id":request_id,"status":"closed","stream_id":id}),
                )
            }
        }
        _ => error(
            "unsupported_operation",
            format!("unsupported method '{method}'"),
            None,
        ),
    }
}

fn stream_data_response(
    stream: &mut LiveStream,
    request_id: Value,
    frames: usize,
) -> StreamResponse {
    let mut interleaved = Vec::with_capacity(frames * 2);
    for _ in 0..frames * 2 {
        if let Some(sample) = stream.pending_samples.pop_front() {
            interleaved.push(sample);
        }
    }
    if interleaved.len() != frames * 2 {
        return StreamResponse::Json(json!({
            "request_id": request_id,
            "error": { "code": "backend_render_failed", "message": "stream buffer ended inside a stereo frame" },
            "revision": stream.revision
        }));
    }
    let frame_start = stream.next_frame;
    let sequence = stream.sequence;
    stream.sequence += 1;
    stream.next_frame += frames as u64;
    let header = json!({
        "request_id": request_id.clone(), "status": "data", "stream_id": stream.id, "generation": stream.generation,
        "session_id": stream.session_id, "revision": stream.revision, "sequence": sequence,
        "frame_start": frame_start, "frame_count": frames, "sample_rate": 44100,
        "channels": 2, "layout": "stereo", "pcm_format": "f32le_planar"
    });
    match rup3_frame(header, &interleaved) {
        Ok(bytes) => StreamResponse::Binary(bytes),
        Err(message) => {
            stream.cancel.store(true, Ordering::Release);
            StreamResponse::Json(json!({
                "request_id": request_id,
                "error": { "code": "backend_render_failed", "message": message },
                "revision": stream.revision
            }))
        }
    }
}

fn terminal_reply(stream: &LiveStream, status: &str, failure: Value, warnings: Value) -> Value {
    let mut value = json!({ "status": status, "stream_id": stream.id, "generation": stream.generation,
        "session_id": stream.session_id, "revision": stream.revision, "next_sequence": stream.sequence,
        "final_frame_exclusive": stream.next_frame, "warnings": warnings });
    if !failure.is_null() {
        value["error"] = failure;
    }
    value
}

fn rup3_frame(mut header: Value, interleaved: &[f32]) -> Result<Vec<u8>, String> {
    let frames = interleaved.len() / 2;
    let mut planar = Vec::with_capacity(interleaved.len());
    for channel in 0..2 {
        for frame in 0..frames {
            planar.push(interleaved[frame * 2 + channel]);
        }
    }
    header["frame_count"] = json!(frames);
    let header_bytes = serde_json::to_vec(&header).map_err(|error| error.to_string())?;
    if header_bytes.len() > MAX_CONTROL_BYTES {
        return Err("RUP3 header exceeds configured message limit".into());
    }
    let payload_bytes = planar
        .len()
        .checked_mul(4)
        .ok_or_else(|| "PCM payload length overflow".to_string())?;
    let capacity = 8usize
        .checked_add(header_bytes.len())
        .and_then(|n| n.checked_add(payload_bytes))
        .ok_or_else(|| "RUP3 frame length overflow".to_string())?;
    let mut frame = Vec::with_capacity(capacity);
    frame.extend_from_slice(b"RUP3");
    frame.extend_from_slice(&(header_bytes.len() as u32).to_le_bytes());
    frame.extend_from_slice(&header_bytes);
    for sample in planar {
        frame.extend_from_slice(&sample.to_le_bytes());
    }
    Ok(frame)
}

async fn send_json(socket: &mut WebSocket, value: Value) -> Result<(), axum::Error> {
    socket.send(Message::Text(value.to_string().into())).await
}

#[cfg(test)]
mod tests {
    use super::{
        dispatch_stream_request, rup3_frame, serve_authenticated_with_streams,
        validate_stream_options, LiveStream, StreamAuth, StreamResponse, CONTRACT_VERSION,
        MAX_STREAM_BLOCK_FRAMES, MAX_WS_MESSAGE_BYTES, RENDER_STREAM_FEATURE, ROUTE, SUBPROTOCOL,
    };
    use axum::extract::{ws::WebSocketUpgrade, State};
    use axum::http::{header, HeaderMap, StatusCode};
    use axum::response::Response;
    use axum::routing::get;
    use futures_util::{SinkExt, StreamExt};
    use serde_json::{json, Value};
    use std::sync::Arc;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::Message as ClientMessage;

    const SECRET: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn auth_requires_exact_origins_and_a_launch_secret() {
        let auth =
            StreamAuth::new(SECRET, ["https://appassets.androidplatform.net".into()]).unwrap();
        assert!(auth.allows_origin("https://appassets.androidplatform.net"));
        assert!(!auth.allows_origin("https://evil.example"));
        assert!(auth.authenticates(SECRET));
        assert!(!auth.authenticates("wrong"));
    }

    #[test]
    fn browser_credentials_are_origin_bound_single_use_and_expire() {
        let auth = StreamAuth::browser_bootstrap(["http://127.0.0.1:4187".into()]).unwrap();
        assert_eq!(
            auth.issue_playback_credential("http://evil.test"),
            Err(super::CredentialIssueError::OriginNotAllowed)
        );
        let credential = auth
            .issue_playback_credential("http://127.0.0.1:4187")
            .unwrap();
        assert!(auth.authenticates(&credential));
        assert!(!auth.authenticates(&credential));

        let expired = auth
            .issue_playback_credential("http://127.0.0.1:4187")
            .unwrap();
        auth.playback_credentials.lock().unwrap().insert(
            expired.clone(),
            std::time::Instant::now() - std::time::Duration::from_secs(1),
        );
        assert!(!auth.authenticates(&expired));
    }

    #[tokio::test]
    async fn credential_route_rejects_missing_and_untrusted_origins() {
        let state = Arc::new(
            crate::state::AppState::new(
                std::path::PathBuf::from("."),
                std::path::PathBuf::from("/tmp/rusted-utau-stream-credential-tests"),
                vec![],
                None,
            )
            .with_stream_auth(
                StreamAuth::browser_bootstrap(["http://127.0.0.1:4187".into()]).unwrap(),
            ),
        );
        let missing = super::playback_credential(State(state.clone()), HeaderMap::new()).await;
        assert_eq!(missing.status(), StatusCode::FORBIDDEN);
        let mut headers = HeaderMap::new();
        headers.insert(header::ORIGIN, "http://evil.test".parse().unwrap());
        let denied = super::playback_credential(State(state.clone()), headers).await;
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
        let mut headers = HeaderMap::new();
        headers.insert(header::ORIGIN, "http://127.0.0.1:4187".parse().unwrap());
        let accepted = super::playback_credential(State(state), headers).await;
        assert_eq!(accepted.status(), StatusCode::OK);
        assert_eq!(
            accepted.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store, private"
        );
    }

    #[test]
    fn rejects_wildcard_path_and_short_credentials() {
        assert!(StreamAuth::new("short", ["https://localhost".into()]).is_err());
        assert!(StreamAuth::new(SECRET, ["https://*.example".into()]).is_err());
        assert!(StreamAuth::new(SECRET, ["https://example.test/app".into()]).is_err());
    }

    #[test]
    fn rup3_frame_has_little_endian_header_and_planar_pcm() {
        let frame = rup3_frame(
            json!({"request_id":"r1","status":"data"}),
            &[1.0, 10.0, 2.0, 20.0],
        )
        .unwrap();
        assert_eq!(&frame[..4], b"RUP3");
        let header_len = u32::from_le_bytes(frame[4..8].try_into().unwrap()) as usize;
        let header: Value = serde_json::from_slice(&frame[8..8 + header_len]).unwrap();
        assert_eq!(header["frame_count"], 2);
        assert_eq!(header["request_id"], "r1");
        let samples = frame[8 + header_len..]
            .chunks_exact(4)
            .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(samples, [1.0, 2.0, 10.0, 20.0]);
    }

    #[test]
    fn maximum_stream_block_fits_the_bounded_websocket_message() {
        let samples = vec![0.0; MAX_STREAM_BLOCK_FRAMES * 2];
        let frame = rup3_frame(json!({"request_id":"r1","status":"data"}), &samples).unwrap();
        let header_len = u32::from_le_bytes(frame[4..8].try_into().unwrap()) as usize;
        assert_eq!(
            frame.len() - 8 - header_len,
            MAX_STREAM_BLOCK_FRAMES * 2 * 4
        );
        assert!(frame.len() <= MAX_WS_MESSAGE_BYTES);
    }

    #[test]
    fn stream_options_reject_nonzero_or_bounded_ranges() {
        let base = json!({"options":{"mode":"playback","sample_rate":44100,"layout":"stereo","range":{"start_frame":0,"end_frame":null}}});
        assert!(validate_stream_options(&base).is_ok());
        let bounded = json!({"options":{"mode":"playback","sample_rate":44100,"layout":"stereo","range":{"start_frame":0,"end_frame":1024}}});
        assert_eq!(
            validate_stream_options(&bounded).unwrap_err().0,
            "unsupported_range"
        );
        let missing_end = json!({"options":{"mode":"playback","sample_rate":44100,"layout":"stereo","range":{"start_frame":0}}});
        assert_eq!(
            validate_stream_options(&missing_end).unwrap_err().0,
            "validation_error"
        );
        let missing_format = json!({"options":{"mode":"playback","layout":"stereo","range":{"start_frame":0,"end_frame":null}}});
        assert_eq!(
            validate_stream_options(&missing_format).unwrap_err().0,
            "validation_error"
        );
    }

    async fn start_test_server() -> (String, tokio::task::JoinHandle<()>) {
        let state = Arc::new(
            crate::state::AppState::new(
                Default::default(),
                std::path::PathBuf::from("/tmp/rusted-utau-ws-tests"),
                Vec::new(),
                None,
            )
            .with_stream_auth(StreamAuth::new(SECRET, ["https://app.example".into()]).unwrap()),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, crate::server::router(state))
                .await
                .unwrap();
        });
        (format!("ws://{address}{ROUTE}"), task)
    }

    fn hello_request(url: &str, origin: &str) -> axum::http::Request<()> {
        let mut request = url.into_client_request().unwrap();
        request.headers_mut().insert(
            header::ORIGIN,
            origin.parse().expect("valid test origin header"),
        );
        request
            .headers_mut()
            .insert(header::SEC_WEBSOCKET_PROTOCOL, SUBPROTOCOL.parse().unwrap());
        request
    }

    async fn pending_stream_test_route(
        State(state): State<Arc<crate::state::AppState>>,
        ws: WebSocketUpgrade,
    ) -> Response {
        let auth = state.stream_auth.clone().unwrap();
        ws.protocols([SUBPROTOCOL])
            .on_upgrade(move |socket| async move {
                let (_event_sender, event_receiver) = std::sync::mpsc::sync_channel(1);
                let (cancel_signal, cancel_events) = tokio::sync::watch::channel(false);
                let stream = LiveStream {
                    id: "stream-test".into(),
                    generation: "generation-test".into(),
                    session_id: "session-test".into(),
                    revision: 1,
                    cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                    cancel_signal,
                    cancel_events,
                    events: Arc::new(std::sync::Mutex::new(event_receiver)),
                    sequence: 0,
                    next_frame: 0,
                    received_frame: 0,
                    pending_samples: std::collections::VecDeque::new(),
                    terminal: None,
                    pending_terminal: None,
                    slot_active: false,
                    state: state.clone(),
                };
                let streams = std::collections::HashMap::from([("stream-test".into(), stream)]);
                serve_authenticated_with_streams(socket, auth, state, streams).await;
            })
    }

    async fn start_pending_stream_test_server() -> (String, tokio::task::JoinHandle<()>) {
        let state = Arc::new(
            crate::state::AppState::new(
                Default::default(),
                std::path::PathBuf::from("/tmp/rusted-utau-ws-tests"),
                Vec::new(),
                None,
            )
            .with_stream_auth(StreamAuth::new(SECRET, ["https://app.example".into()]).unwrap()),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(
                listener,
                axum::Router::new()
                    .route(ROUTE, get(pending_stream_test_route))
                    .with_state(state),
            )
            .await
            .unwrap();
        });
        (format!("ws://{address}{ROUTE}"), task)
    }

    #[tokio::test]
    async fn websocket_checks_origin_and_launch_credential_before_accepting_features() {
        let (url, server) = start_test_server().await;

        let rejected =
            tokio_tungstenite::connect_async(hello_request(&url, "https://evil.example"))
                .await
                .expect_err("unlisted origin must be rejected before upgrade");
        let status = match rejected {
            tokio_tungstenite::tungstenite::Error::Http(response) => response.status(),
            other => panic!("expected HTTP origin rejection, got {other}"),
        };
        assert_eq!(status, StatusCode::FORBIDDEN);

        let (mut socket, _) =
            tokio_tungstenite::connect_async(hello_request(&url, "https://app.example"))
                .await
                .expect("allowlisted origin upgrades");
        socket
            .send(ClientMessage::Text(
                json!({
                    "type": "hello",
                    "session_id": null,
                    "params": {
                        "credential": "wrong-launch-secret",
                        "contract_versions": [CONTRACT_VERSION],
                        "required_feature_groups": []
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        let response = socket.next().await.unwrap().unwrap();
        let ClientMessage::Text(response) = response else {
            panic!("expected JSON rejection")
        };
        assert_eq!(
            serde_json::from_str::<Value>(&response).unwrap()["error"]["code"],
            "hello_rejected"
        );
        let _ = socket.close(None).await;
        server.abort();
    }

    #[tokio::test]
    async fn websocket_hello_advertises_incremental_mixdown_streaming() {
        let (url, server) = start_test_server().await;
        let (mut socket, _) =
            tokio_tungstenite::connect_async(hello_request(&url, "https://app.example"))
                .await
                .expect("allowlisted origin upgrades");
        socket
            .send(ClientMessage::Text(
                json!({
                    "type": "hello",
                    "session_id": null,
                    "params": {
                        "credential": SECRET,
                        "contract_versions": [CONTRACT_VERSION],
                        "required_feature_groups": [RENDER_STREAM_FEATURE]
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        let response = socket.next().await.unwrap().unwrap();
        let ClientMessage::Text(response) = response else {
            panic!("expected JSON hello response")
        };
        let value = serde_json::from_str::<Value>(&response).unwrap();
        assert_eq!(value["status"], "ready");
        assert_eq!(
            value["capabilities"]["feature_groups"],
            json!([RENDER_STREAM_FEATURE])
        );
        let _ = socket.close(None).await;
        server.abort();
    }

    #[tokio::test]
    async fn websocket_hello_rejects_unknown_required_feature_groups() {
        let (url, server) = start_test_server().await;
        let (mut socket, _) =
            tokio_tungstenite::connect_async(hello_request(&url, "https://app.example"))
                .await
                .expect("allowlisted origin upgrades");
        socket
            .send(ClientMessage::Text(
                json!({
                    "type": "hello",
                    "session_id": null,
                    "params": {
                        "credential": SECRET,
                        "contract_versions": [CONTRACT_VERSION],
                        "required_feature_groups": ["not-a-feature"]
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        let response = socket.next().await.unwrap().unwrap();
        let ClientMessage::Text(response) = response else {
            panic!("expected JSON hello response")
        };
        let value = serde_json::from_str::<Value>(&response).unwrap();
        assert_eq!(value["status"], "rejected");
        assert_eq!(value["error"]["code"], "feature_unavailable");
        let _ = socket.close(None).await;
        server.abort();
    }

    #[tokio::test]
    async fn websocket_stream_begin_enforces_current_range_limit() {
        let (url, server) = start_test_server().await;
        let (mut socket, _) =
            tokio_tungstenite::connect_async(hello_request(&url, "https://app.example"))
                .await
                .unwrap();
        socket.send(ClientMessage::Text(json!({"type":"hello","session_id":null,"params":{"credential":SECRET,"contract_versions":[CONTRACT_VERSION],"required_feature_groups":[]}}).to_string().into())).await.unwrap();
        let _ = socket.next().await.unwrap().unwrap();
        socket.send(ClientMessage::Text(json!({
            "request_id":"req-1", "method":"session_render_stream_begin", "session_id":"missing-session",
            "params":{"expected_revision":0,"options":{"mode":"playback","sample_rate":44100,"layout":"stereo","range":{"start_frame":0,"end_frame":1024}}}
        }).to_string().into())).await.unwrap();
        let response = socket.next().await.unwrap().unwrap();
        let ClientMessage::Text(response) = response else {
            panic!("expected stream validation JSON")
        };
        let value: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(value["request_id"], "req-1");
        assert_eq!(value["error"]["code"], "unsupported_range");
        let _ = socket.close(None).await;
        server.abort();
    }

    #[tokio::test]
    async fn closed_stream_close_is_idempotent_and_next_is_rejected() {
        let state = Arc::new(crate::state::AppState::new(
            Default::default(),
            std::path::PathBuf::from("/tmp/rusted-utau-ws-tests"),
            Vec::new(),
            None,
        ));
        let mut streams = std::collections::HashMap::new();
        let mut tombstones = std::collections::HashMap::from([(
            "stream-closed".to_owned(),
            std::time::Instant::now() + super::STREAM_TOMBSTONE_TTL,
        )]);
        let close = json!({"request_id":"r-close","method":"session_render_stream_close","session_id":"s1","params":{"stream_id":"stream-closed"}});
        let super::StreamResponse::Json(close_reply) = dispatch_stream_request(
            &close,
            json!("r-close"),
            &mut streams,
            &mut tombstones,
            &state,
        )
        .await
        else {
            panic!("expected close response")
        };
        assert_eq!(close_reply["status"], "closed");
        let next = json!({"request_id":"r-next","method":"session_render_stream_next","session_id":"s1","params":{"stream_id":"stream-closed","max_frames":1,"wait_ms":0}});
        let super::StreamResponse::Json(next_reply) = dispatch_stream_request(
            &next,
            json!("r-next"),
            &mut streams,
            &mut tombstones,
            &state,
        )
        .await
        else {
            panic!("expected closed-stream error")
        };
        assert_eq!(next_reply["error"]["code"], "stream_closed");
    }

    #[tokio::test]
    async fn cancel_interrupts_a_waiting_next_request() {
        let state = Arc::new(crate::state::AppState::new(
            Default::default(),
            std::path::PathBuf::from("/tmp/rusted-utau-ws-tests"),
            Vec::new(),
            None,
        ));
        let (_event_sender, event_receiver) = std::sync::mpsc::sync_channel(1);
        let (cancel_signal, cancel_events) = tokio::sync::watch::channel(false);
        let stream = LiveStream {
            id: "stream-test".into(),
            generation: "generation-test".into(),
            session_id: "session-test".into(),
            revision: 1,
            cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            cancel_signal: cancel_signal.clone(),
            cancel_events,
            events: Arc::new(std::sync::Mutex::new(event_receiver)),
            sequence: 0,
            next_frame: 0,
            received_frame: 0,
            pending_samples: std::collections::VecDeque::new(),
            terminal: None,
            pending_terminal: None,
            slot_active: false,
            state: state.clone(),
        };
        let cancel_signal_for_request = cancel_signal.clone();
        let streams = std::collections::HashMap::from([("stream-test".to_owned(), stream)]);
        let closed_streams = std::collections::HashMap::new();
        let request = json!({
            "request_id":"r-next",
            "method":"session_render_stream_next",
            "session_id":"session-test",
            "params":{"stream_id":"stream-test","max_frames":1,"wait_ms":1000}
        });
        let wait = tokio::spawn(async move {
            let mut streams = streams;
            let mut closed_streams = closed_streams;
            dispatch_stream_request(
                &request,
                json!("r-next"),
                &mut streams,
                &mut closed_streams,
                &state,
            )
            .await
        });
        tokio::task::yield_now().await;
        cancel_signal_for_request.send(true).unwrap();

        let result = tokio::time::timeout(std::time::Duration::from_millis(100), wait)
            .await
            .expect("cancel should wake an outstanding next before its wait deadline")
            .unwrap();
        let StreamResponse::Json(result) = result else {
            panic!("cancelled next should resolve with a terminal JSON reply")
        };
        assert_eq!(result["request_id"], "r-next");
        assert_eq!(result["status"], "cancelled");
        assert_eq!(result["next_sequence"], 0);
        assert_eq!(result["final_frame_exclusive"], 0);
    }

    #[tokio::test]
    async fn websocket_reads_cancel_while_next_is_waiting() {
        let (url, server) = start_pending_stream_test_server().await;
        let (mut socket, _) =
            tokio_tungstenite::connect_async(hello_request(&url, "https://app.example"))
                .await
                .unwrap();
        socket.send(ClientMessage::Text(json!({
            "type":"hello", "session_id":null,
            "params":{"credential":SECRET,"contract_versions":[CONTRACT_VERSION],"required_feature_groups":[]}
        }).to_string().into())).await.unwrap();
        let _ = socket.next().await.unwrap().unwrap();

        socket.send(ClientMessage::Text(json!({
            "request_id":"r-next", "method":"session_render_stream_next", "session_id":"session-test",
            "params":{"stream_id":"stream-test","max_frames":1,"wait_ms":1000}
        }).to_string().into())).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        socket.send(ClientMessage::Text(json!({
            "request_id":"r-next-2", "method":"session_render_stream_next", "session_id":"session-test",
            "params":{"stream_id":"stream-test","max_frames":1,"wait_ms":1000}
        }).to_string().into())).await.unwrap();
        let busy = tokio::time::timeout(std::time::Duration::from_millis(200), socket.next())
            .await
            .expect("a second next must be rejected without waiting for the first")
            .unwrap()
            .unwrap();
        let ClientMessage::Text(busy) = busy else {
            panic!("stream_busy reply should be JSON")
        };
        let busy: Value = serde_json::from_str(&busy).unwrap();
        assert_eq!(busy["request_id"], "r-next-2");
        assert_eq!(busy["error"]["code"], "stream_busy");

        socket.send(ClientMessage::Text(json!({
            "request_id":"r-deferred", "method":"not_supported", "session_id":"session-test", "params":{}
        }).to_string().into())).await.unwrap();

        let cancel_started = tokio::time::Instant::now();
        socket.send(ClientMessage::Text(json!({
            "request_id":"r-cancel", "method":"session_render_stream_cancel", "session_id":"session-test",
            "params":{"stream_id":"stream-test"}
        }).to_string().into())).await.unwrap();

        let first = tokio::time::timeout(std::time::Duration::from_millis(200), socket.next())
            .await
            .expect("in-flight next must be interrupted by cancel")
            .unwrap()
            .unwrap();
        let second = tokio::time::timeout(std::time::Duration::from_millis(200), socket.next())
            .await
            .expect("cancel request must receive its own correlated terminal reply")
            .unwrap()
            .unwrap();
        let third = tokio::time::timeout(std::time::Duration::from_millis(200), socket.next())
            .await
            .expect("deferred control request should run after the outstanding next resolves")
            .unwrap()
            .unwrap();
        let ClientMessage::Text(first) = first else {
            panic!("next terminal reply should be JSON")
        };
        let ClientMessage::Text(second) = second else {
            panic!("cancel terminal reply should be JSON")
        };
        let ClientMessage::Text(third) = third else {
            panic!("deferred operation error should be JSON")
        };
        let first: Value = serde_json::from_str(&first).unwrap();
        let second: Value = serde_json::from_str(&second).unwrap();
        let third: Value = serde_json::from_str(&third).unwrap();
        assert_eq!(first["request_id"], "r-next");
        assert_eq!(first["status"], "cancelled");
        assert_eq!(second["request_id"], "r-cancel");
        assert_eq!(second["status"], "cancelled");
        assert_eq!(third["request_id"], "r-deferred");
        assert_eq!(third["error"]["code"], "unsupported_operation");
        assert!(cancel_started.elapsed() < std::time::Duration::from_millis(200));

        let _ = socket.close(None).await;
        server.abort();
    }

    #[tokio::test]
    async fn websocket_reads_close_while_next_is_waiting() {
        let (url, server) = start_pending_stream_test_server().await;
        let (mut socket, _) =
            tokio_tungstenite::connect_async(hello_request(&url, "https://app.example"))
                .await
                .unwrap();
        socket.send(ClientMessage::Text(json!({
            "type":"hello", "session_id":null,
            "params":{"credential":SECRET,"contract_versions":[CONTRACT_VERSION],"required_feature_groups":[]}
        }).to_string().into())).await.unwrap();
        let _ = socket.next().await.unwrap().unwrap();

        socket.send(ClientMessage::Text(json!({
            "request_id":"r-next", "method":"session_render_stream_next", "session_id":"session-test",
            "params":{"stream_id":"stream-test","max_frames":1,"wait_ms":1000}
        }).to_string().into())).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        let close_started = tokio::time::Instant::now();
        socket.send(ClientMessage::Text(json!({
            "request_id":"r-close", "method":"session_render_stream_close", "session_id":"session-test",
            "params":{"stream_id":"stream-test"}
        }).to_string().into())).await.unwrap();

        let first = tokio::time::timeout(std::time::Duration::from_millis(200), socket.next())
            .await
            .expect("in-flight next must be interrupted by close")
            .unwrap()
            .unwrap();
        let second = tokio::time::timeout(std::time::Duration::from_millis(200), socket.next())
            .await
            .expect("close request must receive its correlated reply")
            .unwrap()
            .unwrap();
        let ClientMessage::Text(first) = first else {
            panic!("next terminal reply should be JSON")
        };
        let ClientMessage::Text(second) = second else {
            panic!("close reply should be JSON")
        };
        let first: Value = serde_json::from_str(&first).unwrap();
        let second: Value = serde_json::from_str(&second).unwrap();
        assert_eq!(first["request_id"], "r-next");
        assert_eq!(first["status"], "cancelled");
        assert_eq!(second["request_id"], "r-close");
        assert_eq!(second["status"], "closed");
        assert!(close_started.elapsed() < std::time::Duration::from_millis(200));

        let _ = socket.close(None).await;
        server.abort();
    }
}
