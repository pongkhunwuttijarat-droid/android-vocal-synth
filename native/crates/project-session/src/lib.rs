//! Platform-neutral Rust-owned project document and command session.
//!
//! Hosts must treat [`ProjectDoc`] as the authoritative project state. UI
//! adapters may keep disposable read models and input state, but save/render
//! must consume this document (or its canonical YAML snapshot).
//!
//! This crate is extracted from the browser/WASM host so native server and
//! other hosts can use the same project, command, undo, serialization, and
//! view semantics without depending on wasm-bindgen or renderer code.
//!
//! [`identity`] owns stable entity IDs (opaque, Rust-minted, never written to
//! `.ustx`) and the sidecar ID table described in `docs/CONTRACT-v1.md` §6.2.
//! [`Session`] combines that identity with the authoritative document and the
//! §6.9 revision rules; it is the type every host adapter should drive.

pub mod identity;
pub mod session;
pub mod v2;
mod views;

pub use session::{GroupBegin, MutationOutcome, SaveAsPayload, Session};
pub use v2::{capabilities, Capabilities, Change, MutationResult, SessionError};

use commands::{
    AddNoteCommand, ApplyCurveStrokeCommand, ChangeNoteLyricCommand, ChangeNoteToneCommand,
    ChangeNoteTuningCommand, MoveNoteCommand, RemoveNoteCommand, ResizeNoteCommand,
    SetNotePitchCommand, SetNoteVibratoCommand, SetTrackMixFxCommand, SetTrackMuteCommand,
    SetTrackPanCommand, SetTrackSingerCommand, SetTrackSoloCommand, SetTrackVolumeCommand,
    StrokeMode, StrokeTarget, UndoStack,
};
use domain::{PitchPoint, PitchPointShape, UExpressionType, UMixFx, UPart, UPitch, UProject};

/// A Rust-owned project plus reversible edit history and a last-known-good
/// serialized snapshot. Commands validate before mutating; rejected commands
/// leave the document unchanged.
///
/// # Snapshot cost
///
/// `to_yaml` is `clone + before_save + serialize`; measured at **38.4 ms** per
/// call on the 867-note badapple project (debug build) versus 34.5 µs for a
/// plain clone. Serializing after every command therefore dominates the edit
/// path, so the cached snapshot is refreshed **lazily**: a successful command
/// only marks it stale, and [`ProjectDoc::snapshot`] (or [`ProjectDoc::yaml`])
/// does the work when a caller actually needs the bytes. Use
/// [`ProjectDoc::snapshot_is_stale`] to observe the deferral (tests do).
pub struct ProjectDoc {
    project: UProject,
    undo: UndoStack,
    /// Cached canonical YAML, refreshed by `refresh_snapshot`.
    snapshot: String,
    /// True when `project` has changed since `snapshot` was produced.
    snapshot_stale: bool,
}

impl ProjectDoc {
    /// Create a new empty project with engine defaults.
    pub fn new() -> Self {
        Self::from_project(UProject::create())
    }

    /// Parse and normalize a USTX YAML document.
    ///
    /// Uses [`UProject::parse_yaml`] so a UTF-8 BOM is tolerated — every
    /// OpenUtau-written `.ustx` has one, so without this no real project opens.
    pub fn from_yaml(yaml: &str) -> Result<Self, String> {
        Ok(Self::from_project(UProject::parse_yaml(yaml)?))
    }

    /// Wrap an already-parsed, already-normalized project.
    ///
    /// Callers that need the load-time repair report must call
    /// [`domain::UProject::after_load_with_warnings`] themselves and then hand
    /// the project over here (the canonical snapshot is recomputed).
    pub fn from_project(project: UProject) -> Self {
        let snapshot = to_yaml(&project);
        Self {
            project,
            undo: UndoStack::new(),
            snapshot,
            snapshot_stale: false,
        }
    }

    /// Wrap a project without computing the canonical snapshot.
    ///
    /// Used for a group's tentative copy (§6.8): the snapshot is not needed
    /// while the transaction is open, and serializing would dominate the cost
    /// of opening it.
    pub fn from_project_lazy(project: UProject) -> Self {
        Self {
            project,
            undo: UndoStack::new(),
            snapshot: String::new(),
            snapshot_stale: true,
        }
    }

