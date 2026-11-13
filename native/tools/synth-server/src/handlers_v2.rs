//! v2 project-session endpoints (`docs/CONTRACT-v1.md` §6.7).
//!
//! Every handler drives the same `project_session::Session` the other hosts
//! use, so the transport only maps JSON to the logical API:
//!
//! * `SessionError.code()` decides the HTTP status (never a second error model);
//! * the idle-TTL sweep runs before each call and every call touches the
//!   session it uses;
//! * a session is addressed by its opaque id, never by project name.
//!
//! The legacy (`CONTRACT-v1` §0–§5) routes in [`crate::handlers`] are untouched;
//! in particular `POST /project` still does *not* write an identity sidecar.

use std::sync::{Arc, MutexGuard};
use std::time::Instant;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::handlers::JsonBody;
use crate::projects::{ProjectError, RecoverySnapshot};
use crate::render_jobs::JobStatus;
use crate::sessions::{self, SessionEntry, SessionSource};
use crate::state::AppState;

/// `GET /api/v2/capabilities` — versions, features, limits, engines, TTL.
pub async fn capabilities(State(state): State<Arc<AppState>>) -> Response {
    let ttl = registry(&state).ttl();
    Json(sessions::capabilities(ttl, state.so_loaded())).into_response()
}

/// `POST /api/v2/sessions` body: a discriminated `source` (§6.11).
#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    pub source: CreateSource,
    /// Opaque browser-local recovery slot. This is only a storage key.
    #[serde(default)]
    pub recovery_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CreateSource {
    /// A new, empty document.
    New {
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        bpm: Option<f64>,
    },
    /// A project stored by this server (name only; the client never sees paths).
    StoredProject { name: String },
    /// Inline USTX (a client-imported file).
    ImportedUstx { name: String, ustx: String },
}

/// `POST /api/v2/sessions` — open a session and return its first snapshot.
pub async fn create_session(
    State(state): State<Arc<AppState>>,
    JsonBody(req): JsonBody<CreateSessionRequest>,
) -> Response {
    let recovery_id = req.recovery_id;
    let (source, store_name) = match req.source {
        CreateSource::New { name, bpm } => {
            let store_name = name.clone();
            (SessionSource::New { name, bpm }, store_name)
        }
        CreateSource::StoredProject { name } => match state.projects.load(&name) {
            Ok(bytes) => {
                let yaml = String::from_utf8_lossy(&bytes).into_owned();
                // A sidecar is optional: an unreadable one must not fail the
                // open (§6.2.4) — it simply yields fresh ids.
                let sidecar = std::fs::read_to_string(state.projects.sidecar_path_for(&name)).ok();
                let pronunciation_sidecar =
                    std::fs::read_to_string(state.projects.pronunciation_sidecar_path_for(&name))
                        .ok();
                let store_name = name.clone();
                (
                    SessionSource::Stored {
                        name,
                        yaml,
                        sidecar,
                        pronunciation_sidecar,
                    },
                    Some(store_name),
                )
            }
            Err(e) => {
                return session_error(StatusCode::NOT_FOUND, "not_found", e.message(), Value::Null)
            }
        },
        CreateSource::ImportedUstx { name, ustx } => (
            SessionSource::Stored {
                name,
                yaml: ustx,
                // An imported document is a new identity domain: no sidecar.
                sidecar: None,
                pronunciation_sidecar: None,
            },
            None,
        ),
    };

    let seed = seed_from_time();
    let opened = {
        let mut reg = registry(&state);
        reg.sweep(Instant::now());
        reg.open_recoverable(source, seed, Instant::now(), recovery_id, store_name)
    };
    let id = match opened {
        Ok(id) => id,
        Err(message) => {
            return session_error(
                StatusCode::BAD_REQUEST,
                "validation_error",
                message,
                Value::Null,
            )
        }
    };

    created_session(&state, &id)
}

