//! R2 (WORLDLINE-R2) feature path — port of the C# `SynthSegment` /
//! `PhraseSynthV2.SynthFeatures` (Worldline.cs) that feeds the neural
//! vocoder chain (mel.onnx → pc-nsf-hifigan).
//!
//! v1 renders with `PhraseSynth.Synth()` (WORLD resynthesis in C++);
//! v2 instead extracts f0 / spectral envelope / aperiodicity features
//! per phoneme and hands them to ONNX models. This module ports the
//! feature EXTRACTION (analysis + timing + blending); the ONNX runs
//! live in the engine adapter (r2_engine.rs) because the model files
//! are host-provided (bundled with the app, not the plugin).
//!
//! C# reference: core/OpenUtau.Core/Render/Worldline.cs
//!   - `SynthSegment` ctor (gain, F0, WorldAnalysisF0In, stretch)
//!   - `PhraseSynthV2.SynthFeatures()` (blend across p0..p4)

use crate::convert::PhonemeRequest;

/// Per-segment analysis result (mirrors the C# `SynthSegment` fields).
pub struct Segment {
    /// f0 per frame (Hz, 0 = unvoiced).
    pub f0: Vec<f64>,
    /// Spectral envelope per frame (fft_size/2 + 1 bins).
    pub sp_env: Vec<Vec<f64>>,
    /// Aperiodicity per frame.
    pub ap: Vec<Vec<f64>>,
    pub skip_frames: i32,
    pub p0: i32,
    pub p1: i32,
    pub p3: i32,
    pub p4: i32,
}

