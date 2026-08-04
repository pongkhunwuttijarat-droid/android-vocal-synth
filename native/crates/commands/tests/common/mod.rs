//! Shared test fixtures for the `commands` crate integration tests.

// Each test binary compiles this module separately and uses a different
// subset of the helpers, so dead-code warnings are expected per-binary.
#![allow(dead_code)]

use domain::{UPart, UProject, UVoicePart, UWavePart};

/// A project with default expressions, one track (`Track1`) and one empty
/// voice part on track 0 (position 0, duration 1920 = 4 beats at 480 tpqn).
pub fn voice_project() -> UProject {
    let mut project = UProject::create();
    project.parts.push(UPart::Voice(UVoicePart {
        name: "Part 1".into(),
        track_no: 0,
        position: 0,
        duration: 1920,
        ..Default::default()
    }));
    project
}

/// A voice part on track 0 holding three notes:
/// tone 60 @ 0..480, tone 62 @ 480..960, tone 64 @ 960..1440.
pub fn project_with_notes() -> UProject {
    let mut project = voice_project();
    let notes = vec![
        project.create_note_at(60, 0, 480),
        project.create_note_at(62, 480, 480),
        project.create_note_at(64, 960, 480),
    ];
    if let UPart::Voice(vp) = &mut project.parts[0] {
        vp.notes.extend(notes);
    }
    project
}

/// A voice part with the given name, track and position, optionally with
/// notes.
pub fn voice_part(name: &str, track_no: i32, position: i32, duration: i32) -> UVoicePart {
    UVoicePart {
        name: name.into(),
        track_no,
        position,
        duration,
        ..Default::default()
    }
}

/// A wave part on track 0.
pub fn wave_part(name: &str, track_no: i32, position: i32) -> UWavePart {
    UWavePart {
        name: name.into(),
        track_no,
        position,
        relative_path: Some(format!("audio/{name}.wav")),
        file_duration_ms: 1000.0,
        ..Default::default()
    }
}
