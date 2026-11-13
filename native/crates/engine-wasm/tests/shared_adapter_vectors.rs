//! Cross-adapter logical vectors shared with project-session and synth-server.
use engine_wasm::Engine;
use serde_json::{json, Value};

const DEMO: &str = include_str!("../../../tools/synth-cli/tests/data/demo-song.ustx");
const VECTOR: &str = include_str!("../../project-session/tests/vectors/adapter_shared.json");
const CURVE_FIXTURE: &str =
    include_str!("../../project-session/tests/vectors/curve_adapter_shared.ustx");
const CURVE_VECTOR: &str =
    include_str!("../../project-session/tests/vectors/curve_adapter_shared.json");
const DOCUMENT_VECTOR: &str =
    include_str!("../../project-session/tests/vectors/document_adapter_shared.json");
const PART_VECTOR: &str =
    include_str!("../../project-session/tests/vectors/part_adapter_shared.json");
const EDGE_VECTORS: &str =
    include_str!("../../project-session/tests/vectors/project_part_edge_shared.json");
const SPLIT_CURVE_FIXTURE: &str =
    include_str!("../../project-session/tests/vectors/split_curve_shared.ustx");

#[test]
fn shared_document_vector_matches_direct_wasm_engine() {
    let vector: Value = serde_json::from_str(DOCUMENT_VECTOR).unwrap();
    let expected = &vector["expected"];
    let mut engine = Engine::new();
    let opened: Value = serde_json::from_str(&engine.session_open_yaml(DEMO, "").unwrap()).unwrap();
    let outcome: Value = serde_json::from_str(
        &engine
            .session_apply_v2(
                &json!({
                    "expected_revision": opened["revision"], "command": vector["command"]
                })
                .to_string(),
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(outcome["revision"], expected["revision"]);
    assert_eq!(outcome["changed"], expected["changed"]);
    let mut kinds: Vec<_> = outcome["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|change| change["kind"].as_str().unwrap())
        .collect();
    kinds.sort_unstable();
    let mut expected_kinds: Vec<_> = expected["change_kinds"]
        .as_array()
        .unwrap()
        .iter()
        .map(Value::as_str)
        .map(Option::unwrap)
        .collect();
    expected_kinds.sort_unstable();
    assert_eq!(kinds, expected_kinds);
    let snapshot: Value = serde_json::from_str(&engine.session_snapshot_json().unwrap()).unwrap();
    assert_eq!(snapshot["project"]["name"], expected["name"]);
    assert_eq!(snapshot["project"]["comment"], expected["comment"]);
    assert_eq!(snapshot["project"]["key"], expected["key"]);
}

#[test]
fn shared_part_vector_matches_direct_wasm_engine() {
    let vector: Value = serde_json::from_str(PART_VECTOR).unwrap();
    let expected = &vector["expected"];
    let mut engine = Engine::new();
    let opened: Value = serde_json::from_str(&engine.session_open_yaml(DEMO, "").unwrap()).unwrap();
    let part_id = opened["project"]["tracks"][0]["parts"][0]["id"]
        .as_str()
        .unwrap();
    let mut command = vector["command"].clone();
    command["part_id"] = Value::String(part_id.to_string());
    let begin: Value =
        serde_json::from_str(&engine.session_group_begin(0, "shared part vector").unwrap())
            .unwrap();
    let child: Value = serde_json::from_str(
        &engine
            .session_apply_v2(
                &json!({
                    "expected_revision":0,"group_id":begin["group_id"],"command":command
                })
                .to_string(),
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(child["changes"].as_array().unwrap().len(), 0);
    let outcome: Value = serde_json::from_str(
        &engine
            .session_group_commit(begin["group_id"].as_str().unwrap(), 0)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(outcome["revision"], expected["revision"]);
    assert_eq!(outcome["changed"], expected["changed"]);
    let mut kinds: Vec<_> = outcome["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|change| change["kind"].as_str().unwrap())
        .collect();
    kinds.sort_unstable();
    let mut expected_kinds: Vec<_> = expected["change_kinds"]
        .as_array()
        .unwrap()
        .iter()
        .map(Value::as_str)
        .map(Option::unwrap)
        .collect();
    expected_kinds.sort_unstable();
    assert_eq!(kinds, expected_kinds);
    let snapshot: Value = serde_json::from_str(&engine.session_snapshot_json().unwrap()).unwrap();
    assert_eq!(
        snapshot["project"]["tracks"][0]["parts"][0]["name"],
        expected["name"]
    );
    assert_eq!(
        snapshot["project"]["tracks"][0]["parts"][0]["comment"],
        expected["comment"]
    );
}

#[test]
fn split_curve_boundary_is_preserved_by_direct_wasm_engine() {
    let mut engine = Engine::new();
    let opened: Value =
        serde_json::from_str(&engine.session_open_yaml(SPLIT_CURVE_FIXTURE, "").unwrap()).unwrap();
    let part_id = opened["project"]["tracks"][0]["parts"][0]["id"]
        .as_str()
        .unwrap();
    let original = opened["project"]["tracks"][0]["parts"][0]["curves"][0]["points"].clone();
    engine.session_apply_v2(&json!({
        "expected_revision":0,"command":{"type":"part.split","part_id":part_id,"at_tick":960}
    }).to_string()).unwrap();
    let split: Value = serde_json::from_str(&engine.session_snapshot_json().unwrap()).unwrap();
    let parts = split["project"]["tracks"][0]["parts"].as_array().unwrap();
    assert_eq!(
        parts[0]["curves"][0]["points"],
        json!([{"tick":0,"value":0},{"tick":960,"value":10}])
    );
    assert_eq!(
        parts[1]["curves"][0]["points"],
        json!([{"tick":0,"value":10},{"tick":960,"value":20}])
    );
    engine.session_undo_v2(1).unwrap();
    let undone: Value = serde_json::from_str(&engine.session_snapshot_json().unwrap()).unwrap();
    assert_eq!(
        undone["project"]["tracks"][0]["parts"][0]["curves"][0]["points"],
        original
    );
}

#[test]
fn shared_adapter_vector_matches_direct_wasm_engine() {
    let vector: Value = serde_json::from_str(VECTOR).expect("shared vector JSON");
    let mut engine = Engine::new();
    let capabilities: Value =
        serde_json::from_str(&engine.session_capabilities_json().unwrap()).unwrap();
    assert_eq!(
        capabilities["contract_version"],
        vector["capabilities"]["contract_version"]
    );
    assert_eq!(
        capabilities["command_schema_version"],
        vector["capabilities"]["command_schema_version"]
    );
    assert_eq!(
        capabilities["snapshot_schema_version"],
        vector["capabilities"]["snapshot_schema_version"]
    );
    assert_eq!(
        capabilities["id_schema_version"],
        vector["capabilities"]["id_schema_version"]
    );
    assert_eq!(
        capabilities["limits"]["max_stroke_points"],
        vector["capabilities"]["max_stroke_points"]
    );
    assert_eq!(capabilities["render"]["async_jobs"], false);
    let opened: Value = serde_json::from_str(&engine.session_open_yaml(DEMO, "").unwrap()).unwrap();
    let project_read: Value =
        serde_json::from_str(&engine.session_project_json().unwrap()).unwrap();
    let initial_part_id = opened["project"]["tracks"][0]["parts"][0]["id"]
        .as_str()
        .unwrap();
    let part_read: Value =
        serde_json::from_str(&engine.session_part_json(initial_part_id).unwrap()).unwrap();
    assert_eq!(project_read["revision"], opened["revision"]);
    assert_eq!(project_read["project"]["id"], opened["project"]["id"]);
    assert_eq!(part_read["revision"], opened["revision"]);
    assert_eq!(part_read["part"]["id"], initial_part_id);
    let note_id = opened["project"]["tracks"][0]["parts"][0]["notes"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let track_id = opened["project"]["tracks"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let mut group_id: Option<String> = None;
    for step in vector["steps"].as_array().unwrap() {
        let expected = &step["expected"];
        match step["op"].as_str().unwrap() {
            "set_lyric" | "stale_set_lyric" => {
                let result = engine.session_apply_v2(
                    &json!({
                        "expected_revision": 0,
                        "command":{"type":"note.set_lyric","note_id":note_id,"lyric":step["lyric"]}
                    })
                    .to_string(),
                );
                if let Some(code) = expected.get("code").and_then(Value::as_str) {
                    let error: Value =
                        serde_json::from_str(&result.expect_err("expected conflict")).unwrap();
                    assert_eq!(error["code"], code);
                } else {
                    let outcome: Value = serde_json::from_str(&result.unwrap()).unwrap();
                    let mut kinds: Vec<_> = outcome["changes"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|change| change["kind"].as_str().unwrap())
                        .collect();
                    kinds.sort_unstable();
                    let expected_kinds: Vec<_> = expected["change_kinds"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| v.as_str().unwrap())
                        .collect();
                    assert_eq!(outcome["revision"], expected["revision"]);
                    assert_eq!(outcome["changed"], expected["changed"]);
                    assert_eq!(kinds, expected_kinds);
                }
            }
            "undo" => {
                let outcome: Value = serde_json::from_str(
                    &engine
                        .session_undo_v2(expected["revision"].as_u64().unwrap() - 1)
                        .unwrap(),
                )
                .unwrap();
                assert_eq!(outcome["revision"], expected["revision"]);
            }
            "redo" => {
                let outcome: Value = serde_json::from_str(
                    &engine
                        .session_redo_v2(expected["revision"].as_u64().unwrap() - 1)
                        .unwrap(),
                )
                .unwrap();
                assert_eq!(outcome["revision"], expected["revision"]);
            }
            "snapshot" => {
                let snapshot: Value =
                    serde_json::from_str(&engine.session_snapshot_json().unwrap()).unwrap();
                assert_eq!(snapshot["revision"], expected["revision"]);
                assert_eq!(
                    snapshot["project"]["tracks"][0]["parts"][0]["notes"][0]["lyric"],
                    expected["lyric"]
                );
                if let Some(tone) = expected.get("tone") {
                    assert_eq!(
                        snapshot["project"]["tracks"][0]["parts"][0]["notes"][0]["tone"],
                        *tone
                    );
                }
                if let Some(mute) = expected.get("mute") {
                    assert_eq!(snapshot["project"]["tracks"][0]["mute"], *mute);
                }
            }
            "read_notes" => {
                let part_id = opened["project"]["tracks"][0]["parts"][0]["id"]
                    .as_str()
                    .unwrap();
                let notes: Value =
                    serde_json::from_str(&engine.session_part_notes_json(part_id).unwrap())
                        .unwrap();
                assert_eq!(notes["revision"], expected["revision"]);
                assert_eq!(
                    notes["notes"].as_array().unwrap().len(),
                    expected["note_count"]
                );
            }
            "read_tracks" => {
                let tracks: Value =
                    serde_json::from_str(&engine.session_tracks_json().unwrap()).unwrap();
                assert_eq!(tracks["revision"], expected["revision"]);
                assert_eq!(
                    tracks["tracks"].as_array().unwrap().len(),
                    expected["track_count"]
                );
            }
            "group_begin" => {
                let outcome: Value = serde_json::from_str(
                    &engine
                        .session_group_begin(
                            expected["revision"].as_u64().unwrap(),
                            step["name"].as_str().unwrap(),
                        )
                        .unwrap(),
                )
                .unwrap();
                group_id = Some(outcome["group_id"].as_str().unwrap().to_string());
                assert_eq!(outcome["base_revision"], expected["revision"]);
            }
            "group_tone" => {
                let outcome: Value = serde_json::from_str(&engine.session_apply_v2(&json!({
                    "expected_revision": expected["revision"],
                    "group_id": group_id.as_deref().unwrap(),
                    "command":{"type":"note.set_tone","note_id":note_id,"tone":step["tone"]}
                }).to_string()).unwrap()).unwrap();
                assert_eq!(outcome["revision"], expected["revision"]);
                assert_eq!(outcome["changed"], expected["changed"]);
            }
            "group_commit" => {
                let outcome: Value = serde_json::from_str(
                    &engine
                        .session_group_commit(
                            group_id.as_deref().unwrap(),
                            expected["revision"].as_u64().unwrap() - 1,
                        )
                        .unwrap(),
                )
                .unwrap();
                group_id = None;
                assert_eq!(outcome["revision"], expected["revision"]);
                assert_eq!(outcome["changed"], expected["changed"]);
            }
            "track_mute" => {
                let outcome: Value = serde_json::from_str(&engine.session_apply_v2(&json!({
                    "expected_revision": expected["revision"].as_u64().unwrap() - 1,
                    "command":{"type":"track.set_mute","track_id":track_id,"mute":step["mute"]}
                }).to_string()).unwrap()).unwrap();
                let mut kinds: Vec<_> = outcome["changes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|change| change["kind"].as_str().unwrap())
                    .collect();
                kinds.sort_unstable();
                let expected_kinds: Vec<_> = expected["change_kinds"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap())
                    .collect();
                assert_eq!(outcome["revision"], expected["revision"]);
                assert_eq!(outcome["changed"], expected["changed"]);
                assert_eq!(kinds, expected_kinds);
            }
            operation => panic!("unknown shared operation {operation}"),
        }
    }
}

#[test]
fn shared_curve_adapter_vector_matches_direct_wasm_engine() {
    let vector: Value = serde_json::from_str(CURVE_VECTOR).unwrap();
    let mut engine = Engine::new();
    let opened: Value =
        serde_json::from_str(&engine.session_open_yaml(CURVE_FIXTURE, "").unwrap()).unwrap();
    let part_id = opened["project"]["tracks"][0]["parts"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let curve_id = opened["project"]["tracks"][0]["parts"][0]["curves"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    for step in vector["steps"].as_array().unwrap() {
        let expected = &step["expected"];
        match step["op"].as_str().unwrap() {
            "oversized_stroke" => {
                let points: Vec<_> = (0..step["point_count"].as_u64().unwrap())
                    .map(|i| json!({"tick": (i % 961), "value": 1}))
                    .collect();
                let error: Value = serde_json::from_str(&engine.session_apply_v2(&json!({
                    "expected_revision": step["revision"],
                    "command": {
                        "type":"curve.apply_stroke", "part_id":part_id, "curve_id":curve_id,
                        "mode":"replace", "start_tick":0, "end_tick":960, "points":points
                    }
                }).to_string()).expect_err("oversized vector rejected")).unwrap();
                assert_eq!(error["code"], expected["code"]);
                let snapshot: Value =
                    serde_json::from_str(&engine.session_snapshot_json().unwrap()).unwrap();
                assert_eq!(snapshot["revision"], expected["revision"]);
            }
            "curve_apply" => {
                engine
                    .session_apply_v2(
                        &json!({
                            "expected_revision": step["revision"],
                            "command": {
                                "type":"curve.apply_stroke", "part_id":part_id, "curve_id":curve_id,
                                "mode":"replace", "start_tick":0, "end_tick":960,
                                "points":step["points"]
                            }
                        })
                        .to_string(),
                    )
                    .unwrap();
                let curve: Value =
                    serde_json::from_str(&engine.session_curve_json(&part_id, &curve_id).unwrap())
                        .unwrap();
                assert_eq!(curve["points"], expected["points"]);
            }
            "undo" => {
                let outcome: Value = serde_json::from_str(
                    &engine
                        .session_undo_v2(step["revision"].as_u64().unwrap())
                        .unwrap(),
                )
                .unwrap();
                assert_eq!(outcome["revision"], expected["revision"]);
                let curve: Value =
                    serde_json::from_str(&engine.session_curve_json(&part_id, &curve_id).unwrap())
                        .unwrap();
                assert_eq!(curve["points"], expected["points"]);
            }
            operation => panic!("unknown shared curve operation {operation}"),
        }
    }
}

#[test]
fn shared_project_part_invalid_and_noop_vectors_are_atomic_in_wasm() {
    let vectors: Value = serde_json::from_str(EDGE_VECTORS).unwrap();
    for case in vectors["cases"].as_array().unwrap() {
        let mut engine = Engine::new();
        let before: Value =
            serde_json::from_str(&engine.session_open_yaml(DEMO, "").unwrap()).unwrap();
        let mut command = case["command"].clone();
        if command["track_id"] == "@track:0" {
            command["track_id"] = before["project"]["tracks"][0]["id"].clone();
        }
        if command["part_id"] == "@part:0" {
            command["part_id"] = before["project"]["tracks"][0]["parts"][0]["id"].clone();
        }
        let result =
            engine.session_apply_v2(&json!({"expected_revision":0,"command":command}).to_string());
        let expected = &case["expected"];
        if let Some(code) = expected["code"].as_str() {
            let error: Value = serde_json::from_str(&result.unwrap_err()).unwrap();
            assert_eq!(error["code"], code, "{}", case["name"]);
        } else {
            let outcome: Value = serde_json::from_str(&result.unwrap()).unwrap();
            assert_eq!(outcome["changed"], expected["changed"], "{}", case["name"]);
        }
        let after: Value = serde_json::from_str(&engine.session_snapshot_json().unwrap()).unwrap();
        assert_eq!(after["revision"], expected["revision"], "{}", case["name"]);
        assert_eq!(after["project"], before["project"], "{}", case["name"]);
        assert_eq!(after["can_undo"], before["can_undo"], "{}", case["name"]);
        assert_eq!(after["can_redo"], before["can_redo"], "{}", case["name"]);
    }
}