/// Reopen the last Rust-authored checkpoint as a fresh session after restart.
/// Revision and undo history restart with the new process/session as specified
/// by the logical session contract.
pub async fn restore_recovery(
    State(state): State<Arc<AppState>>,
    Path(recovery_id): Path<String>,
) -> Response {
    let checkpoint = match state.projects.load_recovery(&recovery_id) {
        Ok(checkpoint) => checkpoint,
        Err(error) => {
            let status = if matches!(error, ProjectError::NotFound(_)) {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            return session_error(status, "not_found", error.message(), Value::Null);
        }
    };
    let name = checkpoint
        .store_name
        .clone()
        .unwrap_or_else(|| "Recovered project".to_string());
    let source = SessionSource::Stored {
        name,
        yaml: checkpoint.ustx,
        sidecar: Some(checkpoint.sidecar),
        pronunciation_sidecar: Some(checkpoint.pronunciation_sidecar),
    };
    let opened = {
        let mut reg = registry(&state);
        reg.sweep(Instant::now());
        reg.open_recoverable(
            source,
            seed_from_time(),
            Instant::now(),
            Some(recovery_id),
            checkpoint.store_name,
        )
    };
    match opened {
        Ok(id) => created_session(&state, &id),
        Err(message) => session_error(
            StatusCode::BAD_REQUEST,
            "validation_error",
            message,
            Value::Null,
        ),
    }
}

/// Body of a successful `POST /api/v2/sessions` (201 Created, §6.7).
fn created_session(state: &AppState, id: &str) -> Response {
    // Read the TTL BEFORE taking the session lock: `with_session` holds the
    // registry guard for the whole call and `std::sync::Mutex` is not
    // re-entrant, so calling back into the registry from the closure would
    // deadlock.
    let caps = sessions::capabilities(reg_ttl(state), state.so_loaded());
    let response = with_session_entry(
        state,
        id,
        |entry| {
            let session = &mut entry.session;
            let snapshot: Value =
                serde_json::from_str(&session.snapshot_json().map_err(err)?.as_str())
                    .unwrap_or(Value::Null);
            Ok(json!({
                "session_id": id,
                "revision": session.revision(),
                "store_name": entry.store_name,
                "ids_stable": session.ids_stable(),
                "warnings": warnings(session),
                "snapshot": snapshot,
                "capabilities": caps,
            }))
        },
        StatusCode::OK,
    );
    (StatusCode::CREATED, response.into_body()).into_response()
}

/// `GET /api/v2/sessions/{id}` — the authoritative snapshot.
pub async fn read_session(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    with_session(&state, &id, |session| {
        let value: Value = serde_json::from_str(session.snapshot_json().map_err(err)?.as_str())
            .unwrap_or(Value::Null);
        Ok(value)
    })
}

/// `GET /api/v2/sessions/{id}/project` — bounded project metadata/maps read (§6.10).
pub async fn read_project(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    with_session(&state, &id, |session| {
        let value: Value =
            serde_json::from_str(session.project_json().map_err(|e| e.to_json())?.as_str())
                .unwrap_or(Value::Null);
        Ok(value)
    })
}

/// `GET /api/v2/sessions/{id}/parts/{part_id}` — bounded single-part read (§6.10).
pub async fn read_part(
    State(state): State<Arc<AppState>>,
    Path((id, part_id)): Path<(String, String)>,
) -> Response {
    with_session(&state, &id, |session| {
        let value: Value = serde_json::from_str(
            session
                .part_json(&part_id)
                .map_err(|e| e.to_json())?
                .as_str(),
        )
        .unwrap_or(Value::Null);
        Ok(value)
    })
}

/// `POST /api/v2/sessions/{id}/phonemes/compose` — bounded candidate read
/// over the current authoritative note and active server voicebank.
pub async fn phoneme_compose(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<Value>,
) -> Response {
    let request = serde_json::to_string(&body).unwrap_or_default();
    with_session(&state, &id, |session| {
        let requested_bank = body.get("voicebank_id").and_then(Value::as_str);
        let voicebank = requested_bank.and_then(|voicebank_id| state.find_voicebank(voicebank_id));
        if let Some(bank_id) = requested_bank.filter(|_| voicebank.is_none()) {
            return Err(
                project_session::v2::SessionError::not_found("voicebank", bank_id).to_json(),
            );
        }
        let composer = phonemizer::PhonemeComposer::new();
        let value = session
            .phoneme_compose_json(&request, &composer, voicebank.as_deref())
            .map_err(|error| error.to_json())?;
        serde_json::from_str::<Value>(&value).map_err(|error| {
            project_session::v2::SessionError::internal(format!(
                "compose result JSON is invalid: {error}"
            ))
            .to_json()
        })
    })
}

/// `POST /api/v2/sessions/{id}/commands` — the mutation envelope (§6.4).
///
/// Group children go through this route too, carrying `group_id` in the body.
pub async fn session_commands(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<Value>,
) -> Response {
    let request = serde_json::to_string(&body).unwrap_or_default();
    with_session(&state, &id, |session| {
        let result = session.apply_v2(&request).map_err(|e| e.to_json())?;
        Ok(serde_json::to_value(result).unwrap_or(Value::Null))
    })
}

/// `POST /api/v2/sessions/{id}/undo` and `/redo`.
#[derive(Debug, Deserialize)]
pub struct HistoryRequest {
    pub expected_revision: Option<u64>,
}

pub async fn session_undo(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    JsonBody(req): JsonBody<HistoryRequest>,
) -> Response {
    session_history(&state, &id, req.expected_revision, true)
}

pub async fn session_redo(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    JsonBody(req): JsonBody<HistoryRequest>,
) -> Response {
    session_history(&state, &id, req.expected_revision, false)
}

fn session_history(
    state: &AppState,
    id: &str,
    expected_revision: Option<u64>,
    undo: bool,
) -> Response {
    with_session(state, id, |session| {
        let expected_revision = expected_revision.ok_or_else(|| {
            project_session::v2::SessionError::validation_field(
                "expected_revision",
                "expected_revision is required",
            )
            .to_json()
        })?;
        let outcome = if undo {
            session.undo_v2(expected_revision)
        } else {
            session.redo_v2(expected_revision)
        }
        .map_err(|error| error.to_json())?;
        match outcome {
            Some(outcome) => Ok(json!({
                "revision": outcome.revision,
                "command_name": outcome.command_name,
                "changed": outcome.changed,
                "can_undo": outcome.can_undo,
                "can_redo": outcome.can_redo,
                "changes": [],
            })),
            // Nothing to undo/redo: revision and history are unchanged (§6.9).
            None => Ok(json!({
                "revision": session.revision(),
                "changed": false,
                "changes": [],
            })),
        }
    })
}

#[derive(Debug, Deserialize)]
pub struct GroupBeginRequest {
    pub expected_revision: Option<u64>,
    #[serde(default)]
    pub name: Option<String>,
}

/// `POST /api/v2/sessions/{id}/groups` — begin an isolated transaction.
pub async fn group_begin(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    JsonBody(req): JsonBody<GroupBeginRequest>,
) -> Response {
    with_session(&state, &id, |session| {
        let expected_revision = req.expected_revision.ok_or_else(|| {
            project_session::v2::SessionError::validation_field(
                "expected_revision",
                "expected_revision is required",
            )
            .to_json()
        })?;
        let begin = session
            .group_begin(expected_revision, req.name.as_deref().unwrap_or("Edit"))
            .map_err(|e| e.to_json())?;
        Ok(json!({
            "group_id": begin.group_id,
            "base_revision": begin.base_revision,
            "revision": session.revision(),
        }))
    })
}

#[derive(Debug, Deserialize)]
pub struct GroupCommitRequest {
    pub expected_revision: Option<u64>,
}

/// `POST /api/v2/sessions/{id}/groups/{group_id}/commit`.
pub async fn group_commit(
    State(state): State<Arc<AppState>>,
    Path((id, group_id)): Path<(String, String)>,
    JsonBody(req): JsonBody<GroupCommitRequest>,
) -> Response {
    with_session(&state, &id, |session| {
        let expected = req.expected_revision.ok_or_else(|| {
            project_session::v2::SessionError::validation_field(
                "expected_revision",
                "expected_revision is required",
            )
            .to_json()
        })?;
        let result = session
            .group_commit(&group_id, expected)
            .map_err(|e| e.to_json())?;
        Ok(serde_json::to_value(result).unwrap_or(Value::Null))
    })
}

/// `POST /api/v2/sessions/{id}/groups/{group_id}/cancel`.
pub async fn group_cancel(
    State(state): State<Arc<AppState>>,
    Path((id, group_id)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let expected_revision = if body.is_empty() {
        None
    } else {
        match serde_json::from_slice::<Value>(&body) {
            Ok(value) => match value.get("expected_revision").and_then(Value::as_u64) {
                Some(revision) => Some(revision),
                None => {
                    let revision = touch_revision(&state, &id);
                    return session_error_at(
                        StatusCode::BAD_REQUEST,
                        "validation_error",
                        "expected_revision must be an unsigned integer when provided".to_string(),
                        json!({"field": "expected_revision"}),
                        revision,
                    );
                }
            },
            Err(error) => {
                let revision = touch_revision(&state, &id);
                return session_error_at(
                    StatusCode::BAD_REQUEST,
                    "validation_error",
                    format!("invalid JSON body: {error}"),
                    Value::Null,
                    revision,
                );
            }
        }
    };
    with_session(&state, &id, |session| {
        if let Some(expected_revision) = expected_revision {
            if expected_revision != session.revision() {
                return Err(project_session::v2::SessionError::revision_conflict(
                    expected_revision,
                    session.revision(),
                )
                .to_json());
            }
        }
        session.group_cancel(&group_id).map_err(|e| e.to_json())?;
        Ok(json!({"cancelled": true, "changed": false, "revision": session.revision()}))
    })
}

#[derive(Debug, Deserialize)]
pub struct SaveRequest {
    pub name: String,
    pub expected_revision: Option<u64>,
}

/// `PUT /api/v2/sessions/{id}/save` — persist `.ustx` + sidecar atomically.
///
/// This is the **only** path that writes an identity sidecar (decision
/// 2026-09-29); the legacy project endpoint keeps its behavior.
pub async fn session_save(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    JsonBody(req): JsonBody<SaveRequest>,
) -> Response {
    // `ProjectStore` is cheap to clone (a path), which keeps the borrow of
    // `state` out of the closure.
    let projects = state.projects.clone();
    with_session_entry(
        &state,
        &id,
        |entry| {
            let session = &mut entry.session;
            // Saving is not a document change: the revision must not move (§6.9).
            let expected = req.expected_revision.ok_or_else(|| {
                project_session::v2::SessionError::validation_field(
                    "expected_revision",
                    "expected_revision is required",
                )
                .to_json()
            })?;
            if expected != session.revision() {
                return Err(project_session::v2::SessionError::revision_conflict(
                    expected,
                    session.revision(),
                )
                .to_json());
            }
            let yaml = session.yaml();
            let sidecar = session.save_sidecar().map_err(err)?;
            let pronunciation_sidecar = session.save_pronunciation_sidecar().map_err(err)?;
            projects
                .save_with_sidecar(
                    &req.name,
                    yaml.as_bytes(),
                    Some(&sidecar),
                    Some(&pronunciation_sidecar),
                )
                .map_err(|e| {
                    project_session::v2::SessionError::new(
                        project_session::v2::ERR_STORAGE,
                        e.message(),
                    )
                    .to_json()
                })?;
            entry.store_name = Some(req.name.clone());
            entry.checkpointed_revision = None;
            Ok(json!({
                "saved": true,
                "name": req.name,
                "revision": session.revision(),
                "sidecar_written": true,
                "pronunciation_sidecar_written": true,
            }))
        },
        StatusCode::OK,
    )
}

/// `PUT /api/v2/sessions/{id}/save-as` — persist a new project identity while
/// retaining track/part/note/curve IDs. The live session adopts the fork only
/// after both files have been written successfully.
pub async fn session_save_as(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    JsonBody(req): JsonBody<SaveRequest>,
) -> Response {
    let projects = state.projects.clone();
    with_session_entry(
        &state,
        &id,
        |entry| {
            let session = &mut entry.session;
            let expected = req.expected_revision.ok_or_else(|| {
                project_session::v2::SessionError::validation_field(
                    "expected_revision",
                    "expected_revision is required",
                )
                .to_json()
            })?;
            if expected != session.revision() {
                return Err(project_session::v2::SessionError::revision_conflict(
                    expected,
                    session.revision(),
                )
                .to_json());
            }
            let payload = session.prepare_save_as().map_err(err)?;
            projects
                .save_with_sidecar(
                    &req.name,
                    payload.yaml.as_bytes(),
                    Some(&payload.sidecar),
                    Some(&payload.pronunciation_sidecar),
                )
                .map_err(|e| {
                    project_session::v2::SessionError::new(
                        project_session::v2::ERR_STORAGE,
                        e.message(),
                    )
                    .to_json()
                })?;
            let project_id = session.commit_save_as_identity();
            debug_assert_eq!(project_id, payload.project_id);
            entry.store_name = Some(req.name.clone());
            entry.checkpointed_revision = None;
            Ok(json!({
                "saved": true,
                "name": req.name,
                "project_id": project_id,
                "revision": session.revision(),
                "sidecar_written": true,
                "pronunciation_sidecar_written": true,
            }))
        },
        StatusCode::OK,
    )
}

/// `GET /api/v2/sessions/{id}/export` — portable canonical USTX, without the
/// private identity sidecar. Importing it starts a fresh identity domain.
pub async fn session_export(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    with_session(&state, &id, |session| {
        Ok(json!({
            "revision": session.revision(),
            "ustx": session.yaml(),
            "sidecar_included": false,
        }))
    })
}

/// `DELETE /api/v2/sessions/{id}` — close and discard unsaved state.
///
/// Closing also cancels the session's render jobs (§6.12): a job whose session
/// is gone could never be delivered to anyone, and leaving it queued would
/// hold the single worker.
pub async fn close_session(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    // Lock order is sessions -> jobs everywhere. Cancelling while the session
    // is still protected closes the race where a worker could publish audio
    // between removing the session and marking its job cancelled.
    let mut reg = registry(&state);
    if reg.get(&id).is_none() {
        return session_error(
            StatusCode::NOT_FOUND,
            "not_found",
            format!("session '{id}' not found"),
            json!({"kind": "session", "id": id}),
        );
    }
    let cancelled = state.render_jobs.cancel_session(&id);
    reg.close(&id);
    Json(json!({"closed": true, "cancelled_render_jobs": cancelled})).into_response()
}

// ---------------------------------------------------------------------------
// Render jobs (§6.13)
// ---------------------------------------------------------------------------

/// `POST /api/v2/sessions/{id}/render` body.
#[derive(Debug, Deserialize)]
pub struct RenderStartRequest {
    pub expected_revision: Option<u64>,
    /// Voicebank as the host store names it (never a filesystem path in the
    /// logical API — this is the same value `POST /render` has always taken).
    pub voicebank: String,
    #[serde(default)]
    pub track: Option<i32>,
    #[serde(default)]
    pub engine: Option<String>,
    /// Optional per-phrase engines, in render order ("ระหว่างท่อน").
    #[serde(default)]
    pub engines: Option<Vec<String>>,
}

/// `POST /api/v2/sessions/{id}/render` — start an asynchronous render job.
///
/// Returns **202** with the job descriptor immediately. The job holds an
/// immutable snapshot of the session at `expected_revision`, so editing while
/// it renders cannot change what it produces (§6.13 item 1).
pub async fn render_start(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    JsonBody(req): JsonBody<RenderStartRequest>,
) -> Response {
    // Preflight failures still describe the revision the client addressed.
    // Touch here because even a rejected logical operation is session activity.
    let Some(addressed_revision) = touch_revision(&state, &id) else {
        return session_error(
            StatusCode::NOT_FOUND,
            "not_found",
            format!("session '{id}' not found (closed, expired, or wrong id)"),
            json!({"kind": "session", "id": id}),
        );
    };
    let Some(expected_revision) = req.expected_revision else {
        return session_error_at(
            StatusCode::BAD_REQUEST,
            "validation_error",
            "expected_revision is required".to_string(),
            json!({"field": "expected_revision"}),
            Some(addressed_revision),
        );
    };
    if addressed_revision != expected_revision {
        return session_error_at(
            StatusCode::CONFLICT,
            "revision_conflict",
            format!(
                "expected revision {}, current revision is {addressed_revision}",
                expected_revision
            ),
            json!({
                "expected_revision": expected_revision,
                "current_revision": addressed_revision,
            }),
            Some(addressed_revision),
        );
    }
    // Resolve everything that can fail for a user reason *before* a job
    // exists: a bad voicebank or a missing renderer must not leave a job in
    // the table that can only fail.
    let voicebank = match crate::handlers::resolve_voicebank(&state, &req.voicebank) {
        Ok(vb) => vb,
        Err(e) => {
            return session_error_at(
                e.status(),
                "not_found",
                e.message().to_string(),
                json!({"kind": "voicebank", "id": req.voicebank}),
                Some(addressed_revision),
            )
        }
    };
    let Some(renderer) = state.renderer.clone() else {
        return session_error_at(
            StatusCode::INTERNAL_SERVER_ERROR,
            "render_error",
            "renderer not loaded (start the server with --so <path>)".to_string(),
            json!({"kind": "renderer"}),
            Some(addressed_revision),
        );
    };
    let track = req.track.unwrap_or(0);
    if track < 0 {
        return session_error_at(
            StatusCode::BAD_REQUEST,
            "validation_error",
            format!("track {track} must be >= 0"),
            json!({"field": "track"}),
            Some(addressed_revision),
        );
    }
    let engine = req.engine.clone().unwrap_or_else(|| "r1".to_string());
    let engines = req.engines.clone().unwrap_or_default();

    let task_state = state.clone();
    let session_id_for_job = id.clone();
    let session_id_for_task = id.clone();
    let session_id_for_response = id.clone();
    with_session_entry(
        &state,
        &id,
        move |entry| {
            let session = &mut entry.session;
            let current = session.revision();
            if current != expected_revision {
                return Err(project_session::v2::SessionError::revision_conflict(
                    expected_revision,
                    current,
                )
                .to_json());
            }
            // Serialize ONCE, here, under the session lock: this string is the
            // job's immutable input and the worker never re-reads a file.
            let yaml = session.yaml();
            let pronunciations = session.render_pronunciations_for_track(track as usize);
            let job = task_state
                .render_jobs
                .begin(&session_id_for_job, current, &engine, track);
            // Exempt the session from the idle TTL until this job ends.
            entry.begin_render_job();

            let render_id = job.render_id.clone();
            let render_id_for_task = render_id.clone();
            let cancel = job.cancel;
            let progress = job.progress;
            let session_id = session_id_for_task;
            let engine_for_task = engine.clone();
            tokio::spawn(async move {
                // Releasing the slot on drop covers every exit path, including
                // a panic inside the render: the session must not stay exempt
                // from expiry forever.
                let _slot = RenderSlot {
                    state: task_state.clone(),
                    session_id,
                };
                let result = renderer
                    .render_project_snapshot_with_pronunciations(
                        yaml,
                        voicebank,
                        track,
                        &engine_for_task,
                        engines,
                        Some(pronunciations),
                        cancel.clone(),
                        progress,
                    )
                    .await;
                match result {
                    Ok(report) if !report.samples.is_empty() => {
                        task_state.render_jobs.finish_ok(
                            &render_id_for_task,
                            report.samples,
                            report.channels,
                        );
                    }
                    Ok(report) => {
                        // Nothing rendered (every phrase was skipped): a real
                        // failure with the reasons, not an empty wav.
                        task_state.render_jobs.finish_err(
                            &render_id_for_task,
                            "render_error",
                            &no_audio_message(&report),
                        );
                    }
                    Err(e) => {
                        // A cancelled job is already terminal; the error is
                        // the cancel itself and must not overwrite it.
                        if !cancel.load(std::sync::atomic::Ordering::Relaxed) {
                            task_state.render_jobs.finish_err(
                                &render_id_for_task,
                                "render_error",
                                &e,
                            );
                        }
                    }
                }
            });

            Ok(json!({
                "render_id": render_id,
                "session_id": session_id_for_response,
                "revision": current,
                "status": JobStatus::Queued.as_str(),
                "engine": engine,
            }))
        },
        StatusCode::ACCEPTED,
    )
}

/// Draft v2 whole-project mixdown request. This is an add-on route; §6.13's
/// frozen per-track render operation remains unchanged.
#[derive(Debug, Deserialize)]
pub struct MixdownStartRequest {
    pub expected_revision: Option<u64>,
    pub voicebank: String,
    #[serde(default)]
    pub engine: Option<String>,
}

/// `POST /api/v2/sessions/{id}/mixdown` — render all tracks from one immutable
/// session snapshot and publish the stereo WAV through the ordinary job routes.
pub async fn mixdown_start(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    JsonBody(req): JsonBody<MixdownStartRequest>,
) -> Response {
    let Some(addressed_revision) = touch_revision(&state, &id) else {
        return session_error(
            StatusCode::NOT_FOUND,
            "not_found",
            format!("session '{id}' not found (closed, expired, or wrong id)"),
            json!({"kind": "session", "id": id}),
        );
    };
    let Some(expected_revision) = req.expected_revision else {
        return session_error_at(
            StatusCode::BAD_REQUEST,
            "validation_error",
            "expected_revision is required".to_string(),
            json!({"field": "expected_revision"}),
            Some(addressed_revision),
        );
    };
    if addressed_revision != expected_revision {
        return session_error_at(
            StatusCode::CONFLICT,
            "revision_conflict",
            format!(
                "expected revision {}, current revision is {addressed_revision}",
                expected_revision
            ),
            json!({"expected_revision": expected_revision, "current_revision": addressed_revision}),
            Some(addressed_revision),
        );
    }
    let voicebank = match crate::handlers::resolve_voicebank(&state, &req.voicebank) {
        Ok(vb) => vb,
        Err(e) => {
            return session_error_at(
                e.status(),
                "not_found",
                e.message().to_string(),
                json!({"kind": "voicebank", "id": req.voicebank}),
                Some(addressed_revision),
            )
        }
    };
    let Some(renderer) = state.renderer.clone() else {
        return session_error_at(
            StatusCode::INTERNAL_SERVER_ERROR,
            "render_error",
            "renderer not loaded (start the server with --so <path>)".to_string(),
            json!({"kind": "renderer"}),
            Some(addressed_revision),
        );
    };
    let engine = req.engine.unwrap_or_else(|| "r1".to_string());
    let task_state = state.clone();
    let session_id = id.clone();
    let job_session_id = id.clone();
    with_session_entry(
        &state,
        &id,
        move |entry| {
            let session = &mut entry.session;
            let current = session.revision();
            if current != expected_revision {
                return Err(project_session::v2::SessionError::revision_conflict(
                    expected_revision,
                    current,
                )
                .to_json());
            }
            let yaml = session.yaml();
            let pronunciations = (0..session.project().tracks.len())
                .map(|track| session.render_pronunciations_for_track(track))
                .collect::<Vec<_>>();
            let job = task_state
                .render_jobs
                .begin(&job_session_id, current, &engine, -1);
            entry.begin_render_job();
            let render_id = job.render_id.clone();
            let render_id_for_task = render_id.clone();
            let cancel = job.cancel;
            let progress = job.progress;
            let engine_for_task = engine.clone();
            tokio::spawn(async move {
                let _slot = RenderSlot {
                    state: task_state.clone(),
                    session_id,
                };
                let result = renderer
                    .render_mixdown_snapshot(
                        yaml,
                        voicebank,
                        &engine_for_task,
                        pronunciations,
                        cancel.clone(),
                        progress,
                    )
                    .await;
                match result {
                    Ok(report) if !report.samples.is_empty() => {
                        task_state.render_jobs.finish_ok(
                            &render_id_for_task,
                            report.samples,
                            report.channels,
                        );
                    }
                    Ok(report) => {
                        let reasons = report
                            .tracks
                            .iter()
                            .flat_map(|track| track.skipped.iter().cloned())
                            .collect::<Vec<_>>();
                        let message = if reasons.is_empty() {
                            "nothing to mix: no track produced voice or wave audio".to_string()
                        } else {
                            format!(
                                "nothing to mix ({} problem(s)): {}",
                                reasons.len(),
                                reasons.join("; ")
                            )
                        };
                        task_state.render_jobs.finish_err(
                            &render_id_for_task,
                            "render_error",
                            &message,
                        );
                    }
                    Err(e) if !cancel.load(std::sync::atomic::Ordering::Relaxed) => {
                        task_state
                            .render_jobs
                            .finish_err(&render_id_for_task, "render_error", &e);
                    }
                    Err(_) => {}
                }
            });
            Ok(json!({
                "render_id": render_id,
                "session_id": job_session_id,
                "revision": current,
                "status": JobStatus::Queued.as_str(),
                "engine": engine,
                "kind": "mixdown",
            }))
        },
        StatusCode::ACCEPTED,
    )
}

/// `PUT /api/v2/media/{id}` — host adapter for browser-imported media. The
/// project stores only the returned opaque `asset:{id}` reference; bytes stay
/// in the engine service's media resolver.
pub async fn upload_media(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return session_error(
            StatusCode::BAD_REQUEST,
            "validation_error",
            "media id must contain only ASCII letters, numbers, and hyphens".to_string(),
            json!({"field": "id"}),
        );
    }
    let Some(renderer) = state.renderer.as_ref() else {
        return session_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "render_error",
            "media import requires the render service".to_string(),
            json!({"kind": "renderer"}),
        );
    };
    let info = match media::probe(&body) {
        Ok(info) => info,
        Err(error) => {
            return session_error(
                StatusCode::BAD_REQUEST,
                "validation_error",
                format!("unsupported or invalid audio file: {error}"),
                json!({"field": "audio"}),
            )
        }
    };
    let media_ref = format!("asset:{id}");
    if let Err(error) = state.projects.save_media(&id, &body) {
        return session_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "storage_error",
            error.message(),
            json!({"kind": "media", "id": id}),
        );
    }
    renderer.register_media(media_ref.clone(), body.to_vec());
    Json(json!({
        "media_ref": media_ref,
        "duration_ms": info.duration_ms,
        "sample_rate": info.sample_rate,
        "channels": info.channels,
    }))
    .into_response()
}

