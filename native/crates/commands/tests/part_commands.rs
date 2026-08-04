//! Round-trip tests for the part commands.

mod common;

use commands::{
    AddPartCommand, Command, MovePartCommand, RemovePartCommand, RenamePartCommand,
    ResizeVoicePartCommand, ResizeWavePartCommand,
};
use domain::{UCurve, UPart};

#[test]
fn add_part_roundtrip() {
    let mut project = common::voice_project();
    let before = project.clone();
    let mut cmd = AddPartCommand::new(UPart::Voice(common::voice_part("New", 0, 1920, 480)));
    assert_eq!(cmd.name(), "Add part");

    cmd.execute(&mut project).unwrap();
    assert_eq!(project.parts.len(), 2);
    assert_eq!(project.parts[1].display_name(), "New");

    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn remove_part_roundtrip() {
    let mut project = common::voice_project();
    project
        .parts
        .push(UPart::Wave(common::wave_part("W1", 0, 480)));
    let before = project.clone();
    let mut cmd = RemovePartCommand::new(&project, 1).unwrap();
    assert_eq!(cmd.name(), "Remove part");

    cmd.execute(&mut project).unwrap();
    assert_eq!(project.parts.len(), 1);

    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn remove_part_restores_order_after_other_removals() {
    let mut project = common::voice_project();
    project
        .parts
        .push(UPart::Wave(common::wave_part("W1", 0, 480)));
    project
        .parts
        .push(UPart::Wave(common::wave_part("W2", 0, 960)));
    let before = project.clone();

    // Both constructed against the original state, then executed in order.
    let mut first = RemovePartCommand::new(&project, 0).unwrap();
    let mut second = RemovePartCommand::new(&project, 1).unwrap();

    first.execute(&mut project).unwrap();
    second.execute(&mut project).unwrap();
    assert_eq!(project.parts.len(), 1);
    assert_eq!(project.parts[0].display_name(), "W2");

    second.unexecute(&mut project).unwrap();
    first.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn move_part_roundtrip() {
    let mut project = common::voice_project();
    let before = project.clone();
    let mut cmd = MovePartCommand::new(&project, 0, 480, 1).unwrap();
    assert_eq!(cmd.name(), "Move part");

    cmd.execute(&mut project).unwrap();
    assert_eq!(project.parts[0].position(), 480);
    assert_eq!(project.parts[0].track_no(), 1);

    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn move_part_rejects_negative_track() {
    let project = common::voice_project();
    assert!(MovePartCommand::new(&project, 0, 0, -1).is_err());
}

#[test]
fn resize_voice_part_right_edge_roundtrip() {
    let mut project = common::project_with_notes();
    let before = project.clone();
    let mut cmd = ResizeVoicePartCommand::new(&project, 0, 480, false).unwrap();
    assert_eq!(cmd.name(), "Resize part");

    cmd.execute(&mut project).unwrap();
    if let UPart::Voice(vp) = &project.parts[0] {
        assert_eq!(vp.duration, 2400);
        assert_eq!(vp.position, 0);
        // notes untouched when resizing from the right edge
        assert_eq!(vp.notes[1].position, 480);
    }

    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn resize_voice_part_from_start_roundtrip() {
    let mut project = common::project_with_notes();
    // A curve on the part so the xs shift is observable.
    if let UPart::Voice(vp) = &mut project.parts[0] {
        vp.curves.push(UCurve {
            abbr: "dyn".into(),
            xs: vec![0, 480, 960],
            ys: vec![0, 100, 0],
        });
    }
    let before = project.clone();
    let mut cmd = ResizeVoicePartCommand::new(&project, 0, 240, true).unwrap();

    cmd.execute(&mut project).unwrap();
    if let UPart::Voice(vp) = &project.parts[0] {
        assert_eq!(vp.position, -240);
        assert_eq!(vp.duration, 2160);
        assert_eq!(vp.notes[1].position, 720); // shifted +240
        assert_eq!(vp.curves[0].xs, vec![240, 720, 1200]);
    }

    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn resize_voice_part_rejects_negative_duration() {
    let project = common::voice_project();
    // duration 1920 - 3000 < 0
    assert!(ResizeVoicePartCommand::new(&project, 0, -3000, false).is_err());
}

#[test]
fn resize_wave_part_roundtrip() {
    let mut project = common::voice_project();
    project
        .parts
        .push(UPart::Wave(common::wave_part("W1", 0, 480)));
    let before = project.clone();
    let mut cmd = ResizeWavePartCommand::new(&project, 1, 120, true).unwrap();

    cmd.execute(&mut project).unwrap();
    if let UPart::Wave(wp) = &project.parts[1] {
        assert_eq!(wp.position, 360);
        assert_eq!(wp.skip, -120);
    }

    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);

    let mut cmd = ResizeWavePartCommand::new(&project, 1, 120, false).unwrap();
    cmd.execute(&mut project).unwrap();
    if let UPart::Wave(wp) = &project.parts[1] {
        assert_eq!(wp.trim, -120);
        assert_eq!(wp.position, 480);
    }
    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn resize_wave_part_rejects_voice_part() {
    assert!(ResizeWavePartCommand::new(&common::voice_project(), 0, 120, false).is_err());
}

#[test]
fn rename_part_roundtrip() {
    let mut project = common::voice_project();
    let before = project.clone();
    let mut cmd = RenamePartCommand::new(&project, 0, "Vocals").unwrap();
    assert_eq!(cmd.name(), "Rename part");

    cmd.execute(&mut project).unwrap();
    assert_eq!(project.parts[0].display_name(), "Vocals");

    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn constructors_validate_part_index() {
    let project = common::voice_project();
    assert!(RemovePartCommand::new(&project, 7).is_err());
    assert!(MovePartCommand::new(&project, 7, 0, 0).is_err());
    assert!(ResizeVoicePartCommand::new(&project, 7, 10, false).is_err());
    assert!(RenamePartCommand::new(&project, 7, "x").is_err());
}
