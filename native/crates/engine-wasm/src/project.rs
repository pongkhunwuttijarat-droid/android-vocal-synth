//! Rust-owned project document + a JSON command protocol for the web UI.
//!
//! The browser must NOT reimplement musical editing rules (tick arithmetic,
//! rounding, note/part bounds, curve semantics). This module keeps the
//! `domain::UProject` in Rust behind a small JSON surface:
//!
//! * `project_*` — create / load `.ustx` YAML / serialize back to YAML
//! * `apply_command(json)` — one reversible edit, through `commands`
//! * `undo()` / `redo()` / `can_undo()` / `can_redo()`
//! * `*_json()` — read-only view models for the JS renderer
//!
//! ## Command protocol
//!
//! A command is a JSON object with a `cmd` discriminator. Unknown or invalid
//! commands return `Err` and leave the project **untouched** (`commands`
//! validates before mutating).
//!
//! ```json
//! {"cmd":"add_note","part":0,"tone":60,"position":480,"duration":480,"lyric":"la"}
//! {"cmd":"remove_note","part":0,"note":3}
//! {"cmd":"change_tone","part":0,"note":3,"tone":72}
//! {"cmd":"change_lyric","part":0,"note":3,"lyric":"ka"}
//! {"cmd":"move_note","part":0,"note":3,"delta_pos":480,"delta_tone":0}
//! {"cmd":"resize_note","part":0,"note":3,"delta_dur":240,"from_start":false}
//! {"cmd":"set_track_volume","track":0,"volume":-3.0}
//! {"cmd":"set_track_pan","track":0,"pan":0.25}
//! {"cmd":"set_track_mute","track":0,"mute":true}
//! {"cmd":"set_track_solo","track":0,"solo":false}
//! {"cmd":"set_track_name","track":0,"name":"Lead"}
//! {"cmd":"set_singer","track":0,"singer":"teto"}
//! {"cmd":"group_begin","name":"Move notes"}   // batch edits into one undo step
//! {"cmd":"group_end"}
//! ```
//!
//! ## Panic safety
//!
//! `wasm32-unknown-unknown` builds with `panic = "abort"`, so a panic kills
//! the whole instance (project + chunk cache). Every command path here maps
//! errors to `Err` instead of unwrapping, and [`ProjectDoc`] keeps the YAML
//! text of the last good state so a host can rebuild after a trap.

use commands::{
    AddNoteCommand, ChangeNoteLyricCommand, ChangeNoteToneCommand, MoveNoteCommand,
    RemoveNoteCommand, ResizeNoteCommand, SetTrackMuteCommand, SetTrackPanCommand,
    SetTrackSoloCommand, SetTrackVolumeCommand, UndoStack,
};
use domain::{UPart, UProject};

/// A `UProject` plus its undo history and a YAML safety snapshot.
pub struct ProjectDoc {
    project: UProject,
    undo: UndoStack,
    /// YAML of the last known-good state (used to rebuild after a trap).
    snapshot: String,
}

impl ProjectDoc {
    /// New empty project (`UProject::create` + default expressions).
    pub fn new() -> Self {
        let project = UProject::create();
        let snapshot = to_yaml(&project);
        ProjectDoc {
            project,
            undo: UndoStack::new(),
            snapshot,
        }
    }

    /// Parse a `.ustx` YAML document (runs `after_load`).
    pub fn from_yaml(yaml: &str) -> Result<Self, String> {
        let mut project: UProject =
            serde_yaml::from_str(yaml).map_err(|e| format!("parse project: {e}"))?;
        project.after_load().map_err(|e| format!("after_load: {e}"))?;
        let snapshot = to_yaml(&project);
        Ok(ProjectDoc {
            project,
            undo: UndoStack::new(),
            snapshot,
        })
    }

    pub fn project(&self) -> &UProject {
        &self.project
    }

    /// Serialize the current state back to `.ustx` YAML.
    pub fn yaml(&self) -> String {
        to_yaml(&self.project)
    }

