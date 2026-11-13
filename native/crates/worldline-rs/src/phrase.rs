//! Port of `worldline/phrase_synth.cpp` — `PhraseSynth`: accumulate one
//! `Model` per phoneme, blend them on the phrase timeline, apply the
//! expression curves and synthesize.
//!
//! This is the pure-Rust twin of the C++ `PhraseSynth` the `r1`
//! (`WORLDLINE-R`) engine drives through the `.so`. Both consume exactly
//! the same `PhonemeRequest`/`PhraseCurves` produced by
//! `worldline-plugin::convert`, so the only difference is the DSP kernel.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::Mutex;

use world_rs::synthesis::{ExpressionCurves, SynthesisConfig, WorldStreamCursorRs};
use worldline_plugin::{PhonemeRequest, PhraseCurves};

use crate::effects::{auto_gain, get_tension_coefficients, shift_gender_row, vec_maxabs};
use crate::f0::F0Estimator;
use crate::model::{Model, SAFE_GUARD_MINIMUM};
use crate::prepared_cache::PreparedModelCache;
use crate::source::{ResolvedSample, SampleSource};
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

/// Pipeline execution & memory telemetry for `render_phrase_stream`.
#[derive(Debug, Clone, Copy, Default)]
pub struct StreamStatsRs {
    pub first_block_ms: f64,
    pub total_ms: f64,
    pub prep_workers: usize,
    pub synth_workers: usize,
    pub peak_buffered_models: usize,
    pub peak_timeline_frames: usize,
    pub total_frames: usize,
    pub output_samples: usize,
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub prep_wait_ms: f64,
    pub backpressure_wait_ms: f64,
}

