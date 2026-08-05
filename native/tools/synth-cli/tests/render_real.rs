//! Real-audio integration tests against the built `libworldline.so`
//! (ignored by default — they need the prebuilt library and the golden
//! Teto English voicebank):
//!
//! ```sh
//! WORLDLINE_SO=.../native/build/build-linux/libworldline.so \
//!     cargo test -p synth-cli --test render_real -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};

use synth_cli::pipeline::{self, PhonemizerKind, SAMPLE_RATE};
use wavwriter::write_wav_16;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn so_path() -> PathBuf {
    if let Ok(path) = std::env::var("WORLDLINE_SO") {
        return PathBuf::from(path);
    }
    let path = repo_root().join("native/build/build-linux/libworldline.so");
    assert!(
        path.exists(),
        "built libworldline.so not found at {path:?}; set WORLDLINE_SO"
    );
    path
}

fn teto_voicebank_path() -> PathBuf {
    repo_root().join("test/golden/teto-english/library")
}

fn mock_voicebank_path() -> PathBuf {
    repo_root().join("native/test-data/mock-voicebank")
}

fn mock_song_path() -> PathBuf {
    repo_root().join("native/test-data/mock-song.ustx")
}

fn demo_song_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/demo-song.ustx")
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |peak, &s| peak.max(s.abs()))
}

/// THE first-sound test: demo-song (Teto-compatible hints) through the
/// real .so with the full Teto English bank. Non-silent audio, > 1 s, and
/// the wavwriter round-trip must decode to the same sample count.
#[test]
#[ignore = "requires libworldline.so; run with WORLDLINE_SO=... cargo test -p synth-cli --test render_real -- --ignored --nocapture"]
fn renders_demo_song_with_teto_bank_to_audio() {
    let project = pipeline::load_project(&demo_song_path()).expect("load demo-song");
    let voicebank = pipeline::load_voicebank(&teto_voicebank_path()).expect("load teto bank");
    let renderer = worldline_plugin::WorldlineRenderer::open(so_path()).expect("open renderer");

    let report = pipeline::render_project(
        &project,
        &voicebank,
        &renderer,
        0,
        PhonemizerKind::English,
        true,
        &mut None,
        None,
        None,
    )
    .expect("render project");
    assert_eq!(
        report.skipped,
        Vec::<String>::new(),
        "nothing may be skipped"
    );
    assert_eq!(report.phrases_rendered, 2, "one phrase → 2 time-based chunks");
    assert!(!report.samples.is_empty(), "synth produced no samples");

    let duration_ms = report.samples.len() as f64 * 1000.0 / f64::from(SAMPLE_RATE);
    let p = peak(&report.samples);
    println!(
        "demo-song: {} samples ({duration_ms:.1} ms), peak {p:.4}",
        report.samples.len()
    );
    assert!(
        duration_ms > 1000.0,
        "expected >1 s of audio, got {duration_ms:.1} ms"
    );
    assert!(p > 1e-4, "output is silent (peak {p})");

    // The wavwriter round-trip: 16-bit PCM encodes the same length and
    // decodes back (within quantization) via the voicebank reader.
    let wav = write_wav_16(&report.samples, SAMPLE_RATE);
    let parsed = voicebank::parse_wav(&wav).expect("parse written wav");
    assert_eq!(parsed.samples.len(), report.samples.len());
    assert_eq!(parsed.sample_rate, SAMPLE_RATE);
    println!(
        "wav round-trip: {} bytes, {} samples",
        wav.len(),
        parsed.samples.len()
    );
}

