//! Group transactions (§6.8) and their revision rules (§6.9).
//!
//! A group is an isolated tentative transaction: the committed document,
//! revision and undo history are untouched until commit, a failed child closes
//! the group with nothing applied, and cancel restores the pre-group state
//! exactly.

use project_session::v2::{
    SessionError, ERR_GROUP_FAILED, ERR_INVALID_GROUP_STATE, ERR_NOT_FOUND, ERR_SESSION_BUSY,
};
use project_session::Session;
use serde_json::{json, Value};

const DEMO: &str = include_str!("../../../tools/synth-cli/tests/data/demo-song.ustx");

fn snapshot(session: &Session) -> Value {
    serde_json::from_str(&session.snapshot_json().expect("snapshot")).expect("json")
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

fn child(revision: u64, group_id: &str, command: Value) -> String {
    json!({"expected_revision": revision, "group_id": group_id, "command": command}).to_string()
}

fn open() -> Session {
    Session::open(DEMO, None, 1).expect("open")
}

#[test]
fn beginning_a_group_changes_nothing_observable() {
    let mut session = open();
    let yaml = session.yaml();
    let ids = session.identity().clone();

    let begin = session.group_begin(0, "Move notes").expect("begin");

    assert_eq!(begin.base_revision, 0);
    assert!(!begin.group_id.is_empty());
    assert!(session.group_is_open());
    assert_eq!(session.revision(), 0);
    assert_eq!(session.yaml(), yaml);
    assert_eq!(session.identity(), &ids);
    assert!(!session.can_undo(), "a group is not an undo step yet");
}

#[test]
fn children_edit_the_tentative_copy_only() {
    let mut session = open();
    let id = note_id(&session, 2);
    let begin = session.group_begin(0, "Edit").expect("begin");
    let yaml = session.yaml();

    let result = session
        .apply_v2(&child(
            0,
            &begin.group_id,
            json!({"type":"note.set_lyric","note_id":id,"lyric":"tentative"}),
        ))
        .expect("child");

    assert_eq!(result.group_id.as_deref(), Some(begin.group_id.as_str()));
    assert_eq!(result.revision, 0, "children never bump the revision");
    assert!(
        result.changes.is_empty(),
        "tentative state is not public: {:?}",
        result.changes
    );
    // Committed reads and the document are untouched.
    assert_eq!(session.yaml(), yaml);
    let value = snapshot(&session);
    assert_eq!(
        value["project"]["tracks"][0]["parts"][0]["notes"][2]["lyric"],
        "ah[3]"
    );
    assert!(!session.can_undo());

    // ...and the tentative work is not lost: commit publishes it.
    let commit = session.group_commit(&begin.group_id, 0).expect("commit");
    assert_eq!(commit.revision, 1);
    let value = snapshot(&session);
    assert_eq!(
        value["project"]["tracks"][0]["parts"][0]["notes"][2]["lyric"],
        "tentative"
    );
}

#[test]
fn commit_is_one_revision_and_one_undo_step() {
    let mut session = open();
    let id = note_id(&session, 1);
    let begin = session.group_begin(0, "Drag notes").expect("begin");
    for (revision, tone) in [(0u64, 70), (0, 71), (0, 72)] {
        session
            .apply_v2(&child(
                revision,
                &begin.group_id,
                json!({"type":"note.set_tone","note_id":id,"tone":tone}),
            ))
            .expect("child");
    }

    let commit = session.group_commit(&begin.group_id, 0).expect("commit");

    assert_eq!(commit.revision, 1, "one increment for the whole group");
    assert_eq!(commit.command_name, "Drag notes");
    assert!(commit.changed);
    assert!(commit.can_undo);
    // Merged changes report each entity once, with a patch payload.
    let note_changes: Vec<_> = commit
        .changes
        .iter()
        .filter(|c| c.kind == "note.changed")
        .collect();
    assert_eq!(
        note_changes.len(),
        1,
        "one entry per entity: {:?}",
        commit.changes
    );
    assert_eq!(note_changes[0].data.as_ref().unwrap()["tone"], 72);
    assert!(commit.changes_are_well_formed());

    // A single undo returns to the pre-group state.
    let undone = session.undo().expect("undo").expect("step");
    assert_eq!(undone.revision, 2);
    let value = snapshot(&session);
    assert_eq!(
        value["project"]["tracks"][0]["parts"][0]["notes"][1]["tone"],
        62
    );
}

#[test]
fn ids_stay_stable_inside_and_after_a_group() {
    let mut session = open();
    let id = note_id(&session, 2);
    let part = session.identity().tracks[0].parts[0]
        .id
        .as_str()
        .to_string();
    let begin = session.group_begin(0, "Add and edit").expect("begin");

    session
        .apply_v2(&child(
            0,
            &begin.group_id,
            json!({"type":"note.set_lyric","note_id":id,"lyric":"x"}),
        ))
        .expect("edit existing");
    session
        .apply_v2(&child(
            0,
            &begin.group_id,
            json!({"type":"note.add","part_id":part,"tone":72,"position_ticks":2400,
                   "duration_ticks":480,"lyric":"new"}),
        ))
        .expect("add");

    let commit = session.group_commit(&begin.group_id, 0).expect("commit");

    // The existing note kept its id, the new one is reported and addressable.
    assert_eq!(note_id(&session, 2), id);
    let added = commit
        .changes
        .iter()
        .find(|c| c.kind == "note.added")
        .expect("note.added in the merged changes");
    let new_id = added.note_id.clone().expect("new id");
    session
        .apply_v2(
            json!({"expected_revision":1,"command":{"type":"note.set_tone","note_id":new_id,"tone":74}})
                .to_string()
                .as_str(),
        )
        .expect("the committed id must be addressable");
}

#[test]
fn a_cancelled_group_restores_everything_exactly() {
    let mut session = open();
    let id = note_id(&session, 0);
    let yaml = session.yaml();
    let ids = session.identity().clone();

    let begin = session.group_begin(0, "Throwaway").expect("begin");
    session
        .apply_v2(&child(
            0,
            &begin.group_id,
            json!({"type":"note.set_tone","note_id":id,"tone":75}),
        ))
        .expect("child");
    session.group_cancel(&begin.group_id).expect("cancel");

    assert!(!session.group_is_open());
    assert_eq!(session.revision(), 0);
    assert_eq!(session.yaml(), yaml);
    assert_eq!(session.identity(), &ids);
    assert!(!session.can_undo(), "cancel leaves no undo step");
    // The second cancel is not idempotent by contract.
    assert_eq!(
        session
            .group_cancel(&begin.group_id)
            .expect_err("closed")
            .code(),
        ERR_NOT_FOUND
    );
}

#[test]
fn a_failed_child_closes_the_group_and_discards_everything() {
    let mut session = open();
    let id = note_id(&session, 0);
    let yaml = session.yaml();
    let begin = session.group_begin(0, "Bad gesture").expect("begin");

    session
        .apply_v2(&child(
            0,
            &begin.group_id,
            json!({"type":"note.set_tone","note_id":id,"tone":75}),
        ))
        .expect("first child is fine");

    // A resize that would make the duration non-positive is rejected by the
    // engine, which must poison the whole transaction.
    let err = session
        .apply_v2(&child(
            0,
            &begin.group_id,
            json!({"type":"note.resize","note_id":id,"delta_duration_ticks":-100000}),
        ))
        .expect_err("engine rejects this");

    assert_eq!(err.code(), ERR_GROUP_FAILED);
    assert!(!session.group_is_open(), "a failed child closes the group");
    assert_eq!(session.revision(), 0);
    assert_eq!(session.yaml(), yaml, "the tentative edit must be gone");
    assert_eq!(
        session
            .group_commit(&begin.group_id, 0)
            .expect_err("no group to commit")
            .code(),
        ERR_NOT_FOUND
    );
    assert_eq!(
        snapshot(&session)["project"]["tracks"][0]["parts"][0]["notes"][0]["tone"],
        60
    );
}

#[test]
fn other_operations_are_rejected_while_a_group_is_open() {
    let mut session = open();
    let id = note_id(&session, 0);
    let begin = session.group_begin(0, "Busy").expect("begin");

    let busy = session
        .apply_v2(
            json!({"expected_revision":0,"command":{"type":"note.set_tone","note_id":id,"tone":61}})
                .to_string()
                .as_str(),
        )
        .expect_err("must be busy");
    assert_eq!(busy.code(), ERR_SESSION_BUSY);

    assert!(session
        .apply_command(r#"{"cmd":"change_tone","part":0,"note":0,"tone":61}"#)
        .is_err());
    assert!(session.undo().is_err());
    assert!(session.redo().is_err());
    assert_eq!(
        session
            .undo_v2(99)
            .expect_err("busy takes precedence")
            .code(),
        ERR_SESSION_BUSY
    );
    assert_eq!(
        session
            .redo_v2(99)
            .expect_err("busy takes precedence")
            .code(),
        ERR_SESSION_BUSY
    );
    assert_eq!(
        session.group_begin(0, "Nested").expect_err("nested").code(),
        ERR_INVALID_GROUP_STATE
    );
    assert_eq!(session.revision(), 0);
    session.group_cancel(&begin.group_id).expect("cleanup");
}

#[test]
fn an_empty_group_commits_as_a_no_op() {
    let mut session = open();
    let begin = session.group_begin(0, "Nothing").expect("begin");

    let commit = session.group_commit(&begin.group_id, 0).expect("commit");

    assert!(!commit.changed);
    assert_eq!(commit.revision, 0);
    assert!(commit.changes.is_empty());
    assert!(!session.can_undo(), "a no-op group adds no undo step");
}

#[test]
fn a_stale_revision_conflicts_and_discards_the_group() {
    let mut session = open();
    let begin = session.group_begin(0, "Stale").expect("begin");

    let err = session
        .group_commit(&begin.group_id, 7)
        .expect_err("stale revision");
    assert!(err.is_conflict());
    assert!(!session.group_is_open(), "the tentative state is discarded");
    assert_eq!(session.revision(), 0);

    let begin = session.group_begin(0, "Wrong base").expect("begin");
    assert!(session
        .group_commit(&begin.group_id, 1)
        .expect_err("revision moved")
        .is_conflict());
}

#[test]
fn a_child_with_the_wrong_group_id_is_refused() {
    let mut session = open();
    let id = note_id(&session, 0);
    let begin = session.group_begin(0, "Owner").expect("begin");

    let err = session
        .apply_v2(&child(
            0,
            "not-this-group",
            json!({"type":"note.set_tone","note_id":id,"tone":61}),
        ))
        .expect_err("wrong group");

    assert_eq!(err.code(), ERR_INVALID_GROUP_STATE);
    assert!(session.group_is_open(), "the real group is untouched");
    session.group_cancel(&begin.group_id).expect("cleanup");
}

#[test]
fn a_child_with_an_unresolvable_id_fails_and_discards_the_group() {
    // Planning is part of child validation: every failed child poisons the
    // tentative transaction, including errors raised before engine apply.
    let mut session = open();
    let before = session.yaml();
    let begin = session.group_begin(0, "Plan failure").expect("begin");

    let err = session
        .apply_v2(&child(
            0,
            &begin.group_id,
            json!({"type":"note.set_tone","note_id":"ghost","tone":61}),
        ))
        .expect_err("unknown id");

    assert_eq!(err.code(), ERR_GROUP_FAILED);
    assert!(!session.group_is_open());
    assert_eq!(session.revision(), 0);
    assert_eq!(session.yaml(), before);
    assert_eq!(
        session
            .group_commit(&begin.group_id, 0)
            .expect_err("failed group is closed")
            .code(),
        ERR_NOT_FOUND
    );
}

#[test]
fn closing_a_session_with_an_open_group_is_not_a_commit() {
    // Dropping the session must not publish tentative work; the file-level
    // guarantee is that yaml() never contained it.
    let mut session = open();
    let yaml = session.yaml();
    let id = note_id(&session, 0);
    let begin = session.group_begin(0, "Abandoned").expect("begin");
    session
        .apply_v2(&child(
            0,
            &begin.group_id,
            json!({"type":"note.set_tone","note_id":id,"tone":80}),
        ))
        .expect("child");

    assert_eq!(session.yaml(), yaml);
    drop(session);
}

#[test]
fn planning_failures_are_typed() {
    let mut session = open();
    let id = note_id(&session, 0);

    // A missing type is a validation error with a field hint.
    let err = session
        .apply_v2(r#"{"expected_revision":0,"command":{}}"#)
        .expect_err("no type");
    assert_eq!(err.code(), "validation_error");
    assert_eq!(err.details["field"], "type");

    // A family this contract version does not implement yet is explicitly
    // `unsupported_operation`, not a validation error. (`curve.apply_stroke`
    // used to be the example here; it is implemented now, so `part.*` stands
    // in until that lands too.)
    let err = session
        .apply_v2(&format!(
            r#"{{"expected_revision":0,"command":{{"type":"part.add","note_id":"{id}"}}}}"#
        ))
        .expect_err("unsupported");
    assert_eq!(err.code(), "unsupported_operation");
    assert_eq!(err.details["type"], "part.add");
    let _: Option<SessionError> = None;
}
