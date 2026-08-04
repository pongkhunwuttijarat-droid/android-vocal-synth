//! Round-trip tests for the track commands, including the part track_no
//! remapping that plays the role of OpenUtau's `UpdateTrackNo`.

mod common;

use commands::{
    AddTrackCommand, Command, RemoveTrackCommand, RenameTrackCommand, SetTrackMuteCommand,
    SetTrackPanCommand, SetTrackSoloCommand, SetTrackVolumeCommand,
};
use domain::{UPart, UProject, UTrack};

/// Project with two tracks and two parts: a voice part on track 0 and a
/// wave part on track 1.
fn two_track_project() -> UProject {
    let mut project = common::voice_project();
    project.tracks.push(UTrack::new("Track2"));
    project
        .parts
        .push(UPart::Wave(common::wave_part("W1", 1, 0)));
    project
}

#[test]
fn add_track_roundtrip_with_remap() {
    let mut project = two_track_project();
    let before = project.clone();
    let mut cmd = AddTrackCommand::new(UTrack::new("TrackX"), 1);
    assert_eq!(cmd.name(), "Add track");

    cmd.execute(&mut project).unwrap();
    assert_eq!(project.tracks.len(), 3);
    assert_eq!(project.tracks[1].track_name, "TrackX");
    // Part that pointed at track 1 now points at track 2.
    assert_eq!(project.parts[1].track_no(), 2);

    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn add_track_at_end() {
    let mut project = two_track_project();
    let before = project.clone();
    let mut cmd = AddTrackCommand::new(UTrack::new("Track3"), 5); // clamped to end
    cmd.execute(&mut project).unwrap();
    assert_eq!(project.tracks.len(), 3);
    assert_eq!(project.tracks[2].track_name, "Track3");
    assert_eq!(project.parts[1].track_no(), 1); // untouched
    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn remove_track_roundtrip_with_parts() {
    let mut project = two_track_project();
    let before = project.clone();
    let mut cmd = RemoveTrackCommand::new(&project, 1).unwrap();
    assert_eq!(cmd.name(), "Remove track");

    cmd.execute(&mut project).unwrap();
    assert_eq!(project.tracks.len(), 1);
    assert_eq!(project.parts.len(), 1); // wave part on track 1 removed
    assert_eq!(project.parts[0].track_no(), 0);

    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn remove_track_zero_remaps_parts_down() {
    let mut project = two_track_project();
    let before = project.clone();
    let mut cmd = RemoveTrackCommand::new(&project, 0).unwrap();

    cmd.execute(&mut project).unwrap();
    assert_eq!(project.tracks.len(), 1);
    assert_eq!(project.tracks[0].track_name, "Track2");
    assert_eq!(project.parts.len(), 1); // voice part on track 0 removed
    assert_eq!(project.parts[0].track_no(), 0); // was 1, remapped down

    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn remove_track_rejects_bad_index() {
    assert!(RemoveTrackCommand::new(&two_track_project(), 9).is_err());
}

#[test]
fn rename_track_roundtrip() {
    let mut project = two_track_project();
    let before = project.clone();
    let mut cmd = RenameTrackCommand::new(&project, 0, "Vocals").unwrap();
    assert_eq!(cmd.name(), "Rename track");

    cmd.execute(&mut project).unwrap();
    assert_eq!(project.tracks[0].track_name, "Vocals");

    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn set_track_solo_roundtrip() {
    let mut project = two_track_project();
    let before = project.clone();
    let mut cmd = SetTrackSoloCommand::new(&project, 0, true).unwrap();
    assert_eq!(cmd.name(), "Set track solo");

    cmd.execute(&mut project).unwrap();
    assert!(project.tracks[0].solo);
    assert!(project.solo_track_exists());

    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn set_track_mute_roundtrip() {
    let mut project = two_track_project();
    let before = project.clone();
    let mut cmd = SetTrackMuteCommand::new(&project, 1, true).unwrap();
    assert_eq!(cmd.name(), "Set track mute");

    cmd.execute(&mut project).unwrap();
    assert!(project.tracks[1].mute);

    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn set_track_volume_roundtrip() {
    let mut project = two_track_project();
    let before = project.clone();
    let mut cmd = SetTrackVolumeCommand::new(&project, 0, -3.0).unwrap();
    assert_eq!(cmd.name(), "Set track volume");

    cmd.execute(&mut project).unwrap();
    assert_eq!(project.tracks[0].volume, -3.0);

    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn set_track_pan_roundtrip() {
    let mut project = two_track_project();
    let before = project.clone();
    let mut cmd = SetTrackPanCommand::new(&project, 0, 0.5).unwrap();
    assert_eq!(cmd.name(), "Set track pan");

    cmd.execute(&mut project).unwrap();
    assert_eq!(project.tracks[0].pan, 0.5);

    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn track_property_commands_validate_index() {
    let project = two_track_project();
    assert!(RenameTrackCommand::new(&project, 9, "x").is_err());
    assert!(SetTrackSoloCommand::new(&project, 9, true).is_err());
    assert!(SetTrackMuteCommand::new(&project, 9, true).is_err());
    assert!(SetTrackVolumeCommand::new(&project, 9, 0.0).is_err());
    assert!(SetTrackPanCommand::new(&project, 9, 0.0).is_err());
}
