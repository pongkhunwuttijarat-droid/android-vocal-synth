//! Note commands — mirror of `OpenUtau.Core/Commands/NoteCommands.cs`.
//!
//! Every command snapshots the affected note at construction and locates
//! it again on execute/unexecute by value equality (see [`crate::locate`]),
//! so round-trips restore the exact prior state.
//!
//! `AddNoteCommand`, `MoveNoteCommand` and `ResizeNoteCommand` also grow
//! the part duration when the edit would push the note past the part end,
//! mirroring the `NewPartDuration` logic of the OpenUtau constructors.

use domain::{UNote, UPart, UProject};

use crate::command::Command;
use crate::locate::{
    locate_note_index, locate_part_mut, min_dur_tick_for_note_edit, restore_insert_index,
    PartFingerprint,
};

/// Base snapshot shared by the note-editing commands.
///
/// `old_note` is the note as it was at construction, `new_note` is the note
/// after the edit. `note_index` is the note's index at construction (used
/// as a locate hint), `part_index`/`part_fp` locate the owning part.
struct NoteEdit {
    part_index: usize,
    part_fp: PartFingerprint,
    note_index: usize,
    old_note: UNote,
    new_note: UNote,
}

impl NoteEdit {
    /// Locate the part and the note, and resolve the construction-time
    /// snapshot of the note.
    fn from_project(
        project: &UProject,
        part_index: usize,
        note_index: usize,
    ) -> Result<Self, String> {
        let part = match project.parts.get(part_index) {
            Some(UPart::Voice(vp)) => vp,
            _ => return Err("part index does not refer to a voice part".to_string()),
        };
        let old_note = part
            .notes
            .get(note_index)
            .cloned()
            .ok_or_else(|| "note index out of bounds".to_string())?;
        let new_note = old_note.clone();
        Ok(NoteEdit {
            part_index,
            part_fp: PartFingerprint::of(part),
            note_index,
            old_note,
            new_note,
        })
    }
}

/// Locate the note and replace it with `new_note`; optionally grow the part
/// duration (`new_duration > 0` only, like OpenUtau's `NewPartDuration`).
fn execute_note_edit(
    project: &mut UProject,
    edit: &NoteEdit,
    new_duration: Option<i32>,
) -> Result<(), String> {
    let part = locate_part_mut(project, edit.part_index, &edit.part_fp)?;
    let idx = locate_note_index(part, edit.note_index, &edit.old_note, Some(&edit.new_note))?;
    part.notes[idx] = edit.new_note.clone();
    if let Some(d) = new_duration {
        if d > 0 {
            part.duration = d;
        }
    }
    Ok(())
}

/// Locate the note in its edited state and restore `old_note`; optionally
/// restore the part duration.
fn unexecute_note_edit(
    project: &mut UProject,
    edit: &NoteEdit,
    old_duration: Option<i32>,
) -> Result<(), String> {
    let part = locate_part_mut(project, edit.part_index, &edit.part_fp)?;
    let idx = locate_note_index(part, edit.note_index, &edit.new_note, Some(&edit.old_note))?;
    part.notes[idx] = edit.old_note.clone();
    if let Some(d) = old_duration {
        part.duration = d;
    }
    Ok(())
}

/// Add a note to a voice part (`AddNoteCommand`).
///
/// Also grows the part duration when the new note ends past the part end.
pub struct AddNoteCommand {
    name: &'static str,
    part_index: usize,
    part_fp: PartFingerprint,
    note: UNote,
    old_duration: i32,
    new_duration: i32,
}

impl AddNoteCommand {
    /// `AddNoteCommand(part, note)`.
    ///
    /// `part_index` must refer to a voice part in `project`.
    pub fn new(project: &UProject, part_index: usize, note: UNote) -> Result<Self, String> {
        let part = match project.parts.get(part_index) {
            Some(UPart::Voice(vp)) => vp,
            _ => return Err("part index does not refer to a voice part".to_string()),
        };
        let old_duration = part.duration;
        let min_dur = min_dur_tick_for_note_edit(project, part, note.end());
        let new_duration = if part.duration < min_dur { min_dur } else { 0 };
        Ok(AddNoteCommand {
            name: "Add note",
            part_index,
            part_fp: PartFingerprint::of(part),
            note,
            old_duration,
            new_duration,
        })
    }
}

impl Command for AddNoteCommand {
    fn execute(&mut self, project: &mut UProject) -> Result<(), String> {
        if self.note.duration <= 0 {
            return Err("cannot add a note with non-positive duration".to_string());
        }
        if self.note.position < 0 {
            return Err("cannot add a note at a negative position".to_string());
        }
        let part = locate_part_mut(project, self.part_index, &self.part_fp)?;
        part.notes.push(self.note.clone());
        if self.new_duration > 0 {
            part.duration = self.new_duration;
        }
        Ok(())
    }

