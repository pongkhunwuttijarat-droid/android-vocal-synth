//! Rust-only analysis front-end for WORLD features.
//!
//! This is the first executable replacement for the C++ analysis boundary. It
//! deliberately keeps the output layout identical to [`WorldFeatures`]. The
//! F0 estimator is an autocorrelation estimator and the spectral envelope is a
//! windowed magnitude spectrum; these are a deterministic bring-up kernel, not
//! the final DIO/CheapTrick/D4C parity implementation yet.

use crate::fft::RealFft;
use crate::{Cancellation, FeatureError, WorldFeatures};

#[derive(Debug, Clone, Copy)]
pub struct AnalysisConfig {
    pub sample_rate: u32,
    pub frame_period_ms: f64,
    pub fft_size: usize,
    pub min_f0_hz: f64,
    pub max_f0_hz: f64,
}

/// Run the complete Rust analysis chain over a resolved voicebank sample.
/// FRQ remains available on the sample for the final FrqEstimator path.
pub fn analyze_world_sample<C: Cancellation>(
    sample: &voicebank::VoicebankSample,
    config: AnalysisConfig,
    cancel: &C,
) -> Result<WorldFeatures, AnalysisError> {
    analyze_world_audio(&sample.wav.to_mono(), config, cancel)
}

/// Run the full Rust WORLD analysis chain over decoded mono audio.
pub fn analyze_world_audio<C: Cancellation>(
    mono: &[f32],
    config: AnalysisConfig,
    cancel: &C,
) -> Result<WorldFeatures, AnalysisError> {
    analyze_world_audio_with_f0(mono, config, None, cancel)
}

