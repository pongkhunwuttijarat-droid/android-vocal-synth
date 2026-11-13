//! Integration test for OpenUtau managed phonemizer runtime.

use std::path::{Path, PathBuf};
use domain::UPart;
use synth_cli::pipeline::{self, PhonemizerKind};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

#[test]
fn unsupported_openutau_plugin_returns_clear_render_error() {
    let root = repo_root();
    let mut project = pipeline::load_project(&root.join("test/golden/sakura-test.ustx"))
        .expect("load sakura-test.ustx");
    let vb = pipeline::load_voicebank(&root.join("test/golden/teto-english/library"))
        .expect("load teto-english bank");

    project.tracks[0].phonemizer = Some("OpenUtau.Plugin.Builtin.UnsupportedEnPhonemizer".to_string());
    let track = &project.tracks[0];
    let part = match &project.parts[0] {
        UPart::Voice(p) => p,
        other => panic!("expected a voice part, got {other:?}"),
    };

    // Ensure RUSTED_UTAU_PHONEMIZER is not forcing default
    std::env::remove_var("RUSTED_UTAU_PHONEMIZER");

    let result = pipeline::build_phrase_inputs_with_choices(
        &project,
        track,
        part,
        &vb,
        PhonemizerKind::Japanese,
        None,
    );

    match result {
        Ok(_) => panic!("Expected error for unsupported phonemizer"),
        Err(err) => {
            assert!(
                err.contains("unsupported phonemizer plugin 'OpenUtau.Plugin.Builtin.UnsupportedEnPhonemizer'"),
                "Unexpected error message: {err}"
            );
            assert!(
                err.contains("OpenUtau.Plugin.Builtin.JapanesePresampPhonemizer"),
                "Error message should mention supported plugin: {err}"
            );
        }
    }
}

#[test]
fn default_override_bypasses_external_phonemizer() {
    let root = repo_root();
    let mut project = pipeline::load_project(&root.join("test/golden/sakura-test.ustx"))
        .expect("load sakura-test.ustx");
    let vb = pipeline::load_voicebank(&root.join("test/golden/teto-english/library"))
        .expect("load teto-english bank");

    project.tracks[0].phonemizer = Some("OpenUtau.Plugin.Builtin.UnsupportedEnPhonemizer".to_string());
    let track = &project.tracks[0];
    let part = match &project.parts[0] {
        UPart::Voice(p) => p,
        other => panic!("expected a voice part, got {other:?}"),
    };

    // Force default via env override
    std::env::set_var("RUSTED_UTAU_PHONEMIZER", "default");

    let result = pipeline::build_phrase_inputs_with_choices(
        &project,
        track,
        part,
        &vb,
        PhonemizerKind::Japanese,
        None,
    );

    std::env::remove_var("RUSTED_UTAU_PHONEMIZER");
    assert!(result.is_ok(), "RUSTED_UTAU_PHONEMIZER=default should bypass external plugin error: {:?}", result.err());
}

#[test]
fn japanese_presamp_runs_end_to_end_with_real_bank_when_available() {
    let root = repo_root();
    let teto_bank = root.join("test-ustx-example/voicebank/extracted/重音テト OU用日本語統合ライブラリー");
    if !teto_bank.is_dir() {
        eprintln!("skipping: real teto bank not present");
        return;
    }
    let host_dll = root.join("native/tools/openutau-phonemizer-host/bin/Release/net8.0/OpenUtauPhonemizerHost.dll");
    if !host_dll.is_file() {
        eprintln!("skipping: OpenUtauPhonemizerHost.dll not present");
        return;
    }

    std::env::set_var("RUSTED_UTAU_OPENUTAU_PHONEMIZER_HOST", host_dll.to_str().unwrap());
    let vb = pipeline::load_voicebank(&teto_bank).expect("load teto bank");
    let mut project = pipeline::load_project(&root.join("test/golden/sakura-test.ustx"))
        .expect("load sakura-test.ustx");

    project.tracks[0].phonemizer = Some(synth_cli::openutau_phonemizer::JAPANESE_PRESAMP.to_string());
    let track = &project.tracks[0];
    let part = match &project.parts[0] {
        UPart::Voice(p) => p,
        other => panic!("expected a voice part, got {other:?}"),
    };

    std::env::remove_var("RUSTED_UTAU_PHONEMIZER");
    let phrases = pipeline::build_phrase_inputs_with_choices(
        &project,
        track,
        part,
        &vb,
        PhonemizerKind::Japanese,
        None,
    ).expect("build phrase inputs with Japanese Presamp");

    assert!(!phrases.is_empty(), "phrases should not be empty");
    let phonemes = &phrases[0].input.phonemes;
    assert!(!phonemes.is_empty(), "phonemes should not be empty");
    println!("Successfully derived {} phrases, first phrase {} phonemes using Japanese Presamp: {:?}",
        phrases.len(),
        phonemes.len(),
        phonemes.iter().map(|p| &p.phoneme).collect::<Vec<_>>()
    );
}
