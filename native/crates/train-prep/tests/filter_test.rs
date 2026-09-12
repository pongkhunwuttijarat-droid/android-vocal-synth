//! Quality filter tests (plan Task 2).

use train_prep::filter::{apply, passes};
use train_prep::{PhonemeTiming, TrainEntry};

fn entry(frames: usize, voiced_ratio: f64, mel_std: f32) -> TrainEntry {
    let n_voiced = (frames as f64 * voiced_ratio) as usize;
    let f0_hz: Vec<f32> = (0..frames)
        .map(|i| if i < n_voiced { 261.6 } else { 0.0 })
        .collect();
    let mel = (0..frames)
        .map(|_| {
            (0..128)
                .map(|b| (b as f32 / 128.0) * mel_std)
                .collect::<Vec<f32>>()
        })
        .collect();
    TrainEntry {
        aliases: vec!["a".to_string()],
        tone: 60,
        duration_ms: 500.0,
        phonemes: vec![PhonemeTiming {
            phoneme: "a".to_string(),
            position_ms: 0.0,
            duration_ms: 500.0,
        }],
        frames,
        f0_hz,
        mel,
    }
}

#[test]
fn good_entry_passes() {
    let e = entry(44, 0.98, 1.0); // ~500ms, mostly voiced, non-flat mel
    assert!(passes(&e));
}

#[test]
fn too_short_rejected() {
    let e = entry(20, 0.98, 1.0); // < 30 frames
    assert!(!passes(&e));
}

/// Unvoiced-only entries are KEPT: fricatives (/s/, /sh/) are valid singing
/// phonemes and their f0=0 frames are part of the acoustic target. Dropping
/// them collapsed those symbols to PAD at inference. (The gate used to
/// require a voiced ratio >= 0.3.)
#[test]
fn unvoiced_entry_passes() {
    let e = entry(60, 0.1, 1.0); // low voiced ratio, but valid audio
    assert!(passes(&e));
}

#[test]
fn flat_mel_rejected() {
    let e = entry(60, 0.98, 0.001); // std < 0.05 → silence/noise
    assert!(!passes(&e));
}

#[test]
fn apply_counts_each_reason() {
    let good = entry(60, 0.98, 1.0);
    let short = entry(20, 0.98, 1.0);
    let unvoiced = entry(60, 0.1, 1.0);
    let flat = entry(60, 0.98, 0.001);
    let entries = vec![good, short, unvoiced, flat];
    let (out, stats) = apply(&entries);
    // good + unvoiced survive (fricatives are kept); short + flat are cut.
    assert_eq!(out.len(), 2);
    assert_eq!(stats.total, 4);
    assert_eq!(stats.passed, 2);
    assert_eq!(stats.too_short, 1);
    assert_eq!(stats.unvoiced, 0, "unvoiced entries are no longer filtered");
    assert_eq!(stats.flat_mel, 1);
}