    fn unexecute(&mut self, project: &mut UProject) -> Result<(), String> {
        let part = locate_part_mut(project, self.part_index, &self.part_fp)?;
        let idx = part
            .notes
            .iter()
            .rposition(|n| n == &self.note)
            .ok_or_else(|| "added note not found in part".to_string())?;
        part.notes.remove(idx);
        part.duration = self.old_duration;
        Ok(())
    }

    fn name(&self) -> &str {
        self.name
    }
}

/// Remove a note from a voice part (`RemoveNoteCommand`).
pub struct RemoveNoteCommand {
    name: &'static str,
    part_index: usize,
    part_fp: PartFingerprint,
    note_index: usize,
    note: UNote,
}

impl RemoveNoteCommand {
    /// `RemoveNoteCommand(part, note)`.
    pub fn new(project: &UProject, part_index: usize, note_index: usize) -> Result<Self, String> {
        let edit = NoteEdit::from_project(project, part_index, note_index)?;
        Ok(RemoveNoteCommand {
            name: "Remove note",
            part_index: edit.part_index,
            part_fp: edit.part_fp,
            note_index: edit.note_index,
            note: edit.old_note,
        })
    }
}

impl Command for RemoveNoteCommand {
    fn execute(&mut self, project: &mut UProject) -> Result<(), String> {
        let part = locate_part_mut(project, self.part_index, &self.part_fp)?;
        let idx = locate_note_index(part, self.note_index, &self.note, None)?;
        part.notes.remove(idx);
        Ok(())
    }

    fn unexecute(&mut self, project: &mut UProject) -> Result<(), String> {
        let part = locate_part_mut(project, self.part_index, &self.part_fp)?;
        let idx = restore_insert_index(
            &part.notes,
            self.note_index,
            |n| n.position,
            &self.note.position,
        );
        part.notes.insert(idx, self.note.clone());
        Ok(())
    }

    fn name(&self) -> &str {
        self.name
    }
}

/// Move a note by `delta_pos` ticks and `delta_tone` semitones
/// (`MoveNoteCommand`).
pub struct MoveNoteCommand {
    name: &'static str,
    edit: NoteEdit,
    old_duration: i32,
    new_duration: i32,
}

impl MoveNoteCommand {
    /// `MoveNoteCommand(part, note, deltaPos, deltaNoteNum)`.
    pub fn new(
        project: &UProject,
        part_index: usize,
        note_index: usize,
        delta_pos: i32,
        delta_tone: i32,
    ) -> Result<Self, String> {
        let mut edit = NoteEdit::from_project(project, part_index, note_index)?;
        let part = match project.parts.get(part_index) {
            Some(UPart::Voice(vp)) => vp,
            _ => return Err("part index does not refer to a voice part".to_string()),
        };
        edit.new_note.position += delta_pos;
        edit.new_note.tone += delta_tone;
        let old_duration = part.duration;
        let min_dur = min_dur_tick_for_note_edit(project, part, edit.old_note.end() + delta_pos);
        let new_duration = if part.duration < min_dur { min_dur } else { 0 };
        Ok(MoveNoteCommand {
            name: "Move note",
            edit,
            old_duration,
            new_duration,
        })
    }
}

impl Command for MoveNoteCommand {
    fn execute(&mut self, project: &mut UProject) -> Result<(), String> {
        execute_note_edit(project, &self.edit, Some(self.new_duration))
    }

    fn unexecute(&mut self, project: &mut UProject) -> Result<(), String> {
        unexecute_note_edit(project, &self.edit, Some(self.old_duration))
    }

    fn name(&self) -> &str {
        self.name
    }
}

/// Resize a note (`ResizeNoteCommand`).
///
/// `from_start == false` moves the right edge (`duration += delta_dur`);
/// `from_start == true` moves the left edge (`position -= delta_dur`,
/// `duration += delta_dur`, end unchanged).
pub struct ResizeNoteCommand {
    name: &'static str,
    edit: NoteEdit,
    old_duration: i32,
    new_duration: i32,
}

