//! Quality filter (plan Task 2): drop garbage entries before they reach
//! the training set — oto-mapped aliases can still render weird phrases
//! (silence, broken samples, flat mels).

use crate::TrainEntry;

/// Filter criteria (plan defaults):
/// - frames >= 30 (≥ ~350 ms of audio — too-short phrases carry nothing)
/// - unvoiced-only phrases are retained: fricatives are valid singing
///   phonemes and their f0=0 frames are part of the acoustic target
/// - mel not flat (std > 0.05 — a flat mel means silence/noise)
#[derive(Debug, Clone, Copy, Default)]
pub struct FilterStats {
    pub total: usize,
    pub passed: usize,
    pub too_short: usize,
    pub unvoiced: usize,
    pub flat_mel: usize,
}

/// Does this entry pass the quality gate?
pub fn passes(entry: &TrainEntry) -> bool {
    if entry.frames < 30 {
        return false;
    }

    // mel std across all bins — flat → silence/noise.
    let n = entry.mel.len() * 128;
    if n == 0 {
        return false;
    }
    let mean: f64 = entry.mel.iter().flatten().map(|&v| v as f64).sum::<f64>() / n as f64;
    let var: f64 = entry
        .mel
        .iter()
        .flatten()
        .map(|&v| (v as f64 - mean).powi(2))
        .sum::<f64>()
        / n as f64;
    var.sqrt() > 0.05
}

/// Apply the filter to a slice of entries, returning the survivors.
pub fn apply(entries: &[TrainEntry]) -> (Vec<TrainEntry>, FilterStats) {
    let mut stats = FilterStats {
        total: entries.len(),
        ..Default::default()
    };
    let mut out = Vec::with_capacity(entries.len());
    for e in entries {
        if e.frames < 30 {
            stats.too_short += 1;
            continue;
        }
        // Keep unvoiced consonants (e.g. /s/, /sh/) in the Universal
        // vocabulary. Their zero-f0 frames teach the student the noise
        // envelope instead of collapsing the symbol to PAD at inference.
        let n = e.mel.len() * 128;
        let mean: f64 = e.mel.iter().flatten().map(|&v| v as f64).sum::<f64>() / n as f64;
        let var: f64 = e
            .mel
            .iter()
            .flatten()
            .map(|&v| (v as f64 - mean).powi(2))
            .sum::<f64>()
            / n as f64;
        if var.sqrt() <= 0.05 {
            stats.flat_mel += 1;
            continue;
        }
        out.push(e.clone());
        stats.passed += 1;
    }
    (out, stats)
}
