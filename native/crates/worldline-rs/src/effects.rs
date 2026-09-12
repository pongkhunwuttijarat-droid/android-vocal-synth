//! Port of `worldline/model/effects.cpp` + `common/vec_utils.cpp`
//! (`ShiftGender`, `GetTensionCoefficients`, `AutoGain`, `vec_maxabs`).

use crate::spline::NaturalSpline;

/// `vec_maxabs` — largest magnitude in the buffer.
pub fn vec_maxabs(values: &[f64]) -> f64 {
    values.iter().fold(0.0f64, |m, v| m.max(v.abs()))
}

/// `GenderWeights` (effects.cpp): per-bin source indices/weights for a
/// frequency-axis scaling by `ratio`.
fn gender_weights(width: usize, ratio: f64) -> (Vec<usize>, Vec<f64>) {
    let mut indexes = vec![0usize; width];
    let mut weights = vec![0.0f64; width];
    for i in 0..width {
        let p = i as f64 * ratio;
        let i1 = (p.floor() as i64).clamp(0, width as i64 - 1) as usize;
        let i2 = (p.ceil() as i64).clamp(0, width as i64 - 1) as usize;
        if i1 == i2 {
            if i1 == 0 {
                indexes[i] = 1;
                weights[i] = 1.0;
            } else {
                indexes[i] = i1;
                weights[i] = 0.0;
            }
        } else {
            indexes[i] = i1;
            weights[i] = p - p.floor();
        }
    }
    (indexes, weights)
}

/// `ShiftGender(double* sp, int width, int value)` — resample one spectral
/// row along the frequency axis. `value` is in `[-100, 100]`.
///
/// NOTE: the C++ reads `temp[indexes[i] - 1]`, which is out of bounds when
/// `indexes[i] == 0` (reachable for `ratio < 1`, i.e. a negative gender
/// shift). The read is clamped to index 0 here — defined behaviour instead
/// of the reference's UB; for `ratio >= 1` (and for the no-op `value == 0`)
/// both agree exactly.
pub fn shift_gender_row(row: &mut [f64], value: i32) {
    let width = row.len();
    if width == 0 {
        return;
    }
    let ratio = 2f64.powf(f64::from(value) * 0.01);
    if ratio == 1.0 || ratio <= 0.0 {
        return;
    }
    let (indexes, weights) = gender_weights(width, ratio);
    let temp = row.to_vec();
    for i in 0..width {
        let base = indexes[i].saturating_sub(1).min(width - 1);
        let t = weights[i];
        let a = temp[base];
        let b = temp[(base + 1).min(width - 1)];
        row[i] = a * (1.0 - t) + b * t;
    }
}

/// `Logspace` (effects.cpp): geometric ramp from `v0` to `v1`.
fn logspace(vec: &mut [f64], i0: usize, i1: usize, power: f64, v0: f64, v1: f64) {
    if i1 <= i0 || i1 > vec.len() {
        return;
    }
    let delta = (v1 - v0) / (i1 - i0) as f64;
    let mut v = v0;
    for item in vec.iter_mut().take(i1).skip(i0) {
        *item = power.powf(v);
        v += delta;
    }
}

/// `GetTensionCoefficients1` (the unused 1024-bin variant, kept for parity
/// with the reference source).
pub fn get_tension_coefficients_1(f0: f64, fs: i32, value: i32, width: usize) -> Vec<f64> {
    let _ = (f0, fs);
    let mut envelope = vec![0.0; width];
    let p1 = (75.0 / 1024.0 * width as f64) as usize;
    let p2 = (450.0 / 1024.0 * width as f64) as usize;
    let p3 = (525.0 / 1024.0 * width as f64) as usize;
    let p4 = (585.0 / 1024.0 * width as f64) as usize;
    let factor = f64::from(value) * 0.01;
    logspace(&mut envelope, 0, p1, 8.0, 0.0 * factor, 1.0 * factor);
    logspace(&mut envelope, p1, p2, 8.0, 1.0 * factor, 2.0 * factor);
    logspace(&mut envelope, p2, p3, 8.0, 2.0 * factor, 0.0 * factor);
    logspace(&mut envelope, p3, p4, 8.0, 0.0 * factor, -0.75 * factor);
    logspace(&mut envelope, p4, width, 8.0, -0.75 * factor, -0.75 * factor);
    envelope
}