/// `GET /api/v2/render/{render_id}` — poll one job (§6.13 items 2–3).
pub async fn render_status(
    State(state): State<Arc<AppState>>,
    Path(render_id): Path<String>,
) -> Response {
    match state.render_jobs.get(&render_id) {
        Some(view) => Json(view.to_json()).into_response(),
        None => session_error(
            StatusCode::NOT_FOUND,
            "not_found",
            format!("render job '{render_id}' not found"),
            json!({"kind": "render_id", "id": render_id}),
        ),
    }
}

/// `GET /api/v2/render/{render_id}/audio` — the finished wav.
///
/// A job that is still running, failed, or was cancelled has **no audio
/// artifact**, so this is a `not_found` whose `details.status` says why. §6.14
/// has no code for "not finished yet" and inventing one would break clients
/// that branch on the frozen set.
pub async fn render_audio(
    State(state): State<Arc<AppState>>,
    Path(render_id): Path<String>,
) -> Response {
    let Some((samples, revision, channels)) = state.render_jobs.audio(&render_id) else {
        return match state.render_jobs.get(&render_id) {
            Some(view) => session_error_at(
                StatusCode::NOT_FOUND,
                "not_found",
                format!(
                    "render '{}' has no audio (status: {})",
                    view.render_id,
                    view.status.as_str()
                ),
                json!({
                    "kind": "render_audio",
                    "render_id": view.render_id,
                    "status": view.status.as_str(),
                }),
                Some(view.revision),
            ),
            None => session_error(
                StatusCode::NOT_FOUND,
                "not_found",
                format!("render job '{render_id}' not found"),
                json!({"kind": "render_id", "id": render_id}),
            ),
        };
    };
    let wav =
        wavwriter::write_wav_16_channels(&samples, synth_cli::pipeline::SAMPLE_RATE, channels);
    let mut response = crate::handlers::wav_response(wav);
    // The audio carries the revision it was rendered from, so a client can
    // refuse to treat a stale take as current (§6.13 item 6).
    if let Ok(value) = axum::http::HeaderValue::from_str(&revision.to_string()) {
        response
            .headers_mut()
            .insert("x-rusted-utau-render-revision", value);
    }
    response
}

