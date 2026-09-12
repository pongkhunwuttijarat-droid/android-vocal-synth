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
    fn default() -> Self { Self { threshold: 0.0, floor_f0: 47.0 } }
}

#[derive(Debug, Clone, PartialEq)]
pub enum D4cError { InvalidInput, Cancelled }

fn next_randn(s: &mut [u32; 4]) -> f64 {
    let t = s[0] ^ (s[0] << 11); s[0] = s[1]; s[1] = s[2]; s[2] = s[3]; s[3] = (s[3] ^ (s[3] >> 19)) ^ (t ^ (t >> 8));
    let mut sum = s[3] >> 4;
    for _ in 0..11 { let t = s[0] ^ (s[0] << 11); s[0] = s[1]; s[1] = s[2]; s[2] = s[3]; s[3] = (s[3] ^ (s[3] >> 19)) ^ (t ^ (t >> 8)); sum = sum.wrapping_add(s[3] >> 4); }
    sum as f64 / 268435456.0 - 6.0
}

fn nuttall_window(n: usize) -> Vec<f64> {
    (0..n).map(|i| { let p = 2.0 * std::f64::consts::PI * i as f64 / (n - 1).max(1) as f64; 0.355768 - 0.487396 * p.cos() + 0.144232 * (2.0 * p).cos() - 0.012604 * (3.0 * p).cos() }).collect()
}

fn windowed(samples: &[f32], fs: u32, f0: f64, pos: f64, ratio: f64, blackman: bool, rng: &mut [u32; 4]) -> (Vec<f64>, usize) {
    let half = crate::helpers::matlab_round(ratio * fs as f64 / f0 / 2.0).max(1) as usize;
    let len = half * 2 + 1; let origin = crate::helpers::matlab_round(pos * fs as f64 + 0.001) as i64;
    let mut y = vec![0.0; len]; let mut sum = 0.0; let mut weight_sum = 0.0;
    for j in 0..len { let rel = j as f64 - half as f64; let idx = (origin + rel as i64).clamp(0, samples.len().saturating_sub(1) as i64) as usize; let a = std::f64::consts::PI * (2.0 * rel / ratio / fs as f64) * f0; let w = if blackman { 0.42 + 0.5 * a.cos() + 0.08 * (2.0 * a).cos() } else { 0.5 + 0.5 * a.cos() }; y[j] = samples[idx] as f64 * w + next_randn(rng) * 1e-6; sum += y[j]; weight_sum += w; }
    let mean = sum / weight_sum.max(1e-12); for j in 0..len { let rel = j as f64 - half as f64; let a = std::f64::consts::PI * (2.0 * rel / ratio / fs as f64) * f0; let w = if blackman { 0.42 + 0.5 * a.cos() + 0.08 * (2.0 * a).cos() } else { 0.5 + 0.5 * a.cos() }; y[j] -= w * mean; }
    (y, len)
}

fn power_spectrum(input: &[f64]) -> Vec<f64> {
    let n = input.len(); let fft = crate::fft::forward_plan(n); let mut z: Vec<Complex<f64>> = input.iter().map(|&x| Complex { re: x, im: 0.0 }).collect(); fft.process(&mut z); z[..=n / 2].iter().map(|v| v.norm_sqr()).collect()
}

fn static_centroid(samples: &[f32], fs: u32, f0: f64, pos: f64, n: usize, rng: &mut [u32; 4]) -> Vec<f64> {
    let (a, _) = windowed(samples, fs, f0, pos - 0.25 / f0, 4.0, true, rng); let (b, _) = windowed(samples, fs, f0, pos + 0.25 / f0, 4.0, true, rng); let mut out = vec![0.0; n / 2 + 1];
    for (x, scale) in [a, b].iter().zip([1.0, 1.0]) { let mut z: Vec<Complex<f64>> = vec![Complex::default(); n]; let energy = x.iter().map(|v| v * v).sum::<f64>().sqrt(); for (i, &v) in x.iter().enumerate() { z[i].re = v / energy.max(1e-12); } let f = crate::fft::forward_plan(n); f.process(&mut z); let mut wz = vec![Complex::default(); n]; for (i, &v) in x.iter().enumerate() { wz[i].re = v * (i as f64 + 1.0) / energy.max(1e-12); } f.process(&mut wz); for k in 0..=n / 2 { out[k] += scale * (wz[k].re * z[k].re + wz[k].im * z[k].im); } }
    // WORLD applies DCCorrection to the summed centroid before the group
    // delay is formed. Omitting this low-frequency mirror materially changes
    // D4C around voiced fundamentals.
    crate::helpers::dc_correction(&mut out, f0, fs, n);
    out
}

