//! v2 session surface: stable-ID commands, the mutation envelope, the change
//! vocabulary and bounded reads (CONTRACT-v1 §6.4, §6.10, §6.14).
//!
//! The legacy index-addressed path is covered by `session_identity.rs` and
//! `session.rs`; these tests drive the ID-addressed one.

use std::collections::HashSet;

use project_session::v2::{AUTHORITY_PATCH, ERR_NOT_FOUND, ERR_REVISION_CONFLICT, ERR_UNSUPPORTED};
use project_session::{ProjectDoc, Session};
use serde_json::{json, Value};

const DEMO: &str = include_str!("../../../tools/synth-cli/tests/data/demo-song.ustx");

/// One track, one part, one note and one `dyn` curve.
const CURVE_PROJECT: &str = r#"ustx_version: 0.9
name: Curve Session
bpm: 120
beat_per_bar: 4
beat_unit: 4
time_signatures:
- bar_position: 0
  beat_per_bar: 4
  beat_unit: 4
tempos:
- position: 0
  bpm: 120.0
expressions:
  dyn:
    name: dynamics (curve)
    abbr: dyn
    type: Curve
    min: -240.0
    max: 120.0
    default_value: 0.0
tracks:
- track_name: Lead
  singer: teto
voice_parts:
- name: Part1
  track_no: 0
  position: 0
  duration: 1920
  notes:
  - position: 0
    duration: 480
    tone: 60
    lyric: la
  curves:
  - abbr: dyn
    xs: [0, 480]
    ys: [0, 10]
"#;

/// The §6.4.3 vocabulary; a change entry may not invent a kind.
const VOCABULARY: [&str; 13] = [
    "project.changed",
    "track.added",
    "track.removed",
    "track.changed",
    "part.added",
    "part.removed",
    "part.changed",
    "note.added",
    "note.removed",
    "note.changed",
    "curve.changed",
    "history.changed",
    "render.invalidated",
];

fn request(revision: u64, command: Value) -> String {
    json!({"expected_revision": revision, "command": command}).to_string()
}

fn snapshot(session: &Session) -> Value {
    serde_json::from_str(&session.snapshot_json().expect("snapshot")).expect("json")
}

fn part_id(session: &Session) -> String {
    session.identity().tracks[0].parts[0]
        .id
        .as_str()
        .to_string()
}

fn track_id(session: &Session) -> String {
    session.identity().tracks[0].id.as_str().to_string()
}

fn note_id(session: &Session, index: usize) -> String {
    session.identity().tracks[0].parts[0]
        .notes
        .get(index)
        .expect("note id")
        .id
        .as_str()
        .to_string()
}

fn assert_vocabulary_and_authority(changes: &[project_session::Change]) {
    let mut seen = HashSet::new();
    for change in changes {
        assert!(
            change.is_well_formed(),
            "authority/data rule violated: {change:?}"
        );
        assert!(
            VOCABULARY.contains(&change.kind.as_str()),
            "unknown change kind: {change:?}"
        );
        seen.insert(change.kind.as_str());
    }
}

// ---------------------------------------------------------------------------
// Envelope
// ---------------------------------------------------------------------------

