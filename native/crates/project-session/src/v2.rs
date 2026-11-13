//! v2 session surface: typed errors, the change vocabulary, mutation results,
//! and stable-ID command planning (CONTRACT-v1 §6.4, §6.14).
//!
//! # Why planning exists
//!
//! The engine's own commands (`commands::*`) address entities by index, and
//! that is deliberate: they were written for the in-process editor where the
//! index is already known. The v2 contract addresses entities by **stable ID**,
//! so this module resolves IDs against the [`IdentityTable`] and *translates*
//! a v2 command into the equivalent legacy index-addressed command.
//!
//! Translation (rather than a second command implementation) is what keeps the
//! two surfaces from diverging: bounds checks, tick rules, undo semantics and
//! atomicity all stay in `commands`, and the legacy facade keeps working.

use std::collections::HashSet;
use std::fmt;

use serde::Serialize;
use serde_json::{json, Value};

use crate::identity::IdentityTable;
use crate::session::valid_pronunciation;
use crate::views::{self, TrackMetaView};
use domain::{UExpressionType, UPart, UProject, UTempo, UTimeSignature, UVoicePart, UWavePart};

// ---------------------------------------------------------------------------
// Error codes (§6.14)
// ---------------------------------------------------------------------------

pub const ERR_NOT_FOUND: &str = "not_found";
pub const ERR_VALIDATION: &str = "validation_error";
pub const ERR_UNSUPPORTED: &str = "unsupported_operation";
pub const ERR_REVISION_CONFLICT: &str = "revision_conflict";
pub const ERR_SESSION_BUSY: &str = "session_busy";
pub const ERR_INVALID_GROUP_STATE: &str = "invalid_group_state";
pub const ERR_GROUP_FAILED: &str = "group_failed";
pub const ERR_PAYLOAD_TOO_LARGE: &str = "payload_too_large";
pub const ERR_STORAGE: &str = "storage_error";
pub const ERR_RENDER: &str = "render_error";
pub const ERR_RENDER_CANCELLED: &str = "render_cancelled";
pub const ERR_INTERNAL: &str = "internal_error";

/// Contract version advertised by [`capabilities`].
pub const CONTRACT_VERSION: &str = "2-draft3";
/// Wire-schema versions, bumped only with an explicit migration.
pub const COMMAND_SCHEMA_VERSION: u32 = 3;
pub const SNAPSHOT_SCHEMA_VERSION: u32 = 4;
pub const ID_SCHEMA_VERSION: u32 = 1;

/// Shared minimum guarantee: 1 MiB is what every adapter must accept.
///
/// A 10k-point object-form stroke measured ~274 KB, so this leaves room for
/// dense freehand input while staying far below the native server's default
/// 2 MiB request-body limit.
pub const MAX_COMMAND_BYTES: usize = 1_048_576;
/// Shared minimum guarantee for one `curve.apply_stroke`.
pub const MAX_STROKE_POINTS: usize = 20_000;

/// Declared limits; adapters must never advertise more than they enforce.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Limits {
    pub max_command_bytes: usize,
    pub max_stroke_points: usize,
}

/// What this contract version actually supports (§6.15).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Capabilities {
    pub contract_version: &'static str,
    pub command_schema_version: u32,
    pub snapshot_schema_version: u32,
    pub id_schema_version: u32,
    /// Only features that work today are listed — a client must never have to
    /// probe for support by triggering an error.
    pub features: Vec<&'static str>,
    pub limits: Limits,
    pub render: RenderCapabilities,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RenderCapabilities {
    /// Render jobs are not implemented in the session yet (§6.13).
    pub async_jobs: bool,
    pub per_job_cancel: bool,
    pub progress_transports: Vec<&'static str>,
}

/// The capability descriptor for this build.
pub fn capabilities() -> Capabilities {
    Capabilities {
        contract_version: CONTRACT_VERSION,
        command_schema_version: COMMAND_SCHEMA_VERSION,
        snapshot_schema_version: SNAPSHOT_SCHEMA_VERSION,
        id_schema_version: ID_SCHEMA_VERSION,
        features: vec![
            "stable_ids",
            "partial_reads",
            "group_transactions",
            "curve_apply_stroke",
            "curve_clear",
            "track_mix_fx",
            "note_vibrato",
            "project.controls.v1",
        ],
        limits: Limits {
            max_command_bytes: MAX_COMMAND_BYTES,
            max_stroke_points: MAX_STROKE_POINTS,
        },
        render: RenderCapabilities {
            async_jobs: false,
            per_job_cancel: false,
            progress_transports: Vec::new(),
        },
    }
}

/// A typed session failure.
///
/// Frontends branch on [`SessionError::code`]; `message` is for display and
/// may be localized later. Codes are frozen by §6.14 — adapters must not invent
/// their own.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Value::is_null")]
    pub details: Value,
}

impl SessionError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: Value::Null,
        }
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }

    /// An ID that does not exist in this session.
    pub fn not_found(kind: &str, id: &str) -> Self {
        Self::new(ERR_NOT_FOUND, format!("{kind} '{id}' not found"))
            .with_details(json!({"kind": kind, "id": id}))
    }

    /// User-correctable input problem.
    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(ERR_VALIDATION, message)
    }

    /// User-correctable problem about one named field.
    pub fn validation_field(field: &str, message: impl Into<String>) -> Self {
        Self::validation(message).with_details(json!({"field": field}))
    }

    /// A command `type` that is not part of this contract version.
    pub fn validation_unknown_type(command_type: &str) -> Self {
        Self::validation_field("type", format!("unknown command type '{command_type}'"))
            .with_details(json!({"field": "type", "type": command_type}))
    }

    /// A validation failure with a stable machine-readable `reason`.
    ///
    /// The frozen code set (§6.14) stays small on purpose; the finer-grained
    /// contract reasons (`tick_out_of_range`, `abbr_mismatch`, ...) travel in
    /// `details.reason` so a client can branch without parsing prose.
    pub fn validation_reason(field: &str, reason: &str, message: impl Into<String>) -> Self {
        Self::validation(message).with_details(json!({"field": field, "reason": reason}))
    }

    /// The request or command exceeds a declared capability limit (§6.15).
    pub fn payload_too_large(limit: &str, max: usize) -> Self {
        Self::new(
            ERR_PAYLOAD_TOO_LARGE,
            format!("payload exceeds the declared {limit} of {max}"),
        )
        .with_details(json!({"limit": limit, "max": max}))
    }

    /// Known command that v2 does not implement yet.
    pub fn unsupported(command_type: &str) -> Self {
        Self::new(
            ERR_UNSUPPORTED,
            format!("'{command_type}' is not supported by this contract version"),
        )
        .with_details(json!({"type": command_type}))
    }

    /// `expected_revision` does not match the committed revision.
    pub fn revision_conflict(expected: u64, current: u64) -> Self {
        Self::new(
            ERR_REVISION_CONFLICT,
            format!("expected revision {expected} but the session is at {current}"),
        )
        .with_details(json!({"expected_revision": expected, "current_revision": current}))
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ERR_INTERNAL, message)
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn is_conflict(&self) -> bool {
        self.code == ERR_REVISION_CONFLICT
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            format!(
                r#"{{"code":"{}","message":"serialization failed"}}"#,
                ERR_INTERNAL
            )
        })
    }
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for SessionError {}

// ---------------------------------------------------------------------------
// Change vocabulary (§6.4.3)
// ---------------------------------------------------------------------------

pub const AUTHORITY_PATCH: &str = "patch";
pub const AUTHORITY_INVALIDATE: &str = "invalidate";

/// One authoritative change/invalidation entry.
///
/// `authority` is the only thing a client needs to decide whether it may patch
/// its disposable cache: [`AUTHORITY_PATCH`] entries always carry complete
/// canonical `data`, [`AUTHORITY_INVALIDATE`] entries never do.
/// [`Change::is_well_formed`] asserts that rule.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Change {
    pub kind: String,
    pub authority: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub curve_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abbr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_tick: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_tick: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl Default for Change {
    fn default() -> Self {
        Self {
            kind: String::new(),
            authority: AUTHORITY_INVALIDATE,
            track_id: None,
            part_id: None,
            note_id: None,
            curve_id: None,
            abbr: None,
            start_tick: None,
            end_tick: None,
            data: None,
            reason: None,
        }
    }
}

impl Change {
    /// A complete canonical payload for the changed entity.
    pub fn patch(kind: impl Into<String>, data: Value) -> Self {
        Self {
            kind: kind.into(),
            authority: AUTHORITY_PATCH,
            data: Some(data),
            ..Default::default()
        }
    }

