//! Functional Rust WORLD-style feature synthesis.
//!
//! This module is the executable synthesis boundary used by native/WASM hosts.
//! The implementation uses the same contiguous feature contract as analysis,
//! deterministic excitation, minimum-phase-like spectral shaping, and overlap
//! add. The upstream WORLD differential port will tighten each numerical stage
//! against the pinned C++ oracle without changing this public API.

use std::collections::VecDeque;
use std::sync::Arc;

use crate::fft::{Planner, RealFft};
use crate::{Cancellation, WorldFeatures};
use rustfft::num_complex::Complex;

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
        Self {
            sample_rate: 44100,
            frame_period_ms: 512.0 * 1000.0 / 44100.0,
            fft_size: 1024,
            seed: 0x574f524c44,
            tension: 1.0,
            breathiness: 1.0,
            voicing: 1.0,
            output_length: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SynthesisError {
    InvalidConfig,
    FeatureMismatch,
    Cancelled,
    Callback(String),
}

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
pub fn synthesize<C: Cancellation>(
    features: &WorldFeatures,
    config: SynthesisConfig,
    cancel: &C,
) -> Result<Vec<f32>, SynthesisError> {
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
    if config.sample_rate == 0
        || config.fft_size < 4
        || !config.fft_size.is_power_of_two()
        || config.frame_period_ms <= 0.0
        || features.bin_count != config.fft_size / 2 + 1
    {
        return Err(SynthesisError::InvalidConfig);
    }
    features
        .validate()
        .map_err(|_| SynthesisError::FeatureMismatch)?;
    if features.frame_count == 0 {
        return Ok(Vec::new());
    }
    let hop =
        ((config.sample_rate as f64 * config.frame_period_ms / 1000.0).round() as usize).max(1);
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
        if cancel.is_cancelled() {
            return Err(SynthesisError::Cancelled);
        }
        let row = frame_index * bins;
        let f0 = features.f0[frame_index];
        let phase_inc = if f0 > 0.0 {
            2.0 * std::f64::consts::PI * f0 / config.sample_rate as f64
        } else {
            0.0
        };
        // WORLD scales each pulse/noise response by sqrt(noise_size), where
        // noise_size is approximately one period in samples. The bring-up
        // path previously omitted this factor, making real voicebank renders
        // much quieter than the C++ renderer despite non-zero features.
        let response_scale = if f0 > 0.0 {
            (config.sample_rate as f64 / f0).sqrt()
        } else {
            1.0
        };
        let mut phase = 0.0;
        packed.fill(0.0);
        for k in 0..bins {
            let envelope = features.spectral[row + k].max(1e-12) * config.tension.max(0.0);
            let ap = features.aperiodicity[row + k].clamp(0.0, 1.0);
            let random = next_noise(&mut rng);
            let periodic = (1.0 - ap) * config.voicing.clamp(0.0, 1.0);
            let magnitude = envelope
                * response_scale
                * (periodic + ap * config.breathiness.max(0.0) * random.abs());
            if k == 0 {
                packed[0] = magnitude;
            } else if k == config.fft_size / 2 {
                packed[1] = magnitude;
            } else {
                // A deterministic phase ramp keeps adjacent frames stable;
                // noise contribution supplies the aperiodic component.
                let angle = phase + random * std::f64::consts::PI * ap;
                packed[2 * k] = magnitude * angle.cos();
                packed[2 * k + 1] = magnitude * angle.sin();
            }
            phase += phase_inc;
        }
        fft.inverse_packed(&packed, &mut frame)
            .map_err(|_| SynthesisError::InvalidConfig)?;
        let start = frame_index * hop;
        for i in 0..config.fft_size.min(output_len.saturating_sub(start)) {
            let window = (std::f64::consts::PI * (i as f64 + 0.5) / config.fft_size as f64)
                .sin()
                .powi(2);
            output[start + i] += frame[i] * window;
            weights[start + i] += window;
        }
    }
    for (sample, weight) in output.iter_mut().zip(weights) {
        if weight > 1e-12 {
            *sample /= weight;
        }
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
    let mut cepstrum: Vec<Complex<f64>> = log_spectrum
        .iter()
        .map(|&x| Complex { re: x, im: 0.0 })
        .collect();
    inverse.process(&mut cepstrum);
    for value in &mut cepstrum {
        value.im = -value.im;
    }
    for i in 1..n / 2 {
        cepstrum[i] = Complex {
            re: cepstrum[i].re * 2.0,
            im: cepstrum[i].im * -2.0,
        };
    }
    cepstrum[n / 2].im = -cepstrum[n / 2].im;
    for value in &mut cepstrum[n / 2 + 1..] {
        *value = Complex::default();
    }
    forward.process(&mut cepstrum);
    cepstrum
        .into_iter()
        .map(|x| {
            let z = (x.re / n as f64).exp();
            Complex {
                re: z * (x.im / n as f64).cos(),
                im: z * (x.im / n as f64).sin(),
            }
        })
        .collect()
}

struct PeriodicPlans {
    n: usize,
    bins: usize,
    fs: f64,
    frame_period: f64,
    lowest_f0: f64,
    dc: Vec<f64>,
    inverse: Arc<dyn rustfft::Fft<f64>>,
    noise_forward: Arc<dyn rustfft::Fft<f64>>,
    noise_inverse: Arc<dyn rustfft::Fft<f64>>,
    minimum_phase_forward: Arc<dyn rustfft::Fft<f64>>,
    minimum_phase_inverse: Arc<dyn rustfft::Fft<f64>>,
}

impl PeriodicPlans {
    fn new(config: &SynthesisConfig, bins: usize) -> Self {
        let n = config.fft_size;
        let fs = config.sample_rate as f64;
        let frame_period = config.frame_period_ms / 1000.0;
        let lowest_f0 = fs / n as f64 + 1.0;
        let mut dc = vec![0.0; n];
        let mut dc_component = 0.0;
        for i in 0..n / 2 {
            dc[i] =
                0.5 - 0.5 * (2.0 * std::f64::consts::PI * (i as f64 + 1.0) / (n as f64 + 1.0)).cos();
            dc[n - i - 1] = dc[i];
            dc_component += 2.0 * dc[i];
        }
        for x in &mut dc {
            *x /= dc_component;
        }
        let mut planner = Planner::new();
        Self {
            n,
            bins,
            fs,
            frame_period,
            lowest_f0,
            dc,
            inverse: planner.inverse(n),
            noise_forward: planner.forward(n),
            noise_inverse: planner.inverse(n),
            minimum_phase_forward: planner.forward(n),
            minimum_phase_inverse: planner.inverse(n),
        }
    }

    fn coarse_f0(
        &self,
        i: usize,
        frame_base: usize,
        avail_frames: usize,
        f0_slice: &[f64],
    ) -> f64 {
        let value = if i < avail_frames {
            f0_slice[i - frame_base]
        } else if avail_frames > 1 {
            f0_slice[avail_frames - 1 - frame_base] * 2.0
                - f0_slice[avail_frames - 2 - frame_base]
        } else {
            f0_slice[avail_frames - 1 - frame_base]
        };
        if value >= self.lowest_f0 {
            value
        } else {
            0.0
        }
    }

    fn sample_f0(
        &self,
        sample: usize,
        frame_base: usize,
        avail_frames: usize,
        f0_slice: &[f64],
    ) -> f64 {
        let t = sample as f64 / self.fs;
        let pos = t / self.frame_period;
        let floor = pos.floor().max(0.0) as usize;
        let ceil = pos.ceil().max(0.0) as usize;
        let interpolation = pos - floor as f64;
        let floor_f0 = self.coarse_f0(floor, frame_base, avail_frames, f0_slice);
        let ceil_f0 = self.coarse_f0(ceil, frame_base, avail_frames, f0_slice);
        let value = floor_f0 + (ceil_f0 - floor_f0) * interpolation;
        let floor_vuv = if floor_f0 == 0.0 { 0.0 } else { 1.0 };
        let ceil_vuv = if ceil_f0 == 0.0 { 0.0 } else { 1.0 };
        let vuv = floor_vuv + (ceil_vuv - floor_vuv) * interpolation;
        if vuv > 0.5 {
            value
        } else {
            500.0
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_pulse(
        &self,
        index: usize,
        shift: f64,
        noise_size: usize,
        frame_base: usize,
        known_frames: usize,
        f0_slice: &[f64],
        sp_slice: &[f64],
        ap_slice: &[f64],
        curves: &ExpressionCurves,
        rng: &mut [u32; 4],
        out_pulse: &mut [f64],
    ) {
        let n = self.n;
        let bins = self.bins;
        let time = index as f64 / self.fs;
        let pos = time / self.frame_period;
        let lo = (pos.floor().max(0.0) as usize).min(known_frames - 1);
        let hi = (pos.ceil().max(0.0) as usize).min(known_frames - 1);
        let a = if lo == hi { 0.0 } else { pos - lo as f64 };
        let lo_local = lo - frame_base;
        let hi_local = hi - frame_base;
        let mut spectrum = vec![0.0; n];
        let mut ap = vec![0.0; n];
        for k in 0..bins {
            spectrum[k] = ((1.0 - a) * sp_slice[lo_local * bins + k]
                + a * sp_slice[hi_local * bins + k])
                .abs();
        }
        for k in 0..bins {
            ap[k] = ((1.0 - a) * ap_slice[lo_local * bins + k]
                + a * ap_slice[hi_local * bins + k])
                .clamp(0.0, 1.0);
        }
        for k in (bins..n).rev() {
            spectrum[k] = spectrum[n - k];
        }
        for k in (bins..n).rev() {
            ap[k] = ap[n - k];
        }
        let lo_vuv = if self.coarse_f0(lo, frame_base, known_frames, f0_slice) == 0.0 {
            0.0
        } else {
            1.0
        };
        let hi_vuv = if self.coarse_f0(hi, frame_base, known_frames, f0_slice) == 0.0 {
            0.0
        } else {
            1.0
        };
        let current_vuv = lo_vuv + (hi_vuv - lo_vuv) * a;
        let voiced = current_vuv > 0.5;
        let curve_frame = lo_local;
        let tension = curves.tension_at(curve_frame);
        let breathiness = curves.breathiness_at(curve_frame);
        let voicing = curves.voicing_at(curve_frame);
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
                self.minimum_phase_forward.as_ref(),
                self.minimum_phase_inverse.as_ref(),
            )
        } else {
            vec![Complex::default(); n]
        };
        let coefficient = 2.0 * std::f64::consts::PI * shift * self.fs / n as f64;
        let mut freq = min_phase;
        for k in 0..=n / 2 {
            let c = (coefficient * k as f64).cos();
            let s = (1.0 - c * c).max(0.0).sqrt();
            let z = freq[k];
            freq[k] = Complex {
                re: z.re * c + z.im * s,
                im: z.im * c - z.re * s,
            };
        }
        for k in 1..n / 2 {
            freq[n - k] = freq[k].conj();
        }
        self.inverse.process(&mut freq);
        let mut response: Vec<f64> = freq.iter().map(|x| x.re / n as f64).collect();
        response.rotate_left(n / 2);
        let dc_component: f64 = response[n / 2..].iter().sum();
        for i in 0..n / 2 {
            response[i] = -dc_component * self.dc[i];
        }
        for i in n / 2..n {
            response[i] -= dc_component * self.dc[i];
        }
        let mut noise_response = vec![0.0; n];
        let has_aperiodic = !voiced || ap[..bins].iter().any(|&x| x > 1e-12);
        if has_aperiodic {
            let mut noise_freq = vec![Complex::default(); n];
            let noise_len = noise_size.min(n);
            let mut mean = 0.0;
            for i in 0..noise_len {
                let value = next_world_noise(rng);
                noise_freq[i].re = value;
                mean += value;
            }
            mean /= noise_len.max(1) as f64;
            for i in 0..noise_len {
                noise_freq[i].re -= mean;
            }
            self.noise_forward.process(&mut noise_freq);
            let noise_log: Vec<f64> = spectrum
                .iter()
                .zip(ap.iter())
                .map(|(&x, &r)| {
                    let magnitude = if voiced { x * r * r } else { x };
                    (magnitude + 1e-12).ln() / 2.0
                })
                .collect();
            let noise_min = minimum_phase_spectrum_with_plans(
                &noise_log,
                self.minimum_phase_forward.as_ref(),
                self.minimum_phase_inverse.as_ref(),
            );
            for k in 0..n {
                noise_freq[k] *= noise_min[k];
            }
            self.noise_inverse.process(&mut noise_freq);
            for (dst, value) in noise_response.iter_mut().zip(noise_freq) {
                *dst = value.re / n as f64;
            }
            noise_response.rotate_left(n / 2);
        }
        let scale = (noise_size as f64).sqrt();
        for j in 0..n {
            out_pulse[j] = response[j] * voicing * scale + noise_response[j] * breathiness;
        }
    }
}

/// Stateful continuous pure-Rust WORLD synthesis cursor matching
/// `synthesize_with_curves` sample-for-sample with O(block_size + fft_size)
/// output retention and bounded sliding feature windows.
pub struct WorldStreamCursorRs {
    plans: PeriodicPlans,
    block_size: usize,
    sample_idx: usize,
    phase: f64,
    previous_wrap: f64,
    pending_pulses: VecDeque<(usize, f64)>,
    rng: [u32; 4],
    ola: VecDeque<f64>,
    emitted: usize,
    finished: bool,
}

impl WorldStreamCursorRs {
    pub fn new(config: SynthesisConfig, bins: usize, block_size: usize) -> Result<Self, SynthesisError> {
        if config.sample_rate == 0
            || config.fft_size < 4
            || !config.fft_size.is_power_of_two()
            || config.frame_period_ms <= 0.0
            || bins != config.fft_size / 2 + 1
        {
            return Err(SynthesisError::InvalidConfig);
        }
        Ok(Self {
            plans: PeriodicPlans::new(&config, bins),
            block_size: block_size.max(1),
            sample_idx: 0,
            phase: 0.0,
            previous_wrap: 0.0,
            pending_pulses: VecDeque::new(),
            rng: [123456789u32, 362436069, 521288629, 88675123],
            ola: VecDeque::new(),
            emitted: 0,
            finished: false,
        })
    }

    /// Feed finalized feature frames `[frame_base .. avail_frames)` and advance
    /// synthesis. Returns the smallest frame index still required so callers
    /// can evict `[frame_base .. returned)`.
    #[allow(clippy::too_many_arguments)]
    pub fn feed_f64<C: Cancellation, F>(
        &mut self,
        frame_base: usize,
        avail_frames: usize,
        final_total_frames: Option<usize>,
        final_output_len: Option<usize>,
        f0_slice: &[f64],
        sp_slice: &[f64],
        ap_slice: &[f64],
        curves: &ExpressionCurves,
        cancel: &C,
        mut emit: F,
    ) -> Result<usize, SynthesisError>
    where
        F: FnMut(&[f64]) -> Result<(), String>,
    {
        if avail_frames < frame_base {
            return Err(SynthesisError::FeatureMismatch);
        }
        if self.finished {
            return Ok(avail_frames);
        }
        let is_final = final_total_frames.is_some() || final_output_len.is_some();
        let total_frames = final_total_frames.unwrap_or(avail_frames);
        let final_len = final_output_len.unwrap_or(usize::MAX);
        if is_final && (final_len == 0 || total_frames == 0) {
            self.finished = true;
            return Ok(avail_frames);
        }
        if !is_final && avail_frames.saturating_sub(frame_base) < 2 {
            return Ok(frame_base);
        }

        let bins = self.plans.bins;
        let expected_local = avail_frames - frame_base;
        if f0_slice.len() < expected_local
            || sp_slice.len() < expected_local * bins
            || ap_slice.len() < expected_local * bins
        {
            return Err(SynthesisError::FeatureMismatch);
        }

        // Advance time base & pulse queue using exact same recurrence as offline.
        while !is_final || self.sample_idx < final_len {
            if cancel.is_cancelled() {
                return Err(SynthesisError::Cancelled);
            }
            if !is_final {
                let t = self.sample_idx as f64 / self.plans.fs;
                let ceil = (t / self.plans.frame_period).ceil().max(0.0) as usize;
                if ceil >= avail_frames {
                    break;
                }
            }
            let f0_s = self.plans.sample_f0(self.sample_idx, frame_base, avail_frames, f0_slice);
            if self.sample_idx == 0 {
                self.phase = 2.0 * std::f64::consts::PI * f0_s / self.plans.fs;
                self.previous_wrap = self.phase.rem_euclid(2.0 * std::f64::consts::PI);
            } else {
                let prev_idx = self.sample_idx - 1;
                self.phase += 2.0 * std::f64::consts::PI * f0_s / self.plans.fs;
                let wrap = self.phase.rem_euclid(2.0 * std::f64::consts::PI);
                if (wrap - self.previous_wrap).abs() > std::f64::consts::PI {
                    let y1 = self.previous_wrap - 2.0 * std::f64::consts::PI;
                    let x = -y1 / (wrap - y1);
                    self.pending_pulses.push_back((prev_idx, x / self.plans.fs));
                }
                self.previous_wrap = wrap;
            }
            self.sample_idx += 1;
        }

        let n = self.plans.n;
        let mut pulse_buf = vec![0.0; n];
        let mut emit_scratch = Vec::with_capacity(self.block_size);

        while let Some(&(index, shift)) = self.pending_pulses.front() {
            if cancel.is_cancelled() {
                return Err(SynthesisError::Cancelled);
            }
            let noise_size = if self.pending_pulses.len() >= 2 {
                self.pending_pulses[1].0.saturating_sub(index)
            } else if is_final && self.sample_idx >= final_len {
                0
            } else {
                break;
            };
            if noise_size == 0 {
                self.pending_pulses.pop_front();
                continue;
            }
            let time = index as f64 / self.plans.fs;
            let pos = time / self.plans.frame_period;
            let hi_raw = pos.ceil().max(0.0) as usize;
            if !is_final && hi_raw >= avail_frames {
                break;
            }
            let known_frames = if is_final { total_frames } else { avail_frames };

            self.plans.render_pulse(
                index,
                shift,
                noise_size,
                frame_base,
                known_frames,
                f0_slice,
                sp_slice,
                ap_slice,
                curves,
                &mut self.rng,
                &mut pulse_buf,
            );
            self.pending_pulses.pop_front();

            let offset = index as isize - n as isize / 2 + 1;
            for (j, &value) in pulse_buf.iter().enumerate() {
                let at = offset + j as isize;
                if at < 0 {
                    continue;
                }
                let at_u = at as usize;
                if is_final && at_u >= final_len {
                    continue;
                }
                if at_u < self.emitted {
                    continue;
                }
                let rel = at_u - self.emitted;
                if rel >= self.ola.len() {
                    self.ola.resize(rel + 1, 0.0);
                }
                self.ola[rel] += value;
            }

            let safe_end = if let Some(&(next_idx, _)) = self.pending_pulses.front() {
                let raw = (next_idx as isize - n as isize / 2 + 1).max(0) as usize;
                if is_final {
                    raw.min(final_len)
                } else {
                    raw.min(self.sample_idx)
                }
            } else {
                final_len
            };
            if safe_end > self.emitted + self.ola.len() {
                self.ola.resize(safe_end - self.emitted, 0.0);
            }
            while safe_end.saturating_sub(self.emitted) >= self.block_size {
                emit_scratch.clear();
                for _ in 0..self.block_size {
                    emit_scratch.push(self.ola.pop_front().unwrap_or(0.0));
                }
                self.emitted += self.block_size;
                emit(&emit_scratch).map_err(SynthesisError::Callback)?;
            }
        }

        if is_final {
            if self.emitted < final_len {
                let rem = final_len - self.emitted;
                if self.ola.len() < rem {
                    self.ola.resize(rem, 0.0);
                }
                while self.emitted < final_len {
                    let chunk = (final_len - self.emitted).min(self.block_size);
                    emit_scratch.clear();
                    for _ in 0..chunk {
                        emit_scratch.push(self.ola.pop_front().unwrap_or(0.0));
                    }
                    self.emitted += chunk;
                    emit(&emit_scratch).map_err(SynthesisError::Callback)?;
                }
            }
            self.finished = true;
            return Ok(avail_frames);
        }

        let sample_floor = ((self.sample_idx as f64 / self.plans.fs) / self.plans.frame_period)
            .floor()
            .max(0.0) as usize;
        let pulse_floor = self
            .pending_pulses
            .front()
            .map(|p| ((p.0 as f64 / self.plans.fs) / self.plans.frame_period).floor().max(0.0) as usize)
            .unwrap_or(sample_floor);
        let safe_evict = avail_frames
            .saturating_sub(2)
            .min(sample_floor.saturating_sub(1))
            .min(pulse_floor.saturating_sub(1))
            .max(frame_base);
        Ok(safe_evict)
    }
}

fn synthesize_periodic_world<C: Cancellation>(
    features: &WorldFeatures,
    config: SynthesisConfig,
    curves: &ExpressionCurves,
    output_len: usize,
    cancel: &C,
) -> Result<Vec<f32>, SynthesisError> {
    let plans = PeriodicPlans::new(&config, features.bin_count);
    let mut output = vec![0.0; output_len];

    let mut phase =
        2.0 * std::f64::consts::PI * plans.sample_f0(0, 0, features.frame_count, &features.f0) / plans.fs;
    let mut previous_wrap = phase.rem_euclid(2.0 * std::f64::consts::PI);
    let mut pulses = Vec::new();
    for i in 0..output_len.saturating_sub(1) {
        if cancel.is_cancelled() {
            return Err(SynthesisError::Cancelled);
        }
        phase += 2.0
            * std::f64::consts::PI
            * plans.sample_f0(i + 1, 0, features.frame_count, &features.f0)
            / plans.fs;
        let wrap = phase.rem_euclid(2.0 * std::f64::consts::PI);
        if (wrap - previous_wrap).abs() > std::f64::consts::PI {
            let y1 = previous_wrap - 2.0 * std::f64::consts::PI;
            let x = -y1 / (wrap - y1);
            pulses.push((i, x / plans.fs));
        }
        previous_wrap = wrap;
    }
    let mut rng = [123456789u32, 362436069, 521288629, 88675123];
    let mut pulse_buf = vec![0.0; plans.n];
    for (pulse_no, &(index, shift)) in pulses.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(SynthesisError::Cancelled);
        }
        let noise_size = pulses
            .get(pulse_no + 1)
            .map(|p| p.0.saturating_sub(index))
            .unwrap_or(0);
        if noise_size == 0 {
            continue;
        }
        plans.render_pulse(
            index,
            shift,
            noise_size,
            0,
            features.frame_count,
            &features.f0,
            &features.spectral,
            &features.aperiodicity,
            curves,
            &mut rng,
            &mut pulse_buf,
        );
        let offset = index as isize - plans.n as isize / 2 + 1;
        for (j, &value) in pulse_buf.iter().enumerate() {
            let at = offset + j as isize;
            if at >= 0 && (at as usize) < output_len {
                output[at as usize] += value;
            }
        }
    }
    Ok(output.into_iter().map(|x| x as f32).collect())
}

