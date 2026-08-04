//! Part commands — mirror of `OpenUtau.Core/Commands/PartCommands.cs`.
//!
//! Parts are located by value equality with the construction-time snapshot
//! (falling back to the construction-time index hint), so unexecute restores
//! the exact prior state even when list indices shifted.

use domain::{UPart, UProject, UVoicePart, UWavePart};

use crate::command::Command;
use crate::locate::restore_insert_index;

/// Canonical ordering key of a part: `(track_no, position)`.
fn part_key(part: &UPart) -> (i32, i32) {
    (part.track_no(), part.position())
}

/// Index of the first part equal to `expected`, preferring `hint`.
fn locate_part(project: &UProject, hint: usize, expected: &UPart) -> Option<usize> {
    if project.parts.get(hint) == Some(expected) {
        return Some(hint);
    }
    project.parts.iter().position(|p| p == expected)
}

/// Replace the part equal to `expected` with `replacement`.
fn replace_part(
    project: &mut UProject,
    hint: usize,
    expected: &UPart,
    replacement: &UPart,
) -> Result<(), String> {
    let idx = locate_part(project, hint, expected).ok_or_else(|| "part not found".to_string())?;
    project.parts[idx] = replacement.clone();
    Ok(())
}

/// Add a part to the project (`AddPartCommand`).
pub struct AddPartCommand {
    name: &'static str,
    part: UPart,
}

impl AddPartCommand {
    pub fn new(part: UPart) -> Self {
        AddPartCommand {
            name: "Add part",
            part,
        }
    }
}

impl Command for AddPartCommand {
    fn execute(&mut self, project: &mut UProject) -> Result<(), String> {
        project.parts.push(self.part.clone());
        Ok(())
    }

    fn unexecute(&mut self, project: &mut UProject) -> Result<(), String> {
        let idx = project
            .parts
            .iter()
            .rposition(|p| p == &self.part)
            .ok_or_else(|| "added part not found in project".to_string())?;
        project.parts.remove(idx);
        Ok(())
    }

    fn name(&self) -> &str {
        self.name
    }
}

/// Remove a part from the project (`RemovePartCommand`).
pub struct RemovePartCommand {
    name: &'static str,
    part_index: usize,
    part: UPart,
}

impl RemovePartCommand {
    pub fn new(project: &UProject, part_index: usize) -> Result<Self, String> {
        let part = project
            .parts
            .get(part_index)
            .cloned()
            .ok_or_else(|| "part index out of bounds".to_string())?;
        Ok(RemovePartCommand {
            name: "Remove part",
            part_index,
            part,
        })
    }
}

impl Command for RemovePartCommand {
    fn execute(&mut self, project: &mut UProject) -> Result<(), String> {
        let idx = locate_part(project, self.part_index, &self.part)
            .ok_or_else(|| "part not found".to_string())?;
        project.parts.remove(idx);
        Ok(())
    }

    fn unexecute(&mut self, project: &mut UProject) -> Result<(), String> {
        let idx = restore_insert_index(
            &project.parts,
            self.part_index,
            part_key,
            &part_key(&self.part),
        );
        project.parts.insert(idx, self.part.clone());
        Ok(())
    }

    fn name(&self) -> &str {
        self.name
    }
}

/// Move a part to a new position and/or track (`MovePartCommand`).
pub struct MovePartCommand {
    name: &'static str,
    part_index: usize,
    old_part: UPart,
    new_part: UPart,
}

impl MovePartCommand {
    pub fn new(
        project: &UProject,
        part_index: usize,
        new_position: i32,
        new_track_no: i32,
    ) -> Result<Self, String> {
        if new_track_no < 0 {
            return Err("cannot move a part to a negative track".to_string());
        }
        let old_part = project
            .parts
            .get(part_index)
            .cloned()
            .ok_or_else(|| "part index out of bounds".to_string())?;
        let new_part = moved_part(&old_part, new_position, new_track_no);
        Ok(MovePartCommand {
            name: "Move part",
            part_index,
            old_part,
            new_part,
        })
    }
}

fn moved_part(part: &UPart, position: i32, track_no: i32) -> UPart {
    match part {
        UPart::Voice(vp) => {
            let mut vp = vp.clone();
            vp.position = position;
            vp.track_no = track_no;
            UPart::Voice(vp)
        }
        UPart::Wave(wp) => {
            let mut wp = wp.clone();
            wp.position = position;
            wp.track_no = track_no;
            UPart::Wave(wp)
        }
    }
}

impl Command for MovePartCommand {
    fn execute(&mut self, project: &mut UProject) -> Result<(), String> {
        replace_part(project, self.part_index, &self.old_part, &self.new_part)
    }

    fn unexecute(&mut self, project: &mut UProject) -> Result<(), String> {
        replace_part(project, self.part_index, &self.new_part, &self.old_part)
    }

    fn name(&self) -> &str {
        self.name
    }
}

