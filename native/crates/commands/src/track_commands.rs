//! Track commands — mirror of `OpenUtau.Core/Commands/TrackCommands.cs`.
//!
//! In this domain model a track's number is its index in `UProject.tracks`
//! (the domain's `UTrack` has no `TrackNo` field), while parts store an
//! explicit `track_no`. Track add/remove commands therefore remap part
//! track numbers, playing the role of OpenUtau's `TrackCommand.UpdateTrackNo`.
//!
//! Tracks are located by value equality with the construction-time
//! snapshot, falling back to the construction-time index hint.

use domain::{UPart, UProject, UTrack};

use crate::command::Command;

/// Index of the first track equal to `expected`, preferring `hint`.
fn locate_track(project: &UProject, hint: usize, expected: &UTrack) -> Option<usize> {
    if project.tracks.get(hint) == Some(expected) {
        return Some(hint);
    }
    project.tracks.iter().position(|t| t == expected)
}

/// Replace the track equal to `expected` with `replacement`.
fn replace_track(
    project: &mut UProject,
    hint: usize,
    expected: &UTrack,
    replacement: &UTrack,
) -> Result<(), String> {
    let idx = locate_track(project, hint, expected).ok_or_else(|| "track not found".to_string())?;
    project.tracks[idx] = replacement.clone();
    Ok(())
}

fn set_part_track_no(part: &mut UPart, track_no: i32) {
    match part {
        UPart::Voice(vp) => vp.track_no = track_no,
        UPart::Wave(wp) => wp.track_no = track_no,
    }
}

/// `TrackCommand.UpdateTrackNo` for a track inserted at `idx`: parts that
/// pointed at `idx` or later now point one track further down.
fn remap_parts_after_insert(project: &mut UProject, idx: usize) {
    let idx = idx as i32;
    for part in &mut project.parts {
        if part.track_no() >= idx {
            set_part_track_no(part, part.track_no() + 1);
        }
    }
}

/// `TrackCommand.UpdateTrackNo` for a track removed at `idx`: parts that
/// pointed past `idx` now point one track earlier.
fn remap_parts_after_remove(project: &mut UProject, idx: usize) {
    let idx = idx as i32;
    for part in &mut project.parts {
        if part.track_no() > idx {
            set_part_track_no(part, part.track_no() - 1);
        }
    }
}

/// Add a track (`AddTrackCommand`).
///
/// The track is inserted at `track_index` (clamped to the end of the track
/// list, like OpenUtau's `track.TrackNo < project.tracks.Count` check).
pub struct AddTrackCommand {
    name: &'static str,
    track_index: usize,
    track: UTrack,
}

impl AddTrackCommand {
    pub fn new(track: UTrack, track_index: usize) -> Self {
        AddTrackCommand {
            name: "Add track",
            track_index,
            track,
        }
    }
}

impl Command for AddTrackCommand {
    fn execute(&mut self, project: &mut UProject) -> Result<(), String> {
        let idx = self.track_index.min(project.tracks.len());
        project.tracks.insert(idx, self.track.clone());
        remap_parts_after_insert(project, idx);
        Ok(())
    }

    fn unexecute(&mut self, project: &mut UProject) -> Result<(), String> {
        let idx = locate_track(project, self.track_index, &self.track)
            .ok_or_else(|| "added track not found in project".to_string())?;
        project.tracks.remove(idx);
        remap_parts_after_remove(project, idx);
        Ok(())
    }

    fn name(&self) -> &str {
        self.name
    }
}

/// Remove a track and the parts on it (`RemoveTrackCommand`).
///
/// Parts on the removed track are captured at construction and restored by
/// unexecute with their original track number.
pub struct RemoveTrackCommand {
    name: &'static str,
    track_index: usize,
    track: UTrack,
    removed_parts: Vec<UPart>,
}

impl RemoveTrackCommand {
    pub fn new(project: &UProject, track_index: usize) -> Result<Self, String> {
        let track = project
            .tracks
            .get(track_index)
            .cloned()
            .ok_or_else(|| "track index out of bounds".to_string())?;
        let removed_parts = project
            .parts
            .iter()
            .filter(|p| p.track_no() == track_index as i32)
            .cloned()
            .collect();
        Ok(RemoveTrackCommand {
            name: "Remove track",
            track_index,
            track,
            removed_parts,
        })
    }
}

impl Command for RemoveTrackCommand {
    fn execute(&mut self, project: &mut UProject) -> Result<(), String> {
        let idx = locate_track(project, self.track_index, &self.track)
            .ok_or_else(|| "track not found".to_string())?;
        project.tracks.remove(idx);
        for part in &self.removed_parts {
            if let Some(p) = project.parts.iter().position(|p| p == part) {
                project.parts.remove(p);
            }
        }
        remap_parts_after_remove(project, idx);
        Ok(())
    }

    fn unexecute(&mut self, project: &mut UProject) -> Result<(), String> {
        let idx = self.track_index.min(project.tracks.len());
        project.tracks.insert(idx, self.track.clone());
        remap_parts_after_insert(project, idx);
        // Re-add the removed parts at their canonical (track, position)
        // order; the project's part list is kept sorted like OpenUtau's
        // post-validation state.
        for part in &self.removed_parts {
            let key = (part.track_no(), part.position());
            let pos = project
                .parts
                .iter()
                .position(|p| (p.track_no(), p.position()) > key)
                .unwrap_or(project.parts.len());
            project.parts.insert(pos, part.clone());
        }
        Ok(())
    }

    fn name(&self) -> &str {
        self.name
    }
}