/// `POST /api/v2/render/{render_id}/cancel` — stop **this** job (§6.13 item 5).
///
/// Cancelling an already-finished job is a no-op that reports its real
/// outcome; nothing is overwritten.
pub async fn render_cancel(
    State(state): State<Arc<AppState>>,
    Path(render_id): Path<String>,
) -> Response {
    match state.render_jobs.cancel(&render_id) {
        Some(status) => {
            let view = state.render_jobs.get(&render_id);
            Json(json!({
                "render_id": render_id,
                "session_id": view.as_ref().map(|v| v.session_id.as_str()),
                "revision": view.as_ref().map(|v| v.revision),
                "status": status.as_str(),
                "cancelled": status == JobStatus::Cancelled,
            }))
            .into_response()
        }
        None => session_error(
            StatusCode::NOT_FOUND,
            "not_found",
            format!("render job '{render_id}' not found"),
            json!({"kind": "render_id", "id": render_id}),
        ),
    }
}

/// The legacy wording for "the pipeline produced no samples", kept so the v2
/// failure message explains itself the same way `POST /render` does.
fn no_audio_message(report: &synth_cli::pipeline::RenderReport) -> String {
    let total = report.phrases_rendered + report.skipped.len();
    let reasons = if report.skipped.is_empty() {
        "unknown".to_string()
    } else {
        report.skipped.join("; ")
    };
    format!(
        "no audio produced ({}/{} phrases rendered): {reasons}",
        report.phrases_rendered, total
    )
}

