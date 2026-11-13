//! `note.set_pitch` — the per-note pitch curve (CONTRACT-v1 T014).
//!
//! The *model* and the *renderer* already had pitch curves: `domain::UPitch`
//! and `feed::PitchComputer::apply_pitch_points` read `note.pitch.data` and
//! interpolate it. What did not exist was any way to write or read one over the
//! logical API, so the editor had no pitch editor to bind to. These cover the
//! new write path and the new `NoteView.pitch` field it needs to read back:
//! commit, read-back, undo as one transaction, point validation leaving the
//! document untouched, and a USTX round trip.

use project_session::v2::MutationResult;
use project_session::Session;
use serde_json::{json, Value};

const DEMO: &str = include_str!("../../../tools/synth-cli/tests/data/demo-song.ustx");

/// Boot a session on the demo song and return it with the first note id.
fn session_with_note() -> (Session, String) {
    let session = Session::open(DEMO, None, 1).expect("demo song opens");
    let snapshot: Value =
        serde_json::from_str(&session.snapshot_json().expect("snapshot")).expect("snapshot json");
    let notes = notes_of(&snapshot);
    assert!(!notes.is_empty(), "the demo song has notes");
    let note_id = notes[0]["id"].as_str().expect("note id").to_string();
    (session, note_id)
}

fn notes_of(snapshot: &Value) -> Vec<Value> {
    snapshot["project"]["tracks"]
        .as_array()
        .expect("tracks")
        .iter()
        .flat_map(|track| track["parts"].as_array().into_iter().flatten())
        .flat_map(|part| part["notes"].as_array().into_iter().flatten())
        .cloned()
        .collect()
}

fn apply(session: &mut Session, command: Value) -> Result<MutationResult, String> {
    let expected = session.revision();
    session
        .apply_v2(&json!({ "expected_revision": expected, "command": command }).to_string())
        .map_err(|error| error.to_string())
}

fn pitch_of(session: &Session) -> Value {
    let snapshot: Value =
        serde_json::from_str(&session.snapshot_json().expect("snapshot")).expect("snapshot json");
    notes_of(&snapshot)[0]["pitch"].clone()
}

fn curve(x: &[(f64, f64)], shape: &str) -> Value {
    json!({
        "data": x.iter().map(|(px, py)| json!({ "x": px, "y": py, "shape": shape })).collect::<Vec<_>>(),
        "snap_first": true,
    })
}

#[test]
fn a_pitch_curve_commits_and_is_read_back_in_the_note_view() {
    let (mut session, note_id) = session_with_note();
    let before = session.revision();

    // x is milliseconds from the note start, y tenths of a semitone — the
    // units `domain::PitchPoint` and the USTX codec already use.
    let result = apply(
        &mut session,
        json!({
            "type": "note.set_pitch",
            "note_id": note_id,
            "pitch": curve(&[(0.0, 0.0), (120.0, 35.0), (240.0, -20.0)], "io"),
        }),
    )
    .expect("pitch commits");

    assert_eq!(session.revision(), before + 1, "one revision per command");
    let kinds: Vec<&str> = result
        .changes
        .iter()
        .map(|change| change.kind.as_str())
        .collect();
    assert!(
        kinds.contains(&"note.changed"),
        "patch names the note: {kinds:?}"
    );
    assert!(
        kinds.contains(&"render.invalidated"),
        "a pitch change must invalidate the render: {kinds:?}"
    );

    let read = pitch_of(&session);
    assert_eq!(read["snap_first"], json!(true));
    assert_eq!(read["data"].as_array().expect("data").len(), 3);
    assert_eq!(read["data"][1]["x"], json!(120.0));
    assert_eq!(read["data"][1]["y"], json!(35.0));
    assert_eq!(read["data"][1]["shape"], json!("io"));
}

#[test]
fn every_interpolation_shape_survives_the_round_trip() {
    // OpenUtau's `PitchPointShape`: io/l/i/o/sp. `sp` is the one the renderer
    // treats specially (cubic spline when the note has >2 points), so it has to
    // survive the wire rather than collapse to the default.
    for shape in ["io", "l", "i", "o", "sp"] {
        let (mut session, note_id) = session_with_note();
        apply(
            &mut session,
            json!({
                "type": "note.set_pitch",
                "note_id": note_id,
                "pitch": curve(&[(0.0, 0.0), (100.0, 10.0), (200.0, 0.0)], shape),
            }),
        )
        .unwrap_or_else(|error| panic!("{shape} commits: {error}"));
        let read = pitch_of(&session);
        assert_eq!(read["data"][1]["shape"], json!(shape), "shape {shape}");
    }
}

#[test]
fn drawing_a_pitch_curve_undoes_as_one_transaction() {
    let (mut session, note_id) = session_with_note();
    // The demo song's notes already carry a two-point curve (x: -1, x: 1), so
    // "undo restores the previous curve" is measured against what was really
    // there rather than against an assumed empty one.
    let original = pitch_of(&session);

    apply(
        &mut session,
        json!({
            "type": "note.set_pitch",
            "note_id": note_id,
            "pitch": curve(&[(0.0, 0.0), (150.0, 50.0)], "io"),
        }),
    )
    .expect("commit");
    let drawn = session.revision();
    assert_eq!(
        pitch_of(&session)["data"].as_array().expect("data").len(),
        2
    );

    session.undo().expect("undo");
    assert_eq!(session.revision(), drawn + 1, "undo is its own revision");
    assert_eq!(
        pitch_of(&session),
        original,
        "undo restores the curve exactly as it was"
    );

    session.redo().expect("redo");
    assert_eq!(pitch_of(&session)["data"][1]["y"], json!(50.0));
}

