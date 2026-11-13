use project_session::Session;
use serde_json::{json, Value};

const DEMO: &str = include_str!("../../../tools/synth-cli/tests/data/demo-song.ustx");

fn run(session: &mut Session, command: Value) -> Value {
    let request = json!({
        "expected_revision": session.revision(),
        "command": command,
    });
    let result = session
        .apply_v2(&request.to_string())
        .expect("command succeeds");
    serde_json::to_value(result).expect("mutation result JSON")
}

fn pronunciation() -> Value {
    json!({
        "language": "en",
        "mode": "universal",
        "style": "cv",
        "strategy": "g2p_cvvc",
        "language_phones": ["s", "i"],
        "universal_phones": ["s", "i"]
    })
}

fn ids(session: &Session) -> (String, String) {
    let part = &session.identity().tracks[0].parts[0];
    (
        part.id.as_str().to_string(),
        part.notes[0].id.as_str().to_string(),
    )
}

#[test]
fn pronunciation_is_undoable_and_round_trips_in_its_own_sidecar() {
    let mut session = Session::open(DEMO, None, 71).expect("open demo");
    let (part_id, note_id) = ids(&session);
    let base_yaml = session.yaml();

    run(
        &mut session,
        json!({"type":"note.set_pronunciation", "note_id":note_id, "pronunciation":pronunciation()}),
    );
    assert_eq!(session.revision(), 1);
    assert_eq!(
        session.yaml(),
        base_yaml,
        "private pronunciation must not leak into USTX"
    );
    let selected = session.render_pronunciations_for_track(0);
    assert_eq!(selected.len(), 1);
    assert!(selected[0].iter().any(Option::is_some));

    let id_sidecar = session.save_sidecar().expect("identity sidecar");
    let choice_sidecar = session
        .save_pronunciation_sidecar()
        .expect("pronunciation sidecar");
    assert!(choice_sidecar.contains("rusted-utau.pronunciations/1"));
    let reopened = Session::open_with_pronunciation_sidecar(
        &base_yaml,
        Some(&id_sidecar),
        Some(&choice_sidecar),
        72,
    )
    .expect("reopen session");
    assert_eq!(
        reopened.render_pronunciations_for_track(0),
        selected,
        "stable note IDs restore pronunciation choices"
    );
    assert_eq!(reopened.identity().tracks[0].parts[0].id.as_str(), part_id);

    session.undo_v2(session.revision()).expect("undo selection");
    assert!(session.render_pronunciations_for_track(0)[0]
        .iter()
        .all(Option::is_none));
    session.redo_v2(session.revision()).expect("redo selection");
    assert_eq!(session.render_pronunciations_for_track(0), selected);
}

#[test]
fn lyric_edit_clears_choice_and_clear_command_is_atomic() {
    let mut session = Session::open(DEMO, None, 81).expect("open demo");
    let (_, note_id) = ids(&session);
    run(
        &mut session,
        json!({"type":"note.set_pronunciation", "note_id":note_id, "pronunciation":pronunciation()}),
    );
    run(
        &mut session,
        json!({"type":"note.set_lyric", "note_id":note_id, "lyric":"new lyric"}),
    );
    assert!(session.render_pronunciations_for_track(0)[0]
        .iter()
        .all(Option::is_none));

    let (_, note_id) = ids(&session);
    run(
        &mut session,
        json!({"type":"note.set_pronunciation", "note_id":note_id, "pronunciation":pronunciation()}),
    );
    run(
        &mut session,
        json!({"type":"note.set_pronunciation", "note_id":note_id, "pronunciation":Value::Null}),
    );
    assert!(session.render_pronunciations_for_track(0)[0]
        .iter()
        .all(Option::is_none));
}

#[test]
fn pronunciation_and_lyric_changes_refuse_to_retarget_phoneme_data_until_cleared() {
    let yaml = DEMO.replacen(
        "lyric: see[s i]",
        "lyric: see[s i]\n    phoneme_expressions:\n    - {index: 0, abbr: vel, value: 123}\n    phoneme_overrides:\n    - {index: 0, offset: 5}",
        1,
    );
    let mut session = Session::open(&yaml, None, 91).expect("open fixture");
    let (_, note_id) = ids(&session);
    let before = session.snapshot_json().unwrap();
    for command in [
        json!({"type":"note.set_lyric","note_id":note_id,"lyric":"changed"}),
        json!({"type":"note.set_pronunciation","note_id":note_id,"pronunciation":pronunciation()}),
    ] {
        let error = session
            .apply_v2(&json!({"expected_revision":0,"command":command}).to_string())
            .expect_err("dependent phoneme data must not be rebound by index");
        assert_eq!(error.details["reason"], "phoneme_data_requires_clear");
        assert_eq!(session.revision(), 0);
        assert_eq!(session.snapshot_json().unwrap(), before);
    }

    run(
        &mut session,
        json!({"type":"note.clear_phoneme_data","note_id":note_id}),
    );
    let cleared: Value = serde_json::from_str(&session.snapshot_json().unwrap()).unwrap();
    assert_eq!(
        cleared["project"]["tracks"][0]["parts"][0]["notes"][0]["phoneme_expressions"],
        json!([])
    );
    assert_eq!(
        cleared["project"]["tracks"][0]["parts"][0]["notes"][0]["phoneme_overrides"],
        json!([])
    );
    run(
        &mut session,
        json!({"type":"note.set_lyric","note_id":note_id,"lyric":"changed"}),
    );
    assert_eq!(session.revision(), 2);
}
