//! F0 derivation for neural renderers: pitches → f0 Hz, then key-shifted.
//!
//! Chain (feed-data-flow.md §3): `pitches[] (cents, per 5 ticks)` →
//! `ToneToFreq(cents/100)` → `f0[] (Hz/frame)` → `× 2^(toneShift/12)` →
//! `shiftedF0`. Frame sampling follows `DiffSingerPitch`: the f0 grid
//! starts `head_frames × frame_ms` before the first phoneme.

use domain::TimeAxis;

use crate::music_math::{cents_to_freq, shift_freq};

/// Per-frame f0 + key-shifted f0.
#[derive(Debug, Clone, PartialEq)]
pub struct F0Result {
    pub f0_hz: Vec<f64>,
    pub shifted_f0_hz: Vec<f64>,
}

/// Build `total_frames` f0 samples from the per-5-tick pitch grid.
///
/// * `pitches` — cents per 5 ticks (from [`crate::PitchComputer`]).
/// * `grid_start_ms` — project-absolute ms of the grid's first sample
///   (first phoneme `position_ms - leading_ms`). Kept for API symmetry
///   with the curve grid; frame positions derive from `start_ms`.
/// * `grid_start_tick` — project-absolute tick of the grid's first sample.
/// * `frame_ms` — frame size in ms.
/// * `start_ms` — project-absolute ms of frame 0 (DiffSinger: first
///   phoneme `position_ms - head_frames × frame_ms`).
/// * `total_frames` — number of frames (sum of `durations_frames`).
/// * `tone_shifts` — per-frame semitone shifts (same length as
///   `total_frames`), typically from the `shft` expression of the phoneme
///   covering each frame.
/// * `time_axis` — project time axis.
#[allow(clippy::too_many_arguments)] // mirrors the C# feed pipeline arity
pub fn compute_f0(
    pitches: &[f32],
    _grid_start_ms: f64,
    grid_start_tick: i32,
    frame_ms: f64,
    start_ms: f64,
    total_frames: usize,
    tone_shifts: &[f64],
    time_axis: &TimeAxis,
) -> F0Result {
    let per_frame = crate::curve::sample_per_frame(
        pitches,
        start_ms,
        grid_start_tick,
        frame_ms,
        total_frames,
        time_axis,
    );
    let f0_hz: Vec<f64> = per_frame
        .iter()
        .map(|&cents| cents_to_freq(cents as f64))
        .collect();
    let shifted_f0_hz: Vec<f64> = f0_hz
        .iter()
        .zip(tone_shifts.iter().copied())
        .map(|(&f, shift)| shift_freq(f, shift))
        .collect();
    F0Result { f0_hz, shifted_f0_hz }
}

/// Default DiffSinger frame size: `1000 × hop_size / sample_rate` with
/// hop_size 512 at 44100 Hz ≈ 11.61 ms (the common `frameMs` default).
pub const fn default_frame_ms() -> f64 {
    1000.0 * 512.0 / 44100.0
}

/// Head padding frames of the DiffSinger convention.
pub const HEAD_FRAMES: i64 = 8;
/// Tail padding frames of the DiffSinger convention.
pub const TAIL_FRAMES: i64 = 8;

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{UTempo, UTimeSignature};

    fn axis_120bpm() -> TimeAxis {
        let mut axis = TimeAxis::default();
        axis.build_segments(&[UTimeSignature::new(0, 4, 4)], &[UTempo::new(0, 120.0)])
            .expect("build segments");
        axis
    }

    #[test]
    fn flat_pitches_map_to_expected_frequencies() {
        let axis = axis_120bpm();
        // 3 frames of a flat C4 (6000 ct), then 3 of E4 (6400 ct).
        let pitches = vec![6000.0, 6000.0, 6000.0, 6400.0, 6400.0, 6400.0];
        let shifts = vec![0.0; 6];
        let f0 = compute_f0(&pitches, 0.0, 0, 5.2, 0.0, 6, &shifts, &axis);
        let c4 = crate::music_math::tone_to_freq(60.0);
        let e4 = crate::music_math::tone_to_freq(64.0);
        assert!((f0.f0_hz[0] - c4).abs() < 1e-9);
        // Frame 3 at 15.6 ms → tick 14.976 → grid index 2.9952 → interpolated
        // between 6000 and 6400 cents → ≈ 6398 ct → ≈ e4×2^(-2/1200) ≈ 0.9988×e4.
        assert!((f0.f0_hz[3] - e4).abs() < e4 * 0.005); // within 0.5% of E4
        assert_eq!(f0.f0_hz, f0.shifted_f0_hz); // no shift
    }

    #[test]
    fn key_shift_scales_f0() {
        let axis = axis_120bpm();
        let pitches = vec![6900.0]; // A4
        let shifts = vec![12.0];
        let f0 = compute_f0(&pitches, 0.0, 0, 5.2, 0.0, 1, &shifts, &axis);
        assert!((f0.f0_hz[0] - 440.0).abs() < 1e-9);
        assert!((f0.shifted_f0_hz[0] - 880.0).abs() < 1e-9);
        let shifts = vec![-12.0];
        let f0 = compute_f0(&pitches, 0.0, 0, 5.2, 0.0, 1, &shifts, &axis);
        assert!((f0.shifted_f0_hz[0] - 220.0).abs() < 1e-9);
    }

    #[test]
    fn default_frame_ms_matches_diffsinger() {
        let f = default_frame_ms();
        assert!((f - 11.61).abs() < 0.01, "frame_ms {f}");
    }
}
