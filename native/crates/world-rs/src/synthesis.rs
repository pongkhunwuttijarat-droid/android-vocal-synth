//! Functional Rust WORLD-style feature synthesis.
//!
//! This module is the executable synthesis boundary used by native/WASM hosts.
//! The implementation uses the same contiguous feature contract as analysis,
//! deterministic excitation, minimum-phase-like spectral shaping, and overlap
//! add. The upstream WORLD differential port will tighten each numerical stage
//! against the pinned C++ oracle without changing this public API.

use crate::fft::RealFft;
use crate::{Cancellation, WorldFeatures};
use rustfft::num_complex::Complex;

use crate::fft::Planner;

#[derive(Debug, Clone, Copy)]
pub struct SynthesisConfig {
    pub sample_rate: u32,
    pub frame_period_ms: f64,
    pub fft_size: usize,
    pub seed: u64,
    pub tension: f64,
    pub breathiness: f64,
    pub voicing: f64,
    /// Optional exact output length requested by the host/WORLD API.
    pub output_length: Option<usize>,
}

impl Default for SynthesisConfig {
    fn default() -> Self {
        Self { sample_rate: 44100, frame_period_ms: 512.0 * 1000.0 / 44100.0, fft_size: 1024, seed: 0x574f524c44, tension: 1.0, breathiness: 1.0, voicing: 1.0, output_length: None }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SynthesisError { InvalidConfig, FeatureMismatch, Cancelled }

/// Per-frame expression curves for the WORLD `Synthesis` extension
/// (`tension`/`breathiness`/`voicing` arrays in the worldline C++ fork).
///
/// An empty vector means "neutral" (all `1.0`), which is exactly the
/// upstream WORLD behaviour when the corresponding pointer is null. The
/// caller indexes them by the *pulse's* frame (floor of the pulse time in
/// frame units), matching `frame_index` in the C++ `Synthesis` loop.
#[derive(Debug, Clone, Default)]
pub struct ExpressionCurves {
    /// Spectral tension coefficients (1.0 = unmodified), one per frame.
    pub tension: Vec<f64>,
    /// Optional *per-bin* tension matrix, row-major `frame * bin_count + bin`.
    /// The worldline fork multiplies the spectral envelope by a
    /// frequency-dependent tension shape (see `GetTensionCoefficients`), so
    /// a scalar cannot express it. When present it takes precedence over
    /// [`Self::tension`].
    pub tension_bins: Option<Vec<f64>>,
    /// Aperiodic (breath) gain (1.0 = unmodified).
    pub breathiness: Vec<f64>,
    /// Periodic (voicing) gain (1.0 = unmodified).
    pub voicing: Vec<f64>,
}

impl ExpressionCurves {
    fn tension_at(&self, frame: usize) -> f64 {
        self.tension.get(frame).copied().unwrap_or(1.0)
    }
    fn breathiness_at(&self, frame: usize) -> f64 {
        self.breathiness.get(frame).copied().unwrap_or(1.0)
    }
    fn voicing_at(&self, frame: usize) -> f64 {
        self.voicing.get(frame).copied().unwrap_or(1.0)
    }
}

/// Synthesize a mono PCM stream from contiguous WORLD features.
pub fn synthesize<C: Cancellation>(features: &WorldFeatures, config: SynthesisConfig, cancel: &C) -> Result<Vec<f32>, SynthesisError> {
    synthesize_with_curves(features, config, &ExpressionCurves::default(), cancel)
}

/// Synthesize with per-frame expression curves (the worldline fork's
/// `tension`/`breathiness`/`voicing` arrays). `synthesize` is this
/// function with neutral curves.
pub fn synthesize_with_curves<C: Cancellation>(
    features: &WorldFeatures,
    config: SynthesisConfig,
    curves: &ExpressionCurves,
    cancel: &C,
) -> Result<Vec<f32>, SynthesisError> {
    if config.sample_rate == 0 || config.fft_size < 4 || !config.fft_size.is_power_of_two()
        || config.frame_period_ms <= 0.0 || features.bin_count != config.fft_size / 2 + 1
    { return Err(SynthesisError::InvalidConfig); }
    features.validate().map_err(|_| SynthesisError::FeatureMismatch)?;
    if features.frame_count == 0 { return Ok(Vec::new()); }
    let hop = ((config.sample_rate as f64 * config.frame_period_ms / 1000.0).round() as usize).max(1);
    let natural_output_len = (features.frame_count - 1) * hop + config.fft_size;
    let output_len = config.output_length.unwrap_or(natural_output_len);

    // Keep the exact WORLD pulse path for the normal periodic case. This is
    // also the smallest useful differential slice: the upstream renderer
    // places a minimum-phase response at each phase wrap, rather than adding
    // one independently-windowed FFT frame per analysis frame.
    // The periodic WORLD path is the reference-compatible renderer for the
    // production backend. The older frame-FFT pulse/noise approximation is
    // retained only as an explicit diagnostic escape hatch; leaving it as
    // the default made real voicebank renders orders of magnitude quieter
    // than OpenUtau even though their feature grids were correct.
    if std::env::var_os("WORLD_RUST_DISABLE_PERIODIC").is_none() {
        return synthesize_periodic_world(features, config, curves, output_len, cancel);
    }
    let mut output = vec![0.0f64; output_len];
    let mut weights = vec![0.0f64; output_len];
    let mut fft = RealFft::new(config.fft_size).map_err(|_| SynthesisError::InvalidConfig)?;
    let mut packed = vec![0.0; config.fft_size];
    let mut frame = vec![0.0; config.fft_size];
    let mut rng = config.seed;
    let bins = features.bin_count;
    for frame_index in 0..features.frame_count {
        if cancel.is_cancelled() { return Err(SynthesisError::Cancelled); }
        let row = frame_index * bins;
        let f0 = features.f0[frame_index];
        let phase_inc = if f0 > 0.0 { 2.0 * std::f64::consts::PI * f0 / config.sample_rate as f64 } else { 0.0 };
        // WORLD scales each pulse/noise response by sqrt(noise_size), where
        // noise_size is approximately one period in samples. The bring-up
        // path previously omitted this factor, making real voicebank renders
        // much quieter than the C++ renderer despite non-zero features.
        let response_scale = if f0 > 0.0 {
            (config.sample_rate as f64 / f0).sqrt()
        } else { 1.0 };
        let mut phase = 0.0;
        packed.fill(0.0);
        for k in 0..bins {
            let envelope = features.spectral[row + k].max(1e-12) * config.tension.max(0.0);
            let ap = features.aperiodicity[row + k].clamp(0.0, 1.0);
            let random = next_noise(&mut rng);
            let periodic = (1.0 - ap) * config.voicing.clamp(0.0, 1.0);
            let magnitude = envelope * response_scale * (periodic + ap * config.breathiness.max(0.0) * random.abs());
            if k == 0 { packed[0] = magnitude; }
            else if k == config.fft_size / 2 { packed[1] = magnitude; }
            else {
                // A deterministic phase ramp keeps adjacent frames stable;
                // noise contribution supplies the aperiodic component.
                let angle = phase + random * std::f64::consts::PI * ap;
                packed[2 * k] = magnitude * angle.cos();
                packed[2 * k + 1] = magnitude * angle.sin();
            }
            phase += phase_inc;
        }
        fft.inverse_packed(&packed, &mut frame).map_err(|_| SynthesisError::InvalidConfig)?;
        let start = frame_index * hop;
        for i in 0..config.fft_size.min(output_len.saturating_sub(start)) {
            let window = (std::f64::consts::PI * (i as f64 + 0.5) / config.fft_size as f64).sin().powi(2);
            output[start + i] += frame[i] * window;
            weights[start + i] += window;
        }
    }
    for (sample, weight) in output.iter_mut().zip(weights) {
        if weight > 1e-12 { *sample /= weight; }
    }
    Ok(output.into_iter().map(|x| x as f32).collect())
}

/// Build WORLD's minimum-phase complex spectrum from a full log spectrum.
pub fn minimum_phase_spectrum(log_spectrum: &[f64]) -> Vec<Complex<f64>> {
    let n = log_spectrum.len();
    let mut planner = Planner::new();
    let forward = planner.forward(n);
    let inverse = planner.inverse(n);
    minimum_phase_spectrum_with_plans(log_spectrum, forward.as_ref(), inverse.as_ref())
}

fn minimum_phase_spectrum_with_plans(
    log_spectrum: &[f64],
    forward: &dyn rustfft::Fft<f64>,
    inverse: &dyn rustfft::Fft<f64>,
) -> Vec<Complex<f64>> {
    let n = log_spectrum.len();
    let mut cepstrum: Vec<Complex<f64>> = log_spectrum.iter()
        .map(|&x| Complex { re: x, im: 0.0 }).collect();
    inverse.process(&mut cepstrum);
    for value in &mut cepstrum { value.im = -value.im; }
    for i in 1..n / 2 { cepstrum[i] = Complex { re: cepstrum[i].re * 2.0, im: cepstrum[i].im * -2.0 }; }
    cepstrum[n / 2].im = -cepstrum[n / 2].im;
    for value in &mut cepstrum[n / 2 + 1..] { *value = Complex::default(); }
    forward.process(&mut cepstrum);
    cepstrum.into_iter().map(|x| {
        let z = (x.re / n as f64).exp();
        Complex { re: z * (x.im / n as f64).cos(), im: z * (x.im / n as f64).sin() }
    }).collect()
}

fn synthesize_periodic_world<C: Cancellation>(features: &WorldFeatures, config: SynthesisConfig,
    curves: &ExpressionCurves, output_len: usize, cancel: &C) -> Result<Vec<f32>, SynthesisError> {
    let n = config.fft_size;
    let bins = features.bin_count;
    let fs = config.sample_rate as f64;
    let frame_period = config.frame_period_ms / 1000.0;
    let lowest_f0 = fs / n as f64 + 1.0;
    let mut output = vec![0.0; output_len];
    let mut dc = vec![0.0; n];
    let mut dc_component = 0.0;
    for i in 0..n / 2 {
        dc[i] = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * (i as f64 + 1.0) / (n as f64 + 1.0)).cos();
        dc[n - i - 1] = dc[i];
        dc_component += 2.0 * dc[i];
    }
    for x in &mut dc { *x /= dc_component; }

    // Match WORLD's GetTemporalParametersForTimeBase: the coarse F0 used for
    // interpolation is already zeroed below the floor.  Interpolating raw F0
    // values here changes the transition between unvoiced and voiced frames.
    let coarse_f0 = |i: usize| -> f64 {
        let value = if i < features.frame_count {
            features.f0[i]
        } else if features.frame_count > 1 {
            features.f0[features.frame_count - 1] * 2.0
                - features.f0[features.frame_count - 2]
        } else {
            features.f0[features.frame_count - 1]
        };
        if value >= lowest_f0 { value } else { 0.0 }
    };
    let sample_f0 = |sample: usize| -> f64 {
        let t = sample as f64 / fs;
        let pos = t / frame_period;
        let floor = pos.floor().max(0.0) as usize;
        let ceil = pos.ceil().max(0.0) as usize;
        let interpolation = pos - floor as f64;
        let floor_f0 = coarse_f0(floor);
        let ceil_f0 = coarse_f0(ceil);
        let value = floor_f0 + (ceil_f0 - floor_f0) * interpolation;
        let floor_vuv = if floor_f0 == 0.0 { 0.0 } else { 1.0 };
        let ceil_vuv = if ceil_f0 == 0.0 { 0.0 } else { 1.0 };
        let vuv = floor_vuv + (ceil_vuv - floor_vuv) * interpolation;
        // WORLD's GetTimeBase interpolates VUV separately and substitutes
        // kDefaultF0 (500 Hz) for unvoiced samples. Using the CheapTrick
        // floor here (about 22 Hz at fft=2048) destroys phase continuity.
        if vuv > 0.5 { value } else { 500.0 }
    };
    let mut phase = 2.0 * std::f64::consts::PI * sample_f0(0) / fs;
    let mut previous_wrap = phase.rem_euclid(2.0 * std::f64::consts::PI);
    let mut pulses = Vec::new();
    for i in 0..output_len.saturating_sub(1) {
        if cancel.is_cancelled() { return Err(SynthesisError::Cancelled); }
        phase += 2.0 * std::f64::consts::PI * sample_f0(i + 1) / fs;
        let wrap = phase.rem_euclid(2.0 * std::f64::consts::PI);
        if (wrap - previous_wrap).abs() > std::f64::consts::PI {
            let y1 = previous_wrap - 2.0 * std::f64::consts::PI;
            let x = -y1 / (wrap - y1);
            pulses.push((i, x / fs));
        }
        previous_wrap = wrap;
    }
    let mut planner = Planner::new();
    let inverse = planner.inverse(n);
    let noise_forward = planner.forward(n);
    let noise_inverse = planner.inverse(n);
    let minimum_phase_forward = planner.forward(n);
    let minimum_phase_inverse = planner.inverse(n);
    let mut rng = [123456789u32, 362436069, 521288629, 88675123];
    for (pulse_no, &(index, shift)) in pulses.iter().enumerate() {
        if cancel.is_cancelled() { return Err(SynthesisError::Cancelled); }
        // WORLD clamps the look-ahead pulse index to the final pulse. The
        // final response therefore has noise_size == 0 and contributes no
        // periodic energy; do not invent an extra period at the boundary.
        let noise_size = pulses.get(pulse_no + 1).map(|p| p.0.saturating_sub(index)).unwrap_or(0);
        if noise_size == 0 { continue; }
        let time = index as f64 / fs;
        let pos = time / frame_period;
        let lo = pos.floor().max(0.0) as usize;
        let lo = lo.min(features.frame_count - 1);
        let hi = (pos.ceil() as usize).min(features.frame_count - 1);
        let a = if lo == hi { 0.0 } else { pos - lo as f64 };
        let mut spectrum = vec![0.0; n];
        let mut ap = vec![0.0; n];
        for k in 0..bins { spectrum[k] = ((1.0 - a) * features.spectral[lo * bins + k] + a * features.spectral[hi * bins + k]).abs(); }
        for k in 0..bins { ap[k] = ((1.0 - a) * features.aperiodicity[lo * bins + k] + a * features.aperiodicity[hi * bins + k]).clamp(0.0, 1.0); }
        for k in (bins..n).rev() { spectrum[k] = spectrum[n - k]; }
        for k in (bins..n).rev() { ap[k] = ap[n - k]; }
        // The pulse carries the sample-domain interpolated VUV in WORLD.
        let lo_vuv = if coarse_f0(lo) == 0.0 { 0.0 } else { 1.0 };
        let hi_vuv = if coarse_f0(hi) == 0.0 { 0.0 } else { 1.0 };
        let current_vuv = lo_vuv + (hi_vuv - lo_vuv) * a;
        let voiced = current_vuv > 0.5;
        // The C++ `Synthesis` extension reads the expression curves at the
        // pulse's floor frame (`frame_index` in synthesis.cpp), i.e. the
        // curves are piecewise-constant per world frame — not interpolated
        // like the spectrum. Clamped here (the C++ reads out of bounds on
        // the final pulse; clamping is the safe equivalent).
        // The C++ curve arrays are `f0_length` long (`y_length` for the
        // scalar curves), so the valid index range is `0..frame_count`.
        let curve_frame = lo.min(features.frame_count - 1);
        let tension = curves.tension_at(curve_frame);
        let breathiness = curves.breathiness_at(curve_frame);
        let voicing = curves.voicing_at(curve_frame);
        // Per-bin tension shape (worldline `tension[frame][bin]`), mirrored
        // into the upper half exactly like the spectrum/ap rows.
        let tension_row: Option<Vec<f64>> = curves.tension_bins.as_ref().map(|matrix| {
            let mut row = vec![1.0f64; n];
            if matrix.len() >= (curve_frame + 1) * bins {
                row[..bins].copy_from_slice(&matrix[curve_frame * bins..(curve_frame + 1) * bins]);
                for k in (bins..n).rev() {
                    row[k] = row[n - k];
                }
            }
            row
        });
        // WORLD's GetAperiodicRatio squares the D4C amplitude ratio before
        // it is consumed by both the periodic and aperiodic responses.
        let log_spectrum: Vec<f64> = spectrum
            .iter()
            .zip(ap.iter())
            .enumerate()
            .map(|(k, (&x, &r))| {
                let ratio = r * r;
                let magnitude = if voiced { x * (1.0 - ratio) } else { x };
                let t = tension_row.as_ref().map(|row| row[k]).unwrap_or(tension);
                (magnitude * t + 1e-12).ln() / 2.0
            })
            .collect();
        let min_phase = if voiced && ap[0] <= 0.999 {
            minimum_phase_spectrum_with_plans(
                &log_spectrum,
                minimum_phase_forward.as_ref(),
                minimum_phase_inverse.as_ref(),
            )
        } else {
            vec![Complex::default(); n]
        };
        let coefficient = 2.0 * std::f64::consts::PI * shift * fs / n as f64;
        let mut freq = min_phase;
        for k in 0..=n / 2 {
            let c = (coefficient * k as f64).cos();
            let s = (1.0 - c * c).max(0.0).sqrt();
            let z = freq[k];
            freq[k] = Complex { re: z.re * c + z.im * s, im: z.im * c - z.re * s };
        }
        for k in 1..n / 2 { freq[n - k] = freq[k].conj(); }
        inverse.process(&mut freq);
        let mut response: Vec<f64> = freq.iter().map(|x| x.re / n as f64).collect();
        response.rotate_left(n / 2);
        let dc_component: f64 = response[n / 2..].iter().sum();
        // Match WORLD RemoveDCComponent exactly: the first half is replaced
        // by the compensating DC-removal shape; only the second half retains
        // the periodic response before subtraction.
        for i in 0..n / 2 { response[i] = -dc_component * dc[i]; }
        for i in n / 2..n { response[i] -= dc_component * dc[i]; }
        // WORLD's aperiodic response: white Gaussian noise is filtered by
        // the minimum-phase envelope of `spectral * ap^2`. Its inverse FFT
        // contribution is added without the periodic sqrt(noise_size) gain.
        let mut noise_response = vec![0.0; n];
        // C++ GetAperiodicResponse still synthesizes noise for an unvoiced
        // frame even when the supplied AP row is zero; in that case it uses
        // the full spectral envelope rather than `spectrum * ap^2`.
        let has_aperiodic = !voiced || ap[..bins].iter().any(|&x| x > 1e-12);
        if has_aperiodic {
            let mut noise_freq = vec![Complex::default(); n];
            let noise_len = noise_size.min(n);
            let mut mean = 0.0;
            for i in 0..noise_len { let value = next_world_noise(&mut rng); noise_freq[i].re = value; mean += value; }
            mean /= noise_len.max(1) as f64;
            for i in 0..noise_len { noise_freq[i].re -= mean; }
            noise_forward.process(&mut noise_freq);
            // C++ GetAperiodicResponse: log(spectrum * aperiodic_ratio)/2 —
            // the tension curve is NOT applied to the aperiodic response.
            let noise_log: Vec<f64> = spectrum.iter().zip(ap.iter()).map(|(&x, &r)| {
                let magnitude = if voiced { x * r * r } else { x };
                (magnitude + 1e-12).ln() / 2.0
            }).collect();
            let noise_min = minimum_phase_spectrum_with_plans(&noise_log, minimum_phase_forward.as_ref(), minimum_phase_inverse.as_ref());
            for k in 0..n { noise_freq[k] *= noise_min[k]; }
            noise_inverse.process(&mut noise_freq);
            for (dst, value) in noise_response.iter_mut().zip(noise_freq) { *dst = value.re / n as f64; }
            noise_response.rotate_left(n / 2);
        }
        let offset = index as isize - n as isize / 2 + 1;
        // rustfft's inverse is unnormalised and is normalised exactly once
        // above; WORLD applies its single /fft_size factor at this boundary.
        let scale = (noise_size as f64).sqrt();
        for (j, (&periodic, &aperiodic)) in response.iter().zip(noise_response.iter()).enumerate() {
            let value = periodic * voicing * scale + aperiodic * breathiness;
            let at = offset + j as isize;
            if at >= 0 && (at as usize) < output_len { output[at as usize] += value; }
        }
    }
    Ok(output.into_iter().map(|x| x as f32).collect())
}

fn next_noise(state: &mut u64) -> f64 {
    *state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    let unit = ((*state >> 11) as f64) / ((1u64 << 53) as f64);
    unit * 2.0 - 1.0
}

/// WORLD's portable xorshift/CLT normal generator. The fixed state makes the
/// Rust path reproducible while matching the algorithm used by `randn()`.
fn next_world_noise(state: &mut [u32; 4]) -> f64 {
    let t = state[0] ^ (state[0] << 11);
    state[0] = state[1]; state[1] = state[2]; state[2] = state[3];
    state[3] = (state[3] ^ (state[3] >> 19)) ^ (t ^ (t >> 8));
    let mut sum = state[3] >> 4;
    for _ in 0..11 {
        let t = state[0] ^ (state[0] << 11);
        state[0] = state[1]; state[1] = state[2]; state[2] = state[3];
        state[3] = (state[3] ^ (state[3] >> 19)) ^ (t ^ (t >> 8));
        sum = sum.wrapping_add(state[3] >> 4);
    }
    sum as f64 / 268435456.0 - 6.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WorldFeatures;

    #[test]
    fn synthesizes_deterministically() {
        let mut features = WorldFeatures::try_new(4, 33).unwrap();
        features.f0.fill(220.0);
        features.spectral.fill(0.1);
        features.aperiodicity.fill(0.2);
        let config = SynthesisConfig { fft_size: 64, frame_period_ms: 5.0, ..Default::default() };
        let a = synthesize(&features, config, &()).unwrap();
        let b = synthesize(&features, config, &()).unwrap();
        assert_eq!(a, b);
        assert!(a.iter().any(|x| x.abs() > 1e-8));
        assert_eq!(a.len(), 64 + 3 * 221);
    }

    #[test]
    fn cancellation_is_cooperative() {
        struct Stop;
        impl Cancellation for Stop { fn is_cancelled(&self) -> bool { true } }
        let features = WorldFeatures::try_new(1, 33).unwrap();
        assert_eq!(synthesize(&features, SynthesisConfig { fft_size: 64, ..Default::default() }, &Stop), Err(SynthesisError::Cancelled));
    }
}
