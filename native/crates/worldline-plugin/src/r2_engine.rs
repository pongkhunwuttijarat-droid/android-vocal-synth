//! R2 (WORLDLINE-R2) neural engine — port of the C# v2 render path
//! (WorldlineRenderer.cs, `version == 2`): SynthFeatures → mel.onnx →
//! pc-nsf-hifigan vocoder → PCM.
//!
//! The ONNX models are HOST-provided (paths passed in — the app bundles
//! them; the plugin never embeds multi-MB models).

use std::path::Path;

use crate::convert::{build_requests, sample_phrase_curves};
use crate::r2::{self, Features, Segment};
use feed::render_input::RenderInput;

use std::cell::RefCell;
use std::path::PathBuf;

thread_local! {
    /// Per-thread mel dump path (parallel extraction). Falls back to the
    /// WORLD_DUMP_MEL_FILE env var when unset.
    static THREAD_DUMP: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Set (or clear) this thread's mel-dump path — used by parallel
/// extractors where each worker writes its own dump file.
pub fn set_thread_dump_path(path: Option<PathBuf>) {
    THREAD_DUMP.with(|t| *t.borrow_mut() = path);
}

/// Neural engine error.
#[derive(Debug)]
pub enum R2Error {
    MissingModel(String),
    Onnx(String),
    Analysis(String),
    Empty,
}

impl std::fmt::Display for R2Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            R2Error::MissingModel(p) => write!(f, "R2 model missing: {p}"),
            R2Error::Onnx(e) => write!(f, "R2 onnx: {e}"),
            R2Error::Analysis(e) => write!(f, "R2 analysis: {e}"),
            R2Error::Empty => write!(f, "R2 produced no samples"),
        }
    }
}

/// The two ONNX models the R2 chain needs.
pub struct R2Models {
    pub mel_path: std::path::PathBuf,
    pub vocoder_path: std::path::PathBuf,
}

/// R2 neural engine: `RenderInput` → features → mel → HiFiGAN → PCM.
pub struct R2Engine {
    models: R2Models,
    /// dlopen'd libworldline.so analysis functions (F0 / WORLD).
    analysis: worldline_sys::Analysis,
}

impl R2Engine {
    pub fn new(models: R2Models, worldline_so: &Path) -> Result<Self, R2Error> {
        let analysis = worldline_sys::Analysis::open(worldline_so)
            .map_err(|e| R2Error::Analysis(format!("open analysis api: {e}")))?;
        Ok(R2Engine { models, analysis })
    }

