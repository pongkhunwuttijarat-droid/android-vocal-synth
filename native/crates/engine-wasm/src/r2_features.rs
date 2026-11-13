//! R2 feature extraction for the browser engine.
//!
//! The neural ONNX sessions live in the browser's ONNX Runtime Web adapter;
//! this module prepares the same per-phrase WORLD feature tensors on the
//! Rust/WASM side from the already-loaded voicebank samples.

use feed::render_input::RenderInput;
use worldline_plugin::convert::{build_requests, sample_phrase_curves, PhonemeRequest};
use worldline_rs::{MemorySource, SampleSource};

const FS: u32 = 44100;
const HOP: usize = 512;
const FFT_SIZE: usize = 2048;
const FRAME_MS: f64 = HOP as f64 * 1000.0 / FS as f64;
const BINS: usize = FFT_SIZE / 2 + 1;
const F0_FLOOR: f64 = 3.0 * FS as f64 / (FFT_SIZE as f64 - 3.0);

struct Segment {
    f0: Vec<f64>,
    sp: Vec<f64>,
    ap: Vec<f64>,
    skip: i32,
    p0: i32,
    p1: i32,
    p3: i32,
    p4: i32,
}

/// Build R2 input features from the planned chunk's phonemes and in-memory
/// bank samples. Returns a compact wire packet for JS/ORT-WebGPU.
pub fn packet_for_chunk(
    input: &RenderInput,
    source: &MemorySource,
    audio_start_ms: f64,
    start_ms: f64,
    end_ms: f64,
    leading_ms: f64,
    last_chunk: bool,
) -> Result<Vec<u8>, String> {
    let requests = build_requests(input).map_err(|e| format!("R2 build requests: {e}"))?;
    if requests.is_empty() {
        return Err("R2 chunk has no mapped phonemes".to_string());
    }

    let mut segments = Vec::with_capacity(requests.len());
    for request in &requests {
        let sample = source.resolve(&request.wav_path)?;
        if sample.wav.sample_rate != FS {
            return Err(format!(
                "R2 expects 44100 Hz samples, got {} for {}",
                sample.wav.sample_rate, request.phoneme
            ));
        }
        let samples = sample.wav.to_mono();
        segments.push(analyze_segment(request, &samples, sample.frq.as_deref())?);
    }

    let mut f = merge_segments(&segments);
    let curve = sample_phrase_curves(input, worldline_plugin::DEFAULT_FRAME_MS);
    for (i, pitch) in f.0.iter_mut().enumerate() {
        if *pitch > F0_FLOOR {
            if let Some(&override_f0) = curve.f0.get(i) {
                if override_f0 > 0.0 {
                    *pitch = override_f0;
                }
            }
        }
    }

    let frames = f.0.len();
    let mut packet = Vec::with_capacity(44 + frames * (1 + BINS * 2) * 4);
    packet.extend_from_slice(b"R2WF");
    packet.extend_from_slice(&(frames as u32).to_le_bytes());
    packet.extend_from_slice(&(BINS as u32).to_le_bytes());
    for value in [audio_start_ms, start_ms, end_ms, leading_ms] {
        packet.extend_from_slice(&value.to_le_bytes());
    }
    packet.extend_from_slice(&(last_chunk as u32).to_le_bytes());
    for value in &f.0 {
        packet.extend_from_slice(&(*value as f32).to_le_bytes());
    }
    for value in &f.1 {
        packet.extend_from_slice(&(*value as f32).to_le_bytes());
    }
    for value in &f.2 {
        packet.extend_from_slice(&(*value as f32).to_le_bytes());
    }
    Ok(packet)
}

