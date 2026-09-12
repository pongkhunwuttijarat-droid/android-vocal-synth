//! CheapTrick parameter contract and WORLD-compatible sizing rules.
//!
//! The numerical body is intentionally isolated behind this API so the
//! upstream implementation can be ported without changing analysis callers.

pub const FLOOR_F0: f64 = 71.0;
pub const DEFAULT_F0: f64 = 500.0;
pub const SAFE_GUARD_MINIMUM: f64 = 1.0e-12;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CheapTrickOption {
    pub q1: f64,
    pub f0_floor: f64,
    pub fft_size: usize,
}

impl CheapTrickOption {
    pub fn new(sample_rate: u32) -> Self {
        let f0_floor = FLOOR_F0;
        Self { q1: -0.15, f0_floor, fft_size: fft_size_for(sample_rate, f0_floor) }
    }
}

/// Exact WORLD rule: `2^(1 + floor(log2(3*fs/f0_floor + 1)))`.
pub fn fft_size_for(sample_rate: u32, f0_floor: f64) -> usize {
    let exponent = (1.0 + (3.0 * sample_rate as f64 / f0_floor + 1.0).ln() / 2f64.ln()).floor() as u32;
    2usize.saturating_pow(exponent)
}

/// Effective lower F0 limit for a selected FFT size.
pub fn f0_floor_for(sample_rate: u32, fft_size: usize) -> f64 {
    3.0 * sample_rate as f64 / (fft_size as f64 - 3.0)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheapTrickError { InvalidInput, Fft }

/// Rust CheapTrick stage with the WORLD frame/window and cepstral-recovery
/// structure. The remaining interpolation/DC edge cases are covered by the
/// differential gate before replacing the legacy C++ path.
pub fn analyze(
    samples: &[f32], sample_rate: u32, temporal_positions: &[f64], f0: &[f64], option: CheapTrickOption,
) -> Result<Vec<f64>, CheapTrickError> {
    if sample_rate == 0 || temporal_positions.len() != f0.len() || option.fft_size < 4 || !option.fft_size.is_power_of_two() {
        return Err(CheapTrickError::InvalidInput);
    }
    let bins = option.fft_size / 2 + 1;
    let mut fft = crate::fft::RealFft::new(option.fft_size).map_err(|_| CheapTrickError::Fft)?;
    let mut input = vec![0.0; option.fft_size];
    let mut packed = vec![0.0; option.fft_size];
    let mut cepstrum = vec![0.0; option.fft_size];
    let mut out = vec![0.0; f0.len() * bins];
    for (frame_index, (&position, &raw_f0)) in temporal_positions.iter().zip(f0).enumerate() {
        let floor = f0_floor_for(sample_rate, option.fft_size);
        let current_f0 = if raw_f0 <= floor { DEFAULT_F0 } else { raw_f0 };
        let half = ((1.5 * sample_rate as f64 / current_f0) + 0.5).floor() as usize;
        if half == 0 || half * 2 + 1 > option.fft_size { return Err(CheapTrickError::InvalidInput); }
        input.fill(0.0);
        let mut window_values = vec![0.0; half * 2 + 1];
        let origin = crate::helpers::matlab_round(position * sample_rate as f64 + 0.001) as i64;
        let mut norm = 0.0;
        for i in 0..=half * 2 {
            let relative = i as i64 - half as i64;
            let index = (origin + relative).clamp(0, samples.len().saturating_sub(1) as i64) as usize;
            let window = 0.5 * (std::f64::consts::PI * relative as f64 * current_f0 / (1.5 * sample_rate as f64)).cos() + 0.5;
            window_values[i] = window;
            input[i] = samples.get(index).copied().unwrap_or(0.0) as f64 * window;
            norm += window * window;
        }
        let window_sum = (0..=half * 2).map(|i| 0.5 * (std::f64::consts::PI * (i as i64 - half as i64) as f64 * current_f0 / (1.5 * sample_rate as f64)).cos() + 0.5).sum::<f64>();
        let mean = input.iter().take(half * 2 + 1).sum::<f64>() / window_sum;
        for (i, value) in input.iter_mut().take(half * 2 + 1).enumerate() { *value = (*value - window_values[i] * mean) / norm.sqrt(); }
        fft.forward_packed(&input, &mut packed).map_err(|_| CheapTrickError::Fft)?;
        let mut power = vec![0.0; bins];
        power[0] = packed[0] * packed[0];
        power[option.fft_size / 2] = packed[1] * packed[1];
        for k in 1..option.fft_size / 2 { power[k] = packed[2 * k].mul_add(packed[2 * k], packed[2 * k + 1] * packed[2 * k + 1]); }
        crate::helpers::dc_correction(&mut power, current_f0, sample_rate, option.fft_size);
        let width = current_f0 * 2.0 / 3.0;
        let mut smooth = vec![0.0; bins];
        crate::helpers::linear_smoothing(&power, width, sample_rate, option.fft_size, &mut smooth);
        for value in &mut smooth { *value = value.max(SAFE_GUARD_MINIMUM); }
        let mut log_wave = vec![0.0; option.fft_size];
        log_wave[..bins].copy_from_slice(&smooth.iter().map(|x| x.ln()).collect::<Vec<_>>());
        for k in 1..option.fft_size / 2 { log_wave[option.fft_size - k] = log_wave[k]; }
        fft.forward_packed(&log_wave, &mut packed).map_err(|_| CheapTrickError::Fft)?;
        for k in 0..=option.fft_size / 2 { let q = k as f64 / sample_rate as f64; let lifter = if k == 0 { 1.0 } else { (std::f64::consts::PI * current_f0 * q).sin() / (std::f64::consts::PI * current_f0 * q) * ((1.0 - 2.0 * option.q1) + 2.0 * option.q1 * (2.0 * std::f64::consts::PI * current_f0 * q).cos()) }; if k == 0 { packed[0] *= lifter; } else if k == option.fft_size / 2 { packed[1] *= lifter; } else { packed[2 * k] *= lifter; packed[2 * k + 1] = 0.0; } }
        fft.inverse_packed(&packed, &mut cepstrum).map_err(|_| CheapTrickError::Fft)?;
        for k in 0..bins { out[frame_index * bins + k] = cepstrum[k].exp().max(SAFE_GUARD_MINIMUM); }
    }
    Ok(out)
}

pub fn effective_f0(f0: f64, sample_rate: u32, option: CheapTrickOption) -> f64 {
    if f0 <= f0_floor_for(sample_rate, option.fft_size) { DEFAULT_F0 } else { f0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_44100_world_fft_size_is_2048() {
        assert_eq!(CheapTrickOption::new(44100).fft_size, 2048);
        assert!((f0_floor_for(44100, 2048) - 64.69).abs() < 0.1);
    }

    #[test]
    fn low_f0_uses_world_default_f0() {
        let option = CheapTrickOption::new(44100);
        assert_eq!(effective_f0(40.0, 44100, option), DEFAULT_F0);
        assert_eq!(effective_f0(220.0, 44100, option), 220.0);
    }
}