fn smoothed_power(samples: &[f32], fs: u32, f0: f64, pos: f64, n: usize, rng: &mut [u32; 4]) -> Vec<f64> {
    let (x, _) = windowed(samples, fs, f0, pos, 4.0, false, rng); let mut z = vec![0.0; n]; z[..x.len()].copy_from_slice(&x); let mut p = power_spectrum(&z); crate::helpers::dc_correction(&mut p, f0, fs, n); let mut out = vec![0.0; p.len()]; crate::helpers::linear_smoothing(&p, f0, fs, n, &mut out); out
}

fn love_train(samples: &[f32], fs: u32, f0: f64, pos: f64, n: usize, rng: &mut [u32; 4]) -> f64 {
    let (x, len) = windowed(samples, fs, f0, pos, 3.0, true, rng); let mut z = vec![0.0; n]; z[..len.min(n)].copy_from_slice(&x[..len.min(n)]); let p = power_spectrum(&z); let b0 = (100.0 * n as f64 / fs as f64).ceil() as usize; let b1 = (4000.0 * n as f64 / fs as f64).ceil() as usize; let b2 = (7900.0 * n as f64 / fs as f64).ceil() as usize; let mut cumulative = vec![0.0; p.len()]; for i in (b0 + 1)..=b2.min(p.len() - 1) { cumulative[i] = cumulative[i - 1] + p[i]; } (cumulative[b1.min(cumulative.len() - 1)] / cumulative[b2.min(cumulative.len() - 1)].max(1e-30)).clamp(0.0, 1.0)
}

/// Differential-port hook: return the four D4C intermediate stages for one
/// frame. Kept public so the native oracle harness can compare stages without
/// changing the production `analyze` contract.
#[doc(hidden)]
pub fn probe_stages(samples: &[f32], fs: u32, f0: f64, pos: f64, fft_size: usize)
    -> (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>) {
    let d4c_fft = (2.0f64.powf((1.0 + (4.0 * fs as f64 / 47.0 + 1.0).log2()).floor())) as usize;
    let current_f0 = f0.max(47.0);
    let mut rng = [123456789,362436069,521288629,88675123];
    let (a, _) = windowed(samples, fs, current_f0, pos - 0.25 / current_f0, 4.0, true, &mut rng);
    let (b, _) = windowed(samples, fs, current_f0, pos + 0.25 / current_f0, 4.0, true, &mut rng);
    let mut centroid = vec![0.0; d4c_fft / 2 + 1];
    for x in [&a, &b] {
        let energy = x.iter().map(|v| v * v).sum::<f64>().sqrt();
        let mut z = vec![Complex::default(); d4c_fft];
        let mut wz = vec![Complex::default(); d4c_fft];
        for (i, &v) in x.iter().enumerate() { z[i].re = v / energy.max(1e-12); wz[i].re = z[i].re * (i as f64 + 1.0); }
        let f = crate::fft::forward_plan(d4c_fft); f.process(&mut z); f.process(&mut wz);
        for k in 0..=d4c_fft/2 { centroid[k] += wz[k].re*z[k].re + wz[k].im*z[k].im; }
    }
    crate::helpers::dc_correction(&mut centroid, current_f0, fs, d4c_fft);
    let power = smoothed_power(samples, fs, current_f0, pos, d4c_fft, &mut rng);
    let mut group = centroid.iter().zip(&power).map(|(&a,&b)| a/b.max(1e-12)).collect::<Vec<_>>();
    let mut s1=vec![0.0;group.len()]; crate::helpers::linear_smoothing(&group,current_f0/2.0,fs,d4c_fft,&mut s1);
    let mut s2=vec![0.0;group.len()]; crate::helpers::linear_smoothing(&s1,current_f0,fs,d4c_fft,&mut s2);
    for k in 0..group.len(){group[k]=s1[k]-s2[k];}
    let bands=((15000.0f64).min(fs as f64/2.0-3000.0)/3000.0) as usize;
    let wl=((3000.0*d4c_fft as f64/fs as f64) as usize)*2+1; let win=nuttall_window(wl);
    let mut coarse=vec![0.0;bands+2]; let boundary=crate::helpers::matlab_round(d4c_fft as f64*8.0/wl as f64).max(0) as usize;
    let f=crate::fft::forward_plan(d4c_fft);
    for c in 0..bands {let center=(3000*(c+1)*d4c_fft/(fs as usize));let half=wl/2;let mut z=vec![Complex::default();d4c_fft];for j in 0..wl{z[j].re=group[center-half+j]*win[j];}f.process(&mut z);let mut p=z[..=d4c_fft/2].iter().map(|v|v.norm_sqr()).collect::<Vec<_>>();p.sort_by(|a,b|a.partial_cmp(b).unwrap());for j in 1..p.len(){p[j]+=p[j-1];}coarse[c+1]=10.0*(p[d4c_fft/2-boundary-1]/p[d4c_fft/2].max(1e-30)).log10();coarse[c+1]=(coarse[c+1]+(current_f0-100.0)/50.0).min(0.0);}
    coarse[0]=-60.0; coarse[bands+1]=-1e-12;
    (centroid,power,group,coarse)
}