#[test]
fn expected_revision_is_required() {
    let mut session = Session::open(DEMO, None, 1).expect("open");

    let err = session
        .apply_v2(r#"{"command":{"type":"note.set_tone","note_id":"x","tone":60}}"#)
        .expect_err("must require the revision");

    assert_eq!(err.code(), "validation_error");
    assert_eq!(err.details["field"], "expected_revision");
    assert_eq!(session.revision(), 0);
}

#[test]
fn a_stale_expected_revision_is_a_conflict_that_changes_nothing() {
    let mut session = Session::open(DEMO, None, 1).expect("open");
    let id = note_id(&session, 2);
    session
        .apply_v2(&request(
            0,
            json!({"type":"note.set_tone","note_id":id,"tone":67}),
        ))
        .expect("first command");

    let yaml = session.yaml();
    let ids = session.identity().clone();

    let err = session
        .apply_v2(&request(
            0,
            json!({"type":"note.set_tone","note_id":id,"tone":70}),
        ))
        .expect_err("stale revision");

    assert_eq!(err.code(), ERR_REVISION_CONFLICT);
    assert!(err.is_conflict());
    assert_eq!(err.details["current_revision"], 1);
    assert_eq!(session.revision(), 1);
    assert_eq!(session.yaml(), yaml);
    assert_eq!(session.identity(), &ids);
}

#[test]
fn unknown_command_types_and_unimplemented_families_are_distinguished() {
    let mut session = Session::open(DEMO, None, 1).expect("open");

    let unknown = session
        .apply_v2(&request(0, json!({"type":"note.explode"})))
        .expect_err("unknown type");
    assert_eq!(unknown.code(), "validation_error");
    assert_eq!(unknown.details["type"], "note.explode");

    // An unimplemented family is `unsupported_operation`; `curve.apply_stroke`
    // is implemented now, so `part.add` is the current example.
    let unsupported = session
        .apply_v2(&request(0, json!({"type":"part.add"})))
        .expect_err("not implemented yet");
    assert_eq!(unsupported.code(), ERR_UNSUPPORTED);
    assert_eq!(session.revision(), 0);
}

#[test]
fn a_missing_id_is_not_found_and_mutates_nothing() {
    let mut session = Session::open(DEMO, None, 1).expect("open");

    let err = session
        .apply_v2(&request(
            0,
            json!({"type":"note.set_tone","note_id":"not-a-note","tone":60}),
        ))
        .expect_err("unknown id");

    assert_eq!(err.code(), ERR_NOT_FOUND);
    assert_eq!(err.details["kind"], "note");
    assert_eq!(session.revision(), 0);
    assert!(!session.can_undo());
}

// ---------------------------------------------------------------------------
// Notes by stable ID
// ---------------------------------------------------------------------------

#[test]
fn a_note_edit_by_id_reports_a_patch_and_invalidates_render() {
    let mut session = Session::open(DEMO, None, 1).expect("open");
    let id = note_id(&session, 2);

    let result = session
        .apply_v2(&request(
            0,
            json!({"type":"note.set_lyric","note_id":id,"lyric":"ka"}),
        ))
        .expect("set lyric");

    assert_eq!(result.revision, 1);
    assert!(result.changed);
    assert!(result.changes_are_well_formed());
    assert_vocabulary_and_authority(&result.changes);

    let note_change = result
        .changes
        .iter()
        .find(|c| c.kind == "note.changed")
        .expect("note.changed");
    assert_eq!(note_change.authority, AUTHORITY_PATCH);
    assert_eq!(note_change.note_id.as_deref(), Some(id.as_str()));
    assert_eq!(note_change.data.as_ref().unwrap()["lyric"], "ka");
    // The note lives at 1440 with duration 480 in the fixture.
    assert_eq!(note_change.start_tick, Some(1440));
    assert_eq!(note_change.end_tick, Some(1920));
    assert!(result
        .changes
        .iter()
        .any(|c| c.kind == "render.invalidated"));
    assert!(result.changes.iter().any(|c| c.kind == "history.changed"));

    // The document really changed.
    let value = snapshot(&session);
    assert_eq!(
        value["project"]["tracks"][0]["parts"][0]["notes"][2]["lyric"],
        "ka"
    );
}

#[test]
fn ids_stay_addressable_after_the_note_moves() {
    let mut session = Session::open(DEMO, None, 1).expect("open");
    let id = note_id(&session, 1);

    session
        .apply_v2(&request(
            0,
            json!({"type":"note.move","note_id":id,"delta_position_ticks":4800,"delta_tone":2}),
        ))
        .expect("move");

    // The note is now the last one in the document; its id followed it.
    let value = snapshot(&session);
    let notes = value["project"]["tracks"][0]["parts"][0]["notes"]
        .as_array()
        .expect("notes");
    let moved = notes
        .iter()
        .find(|n| n["id"] == id.as_str())
        .expect("moved note keeps its id");
    assert_eq!(moved["position_ticks"], 5760);
    assert_eq!(moved["tone"], 64);

    // ...and it is still addressable by that id.
    session
        .apply_v2(&request(
            1,
            json!({"type":"note.set_lyric","note_id":id,"lyric":"moved"}),
        ))
        .expect("set lyric after move");
    let value = snapshot(&session);
    let notes = value["project"]["tracks"][0]["parts"][0]["notes"]
        .as_array()
        .expect("notes");
    let again = notes.iter().find(|n| n["id"] == id.as_str()).expect("note");
    assert_eq!(again["lyric"], "moved");
}

#[test]
fn resize_reports_the_new_span_and_keeps_the_id() {
    let mut session = Session::open(DEMO, None, 1).expect("open");
    let id = note_id(&session, 0);

    let result = session
        .apply_v2(&request(
            0,
            json!({"type":"note.resize","note_id":id,"delta_duration_ticks":480}),
        ))
        .expect("resize");

    let change = result
        .changes
        .iter()
        .find(|c| c.kind == "note.changed")
        .expect("note.changed");
    assert_eq!(change.start_tick, Some(480));
    assert_eq!(change.end_tick, Some(1440));
    assert_eq!(change.data.as_ref().unwrap()["duration_ticks"], 960);

    let value = snapshot(&session);
    let notes = value["project"]["tracks"][0]["parts"][0]["notes"]
        .as_array()
        .expect("notes");
    assert_eq!(notes[0]["id"], id.as_str());
}

#[test]
fn an_added_note_is_reported_with_an_id_that_can_be_addressed() {
    let mut session = Session::open(DEMO, None, 1).expect("open");
    let part = part_id(&session);

    let result = session
        .apply_v2(&request(
            0,
            json!({"type":"note.add","part_id":part,"tone":72,"position_ticks":2400,
                   "duration_ticks":480,"lyric":"new"}),
        ))
        .expect("add note");

    assert!(result.changes_are_well_formed());
    assert_vocabulary_and_authority(&result.changes);
    let added = result
        .changes
        .iter()
        .find(|c| c.kind == "note.added")
        .expect("note.added");
    assert_eq!(added.authority, AUTHORITY_PATCH);
    let new_id = added.note_id.clone().expect("new note id");
    assert_eq!(added.data.as_ref().unwrap()["lyric"], "new");

    let value = snapshot(&session);
    let notes = value["project"]["tracks"][0]["parts"][0]["notes"]
        .as_array()
        .expect("notes");
    assert_eq!(notes.len(), 5);
    assert!(notes.iter().any(|n| n["id"] == new_id.as_str()));

    session
        .apply_v2(&request(
            1,
            json!({"type":"note.set_tone","note_id":new_id,"tone":74}),
        ))
        .expect("the reported id must be addressable");
}

#[test]
fn removing_a_note_reports_removal_without_a_payload() {
    let mut session = Session::open(DEMO, None, 1).expect("open");
    let id = note_id(&session, 2);

    let result = session
        .apply_v2(&request(0, json!({"type":"note.remove","note_id":id})))
        .expect("remove");

    let removed = result
        .changes
        .iter()
        .find(|c| c.kind == "note.removed")
        .expect("note.removed");
    assert_eq!(removed.note_id.as_deref(), Some(id.as_str()));
    assert!(removed.data.is_none(), "removal must not carry a payload");
    assert!(result.changes_are_well_formed());
}

#[test]
fn a_bad_field_is_a_validation_error_with_the_field_name() {
    let mut session = Session::open(DEMO, None, 1).expect("open");
    let id = note_id(&session, 0);

    let err = session
        .apply_v2(&request(
            0,
            json!({"type":"note.add","part_id":part_id(&session),
                                    "tone":60,"position_ticks":0,"duration_ticks":0}),
        ))
        .expect_err("zero duration");
    assert_eq!(err.details["field"], "duration_ticks");

    let err = session
        .apply_v2(&request(0, json!({"type":"note.resize","note_id":id})))
        .expect_err("missing delta");
    assert_eq!(err.details["field"], "delta_duration_ticks");

    assert_eq!(session.revision(), 0);
}

// ---------------------------------------------------------------------------
// Tracks by stable ID
// ---------------------------------------------------------------------------

#[test]
fn track_edits_use_track_ids_and_patch_metadata() {
    let mut session = Session::open(DEMO, None, 1).expect("open");
    let id = track_id(&session);

    let result = session
        .apply_v2(&request(
            0,
            json!({"type":"track.set_volume_db","track_id":id,"volume":-6.5}),
        ))
        .expect("set volume");

    let change = result
        .changes
        .iter()
        .find(|c| c.kind == "track.changed")
        .expect("track.changed");
    assert_eq!(change.authority, AUTHORITY_PATCH);
    assert_eq!(change.track_id.as_deref(), Some(id.as_str()));
    assert_eq!(change.data.as_ref().unwrap()["volume_db"], -6.5);

    let err = session
        .apply_v2(&request(
            1,
            json!({"type":"track.set_mute","track_id":"nope","mute":true}),
        ))
        .expect_err("unknown track");
    assert_eq!(err.code(), ERR_NOT_FOUND);
}

// ---------------------------------------------------------------------------
// Result shape
// ---------------------------------------------------------------------------

#[test]
fn the_snapshot_is_only_included_on_request() {
    let mut session = Session::open(DEMO, None, 1).expect("open");
    let id = note_id(&session, 0);

    let without = session
        .apply_v2(&request(
            0,
            json!({"type":"note.set_tone","note_id":id,"tone":61}),
        ))
        .expect("command");
    assert!(without.snapshot.is_none(), "snapshot must be opt-in");

    let with = session
        .apply_v2(
            json!({"expected_revision":1,"include_snapshot":true,
                   "command":{"type":"note.set_tone","note_id":id,"tone":62}})
            .to_string()
            .as_str(),
        )
        .expect("command");
    let snapshot = with.snapshot.expect("snapshot requested");
    assert_eq!(snapshot["revision"], 2);
    assert_eq!(
        snapshot["project"]["tracks"][0]["parts"][0]["notes"][0]["tone"],
        62
    );
}

#[test]
fn history_changed_is_reported_only_when_the_capabilities_change() {
    let mut session = Session::open(DEMO, None, 1).expect("open");
    let id = note_id(&session, 0);

    let first = session
        .apply_v2(&request(
            0,
            json!({"type":"note.set_tone","note_id":id,"tone":61}),
        ))
        .expect("first");
    assert!(first.changes.iter().any(|c| c.kind == "history.changed"));

    let second = session
        .apply_v2(&request(
            1,
            json!({"type":"note.set_tone","note_id":id,"tone":62}),
        ))
        .expect("second");
    assert!(
        !second.changes.iter().any(|c| c.kind == "history.changed"),
        "can_undo/can_redo did not change: {:?}",
        second.changes
    );
}

// ---------------------------------------------------------------------------
// Bounded reads
// ---------------------------------------------------------------------------

#[test]
fn bounded_reads_carry_the_authoritative_revision() {
    let session = Session::open(CURVE_PROJECT, None, 4).expect("open");
    let part = part_id(&session);
    let curve = session
        .curve_id(0, 0, 0)
        .expect("curve id")
        .as_str()
        .to_string();

    let notes: Value = serde_json::from_str(&session.part_notes_json(&part).unwrap()).unwrap();
    assert_eq!(notes["revision"], 0);
    assert_eq!(notes["part_id"], part.as_str());
    assert_eq!(notes["notes"][0]["id"], note_id(&session, 0).as_str());
    assert_eq!(notes["notes"][0]["tone"], 60);

    let curve_value: Value =
        serde_json::from_str(&session.curve_json(&part, &curve).unwrap()).unwrap();
    assert_eq!(curve_value["revision"], 0);
    assert_eq!(curve_value["abbr"], "dyn");
    assert_eq!(curve_value["points"][1]["value"], 10);

    let tracks: Value = serde_json::from_str(&session.tracks_json().unwrap()).unwrap();
    assert_eq!(tracks["revision"], 0);
    assert_eq!(tracks["tracks"][0]["id"], track_id(&session).as_str());
    assert_eq!(tracks["tracks"][0]["name"], "Lead");
}

#[test]
fn bounded_reads_reject_unknown_or_mismatched_ids() {
    let session = Session::open(CURVE_PROJECT, None, 4).expect("open");
    let part = part_id(&session);
    let curve = session
        .curve_id(0, 0, 0)
        .expect("curve id")
        .as_str()
        .to_string();

    assert_eq!(
        session
            .part_notes_json("nope")
            .expect_err("unknown part")
            .code(),
        ERR_NOT_FOUND
    );
    assert_eq!(
        session
            .curve_json(&part, "nope")
            .expect_err("unknown curve")
            .code(),
        ERR_NOT_FOUND
    );
    assert_eq!(
        session
            .curve_json("some-other-part", &curve)
            .expect_err("part mismatch")
            .code(),
        ERR_NOT_FOUND
    );
}

// ---------------------------------------------------------------------------
// Lazy snapshot
// ---------------------------------------------------------------------------

#[test]
fn a_command_defers_snapshot_serialization() {
    let mut doc = ProjectDoc::from_yaml(DEMO).expect("open");
    assert!(
        !doc.snapshot_is_stale(),
        "a fresh document has a valid snapshot"
    );

    doc.apply(r#"{"cmd":"change_tone","part":0,"note":2,"tone":67}"#)
        .expect("command");
    assert!(
        doc.snapshot_is_stale(),
        "serializing on every command is what this defers"
    );

    let yaml = doc.snapshot().to_string();
    assert!(
        !doc.snapshot_is_stale(),
        "reading the snapshot refreshes it"
    );
    assert!(
        yaml.contains("tone: 67"),
        "the refreshed snapshot must be current"
    );
    assert_eq!(doc.snapshot(), yaml);

    // A rejected command leaves the cached snapshot valid and unchanged.
    doc.apply(r#"{"cmd":"nope"}"#).expect_err("rejected");
    assert!(!doc.snapshot_is_stale());
    assert_eq!(doc.snapshot(), yaml);
}
