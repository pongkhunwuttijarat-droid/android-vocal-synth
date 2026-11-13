//! D4C aperiodicity stage boundary.
//!
//! D4C produces a frequency-dependent aperiodicity ratio in [0, 1]. This
//! module exposes the production-shaped API and deterministic frame handling;
//! the full LoveTrain band/group-delay body is the next differential port.

use crate::Cancellation;
use rustfft::num_complex::Complex;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct D4cOption {
    pub threshold: f64,
    pub floor_f0: f64,
}

impl Default for D4cOption {
    fn default() -> Self {
        Self {
            threshold: 0.0,
            floor_f0: 47.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum D4cError {
    InvalidInput,
    Cancelled,
}

fn next_randn(s: &mut [u32; 4]) -> f64 {
    let t = s[0] ^ (s[0] << 11);
    s[0] = s[1];
    s[1] = s[2];
    s[2] = s[3];
    s[3] = (s[3] ^ (s[3] >> 19)) ^ (t ^ (t >> 8));
    let mut sum = s[3] >> 4;
    for _ in 0..11 {
        let t = s[0] ^ (s[0] << 11);
        s[0] = s[1];
        s[1] = s[2];
        s[2] = s[3];
        s[3] = (s[3] ^ (s[3] >> 19)) ^ (t ^ (t >> 8));
        sum = sum.wrapping_add(s[3] >> 4);
    }
    sum as f64 / 268435456.0 - 6.0
}

fn nuttall_window(n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| {
            let p = 2.0 * std::f64::consts::PI * i as f64 / (n - 1).max(1) as f64;
            0.355768 - 0.487396 * p.cos() + 0.144232 * (2.0 * p).cos() - 0.012604 * (3.0 * p).cos()
        })
        .collect()
}

#[derive(Default)]
struct D4cWorkspace {
    window_a: Vec<f64>,
    window_b: Vec<f64>,
    window_weights: Vec<f64>,
    fft_a: Vec<Complex<f64>>,
    fft_b: Vec<Complex<f64>>,
    spectrum: Vec<f64>,
    sorted_spectrum: Vec<f64>,
    cumulative: Vec<f64>,
    centroid: Vec<f64>,
    smoothed: Vec<f64>,
    power: Vec<f64>,
    group: Vec<f64>,
    smooth_group: Vec<f64>,
    group2: Vec<f64>,
    segment: Vec<f64>,
    coarse: Vec<f64>,
}

fn windowed(
    samples: &[f32],
    fs: u32,
    f0: f64,
    pos: f64,
    ratio: f64,
    blackman: bool,
    rng: &mut [u32; 4],
    y: &mut Vec<f64>,
    weights: &mut Vec<f64>,
) -> usize {
    let half = crate::helpers::matlab_round(ratio * fs as f64 / f0 / 2.0).max(1) as usize;
    let len = half * 2 + 1;
    let origin = crate::helpers::matlab_round(pos * fs as f64 + 0.001) as i64;
    y.resize(len, 0.0);
    weights.resize(len, 0.0);
    let mut sum = 0.0;
    let mut weight_sum = 0.0;
    for j in 0..len {
        let rel = j as f64 - half as f64;
        let idx = (origin + rel as i64).clamp(0, samples.len().saturating_sub(1) as i64) as usize;
        let a = std::f64::consts::PI * (2.0 * rel / ratio / fs as f64) * f0;
        let w = if blackman {
            0.42 + 0.5 * a.cos() + 0.08 * (2.0 * a).cos()
        } else {
            0.5 + 0.5 * a.cos()
        };
        weights[j] = w;
        y[j] = samples[idx] as f64 * w + next_randn(rng) * 1e-6;
        sum += y[j];
        weight_sum += w;
    }
    let mean = sum / weight_sum.max(1e-12);
    for (value, &weight) in y.iter_mut().zip(weights.iter()) {
        *value -= weight * mean;
    }
    len
}

fn power_spectrum(input: &[f64], fft_buffer: &mut Vec<Complex<f64>>, output: &mut Vec<f64>) {
    let n = input.len();
    let fft = crate::fft::forward_plan(n);
    fft_buffer.resize(n, Complex::default());
    for (slot, &x) in fft_buffer.iter_mut().zip(input) {
        *slot = Complex { re: x, im: 0.0 };
    }
    fft.process(fft_buffer);
    output.resize(n / 2 + 1, 0.0);
    for (dst, value) in output.iter_mut().zip(&fft_buffer[..=n / 2]) {
        *dst = value.norm_sqr();
    }
}

fn static_centroid(
    samples: &[f32],
    fs: u32,
    f0: f64,
    pos: f64,
    n: usize,
    rng: &mut [u32; 4],
    dc_workspace: &mut crate::helpers::DcCorrectionWorkspace,
    workspace: &mut D4cWorkspace,
) {
    windowed(
        samples,
        fs,
        f0,
        pos - 0.25 / f0,
        4.0,
        true,
        rng,
        &mut workspace.window_a,
        &mut workspace.window_weights,
    );
    windowed(
        samples,
        fs,
        f0,
        pos + 0.25 / f0,
        4.0,
        true,
        rng,
        &mut workspace.window_b,
        &mut workspace.window_weights,
    );
    workspace.centroid.resize(n / 2 + 1, 0.0);
    workspace.centroid.fill(0.0);
    let f = crate::fft::forward_plan(n);
    for window in [&workspace.window_a, &workspace.window_b] {
        let energy = window.iter().map(|v| v * v).sum::<f64>().sqrt().max(1e-12);
        workspace.fft_a.resize(n, Complex::default());
        workspace.fft_b.resize(n, Complex::default());
        workspace.fft_a.fill(Complex::default());
        workspace.fft_b.fill(Complex::default());
        for (i, &v) in window.iter().enumerate() {
            workspace.fft_a[i].re = v / energy;
            workspace.fft_b[i].re = v * (i as f64 + 1.0) / energy;
        }
        f.process(&mut workspace.fft_a);
        f.process(&mut workspace.fft_b);
        for k in 0..=n / 2 {
            workspace.centroid[k] += workspace.fft_b[k].re * workspace.fft_a[k].re
                + workspace.fft_b[k].im * workspace.fft_a[k].im;
        }
    }
    // WORLD applies DCCorrection to the summed centroid before the group
    // delay is formed. Omitting this low-frequency mirror materially changes
    // D4C around voiced fundamentals.
    crate::helpers::dc_correction_with_workspace(&mut workspace.centroid, f0, fs, n, dc_workspace);
}

fn smoothed_power(
    samples: &[f32],
    fs: u32,
    f0: f64,
    pos: f64,
    n: usize,
    rng: &mut [u32; 4],
    dc_workspace: &mut crate::helpers::DcCorrectionWorkspace,
    smoothing_workspace: &mut crate::helpers::LinearSmoothingWorkspace,
    workspace: &mut D4cWorkspace,
) {
    let len = windowed(
        samples,
        fs,
        f0,
        pos,
        4.0,
        false,
        rng,
        &mut workspace.window_a,
        &mut workspace.window_weights,
    );
    workspace.segment.resize(n, 0.0);
    workspace.segment.fill(0.0);
    workspace.segment[..len].copy_from_slice(&workspace.window_a[..len]);
    power_spectrum(
        &workspace.segment,
        &mut workspace.fft_a,
        &mut workspace.power,
    );
    crate::helpers::dc_correction_with_workspace(&mut workspace.power, f0, fs, n, dc_workspace);
    workspace.smoothed.resize(workspace.power.len(), 0.0);
    crate::helpers::linear_smoothing_with_workspace(
        &workspace.power,
        f0,
        fs,
        n,
        &mut workspace.smoothed,
        smoothing_workspace,
    );
}

fn love_train(
    samples: &[f32],
    fs: u32,
    f0: f64,
    pos: f64,
    n: usize,
    rng: &mut [u32; 4],
    workspace: &mut D4cWorkspace,
) -> f64 {
    let len = windowed(
        samples,
        fs,
        f0,
        pos,
        3.0,
        true,
        rng,
        &mut workspace.window_a,
        &mut workspace.window_weights,
    );
    workspace.segment.resize(n, 0.0);
    workspace.segment.fill(0.0);
    workspace.segment[..len.min(n)].copy_from_slice(&workspace.window_a[..len.min(n)]);
    power_spectrum(
        &workspace.segment,
        &mut workspace.fft_a,
        &mut workspace.power,
    );
    let b0 = (100.0 * n as f64 / fs as f64).ceil() as usize;
    let b1 = (4000.0 * n as f64 / fs as f64).ceil() as usize;
    let b2 = (7900.0 * n as f64 / fs as f64).ceil() as usize;
    workspace.cumulative.resize(workspace.power.len(), 0.0);
    workspace.cumulative.fill(0.0);
    for i in (b0 + 1)..=b2.min(workspace.power.len() - 1) {
        workspace.cumulative[i] = workspace.cumulative[i - 1] + workspace.power[i];
    }
    (workspace.cumulative[b1.min(workspace.cumulative.len() - 1)]
        / workspace.cumulative[b2.min(workspace.cumulative.len() - 1)].max(1e-30))
    .clamp(0.0, 1.0)
}

/// Differential-port hook: return the four D4C intermediate stages for one
/// frame. Kept public so the native oracle harness can compare stages without
/// changing the production `analyze` contract.
#[doc(hidden)]
pub fn probe_stages(
    samples: &[f32],
    fs: u32,
    f0: f64,
    pos: f64,
    _fft_size: usize,
) -> (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>) {
    let d4c_fft = (2.0f64.powf((1.0 + (4.0 * fs as f64 / 47.0 + 1.0).log2()).floor())) as usize;
    let current_f0 = f0.max(47.0);
    let mut rng = [123456789, 362436069, 521288629, 88675123];
    let mut dc_workspace = crate::helpers::DcCorrectionWorkspace::default();
    let mut smoothing_workspace = crate::helpers::LinearSmoothingWorkspace::default();
    let mut workspace = D4cWorkspace::default();
    static_centroid(
        samples,
        fs,
        current_f0,
        pos,
        d4c_fft,
        &mut rng,
        &mut dc_workspace,
        &mut workspace,
    );
    smoothed_power(
        samples,
        fs,
        current_f0,
        pos,
        d4c_fft,
        &mut rng,
        &mut dc_workspace,
        &mut smoothing_workspace,
        &mut workspace,
    );
    workspace.group.resize(d4c_fft / 2 + 1, 0.0);
    for k in 0..workspace.group.len() {
        workspace.group[k] = workspace.centroid[k] / workspace.smoothed[k].max(1e-12);
    }
    let mut smooth_group = vec![0.0; workspace.group.len()];
    let mut group2 = vec![0.0; workspace.group.len()];
    crate::helpers::linear_smoothing(
        &workspace.group,
        current_f0 / 2.0,
        fs,
        d4c_fft,
        &mut smooth_group,
    );
    crate::helpers::linear_smoothing(&smooth_group, current_f0, fs, d4c_fft, &mut group2);
    for k in 0..workspace.group.len() {
        workspace.group[k] = smooth_group[k] - group2[k];
    }
    let bands = ((15000.0f64).min(fs as f64 / 2.0 - 3000.0) / 3000.0) as usize;
    let wl = ((3000.0 * d4c_fft as f64 / fs as f64) as usize) * 2 + 1;
    let win = nuttall_window(wl);
    let boundary = crate::helpers::matlab_round(d4c_fft as f64 * 8.0 / wl as f64).max(0) as usize;
    let f = crate::fft::forward_plan(d4c_fft);
    workspace.coarse.resize(bands + 2, 0.0);
    workspace.coarse.fill(0.0);
    for c in 0..bands {
        let center = 3000 * (c + 1) * d4c_fft / (fs as usize);
        let half = wl / 2;
        workspace.segment.resize(d4c_fft, 0.0);
        workspace.segment.fill(0.0);
        for j in 0..wl {
            workspace.segment[j] = workspace.group[center - half + j] * win[j];
        }
        workspace.fft_b.resize(d4c_fft, Complex::default());
        workspace.fft_b.fill(Complex::default());
        for (dst, &value) in workspace.fft_b.iter_mut().zip(&workspace.segment) {
            dst.re = value;
        }
        f.process(&mut workspace.fft_b);
        workspace.sorted_spectrum.resize(d4c_fft / 2 + 1, 0.0);
        for (dst, value) in workspace
            .sorted_spectrum
            .iter_mut()
            .zip(&workspace.fft_b[..=d4c_fft / 2])
        {
            *dst = value.norm_sqr();
        }
        workspace
            .sorted_spectrum
            .sort_by(|a, b| a.partial_cmp(b).unwrap());
        for j in 1..workspace.sorted_spectrum.len() {
            workspace.sorted_spectrum[j] += workspace.sorted_spectrum[j - 1];
        }
        workspace.coarse[c + 1] = 10.0
            * (workspace.sorted_spectrum[d4c_fft / 2 - boundary - 1]
                / workspace.sorted_spectrum[d4c_fft / 2].max(1e-30))
            .log10();
        workspace.coarse[c + 1] = (workspace.coarse[c + 1] + (current_f0 - 100.0) / 50.0).min(0.0);
    }
    workspace.coarse[0] = -60.0;
    workspace.coarse[bands + 1] = -1e-12;
    (
        workspace.centroid,
        workspace.smoothed,
        workspace.group,
        workspace.coarse,
    )
}

/// Produce a row-major aperiodicity spectrogram from PCM and an F0 contour.
pub fn analyze<C: Cancellation>(
    samples: &[f32],
    sample_rate: u32,
    time: &[f64],
    f0: &[f64],
    fft_size: usize,
    option: D4cOption,
    cancel: &C,
) -> Result<Vec<f64>, D4cError> {
    if sample_rate == 0 || fft_size < 4 || !fft_size.is_power_of_two() || time.len() != f0.len() {
        return Err(D4cError::InvalidInput);
    }
    let bins = fft_size / 2 + 1;
    let mut output = vec![1.0; f0.len() * bins];
    let d4c_fft =
        (2.0f64.powf((1.0 + (4.0 * sample_rate as f64 / 47.0 + 1.0).log2()).floor())) as usize;
    let coarse_count = ((15000.0f64).min(sample_rate as f64 / 2.0 - 3000.0) / 3000.0) as usize;
    let window_len = ((3000.0 * d4c_fft as f64 / sample_rate as f64) as usize) * 2 + 1;
    let window = nuttall_window(window_len);
    let mut rng = [123456789u32, 362436069, 521288629, 88675123];
    let mut dc_workspace = crate::helpers::DcCorrectionWorkspace::default();
    let mut smoothing_workspace = crate::helpers::LinearSmoothingWorkspace::default();
    let mut workspace = D4cWorkspace::default();
    for (frame, (&position, &pitch)) in time.iter().zip(f0).enumerate() {
        if cancel.is_cancelled() {
            return Err(D4cError::Cancelled);
        }
        // WORLD's D4CLoveTrain/D4C split only on f0 == 0. Low non-zero F0
        // values are still processed, with the general body clamped to the
        // D4C floor internally.
        if pitch <= 0.0 {
            continue;
        }
        let ap0 = love_train(
            samples,
            sample_rate,
            pitch.max(47.0),
            position,
            d4c_fft,
            &mut rng,
            &mut workspace,
        );
        if pitch <= 0.0 || ap0 <= option.threshold {
            continue;
        }
        let current_f0 = pitch.max(47.0);
        static_centroid(
            samples,
            sample_rate,
            current_f0,
            position,
            d4c_fft,
            &mut rng,
            &mut dc_workspace,
            &mut workspace,
        );
        smoothed_power(
            samples,
            sample_rate,
            current_f0,
            position,
            d4c_fft,
            &mut rng,
            &mut dc_workspace,
            &mut smoothing_workspace,
            &mut workspace,
        );
        workspace.group.resize(d4c_fft / 2 + 1, 0.0);
        for k in 0..workspace.group.len() {
            workspace.group[k] = workspace.centroid[k] / workspace.smoothed[k].max(1e-12);
        }
        workspace.smooth_group.resize(workspace.group.len(), 0.0);
        crate::helpers::linear_smoothing_with_workspace(
            &workspace.group,
            current_f0 / 2.0,
            sample_rate,
            d4c_fft,
            &mut workspace.smooth_group,
            &mut smoothing_workspace,
        );
        workspace.group2.resize(workspace.group.len(), 0.0);
        crate::helpers::linear_smoothing_with_workspace(
            &workspace.smooth_group,
            current_f0,
            sample_rate,
            d4c_fft,
            &mut workspace.group2,
            &mut smoothing_workspace,
        );
        for k in 0..workspace.group.len() {
            workspace.group[k] = workspace.smooth_group[k] - workspace.group2[k];
        }
        workspace.coarse.resize(coarse_count + 2, -60.0);
        workspace.coarse.fill(-60.0);
        workspace.segment.resize(d4c_fft, 0.0);
        workspace.fft_b.resize(d4c_fft, Complex::default());
        workspace.spectrum.resize(d4c_fft / 2 + 1, 0.0);
        workspace.sorted_spectrum.resize(d4c_fft / 2 + 1, 0.0);
        for c in 0..coarse_count {
            let center = (3000.0 * (c + 1) as f64 * d4c_fft as f64 / sample_rate as f64) as usize;
            let half = window_len / 2;
            for j in 0..window_len {
                workspace.segment[j] = workspace.group[(center + j)
                    .saturating_sub(half)
                    .min(workspace.group.len() - 1)]
                    * window[j];
            }
            workspace.segment[window_len..].fill(0.0);
            power_spectrum(
                &workspace.segment,
                &mut workspace.fft_b,
                &mut workspace.spectrum,
            );
            workspace
                .sorted_spectrum
                .copy_from_slice(&workspace.spectrum);
            workspace
                .sorted_spectrum
                .sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            for j in 1..workspace.sorted_spectrum.len() {
                workspace.sorted_spectrum[j] += workspace.sorted_spectrum[j - 1];
            }
            let boundary = (d4c_fft as f64 * 8.0 / window_len as f64).round() as usize;
            workspace.coarse[c + 1] = 10.0
                * (workspace.sorted_spectrum[d4c_fft / 2 - boundary - 1]
                    / workspace.sorted_spectrum[d4c_fft / 2].max(1e-30))
                .log10();
            workspace.coarse[c + 1] =
                workspace.coarse[c + 1].min(0.0) + (current_f0 - 100.0) / 50.0;
            workspace.coarse[c + 1] = workspace.coarse[c + 1].min(0.0);
        }
        workspace.coarse[coarse_count + 1] = -1e-12;
        for k in 0..bins {
            let hz = k as f64 * sample_rate as f64 / fft_size as f64;
            // The final WORLD coarse interval runs from the last 3-kHz band
            // to Nyquist, not another fixed 3-kHz interval.
            let last_edge = 3000.0 * coarse_count as f64;
            let (index, t) = if hz < last_edge {
                let p = hz / 3000.0;
                let index = p.floor() as usize;
                (index, p - index as f64)
            } else {
                let width = (sample_rate as f64 / 2.0 - last_edge).max(1.0);
                (coarse_count, ((hz - last_edge) / width).clamp(0.0, 1.0))
            };
            let value = workspace.coarse[index] * (1.0 - t) + workspace.coarse[index + 1] * t;
            output[frame * bins + k] = 10f64.powf(value / 20.0);
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unvoiced_frames_are_fully_aperiodic() {
        let ap = analyze(
            &[0.0; 100],
            44100,
            &[0.0],
            &[0.0],
            64,
            D4cOption::default(),
            &(),
        )
        .unwrap();
        assert_eq!(ap, vec![1.0; 33]);
    }

    #[test]
    fn coarse_band_tail_fill_preserves_fft_input() {
        let n = 256usize;
        let window_len = 61usize;
        let group: Vec<f64> = (0..129)
            .map(|i| ((i * 37 % 101) as f64 - 50.0) / 17.0)
            .collect();
        let window: Vec<f64> = (0..window_len)
            .map(|i| 0.355768 - 0.487396 * (i as f64 * 0.17).cos())
            .collect();
        let center = 70usize;
        let half = window_len / 2;

        let mut baseline = vec![0.0; n];
        for j in 0..window_len {
            baseline[j] = group[(center + j).saturating_sub(half).min(group.len() - 1)] * window[j];
        }
        let mut candidate = vec![f64::NAN; n];
        for j in 0..window_len {
            candidate[j] =
                group[(center + j).saturating_sub(half).min(group.len() - 1)] * window[j];
        }
        candidate[window_len..].fill(0.0);
        assert_eq!(
            candidate, baseline,
            "tail-fill must match a fresh zeroed buffer"
        );
    }

    #[test]
    fn cancellation_is_cooperative() {
        struct Stop;
        impl Cancellation for Stop {
            fn is_cancelled(&self) -> bool {
                true
            }
        }
        assert_eq!(
            analyze(
                &[0.0; 100],
                44100,
                &[0.0],
                &[220.0],
                64,
                D4cOption::default(),
                &Stop
            ),
            Err(D4cError::Cancelled)
        );
    }
}