impl ResizeNoteCommand {
    /// `ResizeNoteCommand(part, note, deltaDur)` extended with a `fromStart`
    /// flag mirroring `ResizeVoicePartCommand`.
    pub fn new(
        project: &UProject,
        part_index: usize,
        note_index: usize,
        delta_dur: i32,
        from_start: bool,
    ) -> Result<Self, String> {
        let mut edit = NoteEdit::from_project(project, part_index, note_index)?;
        let part = match project.parts.get(part_index) {
            Some(UPart::Voice(vp)) => vp,
            _ => return Err("part index does not refer to a voice part".to_string()),
        };
        if from_start {
            edit.new_note.position -= delta_dur;
        }
        edit.new_note.duration += delta_dur;
        let old_duration = part.duration;
        let end_after = if from_start {
            edit.old_note.end()
        } else {
            edit.old_note.end() + delta_dur
        };
        let min_dur = min_dur_tick_for_note_edit(project, part, end_after);
        let new_duration = if part.duration < min_dur { min_dur } else { 0 };
        Ok(ResizeNoteCommand {
            name: "Resize note",
            edit,
            old_duration,
            new_duration,
        })
    }
}

impl Command for ResizeNoteCommand {
    fn execute(&mut self, project: &mut UProject) -> Result<(), String> {
        if self.edit.new_note.duration <= 0 {
            return Err("cannot resize a note to a non-positive duration".to_string());
        }
        execute_note_edit(project, &self.edit, Some(self.new_duration))
    }

    fn unexecute(&mut self, project: &mut UProject) -> Result<(), String> {
        unexecute_note_edit(project, &self.edit, Some(self.old_duration))
    }

    fn name(&self) -> &str {
        self.name
    }
}

/// Change a note's lyric (`ChangeNoteLyricCommand`).
pub struct ChangeNoteLyricCommand {
    name: &'static str,
    edit: NoteEdit,
}

impl ChangeNoteLyricCommand {
    /// `ChangeNoteLyricCommand(part, note, newLyric)`.
    pub fn new(
        project: &UProject,
        part_index: usize,
        note_index: usize,
        new_lyric: impl Into<String>,
    ) -> Result<Self, String> {
        let mut edit = NoteEdit::from_project(project, part_index, note_index)?;
        edit.new_note.lyric = new_lyric.into();
        Ok(ChangeNoteLyricCommand {
            name: "Change lyric",
            edit,
        })
    }
}

impl Command for ChangeNoteLyricCommand {
    fn execute(&mut self, project: &mut UProject) -> Result<(), String> {
        execute_note_edit(project, &self.edit, None)
    }

    fn unexecute(&mut self, project: &mut UProject) -> Result<(), String> {
        unexecute_note_edit(project, &self.edit, None)
    }

    fn name(&self) -> &str {
        self.name
    }
}

/// Change a note's tone (`ChangeNoteToneCommand`).
pub struct ChangeNoteToneCommand {
    name: &'static str,
    edit: NoteEdit,
}

impl ChangeNoteToneCommand {
    /// Set `note.tone` to `new_tone`.
    pub fn new(
        project: &UProject,
        part_index: usize,
        note_index: usize,
        new_tone: i32,
    ) -> Result<Self, String> {
        let mut edit = NoteEdit::from_project(project, part_index, note_index)?;
        edit.new_note.tone = new_tone;
        Ok(ChangeNoteToneCommand {
            name: "Change tone",
            edit,
        })
    }
}

impl Command for ChangeNoteToneCommand {
    fn execute(&mut self, project: &mut UProject) -> Result<(), String> {
        execute_note_edit(project, &self.edit, None)
    }

    fn unexecute(&mut self, project: &mut UProject) -> Result<(), String> {
        unexecute_note_edit(project, &self.edit, None)
    }

    fn name(&self) -> &str {
        self.name
    }
}

/// Change a note's tuning in cents (`ChangeNoteTuningCommand`).
pub struct ChangeNoteTuningCommand {
    name: &'static str,
    edit: NoteEdit,
}

impl ChangeNoteTuningCommand {
    /// `ChangeNoteTuningCommand(part, note, newTuning)`.
    pub fn new(
        project: &UProject,
        part_index: usize,
        note_index: usize,
        new_tuning: i32,
    ) -> Result<Self, String> {
        let mut edit = NoteEdit::from_project(project, part_index, note_index)?;
        edit.new_note.tuning = new_tuning;
        Ok(ChangeNoteTuningCommand {
            name: "Change tuning",
            edit,
        })
    }
}

impl Command for ChangeNoteTuningCommand {
    fn execute(&mut self, project: &mut UProject) -> Result<(), String> {
        execute_note_edit(project, &self.edit, None)
    }

    fn unexecute(&mut self, project: &mut UProject) -> Result<(), String> {
        unexecute_note_edit(project, &self.edit, None)
    }

    fn name(&self) -> &str {
        self.name
    }
}