    /// "This read model is stale" without a payload.
    pub fn invalidate(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            ..Default::default()
        }
    }

    pub fn with_track(mut self, id: impl Into<String>) -> Self {
        self.track_id = Some(id.into());
        self
    }

    pub fn with_part(mut self, id: impl Into<String>) -> Self {
        self.part_id = Some(id.into());
        self
    }

    pub fn with_note(mut self, id: impl Into<String>) -> Self {
        self.note_id = Some(id.into());
        self
    }

    pub fn with_curve(mut self, id: impl Into<String>, abbr: impl Into<String>) -> Self {
        self.curve_id = Some(id.into());
        self.abbr = Some(abbr.into());
        self
    }

    /// Project-tick span affected by the change.
    pub fn with_tick_range(mut self, start: i32, end: i32) -> Self {
        self.start_tick = Some(start);
        self.end_tick = Some(end);
        self
    }

    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// `patch` ⇒ data present; `invalidate` ⇒ data absent. No third state.
    pub fn is_well_formed(&self) -> bool {
        match self.authority {
            AUTHORITY_PATCH => self.data.is_some(),
            AUTHORITY_INVALIDATE => self.data.is_none(),
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Mutation result (§6.4.2)
// ---------------------------------------------------------------------------

/// Result of a committed v2 mutation.
///
/// Always carries `revision` plus the authoritative change information; a full
/// `snapshot` is only present when the caller asked for it (or when an adapter
/// chooses to include one).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MutationResult {
    pub revision: u64,
    pub command_name: String,
    pub changed: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub changes: Vec<Change>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<Value>,
    /// Echoed for a group child/commit so a client can correlate the result
    /// with its open transaction (§6.8). Absent for ordinary mutations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
}

impl MutationResult {
    pub fn to_json(&self) -> Result<String, SessionError> {
        serde_json::to_string(self).map_err(|e| SessionError::internal(e.to_string()))
    }

    /// Every entry satisfies the `authority`/`data` rule.
    pub fn changes_are_well_formed(&self) -> bool {
        self.changes.iter().all(Change::is_well_formed)
    }
}

// ---------------------------------------------------------------------------
// Request envelope (§6.4.1)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Request {
    pub expected_revision: u64,
    pub command: Value,
    pub include_snapshot: bool,
    /// Set when the command belongs to an open group transaction (§6.8).
    pub group_id: Option<String>,
}

/// Parse `{expected_revision, command, include_snapshot?}`.
///
/// `expected_revision` is **required** for state-changing operations (§6.9):
/// a client that omits it gets `validation_error` rather than last-write-wins.
pub fn parse_request(json_str: &str) -> Result<Request, SessionError> {
    let value: Value = serde_json::from_str(json_str)
        .map_err(|e| SessionError::validation(format!("invalid JSON request: {e}")))?;
    let object = value
        .as_object()
        .ok_or_else(|| SessionError::validation("request must be a JSON object"))?;
    let expected_revision = object
        .get("expected_revision")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            SessionError::validation_field(
                "expected_revision",
                "expected_revision is required and must be a non-negative integer",
            )
        })?;
    let command = object
        .get("command")
        .cloned()
        .ok_or_else(|| SessionError::validation_field("command", "command is required"))?;
    if !command.is_object() {
        return Err(SessionError::validation_field(
            "command",
            "command must be a JSON object",
        ));
    }
    let include_snapshot = object
        .get("include_snapshot")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let group_id = object
        .get("group_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok(Request {
        expected_revision,
        command,
        include_snapshot,
        group_id,
    })
}

// ---------------------------------------------------------------------------
// Locations and planning
// ---------------------------------------------------------------------------

/// A resolved voice part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocatedPart {
    pub track: usize,
    pub part_in_track: usize,
    /// Index into `UProject.parts` (what the engine commands expect).
    pub flat_index: usize,
    pub track_id: String,
    pub part_id: String,
}

/// A resolved note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocatedNote {
    pub part: LocatedPart,
    pub note_index: usize,
    pub note_id: String,
}

/// A resolved curve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocatedCurve {
    pub part: LocatedPart,
    pub curve_index: usize,
    pub curve_id: String,
    pub abbr: String,
}

/// A resolved track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocatedTrack {
    pub track: usize,
    pub track_id: String,
}

fn flat_index_of_voice_part(
    project: &UProject,
    track: usize,
    part_in_track: usize,
) -> Option<usize> {
    let target = crate::identity::parts_of_track(project, track)
        .get(part_in_track)
        .copied()?;
    project
        .parts
        .iter()
        .position(|part| std::ptr::eq(part, target))
}

fn track_id_string(ids: &IdentityTable, track: usize) -> Result<String, SessionError> {
    ids.track_id(track)
        .map(|id| id.as_str().to_string())
        .ok_or_else(|| SessionError::internal(format!("no track id for index {track}")))
}

fn part_id_string(ids: &IdentityTable, track: usize, part: usize) -> Result<String, SessionError> {
    ids.part_id(track, part)
        .map(|id| id.as_str().to_string())
        .ok_or_else(|| SessionError::internal(format!("no part id for track {track} part {part}")))
}

/// Resolve a part id to document positions.
pub fn locate_part(
    ids: &IdentityTable,
    project: &UProject,
    part_id: &str,
) -> Result<LocatedPart, SessionError> {
    let (track, part_in_track) = ids
        .find_part(part_id)
        .ok_or_else(|| SessionError::not_found("part", part_id))?;
    let flat_index = flat_index_of_voice_part(project, track, part_in_track).ok_or_else(|| {
        SessionError::internal(format!("part id {part_id} has no document position"))
    })?;
    Ok(LocatedPart {
        track,
        part_in_track,
        flat_index,
        track_id: track_id_string(ids, track)?,
        part_id: part_id.to_string(),
    })
}

/// Resolve a note id to document positions.
pub fn locate_note(
    ids: &IdentityTable,
    project: &UProject,
    note_id: &str,
) -> Result<LocatedNote, SessionError> {
    let (track, part_in_track, note_index) = ids
        .find_note(note_id)
        .ok_or_else(|| SessionError::not_found("note", note_id))?;
    let part = locate_part(ids, project, &part_id_string(ids, track, part_in_track)?)?;
    Ok(LocatedNote {
        part,
        note_index,
        note_id: note_id.to_string(),
    })
}

/// Resolve a curve id to document positions.
pub fn locate_curve(
    ids: &IdentityTable,
    project: &UProject,
    curve_id: &str,
) -> Result<LocatedCurve, SessionError> {
    let (track, part_in_track, curve_index) = ids
        .find_curve(curve_id)
        .ok_or_else(|| SessionError::not_found("curve", curve_id))?;
    let part = locate_part(ids, project, &part_id_string(ids, track, part_in_track)?)?;
    let abbr = ids
        .tracks
        .get(track)
        .and_then(|t| t.parts.get(part_in_track))
        .and_then(|p| p.curves.get(curve_index))
        .map(|c| c.abbr.clone())
        .ok_or_else(|| SessionError::internal(format!("no curve abbr for {curve_id}")))?;
    Ok(LocatedCurve {
        part,
        curve_index,
        curve_id: curve_id.to_string(),
        abbr,
    })
}

/// Resolve a track id.
pub fn locate_track(ids: &IdentityTable, track_id: &str) -> Result<LocatedTrack, SessionError> {
    let track = ids
        .find_track(track_id)
        .ok_or_else(|| SessionError::not_found("track", track_id))?;
    Ok(LocatedTrack {
        track,
        track_id: track_id.to_string(),
    })
}

/// A v2 command resolved into the engine's index-addressed form.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// The v2 command type.
    pub kind: String,
    /// Legacy JSON handed to `ProjectDoc::apply` (same validation/undo path).
    pub legacy_command: String,
    pub target: Target,
    pub creates_note: bool,
    pub removes_note: bool,
}

/// What the command addressed (used to build `changes`).
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Note {
        note_id: String,
        part_id: String,
        track_id: String,
    },
    Part {
        part_id: String,
        track_id: String,
    },
    Track {
        track_id: String,
        track: usize,
    },
    Curve {
        track: usize,
        /// Index of the part within its track (the identity table's layout).
        part_index: usize,
        /// Index of the curve within the part after the edit.
        curve_index: usize,
        /// The curve's id, or `None` when the stroke created it.
        curve_id: Option<String>,
        abbr: String,
    },
}

fn field<'a>(command: &'a Value, key: &str) -> Option<&'a Value> {
    command.get(key).filter(|v| !v.is_null())
}

fn require_str(command: &Value, key: &str) -> Result<String, SessionError> {
    field(command, key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| {
            SessionError::validation_field(key, format!("'{key}' is required and must be a string"))
        })
}