/// Releases a session's render-job slot when the job ends, however it ends.
struct RenderSlot {
    state: Arc<AppState>,
    session_id: String,
}

impl Drop for RenderSlot {
    fn drop(&mut self) {
        if let Ok(mut reg) = self.state.sessions.lock() {
            if let Some(entry) = reg.get_mut(&self.session_id) {
                entry.end_render_job();
            }
        }
    }
}

/// `GET /api/v2/sessions/{id}/parts/{part_id}/notes` (§6.10).
pub async fn part_notes(
    State(state): State<Arc<AppState>>,
    Path((id, part_id)): Path<(String, String)>,
) -> Response {
    with_session(&state, &id, |session| {
        let value: Value = serde_json::from_str(
            session
                .part_notes_json(&part_id)
                .map_err(|e| e.to_json())?
                .as_str(),
        )
        .unwrap_or(Value::Null);
        Ok(value)
    })
}

/// `GET /api/v2/sessions/{id}/parts/{part_id}/curves/{curve_id}` (§6.10).
pub async fn read_curve(
    State(state): State<Arc<AppState>>,
    Path((id, part_id, curve_id)): Path<(String, String, String)>,
) -> Response {
    with_session(&state, &id, |session| {
        let value: Value = serde_json::from_str(
            session
                .curve_json(&part_id, &curve_id)
                .map_err(|e| e.to_json())?
                .as_str(),
        )
        .unwrap_or(Value::Null);
        Ok(value)
    })
}