/// The `synth-note` quick path with the mock voicebank: one "A" phoneme
/// must produce ~500 ms of non-silent audio.
#[test]
#[ignore = "requires libworldline.so; run with WORLDLINE_SO=... cargo test -p synth-cli --test render_real -- --ignored --nocapture"]
fn synth_note_renders_mock_bank_phoneme() {
    let voicebank = pipeline::load_voicebank(&mock_voicebank_path()).expect("load mock bank");
    let renderer = worldline_plugin::WorldlineRenderer::open(so_path()).expect("open renderer");

    let samples = pipeline::synth_note(&voicebank, &renderer, "A", 60, 500.0).expect("synth note");
    assert!(!samples.is_empty());
    let duration_ms = samples.len() as f64 * 1000.0 / f64::from(SAMPLE_RATE);
    let p = peak(&samples);
    println!(
        "synth-note A: {} samples ({duration_ms:.1} ms), peak {p:.4}",
        samples.len()
    );
    assert!(
        duration_ms > 400.0,
        "expected ~500 ms, got {duration_ms:.1} ms"
    );
    assert!(p > 1e-4, "output is silent (peak {p})");
}

/// Honesty check for the milestone report: mock-song's ARPABET hints do
/// not exist in the Teto bank (nor the mock bank), so `render` must
/// report the skip cleanly — not crash — and produce no audio.
#[test]
#[ignore = "requires libworldline.so; run with WORLDLINE_SO=... cargo test -p synth-cli --test render_real -- --ignored --nocapture"]
fn mock_song_with_teto_bank_is_reported_as_skipped() {
    let project = pipeline::load_project(&mock_song_path()).expect("load mock-song");
    let voicebank = pipeline::load_voicebank(&teto_voicebank_path()).expect("load teto bank");
    let renderer = worldline_plugin::WorldlineRenderer::open(so_path()).expect("open renderer");

    let report = pipeline::render_project(
        &project,
        &voicebank,
        &renderer,
        0,
        PhonemizerKind::English,
        false,
        &mut None,
        None,
        None,
    )
    .expect("render project must not crash");
    assert!(report.samples.is_empty());
    assert_eq!(report.skipped.len(), 1, "one phrase, one skip reason");
    println!("skip reason: {}", report.skipped[0]);
    // 3 of mock-song's 9 phonemes (d, d, l) do exist in the Teto bank, so
    // the phrase partially maps and the renderer's 1:1 oto↔phoneme
    // contract rejects it — reported, not crashed.
    assert!(report.skipped[0].contains("render failed"));
}

/// Partial resolution: demo-song hints phonemize to s/i/l/e/d/3/A — only
/// "A" exists in the mock bank, so the renderer's 1:1 oto↔phoneme
/// contract rejects the phrase (reported, not crashed).
#[test]
#[ignore = "requires libworldline.so; run with WORLDLINE_SO=... cargo test -p synth-cli --test render_real -- --ignored --nocapture"]
fn demo_song_with_mock_bank_partially_resolves_and_is_reported() {
    let project = pipeline::load_project(&demo_song_path()).expect("load demo-song");
    let voicebank = pipeline::load_voicebank(&mock_voicebank_path()).expect("load mock bank");
    let renderer = worldline_plugin::WorldlineRenderer::open(so_path()).expect("open renderer");

    let report = pipeline::render_project(
        &project,
        &voicebank,
        &renderer,
        0,
        PhonemizerKind::English,
        false,
        &mut None,
        None,
        None,
    )
    .expect("render project must not crash");
    assert!(report.samples.is_empty());
    assert_eq!(report.skipped.len(), 1);
    println!("skip reason: {}", report.skipped[0]);
    assert!(report.skipped[0].contains("render failed"));
}

/// The track's phonemizer name picks the phonemizer kind.
#[test]
fn phonemizer_kind_from_track_name() {
    let mut track = domain::UTrack::new("Track1");
    assert_eq!(PhonemizerKind::from_track(&track), PhonemizerKind::English);
    track.phonemizer = Some("OpenUtau Japanese VCV Phonemizer".into());
    assert_eq!(PhonemizerKind::from_track(&track), PhonemizerKind::Japanese);
    track.phonemizer = Some("EnglishVCCVPhonemizer".into());
    assert_eq!(PhonemizerKind::from_track(&track), PhonemizerKind::English);
}