    /// Install `after` as the document as **exactly one** undo step.
    ///
    /// This is the group-commit primitive: a group edits a tentative copy, so
    /// the only transition the real undo stack must record is "before → after".
    /// The command owns both snapshots, which makes undo/redo exact no matter
    /// how many child commands the group contained.
    pub fn commit_transaction(&mut self, after: UProject, name: &str) -> Result<(), String> {
        let before = self.project.clone();
        let command = commands::RestoreDocumentCommand::new(before, after, name);
        self.undo.execute(&mut self.project, Box::new(command))?;
        self.snapshot_stale = true;
        Ok(())
    }

    pub fn project(&self) -> &UProject {
        &self.project
    }
    /// Restore sidecar-owned Rusted UTAU metadata before a freshly opened document is
    /// published. This path deliberately does not create undo history.
    pub fn restore_rusted_utau_pronunciations(
        &mut self,
        pronunciations: std::collections::BTreeMap<String, domain::UPronunciation>,
    ) {
        self.project.rusted_utau_pronunciations = pronunciations;
        self.snapshot_stale = true;
    }
    pub fn yaml(&self) -> String {
        to_yaml(&self.project)
    }
    /// The last known-good YAML, refreshed first when the document changed.
    pub fn snapshot(&mut self) -> &str {
        self.refresh_snapshot();
        &self.snapshot
    }
    /// Whether the cached snapshot is behind the document. Calling
    /// [`ProjectDoc::snapshot`] clears this.
    pub fn snapshot_is_stale(&self) -> bool {
        self.snapshot_stale
    }

    fn refresh_snapshot(&mut self) {
        if self.snapshot_stale {
            self.snapshot = to_yaml(&self.project);
            self.snapshot_stale = false;
        }
    }
    pub fn can_undo(&self) -> bool {
        self.undo.can_undo()
    }
    pub fn can_redo(&self) -> bool {
        self.undo.can_redo()
    }
    pub fn undo_len(&self) -> usize {
        self.undo.undo_len()
    }
    pub fn redo_len(&self) -> usize {
        self.undo.redo_len()
    }

