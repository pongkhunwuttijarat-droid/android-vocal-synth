//! Curve sampling: sparse `UCurve` points → dense per-5-tick → per-frame.
//!
//! Mirrors `RenderPhrase.SampleCurve` (step 1, linear interpolation at the
//! 5-tick interval, out-of-range samples falling back to the expression
//! default) and the per-frame resampling of `ResamplerItem` (step 2, where
//! a frame's tick position is lerped inside the sampled grid).
//!
//! The project stores curves sparsely (points + equation, linear between
//! points — OpenUtau's `UCurve` has no per-point shapes); this transformer
//! densifies them once for all renderers.

use domain::{TimeAxis, UCurve};

use crate::pitch::PITCH_INTERVAL;

/// Dense sampling of a sparse curve at the 5-tick interval.
///
/// `start_tick` is the first sampled tick (part-relative), `length` the
/// number of samples (the pitch grid length), `default_value` the
/// expression's custom default used outside the curve's point range, and
/// `convert` the per-expression normalization applied to each sample
/// (identity for most curves, dB→linear for `dyn`).
pub fn sample_curve(
    curve: &UCurve,
    start_tick: i32,
    length: usize,
    default_value: f32,
    convert: impl Fn(f32) -> f32,
) -> Vec<f32> {
    let mut result = Vec::with_capacity(length);
    for i in 0..length {
        let x = start_tick + i as i32 * PITCH_INTERVAL;
        let raw = curve.sample(x).unwrap_or(default_value as i32) as f32;
        result.push(convert(raw));
    }
    result
}

/// Convenience: the same sampling with the identity conversion.
pub fn sample_curve_identity(
    curve: &UCurve,
    start_tick: i32,
    length: usize,
    default_value: f32,
) -> Vec<f32> {
    sample_curve(curve, start_tick, length, default_value, |v| v)
}

/// Resample a per-5-tick array to a per-frame array.
///
/// * `sampled` — the dense per-5-tick grid (e.g. from [`sample_curve`]).
/// * `start_ms` — project-absolute ms of the grid's first sample.
/// * `grid_start_tick` — project-absolute tick of the grid's first sample.
/// * `frame_ms` — frame size in ms (hop_size / sample_rate × 1000).
/// * `total_frames` — number of frames to produce.
///
/// Frame `f` is sampled at `start_ms + f × frame_ms`; the position is
/// converted back to ticks via the time axis, mapped into the grid with
/// linear interpolation, and clamped at the grid bounds.
pub fn sample_per_frame(
    sampled: &[f32],
    start_ms: f64,
    grid_start_tick: i32,
    frame_ms: f64,
    total_frames: usize,
    time_axis: &TimeAxis,
) -> Vec<f32> {
    if sampled.is_empty() {
        return Vec::new();
    }
    let mut result = Vec::with_capacity(total_frames);
    for f in 0..total_frames {
        let ms = start_ms + f as f64 * frame_ms;
        let tick = time_axis.ms_to_non_exact_tick(ms);
        let index = (tick - grid_start_tick as f64) / PITCH_INTERVAL as f64;
        let index = index.clamp(0.0, (sampled.len() - 1) as f64);
        let lo = index.floor() as usize;
        let hi = index.ceil() as usize;
        let alpha = index - lo as f64;
        let value = if lo == hi {
            sampled[lo]
        } else {
            sampled[lo] + (sampled[hi] - sampled[lo]) * alpha as f32
        };
        result.push(value);
    }
    result
}

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
    fn samples_linear_between_points() {
        let curve = UCurve {
            abbr: "genc".into(),
            xs: vec![0, 100, 200],
            ys: vec![0, 100, 0],
        };
        let sampled = sample_curve_identity(&curve, 0, 41, 0.0); // 200/5+1
        assert_eq!(sampled.len(), 41);
        assert_eq!(sampled[0], 0.0);
        assert_eq!(sampled[20], 100.0); // x=100
        assert_eq!(sampled[10], 50.0); // x=50
        assert_eq!(sampled[40], 0.0);
    }

    #[test]
    fn outside_range_uses_default() {
        let curve = UCurve { abbr: "x".into(), xs: vec![100, 200], ys: vec![10, 20] };
        let sampled = sample_curve_identity(&curve, 0, 41, 7.0);
        assert_eq!(sampled[0], 7.0); // x=0 before the first point
        assert_eq!(sampled[19], 7.0); // x=95 still before the first point
        assert_eq!(sampled[20], 10.0); // x=100 exactly on the first point
        assert_eq!(sampled[39], 20.0); // x=195: 10 + 10*0.95 = 19.5, banker-rounded to 20
        assert_eq!(sampled[40], 20.0); // x=200 on the last point
    }

    #[test]
    fn dynamics_convert_db_to_linear() {
        let curve = UCurve { abbr: "dyn".into(), xs: vec![0, 480], ys: vec![0, 100] };
        let min = -240.0f32;
        // 97 samples at ticks 0..480 (every 5 ticks).
        let sampled = sample_curve(&curve, 0, 97, 0.0, |v| {
            if v == min {
                0.0
            } else {
                crate::music_math::decibel_to_linear(v as f64 * 0.1) as f32
            }
        });
        assert_eq!(sampled[0], 1.0); // tick 0, y=0 → 0 dB → linear 1.0
        // tick 480, y=100 → 100×0.1 dB = +10 dB → 10^(10/20) ≈ 3.1623.
        assert!((sampled[96] - 10f32.powf(10.0 / 20.0)).abs() < 1e-5);
    }

    #[test]
    fn per_frame_resampling_lerps() {
        let axis = axis_120bpm();
        // Grid: 10 samples at ticks 0,5,...,45; frame_ms = 5.2 ms (= 5 ticks).
        let sampled: Vec<f32> = (0..10).map(|i| i as f32 * 10.0).collect();
        // start at tick 0 (ms 0), 5 frames of 5.2 ms each -> ticks 0..26.
        let frames = sample_per_frame(&sampled, 0.0, 0, 5.2, 5, &axis);
        assert_eq!(frames.len(), 5);
        assert_eq!(frames[0], 0.0);
        // 5.2 ms → tick 4.992 → index 0.9984 → ≈ 9.984 (interpolated, not 10).
        assert!((frames[1] - 9.984).abs() < 1e-2);
        // Frame 4 at 20.8 ms → tick 19.968 → index 3.9936 → ≈ 39.936.
        assert!((frames[4] - 39.936).abs() < 1e-2);
    }

    #[test]
    fn per_frame_clamps_at_bounds() {
        let axis = axis_120bpm();
        let sampled = vec![100.0, 200.0];
        // Sampling far after the grid (ticks 0..5) clamps to the LAST value.
        // Frame 0 at ms 0 → tick 0 → 100; frames 1-2 at ms 1000/2000 →
        // tick 960/1920 → index 192/384 → clamped to index 1 → 200.
        let frames = sample_per_frame(&sampled, 0.0, 0, 1000.0, 3, &axis);
        assert_eq!(frames[0], 100.0);
        assert_eq!(frames[1], 200.0);
        assert_eq!(frames[2], 200.0);
        // Far BEFORE the grid → clamps to the FIRST value.
        let frames = sample_per_frame(&sampled, 100000.0, 0, 1000.0, 2, &axis);
        assert_eq!(frames, vec![200.0, 200.0]);
    }

    #[test]
    fn empty_grid_gives_empty_result() {
        let axis = axis_120bpm();
        assert!(sample_per_frame(&[], 0.0, 0, 5.2, 4, &axis).is_empty());
    }
}
