//! Integration tests against the real Teto English voicebank in
//! `test/golden/teto-english/library` (checked into the repo).
//!
//! The library dir can be overridden with the `TETO_VOICEBANK_DIR` env var.

use std::path::PathBuf;

use voicebank::{load_voicebank, load_voicebank_with_options, LoadOptions};

fn teto_library() -> PathBuf {
    if let Ok(dir) = std::env::var("TETO_VOICEBANK_DIR") {
        return PathBuf::from(dir);
    }
    let default = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../test/golden/teto-english/library");
    assert!(
        default.is_dir(),
        "Teto voicebank not found at {} (set TETO_VOICEBANK_DIR)",
        default.display()
    );
    default
}

#[test]
fn parses_real_teto_oto_ini() {
    let vb = load_voicebank(&teto_library()).expect("load Teto");
    // 2681 non-empty lines, all valid, no parse errors.
    assert_eq!(vb.otos.len(), 2681);
    assert_eq!(vb.oto_map.len(), 2673); // 8 aliases are duplicated across wavs
    assert!(vb.warnings.is_empty(), "unexpected warnings: {:?}", vb.warnings);

    // oto set discovered at voice/oto.ini.
    assert_eq!(vb.oto_sets.len(), 1);
    assert_eq!(vb.oto_sets[0].name, "voice");

    // Spot-check real aliases (spaces, dashes, @ are all valid; the `+`
    // chars only appear in Teto's wav filenames, not its aliases).
    for alias in ["- @", "@ h@", "h@", "3 h3", "Z@", "@ Z-", "- 3"] {
        assert!(
            vb.oto_map.contains_key(alias),
            "expected alias {alias:?} in oto_map"
        );
    }
    // The wav referenced by the first entries of oto.ini.
    let oto = &vb.oto_map["- @"];
    assert_eq!(oto.wav, "_@_h@_@_@_@-.wav");
    assert_eq!(oto.offset, 120.0);
    assert_eq!(oto.consonant, 124.999);
}

#[test]
fn all_558_wavs_referenced() {
    let vb = load_voicebank(&teto_library()).unwrap();
    let voice_dir = teto_library().join("voice");
    let wavs: std::collections::HashSet<String> = std::fs::read_dir(&voice_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".wav"))
        .collect();
    assert_eq!(wavs.len(), 558);
    let referenced: std::collections::HashSet<&str> = vb
        .otos
        .iter()
        .filter(|o| o.is_valid)
        .map(|o| o.wav.as_str())
        .collect();
    // Every wav on disk is referenced (so no filename aliases were added).
    assert_eq!(referenced.len(), 558);
    for wav in &wavs {
        assert!(referenced.contains(wav.as_str()), "missing reference: {wav}");
    }
    // And every referenced wav exists on disk.
    for oto in &vb.otos {
        assert!(
            voice_dir.join(&oto.wav).is_file(),
            "referenced wav missing: {}",
            oto.wav
        );
    }
}

#[test]
fn filename_aliases_cover_wav_stems() {
    // OpenUtau's `use_filename_as_alias` feature: the wav filename becomes
    // an alias (e.g. `_3_h3_3-`), so phonemes can reference samples by name.
    let opts = LoadOptions {
        use_filename_as_alias: Some(true),
    };
    let vb = load_voicebank_with_options(&teto_library(), &opts).unwrap();
    assert!(vb.oto_map.contains_key("_3_h3_3-"));
    let oto = &vb.oto_map["_3_h3_3-"];
    assert_eq!(oto.wav, "_3_h3_3-.wav");
    // Parameters copied from the entry with the smallest offset for that wav
    // (line 13: alias "- 3", offset 120.0).
    assert_eq!(oto.offset, 120.0);
    assert_eq!(oto.alias, "_3_h3_3-");
    // Without the option, the stem is NOT an alias (it is a wav, not an
    // alias, in the real oto.ini).
    let vb = load_voicebank(&teto_library()).unwrap();
    assert!(!vb.oto_map.contains_key("_3_h3_3-"));
}

