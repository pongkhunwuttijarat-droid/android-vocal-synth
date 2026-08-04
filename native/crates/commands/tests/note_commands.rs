//! Round-trip tests for the note commands: `execute` changes the state as
//! documented, `unexecute` restores it exactly (`PartialEq` on the whole
//! project), and validation rejects invalid edits without mutating.

mod common;

use commands::{
    AddNoteCommand, ChangeNoteLyricCommand, ChangeNoteToneCommand, ChangeNoteTuningCommand,
    Command, MoveNoteCommand, RemoveNoteCommand, ResizeNoteCommand,
};
use domain::{UPart, UProject};

fn notes(project: &UProject) -> &[domain::UNote] {
    match &project.parts[0] {
        UPart::Voice(vp) => &vp.notes,
        UPart::Wave(_) => panic!("expected voice part"),
    }
}

#[test]
fn add_note_roundtrip() {
    let mut project = common::project_with_notes();
    let before = project.clone();
    let note = project.create_note_at(65, 1440, 240);
    let mut cmd = AddNoteCommand::new(&project, 0, note).unwrap();
    assert_eq!(cmd.name(), "Add note");

    cmd.execute(&mut project).unwrap();
    assert_eq!(notes(&project).len(), 4);
    assert_eq!(notes(&project)[3].tone, 65);
    assert_eq!(notes(&project)[3].position, 1440);
    assert_eq!(notes(&project)[3].duration, 240);

    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn add_note_grows_part_duration_past_end() {
    let mut project = common::voice_project();
    if let UPart::Voice(vp) = &mut project.parts[0] {
        vp.duration = 480;
    }
    let before = project.clone();
    let note = project.create_note_at(60, 480, 480); // ends at 960, past 480
    let mut cmd = AddNoteCommand::new(&project, 0, note).unwrap();

    cmd.execute(&mut project).unwrap();
    if let UPart::Voice(vp) = &project.parts[0] {
        assert_eq!(vp.duration, 1440); // next bar beat after 960 is 1440
    }
    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn add_note_no_growth_within_part() {
    let mut project = common::voice_project();
    if let UPart::Voice(vp) = &mut project.parts[0] {
        vp.duration = 1920;
    }
    let note = project.create_note_at(60, 1440, 240); // ends at 1680 < 1920
    let mut cmd = AddNoteCommand::new(&project, 0, note).unwrap();
    cmd.execute(&mut project).unwrap();
    if let UPart::Voice(vp) = &project.parts[0] {
        assert_eq!(vp.duration, 1920);
    }
}

#[test]
fn add_note_rejects_invalid_note_and_part() {
    let mut project = common::project_with_notes();
    let before = project.clone();

    let bad = project.create_note_at(60, 0, 0); // zero duration
    let mut cmd = AddNoteCommand::new(&project, 0, bad).unwrap();
    assert!(cmd.execute(&mut project).is_err());
    assert_eq!(project, before);

    let bad = project.create_note_at(60, -10, 480); // negative position
    let mut cmd = AddNoteCommand::new(&project, 0, bad).unwrap();
    assert!(cmd.execute(&mut project).is_err());
    assert_eq!(project, before);

    assert!(AddNoteCommand::new(&project, 5, project.create_note_at(60, 0, 480)).is_err());
    assert!(AddNoteCommand::new(&project, 0, project.create_note_at(60, 0, 480)).is_ok());
}

#[test]
fn remove_note_roundtrip() {
    let mut project = common::project_with_notes();
    let before = project.clone();
    let mut cmd = RemoveNoteCommand::new(&project, 0, 1).unwrap();
    assert_eq!(cmd.name(), "Remove note");

    cmd.execute(&mut project).unwrap();
    assert_eq!(notes(&project).len(), 2);
    assert_eq!(notes(&project)[1].tone, 64);

    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn remove_note_rejects_missing_part() {
    assert!(RemoveNoteCommand::new(&common::project_with_notes(), 3, 0).is_err());
}

#[test]
fn move_note_roundtrip() {
    let mut project = common::project_with_notes();
    let before = project.clone();
    let mut cmd = MoveNoteCommand::new(&project, 0, 1, 240, -2).unwrap();
    assert_eq!(cmd.name(), "Move note");

    cmd.execute(&mut project).unwrap();
    assert_eq!(notes(&project)[1].position, 720);
    assert_eq!(notes(&project)[1].tone, 60);

    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn move_note_grows_part_duration_past_end() {
    let mut project = common::voice_project();
    let note = project.create_note_at(60, 0, 480);
    if let UPart::Voice(vp) = &mut project.parts[0] {
        vp.notes.push(note);
        vp.duration = 480;
    }
    let before = project.clone();
    // Moves the note end from 480 to 2400 (past the 480-tick part).
    let mut cmd = MoveNoteCommand::new(&project, 0, 0, 1920, 0).unwrap();
    cmd.execute(&mut project).unwrap();
    if let UPart::Voice(vp) = &project.parts[0] {
        // Next bar beat after the moved end (2400 = beat 5) is beat 6 = 2880.
        assert_eq!(vp.duration, 2880);
    }
    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn resize_note_right_edge_roundtrip() {
    let mut project = common::project_with_notes();
    let before = project.clone();
    let mut cmd = ResizeNoteCommand::new(&project, 0, 1, 240, false).unwrap();
    assert_eq!(cmd.name(), "Resize note");

    cmd.execute(&mut project).unwrap();
    assert_eq!(notes(&project)[1].position, 480);
    assert_eq!(notes(&project)[1].duration, 720);

    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn resize_note_left_edge_roundtrip() {
    let mut project = common::project_with_notes();
    let before = project.clone();
    // Move the left edge left by 120: position 480 -> 360, duration 480 -> 600.
    let mut cmd = ResizeNoteCommand::new(&project, 0, 1, 120, true).unwrap();

    cmd.execute(&mut project).unwrap();
    assert_eq!(notes(&project)[1].position, 360);
    assert_eq!(notes(&project)[1].duration, 600);
    assert_eq!(notes(&project)[1].end(), 960); // end unchanged

    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn resize_note_rejects_non_positive_duration() {
    let mut project = common::project_with_notes();
    let before = project.clone();
    // Shrink the 480-tick note by 480 -> duration 0.
    let mut cmd = ResizeNoteCommand::new(&project, 0, 1, -480, false).unwrap();
    assert!(cmd.execute(&mut project).is_err());
    assert_eq!(project, before);
}

#[test]
fn change_lyric_roundtrip() {
    let mut project = common::project_with_notes();
    let before = project.clone();
    let mut cmd = ChangeNoteLyricCommand::new(&project, 0, 1, "la").unwrap();
    assert_eq!(cmd.name(), "Change lyric");

    cmd.execute(&mut project).unwrap();
    assert_eq!(notes(&project)[1].lyric, "la");

    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn change_tone_roundtrip() {
    let mut project = common::project_with_notes();
    let before = project.clone();
    let mut cmd = ChangeNoteToneCommand::new(&project, 0, 2, 71).unwrap();
    assert_eq!(cmd.name(), "Change tone");

    cmd.execute(&mut project).unwrap();
    assert_eq!(notes(&project)[2].tone, 71);

    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn change_tuning_roundtrip() {
    let mut project = common::project_with_notes();
    let before = project.clone();
    let mut cmd = ChangeNoteTuningCommand::new(&project, 0, 0, 25).unwrap();
    assert_eq!(cmd.name(), "Change tuning");

    cmd.execute(&mut project).unwrap();
    assert_eq!(notes(&project)[0].tuning, 25);

    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn note_commands_find_notes_after_index_shift() {
    // All commands constructed against the original project, then executed
    // in order — the removal shifts the third note's index, exercising the
    // value-match fallback. Unexecute in reverse restores exactly.
    let mut project = common::project_with_notes();
    let before = project.clone();

    let mut remove = RemoveNoteCommand::new(&project, 0, 1).unwrap(); // tone 62
    let mut move_note = MoveNoteCommand::new(&project, 0, 2, 240, 1).unwrap(); // tone 64

    remove.execute(&mut project).unwrap();
    move_note.execute(&mut project).unwrap();
    assert_eq!(notes(&project).len(), 2);
    assert_eq!(notes(&project)[1].position, 1200);
    assert_eq!(notes(&project)[1].tone, 65);

    move_note.unexecute(&mut project).unwrap();
    remove.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn consecutive_moves_constructed_upfront_collapse_to_one_move() {
    // Two move commands for the same note constructed against the original
    // state: both resolve to the same target (value-replacement semantics),
    // so the net effect is a single move — and undo restores exactly.
    let mut project = common::project_with_notes();
    let before = project.clone();

    let mut first = MoveNoteCommand::new(&project, 0, 2, 240, 1).unwrap();
    let mut second = MoveNoteCommand::new(&project, 0, 2, 240, 1).unwrap();

    first.execute(&mut project).unwrap();
    second.execute(&mut project).unwrap();
    assert_eq!(notes(&project)[2].position, 1200);
    assert_eq!(notes(&project)[2].tone, 65);

    second.unexecute(&mut project).unwrap();
    first.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}
