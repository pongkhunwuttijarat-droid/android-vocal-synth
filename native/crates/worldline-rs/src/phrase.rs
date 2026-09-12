//! Port of `worldline/phrase_synth.cpp` — `PhraseSynth`: accumulate one
//! `Model` per phoneme, blend them on the phrase timeline, apply the
//! expression curves and synthesize.
//!
//! This is the pure-Rust twin of the C++ `PhraseSynth` the `r1`
//! (`WORLDLINE-R`) engine drives through the `.so`. Both consume exactly
//! the same `PhonemeRequest`/`PhraseCurves` produced by
//! `worldline-plugin::convert`, so the only difference is the DSP kernel.

use std::path::Path;

use worldline_plugin::{PhonemeRequest, PhraseCurves};

use crate::effects::{auto_gain, get_tension_coefficients, shift_gender_row, vec_maxabs};
use crate::f0::F0Estimator;
use crate::model::{Model, SAFE_GUARD_MINIMUM};
use crate::timing::{
    get_in_total_ms, get_time_mapping, model_timing, pad_time_mapping, shift_time_mapping,
    ModelTiming, PADDING,
};

/// Stage-timer shim.
///
/// `std::time::Instant::now()` panics on wasm32-unknown-unknown ("time not
/// implemented on this platform"), so the instrumentation is compiled out
/// there and reports 0.
#[cfg(not(target_arch = "wasm32"))]
fn timer_start() -> Option<std::time::Instant> {
    Some(std::time::Instant::now())
}

#[cfg(target_arch = "wasm32")]
fn timer_start() -> Option<std::time::Instant> {
    None
}

fn timer_ns(start: Option<std::time::Instant>) -> u128 {
    start.map(|s| s.elapsed().as_nanos()).unwrap_or(0)
}

/// The Rust `PhraseSynth`.
#[derive(Default)]
pub struct PhraseSynthRs {
    frame_ms: f64,
    /// Stage timers (ns), reported when `WORLD_RS_TIME=1`.
    t_analysis_ns: u128,
    t_sp_ns: u128,
    t_ap_ns: u128,
    models: Vec<Model>,
    timings: Vec<ModelTiming>,
    f0_curve: Vec<f64>,
    gender_curve: Vec<f64>,
    tension_curve: Vec<f64>,
    breathiness_curve: Vec<f64>,
    voicing_curve: Vec<f64>,
}

impl PhraseSynthRs {
    /// New synth using `frame_ms` for every model (the `.so` build's
    /// `WORLDLINE_FRAME_MS_DEFAULT`: 11.61 ms for v2, 10 ms for v1).
    pub fn new(frame_ms: f64) -> Self {
        Self {
            frame_ms,
            ..Default::default()
        }
    }

    /// `PhraseSynth::AddRequest` — analyze one phoneme and place it on the
    /// phrase timeline.
    ///
    /// `frq` is the raw `<wav>_wav.frq` payload when the bank ships one
    /// (the reference always prefers it over estimation).
    pub fn add_request(&mut self, request: &PhonemeRequest, frq: Option<&[u8]>) -> Result<(), String> {
        let wav = voicebank::read_wav(Path::new(&request.wav_path))
            .map_err(|e| format!("read {}: {e}", request.wav_path))?;
        let mono: Vec<f64> = wav.to_mono().into_iter().map(|s| s as f64).collect();
        self.add_request_from_samples(request, mono, wav.sample_rate as i32, frq)
    }