fn analyze_segment(
    request: &PhonemeRequest,
    samples: &[f32],
    frq_bytes: Option<&[u8]>,
) -> Result<Segment, String> {
    if samples.is_empty() {
        return Err(format!("empty wav for {}", request.phoneme));
    }
    let grid_len = (1000.0 * samples.len() as f64 / FS as f64 / FRAME_MS) as usize + 1;
    let parsed_frq = frq_bytes.and_then(|bytes| voicebank::parse_frq(bytes).ok());
    // A valid, empty FRQ still means “unvoiced”; only a missing/invalid file
    // falls back to DIO, matching the native R2 renderer.
    let f0_src = if let Some(frq) = parsed_frq {
        let mut values = vec![0.0; grid_len];
        if !frq.f0.is_empty() && frq.hop_size > 0 {
            let ratio = HOP as f64 / frq.hop_size as f64;
            for (i, dst) in values.iter_mut().enumerate() {
                let lo = ((i as f64 * ratio).floor() as usize).min(frq.f0.len() - 1);
                let hi = (((i as f64 + 1.0) * ratio).ceil() as usize).min(frq.f0.len() - 1);
                let mut sum = 0.0;
                let mut count = 0;
                for &value in &frq.f0[lo..=hi] {
                    if value > F0_FLOOR {
                        sum += value;
                        count += 1;
                    }
                }
                if count > 0 {
                    *dst = sum / count as f64;
                }
            }
        }
        values
    } else {
        let option = world_rs::f0::DioOption {
            frame_period_ms: FRAME_MS,
            ..Default::default()
        };
        let (values, _) = world_rs::f0::dio(samples, FS, option, &())
            .map_err(|e| format!("DIO {}: {e:?}", request.phoneme))?;
        values
    };

    let src_start = ((request.offset / FRAME_MS).max(0.0)) as usize;
    let src_end_ms = if request.cut_off < 0.0 {
        -request.cut_off + request.offset
    } else {
        samples.len() as f64 * 1000.0 / FS as f64 - request.cut_off
    };
    let src_end = ((src_end_ms / FRAME_MS).ceil() as usize).min(f0_src.len());
    if src_end <= src_start {
        return Err(format!("cutoff before offset: {}", request.phoneme));
    }

    let trim_start = src_start.saturating_sub(2);
    let trim_end = (src_end + 2).min(f0_src.len());
    let f0 = &f0_src[trim_start..trim_end];
    let sample_start = trim_start * HOP;
    let sample_end = (trim_end * HOP).min(samples.len());
    let trimmed =
        &samples[sample_start.min(samples.len())..sample_end.max(sample_start.min(samples.len()))];

    let wav_max = samples.iter().fold(0.0f32, |m, s| m.max(s.abs())) as f64;
    let seg_max = trimmed.iter().fold(0.0f32, |m, s| m.max(s.abs())) as f64;
    let voiced = f0.iter().filter(|&&v| v > F0_FLOOR).count() as f64;
    let ratio = voiced / f0.len().max(1) as f64;
    let weight = 1.0 / (1.0 + (5.0 - 10.0 * ratio).exp());
    let max = seg_max * weight + wav_max * (1.0 - weight);
    let gain = if max < 1e-3 {
        1.0
    } else {
        (0.5 / max).powf(0.86)
    };
    let gained: Vec<f32> = trimmed.iter().map(|s| *s * gain as f32).collect();

    let time: Vec<f64> = (0..f0.len())
        .map(|i| i as f64 * FRAME_MS / 1000.0)
        .collect();
    let option = world_rs::cheaptrick::CheapTrickOption {
        q1: -0.15,
        f0_floor: F0_FLOOR,
        fft_size: FFT_SIZE,
    };
    let sp = world_rs::cheaptrick::analyze(&gained, FS, &time, f0, option)
        .map_err(|e| format!("CheapTrick {}: {e:?}", request.phoneme))?;
    let ap = world_rs::d4c::analyze(
        &gained,
        FS,
        &time,
        f0,
        FFT_SIZE,
        world_rs::d4c::D4cOption {
            threshold: 0.0,
            ..Default::default()
        },
        &(),
    )
    .map_err(|e| format!("D4C {}: {e:?}", request.phoneme))?;

    let src_start_rel = (src_start - trim_start) as f64;
    let src_len_ms = (src_end - src_start) as f64 * FRAME_MS;
    let consonant_speed = 2f64.powf(1.0 - request.con_vel / 100.0);
    let src_consonant_ms = request.consonant;
    let src_vowel_ms = src_len_ms - src_consonant_ms;
    let dst_len = (request.required_length / FRAME_MS).ceil() as usize;
    let dst_len_ms = dst_len as f64 * FRAME_MS;
    let dst_consonant_ms = src_consonant_ms / consonant_speed;
    let dst_vowel_ms = (dst_len_ms - dst_consonant_ms).max(0.0);
    let vowel_speed = if dst_vowel_ms > 0.0 {
        src_vowel_ms / dst_vowel_ms
    } else {
        1.0
    };
    let mut f0_dst = Vec::with_capacity(dst_len);
    let mut sp_dst = Vec::with_capacity(dst_len * BINS);
    let mut ap_dst = Vec::with_capacity(dst_len * BINS);
    for i in 0..dst_len {
        let dst_ms = i as f64 * FRAME_MS;
        let src_ms = if dst_ms < dst_consonant_ms {
            dst_ms * consonant_speed
        } else {
            src_consonant_ms + (dst_ms - dst_consonant_ms) * vowel_speed
        };
        let pos = src_ms / FRAME_MS + src_start_rel;
        let idx = pos.floor() as usize;
        let frac = pos - idx as f64;
        let a = idx.min(f0.len() - 1);
        let b = (idx + 1).min(f0.len() - 1);
        f0_dst.push(f0[a] * (1.0 - frac) + f0[b] * frac);
        for k in 0..BINS {
            let x = a * BINS + k;
            let y = b * BINS + k;
            sp_dst.push(sp[x] * (1.0 - frac) + sp[y] * frac);
            ap_dst.push(ap[x] * (1.0 - frac) + ap[y] * frac);
        }
    }

    let skip = (request.skip_ms / FRAME_MS).round() as i32;
    let p0 = (request.pos_ms / FRAME_MS).round() as i32;
    let p1 = ((request.pos_ms + request.fade_in_ms) / FRAME_MS).round() as i32;
    let p3 = ((request.pos_ms + request.length_ms - request.fade_out_ms) / FRAME_MS).round() as i32;
    let p4 = ((request.pos_ms + request.length_ms) / FRAME_MS).round() as i32;
    let p0 = p0.max(0);
    let p1 = (p0 + 1).max(p1);
    let p3 = (p4 - 1).min(p3);
    Ok(Segment {
        f0: f0_dst,
        sp: sp_dst,
        ap: ap_dst,
        skip,
        p0,
        p1,
        p3,
        p4,
    })
}

