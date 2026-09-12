//! Port of `worldline/model/model.cpp` — one analyzed voicebank sample:
//! F0 estimation, CheapTrick envelope, D4C aperiodicity, trimming and
//! remapping onto the request's frame grid.
//!
//! The numerical body is the pure-Rust `world-rs` WORLD kernel, so this
//! type is the equivalent of the C++ `Model` without a dlopen'd `.so`.

use world_rs::cheaptrick::CheapTrickOption;
use world_rs::WorldFeatures;

use crate::f0::F0Estimator;

/// WORLD `kFloorF0StoneMask` (voiced-frame threshold used by the ratio).
pub const FLOOR_F0_STONE_MASK: f64 = 40.0;
/// WORLD `kMySafeGuardMinimum` (spectral floor).
pub const SAFE_GUARD_MINIMUM: f64 = 1e-12;

/// One analyzed model (a trimmed, remapped sample).
pub struct Model {
    samples: Vec<f64>,
    fs: i32,
    frame_ms: f64,
    estimator: F0Estimator,
    f0: Vec<f64>,
    ts: Vec<f64>,
    fft_size: usize,
    /// Spectral envelope, row-major `f0.len() × bins`.
    sp: Vec<f64>,
    /// Aperiodicity, row-major `f0.len() × bins`.
    ap: Vec<f64>,
}

impl Model {
    /// Analysis model over a decoded sample.
    pub fn new(samples: Vec<f64>, fs: i32, frame_ms: f64, estimator: F0Estimator) -> Self {
        Self {
            samples,
            fs,
            frame_ms,
            estimator,
            f0: Vec::new(),
            ts: Vec::new(),
            fft_size: 0,
            sp: Vec::new(),
            ap: Vec::new(),
        }
    }

    /// Empty synthesis-only model (`Model(int fs, double frame_ms, int fft_size)`).
    pub fn new_synth(fs: i32, frame_ms: f64, fft_size: usize) -> Self {
        Self {
            samples: Vec::new(),
            fs,
            frame_ms,
            estimator: F0Estimator::None,
            f0: Vec::new(),
            ts: Vec::new(),
            fft_size,
            sp: Vec::new(),
            ap: Vec::new(),
        }
    }

    pub fn samples(&self) -> &[f64] {
        &self.samples
    }
    pub fn samples_mut(&mut self) -> &mut Vec<f64> {
        &mut self.samples
    }
    pub fn fs(&self) -> i32 {
        self.fs
    }
    pub fn frame_ms(&self) -> f64 {
        self.frame_ms
    }
    pub fn fft_size(&self) -> usize {
        self.fft_size
    }
    pub fn f0(&self) -> &[f64] {
        &self.f0
    }
    pub fn f0_mut(&mut self) -> &mut Vec<f64> {
        &mut self.f0
    }
    pub fn ts(&self) -> &[f64] {
        &self.ts
    }
    pub fn sp_row(&self, index: usize, bins: usize) -> &[f64] {
        &self.sp[index * bins..(index + 1) * bins]
    }
    pub fn ap_row(&self, index: usize, bins: usize) -> &[f64] {
        &self.ap[index * bins..(index + 1) * bins]
    }
    pub fn sp(&self) -> &[f64] {
        &self.sp
    }
    pub fn ap(&self) -> &[f64] {
        &self.ap
    }
    pub fn set_analysis(&mut self, f0: Vec<f64>, sp: Vec<f64>, ap: Vec<f64>, fft_size: usize) {
        self.f0 = f0;
        self.sp = sp;
        self.ap = ap;
        self.fft_size = fft_size;
    }
    pub fn bins(&self) -> usize {
        self.fft_size / 2 + 1
    }

    /// Total duration of the sample in ms.
    pub fn total_ms(&self) -> f64 {
        self.samples.len() as f64 * 1000.0 / f64::from(self.fs)
    }

    /// `Model::BuildF0` — estimate f0 + its time axis.
    pub fn build_f0(&mut self) {
        let (f0, ts) = self
            .estimator
            .estimate(&self.samples, self.fs, self.frame_ms);
        self.f0 = f0;
        self.ts = ts;
    }

    /// `Model::BuildSp` — CheapTrick with the WORLD default option for `fs`.
    pub fn build_sp(&mut self) -> Result<(), String> {
        let option = CheapTrickOption::new(self.fs as u32);
        self.fft_size = option.fft_size;
        let samples: Vec<f32> = self.samples.iter().map(|s| *s as f32).collect();
        let sp = world_rs::cheaptrick::analyze(&samples, self.fs as u32, &self.ts, &self.f0, option)
            .map_err(|e| format!("cheaptrick: {e:?}"))?;
        self.sp = sp;
        Ok(())
    }

    /// `Model::BuildAp` — D4C with `threshold = 0`.
    pub fn build_ap(&mut self) -> Result<(), String> {
        let option = world_rs::d4c::D4cOption {
            threshold: 0.0,
            ..Default::default()
        };
        let samples: Vec<f32> = self.samples.iter().map(|s| *s as f32).collect();
        let ap = world_rs::d4c::analyze(
            &samples,
            self.fs as u32,
            &self.ts,
            &self.f0,
            self.fft_size,
            option,
            &(),
        )
        .map_err(|e| format!("d4c: {e:?}"))?;
        self.ap = ap;
        Ok(())
    }

