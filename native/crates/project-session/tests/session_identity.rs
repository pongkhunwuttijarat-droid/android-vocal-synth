//! Session-level identity and revision semantics (CONTRACT-v1 §6.2, §6.9).
//!
//! These tests pin the parts of the session contract that host adapters rely
//! on: revision only moves for committed changes, IDs survive undo/redo and
//! save/reopen, identity never leaks into `.ustx`, and group transactions are
//! refused until their isolated semantics exist.

use project_session::identity::IdentityWarning;
use project_session::Session;

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

fn snapshot(session: &Session) -> serde_json::Value {
    serde_json::from_str(&session.snapshot_json().expect("snapshot")).expect("snapshot json")
}

fn note_ids(session: &Session) -> Vec<String> {
    session.identity().tracks[0].parts[0]
        .notes
        .iter()
        .map(|n| n.id.as_str().to_string())
        .collect()
}

#[test]
fn a_new_session_starts_at_revision_zero_with_stable_ids() {
    let session = Session::new(1);

    assert_eq!(session.revision(), 0);
    assert!(session.ids_stable());
    assert!(!session.can_undo());
    assert!(!session.can_redo());
    assert!(
        session.warnings().is_empty(),
        "a brand-new project reports no warnings, got {:?}",
        session.warnings()
    );

    let value = snapshot(&session);
    assert_eq!(value["revision"], 0);
    assert_eq!(value["ids_stable"], true);
    assert_eq!(value["can_undo"], false);
    assert_eq!(
        value["project"]["id"],
        session.identity().project_id.as_str()
    );
    assert!(!session.session_id().is_empty());
}