    /// `AddRequest` over already-decoded mono samples (used by tests and by
    /// hosts that pre-load the voicebank).
    pub fn add_request_from_samples(
        &mut self,
        request: &PhonemeRequest,
        samples: Vec<f64>,
        sample_fs: i32,
        frq: Option<&[u8]>,
    ) -> Result<(), String> {
        let frame_ms = self.frame_ms;
        let estimator = match frq {
            Some(bytes) => match voicebank::parse_frq(bytes) {
                Ok(data) => F0Estimator::Frq(data),
                Err(_) => F0Estimator::Pyin,
            },
            None => F0Estimator::Pyin,
        };
        let t_stage = timer_start();
        let mut model = Model::new(samples, sample_fs, frame_ms, estimator);
        let src_max = vec_maxabs(model.samples());

        // --- F0 ---
        model.build_f0();
        if model.f0().is_empty() {
            return Err(format!("F0 estimation failed for {}", request.phoneme));
        }

        // --- time mapping + trim ---
        let mut mapping = get_time_mapping(&model, request);
        let in_start_ms = request.offset;
        let in_length_ms = get_in_total_ms(&model, request);
        let in_start_frame = (in_start_ms / frame_ms) as i64;
        let in_length_frame = ((in_start_ms + in_length_ms).ceil() / frame_ms) as i64 - in_start_frame;
        let left_trimmed = in_start_frame as f64 * frame_ms;
        if in_start_frame < 0 || in_length_frame <= 0 {
            return Err(format!(
                "invalid input region for {} (start {in_start_frame}, length {in_length_frame})",
                request.phoneme
            ));
        }
        model.trim(in_start_frame as usize, in_length_frame as usize);
        if model.f0().is_empty() {
            return Err(format!("trimmed model is empty for {}", request.phoneme));
        }

        // --- auto gain ---
        let seg_max = vec_maxabs(model.samples());
        let voiced_ratio = model.get_voiced_ratio();
        let volume = request.volume.round() as i32;
        let peak_comp = request.flag_P;
        {
            let samples = model.samples_mut();
            auto_gain(samples, src_max, seg_max, voiced_ratio, volume, peak_comp);
        }

        // --- features ---
        self.t_analysis_ns += timer_ns(t_stage);
        let t_sp = timer_start();
        model.build_sp()?;
        self.t_sp_ns += timer_ns(t_sp);
        let t_ap = timer_start();
        model.build_ap()?;
        self.t_ap_ns += timer_ns(t_ap);

        // --- remap onto the request's frame grid ---
        shift_time_mapping(&mut mapping, -left_trimmed);
        pad_time_mapping(&mut mapping, PADDING);
        model.remap(&mapping);

        let timing = model_timing(request, frame_ms, PADDING);
        if std::env::var_os("WORLD_RS_DBG").is_some() {
            eprintln!(
                "[rs] {} frames={} bins={} trim(start={} len={}) mapping={} p0={} p1={} p3={} p4={} skip={} left={} vol={}",
                request.phoneme,
                model.f0().len(),
                model.bins(),
                in_start_frame,
                in_length_frame,
                mapping.len(),
                timing.p0,
                timing.p1,
                timing.p3,
                timing.p4,
                timing.skip,
                timing.left_extra,
                request.volume,
            );
        }
        self.timings.push(timing);
        self.models.push(model);
        Ok(())
    }

    /// `PhraseSynth::SetCurves`.
    pub fn set_curves(&mut self, curves: &PhraseCurves) {
        self.f0_curve = curves.f0.clone();
        self.gender_curve = curves.gender.clone();
        self.tension_curve = curves.tension.clone();
        self.breathiness_curve = curves.breathiness.clone();
        self.voicing_curve = curves.voicing.clone();
    }

