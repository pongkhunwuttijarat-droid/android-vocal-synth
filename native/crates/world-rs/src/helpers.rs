//! Small numerical helpers whose rounding semantics are part of WORLD's ABI.

/// WORLD's `matlab_round`: halves are rounded away from zero.
pub fn matlab_round(value: f64) -> i32 {
    if value >= 0.0 { (value + 0.5).floor() as i32 } else { (value - 0.5).ceil() as i32 }
}

pub fn clamp_index(index: i32, length: usize) -> usize {
    if length == 0 { return 0; }
    index.clamp(0, length as i32 - 1) as usize
}

pub fn next_power_of_two(value: usize) -> Option<usize> {
    if value == 0 { return Some(1); }
    value.checked_next_power_of_two()
}

/// Port of WORLD's `LinearSmoothing` with mirrored boundaries and cumulative
/// interpolation. `input` contains bins 0..=N/2 and `width` is in Hz.
pub fn linear_smoothing(input: &[f64], width: f64, sample_rate: u32, fft_size: usize, output: &mut [f64]) {
    if input.is_empty() || output.len() != input.len() || width <= 0.0 || fft_size == 0 { return; }
    let boundary = (width * fft_size as f64 / sample_rate as f64) as usize + 1;
    let total = fft_size / 2 + boundary * 2 + 1;
    let mut mirrored = vec![0.0; total];
    for i in 0..boundary { mirrored[i] = input[boundary - i]; }
    for i in boundary..fft_size / 2 + boundary { mirrored[i] = input[i - boundary]; }
    for i in fft_size / 2 + boundary..total { mirrored[i] = input[fft_size / 2 - (i - (fft_size / 2 + boundary))]; }
    let df = sample_rate as f64 / fft_size as f64;
    let mut cumulative = vec![0.0; total];
    cumulative[0] = mirrored[0] * df;
    for i in 1..total { cumulative[i] = cumulative[i - 1] + mirrored[i] * df; }
    let origin = -(boundary as f64 - 0.5) * df;
    for i in 0..input.len() {
        let frequency = i as f64 * df;
        let low = frequency - width / 2.0;
        output[i] = (interp(&cumulative, origin, df, low + width) - interp(&cumulative, origin, df, low)) / width;
    }
}

/// Port of WORLD's low-frequency DC correction. Values below the current F0
/// are mirrored from the spectrum around F0 before subsequent smoothing.
pub fn dc_correction(power: &mut [f64], f0: f64, sample_rate: u32, fft_size: usize) {
    if power.is_empty() || f0 <= 0.0 || sample_rate == 0 || fft_size == 0 { return; }
    let df = sample_rate as f64 / fft_size as f64;
    let upper = (2.0 + f0 * fft_size as f64 / sample_rate as f64) as usize;
    // C++ DCCorrection allocates `upper_limit`, then writes only
    // `upper_limit - 1` low-frequency bins.
    let upper = upper.saturating_sub(1).min(power.len());
    let original = power.to_vec();
    for i in 0..upper {
        let query = f0 - i as f64 * df;
        let position = (query / df).clamp(0.0, (original.len() - 1) as f64);
        let left = position.floor() as usize;
        let fraction = position - left as f64;
        let reflected = if left + 1 < original.len() { original[left] * (1.0 - fraction) + original[left + 1] * fraction } else { original[left] };
        power[i] = original[i] + reflected;
    }
}

fn interp(values: &[f64], origin: f64, step: f64, x: f64) -> f64 {
    let position = ((x - origin) / step).clamp(0.0, (values.len() - 1) as f64);
    let index = position.floor() as usize;
    let frac = position - index as f64;
    if index + 1 < values.len() { values[index] * (1.0 - frac) + values[index + 1] * frac } else { values[index] }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matlab_round_matches_world_for_half_values() {
        assert_eq!(matlab_round(1.5), 2);
        assert_eq!(matlab_round(1.49), 1);
        assert_eq!(matlab_round(-1.5), -2);
        assert_eq!(matlab_round(-1.49), -1);
    }

    #[test]
    fn index_clamping_is_safe() {
        assert_eq!(clamp_index(-5, 4), 0);
        assert_eq!(clamp_index(9, 4), 3);
        assert_eq!(clamp_index(0, 0), 0);
    }

    #[test]
    fn linear_smoothing_preserves_constant_power() {
        let input = vec![2.0; 33];
        let mut output = vec![0.0; 33];
        linear_smoothing(&input, 100.0, 44100, 64, &mut output);
        assert!(output.iter().all(|x| (*x - 2.0).abs() < 1e-10));
    }

    #[test]
    fn dc_correction_only_changes_low_bins() {
        let mut power = vec![1.0; 33];
        dc_correction(&mut power, 220.0, 44100, 64);
        assert!(power[0] > 1.0);
        assert_eq!(&power[10..], &[1.0; 23]);
    }
}
