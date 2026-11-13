//! A real MULTI-TRACK OpenUtau project through the session.
//!
//! The clone's fixtures are single-track, so most session tests never exercise
//! more than one. `test-ustx-example/king.ustx` is a genuine 5-track project,
//! and it is the case that was completely unopenable before the UTF-8 BOM fix
//! (see `domain/tests/ustx_load.rs`).
//!
//! What this pins:
//!
//! * a real OpenUtau file opens at all,
//! * every track gets its own stable ID and they are all distinct,
//! * each track owns the part it really owns,
//! * a track edit by stable ID touches that track and no other,
//! * the snapshot survives a save → reopen round trip.

use project_session::Session;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// The real 5-track project, or `None` when the gitignored fixture is absent.
fn real_multitrack_yaml() -> Option<String> {
    let path = repo_root().join("test-ustx-example/king.ustx");
    if !path.is_file() {
        eprintln!("skipping: test-ustx-example/king.ustx not present (gitignored)");
        return None;
    }
    Some(std::fs::read_to_string(&path).expect("read"))
}

fn open_real() -> Option<Session> {
    let yaml = real_multitrack_yaml()?;
    Some(Session::open(&yaml, None, 1).expect("open the real multitrack project"))
}

fn snapshot(session: &Session) -> Value {
    serde_json::from_str(&session.snapshot_json().expect("snapshot")).expect("snapshot json")
}

fn tracks_of(session: &Session) -> Vec<Value> {
    snapshot(session)["project"]["tracks"]
        .as_array()
        .expect("project.tracks must be an array")
        .clone()
}

#[test]
fn a_real_five_track_project_opens_through_the_session() {
    let Some(session) = open_real() else { return };
    let tracks = tracks_of(&session);
    assert_eq!(
        tracks.len(),
        5,
        "the fixture is a 5-track project; the snapshot must carry all five"
    );
}

#[test]
fn every_track_gets_a_distinct_stable_id() {
    let Some(session) = open_real() else { return };
    let ids: Vec<String> = tracks_of(&session)
        .iter()
        .map(|t| t["id"].as_str().expect("track id").to_string())
        .collect();
    assert_eq!(ids.len(), 5, "expected five tracks, got {ids:?}");
    let mut unique = ids.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        unique.len(),
        5,
        "five tracks must have five distinct stable IDs, got {ids:?}"
    );
}

#[test]
fn each_track_owns_exactly_its_own_part() {
    // The failure this guards against is subtle: a session that resolved parts
    // against the wrong track index would still return *a* valid part — it would
    // just be another track's notes.
    let Some(session) = open_real() else { return };
    for (index, track) in tracks_of(&session).iter().enumerate() {
        let parts = track["parts"]
            .as_array()
            .unwrap_or_else(|| panic!("track {index} must carry a parts array"));
        assert_eq!(
            parts.len(),
            1,
            "each track in this fixture owns exactly one part; got {} for track {index}",
            parts.len()
        );
        let part_id = parts[0]["id"].as_str().expect("part id");
        assert_eq!(
            session.identity().tracks[index].parts[0].id.as_str(),
            part_id,
            "the snapshot's part id must match the identity table for track {index}"
        );
    }
}

#[test]
fn editing_one_track_leaves_the_others_untouched() {
    let Some(mut session) = open_real() else {
        return;
    };
    let before = tracks_of(&session);

    let target = before[2]["id"]
        .as_str()
        .expect("third track id")
        .to_string();
    let revision = session.revision();
    session
        .apply_v2(
            &json!({
                "expected_revision": revision,
                "command": {"type": "track.set_mute", "track_id": target, "mute": true},
            })
            .to_string(),
        )
        .expect("mute the third track");

    let after = tracks_of(&session);
    assert_eq!(before.len(), after.len(), "track count must not change");

    let changed: Vec<usize> = (0..after.len())
        .filter(|i| before[*i] != after[*i])
        .collect();
    assert_eq!(
        changed,
        vec![2],
        "exactly the edited track must change; changed = {changed:?}"
    );
    assert_eq!(
        after[2]["mute"],
        json!(true),
        "the edited track must carry the new value"
    );
}

#[test]
fn a_multitrack_project_survives_a_save_and_reopen() {
    let Some(session) = open_real() else { return };
    let saved = session.yaml();
    let reopened = Session::open(&saved, None, 1).expect("reopen what we saved");

    assert_eq!(
        tracks_of(&session).len(),
        tracks_of(&reopened).len(),
        "a save/reopen cycle must not lose or merge tracks"
    );
    let before_ids: Vec<String> = tracks_of(&session)
        .iter()
        .map(|t| t["id"].as_str().unwrap().to_string())
        .collect();
    let after_ids: Vec<String> = tracks_of(&reopened)
        .iter()
        .map(|t| t["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        before_ids, after_ids,
        "a reopen without a sidecar must mint the same deterministic IDs"
    );
}
