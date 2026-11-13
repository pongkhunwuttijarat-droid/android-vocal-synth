use project_session::Session;
use serde_json::{json, Value};

const SPLIT_CURVE_FIXTURE: &str = include_str!("vectors/split_curve_shared.ustx");
const EDGE_VECTORS: &str = include_str!("vectors/project_part_edge_shared.json");
const DEMO: &str = include_str!("../../../tools/synth-cli/tests/data/demo-song.ustx");

fn run(session: &mut Session, command: Value) -> Result<Value, Value> {
    let request = json!({"expected_revision": session.revision(), "command": command});
    session
        .apply_v2(&request.to_string())
        .map(|result| serde_json::to_value(result).unwrap())
        .map_err(|error| serde_json::from_str(&error.to_json()).unwrap())
}

fn snapshot(session: &Session) -> Value {
    serde_json::from_str(&session.snapshot_json().unwrap()).unwrap()
}

#[test]
fn project_metadata_tempo_and_meter_follow_revision_and_undo_rules() {
    let mut session = Session::new(17);
    let first = snapshot(&session);
    assert_eq!(first["revision"], 0);
    let unchanged = run(
        &mut session,
        json!({"type":"project.set_tempo","position_tick":0,"bpm":120.0}),
    )
    .unwrap();
    assert_eq!(unchanged["changed"], false);
    assert_eq!(session.revision(), 0);

    run(
        &mut session,
        json!({"type":"project.set_metadata","fields":{"name":"Verse","comment":"","key":7}}),
    )
    .unwrap();
    assert_eq!(session.revision(), 1);
    let current = snapshot(&session);
    assert_eq!(current["project"]["name"], "Verse");
    assert_eq!(current["project"]["key"], 7);

    run(
        &mut session,
        json!({"type":"project.set_tempo","position_tick":1920,"bpm":98.5}),
    )
    .unwrap();
    run(
        &mut session,
        json!({"type":"project.set_time_signature","bar_position":4,"numerator":7,"denominator":8}),
    )
    .unwrap();
    assert_eq!(session.revision(), 3);
    assert!(run(
        &mut session,
        json!({"type":"project.remove_tempo","position_tick":0})
    )
    .is_err());
    assert_eq!(session.revision(), 3);

    session.undo_v2(3).unwrap();
    assert_eq!(session.revision(), 4);
    assert_eq!(
        snapshot(&session)["project"]["time_signatures"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    session.redo_v2(4).unwrap();
    assert_eq!(
        snapshot(&session)["project"]["time_signatures"][1]["numerator"],
        7
    );
}

#[test]
fn preserve_timing_scales_all_tick_positions_with_tempo_and_undoes_atomically() {
    let mut session = Session::open(DEMO, None, 19).unwrap();
    run(
        &mut session,
        json!({"type":"project.set_tempo","position_tick":1920,"bpm":150.0}),
    )
    .unwrap();
    let before = snapshot(&session);
    let old_bpm = before["project"]["bpm"].as_f64().unwrap();
    let old_part = before["project"]["tracks"][0]["parts"][0].clone();
    let part_id = old_part["id"].as_str().unwrap().to_string();
    let original_pitch = old_part["notes"][0]["pitch"].clone();
    let original_meter = before["project"]["time_signatures"].clone();
    let new_bpm = old_bpm / 2.0;

    let result = run(
        &mut session,
        json!({"type":"project.preserve_timing","bpm":new_bpm}),
    )
    .unwrap();
    assert_eq!(result["changed"], true);
    assert_eq!(session.revision(), 2);

    let after = snapshot(&session);
    assert_eq!(after["project"]["bpm"], json!(new_bpm));
    assert_eq!(after["project"]["time_signatures"], original_meter);
    assert_eq!(after["project"]["tempos"][1]["position_tick"], 960);
    assert_eq!(after["project"]["tempos"][1]["bpm"], 75.0);
    let new_part = after["project"]["tracks"][0]["parts"][0].clone();
    assert_eq!(new_part["id"], part_id);
    assert_eq!(
        new_part["position_ticks"],
        old_part["position_ticks"].as_i64().unwrap() / 2
    );
    assert_eq!(
        new_part["duration_ticks"],
        old_part["duration_ticks"].as_i64().unwrap() / 2
    );
    assert_eq!(
        new_part["notes"][0]["position_ticks"],
        old_part["notes"][0]["position_ticks"].as_i64().unwrap() / 2
    );
    assert_eq!(
        new_part["notes"][0]["duration_ticks"],
        old_part["notes"][0]["duration_ticks"].as_i64().unwrap() / 2
    );
    assert_eq!(
        new_part["notes"][0]["pitch"], original_pitch,
        "millisecond pitch points stay fixed"
    );

    session.undo_v2(2).unwrap();
    assert_eq!(snapshot(&session)["project"], before["project"]);
    session.redo_v2(3).unwrap();
    assert_eq!(snapshot(&session)["project"], after["project"]);
}

#[test]
fn preserve_timing_rejects_overflow_without_mutating_the_project() {
    let mut session = Session::open(DEMO, None, 29).unwrap();
    let before = snapshot(&session);
    let result = run(
        &mut session,
        json!({"type":"project.preserve_timing","bpm":1.0e308}),
    );
    assert_eq!(result.unwrap_err()["code"], "validation_error");
    assert_eq!(session.revision(), 0);
    assert_eq!(snapshot(&session)["project"], before["project"]);
    assert!(!session.can_undo());
}

#[test]
fn voice_part_create_move_resize_split_and_remove_are_reversible() {
    let mut session = Session::new(23);
    let initial = snapshot(&session);
    let track_id = initial["project"]["tracks"][0]["id"].as_str().unwrap();
    let created = run(&mut session, json!({"type":"part.create","track_id":track_id,"kind":"voice","name":"Verse","position_ticks":480,"duration_ticks":1920})).unwrap();
    assert_eq!(created["changes"][0]["kind"], "part.added");
    let before = snapshot(&session);
    let part = &before["project"]["tracks"][0]["parts"][0];
    let part_id = part["id"].as_str().unwrap().to_string();
    assert_eq!(part["position_ticks"], 480);

    run(
        &mut session,
        json!({"type":"part.move","part_id":part_id,"position_ticks":960}),
    )
    .unwrap();
    run(
        &mut session,
        json!({"type":"part.resize","part_id":part_id,"duration_ticks":2880}),
    )
    .unwrap();
    run(
        &mut session,
        json!({"type":"part.split","part_id":part_id,"at_tick":960}),
    )
    .unwrap();
    let split = snapshot(&session);
    let parts = split["project"]["tracks"][0]["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0]["id"], part_id);
    assert_ne!(parts[0]["id"], parts[1]["id"]);
    assert_eq!(parts[0]["duration_ticks"], 960);
    assert_eq!(parts[1]["position_ticks"], 1920);
    session.undo_v2(session.revision()).unwrap();
    assert_eq!(
        snapshot(&session)["project"]["tracks"][0]["parts"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    run(
        &mut session,
        json!({"type":"part.remove","part_id":part_id}),
    )
    .unwrap();
    assert!(snapshot(&session)["project"]["tracks"][0]["parts"]
        .as_array()
        .unwrap()
        .is_empty());
    session.undo_v2(session.revision()).unwrap();
    assert_eq!(
        snapshot(&session)["project"]["tracks"][0]["parts"][0]["id"],
        part_id
    );
}

#[test]
fn invalid_part_and_project_commands_leave_revision_and_document_unchanged() {
    let mut session = Session::new(29);
    let before = session.yaml();
    assert!(run(
        &mut session,
        json!({"type":"project.set_time_signature","bar_position":1,"numerator":7,"denominator":11})
    )
    .is_err());
    assert!(run(&mut session, json!({"type":"part.create","track_id":"missing","kind":"voice","name":"x","position_ticks":0,"duration_ticks":100})).is_err());
    assert_eq!(session.revision(), 0);
    assert_eq!(session.yaml(), before);
}

#[test]
fn wave_parts_have_stable_ids_media_refs_and_round_trip() {
    let mut session = Session::new(37);
    let track_id = snapshot(&session)["project"]["tracks"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    run(&mut session, json!({"type":"part.create","track_id":track_id,"kind":"wave","name":"Backing","position_ticks":960,"duration_ticks":3840,"media_ref":"asset:backing.wav"})).unwrap();
    let first = snapshot(&session);
    let wave = &first["project"]["tracks"][0]["parts"][0];
    assert_eq!(wave["kind"], "wave");
    assert_eq!(wave["media_ref"], "asset:backing.wav");
    assert_eq!(wave["position_ticks"], 960);
    assert_eq!(wave["duration_ticks"], 3840);
    let part_id = wave["id"].as_str().unwrap().to_string();
    run(
        &mut session,
        json!({"type":"part.set_media_ref","part_id":part_id,"media_ref":"asset:relinked-backing"}),
    )
    .unwrap();
    assert_eq!(
        snapshot(&session)["project"]["tracks"][0]["parts"][0]["media_ref"],
        "asset:relinked-backing"
    );
    assert!(run(
        &mut session,
        json!({"type":"part.set_media_ref","part_id":part_id,"media_ref":"../../outside.mp3"}),
    )
    .is_err());
    assert_eq!(
        snapshot(&session)["project"]["tracks"][0]["parts"][0]["media_ref"],
        "asset:relinked-backing"
    );
    session.undo_v2(session.revision()).unwrap();
    assert_eq!(
        snapshot(&session)["project"]["tracks"][0]["parts"][0]["media_ref"],
        "asset:backing.wav"
    );
    run(
        &mut session,
        json!({"type":"part.move","part_id":part_id,"position_ticks":1440}),
    )
    .unwrap();
    assert_eq!(
        snapshot(&session)["project"]["tracks"][0]["parts"][0]["position_ticks"],
        1440
    );
    let sidecar = session.save_sidecar().unwrap();
    let reopened = Session::open(&session.yaml(), Some(&sidecar), 41).unwrap();
    assert_eq!(
        snapshot(&reopened)["project"]["tracks"][0]["parts"][0]["id"],
        part_id
    );

    assert!(run(&mut session, json!({"type":"part.create","track_id":track_id,"kind":"wave","name":"Bad","position_ticks":0,"duration_ticks":480,"media_ref":"/etc/passwd"})).is_err());
    run(&mut session, json!({"type":"part.set_metadata","part_id":part_id,"fields":{"name":"Backing 2","comment":"bed"}})).unwrap();
    assert_eq!(
        snapshot(&session)["project"]["tracks"][0]["parts"][0]["name"],
        "Backing 2"
    );
    run(
        &mut session,
        json!({"type":"part.resize","part_id":part_id,"duration_ticks":1920}),
    )
    .unwrap();
    assert_eq!(
        snapshot(&session)["project"]["tracks"][0]["parts"][0]["duration_ticks"],
        1920
    );
}

#[test]
fn bounded_project_and_part_reads_include_authoritative_revision_and_shared_views() {
    let mut session = Session::new(43);
    let track_id = snapshot(&session)["project"]["tracks"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    run(&mut session, json!({"type":"part.create","track_id":track_id,"kind":"voice","name":"Read me","position_ticks":0,"duration_ticks":1920})).unwrap();
    let snapshot = snapshot(&session);
    let part_id = snapshot["project"]["tracks"][0]["parts"][0]["id"]
        .as_str()
        .unwrap();
    let project: Value = serde_json::from_str(&session.project_json().unwrap()).unwrap();
    let part: Value = serde_json::from_str(&session.part_json(part_id).unwrap()).unwrap();
    assert_eq!(project["revision"], session.revision());
    assert_eq!(project["project"]["id"], snapshot["project"]["id"]);
    assert_eq!(part["revision"], session.revision());
    assert_eq!(part["track_id"], track_id);
    assert_eq!(part["part"], snapshot["project"]["tracks"][0]["parts"][0]);
    assert!(session.part_json("missing").is_err());
}

#[test]
fn document_commands_group_commit_once_and_stale_retry_is_atomic() {
    let mut session = Session::new(47);
    let begin = session.group_begin(0, "document metadata").unwrap();
    let child = session
        .apply_v2(
            &json!({
                "expected_revision":0,"group_id":begin.group_id,
                "command":{"type":"project.set_metadata","fields":{"name":"Grouped"}}
            })
            .to_string(),
        )
        .unwrap();
    assert_eq!(child.revision, 0);
    assert!(child.changes.is_empty());
    assert_eq!(snapshot(&session)["project"]["name"], "New Project");
    let committed = session.group_commit(&begin.group_id, 0).unwrap();
    assert_eq!(committed.revision, 1);
    let kinds: Vec<_> = committed
        .changes
        .iter()
        .map(|change| change.kind.as_str())
        .collect();
    assert!(kinds.contains(&"project.changed"));
    assert!(kinds.contains(&"render.invalidated"));
    let stale = session
        .apply_v2(
            &json!({
                "expected_revision":0,
                "command":{"type":"project.set_metadata","fields":{"name":"Stale"}}
            })
            .to_string(),
        )
        .unwrap_err();
    assert_eq!(stale.code(), "revision_conflict");
    assert_eq!(snapshot(&session)["project"]["name"], "Grouped");
    assert_eq!(session.revision(), 1);
}

#[test]
fn split_preserves_curve_boundary_sample_and_undo_restores_exact_curve() {
    let mut session = Session::open(SPLIT_CURVE_FIXTURE, None, 53).unwrap();
    let before = snapshot(&session);
    let part_id = before["project"]["tracks"][0]["parts"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let original_points = before["project"]["tracks"][0]["parts"][0]["curves"][0]["points"].clone();
    run(
        &mut session,
        json!({"type":"part.split","part_id":part_id,"at_tick":960}),
    )
    .unwrap();
    let after = snapshot(&session);
    let parts = after["project"]["tracks"][0]["parts"].as_array().unwrap();
    assert_eq!(parts[0]["id"], part_id);
    assert_eq!(
        parts[0]["curves"][0]["points"],
        json!([{"tick":0,"value":0},{"tick":960,"value":10}])
    );
    assert_eq!(
        parts[1]["curves"][0]["points"],
        json!([{"tick":0,"value":10},{"tick":960,"value":20}])
    );
    session.undo_v2(session.revision()).unwrap();
    assert_eq!(
        snapshot(&session)["project"]["tracks"][0]["parts"][0]["curves"][0]["points"],
        original_points
    );
}

#[test]
fn shared_project_part_invalid_and_noop_vectors_are_atomic() {
    let vectors: Value = serde_json::from_str(EDGE_VECTORS).unwrap();
    for case in vectors["cases"].as_array().unwrap() {
        let mut session = Session::open(
            include_str!("../../../tools/synth-cli/tests/data/demo-song.ustx"),
            None,
            101,
        )
        .unwrap();
        let before = snapshot(&session);
        let mut command = case["command"].clone();
        if command["track_id"] == "@track:0" {
            command["track_id"] = before["project"]["tracks"][0]["id"].clone();
        }
        if command["part_id"] == "@part:0" {
            command["part_id"] = before["project"]["tracks"][0]["parts"][0]["id"].clone();
        }
        let expected = &case["expected"];
        let result = run(&mut session, command);
        if let Some(code) = expected["code"].as_str() {
            assert_eq!(result.unwrap_err()["code"], code, "{}", case["name"]);
        } else {
            let outcome = result.unwrap();
            assert_eq!(outcome["changed"], expected["changed"], "{}", case["name"]);
        }
        let after = snapshot(&session);
        assert_eq!(session.revision(), expected["revision"], "{}", case["name"]);
        assert_eq!(after["project"], before["project"], "{}", case["name"]);
        assert_eq!(after["can_undo"], before["can_undo"], "{}", case["name"]);
        assert_eq!(after["can_redo"], before["can_redo"], "{}", case["name"]);
    }
}