/// Resize a voice part (`ResizeVoicePartCommand`).
///
/// `from_start == false` extends/shrinks the right edge (`duration +=
/// delta_dur`); `from_start == true` moves the left edge (`position -=
/// delta_dur`) and shifts notes and curve points by `delta_dur`, exactly
/// like OpenUtau.
pub struct ResizeVoicePartCommand {
    name: &'static str,
    part_index: usize,
    old_part: UVoicePart,
    new_part: UVoicePart,
}

impl ResizeVoicePartCommand {
    pub fn new(
        project: &UProject,
        part_index: usize,
        delta_dur: i32,
        from_start: bool,
    ) -> Result<Self, String> {
        let old_part = match project.parts.get(part_index) {
            Some(UPart::Voice(vp)) => vp.clone(),
            _ => return Err("part index does not refer to a voice part".to_string()),
        };
        let mut new_part = old_part.clone();
        if from_start {
            new_part.position -= delta_dur;
            for note in &mut new_part.notes {
                note.position += delta_dur;
            }
            for curve in &mut new_part.curves {
                for x in &mut curve.xs {
                    *x += delta_dur;
                }
            }
        }
        new_part.duration += delta_dur;
        if new_part.duration < 0 {
            return Err("cannot resize a part to a negative duration".to_string());
        }
        Ok(ResizeVoicePartCommand {
            name: "Resize part",
            part_index,
            old_part,
            new_part,
        })
    }
}

impl Command for ResizeVoicePartCommand {
    fn execute(&mut self, project: &mut UProject) -> Result<(), String> {
        replace_part(
            project,
            self.part_index,
            &UPart::Voice(self.old_part.clone()),
            &UPart::Voice(self.new_part.clone()),
        )
    }

    fn unexecute(&mut self, project: &mut UProject) -> Result<(), String> {
        replace_part(
            project,
            self.part_index,
            &UPart::Voice(self.new_part.clone()),
            &UPart::Voice(self.old_part.clone()),
        )
    }

    fn name(&self) -> &str {
        self.name
    }
}

/// Resize a wave part (`ResizeWavePartCommand`).
///
/// `from_start` adjusts `position`/`skip`, otherwise `trim` is adjusted.
pub struct ResizeWavePartCommand {
    name: &'static str,
    part_index: usize,
    old_part: UWavePart,
    new_part: UWavePart,
}

impl ResizeWavePartCommand {
    pub fn new(
        project: &UProject,
        part_index: usize,
        delta_dur: i32,
        from_start: bool,
    ) -> Result<Self, String> {
        let old_part = match project.parts.get(part_index) {
            Some(UPart::Wave(wp)) => wp.clone(),
            _ => return Err("part index does not refer to a wave part".to_string()),
        };
        let mut new_part = old_part.clone();
        if from_start {
            new_part.position -= delta_dur;
            new_part.skip -= delta_dur;
        } else {
            new_part.trim -= delta_dur;
        }
        Ok(ResizeWavePartCommand {
            name: "Resize part",
            part_index,
            old_part,
            new_part,
        })
    }
}

impl Command for ResizeWavePartCommand {
    fn execute(&mut self, project: &mut UProject) -> Result<(), String> {
        replace_part(
            project,
            self.part_index,
            &UPart::Wave(self.old_part.clone()),
            &UPart::Wave(self.new_part.clone()),
        )
    }

    fn unexecute(&mut self, project: &mut UProject) -> Result<(), String> {
        replace_part(
            project,
            self.part_index,
            &UPart::Wave(self.new_part.clone()),
            &UPart::Wave(self.old_part.clone()),
        )
    }

    fn name(&self) -> &str {
        self.name
    }
}

/// Rename a part (`RenamePartCommand`).
pub struct RenamePartCommand {
    name: &'static str,
    part_index: usize,
    old_part: UPart,
    new_part: UPart,
}

impl RenamePartCommand {
    pub fn new(
        project: &UProject,
        part_index: usize,
        new_name: impl Into<String>,
    ) -> Result<Self, String> {
        let old_part = project
            .parts
            .get(part_index)
            .cloned()
            .ok_or_else(|| "part index out of bounds".to_string())?;
        let new_part = renamed_part(&old_part, &new_name.into());
        Ok(RenamePartCommand {
            name: "Rename part",
            part_index,
            old_part,
            new_part,
        })
    }
}

fn renamed_part(part: &UPart, name: &str) -> UPart {
    match part {
        UPart::Voice(vp) => {
            let mut vp = vp.clone();
            vp.name = name.to_string();
            UPart::Voice(vp)
        }
        UPart::Wave(wp) => {
            let mut wp = wp.clone();
            wp.name = name.to_string();
            UPart::Wave(wp)
        }
    }
}

impl Command for RenamePartCommand {
    fn execute(&mut self, project: &mut UProject) -> Result<(), String> {
        replace_part(project, self.part_index, &self.old_part, &self.new_part)
    }

    fn unexecute(&mut self, project: &mut UProject) -> Result<(), String> {
        replace_part(project, self.part_index, &self.new_part, &self.old_part)
    }

    fn name(&self) -> &str {
        self.name
    }
}
