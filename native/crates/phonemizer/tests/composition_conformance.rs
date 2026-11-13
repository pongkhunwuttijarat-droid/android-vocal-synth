//! T013 composition conformance vectors — pin the canonical JSON of the
//! composition DTOs (the examples in
//! `docs/planning/phoneme-compose-contract-proposal.md`) to the actual
//! serde output. Future host adapters (project-session, engine-wasm,
//! synth-server) must normalize to these shapes.

use phonemizer::{CompositionRequest, MappingMode, NotePronunciation, PhonemeComposer, Style};
use std::collections::HashMap;
use voicebank::{parse_oto_ini, Voicebank};

/// Minimal Japanese VCV bank (oto.ini text only) — the same fixture the
/// `t013_compose_json` example prints the doc examples from.
fn ja_voicebank() -> Voicebank {
    let ini = concat!(
        "_a_a_a.wav=- か,0.0,100.0,-275.001,1.994,10.0\n",
        "_a_a_a.wav=か,100.0,100.0,-275.001,1.994,10.0\n",
        "_a_a_a.wav=a,0.0,100.0,-275.001,1.994,10.0\n",
        "_sa+_sa+_s-.wav=- sa,80.0,251.664,-498.668,201.665,73.333\n",
        "_sa+_sa+_s-.wav=sa,736.336,185.666,-352.001,135.666,44.0\n",
        "_s3_s3_s-.wav=s,742.069,50.731,-122.467,0.731,27.133\n",
    );
    let parsed = parse_oto_ini(ini.as_bytes(), None);
    let otos = parsed.otos;
    let oto_map: HashMap<String, _> = otos.iter().map(|o| (o.alias.clone(), o.clone())).collect();
    Voicebank {
        name: "ja-test".into(),
        base_path: Default::default(),
        path: Default::default(),
        id: "ja-test".into(),
        image: None,
        portrait: None,
        portrait_opacity: 0.0,
        portrait_height: 0,
        author: None,
        voice: None,
        web: None,
        version: None,
        sample: None,
        default_phonemizer: None,
        other_info: String::new(),
        localized_names: HashMap::new(),
        text_file_encoding: encoding_rs::UTF_8,
        singer_type: String::new(),
        use_filename_as_alias: None,
        subbanks: Vec::new(),
        oto_sets: Vec::new(),
        otos,
        oto_map,
        warnings: Vec::new(),
    }
}

/// Canonical request (CONTRACT proposal §phoneme.compose).
const REQUEST_JSON: &str = r#"{
  "input": "か",
  "mode": "universal",
  "style": "auto",
  "limit": 8,
  "tone": 60,
  "voicebank_id": "vb-ja-test"
}"#;

/// Canonical committed pronunciation (CONTRACT proposal §note.set_pronunciation).
const COMMITTED_JSON: &str = r#"{
  "language": "ja",
  "mode": "voicebank_native",
  "style": "cv",
  "strategy": "direct_alias",
  "language_phones": ["か"],
  "universal_phones": ["か"]
}"#;

#[test]
fn canonical_request_round_trips() {
    let request: CompositionRequest = serde_json::from_str(REQUEST_JSON).expect("parse request");
    assert_eq!(request.input, "か");
    assert_eq!(request.mode, MappingMode::Universal);
    assert_eq!(request.style, Style::Auto);
    assert_eq!(request.limit, Some(8));
    assert_eq!(request.tone, 60);
    assert_eq!(request.voicebank_id.as_deref(), Some("vb-ja-test"));
    // Re-serialization is byte-identical field-name-wise.
    let round = serde_json::to_value(&request).expect("serialize request");
    let expected: serde_json::Value = serde_json::from_str(REQUEST_JSON).expect("parse json");
    assert_eq!(round, expected);
}

#[test]
fn compose_result_matches_the_canonical_shape() {
    let request: CompositionRequest = serde_json::from_str(REQUEST_JSON).expect("parse request");
    let vb = ja_voicebank();
    let result = PhonemeComposer::new().compose(&request, Some(&vb));
    let json = serde_json::to_value(&result).expect("serialize result");

    // Top-level shape and field spellings.
    for field in [
        "composition_id",
        "input",
        "language",
        "mode",
        "requested_style",
        "voicebank_id",
        "candidates",
        "diagnostics",
    ] {
        assert!(json.get(field).is_some(), "missing field {field}");
    }
    assert_eq!(json["language"], "ja");
    assert_eq!(json["mode"], "universal");
    assert_eq!(json["requested_style"], "auto");

    let candidates = json["candidates"].as_array().expect("candidates array");
    assert!(candidates.len() >= 3, "expected ≥3 candidates");
    for candidate in candidates {
        for field in [
            "candidate_id",
            "display_text",
            "language",
            "style",
            "strategy",
            "language_phones",
            "universal_phones",
            "voicebank_aliases",
            "unresolved_phones",
        ] {
            assert!(candidate.get(field).is_some(), "missing field {field}");
        }
        assert!(candidate["style"] != "auto", "candidate style must resolve");
    }

    // The strategy set and the resolved alias sequences are pinned.
    let strategies: Vec<&str> = candidates
        .iter()
        .map(|c| c["strategy"].as_str().expect("strategy"))
        .collect();
    for expected in ["direct_alias", "japanese_vcv", "mora_cv", "universal_ipa"] {
        assert!(
            strategies.contains(&expected),
            "missing strategy {expected}"
        );
    }
    let mora = candidates
        .iter()
        .find(|c| c["strategy"] == "mora_cv")
        .expect("mora candidate");
    assert_eq!(mora["language_phones"], serde_json::json!(["か"]));
    assert_eq!(
        mora["voicebank_aliases"],
        serde_json::json!(["か", "a"]),
        "CV head + tail vowel, resolved against the kana bank"
    );
    assert_eq!(mora["unresolved_phones"], serde_json::json!([]));
}

#[test]
fn style_restriction_vector() {
    let request: CompositionRequest = serde_json::from_str(REQUEST_JSON).expect("parse request");
    let mut restricted = request.clone();
    restricted.style = Style::Vccv;
    let vb = ja_voicebank();
    let result = PhonemeComposer::new().compose(&restricted, Some(&vb));
    let json = serde_json::to_value(&result).expect("serialize");
    assert_eq!(json["candidates"], serde_json::json!([]));
    assert_eq!(json["requested_style"], "vccv");
    assert_eq!(
        json["diagnostics"][0],
        "no vccv candidates for this input in the selected voicebank"
    );
}

#[test]
fn committed_pronunciation_round_trips() {
    let committed: NotePronunciation = serde_json::from_str(COMMITTED_JSON).expect("parse");
    let round = serde_json::to_value(&committed).expect("serialize");
    let expected: serde_json::Value = serde_json::from_str(COMMITTED_JSON).expect("parse json");
    assert_eq!(round, expected);

    // Rederive the aliases from the committed universal sequence.
    let vb = ja_voicebank();
    let aliases = committed.resolve_aliases(Some(&vb), 60);
    let alias_json = serde_json::to_value(&aliases).expect("serialize");
    assert_eq!(
        alias_json,
        serde_json::json!([
            { "universal_phone": "か", "alias": "か", "kind": "exact" }
        ])
    );

    // Without a bank every phone is unresolved, verbatim.
    let none = serde_json::to_value(committed.resolve_aliases(None, 60)).expect("serialize");
    assert_eq!(
        none,
        serde_json::json!([
            { "universal_phone": "か", "alias": "か", "kind": "unresolved" }
        ])
    );
}