    /// Apply a JSON command. A rejected individual command leaves the project
    /// unchanged. Successful commands in an open group remain applied until the
    /// host closes (`group_end`) or explicitly cancels (`group_cancel`) it.
    pub fn apply(&mut self, cmd: &str) -> Result<Option<String>, String> {
        let value: serde_json::Value =
            serde_json::from_str(cmd).map_err(|e| format!("parse command: {e}"))?;
        let kind = value
            .get("cmd")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "command has no `cmd` field".to_string())?;
        match kind {
            "group_begin" => {
                self.undo
                    .start_group(str_field(&value, "name").unwrap_or("Edit"));
                return Ok(None);
            }
            "group_end" => {
                self.undo.end_group()?;
                self.snapshot_stale = true;
                return Ok(None);
            }
            "group_cancel" => {
                self.undo.cancel_group(&mut self.project)?;
                self.snapshot_stale = true;
                return Ok(None);
            }
            _ => {}
        }
        let command = self.build(kind, &value)?;
        self.undo
            .execute(&mut self.project, command)
            .map_err(|e| format!("{kind}: {e}"))?;
        self.snapshot_stale = true;
        Ok(Some(snapshot_name(kind)))
    }

    fn build(
        &self,
        kind: &str,
        v: &serde_json::Value,
    ) -> Result<Box<dyn commands::Command>, String> {
        let part = int_field(v, "part").unwrap_or(0) as usize;
        let note = int_field(v, "note");
        let project = &self.project;
        let cmd: Box<dyn commands::Command> = match kind {
            "add_note" => {
                let tone = int_field(v, "tone").ok_or("add_note needs `tone`")?;
                let position = int_field(v, "position").ok_or("add_note needs `position`")?;
                let duration = int_field(v, "duration").ok_or("add_note needs `duration`")?;
                let mut n = project.create_note_at(tone, position, duration);
                if let Some(lyric) = str_field(v, "lyric") {
                    n.lyric = lyric.to_string();
                }
                Box::new(AddNoteCommand::new(project, part, n)?)
            }
            "remove_note" => Box::new(RemoveNoteCommand::new(
                project,
                part,
                note.ok_or("remove_note needs `note`")? as usize,
            )?),
            "change_tone" => Box::new(ChangeNoteToneCommand::new(
                project,
                part,
                note.ok_or("change_tone needs `note`")? as usize,
                int_field(v, "tone").ok_or("change_tone needs `tone`")?,
            )?),
            "set_pitch" => Box::new(SetNotePitchCommand::new(
                project,
                part,
                note.ok_or("set_pitch needs `note`")? as usize,
                parse_pitch_field(v.get("pitch"))?,
            )?),
            "set_vibrato" => Box::new(SetNoteVibratoCommand::new(
                project,
                part,
                note.ok_or("set_vibrato needs `note`")? as usize,
                serde_json::from_value(
                    v.get("vibrato")
                        .cloned()
                        .ok_or("set_vibrato needs `vibrato`")?,
                )
                .map_err(|e| format!("invalid vibrato: {e}"))?,
            )?),
            "set_tuning" => Box::new(ChangeNoteTuningCommand::new(
                project,
                part,
                note.ok_or("set_tuning needs `note`")? as usize,
                int_field(v, "cents").ok_or("set_tuning needs integer `cents`")?,
            )?),
            "change_lyric" => Box::new(ChangeNoteLyricCommand::new(
                project,
                part,
                note.ok_or("change_lyric needs `note`")? as usize,
                str_field(v, "lyric").ok_or("change_lyric needs `lyric`")?,
            )?),
            "move_note" => Box::new(MoveNoteCommand::new(
                project,
                part,
                note.ok_or("move_note needs `note`")? as usize,
                int_field(v, "delta_pos").unwrap_or(0),
                int_field(v, "delta_tone").unwrap_or(0),
            )?),
            "resize_note" => Box::new(ResizeNoteCommand::new(
                project,
                part,
                note.ok_or("resize_note needs `note`")? as usize,
                int_field(v, "delta_dur").ok_or("resize_note needs `delta_dur`")?,
                bool_field(v, "from_start").unwrap_or(false),
            )?),
            "set_track_volume" => Box::new(SetTrackVolumeCommand::new(
                project,
                int_field(v, "track").unwrap_or(0) as usize,
                float_field(v, "volume").ok_or("set_track_volume needs `volume`")?,
            )?),
            "set_track_pan" => Box::new(SetTrackPanCommand::new(
                project,
                int_field(v, "track").unwrap_or(0) as usize,
                float_field(v, "pan").ok_or("set_track_pan needs `pan`")?,
            )?),
            "set_track_mix_fx" => {
                let mix_fx = match v.get("mix_fx") {
                    None => return Err("set_track_mix_fx needs `mix_fx`".into()),
                    Some(serde_json::Value::Null) => None,
                    Some(value) => {
                        let fx: UMixFx = serde_json::from_value(value.clone())
                            .map_err(|e| format!("invalid set_track_mix_fx value: {e}"))?;
                        if let Some((field, message)) = fx.validation_error() {
                            return Err(format!("set_track_mix_fx {field}: {message}"));
                        }
                        Some(fx)
                    }
                };
                Box::new(SetTrackMixFxCommand::new(
                    project,
                    int_field(v, "track").unwrap_or(0) as usize,
                    mix_fx,
                )?)
            }
            "set_track_mute" => Box::new(SetTrackMuteCommand::new(
                project,
                int_field(v, "track").unwrap_or(0) as usize,
                bool_field(v, "mute").ok_or("set_track_mute needs `mute`")?,
            )?),
            "set_track_solo" => Box::new(SetTrackSoloCommand::new(
                project,
                int_field(v, "track").unwrap_or(0) as usize,
                bool_field(v, "solo").ok_or("set_track_solo needs `solo`")?,
            )?),
            "set_track_name" => Box::new(commands::RenameTrackCommand::new(
                project,
                int_field(v, "track").unwrap_or(0) as usize,
                str_field(v, "name").ok_or("set_track_name needs `name`")?,
            )?),
            // `null` clears the singer (an audio-only track); a string sets it.
            "set_track_singer" => Box::new(SetTrackSingerCommand::new(
                project,
                int_field(v, "track").unwrap_or(0) as usize,
                match v.get("singer") {
                    None | Some(serde_json::Value::Null) => None,
                    Some(value) => Some(
                        value
                            .as_str()
                            .ok_or("set_track_singer needs a string or null `singer`")?
                            .to_string(),
                    ),
                },
            )?),
            "apply_curve_stroke" => {
                match project.parts.get(part) {
                    Some(UPart::Voice(_)) => {}
                    Some(UPart::Wave(_)) => {
                        return Err("part index does not refer to a voice part".to_string())
                    }
                    None => return Err(format!("part index {part} is out of range")),
                }
                let abbr = str_field(v, "abbr").ok_or("apply_curve_stroke needs `abbr`")?;
                let descriptor = project
                    .expressions
                    .get(abbr)
                    .ok_or_else(|| format!("curve descriptor '{abbr}' is not registered"))?;
                if descriptor.r#type != UExpressionType::Curve {
                    return Err(format!("expression '{abbr}' is not a Curve descriptor"));
                }
                // Precedent: curve edits clamp to the descriptor's integer
                // bounds; the default comes from `custom_default_value`
                // (what the renderer samples with).
                let min = descriptor.min as i32;
                let max = descriptor.max as i32;
                let simplify_tolerance =
                    5.0_f64.min((descriptor.max as f64 - descriptor.min as f64) * 0.005);
                let default = descriptor.custom_default_value() as i32;
                let mode = match str_field(v, "mode") {
                    Some("clear") => StrokeMode::Clear,
                    Some("replace") | None => StrokeMode::Replace,
                    Some(other) => return Err(format!("unknown stroke mode '{other}'")),
                };
                let start = int_field(v, "start").ok_or("apply_curve_stroke needs `start`")?;
                let end = int_field(v, "end").ok_or("apply_curve_stroke needs `end`")?;
                if start >= end {
                    return Err("stroke interval must satisfy start < end".to_string());
                }
                let raw = parse_stroke_points(v.get("points"))?;
                let stroke = commands::canonicalize(&raw, start, end, mode, min, max);
                let target = match int_field(v, "curve") {
                    Some(index) => StrokeTarget::Existing(index as usize),
                    None => StrokeTarget::Create(abbr.to_string()),
                };
                Box::new(ApplyCurveStrokeCommand::new_with_tolerance(
                    project,
                    part,
                    &target,
                    default,
                    &stroke,
                    simplify_tolerance,
                )?)
            }
            other => return Err(format!("unknown command: {other}")),
        };
        Ok(cmd)
    }

    pub fn undo(&mut self) -> Result<Option<String>, String> {
        let name = self.undo.undo(&mut self.project)?;
        if name.is_some() {
            self.snapshot_stale = true;
        }
        Ok(name)
    }
    pub fn redo(&mut self) -> Result<Option<String>, String> {
        let name = self.undo.redo(&mut self.project)?;
        if name.is_some() {
            self.snapshot_stale = true;
        }
        Ok(name)
    }

    /// Read-only notes for a track, grouped by part.
    pub fn notes_json(&self, track: i32) -> String {
        let mut parts = Vec::new();
        for (part_index, part) in self.project.parts.iter().enumerate() {
            let UPart::Voice(vp) = part else { continue };
            if vp.track_no != track {
                continue;
            }
            let notes: Vec<String> = vp
                .notes
                .iter()
                .enumerate()
                .map(|(i, n)| {
                    format!(
                "{{\"index\":{i},\"position\":{},\"duration\":{},\"tone\":{},\"lyric\":{}}}",
                n.position,n.duration,n.tone,json_string(&n.lyric)
            )
                })
                .collect();
            parts.push(format!("{{\"part\":{part_index},\"name\":{},\"position\":{},\"duration\":{},\"notes\":[{}]}}",json_string(&vp.name),vp.position,vp.duration,notes.join(",")));
        }
        format!("{{\"track\":{track},\"parts\":[{}]}}", parts.join(","))
    }

    /// Read-only expression curves for a track.
    pub fn curves_json(&self, track: i32) -> String {
        let mut parts = Vec::new();
        for (part_index, part) in self.project.parts.iter().enumerate() {
            let UPart::Voice(vp) = part else { continue };
            if vp.track_no != track {
                continue;
            }
            let curves: Vec<String> = vp
                .curves
                .iter()
                .map(|c| {
                    let points: Vec<String> =
                        c.xs.iter()
                            .zip(c.ys.iter())
                            .map(|(x, y)| format!("{{\"x\":{x},\"y\":{y}}}"))
                            .collect();
                    format!(
                        "{{\"abbr\":{},\"points\":[{}]}}",
                        json_string(&c.abbr),
                        points.join(",")
                    )
                })
                .collect();
            parts.push(format!(
                "{{\"part\":{part_index},\"curves\":[{}]}}",
                curves.join(",")
            ));
        }
        format!("{{\"track\":{track},\"parts\":[{}]}}", parts.join(","))
    }

    pub fn tracks_json(&self) -> String {
        let tracks: Vec<String> = self.project.tracks.iter().enumerate().map(|(i,t)|format!(
            "{{\"index\":{i},\"name\":{},\"singer\":{},\"phonemizer\":{},\"mute\":{},\"solo\":{},\"volume\":{},\"pan\":{}}}",
            json_string(&t.track_name),opt_json_string(t.singer.as_deref()),opt_json_string(t.phonemizer.as_deref()),t.mute,t.solo,t.volume,t.pan
        )).collect();
        format!("[{}]", tracks.join(","))
    }

    pub fn meta_json(&self) -> String {
        format!("{{\"name\":{},\"bpm\":{},\"beat_per_bar\":{},\"beat_unit\":{},\"resolution\":{},\"track_count\":{},\"part_count\":{}}}",
            json_string(&self.project.name),self.project.bpm,self.project.beat_per_bar,self.project.beat_unit,self.project.resolution(),self.project.tracks.len(),self.project.parts.len())
    }
}
impl Default for ProjectDoc {
    fn default() -> Self {
        Self::new()
    }
}

