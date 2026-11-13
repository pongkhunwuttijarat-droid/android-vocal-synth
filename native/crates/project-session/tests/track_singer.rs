//! `track.set_singer` — the per-track singer control (CONTRACT-v1 T014).
//!
//! The *read* side was always in the frozen §6 snapshot (`tracks[].singer_id`)
//! and imports correctly, but nothing could write it, so the editor had no
//! singer picker to bind to. These cover the write path end to end: set, clear,
//! undo/redo round-trip, normalisation, per-track independence, and that a
//! rejected command changes nothing.

use project_session::v2::{MutationResult, ERR_NOT_FOUND};
use project_session::Session;
use serde_json::{json, Value};

const DEMO: &str = include_str!("../../../tools/synth-cli/tests/data/demo-song.ustx");

/// Two tracks, so the per-track independence case has something to separate.
/// `DEMO` carries a single track.
const TWO_TRACKS: &str = r#"ustx_version: 0.9
name: Two Tracks
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
tracks:
- track_name: Lead
- track_name: Harmony
voice_parts:
- name: LeadPart
  track_no: 0
  position: 0
  duration: 480
  notes:
  - position: 0
    duration: 480
    tone: 60
    lyric: la
"#;

fn request(revision: u64, command: Value) -> String {
    json!({ "expected_revision": revision, "command": command }).to_string()
}

fn snapshot(session: &Session) -> Value {
    serde_json::from_str(&session.snapshot_json().expect("snapshot")).expect("json")
}

fn track_id(session: &Session, index: usize) -> String {
    session.identity().tracks[index].id.as_str().to_string()
}

/// `singer_id` of every track, in document order.
fn singers(session: &Session) -> Vec<Value> {
    snapshot(session)["project"]["tracks"]
        .as_array()
        .expect("tracks")
        .iter()
        .map(|track| track["singer_id"].clone())
        .collect()
}

/// Apply a singer change, returning the envelope so a test can read `changes`.
fn set_singer(session: &mut Session, index: usize, singer: Value) -> MutationResult {
    let revision = session.revision();
    let id = track_id(session, index);
    session
        .apply_v2(&request(
            revision,
            json!({"type": "track.set_singer", "track_id": id, "singer_id": singer}),
        ))
        .unwrap_or_else(|error| panic!("track.set_singer failed: {error:?}"))
}

#[test]
fn a_track_singer_is_set_and_the_change_names_the_track() {
    let mut session = Session::open(DEMO, None, 1).expect("open");
    let id = track_id(&session, 0);

    let result = set_singer(&mut session, 0, json!("重音テト"));

    assert_eq!(session.revision(), 1);
    assert_eq!(singers(&session)[0], json!("重音テト"));
    assert_eq!(result.command_name, "Track singer");
    assert!(
        result
            .changes
            .iter()
            .any(|change| change.kind == "track.changed"
                && change.track_id.as_deref() == Some(id.as_str())),
        "expected a track.changed change for {id}, got {:?}",
        result.changes
    );
}

#[test]
fn the_change_undoes_and_redoes_as_one_transaction() {
    let mut session = Session::open(DEMO, None, 1).expect("open");
    let before = singers(&session)[0].clone();

    set_singer(&mut session, 0, json!("Someone"));
    assert_ne!(singers(&session)[0], before);

    session.undo_v2(1).expect("undo");
    // Undo restores the previous value; it must not leave a stale singer behind.
    assert_eq!(singers(&session)[0], before);

    // Undo is itself a revision, so redo expects the current one, not the old.
    session.redo_v2(2).expect("redo");
    assert_eq!(singers(&session)[0], json!("Someone"));
}

#[test]
fn null_clears_the_singer_which_is_the_audio_track_state() {
    let mut session = Session::open(DEMO, None, 1).expect("open");

    set_singer(&mut session, 0, json!("Someone"));
    assert_eq!(singers(&session)[0], json!("Someone"));

    set_singer(&mut session, 0, Value::Null);
    assert_eq!(singers(&session)[0], Value::Null);
}

#[test]
fn an_empty_or_whitespace_singer_normalises_to_no_singer() {
    let mut session = Session::open(DEMO, None, 1).expect("open");

    // Otherwise the document would carry a blank singer that the host renders as
    // a named-but-empty row, which is not the same thing as having no singer.
    set_singer(&mut session, 0, json!("   "));
    assert_eq!(singers(&session)[0], Value::Null);
}

#[test]
fn two_tracks_hold_independent_singers() {
    let mut session = Session::open(TWO_TRACKS, None, 1).expect("open");

    set_singer(&mut session, 0, json!("Lead bank"));
    set_singer(&mut session, 1, json!("Harmony bank"));

    let all = singers(&session);
    assert_eq!(all[0], json!("Lead bank"));
    assert_eq!(all[1], json!("Harmony bank"));
}

#[test]
fn a_rejected_singer_change_leaves_the_document_untouched() {
    let mut session = Session::open(DEMO, None, 1).expect("open");
    let before_yaml = session.yaml();
    let before_singers = singers(&session);
    let before_ids = session.identity().clone();

    // Wrong field type: neither a string nor null.
    let err = session
        .apply_v2(&request(
            0,
            json!({"type": "track.set_singer", "track_id": track_id(&session, 0), "singer_id": 7}),
        ))
        .expect_err("a number is not a singer");
    assert_eq!(err.code(), "validation_error");

    // Unknown track id.
    let err = session
        .apply_v2(&request(
            0,
            json!({"type": "track.set_singer", "track_id": "no-such-track", "singer_id": "x"}),
        ))
        .expect_err("unknown track");
    assert_eq!(err.code(), ERR_NOT_FOUND);

    assert_eq!(session.revision(), 0, "no command committed");
    assert_eq!(session.yaml(), before_yaml);
    assert_eq!(singers(&session), before_singers);
    assert_eq!(session.identity(), &before_ids);
}

#[test]
fn the_singer_survives_a_yaml_round_trip() {
    let mut session = Session::open(DEMO, None, 1).expect("open");
    set_singer(&mut session, 0, json!("Round Trip Bank"));

    let yaml = session.yaml();
    let reopened = Session::open(&yaml, None, 1).expect("reopen");
    assert_eq!(singers(&reopened)[0], json!("Round Trip Bank"));
}