fn merge_segments(segments: &[Segment]) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let frames = segments.iter().map(|s| s.p4).max().unwrap_or(0).max(0) as usize + 1;
    let mut f0 = vec![0.0; frames];
    let mut sp = vec![1e-12; frames * BINS];
    let mut ap = vec![1.0; frames * BINS];
    let mut dirty = vec![0i32; frames];
    for segment in segments {
        for j in segment.p0.max(0) as usize..(segment.p4.max(0) as usize).min(frames) {
            let weight = if (j as i32) < segment.p1 {
                (j as i32 - segment.p0) as f64 / (segment.p1 - segment.p0).max(1) as f64
            } else if (j as i32) >= segment.p3 {
                (segment.p4 - j as i32) as f64 / (segment.p4 - segment.p3).max(1) as f64
            } else {
                1.0
            };
            let raw_idx = segment.skip + j as i32 - segment.p0;
            if raw_idx < 0 {
                continue;
            }
            let idx = raw_idx as usize;
            if idx >= segment.f0.len() {
                continue;
            }
            if dirty[j] == 0 || weight > 0.5 {
                f0[j] = segment.f0[idx];
            }
            for k in 0..BINS {
                sp[j * BINS + k] += segment.sp[idx * BINS + k] * weight;
                let (wa, wb) = if dirty[j] == 0 {
                    (0.0, 1.0)
                } else {
                    (1.0 - weight, weight)
                };
                ap[j * BINS + k] = ap[j * BINS + k] * wa + segment.ap[idx * BINS + k] * wb;
            }
            dirty[j] = 1;
        }
    }
    (f0, sp, ap)
}
