//! F0 estimators for the phrase path — port of
//! `worldline/f0/frq_estimator.cpp` (the voicebank's own `.frq` data) and
//! `worldline/f0/pyin_estimator.cpp` (the fallback for samples without one).
//!
//! The reference picks pYIN whenever a sample ships no `.frq`
//! (`PhraseSynth::AddRequest`), so the Rust path uses the same estimator —
//! see [`crate::pyin`] — and a bank without `.frq` files renders the same
//! F0 grid on both sides.

use voicebank::FrqData;

/// WORLD `kFloorF0StoneMask`.
const FLOOR_F0_STONE_MASK: f64 = 40.0;

/// The F0 source attached to an analyzed sample.
pub enum F0Estimator {
    /// No estimator (synthesis-only models).
    None,
    /// `<wav>_wav.frq` pitch data, resampled onto the frame grid.
    Frq(FrqData),
    /// pYIN (libpyin) — the reference's fallback when no `.frq` exists.
    Pyin,
    /// DIO + StoneMask — kept for callers that explicitly want it.
    Dio,
}

impl F0Estimator {
    /// `F0Estimator::Estimate` — returns `(f0, time_axis)`.
    pub fn estimate(&self, samples: &[f64], fs: i32, frame_ms: f64) -> (Vec<f64>, Vec<f64>) {
        match self {
            F0Estimator::None => (Vec::new(), Vec::new()),
            F0Estimator::Frq(frq) => estimate_frq(frq, samples.len(), fs, frame_ms),
            F0Estimator::Pyin => estimate_pyin(samples, fs, frame_ms),
            F0Estimator::Dio => estimate_dio(samples, fs, frame_ms),
        }
    }
}

/// `FrqEstimator::Estimate`: a DIO-sized grid where each frame averages the
/// `.frq` values above the voiced floor (0 when the window has none).
fn estimate_frq(
    frq: &FrqData,
    sample_count: usize,
    fs: i32,
    frame_ms: f64,
) -> (Vec<f64>, Vec<f64>) {
    // GetSamplesForDIO: floor(1000 * length / fs / frame_period) + 1
    let f0_len = if sample_count == 0 {
        0
    } else {
        (1000.0 * sample_count as f64 / f64::from(fs) / frame_ms) as usize + 1
    };
    let mut f0 = vec![0.0f64; f0_len];
    let mut ts = vec![0.0f64; f0_len];
    let hop_size = f64::from(fs) * frame_ms / 1000.0;
    let frq_hop = if frq.hop_size > 0 {
        f64::from(frq.hop_size)
    } else {
        1.0
    };
    let ratio = hop_size / frq_hop;
    for i in 0..f0_len {
        let low = (ratio * i as f64).floor().max(0.0) as usize;
        let high = ((ratio * (1 + i) as f64).ceil() as usize).min(frq.f0.len());
        let (mut sum, mut count) = (0.0f64, 0usize);
        for j in low..high.max(low) {
            if frq.f0[j] > FLOOR_F0_STONE_MASK {
                sum += frq.f0[j];
                count += 1;
            }
        }
        f0[i] = if count > 0 { sum / count as f64 } else { 0.0 };
        ts[i] = i as f64 * frame_ms / 1000.0;
    }
    (f0, ts)
}

/// `PyinEstimator::Estimate` — pYIN on a hop-sized grid, shifted left by one
/// frame (the reference drops the first frame: `copy(raw + 1, raw + f0_len)`).
fn estimate_pyin(samples: &[f64], fs: i32, frame_ms: f64) -> (Vec<f64>, Vec<f64>) {
    let nhop = ((f64::from(fs) * frame_ms / 1000.0).round() as i32).max(1);
    let config = crate::pyin::PyinConfig::new(nhop);
    let raw = crate::pyin::analyze(&config, samples, f64::from(fs));
    if raw.len() <= 1 {
        return (Vec::new(), Vec::new());
    }
    let f0: Vec<f64> = raw[1..].to_vec();
    let ts: Vec<f64> = (0..f0.len())
        .map(|i| f64::from(nhop) * i as f64 / f64::from(fs))
        .collect();
    (f0, ts)
}

/// DIO + StoneMask (the alternative estimator).
fn estimate_dio(samples: &[f64], fs: i32, frame_ms: f64) -> (Vec<f64>, Vec<f64>) {
    let mono: Vec<f32> = samples.iter().map(|s| *s as f32).collect();
    let option = world_rs::f0::DioOption {
        frame_period_ms: frame_ms,
        ..Default::default()
    };
    match world_rs::f0::dio(&mono, fs as u32, option, &()) {
        Ok((mut f0, ts)) => {
            world_rs::f0::stone_mask(&mono, fs as u32, &ts, &mut f0);
            (f0, ts)
        }
        Err(_) => (Vec::new(), Vec::new()),
    }
}