fn snapshot_name(kind: &str) -> String {
    match kind {
        "add_note" => "Add note",
        "remove_note" => "Remove note",
        "change_tone" => "Change tone",
        "set_pitch" => "Draw pitch",
        "change_lyric" => "Change lyric",
        "move_note" => "Move note",
        "resize_note" => "Resize note",
        "set_track_volume" => "Track volume",
        "set_track_pan" => "Track pan",
        "set_track_mix_fx" => "Track Mix FX",
        "set_track_mute" => "Track mute",
        "set_track_solo" => "Track solo",
        "set_track_name" => "Track name",
        "set_track_singer" => "Track singer",
        other => other,
    }
    .to_string()
}
fn to_yaml(project: &UProject) -> String {
    let mut copy = project.clone();
    copy.before_save();
    serde_yaml::to_string(&copy).unwrap_or_default()
}
fn int_field(v: &serde_json::Value, key: &str) -> Option<i32> {
    v.get(key).and_then(|x| x.as_i64()).map(|x| x as i32)
}
fn float_field(v: &serde_json::Value, key: &str) -> Option<f64> {
    v.get(key).and_then(|x| x.as_f64())
}
fn bool_field(v: &serde_json::Value, key: &str) -> Option<bool> {
    v.get(key).and_then(|x| x.as_bool())
}
fn str_field<'a>(v: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(|x| x.as_str())
}
fn json_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}
fn opt_json_string(s: Option<&str>) -> String {
    s.map(json_string).unwrap_or_else(|| "null".to_string())
}

