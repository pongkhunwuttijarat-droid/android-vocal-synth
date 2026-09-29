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
use crate::views::{self, TrackMetaView};
use domain::{UExpressionType, UPart, UProject, UVoicePart};

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
pub const CONTRACT_VERSION: &str = "2-draft1";
/// Wire-schema versions, bumped only with an explicit migration.
pub const COMMAND_SCHEMA_VERSION: u32 = 1;
pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;
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
    let mut seen = 0usize;
    for (flat, part) in project.parts.iter().enumerate() {
        if let UPart::Voice(vp) = part {
            if vp.track_no == track as i32 {
                if seen == part_in_track {
                    return Some(flat);
                }
                seen += 1;
            }
        }
    }
    None
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

fn note_payload(id: &str, note: &domain::UNote) -> Value {
    serde_json::to_value(views::note_view(id, note)).unwrap_or(Value::Null)
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
                            Change::patch("note.added", note_payload(id, note))
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
                            Change::patch("note.changed", note_payload(note_id, note))
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