fn next_noise(state: &mut u64) -> f64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let unit = ((*state >> 11) as f64) / ((1u64 << 53) as f64);
    unit * 2.0 - 1.0
}

/// WORLD's portable xorshift/CLT normal generator. The fixed state makes the
/// Rust path reproducible while matching the algorithm used by `randn()`.
fn next_world_noise(state: &mut [u32; 4]) -> f64 {
    let t = state[0] ^ (state[0] << 11);
    state[0] = state[1];
    state[1] = state[2];
    state[2] = state[3];
    state[3] = (state[3] ^ (state[3] >> 19)) ^ (t ^ (t >> 8));
    let mut sum = state[3] >> 4;
    for _ in 0..11 {
        let t = state[0] ^ (state[0] << 11);
        state[0] = state[1];
        state[1] = state[2];
        state[2] = state[3];
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
        let config = SynthesisConfig {
            fft_size: 64,
            frame_period_ms: 5.0,
            ..Default::default()
        };
        let a = synthesize(&features, config, &()).unwrap();
        let b = synthesize(&features, config, &()).unwrap();
        assert_eq!(a, b);
        assert!(a.iter().any(|x| x.abs() > 1e-8));
        assert_eq!(a.len(), 64 + 3 * 221);
    }

    #[test]
    fn stream_cursor_matches_offline_bit_for_bit() {
        let frames = 60;
        let fft_size = 256;
        let bins = fft_size / 2 + 1;
        let mut features = WorldFeatures::try_new(frames, bins).unwrap();
        for f in 0..frames {
            features.f0[f] = if f < 8 || f > 52 {
                0.0
            } else {
                220.0 + (f as f64) * 2.5
            };
            for k in 0..bins {
                features.spectral[f * bins + k] = 0.05 / (1.0 + k as f64 * 0.1);
                features.aperiodicity[f * bins + k] = 0.15 + (k as f64 / bins as f64) * 0.3;
            }
        }
        let mut tension_bins = Vec::with_capacity(frames * bins);
        for f in 0..frames {
            for _ in 0..bins {
                tension_bins.push(0.9 + (f as f64 % 5.0) * 0.05);
            }
        }
        let curves = ExpressionCurves {
            tension: vec![1.0; frames],
            tension_bins: Some(tension_bins.clone()),
            breathiness: (0..frames).map(|f| 0.8 + (f % 3) as f64 * 0.1).collect(),
            voicing: (0..frames).map(|f| if f < 10 { 0.8 } else { 1.0 }).collect(),
        };
        let out_len = (44100.0 * (frames - 1) as f64 * 10.0 / 1000.0) as usize + 1;
        let config = SynthesisConfig {
            sample_rate: 44100,
            fft_size,
            frame_period_ms: 10.0,
            output_length: Some(out_len),
            ..Default::default()
        };
        let offline = synthesize_with_curves(&features, config, &curves, &()).unwrap();

        let mut cursor = WorldStreamCursorRs::new(config, bins, 256).unwrap();
        let mut streamed_f64 = Vec::new();
        let mut frame_base = 0usize;
        for avail in [12usize, 24, 38, frames] {
            let is_final = avail == frames;
            let local_curves = ExpressionCurves {
                tension: curves.tension[frame_base..avail].to_vec(),
                tension_bins: Some(tension_bins[frame_base * bins..avail * bins].to_vec()),
                breathiness: curves.breathiness[frame_base..avail].to_vec(),
                voicing: curves.voicing[frame_base..avail].to_vec(),
            };
            frame_base = cursor
                .feed_f64(
                    frame_base,
                    avail,
                    is_final.then_some(frames),
                    is_final.then_some(out_len),
                    &features.f0[frame_base..avail],
                    &features.spectral[frame_base * bins..avail * bins],
                    &features.aperiodicity[frame_base * bins..avail * bins],
                    &local_curves,
                    &(),
                    |blk| {
                        streamed_f64.extend_from_slice(blk);
                        Ok(())
                    },
                )
                .unwrap();
        }
        let streamed: Vec<f32> = streamed_f64.into_iter().map(|s| s as f32).collect();
        assert_eq!(offline.len(), streamed.len());
        let max_err = offline
            .iter()
            .zip(&streamed)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert_eq!(max_err, 0.0, "expected bit-exact parity");
    }

    #[test]
    fn cancellation_is_cooperative() {
        struct Stop;
        impl Cancellation for Stop {
            fn is_cancelled(&self) -> bool {
                true
            }
        }
        let features = WorldFeatures::try_new(1, 33).unwrap();
        assert_eq!(
            synthesize(
                &features,
                SynthesisConfig {
                    fft_size: 64,
                    ..Default::default()
                },
                &Stop
            ),
            Err(SynthesisError::Cancelled)
        );
    }
}