/// Port of `SynthSegment` ctor: load the wav, estimate f0, run WORLD
/// analysis with that f0, then stretch consonant/vowel to the target
/// duration.
///
/// Returns `None` when the wav cannot be read (caller skips the phone).
#[allow(clippy::too_many_arguments)]
pub fn analyze_segment(
    analysis: &worldline_sys::Analysis,
    request: &PhonemeRequest,
    samples: &[f32],
    fs: i32,
    hop_size: i32,
    fft_size: i32,
) -> Result<Segment, String> {
    let config = analysis.init_config(fs, hop_size, fft_size);
    let frame_ms = config.frame_ms;

    // --- F0 (C# `F0(samples, fs, cfg.frame_ms, hasFrq ? -1 : 0)`) ---
    // The reference ALWAYS calls with method -1 when a .frq exists (even
    // an empty one — unvoiced samples have zero voiced frames, and the
    // C# loop then leaves the buffer at 0). Only samples WITHOUT a frq
    // file fall back to the C++ DIO estimator. Using DIO on unvoiced
    // samples produces false voiced frames, which skews the auto-gain.
    let (has_frq, frq) = load_frq(&request.wav_path);
    let f0_src = if has_frq {
        // C# `F0(..., -1)`: DIO grid size, buffer unfilled.
        let grid = analysis.f0(samples, fs, frame_ms, -1);
        if grid.is_empty() {
            return Err(format!("F0 grid failed for {}", request.phoneme));
        }
        let mut out = vec![0.0f64; grid.len()];
        if let Some(frq) = frq {
            resample_frq_into(&frq, &config, f0_floor(), &mut out);
        }
        out
    } else {
        let est = analysis.f0(samples, fs, frame_ms, 0);
        if est.is_empty() {
            return Err(format!("F0 failed for {}", request.phoneme));
        }
        est
    };

    // --- Trim to the oto region (C# srcStartFrame / srcEndFrame) ---
    let src_start = ((request.offset / frame_ms).max(0.0)) as usize;
    let src_end_ms = if request.cut_off < 0.0 {
        -request.cut_off + request.offset
    } else {
        samples.len() as f64 * 1000.0 / fs as f64 - request.cut_off
    };
    let src_end = ((src_end_ms / frame_ms).ceil() as usize).min(f0_src.len());
    if src_end <= src_start {
        return Err(format!("cutoff before offset: {}", request.phoneme));
    }

    // Trim ±2 frames like C#.
    let trim_start = src_start.saturating_sub(2);
    let trim_end = (src_end + 2).min(f0_src.len());
    let f0_trimmed = &f0_src[trim_start..trim_end];
    let trim_start_sample = trim_start * hop_size as usize;
    let trim_end_sample = (trim_end * hop_size as usize).min(samples.len());
    let samples_trimmed = &samples[trim_start_sample..trim_end_sample];

    // --- Gain (C# gain = volume * 0.01 * autoGain) ---
    // C# computes wavMax on the FULL wav (before trimming, line 530 of
    // Worldline.cs: `samples.Max(s => Math.Abs(s))` with the untrimmed
    // array) — passing the trimmed slice here under-weights it and
    // scales the whole segment wrong.
    let wav_max = samples.iter().fold(0.0f32, |m, s| m.max(s.abs())) as f64;
    // C# Worldline.cs: `gain = item.volume * 0.01` where item.volume =
    // VOL × 100 (0..200). convert.rs now sends the raw VOL (the ×0.44
    // v1 calibration was removed when r1 switched to the C++ PhraseSynth
    // volume semantics), so no undo factor is needed here.
    let gain = request.volume * 0.01 * auto_gain(samples_trimmed, f0_trimmed, wav_max, 86.0, config.f0_floor as f64);
    let samples_gained: Vec<f32> = samples_trimmed.iter().map(|s| s * gain as f32).collect();

    // --- WORLD analysis with F0 in (C# WorldAnalysisF0In) ---
    let sp_size = (fft_size / 2 + 1) as usize;
    let _n_frames = f0_trimmed.len();
    let (sp_env_src, ap_src) =
        analysis.analysis_f0_in(&config, &samples_gained, f0_trimmed, sp_size);
    let sp_env_src = sp_env_src; // row-major n_frames × sp_size
    let ap_src = ap_src;

    // --- Time-stretch consonant/vowel (C# tDst) ---
    // C# (Worldline.cs 558-584): tSrc spans `srcEndFrame - srcStartFrame`
    // AFTER both were shifted by -trimStartFrame (lines 534-535), so
    // `srcStartFrame` here is an index INTO the trimmed arrays. Using the
    // absolute src_start (or the trimmed length) shifts the stretch by
    // `trim_start` frames — the tail then clamps into the next phoneme's
    // frames (e.g. "s" grabbing the "3" vowel's f0 → voiced 44/44).
    let src_start_rel = (src_start - trim_start) as f64;
    let src_len_ms = (src_end - src_start) as f64 * frame_ms;
    let consonant_speed = 2f64.powf(1.0 - request.con_vel / 100.0);
    let src_consonant_ms = request.consonant;
    let src_vowel_ms = src_len_ms - src_consonant_ms;
    // C# ResamplerItem.cs:63: `durRequired = Math.Ceiling(...)` rounds
    // duration+skipUp to the next 50 ms — convert.rs already did that
    // (required_length = 550 for the 500 ms "s" phone), so tDst spans
    // ceil(durRequired/frame_ms) = 48 frames. Do NOT round again.
    let dst_len = (request.required_length / frame_ms).ceil() as usize;
    let dst_len_ms = dst_len as f64 * frame_ms;
    let dst_consonant_ms = src_consonant_ms / consonant_speed;
    let dst_vowel_ms = (dst_len_ms - dst_consonant_ms).max(0.0);
    let vowel_speed = if dst_vowel_ms > 0.0 {
        src_vowel_ms / dst_vowel_ms
    } else {
        1.0
    };
    let src_start_f = src_start_rel;
    if std::env::var_os("WORLD_DBG_STRETCH").is_some() {
        eprintln!("[stretch-dbg] {} src_start={} trim=[{},{}] rel={} dst_len={} src_len_ms={} consonant={}",
            request.phoneme, src_start, trim_start, trim_end, src_start_rel, dst_len, src_len_ms, src_consonant_ms);
    }

    let mut f0_dst = Vec::with_capacity(dst_len);
    let mut sp_dst = Vec::with_capacity(dst_len);
    let mut ap_dst = Vec::with_capacity(dst_len);
    for i in 0..dst_len {
        let dst_ms = i as f64 * frame_ms;
        let src_ms = if dst_ms < dst_consonant_ms {
            dst_ms * consonant_speed
        } else {
            src_consonant_ms + (dst_ms - dst_consonant_ms) * vowel_speed
        };
        let pos = src_ms / frame_ms + src_start_f;
        let idx = pos.floor() as usize;
        let frac = pos - idx as f64;
        if idx + 1 < f0_trimmed.len() {
            f0_dst.push(
                f0_trimmed[idx] * (1.0 - frac) + f0_trimmed[idx + 1] * frac,
            );
            for k in 0..sp_size {
                sp_dst.push(sp_env_src[idx * sp_size + k] * (1.0 - frac)
                    + sp_env_src[(idx + 1) * sp_size + k] * frac);
                ap_dst.push(ap_src[idx * sp_size + k] * (1.0 - frac)
                    + ap_src[(idx + 1) * sp_size + k] * frac);
            }
        } else {
            let idx = idx.min(f0_trimmed.len() - 1);
            f0_dst.push(f0_trimmed[idx]);
            for k in 0..sp_size {
                sp_dst.push(sp_env_src[idx * sp_size + k]);
                ap_dst.push(ap_src[idx * sp_size + k]);
            }
        }
    }

    // --- Segment timing (C# skipFrames / p0..p4) ---
    let skip_frames = (request.skip_ms / frame_ms).round() as i32;
    let p0 = (request.pos_ms / frame_ms).round() as i32;
    let p1 = ((request.pos_ms + request.fade_in_ms) / frame_ms).round() as i32;
    let p3 = ((request.pos_ms + request.length_ms - request.fade_out_ms) / frame_ms).round() as i32;
    let p4 = ((request.pos_ms + request.length_ms) / frame_ms).round() as i32;
    let p0 = p0.max(0);
    let p1 = (p0 + 1).max(p1);
    let p3 = (p4 - 1).min(p3);

    let sp: Vec<Vec<f64>> = sp_dst.chunks(sp_size).map(|c| c.to_vec()).collect();
    let ap: Vec<Vec<f64>> = ap_dst.chunks(sp_size).map(|c| c.to_vec()).collect();
    if std::env::var_os("WORLD_DBG_STRETCH").is_some() && request.phoneme == "s" {
        let f0v: Vec<String> = f0_trimmed.iter().map(|f| format!("{:.1}", f)).collect();
        eprintln!("[seg-dbg] vol={} skip_ms={:.3} f0_trimmed={} gain={:.4} sp_env_src[2][0]={:.6} seg0[0]={:.6}",
            request.volume, request.skip_ms, f0v.join(","), gain, sp_env_src[2 * sp_size], sp[0][0]);
    }

    Ok(Segment {
        f0: f0_dst,
        sp_env: sp,
        ap,
        skip_frames,
        p0,
        p1,
        p3,
        p4,
    })
}

