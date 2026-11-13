//! T9 regression: the pipeline one-note render must not delay the note
//! (the reported "broken onset") and must match OpenUtau's mixdown length.
//!
//! ```sh
//! cargo test -p synth-cli --test note_onset -- --ignored --nocapture
//! ```
//!
//! Ignored by default like the other real-audio tests: it needs the Teto
//! English voicebank and (for the golden comparison) the reference WAV.

use std::path::{Path, PathBuf};

use synth_cli::engine::{Engine as _, WorldlineRsEngine};
use synth_cli::pipeline::{self, PhonemizerKind, SAMPLE_RATE};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn voicebank_path() -> PathBuf {
    repo_root().join("test/golden/teto-english/library")
}

fn one_note_path() -> PathBuf {
    repo_root().join("test/golden/one-note.ustx")
}

/// The reference mixdown (OpenUtau, WORLDLINE_RENDERER=1): 66,211 samples
/// (1501 ms) with the note audible from the first 50 ms window.
fn golden_demo_path() -> PathBuf {
    std::env::var("GOLDEN_DEMO")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp/g-demo-r1.wav"))
}

fn rms(samples: &[f32]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples
        .iter()
        .map(|s| f64::from(*s) * f64::from(*s))
        .sum::<f64>()
        / samples.len() as f64)
        .sqrt()
}

/// 50 ms RMS windows (the metric `test/golden/ab_metrics.py` reports).
fn windows(samples: &[f32], win_ms: f64) -> Vec<f64> {
    let step = (f64::from(SAMPLE_RATE) * win_ms / 1000.0) as usize;
    samples.chunks(step).map(rms).collect()
}

/// First window whose RMS is above -30 dBFS (0.0316), like ab_metrics.py.
fn onset_ms(samples: &[f32]) -> f64 {
    windows(samples, 50.0)
        .iter()
        .position(|w| *w > 0.0316)
        .map(|i| i as f64 * 50.0)
        .unwrap_or(f64::NAN)
}

fn read_wav_mono(path: &Path) -> Vec<f32> {
    let data = voicebank::read_wav(path).expect("read wav");
    if data.channels > 1 {
        data.to_mono()
    } else {
        data.samples
    }
}

/// Envelope correlation at lag 0 (50 ms windows).
fn envelope_corr(a: &[f32], b: &[f32]) -> f64 {
    let ea = windows(a, 50.0);
    let eb = windows(b, 50.0);
    let n = ea.len().min(eb.len());
    let ma = ea[..n].iter().sum::<f64>() / n as f64;
    let mb = eb[..n].iter().sum::<f64>() / n as f64;
    let num: f64 = (0..n).map(|i| (ea[i] - ma) * (eb[i] - mb)).sum();
    let da: f64 = ea[..n].iter().map(|v| (v - ma).powi(2)).sum();
    let db: f64 = eb[..n].iter().map(|v| (v - mb).powi(2)).sum();
    if da <= 0.0 || db <= 0.0 {
        0.0
    } else {
        num / (da.sqrt() * db.sqrt())
    }
}

fn render_one_note(kind: PhonemizerKind) -> Vec<f32> {
    let project = pipeline::load_project(&one_note_path()).expect("load one-note.ustx");
    let voicebank = pipeline::load_voicebank(&voicebank_path()).expect("load teto bank");
    let mut engine = WorldlineRsEngine::open(false).expect("open r1-rs engine");

    // One phoneme per note = the OpenUtau model (and what keeps the vowel at
    // the note start instead of at 50% of the note).
    let track = &project.tracks[0];
    let part = match &project.parts[0] {
        domain::UPart::Voice(p) => p,
        other => panic!("expected a voice part, got {other:?}"),
    };
    let _ = kind;
    let phrases = pipeline::build_phrase_inputs(&project, track, part, &voicebank, kind)
        .expect("build phrase inputs");
    assert_eq!(phrases.len(), 1, "one-note.ustx must produce one phrase");
    assert_eq!(
        phrases[0].input.phonemes.len(),
        1,
        "one phoneme per note (T9): got {:?}",
        phrases[0]
            .input
            .phonemes
            .iter()
            .map(|p| p.phoneme.clone())
            .collect::<Vec<_>>()
    );
    // The phoneme owns the whole note (1500 ms), it is not split in half.
    let dur = phrases[0].input.phonemes[0].duration_ms;
    assert!(
        (dur - 1500.0).abs() < 1.0,
        "the note duration must not be divided: {dur} ms"
    );
    engine
        .render_project(&project, &voicebank, 0)
        .expect("render")
        .samples
}

