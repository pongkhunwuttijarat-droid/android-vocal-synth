//! The T024 render asset planner must resolve samples from the committed Rust
//! Session pronunciation, and preserve that result across its sidecar reload.

use std::path::{Path, PathBuf};

use engine_wasm::Engine;
use serde_json::{json, Value};

const DEMO: &str = include_str!("../../../tools/synth-cli/tests/data/demo-song.ustx");

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn key_plan(engine: &Engine) -> Value {
    serde_json::from_str(
        &engine
            .session_render_sample_keys_json("[0]")
            .expect("Rust Session sample-key plan"),
    )
    .expect("sample-key plan JSON")
}

fn keys(plan: &Value) -> Vec<String> {
    plan["sampleKeys"]
        .as_array()
        .expect("sampleKeys array")
        .iter()
        .map(|key| key.as_str().expect("string key").to_owned())
        .collect()
}

fn install_oto(engine: &mut Engine, oto_path: &Path) {
    let oto = std::fs::read(oto_path).expect("Teto fixture oto.ini");
    engine.set_voicebank("teto-english", "Teto", "voice", &oto);
}

#[test]
fn sample_keys_follow_committed_pronunciation_and_survive_sidecar_reload() {
    let library = repo_root().join("test/golden/teto-english/library");
    let oto_path = library.join("voice/oto.ini");
    if !oto_path.is_file() {
        eprintln!("skip: {} is not present", oto_path.display());
        return;
    }

    let mut engine = Engine::new();
    install_oto(&mut engine, &oto_path);
    let opened: Value = serde_json::from_str(
        &engine
            .session_open_yaml(DEMO, "")
            .expect("open demo Session"),
    )
    .expect("Session snapshot JSON");
    let initial_plan = key_plan(&engine);
    let initial_keys = keys(&initial_plan);
    assert!(!initial_keys.is_empty());
    assert_eq!(initial_plan["sessionId"], opened["session_id"]);
    assert_eq!(initial_plan["revision"], opened["revision"]);

    let project = &opened["project"];
    let note = &project["tracks"][0]["parts"][0]["notes"][0];
    let project_id = project["id"].as_str().expect("project ID");
    let part_id = project["tracks"][0]["parts"][0]["id"]
        .as_str()
        .expect("part ID");
    let note_id = note["id"].as_str().expect("note ID");
    let compose_request = json!({
        "project_id": project_id,
        "part_id": part_id,
        "note_id": note_id,
        "expected_revision": engine.session_revision(),
        "input": note["lyric"],
        "language": "en",
        "mode": "universal",
        "style": "auto",
        "limit": 20,
        "tone": note["tone"],
    });
    let composed: Value = serde_json::from_str(
        &engine
            .session_phoneme_compose_v2(&compose_request.to_string())
            .expect("compose real demo note"),
    )
    .expect("composition JSON");

    let mut selected_keys = None;
    for candidate in composed["result"]["candidates"]
        .as_array()
        .expect("composition candidates")
    {
        let pronunciation = json!({
            "language": composed["result"]["language"],
            "mode": composed["result"]["mode"],
            "style": candidate["style"],
            "strategy": candidate["strategy"],
            "language_phones": candidate["language_phones"],
            "universal_phones": candidate["universal_phones"],
        });
        let apply = json!({
            "expected_revision": engine.session_revision(),
            "command": {
                "type": "note.set_pronunciation",
                "note_id": note_id,
                "pronunciation": pronunciation,
            }
        });
        if engine.session_apply_v2(&apply.to_string()).is_err() {
            continue;
        }

        let plan = key_plan(&engine);
        assert_eq!(plan["sessionId"], opened["session_id"]);
        assert_eq!(plan["revision"].as_u64(), Some(engine.session_revision()));
        let current_keys = keys(&plan);
        if current_keys != initial_keys {
            selected_keys = Some(current_keys);
            break;
        }
    }

    let selected_keys = selected_keys.expect(
        "at least one valid committed composition candidate must change the exact sample keys",
    );
    let mut sorted_unique = selected_keys.clone();
    sorted_unique.sort();
    sorted_unique.dedup();
    assert_eq!(selected_keys, sorted_unique);

    let id_sidecar = engine.session_save_sidecar().expect("save ID sidecar");
    let pronunciation_sidecar = engine
        .session_save_pronunciation_sidecar()
        .expect("save pronunciation sidecar");
    let mut reopened = Engine::new();
    install_oto(&mut reopened, &oto_path);
    reopened
        .session_open_yaml_with_sidecars(DEMO, &id_sidecar, &pronunciation_sidecar)
        .expect("reopen Session sidecars");
    assert_eq!(
        keys(&key_plan(&reopened)),
        selected_keys,
        "the sidecar-restored pronunciation must pin the same render samples",
    );
}