    /// `Model::Trim(start, length)` — cut the sample (and every analysis
    /// grid) to the requested frame window.
    pub fn trim(&mut self, start: usize, length: usize) {
        let start_samples = (self.frame_ms * start as f64 * f64::from(self.fs) / 1000.0) as usize;
        let length_samples = (self.frame_ms * length as f64 * f64::from(self.fs) / 1000.0) as usize;
        let end_samples = (start_samples + length_samples).min(self.samples.len());
        self.samples = self.samples[start_samples.min(self.samples.len())..end_samples].to_vec();
        let start = start.min(self.f0.len());
        let end = (start + length).min(self.f0.len());
        self.f0 = self.f0[start..end].to_vec();
        self.ts = self.ts[start.min(self.ts.len())..end.min(self.ts.len())].to_vec();
        let bins = self.bins().max(1);
        if !self.sp.is_empty() {
            self.sp = self.sp[start * bins..end * bins].to_vec();
        }
        if !self.ap.is_empty() {
            self.ap = self.ap[start * bins..end * bins].to_vec();
        }
        if let Some(t0) = self.ts.first().copied() {
            for value in self.ts.iter_mut() {
                *value -= t0;
            }
        }
    }

    /// `Model::Remap(mapping)` — resample f0/sp/ap onto new frame positions
    /// (positions in ms).
    pub fn remap(&mut self, mapping: &[f64]) {
        let bins = self.bins().max(1);
        let frames = self.f0.len();
        if frames == 0 {
            return;
        }
        let mut new_f0 = Vec::with_capacity(mapping.len());
        let mut new_sp = Vec::with_capacity(mapping.len() * bins);
        let mut new_ap = Vec::with_capacity(mapping.len() * bins);
        for p in mapping {
            let pos = p / self.frame_ms;
            let idx = pos.floor().max(0.0) as usize;
            let t = pos - idx as f64;
            let i0 = idx.min(frames - 1);
            let i1 = (idx + 1).min(frames - 1);
            let t = if i0 == i1 { 0.0 } else { t };
            new_f0.push(self.f0[i0] * (1.0 - t) + self.f0[i1] * t);
            for k in 0..bins {
                let a = self.sp[i0 * bins + k];
                let b = self.sp[i1 * bins + k];
                new_sp.push(a * (1.0 - t) + b * t);
                let c = self.ap[i0 * bins + k];
                let d = self.ap[i1 * bins + k];
                new_ap.push(c * (1.0 - t) + d * t);
            }
        }
        self.f0 = new_f0;
        self.sp = new_sp;
        self.ap = new_ap;
    }

    /// Shift the gender (frequency axis) of EVERY spectral row — the
    /// `ShiftGender(std::vector<std::vector<double>>&, int)` overload the
    /// classic resampler calls with `flag_g`.
    pub fn shift_gender_all(&mut self, value: i32) {
        if value == 0 {
            return;
        }
        let bins = self.bins().max(1);
        for frame in 0..self.f0.len() {
            crate::effects::shift_gender_row(&mut self.sp[frame * bins..(frame + 1) * bins], value);
        }
    }

    /// `Model::GetVoicedRatio`.
    pub fn get_voiced_ratio(&self) -> f64 {
        if self.f0.is_empty() {
            return 0.0;
        }
        let voiced = self
            .f0
            .iter()
            .filter(|f| **f > FLOOR_F0_STONE_MASK)
            .count();
        voiced as f64 / self.f0.len() as f64
    }

    /// `Model::MsToSamples`.
    pub fn ms_to_samples(&self, ms: f64) -> i32 {
        (ms * f64::from(self.fs) / 1000.0) as i32
    }

    /// `Model::Synth` — WORLD resynthesis from the stored features and the
    /// per-frame expression curves.
    pub fn synth(
        &mut self,
        tension: &[Vec<f64>],
        breathiness: &[f64],
        voicing: &[f64],
    ) -> Result<(), String> {
        let frames = self.f0.len();
        let bins = self.bins().max(1);
        if frames == 0 {
            self.samples = Vec::new();
            return Ok(());
        }
        let y_len = (f64::from(self.fs) * (frames as f64 - 1.0) * self.frame_ms / 1000.0) as usize + 1;
        let mut features = WorldFeatures::try_new(frames, bins).map_err(|e| format!("{e:?}"))?;
        features.f0.copy_from_slice(&self.f0);
        features.spectral.copy_from_slice(&self.sp[..frames * bins]);
        features.aperiodicity.copy_from_slice(&self.ap[..frames * bins]);
        // Per-bin tension (the C++ `tension` array), flattened frame-major.
        let mut tension_bins = Vec::with_capacity(frames * bins);
        for frame in 0..frames {
            match tension.get(frame) {
                Some(row) if row.len() >= bins => tension_bins.extend_from_slice(&row[..bins]),
                _ => tension_bins.extend(std::iter::repeat(1.0).take(bins)),
            }
        }
        let curves = world_rs::synthesis::ExpressionCurves {
            tension: Vec::new(),
            tension_bins: Some(tension_bins),
            breathiness: breathiness.to_vec(),
            voicing: voicing.to_vec(),
        };
        let config = world_rs::synthesis::SynthesisConfig {
            sample_rate: self.fs as u32,
            frame_period_ms: self.frame_ms,
            fft_size: self.fft_size,
            output_length: Some(y_len),
            ..Default::default()
        };
        if std::env::var_os("WORLD_RS_DBG").is_some() {
            eprintln!(
                "[rs-synth] frames={frames} bins={bins} fft={} fs={} frame_ms={} y_len={y_len}",
                self.fft_size, self.fs, self.frame_ms
            );
        }
        let out = world_rs::synthesis::synthesize_with_curves(&features, config, &curves, &())
            .map_err(|e| format!("world synthesis: {e:?}"))?;
        if std::env::var_os("WORLD_RS_DBG").is_some() {
            eprintln!("[rs-synth] out={}", out.len());
        }
        self.samples = out.into_iter().map(|s| s as f64).collect();
        Ok(())
    }
}