/// `GET /api/v2/sessions/{id}/descriptors/{abbr}` — authoritative expression
/// metadata, tagged with the revision that owns the descriptor.
pub async fn expression_descriptor(
    State(state): State<Arc<AppState>>,
    Path((id, abbr)): Path<(String, String)>,
) -> Response {
    with_session(&state, &id, |session| {
        let value: Value = serde_json::from_str(
            &session
                .expression_descriptor_json(&abbr)
                .map_err(|error| error.to_json())?,
        )
        .unwrap_or(Value::Null);
        Ok(value)
    })
}

#[derive(Debug, Deserialize)]
pub struct StrokeContextQuery {
    pub abbr: String,
    #[serde(default)]
    pub curve_id: Option<String>,
}

/// `GET /api/v2/sessions/{id}/parts/{part_id}/stroke-context` — bounded
/// read used before applying a curve stroke, including a curve's first draw.
pub async fn stroke_context(
    State(state): State<Arc<AppState>>,
    Path((id, part_id)): Path<(String, String)>,
    Query(query): Query<StrokeContextQuery>,
) -> Response {
    with_session(&state, &id, |session| {
        let value: Value = serde_json::from_str(
            &session
                .curve_stroke_context_for_abbr_json(
                    &part_id,
                    &query.abbr,
                    query.curve_id.as_deref(),
                )
                .map_err(|error| error.to_json())?,
        )
        .unwrap_or(Value::Null);
        Ok(value)
    })
}

