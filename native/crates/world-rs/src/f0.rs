//! WORLD-compatible F0 stage boundary: DIO followed by StoneMask refinement.
//!
//! The API mirrors the upstream call shape. The current implementation is a
//! deterministic Rust autocorrelation bring-up while the multi-band DIO
//! refinement is being ported against the pinned oracle.

use crate::Cancellation;
use rustfft::{num_complex::Complex, FftPlanner};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DioOption {
    pub frame_period_ms: f64,
    pub f0_floor: f64,
    pub f0_ceil: f64,
    pub allowed_range: f64,
}

impl Default for DioOption {
    fn default() -> Self {
        Self {
            frame_period_ms: 5.0,
            f0_floor: 71.0,
            f0_ceil: 800.0,
            allowed_range: 0.1,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum F0Error {
    InvalidInput,
    Cancelled,
}

/// Estimate F0 and its time axis. Unvoiced frames are represented by zero.
pub fn dio<C: Cancellation>(
    samples: &[f32],
    sample_rate: u32,
    option: DioOption,
    cancel: &C,
) -> Result<(Vec<f64>, Vec<f64>), F0Error> {
    if sample_rate == 0
        || option.frame_period_ms <= 0.0
        || option.f0_floor <= 0.0
        || option.f0_ceil <= option.f0_floor
    {
        return Err(F0Error::InvalidInput);
    }
    let hop = (sample_rate as f64 * option.frame_period_ms / 1000.0)
        .round()
        .max(1.0) as usize;
    // WORLD GetSamplesForDIO: floor(1000 * length / fs / frame_period) + 1.
    let frames = if samples.is_empty() {
        0
    } else {
        (1000.0 * samples.len() as f64 / sample_rate as f64 / option.frame_period_ms) as usize + 1
    };
    let window = (3.0 * sample_rate as f64 / option.f0_floor).round() as usize;
    let min_lag = (sample_rate as f64 / option.f0_ceil).floor().max(1.0) as usize;
    let max_lag = (sample_rate as f64 / option.f0_floor)
        .ceil()
        .min(window.max(2) as f64) as usize;
    let mut f0 = vec![0.0; frames];
    let mut time = vec![0.0; frames];
    // FFT autocorrelation keeps the bring-up estimator's exact lag search
    // semantics while making real voicebank analysis practical at WORLD's
    // 2048-point configuration.
    let ac_size = (2 * window.max(1)).next_power_of_two();
    let mut planner = FftPlanner::<f64>::new();
    let forward = planner.plan_fft_forward(ac_size);
    let inverse = planner.plan_fft_inverse(ac_size);
    let mut spectrum = vec![Complex::default(); ac_size];
    for frame_index in 0..frames {
        if cancel.is_cancelled() {
            return Err(F0Error::Cancelled);
        }
        let center = frame_index * hop;
        time[frame_index] = center as f64 / sample_rate as f64;
        let start = center.saturating_sub(window / 2);
        let end = (start + window).min(samples.len());
        let frame = &samples[start..end];
        let energy = frame.iter().map(|&x| (x as f64) * (x as f64)).sum::<f64>();
        if energy < 1e-10 || min_lag >= frame.len() {
            continue;
        }
        let upper = max_lag.min(frame.len() - 1);
        for (dst, &sample) in spectrum.iter_mut().zip(frame.iter()) {
            *dst = Complex {
                re: sample as f64,
                im: 0.0,
            };
        }
        for dst in spectrum.iter_mut().skip(frame.len()) {
            *dst = Complex::default();
        }
        forward.process(&mut spectrum);
        for value in &mut spectrum {
            *value *= value.conj();
        }
        inverse.process(&mut spectrum);
        let mut best = 0.0;
        let mut best_lag = 0;
        for lag in min_lag..=upper {
            let corr = spectrum[lag].re;
            if corr > best {
                best = corr;
                best_lag = lag;
            }
        }
        if best_lag > 0 && best / energy > 0.2 {
            f0[frame_index] = sample_rate as f64 / best_lag as f64;
        }
    }
    Ok((f0, time))
}

/// StoneMask-compatible refinement hook. It preserves unvoiced frames and
/// applies a bounded local correction for voiced estimates.
pub fn stone_mask(samples: &[f32], sample_rate: u32, time: &[f64], f0: &mut [f64]) {
    if sample_rate == 0 {
        return;
    }
    for (index, value) in f0.iter_mut().enumerate() {
        if *value <= 0.0 || index >= time.len() {
            continue;
        }
        let center = (time[index] * sample_rate as f64).round() as usize;
        let lag = (sample_rate as f64 / *value).round().max(1.0) as usize;
        if center + lag < samples.len() && samples[center].abs() > 1e-5 {
            *value = sample_rate as f64 / lag as f64;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dio_estimates_sine_and_time_axis() {
        let x: Vec<f32> = (0..4410)
            .map(|i| (2.0 * std::f64::consts::PI * 220.0 * i as f64 / 44100.0).sin() as f32)
            .collect();
        let (f0, time) = dio(&x, 44100, DioOption::default(), &()).unwrap();
        assert_eq!(f0.len(), time.len());
        assert!(f0.iter().any(|f| (*f - 220.0).abs() < 5.0));
    }

    #[test]
    fn cancellation_is_reported() {
        struct Stop;
        impl Cancellation for Stop {
            fn is_cancelled(&self) -> bool {
                true
            }
        }
        assert_eq!(
            dio(&[0.0; 100], 44100, DioOption::default(), &Stop),
            Err(F0Error::Cancelled)
        );
    }
}