    /// Render one phrase. `so_path` is unused by the neural path (no
    /// PhraseSynth) but kept for the trait-shaped API.
    #[allow(clippy::too_many_arguments)]
    pub fn render_phrase(
        &self,
        input: &RenderInput,
        _sample_rate: u32,
        fft_size: i32,
        hop_size: i32,
        fs: i32,
    ) -> Result<Vec<f32>, R2Error> {
        // --- 1. Per-phoneme feature extraction (SynthSegment port) ---
        let requests = build_requests(input).map_err(|e| R2Error::Analysis(e.to_string()))?;
        if requests.is_empty() {
            return Err(R2Error::Empty);
        }
        let mut segments: Vec<Segment> = Vec::with_capacity(requests.len());
        for request in &requests {
            let samples = load_wav(&request.wav_path).ok_or_else(|| {
                R2Error::Analysis(format!("cannot read {}", request.wav_path))
            })?;
            let seg = r2::analyze_segment(
                &self.analysis,
                request,
                &samples,
                fs,
                hop_size,
                fft_size,
            )
            .map_err(R2Error::Analysis)?;
            segments.push(seg);
        }
        let features: Features = r2::synth_features(&segments, fft_size);

        // --- 2. f0/expression curve override (SetCurves equivalent) ---
        // C# SynthFeatures: `if (f0Out[i] > config.f0_floor) f0Out[i] =
        // f0Curve[i]` — floor = GetF0FloorForCheapTrick(44100, 2048) =
        // 64.69, NOT the old 40.0 guess.
        let mut features = features;
        let f0_floor = 3.0 * fs as f64 / (fft_size as f64 - 3.0);
        let curve = sample_phrase_curves(input, 11.6);
        for (i, f) in features.f0.iter_mut().enumerate() {
            if *f > f0_floor {
                if let Some(c) = curve.f0.get(i) {
                    if *c > 0.0 {
                        *f = *c;
                    }
                }
            }
        }

        if std::env::var_os("WORLD_DUMP_F0").is_some() {
            let f0s: Vec<String> = features
                .f0
                .iter()
                .map(|f| format!("{:.1}", f))
                .collect();
            eprintln!("[r2-f0] {}", f0s.join(","));
        }

        // --- 3. mel.onnx: f0 + sp_env + ap → mel spectrogram ---
        // C# pads to a 16-multiple length (left 4 frames) before the mel
        // model, then trims the extra hop frames and eases the edges
        // after the vocoder (WorldlineRenderer.cs v2 path).
        let (features_padded, pad_left) = pad_features(&features);
        let mel = run_mel(&self.models.mel_path, &features_padded).map_err(R2Error::Onnx)?;
        if std::env::var_os("WORLD_DUMP_MEL").is_some() {
            let total = features.f0.len();
            let sp = features.sp_size;
            let mut powers = Vec::new();
            for f in 0..total {
                let m = &mel[(pad_left + f) * 128..(pad_left + f + 1) * 128];
                powers.push(m.iter().map(|x| x * x).sum::<f32>() / 128.0);
            }
            eprintln!(
                "[mel-dump] total={total} pad_left={pad_left} sp_size={sp} \
                 mean={:.6} max={:.6} first10={:?}",
                powers.iter().sum::<f32>() / total as f32,
                powers.iter().fold(0.0f32, |m, p| m.max(*p)),
                &powers[..10.min(powers.len())],
            );
        }

        // Env-gated TRAINING dump (train-prep distillation teacher):
        // writes [u32 total_frames][f32 f0 × total][f32 mel × total × 128]
        // (post-pad-trim, score-overridden f0 — exactly what the student
        // must learn). Path resolution: thread-local first (parallel
        // extraction needs per-thread paths — env vars are process-global
        // and would collide), then the WORLD_DUMP_MEL_FILE env var.
        // Side-effect only when a path is set; the golden path is untouched.
        let dump_path = THREAD_DUMP
            .with(|t| t.borrow().clone())
            .or_else(|| std::env::var_os("WORLD_DUMP_MEL_FILE").map(std::path::PathBuf::from));
        if let Some(path) = dump_path {
            let total = features.f0.len();
            let mut buf: Vec<u8> = Vec::with_capacity(4 + total * 4 + total * 128 * 4);
            buf.extend_from_slice(&(total as u32).to_le_bytes());
            for f in 0..total {
                buf.extend_from_slice(&(features.f0[f] as f32).to_le_bytes());
            }
            for f in 0..total {
                let m = &mel[(pad_left + f) * 128..(pad_left + f + 1) * 128];
                for v in m {
                    buf.extend_from_slice(&v.to_le_bytes());
                }
            }
            if let Err(e) = std::fs::write(&path, &buf) {
                eprintln!("[r2-mel-dump] write {}: {e}", path.to_string_lossy());
            }
        }

        // --- 4. pc-nsf-hifigan: mel + f0 → audio ---
        let mut audio =
            run_vocoder(&self.models.vocoder_path, &mel, &features_padded.f0)
                .map_err(R2Error::Onnx)?;

        // --- 5. Trim the pad, then ease in/out like the reference ---
        let hop = hop_size as usize;
        let total_frames = features.f0.len();
        if audio.len() > pad_left * hop {
            audio.drain(0..pad_left * hop);
        }
        audio.truncate(total_frames * hop);
        ease(&mut audio, 512);
        Ok(audio)
    }
}

/// C# padding: `paddedLength = ceil((totalFrames + 4) / 16) * 16`,
/// left pad 4 frames (f0/sp_env zero, ap one). Returns the padded
/// features and the left pad in frames.
fn pad_features(features: &Features) -> (Features, usize) {
    let total = features.f0.len();
    let padded = ((total + 4 + 15) / 16) * 16;
    let left = 4usize;
    let right = padded - total - left;
    let sp = features.sp_size;
    let mut f0 = vec![0.0f64; padded];
    let mut sp_env = vec![0.0f64; padded * sp];
    let mut ap = vec![1.0f64; padded * sp];
    f0[left..left + total].copy_from_slice(&features.f0);
    for i in 0..total {
        let dst = (left + i) * sp;
        let src = i * sp;
        sp_env[dst..dst + sp].copy_from_slice(&features.sp_env[src..src + sp]);
        ap[dst..dst + sp].copy_from_slice(&features.ap[src..src + sp]);
    }
    let _ = right;
    (
        Features {
            f0,
            sp_env,
            ap,
            sp_size: sp,
        },
        left,
    )
}