/// `GET /api/v2/sessions/{id}/tracks` (§6.10).
pub async fn tracks(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    with_session(&state, &id, |session| {
        let value: Value =
            serde_json::from_str(session.tracks_json().map_err(|e| e.to_json())?.as_str())
                .unwrap_or(Value::Null);
        Ok(value)
    })
}

// ---------------------------------------------------------------------------
// Plumbing
// ---------------------------------------------------------------------------

fn registry(state: &AppState) -> MutexGuard<'_, crate::sessions::SessionRegistry> {
    // A poisoned lock must not take the whole server down: the registry holds
    // only data, so recovering the guard is safe and keeps later requests
    // working.
    state
        .sessions
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn reg_ttl(state: &AppState) -> std::time::Duration {
    registry(state).ttl()
}

fn seed_from_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x5EED)
}

/// Run `f` against one live session: sweep expired entries, reset the idle
/// clock, and map the result (or its error) to a response.
fn with_session<F, T>(state: &AppState, id: &str, f: F) -> Response
where
    F: FnOnce(&mut project_session::Session) -> Result<T, String>,
    T: serde::Serialize,
{
    session_call(state, id, f, StatusCode::OK)
}

/// Like [`with_session`], but with the whole entry so a handler can also touch
/// the lifecycle bookkeeping (a render job exempts the session from the idle
/// TTL, §6.12). Both live under one registry lock on purpose: taking the
/// registry twice — once for the session and once for the counter — is what
/// deadlocked the first version of this module.
fn session_call<F, T>(state: &AppState, id: &str, f: F, ok: StatusCode) -> Response
where
    F: FnOnce(&mut project_session::Session) -> Result<T, String>,
    T: serde::Serialize,
{
    with_session_entry(state, id, |entry| f(&mut entry.session), ok)
}

