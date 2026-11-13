//! v2 session endpoints (`docs/CONTRACT-v1.md` §6.7) at the handler level.
//!
//! These call the handlers directly with a real `AppState` — the same style as
//! `api_real.rs` — so no socket or renderer is needed. They pin the parts a
//! client depends on: revision movement, typed status codes, the TTL policy,
//! and the fact that only the v2 save path writes an identity sidecar.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use engine_wasm::Engine;
use serde_json::{json, Value};
use synth_server::handlers::JsonBody;
use synth_server::handlers_v2::{
    self, CreateSessionRequest, CreateSource, GroupBeginRequest, GroupCommitRequest,
    RenderStartRequest, SaveRequest, StrokeContextQuery,
};
use synth_server::state::AppState;

const DEMO: &str = include_str!("../../synth-cli/tests/data/demo-song.ustx");
const SHARED_ADAPTER_VECTOR: &str =
    include_str!("../../../crates/project-session/tests/vectors/adapter_shared.json");
const SHARED_CURVE_FIXTURE: &str =
    include_str!("../../../crates/project-session/tests/vectors/curve_adapter_shared.ustx");
const SHARED_CURVE_VECTOR: &str =
    include_str!("../../../crates/project-session/tests/vectors/curve_adapter_shared.json");
const DOCUMENT_ADAPTER_VECTOR: &str =
    include_str!("../../../crates/project-session/tests/vectors/document_adapter_shared.json");
const PART_ADAPTER_VECTOR: &str =
    include_str!("../../../crates/project-session/tests/vectors/part_adapter_shared.json");
const EDGE_VECTORS: &str =
    include_str!("../../../crates/project-session/tests/vectors/project_part_edge_shared.json");
const SPLIT_CURVE_FIXTURE: &str =
    include_str!("../../../crates/project-session/tests/vectors/split_curve_shared.ustx");