fn optional_str(command: &Value, key: &str) -> Option<String> {
    field(command, key)
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn require_i32(command: &Value, key: &str) -> Result<i32, SessionError> {
    let value = field(command, key).and_then(Value::as_i64).ok_or_else(|| {
        SessionError::validation_field(key, format!("'{key}' is required and must be an integer"))
    })?;
    i32::try_from(value).map_err(|_| {
        SessionError::validation_field(key, format!("'{key}' is out of range for a tick value"))
    })
}

fn optional_i32(command: &Value, key: &str, default: i32) -> Result<i32, SessionError> {
    match field(command, key) {
        Some(_) => require_i32(command, key),
        None => Ok(default),
    }
}

fn require_f64(command: &Value, key: &str) -> Result<f64, SessionError> {
    field(command, key).and_then(Value::as_f64).ok_or_else(|| {
        SessionError::validation_field(key, format!("'{key}' is required and must be a number"))
    })
}

fn require_bool(command: &Value, key: &str) -> Result<bool, SessionError> {
    field(command, key).and_then(Value::as_bool).ok_or_else(|| {
        SessionError::validation_field(key, format!("'{key}' is required and must be a boolean"))
    })
}

fn scale_tick(value: i32, factor: f64, field_name: &str) -> Result<i32, SessionError> {
    let scaled = (value as f64 * factor).round();
    if !scaled.is_finite() || scaled < i32::MIN as f64 || scaled > i32::MAX as f64 {
        return Err(SessionError::validation_field(
            field_name,
            "preserved-timing remap exceeds the supported tick range",
        ));
    }
    Ok(scaled as i32)
}

/// A validated project/part mutation prepared against a cloned project. The
/// live document is replaced only after every invariant has passed.
#[derive(Debug, Clone)]
pub struct DocumentPlan {
    pub kind: String,
    pub command_name: String,
    pub after: UProject,
    pub track_id: Option<String>,
    pub part_id: Option<String>,
    pub note_id: Option<String>,
    pub part_index: Option<usize>,
    pub right_part_index: Option<usize>,
    pub render_invalidated: bool,
}

/// Build the change vocabulary for an already committed structural command.
/// Structural views use invalidation until the bounded project/part read
/// models are consumed by every host adapter.
pub fn document_changes(
    plan: &DocumentPlan,
    ids: &IdentityTable,
    project: &UProject,
) -> Vec<Change> {
    let mut changes = Vec::new();
    let note_range = plan.note_id.as_deref().and_then(|note_id| {
        let located = locate_note(ids, project, note_id).ok()?;
        let part = voice_part_at(project, located.part.track, located.part.part_in_track)?;
        let note = part.notes.get(located.note_index)?;
        Some((
            part.position + note.position,
            part.position + note.position + note.duration,
        ))
    });
    if let Some(note_id) = &plan.note_id {
        if let Ok(located) = locate_note(ids, project, note_id) {
            if let Some(part) =
                voice_part_at(project, located.part.track, located.part.part_in_track)
            {
                if let Some(note) = part.notes.get(located.note_index) {
                    let change =
                        Change::patch("note.changed", note_payload(note_id, note, project))
                            .with_track(located.part.track_id)
                            .with_part(located.part.part_id)
                            .with_note(note_id.clone());
                    changes.push(if let Some((start, end)) = note_range {
                        change.with_tick_range(start, end)
                    } else {
                        change
                    });
                }
            }
        }
    } else if plan.kind.starts_with("project.") {
        changes.push(Change::patch(
            "project.changed",
            views::project_meta_view(ids.project_id.as_str(), project),
        ));
    } else if plan.kind == "track.set_singer" {
        if let Some(track_id) = &plan.track_id {
            changes.push(Change::invalidate("track.changed").with_track(track_id.clone()));
        }
    } else if plan.kind.starts_with("track.") {
        changes.push(Change::invalidate("tracks.changed"));
    } else if plan.kind == "part.create" || plan.kind == "part.import_audio" {
        let part_id = plan
            .part_index
            .and_then(|index| {
                plan.track_id.as_deref().and_then(|track| {
                    ids.find_track(track)
                        .and_then(|track_index| ids.part_id(track_index, index))
                })
            })
            .map(|id| id.as_str().to_string());
        let mut change = Change::invalidate("part.added");
        if let Some(track_id) = &plan.track_id {
            change = change.with_track(track_id.clone());
        }
        if let Some(part_id) = &part_id {
            change = change.with_part(part_id.clone());
        }
        if let (Some(track_id), Some(part_id), Some(index)) =
            (&plan.track_id, part_id, plan.part_index)
        {
            if let Some(data) = part_payload(ids, project, track_id, index) {
                changes.push(
                    Change::patch("part.added", data)
                        .with_track(track_id.clone())
                        .with_part(part_id),
                );
            } else {
                changes.push(change);
            }
        } else {
            changes.push(change);
        }
    } else if plan.kind == "part.remove" {
        let mut change = Change::invalidate("part.removed");
        if let Some(track_id) = &plan.track_id {
            change = change.with_track(track_id.clone());
        }
        if let Some(part_id) = &plan.part_id {
            change = change.with_part(part_id.clone());
        }
        changes.push(change);
    } else if plan.kind == "part.split" {
        let mut left = Change::invalidate("part.changed");
        if let Some(track_id) = &plan.track_id {
            left = left.with_track(track_id.clone());
        }
        if let Some(part_id) = &plan.part_id {
            left = left.with_part(part_id.clone());
        }
        if let (Some(track_id), Some(part_id), Some(index)) =
            (&plan.track_id, &plan.part_id, plan.part_index)
        {
            if let Some(data) = part_payload(ids, project, track_id, index) {
                changes.push(
                    Change::patch("part.changed", data)
                        .with_track(track_id.clone())
                        .with_part(part_id.clone()),
                );
            } else {
                changes.push(left);
            }
        } else {
            changes.push(left);
        }
        let right_id = plan
            .right_part_index
            .and_then(|index| {
                plan.track_id.as_deref().and_then(|track| {
                    ids.find_track(track)
                        .and_then(|track_index| ids.part_id(track_index, index))
                })
            })
            .map(|id| id.as_str().to_string());
        let mut right = Change::invalidate("part.added");
        if let Some(track_id) = &plan.track_id {
            right = right.with_track(track_id.clone());
        }
        if let Some(part_id) = &right_id {
            right = right.with_part(part_id.clone());
        }
        if let (Some(track_id), Some(part_id), Some(index)) =
            (&plan.track_id, right_id, plan.right_part_index)
        {
            if let Some(data) = part_payload(ids, project, track_id, index) {
                changes.push(
                    Change::patch("part.added", data)
                        .with_track(track_id.clone())
                        .with_part(part_id),
                );
            } else {
                changes.push(right);
            }
        } else {
            changes.push(right);
        }
    } else {
        let mut change = Change::invalidate("part.changed");
        if let Some(track_id) = &plan.track_id {
            change = change.with_track(track_id.clone());
        }
        if let Some(part_id) = &plan.part_id {
            change = change.with_part(part_id.clone());
        }
        if let (Some(track_id), Some(part_id), Some(index)) =
            (&plan.track_id, &plan.part_id, plan.part_index)
        {
            if let Some(data) = part_payload(ids, project, track_id, index) {
                changes.push(
                    Change::patch("part.changed", data)
                        .with_track(track_id.clone())
                        .with_part(part_id.clone()),
                );
            } else {
                changes.push(change);
            }
        } else {
            changes.push(change);
        }
    }
    if plan.render_invalidated {
        let change =
            Change::invalidate("render.invalidated").with_reason("project or part changed");
        changes.push(if let Some((start, end)) = note_range {
            change.with_tick_range(start, end)
        } else {
            change
        });
    }
    changes
}

pub(crate) fn part_payload(
    ids: &IdentityTable,
    project: &UProject,
    track_id: &str,
    part_index: usize,
) -> Option<Value> {
    let track = ids.find_track(track_id)?;
    let project_part = crate::identity::parts_of_track(project, track)
        .get(part_index)
        .copied()?;
    let id = ids.part_id(track, part_index)?.as_str();
    let UPart::Voice(part) = project_part else {
        if let UPart::Wave(wave) = project_part {
            return serde_json::to_value(views::wave_part_view(id, wave, project)).ok();
        }
        unreachable!()
    };
    let notes = part
        .notes
        .iter()
        .enumerate()
        .map(|(i, note)| {
            let id = ids.note_id(track, part_index, i)?.as_str().to_string();
            Some(views::note_view(
                &id,
                note,
                project.rusted_utau_pronunciations.get(&id),
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    let curves = part
        .curves
        .iter()
        .enumerate()
        .map(|(i, curve)| {
            let id = ids.curve_id(track, part_index, i)?.as_str().to_string();
            Some(views::curve_view(&id, curve))
        })
        .collect::<Option<Vec<_>>>()?;
    serde_json::to_value(views::part_view(id, part, notes, curves)).ok()
}

/// Plan frozen §6.4.4 document commands without mutating the live project.
/// Existing index-based editor commands remain a separate compatibility API.
pub fn plan_document(
    ids: &IdentityTable,
    project: &UProject,
    command: &Value,
) -> Result<Option<DocumentPlan>, SessionError> {
    let Some(kind) = command.get("type").and_then(Value::as_str) else {
        return Ok(None);
    };
    // Route only the document commands implemented by this planner. The
    // stable-ID command planner still owns the existing track mixer/name
    // operations and part/note edits; intercepting their whole namespaces
    // here would turn supported commands into `unsupported_operation`.
    if !matches!(
        kind,
        "track.create"
            | "track.set_singer"
            | "track.remove"
            | "track.reorder"
            | "part.create"
            | "part.import_audio"
            | "part.remove"
            | "part.move"
            | "part.resize"
            | "part.set_metadata"
            | "part.set_media_ref"
            | "part.split"
            | "note.set_lyric"
            | "note.set_pronunciation"
            | "note.clear_phoneme_data"
            | "project.set_metadata"
            | "project.import_tracks"
            | "project.set_tempo"
            | "project.remove_tempo"
            | "project.preserve_timing"
            | "project.set_time_signature"
            | "project.remove_time_signature"
    ) {
        return Ok(None);
    }
    let mut after = project.clone();
    let mut plan = DocumentPlan {
        kind: kind.to_string(),
        command_name: kind.replace('.', " "),
        after: project.clone(),
        track_id: None,
        part_id: None,
        note_id: None,
        part_index: None,
        right_part_index: None,
        render_invalidated: true,
    };
    match kind {
        "track.create" => {
            let name = require_str(command, "name")?;
            if name.trim().is_empty() {
                return Err(SessionError::validation_field(
                    "name",
                    "name must not be empty",
                ));
            }
            let index = optional_i32(command, "to_index", after.tracks.len() as i32)?;
            if index < 0 || index as usize > after.tracks.len() {
                return Err(SessionError::validation_field(
                    "to_index",
                    "track insertion index is out of range",
                ));
            }
            let index = index as usize;
            after.tracks.insert(index, domain::UTrack::new(name));
            for part in &mut after.parts {
                if part.track_no() >= index as i32 {
                    match part {
                        UPart::Voice(value) => value.track_no += 1,
                        UPart::Wave(value) => value.track_no += 1,
                    }
                }
            }
            plan.command_name = "Add track".to_string();
            plan.render_invalidated = false;
        }
        "track.set_singer" => {
            let track_id = require_str(command, "track_id")?;
            let located = locate_track(ids, &track_id)?;
            let singer = match command.get("singer_id") {
                None | Some(Value::Null) => None,
                Some(value) => {
                    let name = value.as_str().ok_or_else(|| {
                        SessionError::validation_field(
                            "singer_id",
                            "singer_id must be a string or null",
                        )
                    })?;
                    if name.trim().is_empty() {
                        None
                    } else {
                        Some(name.to_string())
                    }
                }
            };
            after.tracks[located.track].singer = singer;
            plan.track_id = Some(track_id);
            plan.command_name = "Track singer".to_string();
        }
        "track.remove" => {
            let track_id = require_str(command, "track_id")?;
            let located = locate_track(ids, &track_id)?;
            if after.tracks.len() <= 1 {
                return Err(SessionError::validation_field(
                    "track_id",
                    "the last project track cannot be removed",
                ));
            }
            let removed = located.track as i32;
            after.tracks.remove(located.track);
            after.parts.retain(|part| part.track_no() != removed);
            for part in &mut after.parts {
                if part.track_no() > removed {
                    match part {
                        UPart::Voice(value) => value.track_no -= 1,
                        UPart::Wave(value) => value.track_no -= 1,
                    }
                }
            }
            plan.track_id = Some(track_id);
            plan.command_name = "Remove track".to_string();
        }
        "track.reorder" => {
            let track_id = require_str(command, "track_id")?;
            let located = locate_track(ids, &track_id)?;
            let to_index = require_i32(command, "to_index")?;
            if to_index < 0 || to_index as usize >= after.tracks.len() {
                return Err(SessionError::validation_field(
                    "to_index",
                    "track destination index is out of range",
                ));
            }
            let from = located.track;
            let to = to_index as usize;
            plan.track_id = Some(track_id);
            if from != to {
                let track = after.tracks.remove(from);
                after.tracks.insert(to, track);
                for part in &mut after.parts {
                    let current = part.track_no();
                    let next = if current == from as i32 {
                        to as i32
                    } else if from < to && (from as i32) < current && current <= to as i32 {
                        current - 1
                    } else if to < from && to as i32 <= current && current < from as i32 {
                        current + 1
                    } else {
                        current
                    };
                    match part {
                        UPart::Voice(value) => value.track_no = next,
                        UPart::Wave(value) => value.track_no = next,
                    }
                }
            }
            plan.command_name = "Reorder track".to_string();
        }
        "note.set_lyric" | "note.set_pronunciation" | "note.clear_phoneme_data" => {
            let note_id = require_str(command, "note_id")?;
            let located = locate_note(ids, project, &note_id)?;
            let Some(UPart::Voice(part)) = after.parts.get_mut(located.part.flat_index) else {
                return Err(SessionError::not_found("note", &note_id));
            };
            let Some(note) = part.notes.get_mut(located.note_index) else {
                return Err(SessionError::not_found("note", &note_id));
            };
            plan.track_id = Some(located.part.track_id);
            plan.part_id = Some(located.part.part_id);
            plan.note_id = Some(note_id.clone());
            let has_phoneme_data =
                !note.phoneme_expressions.is_empty() || !note.phoneme_overrides.is_empty();
            if kind == "note.clear_phoneme_data" {
                note.phoneme_expressions.clear();
                note.phoneme_overrides.clear();
                plan.command_name = "Clear per-phoneme data".to_string();
            } else if kind == "note.set_lyric" {
                let lyric = require_str(command, "lyric")?;
                let resets_pronunciation = after.rusted_utau_pronunciations.contains_key(&note_id);
                if has_phoneme_data && (note.lyric != lyric || resets_pronunciation) {
                    return Err(SessionError::validation_reason(
                        "lyric",
                        "phoneme_data_requires_clear",
                        "clear per-phoneme expressions and overrides before changing lyric or pronunciation",
                    ));
                }
                note.lyric = lyric;
                after.rusted_utau_pronunciations.remove(&note_id);
                plan.command_name = "Set lyric and reset pronunciation".to_string();
            } else {
                let value = command.get("pronunciation").ok_or_else(|| {
                    SessionError::validation_field(
                        "pronunciation",
                        "pronunciation is required; use null to clear",
                    )
                })?;
                if value.is_null() {
                    if has_phoneme_data && after.rusted_utau_pronunciations.contains_key(&note_id) {
                        return Err(SessionError::validation_reason(
                            "pronunciation",
                            "phoneme_data_requires_clear",
                            "clear per-phoneme expressions and overrides before changing lyric or pronunciation",
                        ));
                    }
                    after.rusted_utau_pronunciations.remove(&note_id);
                    plan.command_name = "Clear note pronunciation".to_string();
                } else {
                    let pronunciation: domain::UPronunciation =
                        serde_json::from_value(value.clone()).map_err(|error| {
                            SessionError::validation_field(
                                "pronunciation",
                                format!("invalid pronunciation object: {error}"),
                            )
                        })?;
                    if !valid_pronunciation(&pronunciation) {
                        return Err(SessionError::validation_field(
                            "pronunciation",
                            "language/mode/style/phones do not meet the pronunciation limits",
                        ));
                    }
                    if has_phoneme_data
                        && after.rusted_utau_pronunciations.get(&note_id) != Some(&pronunciation)
                    {
                        return Err(SessionError::validation_reason(
                            "pronunciation",
                            "phoneme_data_requires_clear",
                            "clear per-phoneme expressions and overrides before changing lyric or pronunciation",
                        ));
                    }
                    after.rusted_utau_pronunciations.insert(note_id, pronunciation);
                    plan.command_name = "Set note pronunciation".to_string();
                }
            }
            plan.render_invalidated = true;
        }
        "project.set_metadata" => {
            let fields = command
                .get("fields")
                .and_then(Value::as_object)
                .ok_or_else(|| {
                    SessionError::validation_field("fields", "fields must be an object")
                })?;
            if fields.is_empty() {
                return Err(SessionError::validation_field(
                    "fields",
                    "at least one field is required",
                ));
            }
            for (key, value) in fields {
                match key.as_str() {
                    "name" => {
                        let value = value.as_str().ok_or_else(|| {
                            SessionError::validation_field("name", "name must be a string")
                        })?;
                        if value.trim().is_empty() {
                            return Err(SessionError::validation_field(
                                "name",
                                "name must not be empty",
                            ));
                        }
                        after.name = value.to_string();
                    }
                    "comment" => {
                        after.comment = value
                            .as_str()
                            .ok_or_else(|| {
                                SessionError::validation_field(
                                    "comment",
                                    "comment must be a string",
                                )
                            })?
                            .to_string()
                    }
                    "key" => {
                        let key = value
                            .as_i64()
                            .and_then(|v| i32::try_from(v).ok())
                            .ok_or_else(|| {
                                SessionError::validation_field(
                                    "key",
                                    "key must be an integer from 0 to 11",
                                )
                            })?;
                        if !(0..=11).contains(&key) {
                            return Err(SessionError::validation_field(
                                "key",
                                "key must be an integer from 0 to 11",
                            ));
                        }
                        after.key = key;
                    }
                    _ => {
                        return Err(SessionError::validation_field(
                            "fields",
                            format!("unknown project metadata field '{key}'"),
                        ))
                    }
                }
            }
            plan.render_invalidated = false;
        }
        "project.import_tracks" => {
            let yaml = require_str(command, "ustx_yaml")?;
            let incoming = domain::UProject::parse_yaml(&yaml).map_err(|error| {
                SessionError::validation_field("ustx_yaml", format!("invalid USTX: {error}"))
            })?;
            let offset = after.tracks.len();
            if offset.saturating_add(incoming.tracks.len()) > i32::MAX as usize {
                return Err(SessionError::validation_field(
                    "ustx_yaml",
                    "too many tracks",
                ));
            }
            for (abbr, descriptor) in incoming.expressions {
                if let Some(existing) = after.expressions.get(&abbr) {
                    if existing != &descriptor {
                        return Err(SessionError::validation_field(
                            "ustx_yaml",
                            format!("expression '{}' conflicts with the open project", abbr),
                        ));
                    }
                } else {
                    after.expressions.insert(abbr, descriptor);
                }
            }
            after.tracks.extend(incoming.tracks);
            for mut part in incoming.parts {
                let shifted = part.track_no().saturating_add(offset as i32);
                match &mut part {
                    UPart::Voice(value) => value.track_no = shifted,
                    UPart::Wave(value) => value.track_no = shifted,
                }
                let insertion = after
                    .parts
                    .iter()
                    .position(|existing| {
                        (existing.track_no(), existing.position()) > (shifted, part.position())
                    })
                    .unwrap_or(after.parts.len());
                after.parts.insert(insertion, part);
            }
            plan.command_name = "Import tracks".to_string();
        }
        "project.set_tempo" => {
            let position = require_i32(command, "position_tick")?;
            let bpm = require_f64(command, "bpm")?;
            if position < 0 {
                return Err(SessionError::validation_reason(
                    "position_tick",
                    "tick_out_of_range",
                    "tempo position must be non-negative",
                ));
            }
            if !bpm.is_finite() || bpm <= 0.0 {
                return Err(SessionError::validation_field(
                    "bpm",
                    "bpm must be finite and positive",
                ));
            }
            if let Some(existing) = after
                .tempos
                .iter_mut()
                .find(|tempo| tempo.position == position)
            {
                existing.bpm = bpm;
            } else {
                after.tempos.push(UTempo::new(position, bpm));
            }
        }
        "project.preserve_timing" => {
            let new_bpm = require_f64(command, "bpm")?;
            if !new_bpm.is_finite() || new_bpm <= 0.0 {
                return Err(SessionError::validation_field(
                    "bpm",
                    "bpm must be finite and positive",
                ));
            }
            let old_bpm = after
                .tempos
                .iter()
                .find(|tempo| tempo.position == 0)
                .map(|tempo| tempo.bpm)
                .unwrap_or(after.bpm);
            if !old_bpm.is_finite() || old_bpm <= 0.0 {
                return Err(SessionError::validation_field(
                    "bpm",
                    "the project must have a finite positive origin tempo",
                ));
            }
            let bpm_factor = new_bpm / old_bpm;
            // Keep elapsed time fixed: tick positions scale with tempo, so a
            // slower tempo moves musical events earlier in tick space.
            let tick_factor = bpm_factor;
            for tempo in &mut after.tempos {
                tempo.position = scale_tick(tempo.position, tick_factor, "tempos")?;
                tempo.bpm *= bpm_factor;
                if !tempo.bpm.is_finite() || tempo.bpm <= 0.0 {
                    return Err(SessionError::validation_field(
                        "bpm",
                        "tempo remap produced a non-finite tempo",
                    ));
                }
            }
            after.tempos.sort_by_key(|tempo| tempo.position);
            if after
                .tempos
                .windows(2)
                .any(|pair| pair[0].position == pair[1].position)
            {
                return Err(SessionError::validation_field(
                    "tempos",
                    "tempo markers collide after tick rounding",
                ));
            }
            if let Some(origin) = after.tempos.iter_mut().find(|tempo| tempo.position == 0) {
                origin.bpm = new_bpm;
            } else {
                after.tempos.push(UTempo::new(0, new_bpm));
                after.tempos.sort_by_key(|tempo| tempo.position);
            }
            after.bpm = new_bpm;
            for part in &mut after.parts {
                match part {
                    UPart::Voice(voice) => {
                        voice.position = scale_tick(voice.position, tick_factor, "part.position")?;
                        voice.duration =
                            scale_tick(voice.duration, tick_factor, "part.duration")?.max(1);
                        for note in &mut voice.notes {
                            note.position =
                                scale_tick(note.position, tick_factor, "note.position")?;
                            note.duration =
                                scale_tick(note.duration, tick_factor, "note.duration")?.max(1);
                        }
                        for curve in &mut voice.curves {
                            let mut previous = None;
                            for tick in &mut curve.xs {
                                *tick = scale_tick(*tick, tick_factor, "curve.points")?;
                                if previous.is_some_and(|prior| *tick <= prior) {
                                    return Err(SessionError::validation_field(
                                        "curve.points",
                                        "curve points collide after tick rounding",
                                    ));
                                }
                                previous = Some(*tick);
                            }
                        }
                    }
                    UPart::Wave(wave) => {
                        wave.position = scale_tick(wave.position, tick_factor, "part.position")?;
                    }
                }
            }
            plan.command_name = "Adjust tempo (preserve timing)".to_string();
        }
        "project.remove_tempo" => {
            let position = require_i32(command, "position_tick")?;
            if position <= 0 {
                return Err(SessionError::validation_field(
                    "position_tick",
                    "the tempo at tick zero cannot be removed",
                ));
            }
            let index = after
                .tempos
                .iter()
                .position(|tempo| tempo.position == position)
                .ok_or_else(|| SessionError::not_found("tempo", &position.to_string()))?;
            after.tempos.remove(index);
        }
        "project.set_time_signature" => {
            let bar = require_i32(command, "bar_position")?;
            let numerator = require_i32(command, "numerator")?;
            let denominator = require_i32(command, "denominator")?;
            if bar < 0 {
                return Err(SessionError::validation_field(
                    "bar_position",
                    "bar_position must be non-negative",
                ));
            }
            if numerator <= 0 || denominator <= 0 {
                return Err(SessionError::validation_field(
                    "numerator",
                    "numerator and denominator must be positive",
                ));
            }
            let ticks = 1920_i64.checked_mul(numerator as i64).ok_or_else(|| {
                SessionError::validation_field("numerator", "ticks per bar overflows")
            })?;
            if ticks % denominator as i64 != 0
                || ticks / denominator as i64 <= 0
                || ticks / denominator as i64 > i32::MAX as i64
            {
                return Err(SessionError::validation_field(
                    "denominator",
                    "ticks per bar must be a positive integer within tick range",
                ));
            }
            if let Some(existing) = after
                .time_signatures
                .iter_mut()
                .find(|sig| sig.bar_position == bar)
            {
                *existing = UTimeSignature::new(bar, numerator, denominator);
            } else {
                after
                    .time_signatures
                    .push(UTimeSignature::new(bar, numerator, denominator));
            }
        }
        "project.remove_time_signature" => {
            let bar = require_i32(command, "bar_position")?;
            if bar <= 0 {
                return Err(SessionError::validation_field(
                    "bar_position",
                    "the time signature at bar zero cannot be removed",
                ));
            }
            let index = after
                .time_signatures
                .iter()
                .position(|sig| sig.bar_position == bar)
                .ok_or_else(|| SessionError::not_found("time_signature", &bar.to_string()))?;
            after.time_signatures.remove(index);
        }
        "part.create" | "part.import_audio" => {
            let track_id = require_str(command, "track_id")?;
            let track_index = ids
                .find_track(&track_id)
                .ok_or_else(|| SessionError::not_found("track", &track_id))?;
            let is_audio_import = kind == "part.import_audio";
            let kind_value = if is_audio_import {
                "wave".to_string()
            } else {
                require_str(command, "kind")?
            };
            let name = require_str(command, "name")?;
            if name.trim().is_empty() {
                return Err(SessionError::validation_field(
                    "name",
                    "name must not be empty",
                ));
            }
            let position = require_i32(command, "position_ticks")?;
            let duration_ms = if is_audio_import {
                let value = require_f64(command, "duration_ms")?;
                if value <= 0.0 {
                    return Err(SessionError::validation_field(
                        "duration_ms",
                        "duration_ms must be positive",
                    ));
                }
                Some(value)
            } else {
                None
            };
            let duration = match duration_ms {
                Some(ms) => after.time_axis.ms_to_tick_at(ms, position),
                None => require_i32(command, "duration_ticks")?,
            };
            if position < 0 || duration <= 0 {
                return Err(SessionError::validation_field(
                    "position_ticks",
                    "part position must be non-negative and duration positive",
                ));
            }
            let part = match kind_value.as_str() {
                "voice" => UPart::Voice(UVoicePart {
                    name,
                    comment: optional_str(command, "comment").unwrap_or_default(),
                    track_no: track_index as i32,
                    position,
                    duration,
                    ..UVoicePart::default()
                }),
                "wave" => {
                    let media_ref = require_str(command, "media_ref")?;
                    if media_ref.trim().is_empty()
                        || media_ref.starts_with('/')
                        || media_ref.starts_with('\\')
                        || media_ref.as_bytes().get(1) == Some(&b':')
                        || media_ref.split('/').any(|segment| segment == "..")
                    {
                        return Err(SessionError::validation_field("media_ref", "media_ref must be a non-empty project-relative reference, not an absolute path"));
                    }
                    let start_ms = after.time_axis.tick_to_ms(position as f64);
                    let end_ms = after
                        .time_axis
                        .tick_to_ms(position.saturating_add(duration) as f64);
                    UPart::Wave(UWavePart {
                        name,
                        comment: optional_str(command, "comment").unwrap_or_default(),
                        track_no: track_index as i32,
                        position,
                        relative_path: Some(media_ref),
                        file_duration_ms: duration_ms.unwrap_or(end_ms - start_ms),
                        ..UWavePart::default()
                    })
                }
                _ => {
                    return Err(SessionError::validation_field(
                        "kind",
                        "kind must be 'voice' or 'wave'",
                    ))
                }
            };
            let insertion = after
                .parts
                .iter()
                .position(|part| {
                    part.track_no() > track_index as i32
                        || (part.track_no() == track_index as i32 && part.position() > position)
                })
                .unwrap_or(after.parts.len());
            after.parts.insert(insertion, part);
            plan.track_id = Some(track_id);
            plan.part_index = Some(
                after.parts[..insertion]
                    .iter()
                    .filter(|part| part.track_no() == track_index as i32)
                    .count(),
            );
        }
        "part.remove" | "part.move" | "part.resize" | "part.set_metadata"
        | "part.set_media_ref" | "part.split" => {
            let part_id = require_str(command, "part_id")?;
            let located = locate_part(ids, project, &part_id)?;
            plan.track_id = Some(located.track_id.clone());
            plan.part_id = Some(part_id.clone());
            plan.part_index = Some(located.part_in_track);
            match kind {
                "part.remove" => {
                    after.parts.remove(located.flat_index);
                }
                "part.move" => {
                    let position = require_i32(command, "position_ticks")?;
                    if position < 0 {
                        return Err(SessionError::validation_field(
                            "position_ticks",
                            "position_ticks must be non-negative",
                        ));
                    }
                    match after.parts.get_mut(located.flat_index) {
                        Some(UPart::Voice(part)) => part.position = position,
                        Some(UPart::Wave(part)) => part.position = position,
                        None => {
                            return Err(SessionError::internal(
                                "part index disappeared during move",
                            ))
                        }
                    }
                    let item = after.parts.remove(located.flat_index);
                    let index = after
                        .parts
                        .iter()
                        .position(|p| {
                            (p.track_no(), p.position()) > (item.track_no(), item.position())
                        })
                        .unwrap_or(after.parts.len());
                    after.parts.insert(index, item);
                    plan.part_index = Some(
                        after.parts[..index]
                            .iter()
                            .filter(|p| p.track_no() == located.track as i32)
                            .count(),
                    );
                }
                "part.resize" => {
                    let duration = require_i32(command, "duration_ticks")?;
                    if duration <= 0 {
                        return Err(SessionError::validation_field(
                            "duration_ticks",
                            "duration_ticks must be positive",
                        ));
                    }
                    match after.parts.get_mut(located.flat_index) {
                        Some(UPart::Voice(part)) => {
                            if part
                                .notes
                                .iter()
                                .any(|note| note.position.saturating_add(note.duration) > duration)
                                || part
                                    .curves
                                    .iter()
                                    .any(|curve| curve.xs.iter().any(|tick| *tick > duration))
                            {
                                return Err(SessionError::validation_reason(
                                    "duration_ticks",
                                    "content_out_of_bounds",
                                    "new duration would exclude note or curve content",
                                ));
                            }
                            part.duration = duration;
                        }
                        Some(UPart::Wave(part)) => {
                            let total_ticks =
                                duration.saturating_add(part.skip).saturating_add(part.trim);
                            if total_ticks <= 0 {
                                return Err(SessionError::validation_field(
                                    "duration_ticks",
                                    "duration_ticks must be positive",
                                ));
                            }
                            let start_ms = after.time_axis.tick_to_ms(part.position as f64);
                            let end_ms = after
                                .time_axis
                                .tick_to_ms(part.position.saturating_add(total_ticks) as f64);
                            part.file_duration_ms = end_ms - start_ms;
                        }
                        None => {
                            return Err(SessionError::internal(
                                "part index disappeared during resize",
                            ))
                        }
                    }
                }
                "part.set_metadata" => {
                    let fields = command
                        .get("fields")
                        .and_then(Value::as_object)
                        .ok_or_else(|| {
                            SessionError::validation_field("fields", "fields must be an object")
                        })?;
                    if fields.is_empty() {
                        return Err(SessionError::validation_field(
                            "fields",
                            "at least one field is required",
                        ));
                    }
                    let Some(part) = after.parts.get_mut(located.flat_index) else {
                        return Err(SessionError::internal(
                            "part index disappeared during metadata update",
                        ));
                    };
                    for (key, value) in fields {
                        match key.as_str() {
                            "name" => {
                                let name = value.as_str().ok_or_else(|| {
                                    SessionError::validation_field("name", "name must be a string")
                                })?;
                                if name.trim().is_empty() {
                                    return Err(SessionError::validation_field(
                                        "name",
                                        "name must not be empty",
                                    ));
                                }
                                match part {
                                    UPart::Voice(p) => p.name = name.to_string(),
                                    UPart::Wave(p) => p.name = name.to_string(),
                                }
                            }
                            "comment" => {
                                let comment = value
                                    .as_str()
                                    .ok_or_else(|| {
                                        SessionError::validation_field(
                                            "comment",
                                            "comment must be a string",
                                        )
                                    })?
                                    .to_string();
                                match part {
                                    UPart::Voice(p) => p.comment = comment,
                                    UPart::Wave(p) => p.comment = comment,
                                }
                            }
                            _ => {
                                return Err(SessionError::validation_field(
                                    "fields",
                                    format!("unknown part metadata field '{key}'"),
                                ))
                            }
                        }
                    }
                    plan.render_invalidated = false;
                }
                "part.set_media_ref" => {
                    let media_ref = require_str(command, "media_ref")?;
                    if media_ref.trim().is_empty()
                        || media_ref.starts_with('/')
                        || media_ref.starts_with('\\')
                        || media_ref.as_bytes().get(1) == Some(&b':')
                        || media_ref.split('/').any(|segment| segment == "..")
                    {
                        return Err(SessionError::validation_field(
                            "media_ref",
                            "media_ref must be a non-empty project-relative reference, not an absolute path",
                        ));
                    }
                    match after.parts.get_mut(located.flat_index) {
                        Some(UPart::Wave(part)) => part.relative_path = Some(media_ref),
                        Some(UPart::Voice(_)) => {
                            return Err(SessionError::unsupported("part.set_media_ref.voice"));
                        }
                        None => {
                            return Err(SessionError::internal(
                                "part index disappeared during media relink",
                            ))
                        }
                    }
                }
                "part.split" => {
                    let at = require_i32(command, "at_tick")?;
                    let Some(UPart::Voice(original)) = project.parts.get(located.flat_index) else {
                        return Err(SessionError::unsupported("part.split.wave"));
                    };
                    if at <= 0 || at >= original.duration {
                        return Err(SessionError::validation_field(
                            "at_tick",
                            "split tick must be within the part",
                        ));
                    }
                    if original.notes.iter().any(|note| {
                        note.position < at && note.position.saturating_add(note.duration) > at
                    }) {
                        return Err(SessionError::validation_reason(
                            "at_tick",
                            "note_crosses_split",
                            "a note crosses the split boundary",
                        ));
                    }
                    let mut left = original.clone();
                    let mut right = original.clone();
                    left.duration = at;
                    right.position = original.position.saturating_add(at);
                    right.duration = original.duration - at;
                    left.notes.retain(|note| note.position < at);
                    right.notes = original
                        .notes
                        .iter()
                        .filter(|note| note.position >= at)
                        .cloned()
                        .map(|mut note| {
                            note.position -= at;
                            note
                        })
                        .collect();
                    left.curves.clear();
                    right.curves.clear();
                    for original_curve in &original.curves {
                        let mut left_curve = original_curve.clone();
                        let mut right_curve = original_curve.clone();
                        left_curve.xs.clear();
                        left_curve.ys.clear();
                        right_curve.xs.clear();
                        right_curve.ys.clear();
                        for (&tick, &value) in original_curve.xs.iter().zip(&original_curve.ys) {
                            if tick < at {
                                left_curve.xs.push(tick);
                                left_curve.ys.push(value);
                            }
                            if tick > at {
                                right_curve.xs.push(tick - at);
                                right_curve.ys.push(value);
                            }
                        }
                        if let Some(value) = original_curve.sample(at) {
                            left_curve.xs.push(at);
                            left_curve.ys.push(value);
                            right_curve.xs.insert(0, 0);
                            right_curve.ys.insert(0, value);
                        }
                        left.curves.push(left_curve);
                        right.curves.push(right_curve);
                    }
                    after.parts[located.flat_index] = UPart::Voice(left);
                    let insertion = located.flat_index + 1;
                    after.parts.insert(insertion, UPart::Voice(right));
                    plan.right_part_index = Some(located.part_in_track + 1);
                }
                _ => unreachable!(),
            }
        }
        _ => return Err(SessionError::unsupported(kind)),
    }
    after.validate().map_err(SessionError::validation)?;
    plan.after = after;
    Ok(Some(plan))
}

/// The voice part a resolved location points at.
pub fn part_at<'a>(
    project: &'a UProject,
    located: &LocatedPart,
) -> Result<&'a UVoicePart, SessionError> {
    match project.parts.get(located.flat_index) {
        Some(UPart::Voice(vp)) => Ok(vp),
        _ => Err(SessionError::internal(format!(
            "part id {} has no voice part at index {}",
            located.part_id, located.flat_index
        ))),
    }
}

/// Translate a v2 command into the engine's index-addressed form.
///
/// Unknown or not-yet-implemented types fail before anything is mutated.
pub fn plan(
    ids: &IdentityTable,
    project: &UProject,
    command: &Value,
) -> Result<Plan, SessionError> {
    let kind = command
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| SessionError::validation_field("type", "command has no 'type' field"))?
        .to_string();

    let note_target = |located: &LocatedNote| Target::Note {
        note_id: located.note_id.clone(),
        part_id: located.part.part_id.clone(),
        track_id: located.part.track_id.clone(),
    };

    match kind.as_str() {
        "note.add" => {
            let located = locate_part(ids, project, &require_str(command, "part_id")?)?;
            let tone = require_i32(command, "tone")?;
            let position = require_i32(command, "position_ticks")?;
            let duration = require_i32(command, "duration_ticks")?;
            if duration <= 0 {
                return Err(SessionError::validation_field(
                    "duration_ticks",
                    "duration_ticks must be positive",
                ));
            }
            let mut legacy = json!({
                "cmd": "add_note",
                "part": located.flat_index,
                "tone": tone,
                "position": position,
                "duration": duration,
            });
            if let Some(lyric) = optional_str(command, "lyric") {
                legacy["lyric"] = Value::String(lyric);
            }
            Ok(Plan {
                kind,
                legacy_command: legacy.to_string(),
                target: Target::Part {
                    part_id: located.part_id.clone(),
                    track_id: located.track_id.clone(),
                },
                creates_note: true,
                removes_note: false,
            })
        }
        "note.remove" => {
            let located = locate_note(ids, project, &require_str(command, "note_id")?)?;
            Ok(Plan {
                kind,
                legacy_command: json!({
                    "cmd": "remove_note",
                    "part": located.part.flat_index,
                    "note": located.note_index,
                })
                .to_string(),
                target: note_target(&located),
                creates_note: false,
                removes_note: true,
            })
        }
        "note.set_lyric" => {
            let located = locate_note(ids, project, &require_str(command, "note_id")?)?;
            let lyric = require_str(command, "lyric")?;
            Ok(Plan {
                kind,
                legacy_command: json!({
                    "cmd": "change_lyric",
                    "part": located.part.flat_index,
                    "note": located.note_index,
                    "lyric": lyric,
                })
                .to_string(),
                target: note_target(&located),
                creates_note: false,
                removes_note: false,
            })
        }
        "note.set_pitch" => {
            let located = locate_note(ids, project, &require_str(command, "note_id")?)?;
            Ok(Plan {
                kind,
                legacy_command: json!({
                    "cmd": "set_pitch",
                    "part": located.part.flat_index,
                    "note": located.note_index,
                    "pitch": command.get("pitch").cloned().unwrap_or(serde_json::Value::Null),
                })
                .to_string(),
                target: note_target(&located),
                creates_note: false,
                removes_note: false,
            })
        }
        "note.set_vibrato" => {
            let located = locate_note(ids, project, &require_str(command, "note_id")?)?;
            let vibrato = command
                .get("vibrato")
                .cloned()
                .ok_or_else(|| SessionError::validation("note.set_vibrato requires `vibrato`"))?;
            Ok(Plan {
                kind,
                legacy_command: json!({
                    "cmd": "set_vibrato",
                    "part": located.part.flat_index,
                    "note": located.note_index,
                    "vibrato": vibrato,
                })
                .to_string(),
                target: note_target(&located),
                creates_note: false,
                removes_note: false,
            })
        }
        "note.set_tuning" => {
            let located = locate_note(ids, project, &require_str(command, "note_id")?)?;
            let cents = require_i32(command, "cents")?;
            if !(-1200..=1200).contains(&cents) {
                return Err(SessionError::validation_field(
                    "cents",
                    "cents must be within -1200..=1200",
                ));
            }
            Ok(Plan {
                kind,
                legacy_command: json!({
                    "cmd": "set_tuning",
                    "part": located.part.flat_index,
                    "note": located.note_index,
                    "cents": cents,
                })
                .to_string(),
                target: note_target(&located),
                creates_note: false,
                removes_note: false,
            })
        }
        "note.set_tone" => {
            let located = locate_note(ids, project, &require_str(command, "note_id")?)?;
            let tone = require_i32(command, "tone")?;
            Ok(Plan {
                kind,
                legacy_command: json!({
                    "cmd": "change_tone",
                    "part": located.part.flat_index,
                    "note": located.note_index,
                    "tone": tone,
                })
                .to_string(),
                target: note_target(&located),
                creates_note: false,
                removes_note: false,
            })
        }
        "note.move" => {
            let located = locate_note(ids, project, &require_str(command, "note_id")?)?;
            let delta_pos = optional_i32(command, "delta_position_ticks", 0)?;
            let delta_tone = optional_i32(command, "delta_tone", 0)?;
            Ok(Plan {
                kind,
                legacy_command: json!({
                    "cmd": "move_note",
                    "part": located.part.flat_index,
                    "note": located.note_index,
                    "delta_pos": delta_pos,
                    "delta_tone": delta_tone,
                })
                .to_string(),
                target: note_target(&located),
                creates_note: false,
                removes_note: false,
            })
        }
        "note.resize" => {
            let located = locate_note(ids, project, &require_str(command, "note_id")?)?;
            let delta_dur = require_i32(command, "delta_duration_ticks")?;
            let from_start = field(command, "from_start")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            Ok(Plan {
                kind,
                legacy_command: json!({
                    "cmd": "resize_note",
                    "part": located.part.flat_index,
                    "note": located.note_index,
                    "delta_dur": delta_dur,
                    "from_start": from_start,
                })
                .to_string(),
                target: note_target(&located),
                creates_note: false,
                removes_note: false,
            })
        }
        "track.set_mute" | "track.set_solo" => {
            let located = locate_track(ids, &require_str(command, "track_id")?)?;
            let (legacy_name, field_name) = if kind == "track.set_mute" {
                ("set_track_mute", "mute")
            } else {
                ("set_track_solo", "solo")
            };
            let value = require_bool(command, field_name)?;
            Ok(Plan {
                kind,
                legacy_command: json!({
                    "cmd": legacy_name,
                    "track": located.track,
                    field_name: value,
                })
                .to_string(),
                target: Target::Track {
                    track_id: located.track_id,
                    track: located.track,
                },
                creates_note: false,
                removes_note: false,
            })
        }
        "track.set_volume_db" | "track.set_pan" => {
            let located = locate_track(ids, &require_str(command, "track_id")?)?;
            let (legacy_name, field_name) = if kind == "track.set_volume_db" {
                ("set_track_volume", "volume")
            } else {
                ("set_track_pan", "pan")
            };
            let value = require_f64(command, field_name)?;
            Ok(Plan {
                kind,
                legacy_command: json!({
                    "cmd": legacy_name,
                    "track": located.track,
                    field_name: value,
                })
                .to_string(),
                target: Target::Track {
                    track_id: located.track_id,
                    track: located.track,
                },
                creates_note: false,
                removes_note: false,
            })
        }
        "track.set_mix_fx" => {
            let located = locate_track(ids, &require_str(command, "track_id")?)?;
            let value = command.get("mix_fx").ok_or_else(|| {
                SessionError::validation_field("mix_fx", "mix_fx is required (use null to clear)")
            })?;
            if !value.is_null() {
                let fx: domain::UMixFx =
                    serde_json::from_value(value.clone()).map_err(|error| {
                        SessionError::validation_field(
                            "mix_fx",
                            format!("invalid Mix FX object: {error}"),
                        )
                    })?;
                if let Some((field, message)) = fx.validation_error() {
                    return Err(SessionError::validation_field(field, message));
                }
            }
            Ok(Plan {
                kind,
                legacy_command: json!({
                    "cmd": "set_track_mix_fx",
                    "track": located.track,
                    "mix_fx": value,
                })
                .to_string(),
                target: Target::Track {
                    track_id: located.track_id,
                    track: located.track,
                },
                creates_note: false,
                removes_note: false,
            })
        }
        "track.rename" => {
            let located = locate_track(ids, &require_str(command, "track_id")?)?;
            let name = require_str(command, "name")?;
            Ok(Plan {
                kind,
                legacy_command: json!({
                    "cmd": "set_track_name",
                    "track": located.track,
                    "name": name,
                })
                .to_string(),
                target: Target::Track {
                    track_id: located.track_id,
                    track: located.track,
                },
                creates_note: false,
                removes_note: false,
            })
        }
        "curve.apply_stroke" => plan_curve_stroke(ids, project, command, &kind),
        // Recognized families that this contract version does not implement
        // yet. `part.*` and `project.*` follow.
        other
            if other.starts_with("curve.")
                || other.starts_with("part.")
                || other.starts_with("project.")
                || other.starts_with("track.add")
                || other.starts_with("track.remove")
                || other.starts_with("track.reorder") =>
        {
            Err(SessionError::unsupported(other))
        }
        other => Err(SessionError::validation_unknown_type(other)),
    }
}

// ---------------------------------------------------------------------------
// Change generation
// ---------------------------------------------------------------------------

/// Inputs the change builder needs from the session.
pub struct ChangeContext<'a> {
    pub project: &'a UProject,
    pub ids: &'a IdentityTable,
    /// Note ids bound before the mutation (used to detect created notes).
    pub before_note_ids: &'a HashSet<String>,
    pub history_before: (bool, bool),
    pub history_after: (bool, bool),
}

fn voice_part_at(project: &UProject, track: usize, part_in_track: usize) -> Option<&UVoicePart> {
    let flat = flat_index_of_voice_part(project, track, part_in_track)?;
    match project.parts.get(flat) {
        Some(UPart::Voice(vp)) => Some(vp),
        _ => None,
    }
}

fn note_payload(id: &str, note: &domain::UNote, project: &UProject) -> Value {
    serde_json::to_value(views::note_view(
        id,
        note,
        project.rusted_utau_pronunciations.get(id),
    ))
    .unwrap_or(Value::Null)
}

/// Build the §6.4.3 change list for a committed `plan`.
///
/// Called *after* identity re-reconciliation, so ids created by the mutation are
/// already bound (that is how `note.added` finds its subject).
pub fn changes_for(plan: &Plan, ctx: &ChangeContext<'_>) -> Vec<Change> {
    let mut changes = Vec::new();

    if plan.creates_note {
        let after = ctx.ids.note_id_set();
        let mut created: Vec<&String> = after.difference(ctx.before_note_ids).collect();
        // Deterministic order: HashSet iteration is not stable.
        created.sort();
        for id in created {
            if let Some((track, part, note_index)) = ctx.ids.find_note(id) {
                if let Some(vp) = voice_part_at(ctx.project, track, part) {
                    if let Some(note) = vp.notes.get(note_index) {
                        let start = vp.position + note.position;
                        let end = start + note.duration;
                        changes.push(
                            Change::patch("note.added", note_payload(id, note, ctx.project))
                                .with_track(track_id_string(ctx.ids, track).unwrap_or_default())
                                .with_part(part_id_string(ctx.ids, track, part).unwrap_or_default())
                                .with_note(id)
                                .with_tick_range(start, end),
                        );
                    }
                }
            }
        }
        changes.push(Change::invalidate("render.invalidated").with_reason("note added"));
    } else if plan.removes_note {
        if let Target::Note {
            note_id,
            part_id,
            track_id,
        } = &plan.target
        {
            changes.push(
                Change::invalidate("note.removed")
                    .with_track(track_id.clone())
                    .with_part(part_id.clone())
                    .with_note(note_id.clone()),
            );
            changes.push(Change::invalidate("render.invalidated").with_reason("note removed"));
        }
    } else {
        match &plan.target {
            Target::Note {
                note_id,
                part_id,
                track_id,
            } => {
                // Resolve again: the note may have moved.
                let located = ctx.ids.find_note(note_id).and_then(|(track, part, index)| {
                    voice_part_at(ctx.project, track, part)
                        .and_then(|vp| vp.notes.get(index).map(|note| (vp, note)))
                });
                match located {
                    Some((vp, note)) => {
                        let start = vp.position + note.position;
                        let end = start + note.duration;
                        changes.push(
                            Change::patch("note.changed", note_payload(note_id, note, ctx.project))
                                .with_track(track_id.clone())
                                .with_part(part_id.clone())
                                .with_note(note_id.clone())
                                .with_tick_range(start, end),
                        );
                        changes.push(
                            Change::invalidate("render.invalidated")
                                .with_reason("note changed")
                                .with_tick_range(start, end),
                        );
                    }
                    None => {
                        changes.push(
                            Change::invalidate("note.changed")
                                .with_track(track_id.clone())
                                .with_part(part_id.clone())
                                .with_note(note_id.clone()),
                        );
                        changes.push(
                            Change::invalidate("render.invalidated").with_reason("note changed"),
                        );
                    }
                }
            }
            Target::Curve {
                track,
                part_index,
                curve_index,
                curve_id,
                abbr,
            } => {
                let resolved = curve_id.clone().or_else(|| {
                    ctx.ids
                        .curve_id(*track, *part_index, *curve_index)
                        .map(|id| id.as_str().to_string())
                });
                let found = voice_part_at(ctx.project, *track, *part_index).and_then(|vp| {
                    vp.curves
                        .get(*curve_index)
                        .filter(|c| c.abbr == *abbr)
                        .map(|c| (vp, c))
                });
                match (resolved, found) {
                    (Some(id), Some((vp, curve))) => {
                        let first = curve.xs.first().copied().unwrap_or(0);
                        let last = curve.xs.last().copied().unwrap_or(0);
                        let view = views::curve_view(&id, curve);
                        changes.push(
                            Change::patch(
                                "curve.changed",
                                serde_json::to_value(view).unwrap_or(Value::Null),
                            )
                            .with_track(track_id_string(ctx.ids, *track).unwrap_or_default())
                            .with_part(
                                part_id_string(ctx.ids, *track, *part_index).unwrap_or_default(),
                            )
                            .with_curve(&id, abbr.clone())
                            .with_tick_range(vp.position + first, vp.position + last),
                        );
                    }
                    (id, _) => {
                        // The curve is gone (a `clear` emptied it) or its index
                        // now holds something else: invalidate, never patch.
                        let mut change = Change::invalidate("curve.changed");
                        if let Some(id) = id {
                            change = change.with_curve(id, abbr.clone());
                        } else {
                            change = change
                                .with_part(
                                    part_id_string(ctx.ids, *track, *part_index)
                                        .unwrap_or_default(),
                                )
                                .with_track(track_id_string(ctx.ids, *track).unwrap_or_default());
                        }
                        changes.push(change);
                    }
                }
                changes.push(Change::invalidate("render.invalidated").with_reason("curve changed"));
            }
            Target::Part { part_id, track_id } => {
                changes.push(
                    Change::invalidate("part.changed")
                        .with_track(track_id.clone())
                        .with_part(part_id.clone()),
                );
                changes.push(Change::invalidate("render.invalidated").with_reason("part changed"));
            }
            Target::Track { track_id, track } => {
                match ctx.project.tracks.get(*track) {
                    Some(t) => {
                        let payload: TrackMetaView = views::track_meta_view(track_id, t);
                        changes.push(
                            Change::patch(
                                "track.changed",
                                serde_json::to_value(payload).unwrap_or(Value::Null),
                            )
                            .with_track(track_id.clone()),
                        );
                    }
                    None => {
                        changes
                            .push(Change::invalidate("track.changed").with_track(track_id.clone()));
                    }
                }
                changes.push(Change::invalidate("render.invalidated").with_reason("track changed"));
            }
        }
    }

    if ctx.history_before != ctx.history_after {
        changes.push(Change::patch(
            "history.changed",
            json!({"can_undo": ctx.history_after.0, "can_redo": ctx.history_after.1}),
        ));
    }

    changes
}

/// Merge a change into a pending group set: one entry per `(kind, entity)`.
///
/// A group can touch the same note repeatedly, so the commit result reports the
/// *final* state of each entity rather than every intermediate step.
pub fn merge_change(pending: &mut Vec<Change>, incoming: Change) {
    let key = |c: &Change| {
        format!(
            "{}|{}|{}|{}|{}|{}",
            c.kind,
            c.track_id.as_deref().unwrap_or(""),
            c.part_id.as_deref().unwrap_or(""),
            c.note_id.as_deref().unwrap_or(""),
            c.curve_id.as_deref().unwrap_or(""),
            c.abbr.as_deref().unwrap_or(""),
        )
    };
    let incoming_key = key(&incoming);
    if let Some(existing) = pending.iter_mut().find(|c| key(c) == incoming_key) {
        *existing = incoming;
    } else {
        pending.push(incoming);
    }
}

/// Parse the v2 object-form stroke points (`[{tick, value}, ...]`).
///
/// Object form only in v2: parallel arrays are deliberately not accepted, so
/// there is exactly one shape to validate and document.
fn parse_v2_stroke_points(command: &Value) -> Result<Vec<(i32, i32)>, SessionError> {
    let Some(array) = field(command, "points").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(array.len());
    for (index, item) in array.iter().enumerate() {
        let tick = item.get("tick").and_then(Value::as_i64).ok_or_else(|| {
            SessionError::validation_field("points", format!("points[{index}].tick must be a tick"))
        })?;
        let value = item.get("value").and_then(Value::as_i64).ok_or_else(|| {
            SessionError::validation_field(
                "points",
                format!("points[{index}].value must be an integer"),
            )
        })?;
        out.push((tick as i32, value as i32));
    }
    Ok(out)
}

/// Plan `curve.apply_stroke` (§6.6.2).
///
/// Rust owns every decision here: whether the descriptor is a curve, whether a
/// curve may be created, which curve an id addresses, and every range check.
/// Rejections are explicit (`validation_error` + `details.reason`); only point
/// values are clamped, matching the existing curve-edit precedent.
fn plan_curve_stroke(
    ids: &IdentityTable,
    project: &UProject,
    command: &Value,
    kind: &str,
) -> Result<Plan, SessionError> {
    let part = locate_part(ids, project, &require_str(command, "part_id")?)?;
    let vp = part_at(project, &part)?;

    let create = field(command, "create")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let requested_curve = field(command, "curve_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let requested_abbr = optional_str(command, "abbr");

    let mode = require_str(command, "mode")?;
    if mode != "replace" && mode != "clear" {
        return Err(SessionError::validation_field(
            "mode",
            format!("unsupported stroke mode '{mode}'"),
        ));
    }

    let start = require_i32(command, "start_tick")?;
    let end = require_i32(command, "end_tick")?;
    if start >= end {
        return Err(SessionError::validation_reason(
            "end_tick",
            "tick_out_of_range",
            "start_tick must be < end_tick",
        ));
    }
    if start < 0 || end > vp.duration {
        return Err(SessionError::validation_reason(
            "start_tick",
            "tick_out_of_range",
            format!(
                "the stroke interval must lie inside the part (0..{})",
                vp.duration
            ),
        ));
    }

    let raw = parse_v2_stroke_points(command)?;
    if raw.len() > MAX_STROKE_POINTS {
        return Err(SessionError::payload_too_large(
            "max_stroke_points",
            MAX_STROKE_POINTS,
        ));
    }
    if mode == "replace" && raw.is_empty() {
        return Err(SessionError::validation_reason(
            "points",
            "empty_stroke",
            "mode 'replace' needs at least one point",
        ));
    }
    if mode == "clear" && !raw.is_empty() {
        return Err(SessionError::validation_reason(
            "points",
            "unexpected_points",
            "mode 'clear' takes no points",
        ));
    }
    for (tick, _) in &raw {
        if *tick < start || *tick > end {
            return Err(SessionError::validation_reason(
                "points",
                "stroke_point_out_of_range",
                format!("point tick {tick} is outside [{start}, {end}]"),
            ));
        }
    }

    let (abbr, curve_index, curve_id) = match &requested_curve {
        Some(id) => {
            if create {
                return Err(SessionError::validation_field(
                    "create",
                    "create cannot be combined with a curve_id",
                ));
            }
            let located = locate_curve(ids, project, id)?;
            if located.part.part_id != part.part_id {
                return Err(SessionError::not_found("curve", id));
            }
            if let Some(abbr) = &requested_abbr {
                if abbr != &located.abbr {
                    return Err(SessionError::validation_reason(
                        "abbr",
                        "abbr_mismatch",
                        "abbr does not match the curve the id addresses",
                    ));
                }
            }
            (located.abbr, located.curve_index, Some(located.curve_id))
        }
        None => {
            if !create {
                return Err(SessionError::validation_field(
                    "curve_id",
                    "curve_id is required unless create is true",
                ));
            }
            let abbr = require_str(command, "abbr")?;
            if vp.curves.iter().any(|c| c.abbr == abbr) {
                return Err(SessionError::validation_reason(
                    "abbr",
                    "duplicate_curve_descriptor",
                    "this part already has a curve with that descriptor",
                ));
            }
            (abbr, vp.curves.len(), None)
        }
    };

    let descriptor = project.expressions.get(&abbr).ok_or_else(|| {
        SessionError::validation_reason(
            "abbr",
            "unsupported_curve_descriptor",
            format!("curve descriptor '{abbr}' is not registered"),
        )
    })?;
    if descriptor.r#type != UExpressionType::Curve {
        return Err(SessionError::validation_reason(
            "abbr",
            "unsupported_curve_descriptor",
            format!("expression '{abbr}' is not a Curve descriptor"),
        ));
    }

    let points: Vec<Value> = raw
        .iter()
        .map(|(tick, value)| json!([tick, value]))
        .collect();
    let mut legacy = json!({
        "cmd": "apply_curve_stroke",
        "part": part.flat_index,
        "abbr": abbr,
        "mode": mode,
        "start": start,
        "end": end,
        "points": points,
    });
    if curve_id.is_some() {
        legacy["curve"] = json!(curve_index);
    }

    Ok(Plan {
        kind: kind.to_string(),
        legacy_command: legacy.to_string(),
        target: Target::Curve {
            track: part.track,
            part_index: part.part_in_track,
            curve_index,
            curve_id,
            abbr,
        },
        creates_note: false,
        removes_note: false,
    })
}