/// Rename a track (`RenameTrackCommand`).
pub struct RenameTrackCommand {
    name: &'static str,
    track_index: usize,
    old_track: UTrack,
    new_track: UTrack,
}

impl RenameTrackCommand {
    pub fn new(
        project: &UProject,
        track_index: usize,
        new_name: impl Into<String>,
    ) -> Result<Self, String> {
        let old_track = project
            .tracks
            .get(track_index)
            .cloned()
            .ok_or_else(|| "track index out of bounds".to_string())?;
        let mut new_track = old_track.clone();
        new_track.track_name = new_name.into();
        Ok(RenameTrackCommand {
            name: "Rename track",
            track_index,
            old_track,
            new_track,
        })
    }
}

impl Command for RenameTrackCommand {
    fn execute(&mut self, project: &mut UProject) -> Result<(), String> {
        replace_track(project, self.track_index, &self.old_track, &self.new_track)
    }

    fn unexecute(&mut self, project: &mut UProject) -> Result<(), String> {
        replace_track(project, self.track_index, &self.new_track, &self.old_track)
    }

    fn name(&self) -> &str {
        self.name
    }
}

/// Set a track's solo flag.
pub struct SetTrackSoloCommand {
    name: &'static str,
    track_index: usize,
    old_track: UTrack,
    new_track: UTrack,
}

impl SetTrackSoloCommand {
    pub fn new(project: &UProject, track_index: usize, solo: bool) -> Result<Self, String> {
        let old_track = project
            .tracks
            .get(track_index)
            .cloned()
            .ok_or_else(|| "track index out of bounds".to_string())?;
        let mut new_track = old_track.clone();
        new_track.solo = solo;
        Ok(SetTrackSoloCommand {
            name: "Set track solo",
            track_index,
            old_track,
            new_track,
        })
    }
}

impl Command for SetTrackSoloCommand {
    fn execute(&mut self, project: &mut UProject) -> Result<(), String> {
        replace_track(project, self.track_index, &self.old_track, &self.new_track)
    }

    fn unexecute(&mut self, project: &mut UProject) -> Result<(), String> {
        replace_track(project, self.track_index, &self.new_track, &self.old_track)
    }

    fn name(&self) -> &str {
        self.name
    }
}

/// Set a track's mute flag.
pub struct SetTrackMuteCommand {
    name: &'static str,
    track_index: usize,
    old_track: UTrack,
    new_track: UTrack,
}

impl SetTrackMuteCommand {
    pub fn new(project: &UProject, track_index: usize, mute: bool) -> Result<Self, String> {
        let old_track = project
            .tracks
            .get(track_index)
            .cloned()
            .ok_or_else(|| "track index out of bounds".to_string())?;
        let mut new_track = old_track.clone();
        new_track.mute = mute;
        Ok(SetTrackMuteCommand {
            name: "Set track mute",
            track_index,
            old_track,
            new_track,
        })
    }
}

impl Command for SetTrackMuteCommand {
    fn execute(&mut self, project: &mut UProject) -> Result<(), String> {
        replace_track(project, self.track_index, &self.old_track, &self.new_track)
    }

    fn unexecute(&mut self, project: &mut UProject) -> Result<(), String> {
        replace_track(project, self.track_index, &self.new_track, &self.old_track)
    }

    fn name(&self) -> &str {
        self.name
    }
}

/// Set a track's volume (dB).
pub struct SetTrackVolumeCommand {
    name: &'static str,
    track_index: usize,
    old_track: UTrack,
    new_track: UTrack,
}

impl SetTrackVolumeCommand {
    pub fn new(project: &UProject, track_index: usize, volume: f64) -> Result<Self, String> {
        let old_track = project
            .tracks
            .get(track_index)
            .cloned()
            .ok_or_else(|| "track index out of bounds".to_string())?;
        let mut new_track = old_track.clone();
        new_track.volume = volume;
        Ok(SetTrackVolumeCommand {
            name: "Set track volume",
            track_index,
            old_track,
            new_track,
        })
    }
}

impl Command for SetTrackVolumeCommand {
    fn execute(&mut self, project: &mut UProject) -> Result<(), String> {
        replace_track(project, self.track_index, &self.old_track, &self.new_track)
    }

    fn unexecute(&mut self, project: &mut UProject) -> Result<(), String> {
        replace_track(project, self.track_index, &self.new_track, &self.old_track)
    }

    fn name(&self) -> &str {
        self.name
    }
}

/// Set a track's pan.
pub struct SetTrackPanCommand {
    name: &'static str,
    track_index: usize,
    old_track: UTrack,
    new_track: UTrack,
}

impl SetTrackPanCommand {
    pub fn new(project: &UProject, track_index: usize, pan: f64) -> Result<Self, String> {
        let old_track = project
            .tracks
            .get(track_index)
            .cloned()
            .ok_or_else(|| "track index out of bounds".to_string())?;
        let mut new_track = old_track.clone();
        new_track.pan = pan;
        Ok(SetTrackPanCommand {
            name: "Set track pan",
            track_index,
            old_track,
            new_track,
        })
    }
}

impl Command for SetTrackPanCommand {
    fn execute(&mut self, project: &mut UProject) -> Result<(), String> {
        replace_track(project, self.track_index, &self.old_track, &self.new_track)
    }

    fn unexecute(&mut self, project: &mut UProject) -> Result<(), String> {
        replace_track(project, self.track_index, &self.new_track, &self.old_track)
    }

    fn name(&self) -> &str {
        self.name
    }
}