/// C# `GetAutoGain`: logistic blend of segment max and wav max.
fn auto_gain(samples: &[f32], f0: &[f64], wav_max: f64, peak_comp: f64, f0_floor: f64) -> f64 {
    let seg_max = samples.iter().fold(0.0f32, |m, s| m.max(s.abs())) as f64;
    let voiced = f0.iter().filter(|&&f| f > f0_floor).count() as f64;
    let ratio = voiced / f0.len().max(1) as f64;
    // C# GetAutoGain: `1/(1 + Math.Exp(5 - 10*ratio))` — NOT negated.
    // `(-(5-10r)).exp()` = Exp(10r-5) flips the logistic: for s (r≈0.19)
    // C# weight ≈ 0.042 (max ≈ wavMax, autoGain ≈ 0.56) but the flipped
    // form gave 0.958 (max ≈ segMax → autoGain 1.04) — the s gain 1.86×.
    let weight = 1.0 / (1.0 + (5.0 - 10.0 * ratio).exp());
    let max = seg_max * weight + wav_max * (1.0 - weight);
    if max < 1e-3 {
        1.0
    } else {
        (0.5 / max).powf(peak_comp * 0.01)
    }
}

/// Port of `PhraseSynthV2.SynthFeatures()`: blend all segments into
/// whole-phrase f0 / sp_env / ap arrays (weighted by p0..p4).
pub fn synth_features(segments: &[Segment], fft_size: i32) -> Features {
    let sp_size = (fft_size / 2 + 1) as usize;
    let total_frames = segments.iter().map(|s| s.p4).max().unwrap_or(0) as usize + 1;
    let mut f0_out = vec![0.0f64; total_frames];
    let mut sp_out = vec![1e-12f64; total_frames * sp_size];
    let mut ap_out = vec![1.0f64; total_frames * sp_size];
    let mut dirty = vec![0i32; total_frames];

    for seg in segments {
        for j in (seg.p0.max(0) as usize)..(seg.p4 as usize).min(total_frames) {
            let weight = if (j as i32) < seg.p1 {
                (j as i32 - seg.p0) as f64 / (seg.p1 - seg.p0).max(1) as f64
            } else if (j as i32) >= seg.p3 {
                (seg.p4 - j as i32) as f64 / (seg.p4 - seg.p3).max(1) as f64
            } else {
                1.0
            };
            let seg_idx = (seg.skip_frames + j as i32 - seg.p0) as usize;
            if seg_idx >= seg.f0.len() {
                continue;
            }
            if dirty[j] == 0 || weight > 0.5 {
                f0_out[j] = seg.f0[seg_idx];
            }
            for k in 0..sp_size {
                sp_out[j * sp_size + k] += seg.sp_env[seg_idx][k] * weight;
                let wa = if dirty[j] == 0 { 0.0 } else { 1.0 - weight };
                let wb = if dirty[j] == 0 { 1.0 } else { weight };
                ap_out[j * sp_size + k] =
                    ap_out[j * sp_size + k] * wa + seg.ap[seg_idx][k] * wb;
            }
            dirty[j] = 1;
        }
    }

    Features {
        f0: f0_out,
        sp_env: sp_out,
        ap: ap_out,
        sp_size,
    }
}