/// Produce a row-major aperiodicity spectrogram from PCM and an F0 contour.
pub fn analyze<C: Cancellation>(
    samples: &[f32], sample_rate: u32, time: &[f64], f0: &[f64], fft_size: usize,
    option: D4cOption, cancel: &C,
) -> Result<Vec<f64>, D4cError> {
    if sample_rate == 0 || fft_size < 4 || !fft_size.is_power_of_two() || time.len() != f0.len() { return Err(D4cError::InvalidInput); }
    let bins = fft_size / 2 + 1;
    let mut output = vec![1.0; f0.len() * bins];
    let d4c_fft = (2.0f64.powf((1.0 + (4.0 * sample_rate as f64 / 47.0 + 1.0).log2()).floor())) as usize;
    let coarse_count = ((15000.0f64).min(sample_rate as f64 / 2.0 - 3000.0) / 3000.0) as usize;
    let window_len = ((3000.0 * d4c_fft as f64 / sample_rate as f64) as usize) * 2 + 1;
    let window = nuttall_window(window_len);
    let mut rng = [123456789u32, 362436069, 521288629, 88675123];
    for (frame, (&position, &pitch)) in time.iter().zip(f0).enumerate() {
        if cancel.is_cancelled() { return Err(D4cError::Cancelled); }
        // WORLD's D4CLoveTrain/D4C split only on f0 == 0. Low non-zero F0
        // values are still processed, with the general body clamped to the
        // D4C floor internally.
        if pitch <= 0.0 { continue; }
        let mut ap0 = 0.0;
        if pitch > 0.0 {
            ap0 = love_train(samples, sample_rate, pitch.max(47.0), position, d4c_fft, &mut rng);
        }
        if pitch <= 0.0 || ap0 <= option.threshold { continue; }
        let current_f0 = pitch.max(47.0);
        let static_centroid = static_centroid(samples, sample_rate, current_f0, position, d4c_fft, &mut rng);
        let smoothed = smoothed_power(samples, sample_rate, current_f0, position, d4c_fft, &mut rng);
        let mut group = vec![0.0; d4c_fft / 2 + 1];
        for k in 0..group.len() { group[k] = static_centroid[k] / smoothed[k].max(1e-12); }
        let mut smooth_group = vec![0.0; group.len()];
        crate::helpers::linear_smoothing(&group, current_f0 / 2.0, sample_rate, d4c_fft, &mut smooth_group);
        let mut group2 = vec![0.0; group.len()];
        crate::helpers::linear_smoothing(&smooth_group, current_f0, sample_rate, d4c_fft, &mut group2);
        for k in 0..group.len() { group[k] = smooth_group[k] - group2[k]; }
        let mut coarse = vec![-60.0; coarse_count + 2];
        for c in 0..coarse_count {
            let center = (3000.0 * (c + 1) as f64 * d4c_fft as f64 / sample_rate as f64) as usize;
            let half = window_len / 2;
            let mut segment = vec![0.0; d4c_fft];
            for j in 0..window_len { segment[j] = group[(center + j).saturating_sub(half).min(group.len() - 1)] * window[j]; }
            let powers = power_spectrum(&segment);
            let mut sorted = powers;
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            for j in 1..sorted.len() { sorted[j] += sorted[j - 1]; }
            let boundary = (d4c_fft as f64 * 8.0 / window_len as f64).round() as usize;
            coarse[c + 1] = 10.0 * (sorted[d4c_fft / 2 - boundary - 1] / sorted[d4c_fft / 2].max(1e-30)).log10();
            coarse[c + 1] = coarse[c + 1].min(0.0) + (current_f0 - 100.0) / 50.0;
            coarse[c + 1] = coarse[c + 1].min(0.0);
        }
        coarse[coarse_count + 1] = -1e-12;
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
            let value = coarse[index] * (1.0 - t) + coarse[index + 1] * t;
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
        let ap = analyze(&[0.0; 100], 44100, &[0.0], &[0.0], 64, D4cOption::default(), &()).unwrap();
        assert_eq!(ap, vec![1.0; 33]);
    }

    #[test]
    fn cancellation_is_cooperative() {
        struct Stop;
        impl Cancellation for Stop { fn is_cancelled(&self) -> bool { true } }
        assert_eq!(analyze(&[0.0; 100], 44100, &[0.0], &[220.0], 64, D4cOption::default(), &Stop), Err(D4cError::Cancelled));
    }
}
