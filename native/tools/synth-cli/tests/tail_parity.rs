//! T21: the LAST chunk of a phrase must keep the renderer's own tail.
//!
//! `trim_chunk_audio` used to cap the last chunk at the plan-derived
//! `chunk.end_ms` (built from phoneme spans, engine independent), which
//! truncated the tail fade-out: r1 lost 61 samples and r2 lost 511 (= hop-1),
//! making our r2 the same length as our r1 (T20/T21).
//!
//! ```sh
//! cargo test -p synth-cli --test tail_parity
//! cargo test -p synth-cli --test tail_parity -- --ignored --nocapture   # needs .so + models/r2
//! ```

use std::path::{Path, PathBuf};

use synth_cli::chunking;
use synth_cli::pipeline::{self, PhonemizerKind, SAMPLE_RATE};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn one_note_path() -> PathBuf {
    repo_root().join("test/golden/one-note.ustx")
}

fn bank_path() -> PathBuf {
    repo_root().join("test/golden/teto-english/library")
}

/// The last chunk's audio length must follow the RENDER, not the plan.
#[test]
fn last_chunk_keeps_the_renderer_tail() {
    let project = pipeline::load_project(&one_note_path()).expect("load one-note.ustx");
    let voicebank = pipeline::load_voicebank(&bank_path()).expect("load teto bank");
    let track = &project.tracks[0];
    let part = match &project.parts[0] {
        domain::UPart::Voice(p) => p,
        other => panic!("expected a voice part, got {other:?}"),
    };
    let phrases =
        pipeline::build_phrase_inputs(&project, track, part, &voicebank, PhonemizerKind::Universal)
            .expect("build phrase inputs");
    assert_eq!(phrases.len(), 1, "one-note.ustx must produce one phrase");
    let phrase = &phrases[0];
    let chunks = chunking::plan_chunks(
        &phrase.input.phonemes,
        &phrase.input.notes,
        pipeline::CHUNK_TARGET_MS,
        pipeline::CHUNK_CONTEXT_NOTES,
    );
    assert!(
        !chunks.is_empty(),
        "the phrase must plan at least one chunk"
    );
    let last = chunks.last().unwrap();
    let audio_start_ms = phrase.input.render_origin_ms;
    let sr = f64::from(SAMPLE_RATE) / 1000.0;
    let plan_len = ((last.end_ms - audio_start_ms) * sr) as usize;
    // A render that runs past the plan, exactly like the real engines
    // (r2 = +511 = hop-1, r1 = +61).
    let rendered = vec![0.25f32; plan_len + 511];
    let (samples, position_ms, leading_ms) =
        pipeline::trim_chunk_audio(phrase, last, phrase.input.render_origin_ms, &rendered);
    let start_i = (((last.start_ms - leading_ms) - audio_start_ms) * sr).max(0.0) as usize;
    assert_eq!(
        samples.len(),
        rendered.len() - start_i,
        "the last chunk must keep the renderer's tail (plan_len {plan_len}, rendered {})",
        rendered.len()
    );
    assert!(
        samples.len() > plan_len - start_i,
        "tail was truncated at the plan end"
    );
    assert_eq!(position_ms, last.start_ms);
}

/// E2E (ignored): one-note through r1 vs r2 must differ by the r2 extra tail
/// (511 samples = hop - 1) and each must match the OpenUtau golden length.
#[test]
#[ignore = "needs libworldline.so + models/r2 (run with --ignored)"]
fn one_note_r2_is_511_samples_longer_than_r1() {
    use synth_cli::engine::{Engine as _, WorldlineR2Engine, WorldlineRsEngine};

    let root = repo_root();
    let so = std::env::var("WORLDLINE_SO")
        .map(PathBuf::from)
        .unwrap_or_else(|_| root.join("native/build/build-linux/libworldline.so"));
    let mel = std::env::var("MEL_ONNX")
        .map(PathBuf::from)
        .unwrap_or_else(|_| root.join("models/r2/mel.onnx"));
    let vocoder = std::env::var("VOCODER_ONNX")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            root.join("models/r2/pc-nsf-hifigan/pc_nsf_hifigan_44.1k_hop512_128bin_2025.02.onnx")
        });
    if !mel.is_file() || !vocoder.is_file() {
        eprintln!(
            "models/r2 not present (mel={} vocoder={}); skipping",
            mel.display(),
            vocoder.display()
        );
        return;
    }
    let project = pipeline::load_project(&one_note_path()).expect("load one-note.ustx");
    let voicebank = pipeline::load_voicebank(&bank_path()).expect("load teto bank");

    let mut r1 = WorldlineRsEngine::open(false).expect("open r1-rs");
    let r1_len = r1
        .render_project(&project, &voicebank, 0)
        .expect("render r1")
        .samples
        .len();

    let mut r2 = WorldlineR2Engine::open(&so, mel, vocoder, false).expect("open r2");
    let r2_len = r2
        .render_project(&project, &voicebank, 0)
        .expect("render r2")
        .samples
        .len();

    println!(
        "r1 {r1_len} samples · r2 {r2_len} samples · diff {}",
        r2_len as i64 - r1_len as i64
    );
    // Golden lengths (out/ab/golden-openutau-r{1,2}-note-VALID.wav): 66,211 / 66,722.
    assert_eq!(r1_len, 66211, "r1 mixdown must match the golden r1 length");
    assert_eq!(r2_len, 66722, "r2 mixdown must match the golden r2 length");
    assert_eq!(r2_len - r1_len, 511, "the r2 tail is hop - 1 samples");
}
