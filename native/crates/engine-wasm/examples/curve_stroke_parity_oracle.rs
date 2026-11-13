//! T009 native Session oracle for `web/app/curve-stroke-transport.parity.test.mjs`.
//!
//! Reads a JSON array from stdin. Each item contains a USTX `yaml` string,
//! stroke bounds, raw points, and reduced points. Emits the native Session's
//! final curve points for both commands so the Node test can compare them with
//! the direct-WASM Session and the JavaScript reducer.
use engine_wasm::Engine;
use serde_json::{json, Value};
use std::io::{self, Read};

fn apply(yaml: &str, vector: &Value, points: &Value) -> Value {
    let mut engine = Engine::new();
    let opened: Value = serde_json::from_str(&engine.session_open_yaml(yaml, "").unwrap()).unwrap();
    let part = &opened["project"]["tracks"][0]["parts"][0];
    let part_id = part["id"].as_str().unwrap();
    let create = vector["create"].as_bool().unwrap_or(true);
    let curve_id = if create {
        Value::Null
    } else {
        part["curves"][0]["id"].clone()
    };
    let command = json!({
        "type":"curve.apply_stroke", "part_id":part_id, "curve_id":curve_id,
        "abbr":"dyn", "create":create, "mode":"replace",
        "start_tick":vector["startTick"], "end_tick":vector["endTick"], "points":points
    });
    engine
        .session_apply_v2(&json!({"expected_revision":0,"command":command}).to_string())
        .unwrap();
    let snapshot: Value = serde_json::from_str(&engine.session_snapshot_json().unwrap()).unwrap();
    snapshot["project"]["tracks"][0]["parts"][0]["curves"]
        .as_array()
        .unwrap()
        .iter()
        .find(|curve| curve["abbr"] == "dyn")
        .unwrap()["points"]
        .clone()
}

fn main() {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).unwrap();
    let vectors: Vec<Value> = serde_json::from_str(&input).unwrap();
    let results: Vec<Value> = vectors
        .iter()
        .map(|vector| {
            let yaml = vector["yaml"].as_str().unwrap();
            json!({
                "raw":apply(yaml, vector, &vector["rawPoints"]),
                "reduced":apply(yaml, vector, &vector["reducedPoints"])
            })
        })
        .collect();
    println!("{}", serde_json::to_string(&results).unwrap());
}