#[test]
fn parses_real_teto_character_txt() {
    let vb = load_voicebank(&teto_library()).unwrap();
    // Shift-JIS decoded: 重音テト（かさねてと）音声ライブラリー
    assert_eq!(vb.name, "重音テト（かさねてと）音声ライブラリー");
    assert_eq!(vb.image.as_deref(), Some("teto.bmp"));
    assert_eq!(vb.web.as_deref(), Some("http://kasaneteto.jp/"));
    assert_eq!(vb.sample.as_deref(), Some("重音テト単独音\\_にゃ.wav"));
    // Unrecognized key/value lines land in other_info.
    assert!(vb.other_info.contains("性別"));
    assert!(vb.other_info.contains("フランスパン"));
    assert!(vb.other_info.contains("小山乃舞世"));
    // Encoding resolved to Shift-JIS for this legacy bank.
    assert_eq!(vb.text_file_encoding, encoding_rs::SHIFT_JIS);
}

#[test]
fn reads_real_teto_wav() {
    let vb = load_voicebank(&teto_library()).unwrap();
    let oto = &vb.oto_map["3 h3"];
    let wav = vb.read_wav(oto).expect("read wav");
    assert_eq!(wav.sample_rate, 44100);
    assert_eq!(wav.channels, 1);
    assert_eq!(wav.bits_per_sample, 16);
    // 235788 data bytes / 2 bytes per sample.
    assert_eq!(wav.samples.len(), 117894);
    assert!(!wav.samples.is_empty());
    // Normalized PCM range.
    assert!(wav.samples.iter().all(|s| s.abs() <= 1.000001));
    assert!(wav.samples.iter().any(|s| s.abs() > 0.001), "not silent");
    // Direct file read agrees.
    let direct = voicebank::read_wav(&teto_library().join("voice/_3_h3_3-.wav")).unwrap();
    assert_eq!(direct.samples, wav.samples);
}

#[test]
fn reads_real_teto_frq() {
    let vb = load_voicebank(&teto_library()).unwrap();
    let oto = &vb.oto_map["3 h3"];
    let frq = vb.read_frq(oto).expect("read frq");
    assert_eq!(frq.hop_size, 256);
    assert!(frq.average_f0 > 0.0, "average f0 = {}", frq.average_f0);
    assert_eq!(frq.f0.len(), 461);
    assert_eq!(frq.amp.len(), 461);
    assert!(frq.f0.iter().any(|f| *f > 0.0), "no voiced frames");
}

#[test]
fn teto_has_no_prefix_map_so_lookup_is_plain() {
    let vb = load_voicebank(&teto_library()).unwrap();
    // No prefix.map / character.yaml => one default empty subbank.
    assert_eq!(vb.subbanks.len(), 1);
    assert_eq!(vb.subbanks[0].color, "");
    assert!(vb.subbanks[0].tones.is_empty());
    // Lookup works for any tone via the plain alias.
    let oto = vb.lookup("3 h3", 60).unwrap();
    assert_eq!(oto.alias, "3 h3");
    assert_eq!(vb.lookup("3 h3", 100).unwrap().alias, "3 h3");
    assert!(vb.lookup("does-not-exist", 60).is_none());
    // Special characters (@, -, spaces) resolve in the map.
    assert!(vb.lookup_plain("Z@").is_some());
    assert!(vb.lookup_plain("@ Z-").is_some());
    assert!(vb.lookup_plain("- @").is_some());
}

#[test]
fn prefix_map_parser_handles_empty_file() {
    // The acceptance criterion: prefix.map must parse even when empty/missing.
    assert!(voicebank::parse_prefix_map(b"", "", None).is_empty());
    assert!(voicebank::parse_prefix_map(b"\r\n\r\n", "", None).is_empty());
}
