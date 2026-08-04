//! Pipeline integration tests (no `.so` needed): ustx → phonemes →
//! phrase groups → `RenderInput`, plus the oto-mapping gate that decides
//! whether the worldline renderer can produce audio.
//!
//! The real-audio counterparts (gated on `WORLDLINE_SO`) live in
//! `tests/render_real.rs`.

use std::path::{Path, PathBuf};

use domain::UPart;
use synth_cli::pipeline::{self, PhonemizerKind};

/// Repo root, resolved from `native/tools/synth-cli` (CARGO_MANIFEST_DIR).
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn mock_song_path() -> PathBuf {
    repo_root().join("native/test-data/mock-song.ustx")
}

fn mock_voicebank_path() -> PathBuf {
    repo_root().join("native/test-data/mock-voicebank")
}

fn demo_song_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/demo-song.ustx")
}

fn teto_voicebank_path() -> PathBuf {
    repo_root().join("test/golden/teto-english/library")
}

#[test]
fn mock_song_derives_nine_phonemes_in_one_phrase_without_oto_mapping() {
    let project = pipeline::load_project(&mock_song_path()).expect("load mock-song");
    let voicebank = pipeline::load_voicebank(&mock_voicebank_path()).expect("load mock bank");
    let track = &project.tracks[0];
    let part = match &project.parts[0] {
        UPart::Voice(part) => part,
        UPart::Wave(_) => panic!("expected a voice part"),
    };

    let phrases =
        pipeline::build_phrase_inputs(&project, track, part, &voicebank, PhonemizerKind::English)
            .expect("build phrase inputs");
    // One phrase of 9 phonemes, like the phrase crate's mock_song test.
    assert_eq!(phrases.len(), 1);
    assert_eq!(phrases[0].input.phonemes.len(), 9);
    assert!(
        phrases[0].input.sample_based.is_none(),
        "mock-song phonemes (r iy d eh a l ae) do not map in the mock bank \
         (aliases 3/A/aI/...), so the sample-based section must stay empty"
    );
}

#[test]
fn demo_song_with_teto_bank_maps_every_phoneme_to_an_oto_entry() {
    let project = pipeline::load_project(&demo_song_path()).expect("load demo-song");
    let voicebank = pipeline::load_voicebank(&teto_voicebank_path()).expect("load teto bank");
    let track = &project.tracks[0];
    let part = match &project.parts[0] {
        UPart::Voice(part) => part,
        UPart::Wave(_) => panic!("expected a voice part"),
    };

    let phrases =
        pipeline::build_phrase_inputs(&project, track, part, &voicebank, PhonemizerKind::English)
            .expect("build phrase inputs");
    assert_eq!(phrases.len(), 1);
    let input = &phrases[0].input;
    // see[s i] led[l e d] ah[3] la[l A] → 8 hint tokens; greedy pairing
    // against the Teto oto table may merge adjacent tokens into 2-token
    // aliases (e.g. "i l", "e d", "3 l" all exist), so the count is
    // 5..=8 — what matters for the render gate is that every emitted
    // phoneme maps to an oto entry.
    let names: Vec<&str> = input.phonemes.iter().map(|p| p.phoneme.as_str()).collect();
    println!("phonemes: {names:?}");
    assert!(
        (5..=8).contains(&input.phonemes.len()),
        "unexpected phoneme count: {names:?}"
    );

    let sample_based = input
        .sample_based
        .as_ref()
        .expect("every phoneme maps to an oto entry");
    assert_eq!(sample_based.oto.len(), input.phonemes.len());
    // The referenced wavs must actually exist on disk.
    for entry in &sample_based.oto {
        assert!(
            Path::new(&entry.wav_path).exists(),
            "oto wav missing: {}",
            entry.wav_path
        );
    }
}

#[test]
fn synth_note_phoneme_without_oto_entry_is_rejected_with_aliases() {
    let voicebank = pipeline::load_voicebank(&mock_voicebank_path()).expect("load mock bank");
    // "r" does not exist in the mock bank (3/A/aI/aU/...); the error must
    // list what does. No renderer is needed to reach this failure.
    let err = pipeline::synth_note_validate(&voicebank, "r", 60, 500.0).expect_err("must fail");
    assert!(err.contains("no oto entry"), "unexpected error: {err}");
    assert!(
        err.contains("3"),
        "error should list available aliases: {err}"
    );
}

#[test]
fn synth_note_phoneme_with_oto_entry_builds_a_mapped_phrase() {
    let voicebank = pipeline::load_voicebank(&mock_voicebank_path()).expect("load mock bank");
    // "A" maps in the mock bank (preutter 20, overlap 40, wav
    // _a+_ha+_a+_a+_a+-.wav): the synthetic note's phrase must carry a
    // complete sample-based section.
    let phrase = pipeline::synth_note_input(&voicebank, "A", 60, 500.0).expect("build note");
    assert_eq!(phrase.input.phonemes.len(), 1);
    assert_eq!(phrase.input.phonemes[0].phoneme, "A");
    let sample_based = phrase
        .input
        .sample_based
        .as_ref()
        .expect("A maps in the mock bank");
    assert_eq!(sample_based.oto.len(), 1);
    assert_eq!(sample_based.oto[0].alias, "A");
    assert!(
        Path::new(&sample_based.oto[0].wav_path).exists(),
        "oto wav missing: {}",
        sample_based.oto[0].wav_path
    );
}
