//! Integration tests against the real `libworldline.so` (ignored by
//! default — they need the prebuilt library).
//!
//! ```sh
//! WORLDLINE_SO=.../native/build/build-linux/libworldline.so \
//!     cargo test -p worldline-plugin -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};

use feed::render_input::{
    Curves, OtoEntry, PhraseInfo, RenderInput, RenderNote, RenderPhoneme, SampleBased,
};

const DEFAULT_SO: &str =
    "/home/seal/project/android-voice-synth/native/build/build-linux/libworldline.so";
const SAMPLE_RATE: u32 = 44100;

fn so_path() -> PathBuf {
    if let Ok(path) = std::env::var("WORLDLINE_SO") {
        return PathBuf::from(path);
    }
    let path = PathBuf::from(DEFAULT_SO);
    assert!(
        path.exists(),
        "built libworldline.so not found at {path:?}; set WORLDLINE_SO"
    );
    path
}

fn mock_wav_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/mock-voicebank/voice/_3_h3_3-.wav")
}

/// One "3 h3" phoneme at C4, 120 bpm, 300 ms, from the mock voicebank.
fn mock_input() -> RenderInput {
    let grid_len = 107; // (240 leading + 288 duration ticks) / 5 + 1
    RenderInput {
        phrase: PhraseInfo {
            position_ms: 500.0,
            duration_ms: 300.0,
            leading_ms: 250.0,
            leading_ticks: 240,
            time_axis_hint: Some("120bpm 4/4".into()),
        },
        notes: vec![RenderNote {
            lyric: "3".into(),
            tone: 60,
            position_ms: 500.0,
            duration_ms: 300.0,
        }],
        phonemes: vec![RenderPhoneme {
            phoneme: "3 h3".into(),
            position_ms: 500.0,
            duration_ms: 300.0,
            leading_ms: 250.0,
            overlap_ms: 83.333,
            tone: 60,
            tone_shift: 0,
            index: 0,
            parent_note: 0,
        }],
        pitches_cents: vec![6000; grid_len],
        curves: Curves {
            dynamics: vec![1.0; grid_len],
            gender: vec![0.0; grid_len],
            breathiness: vec![0.0; grid_len],
            tension: vec![0.0; grid_len],
            voicing: vec![100.0; grid_len],
            extra: Vec::new(),
        },
        sample_based: Some(SampleBased {
            oto: vec![OtoEntry {
                alias: "3 h3".into(),
                file: "_3_h3_3-.wav".into(),
                wav_path: mock_wav_path().to_string_lossy().into_owned(),
                offset: 442.3,
                consonant: 375.0,
                cutoff: -583.333,
                preutter: 250.0,
                overlap: 83.333,
                envelope: Vec::new(),
                flags: String::new(),
            }],
            wav_path: Some(mock_wav_path().to_string_lossy().into_owned()),
            envelope: Vec::new(),
            flags: String::new(),
        }),
        neural: None,
    }
}

/// Render one phoneme from the mock voicebank through the real .so and
/// check that non-silent PCM comes out; the second render must also work
/// (a fresh PhraseSynth is spawned per phrase).
#[test]
#[ignore = "requires the built libworldline.so; run with WORLDLINE_SO=... cargo test -- --ignored --nocapture"]
fn renders_one_phoneme_to_pcm() {
    let path = so_path();
    println!("loading {path:?}");
    let renderer = worldline_plugin::WorldlineRenderer::open(&path).expect("open renderer");

    let samples = renderer
        .render_phrase(&mock_input())
        .expect("render phrase");
    println!(
        "samples: {} ({} ms), peak: {:.4}",
        samples.len(),
        samples.len() as f64 * 1000.0 / SAMPLE_RATE as f64,
        samples.iter().fold(0.0f32, |peak, &s| peak.max(s.abs()))
    );
    assert!(!samples.is_empty(), "synth produced no samples");
    assert!(
        samples.len() > SAMPLE_RATE as usize / 10,
        "expected at least 100 ms of audio, got {} samples",
        samples.len()
    );
    let peak = samples.iter().fold(0.0f32, |peak, &s| peak.max(s.abs()));
    assert!(peak > 1e-4, "output is silent (peak {peak})");

    // A second render must produce nearly identical output (fresh
    // PhraseSynth, same input). NOT bit-exact: the C++ WORLD/pyin DSP
    // reads some uninitialized heap memory (values depend on heap
    // layout), so output varies ~2% peak between runs/threads — the
    // same reason OpenUtau golden tests use tolerances, not ==.
    let again = renderer
        .render_phrase(&mock_input())
        .expect("second render");
    assert_eq!(again.len(), samples.len());
    let max_diff = again
        .iter()
        .zip(&samples)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    let mean_diff = again
        .iter()
        .zip(&samples)
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / again.len() as f32;
    let peak = samples.iter().fold(0.0f32, |peak, &s| peak.max(s.abs()));
    println!("determinism: max_diff={max_diff:.2e} mean_diff={mean_diff:.2e} peak={peak:.4}");
    // Same input → same output within DSP noise (~2% of peak).
    assert!(
        max_diff < peak * 0.05,
        "two renders of the same input differ beyond DSP noise \
         (max_diff={max_diff:.2e}, peak={peak:.4})"
    );
}

/// The pure conversion half must agree with the .so-backed half on the
/// request fields the .so consumes.
#[test]
#[ignore = "requires the built libworldline.so; run with WORLDLINE_SO=... cargo test -- --ignored --nocapture"]
fn conversion_and_render_agree() {
    let input = mock_input();
    let requests = worldline_plugin::build_requests(&input).expect("build requests");
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    println!(
        "request: tone={} con_vel={} offset={} required_length={} consonant={} cut_off={} volume={} bend_len={}",
        request.tone,
        request.con_vel,
        request.offset,
        request.required_length,
        request.consonant,
        request.cut_off,
        request.volume,
        request.pitch_bend.len()
    );
    // smoke.c invariant: length_ms must not exceed required_length.
    assert!(request.length_ms <= request.required_length);
    assert_eq!(request.tone, 60);
    assert_eq!(request.con_vel, 100.0);
    assert_eq!(request.volume, 100.0);
    assert_eq!(request.flag_P, 86);
    assert_eq!(request.flag_Mv, 100);
    assert!(!request.pitch_bend.is_empty());
    assert!(request.pitch_bend.iter().all(|&b| b == 0));

    let renderer = worldline_plugin::WorldlineRenderer::open(so_path()).expect("open renderer");
    let samples = renderer.render_phrase(&input).expect("render phrase");
    assert!(!samples.is_empty());
}