#[test]
fn a_rejected_curve_leaves_the_document_and_history_untouched() {
    let (mut session, note_id) = session_with_note();
    let original = pitch_of(&session);
    // Two points at the same x: `UPitch::Sample` and the feed's interpolation
    // both need strictly increasing x, so this must be refused rather than
    // accepted with an ambiguous span.
    let before = session.revision();
    let error = apply(
        &mut session,
        json!({
            "type": "note.set_pitch",
            "note_id": note_id,
            "pitch": curve(&[(100.0, 0.0), (100.0, 10.0)], "io"),
        }),
    )
    .expect_err("duplicate x must be refused");
    assert!(
        error.contains("strictly increase"),
        "the error names the rule: {error}"
    );
    assert_eq!(session.revision(), before, "no revision on refusal");
    assert_eq!(pitch_of(&session), original, "the curve is untouched");

    // An unknown shape is refused the same way: silently defaulting it would
    // draw a different curve than the user asked for.
    let error = apply(
        &mut session,
        json!({
            "type": "note.set_pitch",
            "note_id": note_id,
            "pitch": curve(&[(0.0, 0.0)], "banana"),
        }),
    )
    .expect_err("unknown shape must be refused");
    assert!(
        error.contains("banana"),
        "the error names the shape: {error}"
    );
    assert_eq!(session.revision(), before, "still no revision");
    assert_eq!(pitch_of(&session), original);

    // A point before the note start is refused. Files carry such curves as
    // placeholders (the demo song stores x -40…40) and the import codec keeps
    // reading them, but a command is a finished curve the editor drew, and it
    // always anchors the first point at 0.
    let error = apply(
        &mut session,
        json!({
            "type": "note.set_pitch",
            "note_id": note_id,
            "pitch": curve(&[(-40.0, 0.0), (40.0, 0.0)], "io"),
        }),
    )
    .expect_err("a point before the note must be refused");
    assert!(
        error.contains("before the note"),
        "the error names the rule: {error}"
    );
    assert_eq!(session.revision(), before, "still no revision");
    assert_eq!(pitch_of(&session), original);
}

#[test]
fn an_unknown_note_is_not_found_and_an_absent_pitch_clears_the_curve() {
    let (mut session, note_id) = session_with_note();
    let error = apply(
        &mut session,
        json!({ "type": "note.set_pitch", "note_id": "no-such-note", "pitch": curve(&[], "io") }),
    )
    .expect_err("unknown note must be refused");
    assert!(error.contains("not_found"), "{error}");

    // No `pitch` field at all means an empty curve, which is how the editor
    // clears a note's pitch without a separate command.
    apply(
        &mut session,
        json!({ "type": "note.set_pitch", "note_id": note_id }),
    )
    .expect("clearing commits");
    let cleared = pitch_of(&session);
    assert!(
        cleared["data"].as_array().expect("data").is_empty(),
        "the curve is empty, not absent: {cleared}"
    );
}

#[test]
fn tuning_is_stable_id_addressed_bounded_readable_and_undoable() {
    let (mut session, note_id) = session_with_note();
    let original = notes_of(&serde_json::from_str(&session.snapshot_json().unwrap()).unwrap())[0]
        ["tuning_cents"]
        .clone();
    let result = apply(
        &mut session,
        json!({"type":"note.set_tuning","note_id":note_id,"cents":-275}),
    )
    .expect("valid tuning commits");
    assert!(result
        .changes
        .iter()
        .any(|change| change.kind == "note.changed"));
    assert!(result
        .changes
        .iter()
        .any(|change| change.kind == "render.invalidated"));
    let after: Value = serde_json::from_str(&session.snapshot_json().unwrap()).unwrap();
    assert_eq!(notes_of(&after)[0]["tuning_cents"], json!(-275));

    let revision = session.revision();
    let rejected = apply(
        &mut session,
        json!({"type":"note.set_tuning","note_id":note_id,"cents":1201}),
    )
    .unwrap_err();
    assert!(rejected.contains("validation_error"));
    assert_eq!(session.revision(), revision);
    assert_eq!(
        notes_of(&serde_json::from_str(&session.snapshot_json().unwrap()).unwrap())[0]
            ["tuning_cents"],
        json!(-275)
    );

    session.undo_v2(revision).unwrap();
    assert_eq!(
        notes_of(&serde_json::from_str(&session.snapshot_json().unwrap()).unwrap())[0]
            ["tuning_cents"],
        original
    );
}

#[test]
fn a_pitch_curve_survives_a_ustx_round_trip() {
    let (mut session, note_id) = session_with_note();
    apply(
        &mut session,
        json!({
            "type": "note.set_pitch",
            "note_id": note_id,
            "pitch": curve(&[(0.0, 0.0), (90.0, 25.0), (180.0, -5.0)], "sp"),
        }),
    )
    .expect("commit");

    let yaml = session.yaml();
    assert!(yaml.contains("pitch"), "the exported yaml carries pitch");

    let reopened = Session::open(&yaml, None, 2).expect("reopen");
    let read = pitch_of(&reopened);
    let data = read["data"].as_array().expect("data");
    assert_eq!(data.len(), 3, "all three points survive: {read}");
    assert_eq!(data[1]["y"], json!(25.0));
    assert_eq!(data[1]["shape"], json!("sp"), "the shape is not flattened");
}