#[tokio::test]
async fn shared_document_vector_matches_native_http_routes() {
    let vector: Value = serde_json::from_str(DOCUMENT_ADAPTER_VECTOR).unwrap();
    let expected = &vector["expected"];
    let state = state("shared-document-vector");
    let id = open_stored(&state, "shared-document").await;
    let (status, outcome) = command(
        &state,
        &id,
        json!({"expected_revision":0,"command":vector["command"]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
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
    let (status, snapshot) =
        body(handlers_v2::read_session(State(state), axum::extract::Path(id)).await).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(snapshot["project"]["name"], expected["name"]);
    assert_eq!(snapshot["project"]["comment"], expected["comment"]);
    assert_eq!(snapshot["project"]["key"], expected["key"]);
}

#[tokio::test]
async fn project_controls_v31_reads_are_available_through_native_http() {
    let state = state("project-controls-v31-reads");
    let id = open_stored(&state, "project-controls-v31").await;
    let (_, snapshot) = body(
        handlers_v2::read_session(State(state.clone()), axum::extract::Path(id.clone())).await,
    )
    .await;
    let part_id = snapshot["project"]["tracks"][0]["parts"][0]["id"]
        .as_str()
        .expect("first part id")
        .to_string();

    let (status, descriptor) = body(
        handlers_v2::expression_descriptor(
            State(state.clone()),
            axum::extract::Path((id.clone(), "dyn".to_string())),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "descriptor: {descriptor}");
    assert_eq!(descriptor["revision"], snapshot["revision"]);
    assert_eq!(descriptor["descriptor"]["abbr"], "dyn");

    let (status, stroke) = body(
        handlers_v2::stroke_context(
            State(state),
            axum::extract::Path((id, part_id.clone())),
            axum::extract::Query(StrokeContextQuery {
                abbr: "dyn".to_string(),
                curve_id: None,
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "stroke context: {stroke}");
    assert_eq!(stroke["revision"], snapshot["revision"]);
    assert_eq!(stroke["part_id"], part_id);
    assert_eq!(stroke["abbr"], "dyn");
    assert!(stroke["points"].is_array());
    assert!(stroke["descriptor"]["min_value"].is_number());
}

#[tokio::test]
async fn pronunciation_compose_and_commit_match_direct_wasm_session() {
    let state = state("pronunciation-adapter-parity");
    let id = open_stored(&state, "pronunciation-adapter-parity").await;
    let (status, initial) = body(
        handlers_v2::read_session(State(state.clone()), axum::extract::Path(id.clone())).await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let project_id = initial["project"]["id"].as_str().unwrap().to_string();
    let part = &initial["project"]["tracks"][0]["parts"][0];
    let part_id = part["id"].as_str().unwrap().to_string();
    let note_id = part["notes"][0]["id"].as_str().unwrap().to_string();
    let (yaml, sidecar) = {
        let mut registry = state.sessions.lock().unwrap();
        let session = &mut registry.get_mut(&id).unwrap().session;
        (session.yaml(), session.save_sidecar().unwrap())
    };

    let request = json!({
        "project_id": project_id,
        "part_id": part_id,
        "note_id": note_id,
        "expected_revision": 0,
        "input": "city",
        "language": "en",
        "mode": "universal",
        "style": "cvvc",
        "limit": 8,
        "tone": 60
    });
    let (status, native_compose) = body(
        handlers_v2::phoneme_compose(
            State(state.clone()),
            axum::extract::Path(id.clone()),
            JsonBody(request.clone()),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let mut wasm = Engine::new();
    wasm.session_open_yaml(&yaml, &sidecar).unwrap();
    let wasm_compose: Value = serde_json::from_str(
        &wasm
            .session_phoneme_compose_v2(&request.to_string())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(native_compose, wasm_compose);
    let candidate = native_compose["result"]["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|candidate| candidate["strategy"] == "g2p_cvvc")
        .expect("demo G2P candidate");
    let pronunciation = json!({
        "language": candidate["language"],
        "mode": "universal",
        "style": candidate["style"],
        "strategy": candidate["strategy"],
        "language_phones": candidate["language_phones"],
        "universal_phones": candidate["universal_phones"]
    });
    let command_request = json!({
        "expected_revision": 0,
        "command": {
            "type": "note.set_pronunciation",
            "note_id": note_id,
            "pronunciation": pronunciation.clone()
        }
    });
    let (status, native_commit) = command(&state, &id, command_request.clone()).await;
    assert_eq!(status, StatusCode::OK);
    let wasm_commit: Value =
        serde_json::from_str(&wasm.session_apply_v2(&command_request.to_string()).unwrap())
            .unwrap();
    assert_eq!(native_commit["revision"], wasm_commit["revision"]);
    assert_eq!(native_commit["changes"], wasm_commit["changes"]);

    let (status, saved) = body(
        handlers_v2::session_save(
            State(state.clone()),
            axum::extract::Path(id.clone()),
            JsonBody(SaveRequest {
                name: "pronunciation-roundtrip".into(),
                expected_revision: Some(1),
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let pronunciation_sidecar = std::fs::read_to_string(
        state
            .projects
            .pronunciation_sidecar_path_for("pronunciation-roundtrip"),
    )
    .expect("pronunciation sidecar is persisted beside USTX");
    assert!(pronunciation_sidecar.contains("rusted-utau.pronunciations/1"));
    assert!(pronunciation_sidecar.contains("g2p_cvvc"));
    let reopened_id = open_stored(&state, "pronunciation-roundtrip").await;
    let (status, reopened) = body(
        handlers_v2::read_session(State(state.clone()), axum::extract::Path(reopened_id)).await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        reopened["project"]["tracks"][0]["parts"][0]["notes"][0]["pronunciation"],
        pronunciation
    );
}

#[tokio::test]
async fn shared_part_vector_matches_native_http_routes() {
    let vector: Value = serde_json::from_str(PART_ADAPTER_VECTOR).unwrap();
    let expected = &vector["expected"];
    let state = state("shared-part-vector");
    let id = open_stored(&state, "shared-part").await;
    let (part_id, revision) = {
        let registry = state.sessions.lock().unwrap();
        let session = &registry.get(&id).unwrap().session;
        (
            session.identity().tracks[0].parts[0].id.to_string(),
            session.revision(),
        )
    };
    let mut command_body = vector["command"].clone();
    command_body["part_id"] = Value::String(part_id);
    let (status, begin) = body(
        handlers_v2::group_begin(
            State(state.clone()),
            axum::extract::Path(id.clone()),
            JsonBody(GroupBeginRequest {
                expected_revision: Some(revision),
                name: Some("shared part vector".into()),
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let group_id = begin["group_id"].as_str().unwrap().to_string();
    let (status, child) = command(
        &state,
        &id,
        json!({"expected_revision":revision,"group_id":group_id,"command":command_body}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(child["changes"].as_array().unwrap().len(), 0);
    let (status, outcome) = body(
        handlers_v2::group_commit(
            State(state.clone()),
            axum::extract::Path((id.clone(), group_id)),
            JsonBody(GroupCommitRequest {
                expected_revision: Some(revision),
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
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
    let (status, snapshot) =
        body(handlers_v2::read_session(State(state), axum::extract::Path(id)).await).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        snapshot["project"]["tracks"][0]["parts"][0]["name"],
        expected["name"]
    );
    assert_eq!(
        snapshot["project"]["tracks"][0]["parts"][0]["comment"],
        expected["comment"]
    );
}

#[tokio::test]
async fn split_curve_boundary_is_preserved_by_native_http_adapter() {
    let state = state("split-curve-adapter");
    let (status, opened) = body(
        handlers_v2::create_session(
            State(state.clone()),
            JsonBody(CreateSessionRequest {
                source: CreateSource::ImportedUstx {
                    name: "split-curve".into(),
                    ustx: SPLIT_CURVE_FIXTURE.into(),
                },
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = opened["session_id"].as_str().unwrap().to_string();
    let part_id = opened["snapshot"]["project"]["tracks"][0]["parts"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let original =
        opened["snapshot"]["project"]["tracks"][0]["parts"][0]["curves"][0]["points"].clone();
    let (status, _) = command(
        &state,
        &id,
        json!({
            "expected_revision":0,"command":{"type":"part.split","part_id":part_id,"at_tick":960}
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, split) = body(
        handlers_v2::read_session(State(state.clone()), axum::extract::Path(id.clone())).await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let parts = split["project"]["tracks"][0]["parts"].as_array().unwrap();
    assert_eq!(
        parts[0]["curves"][0]["points"],
        json!([{"tick":0,"value":0},{"tick":960,"value":10}])
    );
    assert_eq!(
        parts[1]["curves"][0]["points"],
        json!([{"tick":0,"value":10},{"tick":960,"value":20}])
    );
    let (status, _) = body(
        handlers_v2::session_undo(
            State(state.clone()),
            axum::extract::Path(id.clone()),
            JsonBody(handlers_v2::HistoryRequest {
                expected_revision: Some(1),
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, undone) =
        body(handlers_v2::read_session(State(state), axum::extract::Path(id)).await).await;
    assert_eq!(
        undone["project"]["tracks"][0]["parts"][0]["curves"][0]["points"],
        original
    );
}

fn temp_root(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rusted-utau-v2-{}-{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp root");
    dir
}

fn state(tag: &str) -> Arc<AppState> {
    let root = temp_root(tag);
    Arc::new(AppState::new(root.clone(), root, Vec::new(), None))
}

async fn body(resp: Response) -> (StatusCode, Value) {
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

/// Open a session over the stored `demo-song` project and return its id.
async fn open_stored(state: &Arc<AppState>, name: &str) -> String {
    state
        .projects
        .save(name, DEMO.as_bytes())
        .expect("seed project");
    let (status, value) = body(
        handlers_v2::create_session(
            State(state.clone()),
            JsonBody(CreateSessionRequest {
                source: CreateSource::StoredProject {
                    name: name.to_string(),
                },
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "create failed: {value}");
    value["session_id"]
        .as_str()
        .expect("session id")
        .to_string()
}

async fn command(state: &Arc<AppState>, id: &str, request: Value) -> (StatusCode, Value) {
    body(
        handlers_v2::session_commands(
            State(state.clone()),
            axum::extract::Path(id.to_string()),
            JsonBody(request),
        )
        .await,
    )
    .await
}

#[tokio::test]
async fn shared_project_part_invalid_and_noop_vectors_are_atomic_over_native_http() {
    let vectors: Value = serde_json::from_str(EDGE_VECTORS).unwrap();
    for (index, case) in vectors["cases"].as_array().unwrap().iter().enumerate() {
        let state = state(&format!("project-part-edge-{index}"));
        let id = open_stored(&state, &format!("edge-project-{index}")).await;
        let (_, before) = body(
            handlers_v2::read_session(State(state.clone()), axum::extract::Path(id.clone())).await,
        )
        .await;
        let mut command_body = case["command"].clone();
        if command_body["track_id"] == "@track:0" {
            command_body["track_id"] = before["project"]["tracks"][0]["id"].clone();
        }
        if command_body["part_id"] == "@part:0" {
            command_body["part_id"] = before["project"]["tracks"][0]["parts"][0]["id"].clone();
        }
        let (status, result) = command(
            &state,
            &id,
            json!({"expected_revision":0,"command":command_body}),
        )
        .await;
        let expected = &case["expected"];
        if let Some(code) = expected["code"].as_str() {
            assert_eq!(result["error"]["code"], code, "{}", case["name"]);
            assert_ne!(status, StatusCode::OK, "{}", case["name"]);
        } else {
            assert_eq!(status, StatusCode::OK, "{}", case["name"]);
            assert_eq!(result["changed"], expected["changed"], "{}", case["name"]);
        }
        let (_, after) =
            body(handlers_v2::read_session(State(state.clone()), axum::extract::Path(id)).await)
                .await;
        assert_eq!(after["revision"], expected["revision"], "{}", case["name"]);
        assert_eq!(after["project"], before["project"], "{}", case["name"]);
        assert_eq!(after["can_undo"], before["can_undo"], "{}", case["name"]);
        assert_eq!(after["can_redo"], before["can_redo"], "{}", case["name"]);
    }
}

#[tokio::test]
async fn capabilities_report_the_session_contract_and_host_facts() {
    let state = state("caps");
    let (status, value) = body(handlers_v2::capabilities(State(state.clone())).await).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["contract_version"], "2-draft3");
    assert_eq!(value["session_idle_ttl_seconds"], 30 * 60);
    assert_eq!(value["limits"]["max_stroke_points"], 20_000);
    assert!(value["features"]
        .as_array()
        .unwrap()
        .iter()
        .any(|feature| feature == "project.controls.v1"));
    assert_eq!(
        value["engines"],
        json!([{"id": "r1-rs", "available": true}]),
        "without --so only the pure-Rust engine may be advertised"
    );
    assert_eq!(value["render"]["async_jobs"], true);
    assert_eq!(value["render"]["per_job_cancel"], true);
    assert_eq!(value["render"]["progress_transports"], json!(["poll"]));
}

#[tokio::test]
async fn a_stored_project_opens_with_a_snapshot_and_a_revision() {
    let state = state("open");
    let id = open_stored(&state, "scale-demo").await;

    let (status, value) = body(
        handlers_v2::read_session(State(state.clone()), axum::extract::Path(id.clone())).await,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["revision"], 0);
    assert_eq!(value["ids_stable"], false, "no sidecar → ids regenerated");
    assert_eq!(
        value["project"]["tracks"][0]["parts"][0]["notes"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(state.sessions.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_command_moves_the_revision_and_a_stale_one_conflicts() {
    let state = state("commands");
    let id = open_stored(&state, "scale-demo").await;
    let note_id = {
        let reg = state.sessions.lock().unwrap();
        reg.get(&id)
            .unwrap()
            .session
            .identity()
            .note_id(0, 0, 2)
            .unwrap()
            .as_str()
            .to_string()
    };

    let (status, value) = command(
        &state,
        &id,
        json!({"expected_revision":0,
               "command":{"type":"note.set_lyric","note_id":note_id,"lyric":"ka"}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(value["revision"], 1);
    assert_eq!(value["changed"], true);
    assert!(value["changes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["kind"] == "note.changed"));

    // Same (now stale) revision again → 409 with the frozen code.
    let (status, value) = command(
        &state,
        &id,
        json!({"expected_revision":0,
               "command":{"type":"note.set_lyric","note_id":note_id,"lyric":"ko"}}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(value["error"]["code"], "revision_conflict");
    assert_eq!(value["error"]["details"]["current_revision"], 1);

    // A rejected command leaves the session at revision 1.
    let (_, value) = body(
        handlers_v2::read_session(State(state.clone()), axum::extract::Path(id.clone())).await,
    )
    .await;
    assert_eq!(value["revision"], 1);
}

#[tokio::test]
async fn unknown_ids_and_unsupported_families_map_to_their_status() {
    let state = state("errors");
    let id = open_stored(&state, "scale-demo").await;

    let (status, value) = command(
        &state,
        &id,
        json!({"expected_revision":0,"command":{"type":"note.set_tone","note_id":"ghost","tone":60}}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(value["error"]["code"], "not_found");

    let (status, value) = command(
        &state,
        &id,
        json!({"expected_revision":0,"command":{"type":"part.add"}}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(value["error"]["code"], "unsupported_operation");

    // Unknown session → 404, never a 500.
    let (status, value) = command(
        &state,
        "no-such-session",
        json!({"expected_revision":0,"command":{}}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(value["error"]["code"], "not_found");
}

#[tokio::test]
async fn a_group_commits_as_one_revision_increment() {
    let state = state("groups");
    let id = open_stored(&state, "scale-demo").await;
    let note_id = {
        let reg = state.sessions.lock().unwrap();
        reg.get(&id)
            .unwrap()
            .session
            .identity()
            .note_id(0, 0, 1)
            .unwrap()
            .as_str()
            .to_string()
    };

    let (status, value) = body(
        handlers_v2::group_begin(
            State(state.clone()),
            axum::extract::Path(id.clone()),
            JsonBody(GroupBeginRequest {
                expected_revision: Some(0),
                name: Some("Drag".into()),
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    let group_id = value["group_id"].as_str().expect("group id").to_string();
    assert_eq!(value["revision"], 0);

    // Children ride the normal command route with `group_id`.
    for tone in [70, 71, 72] {
        let (status, value) = command(
            &state,
            &id,
            json!({"expected_revision":0,"group_id":group_id,
                   "command":{"type":"note.set_tone","note_id":note_id,"tone":tone}}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_eq!(
            value["revision"], 0,
            "children never move the public revision"
        );
        assert!(value["changes"].as_array().unwrap().is_empty());
    }

    // An unrelated mutation while the group is open is rejected.
    let (status, value) = command(
        &state,
        &id,
        json!({"expected_revision":0,"command":{"type":"note.set_tone","note_id":note_id,"tone":60}}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(value["error"]["code"], "session_busy");

    let (status, value) = body(
        handlers_v2::group_commit(
            State(state.clone()),
            axum::extract::Path((id.clone(), group_id.clone())),
            JsonBody(GroupCommitRequest {
                expected_revision: Some(0),
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(value["revision"], 1);
    assert!(value["changes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["kind"] == "note.changed"));
}

#[tokio::test]
async fn saving_writes_both_files_and_does_not_move_the_revision() {
    let state = state("save");
    let id = open_stored(&state, "scale-demo").await;
    let note_id = {
        let reg = state.sessions.lock().unwrap();
        reg.get(&id)
            .unwrap()
            .session
            .identity()
            .note_id(0, 0, 0)
            .unwrap()
            .as_str()
            .to_string()
    };
    command(
        &state,
        &id,
        json!({"expected_revision":0,
               "command":{"type":"note.set_lyric","note_id":note_id,"lyric":"ka"}}),
    )
    .await;

    let (status, value) = body(
        handlers_v2::session_save(
            State(state.clone()),
            axum::extract::Path(id.clone()),
            JsonBody(SaveRequest {
                name: "saved-song".to_string(),
                expected_revision: Some(1),
            }),
        )
        .await,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(value["saved"], true);
    assert_eq!(value["revision"], 1, "save is not a document change");
    assert_eq!(value["sidecar_written"], true);

    let project = state.projects.load("saved-song").expect("project written");
    assert!(String::from_utf8_lossy(&project).contains("ka"));
    let sidecar = std::fs::read_to_string(state.projects.sidecar_path_for("saved-song"))
        .expect("sidecar written");
    assert!(sidecar.contains("rusted-utau.ids/1"));
    assert!(
        !String::from_utf8_lossy(&project).contains("rusted-utau"),
        "identity never leaks into the .ustx"
    );
}

#[tokio::test]
async fn exporting_returns_portable_ustx_without_sidecar_or_revision_change() {
    let state = state("export");
    let id = open_stored(&state, "export-demo").await;
    let revision_before = state
        .sessions
        .lock()
        .unwrap()
        .get(&id)
        .unwrap()
        .session
        .revision();

    let (status, exported) = body(
        handlers_v2::session_export(State(state.clone()), axum::extract::Path(id.clone())).await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{exported}");
    assert_eq!(exported["revision"], revision_before);
    assert_eq!(exported["sidecar_included"], false);
    let yaml = exported["ustx"].as_str().expect("portable USTX");
    assert!(!yaml.contains("rusted-utau.ids"));
    assert!(!yaml.contains("rusted-utau"), "identity must not leak into export");
}

#[tokio::test]
async fn the_legacy_project_endpoint_still_writes_no_sidecar() {
    // Backward compatibility (decision 2026-09-29): only the v2 save path
    // touches the identity sidecar.
    let state = state("legacy");
    let req = synth_server::handlers::ProjectSaveRequest {
        name: "legacy-song".to_string(),
        project: json!({
            "name": "Legacy",
            "bpm": 110,
            "time_signature": "4/4",
            "tracks": [{"name": "Vocal", "color": "#7358E9",
                        "notes": [{"lyric": "la", "pitch": 0, "position": 0.0, "duration": 1.0}]}]
        }),
    };
    let (status, _) = body(
        synth_server::handlers::project_save(State(state.clone()), JsonBody(req))
            .await
            .into_response(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(state.projects.load("legacy-song").is_ok());
    assert!(
        !state.projects.sidecar_path_for("legacy-song").exists(),
        "the legacy path must not create an identity sidecar"
    );
}

#[tokio::test]
async fn closing_a_session_makes_further_calls_not_found() {
    let state = state("close");
    let id = open_stored(&state, "scale-demo").await;

    let (status, value) = body(
        handlers_v2::close_session(State(state.clone()), axum::extract::Path(id.clone())).await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["closed"], true);

    let (status, value) =
        body(handlers_v2::read_session(State(state.clone()), axum::extract::Path(id)).await).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(value["error"]["code"], "not_found");
}

#[tokio::test]
async fn sessions_are_isolated_from_each_other() {
    let state = state("isolated");
    let first = open_stored(&state, "scale-demo").await;
    let second = open_stored(&state, "scale-demo").await;
    assert_ne!(first, second);

    let note_id = {
        let reg = state.sessions.lock().unwrap();
        reg.get(&first)
            .unwrap()
            .session
            .identity()
            .note_id(0, 0, 0)
            .unwrap()
            .as_str()
            .to_string()
    };
    command(
        &state,
        &first,
        json!({"expected_revision":0,
               "command":{"type":"note.set_lyric","note_id":note_id,"lyric":"only-first"}}),
    )
    .await;

    let (_, value) =
        body(handlers_v2::read_session(State(state.clone()), axum::extract::Path(second)).await)
            .await;
    assert_eq!(value["revision"], 0, "the second session must be untouched");
    let notes = value["project"]["tracks"][0]["parts"][0]["notes"]
        .as_array()
        .unwrap();
    assert_ne!(notes[0]["lyric"], "only-first");
}

#[tokio::test]
async fn an_idle_session_expires_and_is_reported_not_found() {
    // Short TTL so the sweep closes it without a long test: the session is
    // created and read normally, then left idle past the deadline (§6.12).
    let root = temp_root("ttl");
    let state = Arc::new(
        AppState::new(root.clone(), root, Vec::new(), None)
            .with_session_ttl(Duration::from_millis(300)),
    );
    let id = open_stored(&state, "scale-demo").await;
    tokio::time::sleep(Duration::from_millis(400)).await;

    let (status, value) =
        body(handlers_v2::read_session(State(state.clone()), axum::extract::Path(id)).await).await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{value}");
    assert!(
        state.sessions.lock().unwrap().is_empty(),
        "the session was closed"
    );
}

#[tokio::test]
async fn a_session_with_an_active_render_job_is_exempt_from_idle_ttl() {
    let root = temp_root("render-ttl-exemption");
    let state = Arc::new(
        AppState::new(root.clone(), root, Vec::new(), None)
            .with_session_ttl(Duration::from_millis(300)),
    );
    let id = open_stored(&state, "render-ttl-demo").await;
    state
        .sessions
        .lock()
        .unwrap()
        .get_mut(&id)
        .unwrap()
        .begin_render_job();
    tokio::time::sleep(Duration::from_millis(400)).await;
    let (status, _) = body(
        handlers_v2::read_session(State(state.clone()), axum::extract::Path(id.clone())).await,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "active render must keep the session alive"
    );
    state
        .sessions
        .lock()
        .unwrap()
        .get_mut(&id)
        .unwrap()
        .end_render_job();
    tokio::time::sleep(Duration::from_millis(400)).await;
    let (status, _) =
        body(handlers_v2::read_session(State(state.clone()), axum::extract::Path(id)).await).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "session expires after the render slot closes"
    );
}

#[tokio::test]
async fn bounded_reads_are_revision_tagged_over_http() {
    let state = state("reads");
    let id = open_stored(&state, "scale-demo").await;
    let (part_id, note_id) = {
        let reg = state.sessions.lock().unwrap();
        let session = &reg.get(&id).unwrap().session;
        let table = session.identity();
        (
            table.tracks[0].parts[0].id.as_str().to_string(),
            table.tracks[0].parts[0].notes[0].id.as_str().to_string(),
        )
    };
    command(
        &state,
        &id,
        json!({"expected_revision":0,
               "command":{"type":"note.set_lyric","note_id":note_id,"lyric":"ka"}}),
    )
    .await;

    let (status, value) = body(
        handlers_v2::read_project(State(state.clone()), axum::extract::Path(id.clone())).await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["revision"], 1);
    assert!(value["project"]["id"].as_str().is_some());

    let (status, value) = body(
        handlers_v2::read_part(
            State(state.clone()),
            axum::extract::Path((id.clone(), part_id.clone())),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["revision"], 1);
    assert_eq!(value["part_id"], part_id);
    assert_eq!(value["part"]["id"], part_id);

    let (status, value) = body(
        handlers_v2::part_notes(
            State(state.clone()),
            axum::extract::Path((id.clone(), part_id.clone())),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["revision"], 1);
    assert_eq!(value["notes"].as_array().unwrap().len(), 4);

    let (status, value) =
        body(handlers_v2::tracks(State(state.clone()), axum::extract::Path(id)).await).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["revision"], 1);
    assert_eq!(value["tracks"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn native_server_and_wasm_share_stable_id_group_history_and_reopen_semantics() {
    let state = state("native-wasm-parity");
    let id = open_stored(&state, "scale-demo").await;
    let mut wasm = Engine::new();
    let wasm_open: Value =
        serde_json::from_str(&wasm.session_open_yaml(DEMO, "").expect("WASM open")).unwrap();
    assert_eq!(wasm_open["revision"], 0);
    assert_eq!(wasm_open["ids_stable"], false);

    let native_note_id = {
        let reg = state.sessions.lock().unwrap();
        reg.get(&id)
            .unwrap()
            .session
            .identity()
            .note_id(0, 0, 0)
            .unwrap()
            .as_str()
            .to_string()
    };
    let wasm_note_id = wasm_open["project"]["tracks"][0]["parts"][0]["notes"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let native_edit = command(
        &state,
        &id,
        json!({"expected_revision":0,"command":{"type":"note.set_lyric","note_id":native_note_id,"lyric":"ka"}}),
    )
    .await;
    let wasm_edit: Value = serde_json::from_str(
        &wasm
            .session_apply_v2(
                &json!({
                    "expected_revision":0,
                    "command":{"type":"note.set_lyric","note_id":wasm_note_id,"lyric":"ka"}
                })
                .to_string(),
            )
            .expect("WASM note command"),
    )
    .unwrap();
    assert_eq!(native_edit.0, StatusCode::OK);
    assert_eq!(native_edit.1["revision"], wasm_edit["revision"]);
    assert_eq!(native_edit.1["revision"], 1);

    let (native_status, native_begin) = body(
        handlers_v2::group_begin(
            State(state.clone()),
            axum::extract::Path(id.clone()),
            JsonBody(GroupBeginRequest {
                expected_revision: Some(1),
                name: Some("Parity gesture".to_string()),
            }),
        )
        .await,
    )
    .await;
    let wasm_begin: Value = serde_json::from_str(
        &wasm
            .session_group_begin(1, "Parity gesture")
            .expect("WASM group begin"),
    )
    .unwrap();
    assert_eq!(native_status, StatusCode::OK);
    assert_eq!(native_begin["base_revision"], wasm_begin["base_revision"]);
    let native_group_id = native_begin["group_id"].as_str().unwrap();
    let wasm_group_id = wasm_begin["group_id"].as_str().unwrap();

    let native_group_edit = command(
        &state,
        &id,
        json!({"expected_revision":1,"group_id":native_group_id,
               "command":{"type":"note.set_tone","note_id":native_note_id,"tone":62}}),
    )
    .await;
    let wasm_group_edit: Value = serde_json::from_str(
        &wasm
            .session_apply_v2(
                &json!({
                    "expected_revision":1,"group_id":wasm_group_id,
                    "command":{"type":"note.set_tone","note_id":wasm_note_id,"tone":62}
                })
                .to_string(),
            )
            .expect("WASM grouped command"),
    )
    .unwrap();
    assert_eq!(native_group_edit.0, StatusCode::OK);
    assert_eq!(native_group_edit.1["revision"], wasm_group_edit["revision"]);
    assert_eq!(
        native_group_edit.1["revision"], 1,
        "group children stay tentative"
    );

    let (native_status, native_commit) = body(
        handlers_v2::group_commit(
            State(state.clone()),
            axum::extract::Path((id.clone(), native_group_id.to_string())),
            JsonBody(GroupCommitRequest {
                expected_revision: Some(1),
            }),
        )
        .await,
    )
    .await;
    let wasm_commit: Value = serde_json::from_str(
        &wasm
            .session_group_commit(wasm_group_id, 1)
            .expect("WASM group commit"),
    )
    .unwrap();
    assert_eq!(native_status, StatusCode::OK);
    assert_eq!(native_commit["revision"], wasm_commit["revision"]);
    assert_eq!(native_commit["revision"], 2);

    let (stale_undo_status, stale_undo) = body(
        handlers_v2::session_undo(
            State(state.clone()),
            axum::extract::Path(id.clone()),
            JsonBody(handlers_v2::HistoryRequest {
                expected_revision: Some(1),
            }),
        )
        .await,
    )
    .await;
    assert_eq!(stale_undo_status, StatusCode::CONFLICT);
    assert_eq!(stale_undo["error"]["code"], "revision_conflict");
    assert_eq!(stale_undo["revision"], 2);

    let (native_undo_status, native_undo) = body(
        handlers_v2::session_undo(
            State(state.clone()),
            axum::extract::Path(id.clone()),
            JsonBody(handlers_v2::HistoryRequest {
                expected_revision: Some(2),
            }),
        )
        .await,
    )
    .await;
    let wasm_undo: Value =
        serde_json::from_str(&wasm.session_undo_v2(2).expect("WASM undo")).unwrap();
    assert_eq!(native_undo_status, StatusCode::OK);
    assert_eq!(native_undo["revision"], wasm_undo["revision"]);
    assert_eq!(native_undo["revision"], 3);
    let (pre_save_status, pre_save_snapshot) = body(
        handlers_v2::read_session(State(state.clone()), axum::extract::Path(id.clone())).await,
    )
    .await;
    assert_eq!(pre_save_status, StatusCode::OK);
    assert_eq!(
        pre_save_snapshot["project"]["tracks"][0]["parts"][0]["notes"][0]["lyric"], "ka",
        "undo of the group should retain the earlier lyric edit"
    );

    // Save each host's canonical YAML + sidecar, reopen, and verify its IDs
    // are stable and still address the same logical note.
    let (stale_save_status, stale_save) = body(
        handlers_v2::session_save(
            State(state.clone()),
            axum::extract::Path(id.clone()),
            JsonBody(SaveRequest {
                name: "parity-save".to_string(),
                expected_revision: Some(2),
            }),
        )
        .await,
    )
    .await;
    assert_eq!(stale_save_status, StatusCode::CONFLICT);
    assert_eq!(stale_save["error"]["code"], "revision_conflict");
    assert_eq!(stale_save["revision"], 3);

    let (save_status, save_value) = body(
        handlers_v2::session_save(
            State(state.clone()),
            axum::extract::Path(id),
            JsonBody(SaveRequest {
                name: "parity-save".to_string(),
                expected_revision: Some(3),
            }),
        )
        .await,
    )
    .await;
    assert_eq!(save_status, StatusCode::OK, "{save_value}");
    let wasm_yaml = wasm.project_yaml();
    let wasm_sidecar = wasm.session_save_sidecar().expect("WASM sidecar");
    let mut wasm_reopened = Engine::new();
    let wasm_reopened_snapshot: Value = serde_json::from_str(
        &wasm_reopened
            .session_open_yaml(&wasm_yaml, &wasm_sidecar)
            .expect("WASM reopen"),
    )
    .unwrap();
    assert_eq!(wasm_reopened_snapshot["ids_stable"], true);
    assert_eq!(
        wasm_reopened_snapshot["project"]["tracks"][0]["parts"][0]["notes"][0]["id"],
        wasm_note_id,
    );

    let (open_status, open_value) = body(
        handlers_v2::create_session(
            State(state.clone()),
            JsonBody(CreateSessionRequest {
                source: CreateSource::StoredProject {
                    name: "parity-save".to_string(),
                },
            }),
        )
        .await,
    )
    .await;
    assert_eq!(open_status, StatusCode::CREATED, "{open_value}");
    let native_reopened = open_value["session_id"].as_str().unwrap().to_string();
    let (native_read_status, native_snapshot) = body(
        handlers_v2::read_session(State(state.clone()), axum::extract::Path(native_reopened)).await,
    )
    .await;
    assert_eq!(native_read_status, StatusCode::OK);
    assert_eq!(native_snapshot["ids_stable"], true);
    assert_eq!(
        native_snapshot["project"]["tracks"][0]["parts"][0]["notes"][0]["id"],
        native_note_id
    );
    assert_eq!(
        native_snapshot["project"]["tracks"][0]["parts"][0]["notes"][0]["lyric"],
        "ka"
    );
    assert_eq!(
        wasm_reopened_snapshot["project"]["tracks"][0]["parts"][0]["notes"][0]["lyric"],
        "ka"
    );
}

#[tokio::test]
async fn native_http_and_direct_wasm_share_curve_commit_simplify_and_undo() {
    const CURVE_USTX: &str = r#"ustx_version: 0.9
name: Curve Parity
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
    name: dynamics
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
    let state = state("curve-adapter-parity");
    let (status, opened) = body(
        handlers_v2::create_session(
            State(state.clone()),
            JsonBody(CreateSessionRequest {
                source: CreateSource::ImportedUstx {
                    name: "curve-parity".into(),
                    ustx: CURVE_USTX.into(),
                },
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{opened}");
    let id = opened["session_id"].as_str().unwrap().to_string();
    let mut wasm = Engine::new();
    let wasm_open: Value = serde_json::from_str(
        &wasm
            .session_open_yaml(CURVE_USTX, "")
            .expect("WASM open curve fixture"),
    )
    .unwrap();
    let (native_part, native_curve) = {
        let registry = state.sessions.lock().unwrap();
        let table = registry.get(&id).unwrap().session.identity();
        let part = table.tracks[0].parts[0].id.as_str().to_string();
        let curve = table.tracks[0].parts[0].curves[0].id.as_str().to_string();
        (part, curve)
    };
    let wasm_part = wasm_open["project"]["tracks"][0]["parts"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let wasm_curve = wasm_open["project"]["tracks"][0]["parts"][0]["curves"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let command_value = |part: String, curve: String| {
        json!({
            "expected_revision": 0,
            "command": {"type":"curve.apply_stroke", "part_id":part, "curve_id":curve,
                "mode":"replace", "start_tick":0, "end_tick":960,
                "points":[{"tick":240,"value":5},{"tick":480,"value":10},
                          {"tick":720,"value":5}]}
        })
    };
    let (native_status, native_edit) = command(
        &state,
        &id,
        command_value(native_part.clone(), native_curve.clone()),
    )
    .await;
    let wasm_edit: Value = serde_json::from_str(
        &wasm
            .session_apply_v2(&command_value(wasm_part, wasm_curve).to_string())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(native_status, StatusCode::OK);
    assert_eq!(native_edit["revision"], wasm_edit["revision"]);
    assert_eq!(
        native_edit["changes"][0]["data"]["points"],
        wasm_edit["changes"][0]["data"]["points"]
    );
    assert_eq!(
        native_edit["changes"][0]["data"]["points"],
        json!([
            {"tick":0,"value":0},
            {"tick":480,"value":10},
            {"tick":960,"value":0}
        ])
    );

    let (native_undo_status, native_undo) = body(
        handlers_v2::session_undo(
            State(state.clone()),
            axum::extract::Path(id.clone()),
            JsonBody(handlers_v2::HistoryRequest {
                expected_revision: Some(1),
            }),
        )
        .await,
    )
    .await;
    let wasm_undo: Value = serde_json::from_str(&wasm.session_undo_v2(1).unwrap()).unwrap();
    assert_eq!(native_undo_status, StatusCode::OK);
    assert_eq!(native_undo["revision"], wasm_undo["revision"]);
    let native_points = {
        let registry = state.sessions.lock().unwrap();
        registry
            .get(&id)
            .unwrap()
            .session
            .curve_json(&native_part, &native_curve)
            .unwrap()
    };
    let wasm_points = wasm.session_snapshot_json().unwrap();
    let native_value: Value = serde_json::from_str(&native_points).unwrap();
    let wasm_value: Value = serde_json::from_str(&wasm_points).unwrap();
    assert_eq!(
        native_value["points"],
        wasm_value["project"]["tracks"][0]["parts"][0]["curves"][0]["points"]
    );
}

#[tokio::test]
async fn a_new_session_uses_the_requested_name_and_bpm_at_revision_zero() {
    let state = state("new-session");
    let (status, value) = body(
        handlers_v2::create_session(
            State(state.clone()),
            JsonBody(CreateSessionRequest {
                source: CreateSource::New {
                    name: Some("Contract New".to_string()),
                    bpm: Some(123.0),
                },
            }),
        )
        .await,
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{value}");
    assert_eq!(value["revision"], 0);
    assert_eq!(value["snapshot"]["project"]["name"], "Contract New");
    assert_eq!(value["snapshot"]["project"]["bpm"], 123.0);

    let mut wasm = Engine::new();
    let wasm_value: Value = serde_json::from_str(
        &wasm
            .session_new_json(r#"{"name":"Contract New","bpm":123.0}"#)
            .expect("WASM new session"),
    )
    .unwrap();
    assert_eq!(wasm_value["revision"], value["revision"]);
    assert_eq!(
        wasm_value["project"]["name"],
        value["snapshot"]["project"]["name"]
    );
    assert_eq!(
        wasm_value["project"]["bpm"],
        value["snapshot"]["project"]["bpm"]
    );
}

#[tokio::test]
async fn a_missing_curve_uses_the_frozen_part_scoped_route_and_error_envelope() {
    let state = state("curve-route");
    let id = open_stored(&state, "scale-demo").await;
    let part_id = {
        let reg = state.sessions.lock().unwrap();
        reg.get(&id).unwrap().session.identity().tracks[0].parts[0]
            .id
            .as_str()
            .to_string()
    };
    let (status, value) = body(
        handlers_v2::read_curve(
            State(state.clone()),
            axum::extract::Path((id, part_id, "missing-curve".to_string())),
        )
        .await,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{value}");
    assert_eq!(value["error"]["code"], "not_found");
    assert_eq!(value["revision"], 0);
}

#[tokio::test]
async fn missing_expected_revision_is_a_revision_tagged_validation_error() {
    let state = state("required-revision");
    let id = open_stored(&state, "scale-demo").await;

    let (undo_status, undo_error) = body(
        handlers_v2::session_undo(
            State(state.clone()),
            axum::extract::Path(id.clone()),
            JsonBody(handlers_v2::HistoryRequest {
                expected_revision: None,
            }),
        )
        .await,
    )
    .await;
    assert_eq!(undo_status, StatusCode::BAD_REQUEST);
    assert_eq!(undo_error["error"]["code"], "validation_error");
    assert_eq!(undo_error["error"]["details"]["field"], "expected_revision");
    assert_eq!(undo_error["revision"], 0);

    let (save_status, save_error) = body(
        handlers_v2::session_save(
            State(state.clone()),
            axum::extract::Path(id.clone()),
            JsonBody(SaveRequest {
                name: "missing-revision".to_string(),
                expected_revision: None,
            }),
        )
        .await,
    )
    .await;
    assert_eq!(save_status, StatusCode::BAD_REQUEST);
    assert_eq!(save_error["error"]["code"], "validation_error");
    assert_eq!(save_error["revision"], 0);

    let (render_status, render_error) = body(
        handlers_v2::render_start(
            State(state.clone()),
            axum::extract::Path(id.clone()),
            JsonBody(RenderStartRequest {
                expected_revision: None,
                voicebank: "missing-bank".to_string(),
                track: Some(0),
                engine: None,
                engines: None,
            }),
        )
        .await,
    )
    .await;
    assert_eq!(render_status, StatusCode::BAD_REQUEST);
    assert_eq!(render_error["error"]["code"], "validation_error");
    assert_eq!(render_error["revision"], 0);

    let (begin_status, begin) = body(
        handlers_v2::group_begin(
            State(state.clone()),
            axum::extract::Path(id.clone()),
            JsonBody(GroupBeginRequest {
                expected_revision: Some(0),
                name: Some("cancel revision".to_string()),
            }),
        )
        .await,
    )
    .await;
    assert_eq!(begin_status, StatusCode::OK);
    let group_id = begin["group_id"].as_str().unwrap().to_string();
    let (stale_cancel_status, stale_cancel) = body(
        handlers_v2::group_cancel(
            State(state.clone()),
            axum::extract::Path((id.clone(), group_id.clone())),
            axum::body::Bytes::from_static(br#"{"expected_revision":1}"#),
        )
        .await,
    )
    .await;
    assert_eq!(stale_cancel_status, StatusCode::CONFLICT);
    assert_eq!(stale_cancel["error"]["code"], "revision_conflict");
    assert_eq!(stale_cancel["revision"], 0);
    let (cancel_status, cancel) = body(
        handlers_v2::group_cancel(
            State(state.clone()),
            axum::extract::Path((id, group_id)),
            axum::body::Bytes::from_static(br#"{"expected_revision":0}"#),
        )
        .await,
    )
    .await;
    assert_eq!(cancel_status, StatusCode::OK);
    assert_eq!(cancel["changed"], false);
    assert_eq!(cancel["revision"], 0);
}

#[tokio::test]
async fn an_imported_ustx_session_starts_fresh() {
    let state = state("import");
    let (status, value) = body(
        handlers_v2::create_session(
            State(state.clone()),
            JsonBody(CreateSessionRequest {
                source: CreateSource::ImportedUstx {
                    name: "from-openutau".to_string(),
                    ustx: DEMO.to_string(),
                },
            }),
        )
        .await,
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{value}");
    assert_eq!(
        value["ids_stable"], false,
        "an import is a new identity domain"
    );
    assert_eq!(value["revision"], 0);
    assert!(
        !value["snapshot"]["project"]["id"]
            .as_str()
            .unwrap()
            .is_empty(),
        "the snapshot must carry a project id"
    );
}

#[tokio::test]
async fn render_start_checks_revision_before_preflight() {
    let state = state("render-revision-order");
    let id = open_stored(&state, "scale-demo").await;
    let (status, value) = body(
        handlers_v2::render_start(
            State(state.clone()),
            axum::extract::Path(id),
            JsonBody(RenderStartRequest {
                expected_revision: Some(99),
                voicebank: "does-not-exist".to_string(),
                track: Some(0),
                engine: None,
                engines: None,
            }),
        )
        .await,
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT, "{value}");
    assert_eq!(value["error"]["code"], "revision_conflict");
    assert_eq!(value["revision"], 0);
}

#[tokio::test]
async fn render_jobs_are_polled_cancelled_and_kept_isolated_by_id() {
    let state = state("render-poll");
    let first = state.render_jobs.begin("session-a", 4, "r1", 0);
    let second = state.render_jobs.begin("session-a", 5, "r1", 0);

    let (status, value) = body(
        handlers_v2::render_status(
            State(state.clone()),
            axum::extract::Path(first.render_id.clone()),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["session_id"], "session-a");
    assert_eq!(value["revision"], 4);
    assert_eq!(value["status"], "queued");

    let (status, value) = body(
        handlers_v2::render_cancel(
            State(state.clone()),
            axum::extract::Path(first.render_id.clone()),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["status"], "cancelled");
    assert_eq!(value["cancelled"], true);
    assert_eq!(
        state.render_jobs.get(&second.render_id).unwrap().status,
        synth_server::render_jobs::JobStatus::Queued,
        "cancelling one ID must not affect another job"
    );

    // Terminal cancel is deterministic and cannot overwrite the outcome.
    let (status, value) = body(
        handlers_v2::render_cancel(
            State(state.clone()),
            axum::extract::Path(first.render_id.clone()),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["status"], "cancelled");
    assert_eq!(value["cancelled"], true);

    let (status, value) = body(
        handlers_v2::render_status(State(state.clone()), axum::extract::Path("absent".into()))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(value["error"]["code"], "not_found");
}

#[tokio::test]
async fn completed_render_audio_is_revision_tagged_and_cancelled_audio_is_unavailable() {
    let state = state("render-audio");
    let done = state.render_jobs.begin("session-a", 12, "r1", 0);
    assert!(state
        .render_jobs
        .finish_ok(&done.render_id, vec![0.0, 0.25, -0.25], 1));

    let response = handlers_v2::render_audio(
        State(state.clone()),
        axum::extract::Path(done.render_id.clone()),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "audio/wav");
    assert_eq!(response.headers()["x-rusted-utau-render-revision"], "12");

    let cancelled = state.render_jobs.begin("session-a", 13, "r1", 0);
    state.render_jobs.cancel(&cancelled.render_id);
    let (status, value) = body(
        handlers_v2::render_audio(
            State(state.clone()),
            axum::extract::Path(cancelled.render_id.clone()),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(value["error"]["code"], "not_found");
    assert_eq!(value["error"]["details"]["status"], "cancelled");
}

#[tokio::test]
async fn closing_a_session_cancels_its_active_render_jobs() {
    let state = state("close-render");
    let id = open_stored(&state, "scale-demo").await;
    let job = state.render_jobs.begin(&id, 0, "r1", 0);
    {
        let mut registry = state.sessions.lock().unwrap();
        registry.get_mut(&id).unwrap().begin_render_job();
    }

    let (status, value) = body(
        handlers_v2::close_session(State(state.clone()), axum::extract::Path(id.clone())).await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["closed"], true);
    assert_eq!(value["cancelled_render_jobs"][0], job.render_id);
    assert!(job.cancel.load(std::sync::atomic::Ordering::Relaxed));
    assert_eq!(
        state.render_jobs.get(&job.render_id).unwrap().status,
        synth_server::render_jobs::JobStatus::Cancelled
    );
}

#[tokio::test]
async fn shared_adapter_vector_matches_native_http_routes() {
    let vector: Value = serde_json::from_str(SHARED_ADAPTER_VECTOR).expect("shared vector JSON");
    let state = state("shared-adapter-vector");
    let (_, capabilities) = body(handlers_v2::capabilities(State(state.clone())).await).await;
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
    let id = open_stored(&state, "shared-vector-demo").await;
    let (status, initial) = body(
        handlers_v2::read_session(State(state.clone()), axum::extract::Path(id.clone())).await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let note_id = initial["project"]["tracks"][0]["parts"][0]["notes"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let track_id = initial["project"]["tracks"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let mut group_id: Option<String> = None;
    for step in vector["steps"].as_array().unwrap() {
        let expected = &step["expected"];
        match step["op"].as_str().unwrap() {
            "set_lyric" | "stale_set_lyric" => {
                let (status, outcome) = command(
                    &state,
                    &id,
                    json!({
                        "expected_revision": 0,
                        "command":{"type":"note.set_lyric","note_id":note_id,"lyric":step["lyric"]}
                    }),
                )
                .await;
                if let Some(code) = expected.get("code").and_then(Value::as_str) {
                    assert_eq!(status, StatusCode::CONFLICT);
                    assert_eq!(outcome["error"]["code"], code);
                    assert_eq!(outcome["revision"], expected["revision"]);
                } else {
                    assert_eq!(status, StatusCode::OK);
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
            "undo" | "redo" => {
                let expected_revision = expected["revision"].as_u64().unwrap() - 1;
                let request = JsonBody(handlers_v2::HistoryRequest {
                    expected_revision: Some(expected_revision),
                });
                let response = if step["op"] == "undo" {
                    handlers_v2::session_undo(
                        State(state.clone()),
                        axum::extract::Path(id.clone()),
                        request,
                    )
                    .await
                } else {
                    handlers_v2::session_redo(
                        State(state.clone()),
                        axum::extract::Path(id.clone()),
                        request,
                    )
                    .await
                };
                let (status, outcome) = body(response).await;
                assert_eq!(status, StatusCode::OK);
                assert_eq!(outcome["revision"], expected["revision"]);
            }
            "snapshot" => {
                let (status, snapshot) = body(
                    handlers_v2::read_session(
                        State(state.clone()),
                        axum::extract::Path(id.clone()),
                    )
                    .await,
                )
                .await;
                assert_eq!(status, StatusCode::OK);
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
                let part_id = initial["project"]["tracks"][0]["parts"][0]["id"]
                    .as_str()
                    .unwrap();
                let (status, notes) = body(
                    handlers_v2::part_notes(
                        State(state.clone()),
                        axum::extract::Path((id.clone(), part_id.to_string())),
                    )
                    .await,
                )
                .await;
                assert_eq!(status, StatusCode::OK);
                assert_eq!(notes["revision"], expected["revision"]);
                assert_eq!(
                    notes["notes"].as_array().unwrap().len(),
                    expected["note_count"]
                );
            }
            "read_tracks" => {
                let (status, tracks) = body(
                    handlers_v2::tracks(State(state.clone()), axum::extract::Path(id.clone()))
                        .await,
                )
                .await;
                assert_eq!(status, StatusCode::OK);
                assert_eq!(tracks["revision"], expected["revision"]);
                assert_eq!(
                    tracks["tracks"].as_array().unwrap().len(),
                    expected["track_count"]
                );
            }
            "group_begin" => {
                let (status, outcome) = body(
                    handlers_v2::group_begin(
                        State(state.clone()),
                        axum::extract::Path(id.clone()),
                        JsonBody(GroupBeginRequest {
                            expected_revision: Some(expected["revision"].as_u64().unwrap()),
                            name: Some(step["name"].as_str().unwrap().to_string()),
                        }),
                    )
                    .await,
                )
                .await;
                assert_eq!(status, StatusCode::OK);
                group_id = Some(outcome["group_id"].as_str().unwrap().to_string());
                assert_eq!(outcome["base_revision"], expected["revision"]);
            }
            "group_tone" => {
                let (status, outcome) = command(
                    &state,
                    &id,
                    json!({
                        "expected_revision": expected["revision"],
                        "group_id": group_id.as_deref().unwrap(),
                        "command":{"type":"note.set_tone","note_id":note_id,"tone":step["tone"]}
                    }),
                )
                .await;
                assert_eq!(status, StatusCode::OK);
                assert_eq!(outcome["revision"], expected["revision"]);
                assert_eq!(outcome["changed"], expected["changed"]);
            }
            "group_commit" => {
                let (status, outcome) = body(
                    handlers_v2::group_commit(
                        State(state.clone()),
                        axum::extract::Path((id.clone(), group_id.as_deref().unwrap().to_string())),
                        JsonBody(GroupCommitRequest {
                            expected_revision: Some(expected["revision"].as_u64().unwrap() - 1),
                        }),
                    )
                    .await,
                )
                .await;
                assert_eq!(status, StatusCode::OK);
                group_id = None;
                assert_eq!(outcome["revision"], expected["revision"]);
                assert_eq!(outcome["changed"], expected["changed"]);
            }
            "track_mute" => {
                let (status, outcome) = command(
                    &state,
                    &id,
                    json!({
                        "expected_revision": expected["revision"].as_u64().unwrap() - 1,
                        "command":{"type":"track.set_mute","track_id":track_id,"mute":step["mute"]}
                    }),
                )
                .await;
                assert_eq!(status, StatusCode::OK);
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

#[tokio::test]
async fn shared_curve_adapter_vector_matches_native_http_routes() {
    let vector: Value = serde_json::from_str(SHARED_CURVE_VECTOR).unwrap();
    let state = state("shared-curve-vector");
    let (status, opened) = body(
        handlers_v2::create_session(
            State(state.clone()),
            JsonBody(CreateSessionRequest {
                source: CreateSource::ImportedUstx {
                    name: "shared-curve".into(),
                    ustx: SHARED_CURVE_FIXTURE.into(),
                },
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = opened["session_id"].as_str().unwrap().to_string();
    let part_id = opened["snapshot"]["project"]["tracks"][0]["parts"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let curve_id = opened["snapshot"]["project"]["tracks"][0]["parts"][0]["curves"][0]["id"]
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
                let (status, error) = command(
                    &state,
                    &id,
                    json!({
                        "expected_revision": step["revision"],
                        "command": {
                            "type":"curve.apply_stroke", "part_id":part_id, "curve_id":curve_id,
                            "mode":"replace", "start_tick":0, "end_tick":960, "points":points
                        }
                    }),
                )
                .await;
                assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
                assert_eq!(error["error"]["code"], expected["code"]);
                assert_eq!(error["revision"], expected["revision"]);
                let (read_status, curve) = body(
                    handlers_v2::read_curve(
                        State(state.clone()),
                        axum::extract::Path((id.clone(), part_id.clone(), curve_id.clone())),
                    )
                    .await,
                )
                .await;
                assert_eq!(read_status, StatusCode::OK);
                assert_eq!(curve["revision"], expected["revision"]);
                assert_eq!(
                    curve["points"],
                    json!([{"tick":0,"value":0},{"tick":480,"value":10}])
                );
            }
            "curve_apply" => {
                let (status, _) = command(
                    &state,
                    &id,
                    json!({
                        "expected_revision": step["revision"],
                        "command": {
                            "type":"curve.apply_stroke", "part_id":part_id, "curve_id":curve_id,
                            "mode":"replace", "start_tick":0, "end_tick":960,
                            "points":step["points"]
                        }
                    }),
                )
                .await;
                assert_eq!(status, StatusCode::OK);
                let (status, curve) = body(
                    handlers_v2::read_curve(
                        State(state.clone()),
                        axum::extract::Path((id.clone(), part_id.clone(), curve_id.clone())),
                    )
                    .await,
                )
                .await;
                assert_eq!(status, StatusCode::OK);
                assert_eq!(curve["revision"], expected["revision"]);
                assert_eq!(curve["points"], expected["points"]);
            }
            "undo" => {
                let (status, outcome) = body(
                    handlers_v2::session_undo(
                        State(state.clone()),
                        axum::extract::Path(id.clone()),
                        JsonBody(handlers_v2::HistoryRequest {
                            expected_revision: Some(step["revision"].as_u64().unwrap()),
                        }),
                    )
                    .await,
                )
                .await;
                assert_eq!(status, StatusCode::OK);
                assert_eq!(outcome["revision"], expected["revision"]);
                let (status, curve) = body(
                    handlers_v2::read_curve(
                        State(state.clone()),
                        axum::extract::Path((id.clone(), part_id.clone(), curve_id.clone())),
                    )
                    .await,
                )
                .await;
                assert_eq!(status, StatusCode::OK);
                assert_eq!(curve["points"], expected["points"]);
            }
            operation => panic!("unknown shared curve operation {operation}"),
        }
    }
}

#[tokio::test]
async fn save_as_forks_project_id_only_and_reopens_with_continuous_entity_ids() {
    let state = state("save-as-lifecycle");
    let id = open_stored(&state, "save-as-source").await;
    let (old_project_id, note_id) = {
        let registry = state.sessions.lock().unwrap();
        let table = registry.get(&id).unwrap().session.identity();
        (
            table.project_id.as_str().to_string(),
            table.note_id(0, 0, 0).unwrap().as_str().to_string(),
        )
    };
    let (failed_status, _) = body(
        handlers_v2::session_save_as(
            State(state.clone()),
            axum::extract::Path(id.clone()),
            JsonBody(SaveRequest {
                name: "../invalid".into(),
                expected_revision: Some(0),
            }),
        )
        .await,
    )
    .await;
    assert_ne!(failed_status, StatusCode::OK);
    {
        let registry = state.sessions.lock().unwrap();
        assert_eq!(
            registry
                .get(&id)
                .unwrap()
                .session
                .identity()
                .project_id
                .as_str(),
            old_project_id
        );
    }
    let (status, saved) = body(
        handlers_v2::session_save_as(
            State(state.clone()),
            axum::extract::Path(id.clone()),
            JsonBody(SaveRequest {
                name: "save-as-copy".into(),
                expected_revision: Some(0),
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_ne!(saved["project_id"], old_project_id);
    assert_eq!(saved["revision"], 0);
    let (active_project_id, active_note_id) = {
        let registry = state.sessions.lock().unwrap();
        let table = registry.get(&id).unwrap().session.identity();
        (
            table.project_id.as_str().to_string(),
            table.note_id(0, 0, 0).unwrap().as_str().to_string(),
        )
    };
    assert_eq!(active_project_id, saved["project_id"]);
    assert_eq!(active_note_id, note_id);

    let (status, opened) = body(
        handlers_v2::create_session(
            State(state.clone()),
            JsonBody(CreateSessionRequest {
                source: CreateSource::StoredProject {
                    name: "save-as-copy".into(),
                },
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(opened["ids_stable"], true);
    assert_eq!(opened["snapshot"]["project"]["id"], saved["project_id"]);
    assert_eq!(
        opened["snapshot"]["project"]["tracks"][0]["parts"][0]["notes"][0]["id"],
        note_id
    );
}