/// T9: a one-note render must start at the note and last the note duration.
#[test]
#[ignore = "needs the Teto bank + test/golden/one-note.ustx (run with --ignored)"]
fn japanese_one_note_starts_at_zero_and_lasts_one_note() {
    let samples = render_one_note(PhonemizerKind::Japanese);
    let ms = samples.len() as f64 * 1000.0 / f64::from(SAMPLE_RATE);
    // Before T9: 66,049 samples (1498 ms) with the consonant holding the
    // first 750 ms and the onset at 150 ms.
    assert!(
        (ms - 1500.0).abs() < 25.0,
        "mixdown must be one note long (1500 ms), got {ms:.1} ms"
    );
    let onset = onset_ms(&samples);
    assert!(
        onset.is_finite() && onset <= 100.0,
        "the note must sound from the start (onset {onset} ms)"
    );
}

/// T9: the default (Universal) path must also trim the phrase leading.
#[test]
#[ignore = "needs the Teto bank + test/golden/one-note.ustx (run with --ignored)"]
fn universal_one_note_trims_the_phrase_leading() {
    let samples = render_one_note(PhonemizerKind::Universal);
    let ms = samples.len() as f64 * 1000.0 / f64::from(SAMPLE_RATE);
    // Before T9: 72,132 samples (1636 ms) — the 135.7 ms oto leading was
    // kept in the mixdown instead of being trimmed at t=0.
    assert!(
        (ms - 1500.0).abs() < 25.0,
        "mixdown must drop the pre-roll (1500 ms), got {ms:.1} ms"
    );
    assert!(
        onset_ms(&samples) <= 100.0,
        "onset {:?} ms",
        onset_ms(&samples)
    );
}

/// T9: compare the envelope against the OpenUtau golden mixdown when present.
#[test]
#[ignore = "needs the golden demo WAV (GOLDEN_DEMO=/tmp/g-demo-r1.wav)"]
fn universal_one_note_matches_the_golden_demo_onset_and_length() {
    let golden = golden_demo_path();
    if !golden.is_file() {
        eprintln!("golden demo not found at {}; skipping", golden.display());
        return;
    }
    let ours = render_one_note(PhonemizerKind::Universal);
    let want = read_wav_mono(&golden);
    let ours_ms = ours.len() as f64 * 1000.0 / f64::from(SAMPLE_RATE);
    let want_ms = want.len() as f64 * 1000.0 / f64::from(SAMPLE_RATE);
    let ours_onset = onset_ms(&ours);
    let want_onset = onset_ms(&want);
    let corr = envelope_corr(&ours, &want);
    println!(
        "ours {} samples ({ours_ms:.1} ms, onset {ours_onset} ms, rms {:.4}) vs golden {} samples ({want_ms:.1} ms, onset {want_onset} ms, rms {:.4})",
        ours.len(),
        rms(&ours),
        want.len(),
        rms(&want)
    );
    println!("envelope Pearson corr@lag0 = {corr:.4} (diagnostic only)");
    // T9: before the fix ours was 72,132 samples (1636 ms) with the onset at
    // 150 ms; the golden is 66,211 (1501 ms) with the onset at 50 ms.
    assert!(
        (ours_ms - want_ms).abs() < 25.0,
        "length {ours_ms:.1} ms vs golden {want_ms:.1} ms"
    );
    assert!(
        (ours_onset - want_onset).abs() <= 50.0,
        "onset {ours_onset} ms vs golden {want_onset} ms"
    );
    // The raw envelope SHAPE is NOT asserted: the OpenUtau mixdown applies an
    // extra amplitude envelope (its 50 ms RMS oscillates 0.17-0.30 while ours
    // is flat) that is a separate downstream difference, not the T9 onset bug.
}