/// Parse `pitch: {data: [{x, y, shape}], snap_first}` from the wire.
///
/// `x` is milliseconds from the note start and `y` is tenths of a semitone
/// relative to the note's tone — the units `domain::PitchPoint` and the USTX
/// codec already use, so a host sends what it drew. `shape` is the lowercase
/// C# enum name (`io`/`l`/`i`/`o`/`sp`) and defaults to `io`, which is what
/// OpenUtau's `PitchPoint` constructor defaults to.
fn parse_pitch_field(value: Option<&serde_json::Value>) -> Result<UPitch, String> {
    let Some(value) = value else {
        return Ok(UPitch::default());
    };
    let snap_first = value
        .get("snap_first")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    let mut data = Vec::new();
    if let Some(points) = value.get("data").and_then(serde_json::Value::as_array) {
        for point in points {
            let x = point
                .get("x")
                .and_then(serde_json::Value::as_f64)
                .ok_or("pitch point needs a numeric `x`")?;
            let y = point
                .get("y")
                .and_then(serde_json::Value::as_f64)
                .ok_or("pitch point needs a numeric `y`")?;
            let shape = match point.get("shape").and_then(serde_json::Value::as_str) {
                None | Some("io") => PitchPointShape::Io,
                Some("l") => PitchPointShape::L,
                Some("i") => PitchPointShape::I,
                Some("o") => PitchPointShape::O,
                Some("sp") => PitchPointShape::Sp,
                Some(other) => return Err(format!("unknown pitch point shape '{other}'")),
            };
            data.push(PitchPoint {
                x: x as f32,
                y: y as f32,
                shape,
            });
        }
    }
    Ok(UPitch { data, snap_first })
}

/// Parse `points: [[tick, value], ...]` for a curve stroke.
fn parse_stroke_points(value: Option<&serde_json::Value>) -> Result<Vec<(i32, i32)>, String> {
    let Some(array) = value.and_then(|v| v.as_array()) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(array.len());
    for item in array {
        let pair = item
            .as_array()
            .filter(|p| p.len() == 2)
            .ok_or("stroke points must be [tick, value] pairs")?;
        let tick = pair[0]
            .as_i64()
            .ok_or("stroke point tick must be an integer")?;
        let val = pair[1]
            .as_i64()
            .ok_or("stroke point value must be an integer")?;
        out.push((tick as i32, val as i32));
    }
    Ok(out)
}