/// Whole-phrase features (the mel model inputs).
pub struct Features {
    pub f0: Vec<f64>,
    pub sp_env: Vec<f64>,
    pub ap: Vec<f64>,
    pub sp_size: usize,
}

/// A parsed .frq file (FREQ0003) — the voicebank's own pitch data.
struct FrqData {
    hop_size: i32,
    f0: Vec<f64>,
}

/// Load `<wav>_wav.frq` next to the sample wav — reuses the voicebank
/// crate's canonical `frq_path_for_wav` (C# `VoicebankFiles.GetFrqFile`)
/// AND `parse_frq` (FREQ0003: header, hop, avg, 16 blank bytes, len@36).
/// Returns `(has_frq_file, f0_data)` — an empty frq (unvoiced sample)
/// still means "use the frq path": the caller zero-fills the grid,
/// exactly like the C# loop leaving the buffer at 0.
fn load_frq(wav_path: &str) -> (bool, Option<FrqData>) {
    let path = std::path::Path::new(wav_path);
    let frq = voicebank::frq_path_for_wav(path);
    let Some(bytes) = std::fs::read(&frq).ok() else {
        return (false, None); // no frq file → caller falls back to DIO
    };
    let Ok(parsed) = voicebank::parse_frq(&bytes) else {
        return (false, None);
    };
    if parsed.f0.is_empty() {
        return (true, None);
    }
    (
        true,
        Some(FrqData {
            hop_size: parsed.hop_size,
            f0: parsed.f0,
        }),
    )
}

/// C# `F0(..., -1)` + the frq resample loop (Worldline.cs 495-517):
/// `ratio = config.hop_size / frq.hopSize`; each target frame averages
/// the source frames whose f0 > f0_floor. Writes into `out` (the DIO
/// grid, pre-zeroed — unvoiced regions stay 0 exactly like the C# loop
/// leaving the buffer at 0).
fn resample_frq_into(frq: &FrqData, config: &worldline_sys::AnalysisConfig, f0_floor: f64, out: &mut [f64]) {
    let ratio = config.hop_size as f64 / frq.hop_size as f64;
    for i in 0..out.len() {
        let i_f = i as f64;
        let index0 = ((i_f * ratio).floor() as usize).min(frq.f0.len() - 1);
        let index1 = (((i_f + 1.0) * ratio).ceil() as usize).min(frq.f0.len() - 1);
        let mut sum = 0.0;
        let mut count = 0;
        for j in index0..=index1 {
            if frq.f0[j] > f0_floor {
                sum += frq.f0[j];
                count += 1;
            }
        }
        if count > 0 {
            out[i] = sum / count as f64;
        } else {
            out[i] = 0.0;
        }
    }
}

/// The C# `config.f0_floor` = GetF0FloorForCheapTrick(44100, 2048) =
/// 3·fs/(fft−3) = 64.69 Hz (NOT 40 — that was a stale guess; the C++
/// InitAnalysisConfig sets it from the FFT size).
fn f0_floor() -> f64 {
    3.0 * 44100.0 / (2048.0 - 3.0)
}