/// Analyze and remap one phoneme request into `(Model, ModelTiming)`, consulting
/// `cache` when supplied.
pub fn analyze_request_with_cache(
    request: &PhonemeRequest,
    samples: Vec<f64>,
    sample_fs: i32,
    frq: Option<&[u8]>,
    frame_ms: f64,
    cache: Option<&Mutex<PreparedModelCache>>,
    timings_out: Option<(&mut u128, &mut u128, &mut u128)>,
) -> Result<(Model, ModelTiming, bool), String> {
    let timing = model_timing(request, frame_ms, PADDING);
    let cache_key = if cache.is_some() {
        Some(PreparedModelCache::key(
            request, &samples, sample_fs, frq, frame_ms,
        ))
    } else {
        None
    };
    if let (Some(cache), Some(key)) = (cache, cache_key.as_ref()) {
        if let Ok(mut guard) = cache.lock() {
            if let Some(model) = guard.get(key) {
                return Ok((model, timing, true));
            }
        }
    }

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
    let in_length_frame =
        ((in_start_ms + in_length_ms).ceil() / frame_ms) as i64 - in_start_frame;
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

    let analysis_ns = timer_ns(t_stage);
    let t_sp = timer_start();
    model.build_sp()?;
    let sp_ns = timer_ns(t_sp);
    let t_ap = timer_start();
    model.build_ap()?;
    let ap_ns = timer_ns(t_ap);
    if let Some((a, s, p)) = timings_out {
        *a += analysis_ns;
        *s += sp_ns;
        *p += ap_ns;
    }

    // --- remap onto the request's frame grid ---
    shift_time_mapping(&mut mapping, -left_trimmed);
    pad_time_mapping(&mut mapping, PADDING);
    model.remap(&mapping);

    if let (Some(cache), Some(key)) = (cache, cache_key) {
        if let Ok(mut guard) = cache.lock() {
            guard.put(key, &model);
        }
    }
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
    Ok((model, timing, false))
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
    pub fn add_request(
        &mut self,
        request: &PhonemeRequest,
        frq: Option<&[u8]>,
    ) -> Result<(), String> {
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
        self.add_request_from_samples_cached(request, samples, sample_fs, frq, None)
    }

    /// `AddRequest` with an optional exact prepared-model cache.
    pub fn add_request_from_samples_cached(
        &mut self,
        request: &PhonemeRequest,
        samples: Vec<f64>,
        sample_fs: i32,
        frq: Option<&[u8]>,
        mut cache: Option<&mut PreparedModelCache>,
    ) -> Result<(), String> {
        let frame_ms = self.frame_ms;
        let cache_key = cache
            .as_ref()
            .map(|_| PreparedModelCache::key(request, &samples, sample_fs, frq, frame_ms));
        if let (Some(cache), Some(key)) = (cache.as_deref_mut(), cache_key.as_ref()) {
            if let Some(model) = cache.get(key) {
                self.timings.push(model_timing(request, frame_ms, PADDING));
                self.models.push(model);
                return Ok(());
            }
        }
        let (model, timing, _) = analyze_request_with_cache(
            request,
            samples,
            sample_fs,
            frq,
            frame_ms,
            None,
            Some((&mut self.t_analysis_ns, &mut self.t_sp_ns, &mut self.t_ap_ns)),
        )?;
        if let (Some(cache), Some(key)) = (cache.as_deref_mut(), cache_key) {
            cache.put(key, &model);
        }
        self.timings.push(timing);
        self.models.push(model);
        Ok(())
    }

    /// `AddRequest` consulting a shared `Mutex<PreparedModelCache>` only for cache get/put.
    pub fn add_request_from_samples_shared_cache(
        &mut self,
        request: &PhonemeRequest,
        samples: Vec<f64>,
        sample_fs: i32,
        frq: Option<&[u8]>,
        cache: Option<&Mutex<PreparedModelCache>>,
    ) -> Result<(), String> {
        let (model, timing, _) = analyze_request_with_cache(
            request,
            samples,
            sample_fs,
            frq,
            self.frame_ms,
            cache,
            Some((&mut self.t_analysis_ns, &mut self.t_sp_ns, &mut self.t_ap_ns)),
        )?;
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
            let total =
                (self.t_analysis_ns + self.t_sp_ns + self.t_ap_ns + synth_ns) as f64 / 1.0e6;
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

fn curve_at(curve: &[f64], index: usize, default: f64) -> f64 {
    curve
        .get(index)
        .or_else(|| curve.last())
        .copied()
        .unwrap_or(default)
}

#[cfg(not(target_arch = "wasm32"))]
fn elapsed_ms(start: Option<std::time::Instant>) -> f64 {
    start.map(|s| s.elapsed().as_secs_f64() * 1000.0).unwrap_or(0.0)
}

#[cfg(target_arch = "wasm32")]
fn elapsed_ms(_start: Option<std::time::Instant>) -> f64 {
    0.0
}

struct IncrementalStreamer<'a, F>
where
    F: FnMut(&[f32]) -> Result<(), String>,
{
    model_frame_ms: f64,
    curves: &'a PhraseCurves,
    suffix_min_p0: Vec<usize>,
    total_count: usize,
    block_samples: usize,
    callback: F,
    cursor: Option<WorldStreamCursorRs>,
    fs: i32,
    fft_size: usize,
    bins: usize,
    tail_samples: usize,
    raw_base: usize,
    cur_raw_len: usize,
    raw_f0: VecDeque<f64>,
    raw_sp: VecDeque<Vec<f64>>,
    raw_ap: VecDeque<Vec<f64>>,
    raw_dirty: VecDeque<i32>,
    curve_base: usize,
    converted_end: usize,
    c_f0: VecDeque<f64>,
    c_sp: VecDeque<Vec<f64>>,
    c_ap: VecDeque<Vec<f64>>,
    c_ten: VecDeque<Vec<f64>>,
    c_bre: VecDeque<f64>,
    c_voi: VecDeque<f64>,
    tail_pending: VecDeque<f32>,
    float_scratch: Vec<f32>,
    first_block_ms: Option<f64>,
    emitted_total: usize,
    peak_timeline_frames: usize,
    last_fed_end: usize,
    started: Option<std::time::Instant>,
}

impl<'a, F> IncrementalStreamer<'a, F>
where
    F: FnMut(&[f32]) -> Result<(), String>,
{
    fn new(
        model_frame_ms: f64,
        curves: &'a PhraseCurves,
        suffix_min_p0: Vec<usize>,
        total_count: usize,
        block_samples: usize,
        callback: F,
        started: Option<std::time::Instant>,
    ) -> Self {
        let block_samples = block_samples.max(1);
        Self {
            model_frame_ms,
            curves,
            suffix_min_p0,
            total_count,
            block_samples,
            callback,
            cursor: None,
            fs: 0,
            fft_size: 0,
            bins: 0,
            tail_samples: 0,
            raw_base: 0,
            cur_raw_len: 0,
            raw_f0: VecDeque::new(),
            raw_sp: VecDeque::new(),
            raw_ap: VecDeque::new(),
            raw_dirty: VecDeque::new(),
            curve_base: 0,
            converted_end: 0,
            c_f0: VecDeque::new(),
            c_sp: VecDeque::new(),
            c_ap: VecDeque::new(),
            c_ten: VecDeque::new(),
            c_bre: VecDeque::new(),
            c_voi: VecDeque::new(),
            tail_pending: VecDeque::new(),
            float_scratch: Vec::with_capacity(block_samples),
            first_block_ms: None,
            emitted_total: 0,
            peak_timeline_frames: 0,
            last_fed_end: 0,
            started,
        }
    }

    fn push_model(
        &mut self,
        k: usize,
        model: Model,
        timing: ModelTiming,
        next_ready: bool,
    ) -> Result<(), String> {
        if self.cursor.is_none() {
            self.fs = model.fs();
            self.fft_size = model.fft_size();
            self.bins = model.bins();
            if self.fft_size == 0 || self.bins == 0 {
                return Err("model has no analysis (BuildSp not run)".into());
            }
            self.tail_samples = (f64::from(self.fs) * 10.0 / 1000.0) as usize;
            let config = SynthesisConfig {
                sample_rate: self.fs as u32,
                frame_period_ms: self.model_frame_ms,
                fft_size: self.fft_size,
                output_length: None,
                ..Default::default()
            };
            let cursor = WorldStreamCursorRs::new(config, self.bins, self.block_samples)
                .map_err(|e| format!("world stream cursor init: {e:?}"))?;
            self.cursor = Some(cursor);
        }

        let bins = self.bins;
        let fs = self.fs;
        let p4 = timing.p4.max(0) as usize;
        if p4 > self.cur_raw_len {
            let add = p4 - self.cur_raw_len;
            self.raw_f0.extend(std::iter::repeat(0.0).take(add));
            for _ in 0..add {
                self.raw_sp.push_back(vec![SAFE_GUARD_MINIMUM; bins]);
                self.raw_ap.push_back(vec![1.0; bins]);
            }
            self.raw_dirty.extend(std::iter::repeat(0).take(add));
            self.cur_raw_len = p4;
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
            let local_i = i - self.raw_base;
            if self.raw_dirty[local_i] == 0 || weight > 0.5 {
                self.raw_f0[local_i] = model.f0()[model_i];
            }
            let wa = if self.raw_dirty[local_i] == 0 {
                0.0
            } else {
                1.0 - weight
            };
            let wb = if self.raw_dirty[local_i] == 0 {
                1.0
            } else {
                weight
            };
            let sp_row = model.sp_row(model_i, bins);
            let ap_row = model.ap_row(model_i, bins);
            let dst_sp = &mut self.raw_sp[local_i];
            let dst_ap = &mut self.raw_ap[local_i];
            for j in 0..bins {
                dst_sp[j] += sp_row[j] * weight;
                dst_ap[j] = dst_ap[j] * wa + ap_row[j] * wb;
            }
            self.raw_dirty[local_i] = 1;
        }
        drop(model);

        let is_final = k + 1 == self.total_count;
        let finalized_end = if is_final {
            self.cur_raw_len
        } else {
            self.cur_raw_len.min(self.suffix_min_p0[k + 1])
        };
        let target_convert_end = if is_final {
            self.cur_raw_len
        } else {
            self.converted_end.max(finalized_end.saturating_sub(1))
        };

        let mut dup: Option<(f64, Vec<f64>, Vec<f64>)> = None;
        for i in self.converted_end..target_convert_end {
            let mut f0_val = self.raw_f0.pop_front().expect("raw_f0");
            let mut sp_row = self.raw_sp.pop_front().expect("raw_sp");
            let ap_row = self.raw_ap.pop_front().expect("raw_ap");
            self.raw_dirty.pop_front();
            self.raw_base += 1;
            if is_final && i + 1 == self.cur_raw_len {
                dup = Some((f0_val, sp_row.clone(), ap_row.clone()));
            }
            let cf0 = curve_at(&self.curves.f0, i, 0.0);
            let cgen = curve_at(&self.curves.gender, i, 0.5);
            let cten = curve_at(&self.curves.tension, i, 0.5);
            let cbre = curve_at(&self.curves.breathiness, i, 0.5);
            let cvoi = curve_at(&self.curves.voicing, i, 1.0);
            if f0_val > 0.0 {
                f0_val = cf0;
            }
            let gender_value = ((cgen - 0.5) * 200.0) as i32;
            shift_gender_row(&mut sp_row, gender_value);
            let bre = if cbre > 0.5 { cbre * 4.0 } else { cbre * 2.0 };
            let ten =
                get_tension_coefficients(cf0, fs, ((cten - 0.5) * 200.0) as i32, bins);
            self.c_f0.push_back(f0_val);
            self.c_sp.push_back(sp_row);
            self.c_ap.push_back(ap_row);
            self.c_ten.push_back(ten);
            self.c_bre.push_back(bre);
            self.c_voi.push_back(cvoi);
        }
        self.converted_end = target_convert_end;

        if is_final {
            if let Some((mut f0_val, mut sp_row, ap_row)) = dup {
                let i = self.cur_raw_len;
                let cf0 = curve_at(&self.curves.f0, i, 0.0);
                let cgen = curve_at(&self.curves.gender, i, 0.5);
                let cten = curve_at(&self.curves.tension, i, 0.5);
                let cbre = curve_at(&self.curves.breathiness, i, 0.5);
                let cvoi = curve_at(&self.curves.voicing, i, 1.0);
                if f0_val > 0.0 {
                    f0_val = cf0;
                }
                let gender_value = ((cgen - 0.5) * 200.0) as i32;
                shift_gender_row(&mut sp_row, gender_value);
                let bre = if cbre > 0.5 { cbre * 4.0 } else { cbre * 2.0 };
                let ten =
                    get_tension_coefficients(cf0, fs, ((cten - 0.5) * 200.0) as i32, bins);
                self.c_f0.push_back(f0_val);
                self.c_sp.push_back(sp_row);
                self.c_ap.push_back(ap_row);
                self.c_ten.push_back(ten);
                self.c_bre.push_back(bre);
                self.c_voi.push_back(cvoi);
                self.converted_end += 1;
            }
        }

        self.peak_timeline_frames = self
            .peak_timeline_frames
            .max(self.raw_f0.len() + self.c_f0.len());

        let avail = self.converted_end - self.curve_base;
        let should_feed = is_final
            || (avail >= 2
                && (!next_ready
                    || self.first_block_ms.is_none()
                    || self.converted_end.saturating_sub(self.last_fed_end) >= 8));
        if should_feed && (is_final || avail >= 2) {
            if is_final && self.converted_end == 0 {
                return Ok(());
            }
            let win_f0: Vec<f64> = self.c_f0.iter().copied().collect();
            let mut win_sp = Vec::with_capacity(avail * bins);
            for row in &self.c_sp {
                win_sp.extend_from_slice(row);
            }
            let mut win_ap = Vec::with_capacity(avail * bins);
            for row in &self.c_ap {
                win_ap.extend_from_slice(row);
            }
            let mut win_ten = Vec::with_capacity(avail * bins);
            for row in &self.c_ten {
                win_ten.extend_from_slice(row);
            }
            let curves_win = ExpressionCurves {
                tension: Vec::new(),
                tension_bins: Some(win_ten),
                breathiness: self.c_bre.iter().copied().collect(),
                voicing: self.c_voi.iter().copied().collect(),
            };
            let final_y_len = is_final.then(|| {
                (f64::from(fs) * (self.converted_end as f64 - 1.0) * self.model_frame_ms
                    / 1000.0) as usize
                    + 1
            });

            let tail_samples = self.tail_samples;
            let block_samples = self.block_samples;
            let tail_pending = &mut self.tail_pending;
            let float_scratch = &mut self.float_scratch;
            let first_block_ms = &mut self.first_block_ms;
            let emitted_total = &mut self.emitted_total;
            let callback = &mut self.callback;
            let started = self.started;
            let cursor = self.cursor.as_mut().expect("cursor initialized");

            let consumed_end = cursor
                .feed_f64(
                    self.curve_base,
                    self.converted_end,
                    is_final.then_some(self.converted_end),
                    final_y_len,
                    &win_f0,
                    &win_sp,
                    &win_ap,
                    &curves_win,
                    &(),
                    |samples| {
                        tail_pending.extend(samples.iter().map(|&s| s as f32));
                        while tail_pending.len().saturating_sub(tail_samples) >= block_samples {
                            float_scratch.clear();
                            for _ in 0..block_samples {
                                float_scratch.push(tail_pending.pop_front().unwrap());
                            }
                            if first_block_ms.is_none() {
                                *first_block_ms = Some(elapsed_ms(started));
                            }
                            callback(float_scratch)?;
                            *emitted_total += float_scratch.len();
                        }
                        Ok(())
                    },
                )
                .map_err(|e| match e {
                    world_rs::synthesis::SynthesisError::Callback(msg) => msg,
                    other => format!("world stream synthesis: {other:?}"),
                })?;

            self.last_fed_end = self.converted_end;
            while self.curve_base < consumed_end && !self.c_f0.is_empty() {
                self.c_f0.pop_front();
                self.c_sp.pop_front();
                self.c_ap.pop_front();
                self.c_ten.pop_front();
                self.c_bre.pop_front();
                self.c_voi.pop_front();
                self.curve_base += 1;
            }
        }
        Ok(())
    }

    fn finish(mut self, stats: &mut StreamStatsRs) -> Result<(), String> {
        let len = self.tail_pending.len();
        if len > 0 {
            for i in 0..self.tail_samples.min(len) {
                let idx = len - 1 - i;
                self.tail_pending[idx] =
                    (f64::from(self.tail_pending[idx]) * (i as f64 / self.tail_samples as f64))
                        as f32;
            }
            while !self.tail_pending.is_empty() {
                let chunk = self.tail_pending.len().min(self.block_samples);
                self.float_scratch.clear();
                for _ in 0..chunk {
                    self.float_scratch.push(self.tail_pending.pop_front().unwrap());
                }
                if self.first_block_ms.is_none() {
                    self.first_block_ms = Some(elapsed_ms(self.started));
                }
                (self.callback)(&self.float_scratch)?;
                self.emitted_total += self.float_scratch.len();
            }
        }
        stats.first_block_ms = self.first_block_ms.unwrap_or_else(|| elapsed_ms(self.started));
        stats.total_ms = elapsed_ms(self.started);
        stats.peak_timeline_frames = self.peak_timeline_frames;
        stats.total_frames = self.converted_end;
        stats.output_samples = self.emitted_total;
        Ok(())
    }
}

fn resolve_cached<S: SampleSource + ?Sized>(
    source: &S,
    cache: &Mutex<HashMap<String, ResolvedSample>>,
    wav_path: &str,
) -> Result<ResolvedSample, String> {
    if let Ok(guard) = cache.lock() {
        if let Some(hit) = guard.get(wav_path) {
            return Ok(hit.clone());
        }
    }
    let resolved = source.resolve(wav_path)?;
    if let Ok(mut guard) = cache.lock() {
        guard.insert(wav_path.to_string(), resolved.clone());
    }
    Ok(resolved)
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_positive_env(name: &str) -> Option<usize> {
    std::env::var(name)
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|n| *n > 0)
}

pub fn render_phrase_stream_with_source_and_cache<S, F>(
    input: &feed::render_input::RenderInput,
    curve_frame_ms: f64,
    model_frame_ms: f64,
    source: &S,
    cache: Option<&Mutex<PreparedModelCache>>,
    block_samples: usize,
    mut callback: F,
) -> Result<(usize, StreamStatsRs), String>
where
    S: SampleSource + Sync + ?Sized,
    F: FnMut(&[f32]) -> Result<(), String>,
{
    let started = timer_start();
    let requests =
        worldline_plugin::build_requests(input).map_err(|e| format!("build_requests: {e}"))?;
    if requests.is_empty() {
        return Ok((0, StreamStatsRs::default()));
    }
    let curves = worldline_plugin::sample_phrase_curves(input, curve_frame_ms);
    let total_count = requests.len();
    let mut suffix_min_p0 = vec![usize::MAX; total_count + 1];
    for i in (0..total_count).rev() {
        let t = model_timing(&requests[i], model_frame_ms, PADDING);
        suffix_min_p0[i] = (t.p0.max(0) as usize).min(suffix_min_p0[i + 1]);
    }

    let wav_cache: Mutex<HashMap<String, ResolvedSample>> = Mutex::new(HashMap::new());
    let mut stats = StreamStatsRs::default();
    stats.synth_workers = 1;

    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut prep_workers = total_count.min(2);
        if let Some(v) = parse_positive_env("WORLDLINE_STREAM_PREP_THREADS")
            .or_else(|| parse_positive_env("WORLDLINE_PREP_THREADS"))
        {
            prep_workers = v.min(total_count).max(1);
        }
        stats.prep_workers = prep_workers;
        let max_in_flight = parse_positive_env("WORLDLINE_STREAM_MAX_INFLIGHT")
            .unwrap_or_else(|| 4.max(prep_workers * 4))
            .max(1);

        struct Coord {
            next_assign: usize,
            commit_idx: usize,
            buffered: usize,
            peak_buffered: usize,
            cache_hits: usize,
            cache_misses: usize,
            prep_wait_ns: u128,
            backpressure_wait_ns: u128,
            cancelled: bool,
            slots: Vec<Option<Result<(Model, ModelTiming), String>>>,
        }
        let coord = Mutex::new(Coord {
            next_assign: 0,
            commit_idx: 0,
            buffered: 0,
            peak_buffered: 0,
            cache_hits: 0,
            cache_misses: 0,
            prep_wait_ns: 0,
            backpressure_wait_ns: 0,
            cancelled: false,
            slots: (0..total_count).map(|_| None).collect(),
        });
        let cv_workers = std::sync::Condvar::new();
        let cv_synth = std::sync::Condvar::new();

        let mut consumer = IncrementalStreamer::new(
            model_frame_ms,
            &curves,
            suffix_min_p0,
            total_count,
            block_samples,
            &mut callback,
            started,
        );

        let stream_res: Result<(), String> = std::thread::scope(|scope| {
            for _ in 0..prep_workers {
                scope.spawn(|| loop {
                    let index = {
                        let mut guard = coord.lock().expect("coord lock");
                        let wait_t = if !guard.cancelled
                            && guard.next_assign < total_count
                            && (guard.next_assign - guard.commit_idx) >= max_in_flight
                        {
                            timer_start()
                        } else {
                            None
                        };
                        while !guard.cancelled
                            && guard.next_assign < total_count
                            && (guard.next_assign - guard.commit_idx) >= max_in_flight
                        {
                            guard = cv_workers.wait(guard).expect("cv_workers wait");
                        }
                        if wait_t.is_some() {
                            guard.backpressure_wait_ns =
                                guard.backpressure_wait_ns.saturating_add(timer_ns(wait_t));
                        }
                        if guard.cancelled || guard.next_assign >= total_count {
                            return;
                        }
                        let idx = guard.next_assign;
                        guard.next_assign += 1;
                        idx
                    };

                    let request = &requests[index];
                    let prep_res = resolve_cached(source, &wav_cache, &request.wav_path).and_then(
                        |resolved| {
                            worldline_plugin::validate_request(
                                request,
                                resolved.wav.sample_rate,
                                resolved.wav.samples.len(),
                            )
                            .map_err(|e| format!("validate {}: {e}", request.phoneme))?;
                            let mono: Vec<f64> =
                                resolved.wav.to_mono().into_iter().map(|s| s as f64).collect();
                            analyze_request_with_cache(
                                request,
                                mono,
                                resolved.wav.sample_rate as i32,
                                resolved.frq.as_deref(),
                                model_frame_ms,
                                cache,
                                None,
                            )
                        },
                    );

                    let mut guard = coord.lock().expect("coord lock");
                    if guard.cancelled {
                        return;
                    }
                    match prep_res {
                        Ok((model, timing, hit)) => {
                            if hit {
                                guard.cache_hits += 1;
                            } else {
                                guard.cache_misses += 1;
                            }
                            guard.slots[index] = Some(Ok((model, timing)));
                            guard.buffered += 1;
                            guard.peak_buffered = guard.peak_buffered.max(guard.buffered);
                        }
                        Err(e) => {
                            guard.slots[index] = Some(Err(e));
                        }
                    }
                    cv_synth.notify_one();
                });
            }

            for k in 0..total_count {
                let (item, next_ready) = {
                    let mut guard = coord.lock().expect("coord lock");
                    let wait_t = if guard.slots[k].is_none() && !guard.cancelled {
                        timer_start()
                    } else {
                        None
                    };
                    while guard.slots[k].is_none() && !guard.cancelled {
                        guard = cv_synth.wait(guard).expect("cv_synth wait");
                    }
                    if wait_t.is_some() {
                        guard.prep_wait_ns = guard.prep_wait_ns.saturating_add(timer_ns(wait_t));
                    }
                    if guard.cancelled {
                        return Err("stream cancelled".to_string());
                    }
                    let slot = guard.slots[k].take().expect("slot checked");
                    guard.commit_idx = k + 1;
                    if slot.is_ok() {
                        guard.buffered = guard.buffered.saturating_sub(1);
                    }
                    let next_ready = k + 1 < total_count && guard.slots[k + 1].is_some();
                    (slot, next_ready)
                };
                cv_workers.notify_all();

                let res = match item {
                    Ok((model, timing)) => consumer.push_model(k, model, timing, next_ready),
                    Err(e) => Err(e),
                };
                if let Err(err) = res {
                    {
                        let mut guard = coord.lock().expect("coord lock");
                        guard.cancelled = true;
                    }
                    cv_workers.notify_all();
                    return Err(err);
                }
            }

            let guard = coord.lock().expect("coord lock");
            stats.peak_buffered_models = guard.peak_buffered;
            stats.cache_hits = guard.cache_hits;
            stats.cache_misses = guard.cache_misses;
            stats.prep_wait_ms = guard.prep_wait_ns as f64 / 1_000_000.0;
            stats.backpressure_wait_ms = guard.backpressure_wait_ns as f64 / 1_000_000.0;
            Ok(())
        });

        stream_res?;
        consumer.finish(&mut stats)?;
        return Ok((stats.output_samples, stats));
    }

    #[cfg(target_arch = "wasm32")]
    {
        stats.prep_workers = 1;
        stats.peak_buffered_models = 1;
        let mut consumer = IncrementalStreamer::new(
            model_frame_ms,
            &curves,
            suffix_min_p0,
            total_count,
            block_samples,
            &mut callback,
            started,
        );
        for (k, request) in requests.iter().enumerate() {
            let resolved = resolve_cached(source, &wav_cache, &request.wav_path)?;
            worldline_plugin::validate_request(
                request,
                resolved.wav.sample_rate,
                resolved.wav.samples.len(),
            )
            .map_err(|e| format!("validate {}: {e}", request.phoneme))?;
            let mono: Vec<f64> =
                resolved.wav.to_mono().into_iter().map(|s| s as f64).collect();
            let (model, timing, hit) = analyze_request_with_cache(
                request,
                mono,
                resolved.wav.sample_rate as i32,
                resolved.frq.as_deref(),
                model_frame_ms,
                cache,
                None,
            )?;
            if hit {
                stats.cache_hits += 1;
            } else {
                stats.cache_misses += 1;
            }
            consumer.push_model(k, model, timing, false)?;
        }
        consumer.finish(&mut stats)?;
        Ok((stats.output_samples, stats))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model_frame_ms, render_phrase_stream_with_source_and_cache, render_phrase_with_source,
        MemorySource, PreparedModelCache,
    };
    use feed::render_input::{
        Curves, EnvelopePoint, OtoEntry, PhraseInfo, RenderInput, RenderNote, RenderPhoneme,
        SampleBased, TempoMapPoint,
    };

    fn harmonic_wav_bytes(freq_hz: f64, duration_s: f64) -> Vec<u8> {
        let fs = 44_100;
        let n = (fs as f64 * duration_s) as usize;
        let samples: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f64 / fs as f64;
                let w = 2.0 * std::f64::consts::PI * freq_hz * t;
                (0.35 * w.sin() + 0.2 * (2.0 * w).sin() + 0.1 * (3.0 * w).sin()) as f32
            })
            .collect();
        wavwriter::write_wav_16(&samples, fs as u32)
    }

    fn synthetic_phrase_input() -> (RenderInput, MemorySource) {
        let mut source = MemorySource::new();
        source
            .insert_wav_bytes("/mem/long.wav", &harmonic_wav_bytes(220.0, 1.1))
            .unwrap();
        source
            .insert_wav_bytes("/mem/short.wav", &harmonic_wav_bytes(261.63, 0.35))
            .unwrap();

        let count = 5;
        let step_ms = 180.0;
        let dur_ms = 240.0;
        let mut notes = Vec::with_capacity(count);
        let mut phonemes = Vec::with_capacity(count);
        let mut otos = Vec::with_capacity(count);
        for i in 0..count {
            let pos = i as f64 * step_ms;
            let tone = 60 + (i as i32 % 4);
            let wav_path = if i == 0 {
                "/mem/long.wav"
            } else {
                "/mem/short.wav"
            };
            notes.push(RenderNote {
                lyric: format!("p{i}"),
                tone,
                position_ms: pos,
                duration_ms: dur_ms,
            });
            phonemes.push(RenderPhoneme {
                phoneme: format!("p{i}"),
                position_ms: pos,
                duration_ms: dur_ms,
                leading_ms: 20.0,
                overlap_ms: if i == 0 { 0.0 } else { 40.0 },
                tone,
                adjusted_tempo: 120.0,
                consonant_velocity: 100.0 + (i as f32) * 5.0,
                volume: 95.0,
                modulation: 10.0,
                tone_shift: 0,
                index: i as i32,
                parent_note: i,
            });
            otos.push(OtoEntry {
                alias: format!("p{i}"),
                file: wav_path.to_string(),
                wav_path: wav_path.to_string(),
                offset: 30.0,
                consonant: 50.0,
                cutoff: -250.0,
                preutter: 20.0,
                overlap: if i == 0 { 0.0 } else { 40.0 },
                envelope: vec![
                    EnvelopePoint::new(-20.0, 0.0),
                    EnvelopePoint::new(0.0, 1.0),
                    EnvelopePoint::new(40.0, 1.0),
                    EnvelopePoint::new(190.0, 0.85),
                    EnvelopePoint::new(240.0, 0.0),
                ],
                flags: "g2B10H10P86Mt0Mb0Mv100".to_string(),
            });
        }

        let curve_len = 500;
        let pitches_f32: Vec<f32> = (0..curve_len)
            .map(|i| 6000.0 + (i as f32 * 0.15).sin() * 80.0)
            .collect();
        let pitches_i32 = pitches_f32.iter().map(|v| v.round() as i32).collect();
        let curves = Curves {
            dynamics: (0..curve_len)
                .map(|i| 0.85 + 0.1 * (i as f32 * 0.08).cos())
                .collect(),
            gender: (0..curve_len)
                .map(|i| (i as f32 * 0.05).sin() * 6.0)
                .collect(),
            breathiness: vec![12.0; curve_len],
            tension: vec![5.0; curve_len],
            voicing: vec![92.0; curve_len],
            extra: Vec::new(),
        };

        let input = RenderInput {
            phrase: PhraseInfo {
                position_ms: 0.0,
                duration_ms: (count - 1) as f64 * step_ms + dur_ms,
                leading_ms: 20.0,
                leading_ticks: 20,
                tempo_map: vec![TempoMapPoint {
                    tick_pos: 0,
                    ms_pos: 0.0,
                    bpm: 120.0,
                }],
                time_axis_hint: Some("120bpm 4/4".to_string()),
            },
            notes,
            phonemes,
            pitches_cents: pitches_i32,
            pitches_cents_f32: pitches_f32,
            curves,
            curve_grid_start_tick: 0,
            render_origin_ms: 0.0,
            sample_based: Some(SampleBased {
                oto: otos,
                wav_path: Some("/mem/long.wav".to_string()),
                envelope: Vec::new(),
                flags: String::new(),
            }),
            neural: None,
        };
        (input, source)
    }

    #[test]
    fn stream_matches_offline_bit_for_bit_across_block_sizes() {
        let (input, source) = synthetic_phrase_input();
        let curve_ms = worldline_plugin::DEFAULT_FRAME_MS;
        let model_ms = model_frame_ms();
        let offline =
            render_phrase_with_source(&input, curve_ms, model_ms, &source).expect("offline render");
        let offline2 =
            render_phrase_with_source(&input, curve_ms, model_ms, &source).expect("offline2 render");
        assert_eq!(offline, offline2, "offline must be deterministic");
        assert!(!offline.is_empty());

        for block in [192usize, 1024usize] {
            let mut streamed = Vec::new();
            let mut blocks = 0usize;
            let (total, stats) = render_phrase_stream_with_source_and_cache(
                &input,
                curve_ms,
                model_ms,
                &source,
                None,
                block,
                |chunk| {
                    blocks += 1;
                    streamed.extend_from_slice(chunk);
                    Ok(())
                },
            )
            .expect("stream render");
            assert!(blocks > 1);
            assert_eq!(total, offline.len());
            assert_eq!(stats.output_samples, offline.len());
            assert_eq!(stats.synth_workers, 1);
            assert!(stats.prep_workers >= 1);
            assert_eq!(streamed.len(), offline.len());
            let max_err = streamed
                .iter()
                .zip(offline.iter())
                .map(|(a, b)| (a - b).abs())
                .fold(0.0f32, f32::max);
            assert_eq!(max_err, 0.0, "block={block} max_err={max_err}");
        }
    }

    #[test]
    fn stream_warmed_cache_hits_all_requests_and_matches_offline() {
        let (input, source) = synthetic_phrase_input();
        let curve_ms = worldline_plugin::DEFAULT_FRAME_MS;
        let model_ms = model_frame_ms();
        let offline =
            render_phrase_with_source(&input, curve_ms, model_ms, &source).expect("offline render");
        let cache = Mutex::new(PreparedModelCache::new(32 * 1024 * 1024));

        let mut cold_pcm = Vec::new();
        let (_, cold_stats) = render_phrase_stream_with_source_and_cache(
            &input,
            curve_ms,
            model_ms,
            &source,
            Some(&cache),
            512,
            |chunk| {
                cold_pcm.extend_from_slice(chunk);
                Ok(())
            },
        )
        .expect("cold stream");
        assert_eq!(cold_stats.cache_misses, 5);
        assert_eq!(cold_stats.cache_hits, 0);

        let mut warm_pcm = Vec::new();
        let (_, warm_stats) = render_phrase_stream_with_source_and_cache(
            &input,
            curve_ms,
            model_ms,
            &source,
            Some(&cache),
            512,
            |chunk| {
                warm_pcm.extend_from_slice(chunk);
                Ok(())
            },
        )
        .expect("warm stream");
        assert_eq!(warm_stats.cache_hits, 5);
        assert_eq!(warm_stats.cache_misses, 0);
        assert_eq!(cold_pcm, offline);
        assert_eq!(warm_pcm, offline);
    }

    #[test]
    fn stream_cancellation_aborts_cleanly_without_hanging() {
        let (input, source) = synthetic_phrase_input();
        let curve_ms = worldline_plugin::DEFAULT_FRAME_MS;
        let model_ms = model_frame_ms();
        let mut callbacks = 0usize;
        let err = render_phrase_stream_with_source_and_cache(
            &input,
            curve_ms,
            model_ms,
            &source,
            None,
            256,
            |_| {
                callbacks += 1;
                if callbacks >= 1 {
                    return Err("cancelled by consumer".to_string());
                }
                Ok(())
            },
        )
        .expect_err("expected cancellation");
        assert_eq!(err, "cancelled by consumer");
        assert_eq!(callbacks, 1);
    }
}