    /// `PhraseSynth::Synth` — blend, apply the expression curves and
    /// resynthesize.
    pub fn synth(&self) -> Result<Vec<f32>, String> {
        // The synth stage is measured locally (this method takes `&self`).
        if self.models.is_empty() {
            return Ok(Vec::new());
        }
        let fs = self.models[0].fs();
        let fft_size = self.models[0].fft_size();
        let bins = self.models[0].bins();
        if fft_size == 0 || bins == 0 {
            return Err("model has no analysis (BuildSp not run)".into());
        }

        let mut f0: Vec<f64> = Vec::new();
        let mut sp: Vec<f64> = Vec::new();
        let mut ap: Vec<f64> = Vec::new();
        let mut dirty: Vec<i32> = Vec::new();

        for (model, timing) in self.models.iter().zip(&self.timings) {
            let p4 = timing.p4.max(0) as usize;
            if p4 > f0.len() {
                f0.resize(p4, 0.0);
            }
            if p4 * bins > sp.len() {
                sp.resize(p4 * bins, SAFE_GUARD_MINIMUM);
                ap.resize(p4 * bins, 1.0);
                dirty.resize(p4, 0);
            }
            let p0 = timing.p0.max(0) as usize;
            for i in p0..p4 {
                let mut weight = 1.0;
                if (i as i32) < timing.p1 {
                    let denom = (timing.p1 - timing.p0).max(1);
                    weight = (i as i32 - timing.p0) as f64 / denom as f64;
                } else if (i as i32) >= timing.p3 {
                    let denom = (timing.p4 - timing.p3).max(1);
                    weight = (timing.p4 - i as i32) as f64 / denom as f64;
                }
                let model_i = timing.left_extra as i64 + i64::from(timing.skip) + i as i64
                    - i64::from(timing.p0);
                if model_i < timing.left_extra as i64 {
                    continue;
                }
                let model_i = model_i as usize;
                if model_i >= model.f0().len() {
                    continue;
                }
                if dirty[i] == 0 || weight > 0.5 {
                    f0[i] = model.f0()[model_i];
                }
                let wa = if dirty[i] == 0 { 0.0 } else { 1.0 - weight };
                let wb = if dirty[i] == 0 { 1.0 } else { weight };
                let sp_row = model.sp_row(model_i, bins);
                let ap_row = model.ap_row(model_i, bins);
                for j in 0..bins {
                    sp[i * bins + j] += sp_row[j] * weight;
                    ap[i * bins + j] = ap[i * bins + j] * wa + ap_row[j] * wb;
                }
                dirty[i] = 1;
            }
        }

        // Grow by one frame, repeating the last value (C++ `resize(length, back())`).
        let length = f0.len() + 1;
        if f0.is_empty() {
            return Ok(Vec::new());
        }
        let last_f0 = *f0.last().expect("non-empty");
        let last_sp = sp[(f0.len() - 1) * bins..].to_vec();
        let last_ap = ap[(f0.len() - 1) * bins..].to_vec();
        f0.push(last_f0);
        sp.extend_from_slice(&last_sp);
        ap.extend_from_slice(&last_ap);

        let extend_curve = |curve: &[f64], default: f64| -> Vec<f64> {
            let mut out = curve.to_vec();
            let fill = out.last().copied().unwrap_or(default);
            out.resize(length, fill);
            out
        };
        let f0_curve = extend_curve(&self.f0_curve, 0.0);
        let gender_curve = extend_curve(&self.gender_curve, 0.5);
        let tension_curve = extend_curve(&self.tension_curve, 0.5);
        let breathiness_curve = extend_curve(&self.breathiness_curve, 0.5);
        let voicing_curve = extend_curve(&self.voicing_curve, 1.0);

        let mut tension_bins: Vec<Vec<f64>> = Vec::with_capacity(length);
        let mut breathiness = vec![1.0f64; length];
        let mut voicing = vec![1.0f64; length];
        for i in 0..length {
            // Voiced frames take the phrase f0 curve; unvoiced keep the model's.
            if f0[i] > 0.0 {
                f0[i] = f0_curve[i];
            }
            // Gender: in-place frequency-axis shift on this frame's row.
            let gender_value = ((gender_curve[i] - 0.5) * 200.0) as i32;
            shift_gender_row(&mut sp[i * bins..(i + 1) * bins], gender_value);
            // Breathiness: the reference doubles/halves around 0.5.
            breathiness[i] = if breathiness_curve[i] > 0.5 {
                breathiness_curve[i] * 4.0
            } else {
                breathiness_curve[i] * 2.0
            };
            tension_bins.push(get_tension_coefficients(
                f0_curve[i],
                fs,
                ((tension_curve[i] - 0.5) * 200.0) as i32,
                bins,
            ));
            voicing[i] = voicing_curve[i];
        }

        let t_synth = timer_start();
        let mut final_model = Model::new_synth(fs, self.frame_ms, fft_size);
        final_model.set_analysis(f0, sp, ap, fft_size);
        final_model.synth(&tension_bins, &breathiness, &voicing)?;
        let synth_ns = timer_ns(t_synth);

        if std::env::var_os("WORLD_RS_TIME").is_some() {
            let ms = |ns: u128| ns as f64 / 1.0e6;
            let total = (self.t_analysis_ns + self.t_sp_ns + self.t_ap_ns + synth_ns) as f64
                / 1.0e6;
            eprintln!(
                "[time] analysis(f0/trim/remap)={:.1}ms cheaptrick={:.1}ms d4c={:.1}ms synth={:.1}ms | total {:.1}ms | cheaptrick {:.0}% d4c {:.0}% synth {:.0}%",
                ms(self.t_analysis_ns),
                ms(self.t_sp_ns),
                ms(self.t_ap_ns),
                ms(synth_ns),
                total,
                100.0 * ms(self.t_sp_ns) / total.max(1.0),
                100.0 * ms(self.t_ap_ns) / total.max(1.0),
                100.0 * ms(synth_ns) / total.max(1.0),
            );
        }
        let mut samples: Vec<f64> = final_model.samples().to_vec();
        // 10 ms fade-out "to ease abruptive ending".
        let fade_out_samples = (f64::from(fs) * 10.0 / 1000.0) as usize;
        let n = samples.len();
        for i in 0..fade_out_samples.min(n) {
            samples[n - 1 - i] *= i as f64 / fade_out_samples as f64;
        }
        Ok(samples.into_iter().map(|s| s as f32).collect())
    }
}