#[test]
fn a_committed_command_bumps_the_revision_once_and_keeps_ids() {
    let mut session = Session::open(DEMO, None, 1).expect("open");
    let before = note_ids(&session);

    let outcome = session
        .apply_command(r#"{"cmd":"change_tone","part":0,"note":2,"tone":67}"#)
        .expect("change_tone");

    assert_eq!(outcome.revision, 1);
    assert!(outcome.changed);
    assert!(outcome.can_undo);
    assert_eq!(session.revision(), 1);
    assert_eq!(
        note_ids(&session),
        before,
        "an in-place edit moved identity"
    );

    let value = snapshot(&session);
    assert_eq!(value["revision"], 1);
    assert_eq!(
        value["project"]["tracks"][0]["parts"][0]["notes"][2]["tone"],
        67
    );
}

#[test]
fn a_rejected_command_changes_nothing_at_all() {
    let mut session = Session::open(DEMO, None, 1).expect("open");
    session
        .apply_command(r#"{"cmd":"change_tone","part":0,"note":2,"tone":67}"#)
        .unwrap();
    let yaml = session.yaml();
    let ids = note_ids(&session);
    let revision = session.revision();
    let undo_len = session.identity().len();

    let err = session
        .apply_command(r#"{"cmd":"nope"}"#)
        .expect_err("must reject");

    assert!(err.contains("unknown command"), "unexpected error: {err}");
    assert_eq!(session.revision(), revision);
    assert_eq!(session.yaml(), yaml);
    assert_eq!(note_ids(&session), ids);
    assert_eq!(session.identity().len(), undo_len);
    assert!(
        session.can_undo(),
        "history must survive a rejected command"
    );
}

#[test]
fn ids_survive_undo_and_redo() {
    let mut session = Session::open(DEMO, None, 1).expect("open");
    let baseline = note_ids(&session);

    session
        .apply_command(r#"{"cmd":"change_tone","part":0,"note":1,"tone":70}"#)
        .unwrap();
    assert_eq!(note_ids(&session), baseline);

    let undone = session.undo().expect("undo").expect("something to undo");
    assert_eq!(undone.revision, 2, "undo advances the revision (§6.9)");
    assert_eq!(note_ids(&session), baseline);

    let redone = session.redo().expect("redo").expect("something to redo");
    assert_eq!(redone.revision, 3);
    assert_eq!(note_ids(&session), baseline);
}

#[test]
fn a_noop_undo_does_not_move_the_revision() {
    let mut session = Session::new(1);

    assert!(session.undo().expect("undo").is_none());
    assert!(session.redo().expect("redo").is_none());
    assert_eq!(session.revision(), 0);
}

#[test]
fn opening_without_a_sidecar_regenerates_ids_and_says_so() {
    let session = Session::open(DEMO, None, 1).expect("open");

    assert!(!session.ids_stable());
    assert!(
        session
            .warnings()
            .contains(&IdentityWarning::IdsRegenerated),
        "expected ids_regenerated, got {:?}",
        session.warnings()
    );
    assert_eq!(session.revision(), 0);
}

#[test]
fn opening_with_a_sidecar_adopts_the_ids() {
    let mut source = Session::open(DEMO, None, 7).expect("open");
    let sidecar = source.save_sidecar().expect("sidecar");

    let reopened = Session::open(DEMO, Some(&sidecar), 99).expect("reopen");

    assert!(reopened.ids_stable());
    assert!(reopened.warnings().is_empty(), "{:?}", reopened.warnings());
    assert_eq!(
        reopened.identity().project_id,
        source.identity().project_id,
        "project id must come from the sidecar"
    );
    assert_eq!(note_ids(&reopened), note_ids(&source));
    assert_eq!(reopened.identity(), source.identity());
}

#[test]
fn a_newly_created_note_keeps_its_id_across_save_and_reopen() {
    let mut session = Session::open(DEMO, None, 3).expect("open");
    session
        .apply_command(
            r#"{"cmd":"add_note","part":0,"tone":72,"position":2400,"duration":480,"lyric":"new"}"#,
        )
        .expect("add_note");
    let created = note_ids(&session).pop().expect("new note id");

    let yaml = session.yaml();
    let sidecar = session.save_sidecar().expect("sidecar");
    let reopened = Session::open(&yaml, Some(&sidecar), 5).expect("reopen");

    let ids = note_ids(&reopened);
    assert_eq!(ids.len(), 5);
    assert_eq!(ids[4], created, "a persisted id must survive reopen");
    assert_eq!(
        &ids[..4],
        &note_ids(&session)[..4],
        "existing notes must keep their ids"
    );
}

#[test]
fn copied_note_gets_a_fresh_id_that_also_persists() {
    // Duplicate by hand (no copy/paste command yet): the new entity must be
    // distinguishable from the original it was cloned from.
    let mut session = Session::open(DEMO, None, 3).expect("open");
    session
        .apply_command(
            r#"{"cmd":"add_note","part":0,"tone":60,"position":480,"duration":480,"lyric":"see[s i]"}"#,
        )
        .expect("add_note");
    let ids = note_ids(&session);
    assert_eq!(ids.len(), 5);
    assert_ne!(ids[0], ids[4], "a duplicate must not reuse the id");

    let yaml = session.yaml();
    let sidecar = session.save_sidecar().expect("sidecar");
    let reopened = Session::open(&yaml, Some(&sidecar), 5).expect("reopen");
    assert_eq!(note_ids(&reopened).len(), 5);
    assert!(note_ids(&reopened).contains(&ids[4]));
}

#[test]
fn identity_never_appears_in_the_ustx() {
    let mut session = Session::open(CURVE_PROJECT, None, 4).expect("open");
    let curve_id = session
        .curve_id(0, 0, 0)
        .expect("curve id")
        .as_str()
        .to_string();

    let yaml = session.yaml();

    assert!(
        !yaml.contains("lilt"),
        "sidecar hint leaked into the project"
    );
    assert!(!yaml.contains(session.identity().project_id.as_str()));
    assert!(!yaml.contains(&curve_id));
    assert!(!yaml.contains(session.session_id()));
    assert!(
        yaml.contains("abbr: dyn"),
        "the curve itself must still be written"
    );

    // ...and it stays true after a mutation + resave.
    session
        .apply_command(r#"{"cmd":"change_tone","part":0,"note":0,"tone":61}"#)
        .unwrap();
    let sidecar = session.save_sidecar().expect("sidecar");
    assert!(sidecar.contains(&curve_id), "the sidecar is where ids live");
    assert!(!session.yaml().contains(&curve_id));
}

#[test]
fn snapshot_reports_the_ids_the_table_holds() {
    let session = Session::open(CURVE_PROJECT, None, 4).expect("open");
    let value = snapshot(&session);
    let table = session.identity();

    let part = &value["project"]["tracks"][0]["parts"][0];
    assert_eq!(part["id"], table.tracks[0].parts[0].id.as_str());
    assert_eq!(
        part["notes"][0]["id"],
        table.tracks[0].parts[0].notes[0].id.as_str()
    );
    assert_eq!(
        part["curves"][0]["id"],
        table.tracks[0].parts[0].curves[0].id.as_str()
    );
    assert_eq!(part["curves"][0]["abbr"], "dyn");
    assert_eq!(part["curves"][0]["points"][1]["tick"], 480);
    assert_eq!(part["curves"][0]["points"][1]["value"], 10);
    assert_eq!(
        value["project"]["tracks"][0]["id"],
        table.tracks[0].id.as_str()
    );
}

#[test]
fn curve_ids_survive_an_edit_and_the_snapshot_updates() {
    let mut session = Session::open(CURVE_PROJECT, None, 4).expect("open");
    let before = session.curve_id(0, 0, 0).expect("curve id").clone();

    // No curve command exists yet, so exercise the re-reconcile path through a
    // note edit and check the curve identity is untouched.
    session
        .apply_command(r#"{"cmd":"change_lyric","part":0,"note":0,"lyric":"ka"}"#)
        .unwrap();

    assert_eq!(session.curve_id(0, 0, 0), Some(&before));
    let value = snapshot(&session);
    assert_eq!(
        value["project"]["tracks"][0]["parts"][0]["curves"][0]["id"],
        before.as_str()
    );
}

#[test]
fn group_commands_are_refused_until_the_transaction_semantics_exist() {
    let mut session = Session::open(DEMO, None, 1).expect("open");
    let yaml = session.yaml();

    let err = session
        .apply_command(r#"{"cmd":"group_begin","name":"Move notes"}"#)
        .expect_err("groups must be refused");

    assert!(
        err.contains("unsupported_operation"),
        "unexpected error: {err}"
    );
    assert_eq!(session.revision(), 0);
    assert_eq!(session.yaml(), yaml);
    assert!(!session.can_undo());
}

#[test]
fn saving_the_sidecar_does_not_move_the_revision() {
    let mut session = Session::open(DEMO, None, 1).expect("open");
    session
        .apply_command(r#"{"cmd":"change_tone","part":0,"note":2,"tone":67}"#)
        .unwrap();
    let revision = session.revision();
    let yaml = session.yaml();

    let sidecar = session.save_sidecar().expect("save");

    assert!(
        sidecar.contains("document_fingerprint"),
        "a save records the canonical document hash"
    );
    assert_eq!(
        session.revision(),
        revision,
        "save is not a document change"
    );
    assert_eq!(session.yaml(), yaml);
}

#[test]
fn curve_repair_warnings_reach_the_session() {
    let damaged = CURVE_PROJECT.replace("xs: [0, 480]", "xs: [0, 480, 960]");
    assert!(damaged.contains("xs: [0, 480, 960]"));

    let session = Session::open(&damaged, None, 4).expect("open must not fail");

    assert!(
        session
            .warnings()
            .contains(&IdentityWarning::RepairedCurveArrays),
        "expected repaired_curve_arrays, got {:?}",
        session.warnings()
    );
    // The repair is visible in the snapshot: only the paired prefix survives.
    let value = snapshot(&session);
    let points = value["project"]["tracks"][0]["parts"][0]["curves"][0]["points"]
        .as_array()
        .expect("points");
    assert_eq!(points.len(), 2);
}

#[test]
fn a_project_without_tracks_is_repaired_and_reported() {
    // Drop the tracks block: the session must still open with a usable track
    // and report the repair code the frontend branches on.
    let mut yaml = String::new();
    let mut skipping = false;
    for line in DEMO.lines() {
        if line.starts_with("tracks:") {
            skipping = true;
            continue;
        }
        if skipping && (line.starts_with("- ") || line.starts_with("  ")) {
            continue;
        }
        skipping = false;
        yaml.push_str(line);
        yaml.push('\n');
    }
    assert!(!yaml.contains("tracks:"));

    let session = Session::open(&yaml, None, 1).expect("open must not fail");

    assert!(
        session
            .warnings()
            .iter()
            .any(|w| w.code() == "missing_tracks_repaired"),
        "expected missing_tracks_repaired, got {:?}",
        session.warnings()
    );
    let value = snapshot(&session);
    assert_eq!(
        value["project"]["tracks"][0]["name"], "Track1",
        "the repaired track must be visible in the snapshot"
    );
    // ...and the session has a stable id for it.
    assert!(session.identity().track_id(0).is_some());
}

#[test]
fn reopening_with_a_corrupt_sidecar_still_opens() {
    let session = Session::open(DEMO, Some("{ not json"), 1).expect("open must not fail");

    assert!(!session.ids_stable());
    assert!(session
        .warnings()
        .contains(&IdentityWarning::IdsRegenerated));
    assert_eq!(note_ids(&session).len(), 4);
}