/// The general form: `f` sees the whole [`SessionEntry`].
fn with_session_entry<F, T>(state: &AppState, id: &str, f: F, ok: StatusCode) -> Response
where
    F: FnOnce(&mut SessionEntry) -> Result<T, String>,
    T: serde::Serialize,
{
    let now = Instant::now();
    let mut reg = registry(state);
    reg.sweep(now);
    let Some(entry) = reg.get_mut(id) else {
        return session_error(
            StatusCode::NOT_FOUND,
            "not_found",
            format!("session '{id}' not found (closed, expired, or wrong id)"),
            json!({"kind": "session", "id": id}),
        );
    };
    // Any call that reaches a live session is activity (§6.12).
    entry.touch(now);
    match f(entry) {
        Ok(value) => {
            let mut recovery_warning = None;
            if entry.recovery_id.is_some()
                && entry.checkpointed_revision != Some(entry.session.revision())
            {
                match persist_recovery(&state.projects, entry) {
                    Ok(()) => entry.checkpointed_revision = Some(entry.session.revision()),
                    Err(error) => {
                        let message = format!(
                            "Automatic recovery checkpoint failed; this project may not survive a server restart: {}",
                            error.message()
                        );
                        eprintln!("session recovery checkpoint failed: {message}");
                        recovery_warning = Some(json!({
                            "code": "recovery_checkpoint_failed",
                            "message": message,
                        }));
                    }
                }
            }
            let mut response_value = serde_json::to_value(value).unwrap_or(Value::Null);
            if let Some(warning) = recovery_warning {
                if !response_value.is_object() {
                    response_value = json!({"result": response_value});
                }
                if let Value::Object(fields) = &mut response_value {
                    fields.insert("recovery_warning".to_string(), warning);
                }
            }
            (ok, Json(response_value)).into_response()
        }
        Err(json_error) => {
            // The session layer owns the logical error; the adapter wraps it
            // in the frozen HTTP envelope and includes the current revision.
            let value: Value = serde_json::from_str(&json_error).unwrap_or(Value::Null);
            let code = value
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("internal_error");
            (
                status_for(code),
                Json(json!({
                    "error": value,
                    "revision": entry.session.revision(),
                })),
            )
                .into_response()
        }
    }
}

fn persist_recovery(
    projects: &crate::projects::ProjectStore,
    entry: &mut SessionEntry,
) -> Result<(), crate::projects::ProjectError> {
    let Some(recovery_id) = entry.recovery_id.as_deref() else {
        return Ok(());
    };
    let yaml = entry.session.yaml();
    let sidecar = entry
        .session
        .save_sidecar()
        .map_err(crate::projects::ProjectError::Io)?;
    let pronunciation_sidecar = entry
        .session
        .save_pronunciation_sidecar()
        .map_err(crate::projects::ProjectError::Io)?;
    projects.save_recovery(
        recovery_id,
        &RecoverySnapshot {
            store_name: entry.store_name.clone(),
            document_name: None,
            ustx: yaml,
            sidecar,
            pronunciation_sidecar,
        },
    )
}

/// The HTTP status for a frozen §6.14 error code.
pub fn status_for(code: &str) -> StatusCode {
    match code {
        "not_found" => StatusCode::NOT_FOUND,
        "validation_error" | "unsupported_operation" => StatusCode::BAD_REQUEST,
        "revision_conflict"
        | "session_busy"
        | "invalid_group_state"
        | "group_failed"
        | "render_cancelled" => StatusCode::CONFLICT,
        "payload_too_large" => StatusCode::PAYLOAD_TOO_LARGE,
        "storage_error" | "render_error" | "internal_error" => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn session_error(status: StatusCode, code: &str, message: String, details: Value) -> Response {
    session_error_at(status, code, message, details, None)
}

fn session_error_at(
    status: StatusCode,
    code: &str,
    message: String,
    details: Value,
    revision: Option<u64>,
) -> Response {
    let mut error = json!({"code": code, "message": message});
    if !details.is_null() {
        error["details"] = details;
    }
    let mut body = json!({"error": error});
    if let Some(revision) = revision {
        body["revision"] = json!(revision);
    }
    (status, Json(body)).into_response()
}

fn touch_revision(state: &AppState, id: &str) -> Option<u64> {
    let now = Instant::now();
    let mut reg = registry(state);
    reg.sweep(now);
    reg.touch(id, now).map(|entry| entry.session.revision())
}

/// Wrap a plain session-layer error message (`Result<_, String>` from
/// `snapshot_json`, `part_notes_json`, …) as the typed JSON error the transport
/// returns, so every failure carries a §6.14 code.
fn err(message: String) -> String {
    project_session::v2::SessionError::internal(message).to_json()
}

fn warnings(session: &project_session::Session) -> Vec<Value> {
    session
        .warnings()
        .iter()
        .map(|w| json!({"code": w.code()}))
        .collect()
}