    /// The last known-good YAML (survives a failed command).
    pub fn snapshot(&self) -> &str {
        &self.snapshot
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

    /// Apply one command. On `Err` the project is unchanged.
    pub fn apply(&mut self, cmd: &str) -> Result<Option<String>, String> {
        let value: serde_json::Value =
            serde_json::from_str(cmd).map_err(|e| format!("parse command: {e}"))?;
        let kind = value
            .get("cmd")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "command has no `cmd` field".to_string())?;

        // Grouping is handled by the stack itself (not a Command).
        match kind {
            "group_begin" => {
                self.undo
                    .start_group(str_field(&value, "name").unwrap_or("Edit"));
                return Ok(None);
            }
            "group_end" => {
                self.undo.end_group()?;
                self.snapshot = self.yaml();
                return Ok(None);
            }
            _ => {}
        }

        let command = self.build(kind, &value)?;
        self.undo
            .execute(&mut self.project, command)
            .map_err(|e| format!("{kind}: {e}"))?;
        // Only a successful execute refreshes the snapshot. The undo-stack
        // name is the command's own `name()`, reported by the UI.
        self.snapshot = self.yaml();
        Ok(Some(snapshot_name(&self.project, kind)))
    }

    /// Build the `Command` for `kind` (validation happens in the ctor).
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
            other => return Err(format!("unknown command: {other}")),
        };
        Ok(cmd)
    }

    pub fn undo(&mut self) -> Result<Option<String>, String> {
        let name = self.undo.undo(&mut self.project)?;
        self.snapshot = self.yaml();
        Ok(name)
    }

    pub fn redo(&mut self) -> Result<Option<String>, String> {
        let name = self.undo.redo(&mut self.project)?;
        self.snapshot = self.yaml();
        Ok(name)
    }

    /// View model for the piano roll: parts and their notes for `track`.
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
                        n.position,
                        n.duration,
                        n.tone,
                        json_string(&n.lyric)
                    )
                })
                .collect();
            parts.push(format!(
                "{{\"part\":{part_index},\"name\":{},\"position\":{},\"duration\":{},\"notes\":[{}]}}",
                json_string(&vp.name),
                vp.position,
                vp.duration,
                notes.join(",")
            ));
        }
        format!("{{\"track\":{track},\"parts\":[{}]}}", parts.join(","))
    }

    /// View model for the expression lanes: curves per part.
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
                    // `UCurve` stores parallel `xs`/`ys` arrays (ticks/value).
                    let points: Vec<String> = c
                        .xs
                        .iter()
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

    /// Track list view model (name, mute/solo/volume/pan, singer, phonemizer).
    pub fn tracks_json(&self) -> String {
        let tracks: Vec<String> = self
            .project
            .tracks
            .iter()
            .enumerate()
            .map(|(i, t)| {
                format!(
                    "{{\"index\":{i},\"name\":{},\"singer\":{},\"phonemizer\":{},\"mute\":{},\"solo\":{},\"volume\":{},\"pan\":{}}}",
                    json_string(&t.track_name),
                    opt_json_string(t.singer.as_deref()),
                    opt_json_string(t.phonemizer.as_deref()),
                    t.mute,
                    t.solo,
                    t.volume,
                    t.pan
                )
            })
            .collect();
        format!("[{}]", tracks.join(","))
    }

    /// Project metadata the UI needs for the time axis.
    pub fn meta_json(&self) -> String {
        format!(
            "{{\"name\":{},\"bpm\":{},\"beat_per_bar\":{},\"beat_unit\":{},\"resolution\":{},\"track_count\":{},\"part_count\":{}}}",
            json_string(&self.project.name),
            self.project.bpm,
            self.project.beat_per_bar,
            self.project.beat_unit,
            self.project.resolution(),
            self.project.tracks.len(),
            self.project.parts.len()
        )
    }
}

impl Default for ProjectDoc {
    fn default() -> Self {
        Self::new()
    }
}

/// Human-readable label for an applied command (the UI shows it in the
/// undo affordance).
fn snapshot_name(_project: &UProject, kind: &str) -> String {
    match kind {
        "add_note" => "Add note",
        "remove_note" => "Remove note",
        "change_tone" => "Change tone",
        "change_lyric" => "Change lyric",
        "move_note" => "Move note",
        "resize_note" => "Resize note",
        "set_track_volume" => "Track volume",
        "set_track_pan" => "Track pan",
        "set_track_mute" => "Track mute",
        "set_track_solo" => "Track solo",
        "set_track_name" => "Track name",
        other => other,
    }
    .to_string()
}

/// Serialize to `.ustx` YAML.
///
/// MUST go through `UProject::before_save`: `parts` is `#[serde(skip)]` and
/// only `voice_parts` is serialized, so without it the emitted YAML has no
/// notes at all (it round-trips to an empty project).
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

/// Minimal JSON string escaping (enough for lyrics, names, abbreviations).
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn opt_json_string(s: Option<&str>) -> String {
    match s {
        Some(s) => json_string(s),
        None => "null".to_string(),
    }
}