/// C# post-vocoder ease: linear ramp over `n` samples at both edges.
fn ease(samples: &mut [f32], n: usize) {
    let n = n.min(samples.len());
    for i in 0..n {
        let g = i as f64 / n as f64;
        samples[i] = (samples[i] as f64 * g) as f32;
    }
    let len = samples.len();
    for i in 0..n {
        let g = (n - i) as f64 / n as f64;
        samples[len - n + i] = (samples[len - n + i] as f64 * g) as f32;
    }
}

fn load_wav(path: &str) -> Option<Vec<f32>> {
    let wav = voicebank::read_wav(Path::new(path)).ok()?;
    Some(wav.to_mono().into_iter().map(|s| s as f32).collect())
}

/// mel.onnx: [1, N, sp_size] sp_env + [1, N] f0 + [1, N, sp_size] ap →
/// [1, N, 128] mel.
fn run_mel(mel_path: &Path, features: &Features) -> Result<Vec<f32>, String> {
    if !mel_path.is_file() {
        return Err(format!("mel model not found: {}", mel_path.display()));
    }
    let n = features.f0.len();
    let sp = features.sp_size;
    let mut session = ort::session::Session::builder()
        .and_then(|mut b| b.commit_from_file(mel_path))
        .map_err(|e| format!("load mel: {e}"))?;

    // Inputs: f0 [1,N] f32, sp_env [1,N,sp] f32, ap [1,N,sp] f32.
    let f0_f: Vec<f32> = features.f0.iter().map(|&v| v as f32).collect();
    let sp_f: Vec<f32> = features.sp_env.iter().map(|&v| v as f32).collect();
    let ap_f: Vec<f32> = features.ap.iter().map(|&v| v as f32).collect();
    let sp_arr = ndarray::Array3::from_shape_vec((1, n, sp), sp_f)
        .map_err(|e| format!("sp shape: {e}"))?;
    let f0_arr = ndarray::Array2::from_shape_vec((1, n), f0_f)
        .map_err(|e| format!("f0 shape: {e}"))?;
    let ap_arr = ndarray::Array3::from_shape_vec((1, n, sp), ap_f)
        .map_err(|e| format!("ap shape: {e}"))?;
    let inputs = ort::inputs![
        ort::value::Value::from_array(f0_arr).map_err(|e| format!("f0 tensor: {e}"))?,
        ort::value::Value::from_array(sp_arr).map_err(|e| format!("sp tensor: {e}"))?,
        ort::value::Value::from_array(ap_arr).map_err(|e| format!("ap tensor: {e}"))?,
    ];
    let out = session
        .run(inputs)
        .map_err(|e| format!("mel run: {e}"))?;
    // Output "mel": [1, N, 128].
    let (_, mel_slice) = out[0]
        .try_extract_tensor::<f32>()
        .map_err(|e| format!("mel extract: {e}"))?;
    Ok(mel_slice.to_vec())
}

/// pc-nsf-hifigan: mel [1,N,128] + f0 [1,N] → audio [1, N*hop].
fn run_vocoder(vocoder_path: &Path, mel: &[f32], f0: &[f64]) -> Result<Vec<f32>, String> {
    if !vocoder_path.is_file() {
        return Err(format!("vocoder model not found: {}", vocoder_path.display()));
    }
    let n_mel = mel.len() / 128;
    let mut session = ort::session::Session::builder()
        .and_then(|mut b| b.commit_from_file(vocoder_path))
        .map_err(|e| format!("load vocoder: {e}"))?;
    let mel_f = mel.to_vec();
    let f0_f: Vec<f32> = f0.iter().map(|&v| v as f32).collect();
    let mel_arr = ndarray::Array3::from_shape_vec((1, n_mel, 128), mel_f)
        .map_err(|e| format!("mel shape: {e}"))?;
    let f0_arr = ndarray::Array2::from_shape_vec((1, n_mel), f0_f)
        .map_err(|e| format!("f0 shape: {e}"))?;
    let inputs = ort::inputs![
        ort::value::Value::from_array(mel_arr).map_err(|e| format!("mel tensor: {e}"))?,
        ort::value::Value::from_array(f0_arr).map_err(|e| format!("f0 tensor: {e}"))?,
    ];
    let out = session
        .run(inputs)
        .map_err(|e| format!("vocoder run: {e}"))?;
    let (_, audio) = out[0]
        .try_extract_tensor::<f32>()
        .map_err(|e| format!("audio extract: {e}"))?;
    Ok(audio.to_vec())
}