/// Analyze spectral/aperiodicity features using a caller-provided F0 grid.
/// This is the Rust equivalent of WORLDLINE's `FrqEstimator` path: shipped
/// voicebanks with `<wav>_wav.frq` must not be re-estimated by DIO.
pub fn analyze_world_audio_with_f0<C: Cancellation>(
    mono: &[f32],
    config: AnalysisConfig,
    supplied_f0: Option<&[f64]>,
    cancel: &C,
) -> Result<WorldFeatures, AnalysisError> {
    let dio_option = crate::f0::DioOption {
        frame_period_ms: config.frame_period_ms,
        f0_floor: config.min_f0_hz,
        f0_ceil: config.max_f0_hz,
        ..Default::default()
    };
    // FrqEstimator is authoritative for voicebank samples.  In particular,
    // C++ Model::Trim can leave a sample buffer whose length would make a
    // fresh DIO call report one extra terminal frame, while the already
    // trimmed F0 vector has the requested model length.  Build the time axis
    // from the supplied grid in that case instead of rejecting the request.
    let (mut f0, time) = if let Some(values) = supplied_f0 {
        if values.is_empty() {
            return Err(AnalysisError::InvalidConfig);
        }
        let time = (0..values.len())
            .map(|i| i as f64 * config.frame_period_ms / 1000.0)
            .collect::<Vec<_>>();
        (values.to_vec(), time)
    } else {
        crate::f0::dio(mono, config.sample_rate, dio_option, cancel)
            .map_err(|error| match error {
                crate::f0::F0Error::Cancelled => AnalysisError::Cancelled,
                crate::f0::F0Error::InvalidInput => AnalysisError::InvalidConfig,
            })?
    };
    if supplied_f0.is_none() {
        crate::f0::stone_mask(mono, config.sample_rate, &time, &mut f0);
    }
    let option = crate::cheaptrick::CheapTrickOption {
        q1: -0.15,
        f0_floor: config.min_f0_hz,
        fft_size: config.fft_size,
    };
    let spectral = crate::cheaptrick::analyze(mono, config.sample_rate, &time, &f0, option)
        .map_err(|_| AnalysisError::InvalidConfig)?;
    let aperiodicity = crate::d4c::analyze(
        mono,
        config.sample_rate,
        &time,
        &f0,
        config.fft_size,
        Default::default(),
        cancel,
    )
    .map_err(|error| match error {
        crate::d4c::D4cError::Cancelled => AnalysisError::Cancelled,
        crate::d4c::D4cError::InvalidInput => AnalysisError::InvalidConfig,
    })?;
    Ok(WorldFeatures {
        frame_count: f0.len(),
        bin_count: config.fft_size / 2 + 1,
        f0,
        spectral,
        aperiodicity,
    })
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        // WORLDLINE v2 (the official `libworldline.so` used by the fixture)
        // advances WORLD frames by one 512-sample hop at 44.1 kHz.
        Self { sample_rate: 44100, frame_period_ms: 512.0 * 1000.0 / 44100.0, fft_size: 1024, min_f0_hz: 50.0, max_f0_hz: 1100.0 }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AnalysisError {
    InvalidConfig,
    Feature(FeatureError),
    Cancelled,
}

impl From<FeatureError> for AnalysisError {
    fn from(value: FeatureError) -> Self { Self::Feature(value) }
}

/// Analyze mono PCM into contiguous Rust-owned feature rows.
pub fn analyze<C: Cancellation>(samples: &[f32], config: AnalysisConfig, cancel: &C) -> Result<WorldFeatures, AnalysisError> {
    if config.sample_rate == 0 || config.fft_size < 4 || !config.fft_size.is_power_of_two()
        || config.frame_period_ms <= 0.0 || config.min_f0_hz <= 0.0
        || config.max_f0_hz <= config.min_f0_hz
    { return Err(AnalysisError::InvalidConfig); }
    let hop = ((config.sample_rate as f64 * config.frame_period_ms / 1000.0).round() as usize).max(1);
    // WORLD's GetSamplesForDIO: floor(1000 * length / fs / frame_ms) + 1.
    // Keep the terminal boundary frame even when the sample count is an
    // exact multiple of the hop.
    let frames = if samples.is_empty() { 0 } else { (samples.len() / hop) + 1 };
    let bins = config.fft_size / 2 + 1;
    let mut out = WorldFeatures::try_new(frames, bins)?;
    let mut fft = RealFft::new(config.fft_size).map_err(|_| AnalysisError::InvalidConfig)?;
    let mut frame = vec![0.0f64; config.fft_size];
    let mut packed = vec![0.0f64; config.fft_size];
    for frame_index in 0..frames {
        if cancel.is_cancelled() { return Err(AnalysisError::Cancelled); }
        frame.fill(0.0);
        let center = frame_index * hop;
        let start = center.saturating_sub(config.fft_size / 2);
        let end = (start + config.fft_size).min(samples.len());
        for (i, &sample) in samples[start..end].iter().enumerate() {
            let phase = (i as f64 + 0.5) / config.fft_size as f64;
            frame[i] = sample as f64 * (std::f64::consts::PI * phase).sin().powi(2);
        }
        out.f0[frame_index] = estimate_f0(&frame, config.sample_rate as f64, config.min_f0_hz, config.max_f0_hz);
        fft.forward_packed(&frame, &mut packed).map_err(|_| AnalysisError::InvalidConfig)?;
        let row = frame_index * bins;
        for bin in 0..bins {
            let re = if bin == 0 { packed[0] } else if bin == config.fft_size / 2 { packed[1] } else { packed[2 * bin] };
            let im = if bin == 0 || bin == config.fft_size / 2 { 0.0 } else { packed[2 * bin + 1] };
            let magnitude = (re * re + im * im).sqrt() / config.fft_size as f64;
            out.spectral[row + bin] = magnitude.max(1e-12);
            // Bring-up aperiodicity: low energy is treated as noise-like.
            out.aperiodicity[row + bin] = (1.0f64 - magnitude / (magnitude + 1.0f64)).clamp(0.0, 1.0);
        }
    }
    Ok(out)
}

/// Analyze a resolved voicebank asset without duplicating WAV/FRQ handling in
/// the engine adapter. FRQ is retained by the sample contract for the final
/// DIO/StoneMask-compatible estimator; the bring-up estimator uses the decoded
/// waveform first.
pub fn analyze_voicebank_sample<C: Cancellation>(
    sample: &voicebank::VoicebankSample,
    config: AnalysisConfig,
    cancel: &C,
) -> Result<WorldFeatures, AnalysisError> {
    let mono = sample.wav.to_mono();
    analyze(&mono, config, cancel)
}

fn estimate_f0(frame: &[f64], sample_rate: f64, min_f0: f64, max_f0: f64) -> f64 {
    let min_lag = (sample_rate / max_f0).floor().max(1.0) as usize;
    let max_lag = (sample_rate / min_f0).ceil() as usize;
    let energy = frame.iter().map(|x| x * x).sum::<f64>();
    if energy <= 1e-12 || min_lag >= frame.len() { return 0.0; }
    let max_lag = max_lag.min(frame.len() - 1);
    let mut best = 0.0;
    let mut best_lag = 0;
    for lag in min_lag..=max_lag {
        let corr = frame[..frame.len() - lag].iter().zip(&frame[lag..]).map(|(a, b)| a * b).sum::<f64>();
        if corr > best { best = corr; best_lag = lag; }
    }
    if best_lag == 0 || best / energy < 0.2 { 0.0 } else { sample_rate / best_lag as f64 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyzes_sine_with_contiguous_rows() {
        let samples: Vec<f32> = (0..4410).map(|i| (2.0 * std::f64::consts::PI * 220.0 * i as f64 / 44100.0).sin() as f32).collect();
        let features = analyze(&samples, AnalysisConfig::default(), &()).unwrap();
        assert!(features.frame_count > 0);
        assert_eq!(features.bin_count, 513);
        assert!(features.f0.iter().any(|f| (*f - 220.0).abs() < 5.0));
        features.validate().unwrap();
    }

    #[test]
    fn cancellation_stops_before_next_frame() {
        struct Stop;
        impl Cancellation for Stop { fn is_cancelled(&self) -> bool { true } }
        let err = analyze(&[0.1; 1000], AnalysisConfig::default(), &Stop).unwrap_err();
        assert_eq!(err, AnalysisError::Cancelled);
    }

    #[test]
    fn supplied_f0_grid_can_omit_dio_terminal_frame() {
        let config = AnalysisConfig { frame_period_ms: 10.0, fft_size: 2048, ..Default::default() };
        // C++ Model::Trim can produce exactly N*hop samples but retain an
        // F0 vector of N rows. A fresh DIO count would report N+1.
        let samples = vec![0.0f32; 205 * 441];
        let supplied = vec![220.0; 205];
        let features = analyze_world_audio_with_f0(&samples, config, Some(&supplied), &()).unwrap();
        assert_eq!(features.frame_count, supplied.len());
    }
}