/// `GetTensionCoefficients(f0, fs, value, width)` — the production path:
/// a natural cubic spline over the spectral envelope's tension shape,
/// evaluated per bin and exponentiated.
pub fn get_tension_coefficients(f0: f64, fs: i32, value: i32, width: usize) -> Vec<f64> {
    let mut envelope = vec![1.0f64; width];
    if f0 < 50.0 || width == 0 || fs <= 0 {
        return envelope;
    }
    let v = f64::from(value) * 0.01;
    let s0 = -1.5 * v;
    let s1 = if v < 0.0 { 4.0 * v } else { 2.0 * v };
    let mut px: Vec<f64> = Vec::new();
    let mut py: Vec<f64> = Vec::new();
    let f0_bins = f0 / (f64::from(fs) / 2.0) * width as f64;
    let mut x: i64 = 0;
    px.push(x as f64 * f0_bins);
    py.push(s0);
    x += 1;
    px.push(x as f64 * f0_bins);
    py.push(s0);
    x += 3;
    while (x as f64) * f0_bins < 250.0 {
        px.push(x as f64 * f0_bins);
        py.push(s1);
        x += 1;
    }
    while (x as f64) * f0_bins < 350.0 {
        x += 1;
    }
    while (x as f64) * f0_bins < width as f64 + f0_bins {
        px.push(x as f64 * f0_bins);
        py.push(0.0);
        x += 1;
    }
    let Some(spline) = NaturalSpline::new(px, py) else {
        return envelope;
    };
    for (i, item) in envelope.iter_mut().enumerate() {
        *item = spline.eval(i as f64).exp();
    }
    envelope
}

/// `AutoGain(samples, src_max, out_max, voiced_ratio, volume, peakComp)`.
pub fn auto_gain(
    samples: &mut [f64],
    src_max: f64,
    out_max: f64,
    voiced_ratio: f64,
    volume: i32,
    peak_comp: i32,
) {
    let gain = f64::from(volume) * 0.01;
    let weight = 1.0 / (1.0 + (5.0 - 10.0 * voiced_ratio).exp());
    let max = out_max * weight + src_max * (1.0 - weight);
    let auto = if max == 0.0 {
        1.0
    } else {
        (0.5 / max).powf(f64::from(peak_comp) * 0.01)
    };
    if auto * gain != 1.0 {
        for sample in samples.iter_mut() {
            *sample = *sample * auto * gain;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_gender_shift_is_a_noop() {
        let mut row = vec![1.0, 2.0, 3.0, 4.0];
        let before = row.clone();
        shift_gender_row(&mut row, 0);
        assert_eq!(row, before);
    }

    #[test]
    fn neutral_tension_is_flat_ones() {
        // value == 0 (curve 0.5) → s0 == s1 == 0 → exp(0) == 1 everywhere.
        let coeffs = get_tension_coefficients(220.0, 44100, 0, 1025);
        assert!(coeffs.iter().all(|c| (*c - 1.0).abs() < 1e-12));
    }

    #[test]
    fn low_f0_short_circuits_to_ones() {
        let coeffs = get_tension_coefficients(10.0, 44100, 100, 1025);
        assert!(coeffs.iter().all(|c| (*c - 1.0).abs() < 1e-12));
    }

    #[test]
    fn auto_gain_unity_when_gain_and_auto_are_one() {
        let mut samples = vec![0.5, -0.5];
        auto_gain(&mut samples, 0.5, 0.5, 1.0, 100, 0);
        assert_eq!(samples, vec![0.5, -0.5]);
    }
}
