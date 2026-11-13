//! Compare the Rust phonemizers with the OpenUtau 1:1 test vectors and their
//! checked-in Classic voicebank oto fixtures. Run from the repo root with:
//! `cargo run -p phonemizer --example t013_openutau_fixture_compare`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use domain::UNote;
use phonemizer::{EnglishCvvcPhonemizer, JapaneseVcvPhonemizer, Phonemizer};
use voicebank::{parse_character_yaml, parse_oto_ini, Subbank, Voicebank};

fn main() {
    let root = repo_root();
    let fixtures = root
        .join("ref(openutau+openutau mobile)")
        .join("desktop-ref/OpenUtau.Test/Files");

    let ja_vcv = fixture_bank(&fixtures.join("ja_vcv"), "shift_jis");
    let ja_notes = ["- ず", "u と", "o R"]
        .into_iter()
        .zip([57, 60, 62])
        .map(|(lyric, tone)| note(lyric, tone))
        .collect::<Vec<_>>();
    show(
        "Japanese VCV (official JaVcvTest phonetic-alias case)",
        &JapaneseVcvPhonemizer.process(&ja_notes, Some(&ja_vcv)),
        &["- ずA3", "u とA3", "o RD4"],
        Some(&ja_vcv),
    );

    let en_vccv = fixture_bank(&fixtures.join("en_vccv"), "utf-8");
    let en_hint = [note("read [r E d]", 60)];
    show(
        "English VCCV (official EnVCCVTest hint case)",
        &EnglishCvvcPhonemizer::new().process(&en_hint, Some(&en_vccv)),
        &["-rE", "Ed-"],
        Some(&en_vccv),
    );

    let en_words = [note("test", 60), note("words", 60)];
    show(
        "English VCCV (official EnVCCVTest word case)",
        &EnglishCvvcPhonemizer::new().process(&en_words, Some(&en_vccv)),
        &["-te", "es-", "st", "w3", "3d-", "dz-"],
        Some(&en_vccv),
    );
}

fn note(lyric: &str, tone: i32) -> UNote {
    UNote {
        lyric: lyric.to_string(),
        duration: 240,
        tone,
        ..Default::default()
    }
}

fn show(name: &str, phonemes: &[domain::UPhoneme], expected: &[&str], bank: Option<&Voicebank>) {
    let raw: Vec<&str> = phonemes.iter().map(|p| p.phoneme.as_str()).collect();
    let resolved: Vec<String> = phonemes
        .iter()
        .map(|p| {
            bank.and_then(|vb| vb.lookup(&p.phoneme, p.tone))
                .map(|oto| oto.alias.clone())
                .unwrap_or_else(|| p.phoneme.clone())
        })
        .collect();
    let resolved_refs: Vec<&str> = resolved.iter().map(String::as_str).collect();
    let matches = resolved_refs == expected;
    println!("{}: {}", if matches { "PASS" } else { "DIFF" }, name);
    println!("  OpenUtau expected: [{}]", expected.join(" | "));
    println!("  Rust phonemes:     [{}]", raw.join(" | "));
    println!("  Resolved oto:      [{}]", resolved_refs.join(" | "));
}

fn fixture_bank(dir: &Path, encoding: &str) -> Voicebank {
    let mut paths = Vec::new();
    collect_oto_paths(dir, &mut paths);
    paths.sort();
    let mut combined = Vec::new();
    for path in paths {
        combined.extend(std::fs::read(&path).expect("read fixture oto.ini"));
        combined.push(b'\n');
    }
    let parsed = parse_oto_ini(&combined, Some(encoding));
    let oto_map = parsed
        .otos
        .iter()
        .cloned()
        .map(|oto| (oto.alias.clone(), oto))
        .collect::<HashMap<_, _>>();
    let yaml = std::fs::read(dir.join("character.yaml")).expect("read character.yaml");
    let cfg = parse_character_yaml(yaml.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&yaml))
        .expect("parse character.yaml");
    let subbanks = cfg
        .subbanks
        .iter()
        .map(|subbank| {
            Subbank::from_ranges(
                subbank.color.clone().unwrap_or_default(),
                subbank.prefix.clone().unwrap_or_default(),
                subbank.suffix.clone().unwrap_or_default(),
                subbank.tone_ranges.clone(),
            )
        })
        .collect();
    Voicebank {
        name: dir.file_name().unwrap().to_string_lossy().into_owned(),
        base_path: dir.parent().unwrap_or(dir).to_path_buf(),
        path: dir.to_path_buf(),
        id: dir.file_name().unwrap().to_string_lossy().into_owned(),
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
        text_file_encoding: voicebank::text::detect_encoding(&combined, Some(encoding)),
        singer_type: String::new(),
        use_filename_as_alias: None,
        subbanks,
        oto_sets: Vec::new(),
        otos: parsed.otos,
        oto_map,
        warnings: Vec::new(),
    }
}

fn collect_oto_paths(dir: &Path, paths: &mut Vec<PathBuf>) {
    if dir.join("oto.ini").is_file() {
        paths.push(dir.join("oto.ini"));
    }
    for entry in std::fs::read_dir(dir).expect("read fixture directory") {
        let path = entry.expect("read fixture entry").path();
        if path.is_dir() {
            collect_oto_paths(&path, paths);
        }
    }
}

fn repo_root() -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    path.canonicalize().expect("resolve repository root")
}
