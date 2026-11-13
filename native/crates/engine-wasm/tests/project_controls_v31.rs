use engine_wasm::Engine;
use serde_json::Value;

const DEMO: &str = include_str!("../../../tools/synth-cli/tests/data/demo-song.ustx");

#[test]
fn project_controls_v31_is_advertised_and_curve_reads_are_available() {
    let mut engine = Engine::new();
    let snapshot: Value =
        serde_json::from_str(&engine.session_open_yaml(DEMO, "").unwrap()).unwrap();
    let part_id = snapshot["project"]["tracks"][0]["parts"][0]["id"]
        .as_str()
        .unwrap();

    let capabilities: Value =
        serde_json::from_str(&engine.session_capabilities_json().unwrap()).unwrap();
    assert!(capabilities["features"]
        .as_array()
        .unwrap()
        .iter()
        .any(|feature| feature == "project.controls.v1"));

    let descriptor: Value =
        serde_json::from_str(&engine.session_expression_descriptor_json("dyn").unwrap()).unwrap();
    let stroke: Value = serde_json::from_str(
        &engine
            .session_curve_stroke_context_for_abbr_json(part_id, "dyn", None)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(descriptor["revision"], snapshot["revision"]);
    assert_eq!(stroke["revision"], snapshot["revision"]);
    assert_eq!(stroke["part_id"], part_id);
}
