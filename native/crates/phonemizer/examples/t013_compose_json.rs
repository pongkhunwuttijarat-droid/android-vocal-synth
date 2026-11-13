//! T013 canonical JSON generator — prints the serde output of the
//! composition DTOs for the contract proposal examples in
//! `docs/planning/phoneme-compose-contract-proposal.md`.
//!
//! Run: cargo run -p phonemizer --example t013_compose_json

use std::collections::HashMap;

use phonemizer::{CompositionRequest, MappingMode, NotePronunciation, PhonemeComposer, Style};
use voicebank::{parse_oto_ini, Voicebank};

fn ja_voicebank() -> voicebank::Voicebank {
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

fn main() {
    let vb = ja_voicebank();
    let composer = PhonemeComposer::new();

    let mut request = CompositionRequest {
        input: "か".to_string(),
        language: None,
        mode: MappingMode::Universal,
        style: Style::Auto,
        limit: Some(8),
        tone: 60,
        voicebank_id: Some("vb-ja-test".to_string()),
    };
    let result = composer.compose(&request, Some(&vb));
    println!("── request ──");
    println!("{}", serde_json::to_string_pretty(&request).unwrap());
    println!("── result (auto language, auto style) ──");
    println!("{}", serde_json::to_string_pretty(&result).unwrap());

    request.style = Style::Vccv;
    let restricted = composer.compose(&request, Some(&vb));
    println!("── result (style=vccv restriction) ──");
    println!("{}", serde_json::to_string_pretty(&restricted).unwrap());

    let mora = result
        .candidates
        .iter()
        .find(|c| c.strategy == phonemizer::Strategy::MoraCv)
        .expect("mora candidate");
    let committed = NotePronunciation::from_candidate(mora, MappingMode::VoicebankNative)
        .expect("mora candidate has universal phones");
    println!("── committed note pronunciation ──");
    println!("{}", serde_json::to_string_pretty(&committed).unwrap());
    println!("── rederived aliases on the same bank ──");
    println!(
        "{}",
        serde_json::to_string_pretty(&committed.resolve_aliases(Some(&vb), 60)).unwrap()
    );
}
