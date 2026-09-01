//! DiffSinger model runner (M4) — port of the C# `DiffSingerRenderer`
//! acoustic + vocoder sessions (C# lines 222-511): acoustic.onnx
//! (tokens/durations/f0 → mel) then vocoder.onnx (mel + f0 → PCM).
//!
//! The mel-spec validation mirrors `InvokeDiffsinger` (C#:151-220): the
//! acoustic and vocoder configs must agree on sample rate, hop/win/fft
//! size, mel bins and fmin/fmax; a mismatching mel base is transformed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ort::session::Session;

use crate::config::{DsConfig, DsVocoderConfig, MAX_MEL_BINS};
use crate::dict::LyricDict;
use crate::dsdur::DsDur;
use crate::phonemes;
use crate::tensors::{apply_tone_shift, DsAcousticInput};

/// A loaded DiffSinger voicebank: dsconfig + phoneme table + sessions.
/// Sessions are `Mutex`-wrapped — the pipeline's `PhraseRenderer` trait
/// renders through `&self`, but ort sessions need `&mut` to run.
pub struct DsModel {
    pub config: DsConfig,
    pub phoneme_table: HashMap<String, i64>,
    pub vocoder_config: DsVocoderConfig,
    /// The bank's lyric dict (dsdict) — drives the C#-identical
    /// tokenization (romaji → phonemes, case kept verbatim).
    pub dict: LyricDict,
    /// The duration predictor (dsdur) when the bank ships one — the
    /// OpenUtau-identical phoneme durations.
    pub dsdur: Option<DsDur>,
    acoustic: Mutex<Session>,
    vocoder: Mutex<Session>,
    /// The speaker embed vector (config.speakers[0] → "<dir>/<name>.emb",
    /// raw f32 × hidden_size — the C# BinaryReader.ReadSingle × hidden).
    spk_embed: Option<Vec<f32>>,
    /// Acoustic model input names (tokens/durations/f0 + optional
    /// speedup/depth/spk_embed/gender/velocity) — decides which extra
    /// diffusion inputs to feed.
    acoustic_inputs: Vec<String>,
}

/// C# `Preferences.Default.DiffSingerSteps` — the default denoise steps.
/// speedup = 1000 / steps (C#:284-288): steps 50 → speedup 20.
const DEFAULT_STEPS: i64 = 50;

fn compute_speedup(steps: i64) -> i64 {
    let mut speedup = (1000 / steps).max(1);
    while 1000 % speedup != 0 && speedup > 1 {
        speedup -= 1;
    }
    speedup
}

fn load_yaml<T: serde::de::DeserializeOwned>(path: &Path, what: &str) -> Result<T, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("read {what} {}: {e}", path.display()))?;
    serde_yaml::from_str(&text).map_err(|e| format!("parse {what} {}: {e}", path.display()))
}

fn open_session(path: &Path, what: &str) -> Result<Session, String> {
    if !path.is_file() {
        return Err(format!("{what} model not found: {}", path.display()));
    }
    let mut builder = Session::builder().map_err(|e| format!("{what} builder: {e}"))?;
    // Android GPU/NPU via the NNAPI EP (CPU fallback per-op — the
    // diffusion models' exotic ops fall back automatically).
    #[cfg(target_os = "android")]
    {
        builder = builder
            .with_execution_providers([ort::execution_providers::NNAPI::default().build()])
            .map_err(|e| format!("{what} NNAPI EP: {e}"))?;
    }
    builder
        .commit_from_file(path)
        .map_err(|e| format!("load {what} {}: {e}", path.display()))
}

/// Mel-spec checks — C#:151-220 (`InvokeDiffsinger` validation block).
fn validate_mel_spec(
    acoustic: &DsConfig,
    vocoder: &DsVocoderConfig,
) -> Result<(), String> {
    for (what, bins) in [
        ("vocoder", vocoder.num_mel_bins),
        ("acoustic model", acoustic.num_mel_bins),
    ] {
        if !(1..=MAX_MEL_BINS).contains(&bins) {
            return Err(format!(
                "{what} num_mel_bins must be between 1 and {MAX_MEL_BINS}, but got {bins}"
            ));
        }
    }
    for (what, base) in [("vocoder", &vocoder.mel_base), ("acoustic model", &acoustic.mel_base)] {
        if base != "10" && base != "e" {
            return Err(format!("Mel base must be \"10\" or \"e\", but got \"{base}\" from {what}"));
        }
    }
    for (what, scale) in [
        ("vocoder", &vocoder.mel_scale),
        ("acoustic model", &acoustic.mel_scale),
    ] {
        if scale != "slaney" && scale != "htk" {
            return Err(format!(
                "Mel scale must be \"slaney\" or \"htk\", but got \"{scale}\" from {what}"
            ));
        }
    }
    let pairs: [(&str, i32, i32); 5] = [
        ("sample rate", vocoder.sample_rate, acoustic.sample_rate),
        ("hop size", vocoder.hop_size, acoustic.hop_size),
        ("win size", vocoder.win_size, acoustic.win_size),
        ("FFT size", vocoder.fft_size, acoustic.fft_size),
        ("mel bins", vocoder.num_mel_bins, acoustic.num_mel_bins),
    ];
    for (name, v, a) in pairs {
        if v != a {
            return Err(format!(
                "Vocoder and acoustic model has mismatching {name} ({v} != {a})"
            ));
        }
    }
    for (name, v, a) in [
        ("fmin", vocoder.mel_fmin, acoustic.mel_fmin),
        ("fmax", vocoder.mel_fmax, acoustic.mel_fmax),
    ] {
        if (v - a).abs() > 1e-5 {
            return Err(format!(
                "Vocoder and acoustic model has mismatching {name} ({v} != {a})"
            ));
        }
    }
    if vocoder.mel_scale != acoustic.mel_scale {
        return Err(format!(
            "Vocoder and acoustic model has mismatching mel scale ({} != {})",
            vocoder.mel_scale, acoustic.mel_scale
        ));
    }
    Ok(())
}

impl DsModel {
    /// Load a DiffSinger voicebank directory:
    /// `dsconfig.yaml` + phonemes table + acoustic onnx + vocoder dir.
    pub fn load(model_dir: &Path) -> Result<Self, String> {
        let config: DsConfig =
            load_yaml(&model_dir.join("dsconfig.yaml"), "dsconfig")?;
        config.validate()?;

        let phoneme_table =
            phonemes::load_phonemes(&model_dir.join(&config.phonemes))?;

        let acoustic_path = model_dir.join(config.acoustic.as_deref().unwrap_or(""));
        let acoustic = open_session(&acoustic_path, "acoustic")?;
        let acoustic_inputs: Vec<String> =
            acoustic.inputs().iter().map(|i| i.name().to_string()).collect();

        let vocoder_dir = model_dir.join(config.vocoder.as_deref().unwrap_or(""));
        let vocoder_config: DsVocoderConfig =
            load_yaml(&vocoder_dir.join("vocoder.yaml"), "vocoder")?;
        let vocoder = open_session(&vocoder_dir.join(&vocoder_config.model), "vocoder")?;

        validate_mel_spec(&config, &vocoder_config)?;

        let dsdur = DsDur::load(model_dir, &config)?;

        // Speaker embed (config.speakers[0] → "<dir>/<name>.emb") — the
        // C# reads hidden_size raw f32s (BinaryReader.ReadSingle).
        let spk_embed = config.speakers.first().map(|speaker| {
            let path = model_dir.join(format!("{speaker}.emb"));
            let bytes = std::fs::read(&path)
                .map_err(|e| format!("speaker embed {}: {e}", path.display()))?;
            let hidden = config.hidden_size.max(1) as usize;
            if bytes.len() < hidden * 4 {
                return Err(format!(
                    "speaker embed {}: {} bytes, expected {}",
                    path.display(),
                    bytes.len(),
                    hidden * 4
                ));
            }
            let mut v = Vec::with_capacity(hidden);
            for i in 0..hidden {
                v.push(f32::from_le_bytes([
                    bytes[i * 4],
                    bytes[i * 4 + 1],
                    bytes[i * 4 + 2],
                    bytes[i * 4 + 3],
                ]));
            }
            Ok(v)
        }).transpose()?;

        let dict = {
            let root = LyricDict::load(model_dir)?;
            if root.vowels().is_empty() && root.lookup("sa").is_none() {
                // Modern banks keep the dicts under dsdur/.
                LyricDict::load(&model_dir.join("dsdur"))?
            } else {
                root
            }
        };

        Ok(DsModel {
            config,
            phoneme_table,
            vocoder_config,
            dict,
            dsdur,
            spk_embed,
            acoustic: Mutex::new(acoustic),
            vocoder: Mutex::new(vocoder),
            acoustic_inputs,
        })
    }

    /// Full render: acoustic (tokens/durations/f0 + diffusion speedup →
    /// mel) → mel base transform → vocoder (mel + original f0 → PCM).
    pub fn render(&self, input: &DsAcousticInput) -> Result<Vec<f32>, String> {
        let total = input.total_frames;

        // --- acoustic ---
        // The C# feeds the SHIFTED f0 to the acoustic model when the
        // vocoder is pitch-controllable (C#:259-265), the original
        // otherwise; the vocoder ALWAYS gets the original f0 (C#:488).
        let acoustic_f0: Vec<f32> = if self.vocoder_config.pitch_controllable {
            apply_tone_shift(&input.f0, &input.tone_shift)
        } else {
            input.f0.clone()
        };
        let tokens_val = ort::value::Tensor::from_array((
            [1usize, input.tokens.len()],
            input.tokens.clone().into_boxed_slice(),
        ))
        .map_err(|e| format!("tokens tensor: {e}"))?;
        let durations_val = ort::value::Tensor::from_array((
            [1usize, input.durations.len()],
            input.durations.clone().into_boxed_slice(),
        ))
        .map_err(|e| format!("durations tensor: {e}"))?;
        let f0_val = ort::value::Tensor::from_array((
            [1usize, total],
            acoustic_f0.into_boxed_slice(),
        ))
        .map_err(|e| format!("f0 tensor: {e}"))?;
        // Diffusion acceleration: the model either takes "speedup"
        // (plain: max(1, 1000/steps), C#:276-291) or "depth" (variable
        // depth). kiritan-family models take "speedup".
        let has_speedup = self.acoustic_inputs.iter().any(|n| n == "speedup");
        let has_depth = self.acoustic_inputs.iter().any(|n| n == "depth");
        let has_spk = self.acoustic_inputs.iter().any(|n| n == "spk_embed");
        let has_gender = self.acoustic_inputs.iter().any(|n| n == "gender");
        let has_velocity = self.acoustic_inputs.iter().any(|n| n == "velocity");
        // Named inputs (the C# NamedOnnxValue) — the model matches by
        // NAME, so the declaration order never matters.
        let mut acoustic_values: Vec<(std::borrow::Cow<'static, str>, ort::session::SessionInputValue)> =
            vec![
                ("tokens".into(), tokens_val.into()),
                ("durations".into(), durations_val.into()),
                ("f0".into(), f0_val.into()),
            ];
        if self.config.use_continuous_acceleration {
            // Continuous-acceleration models take a "steps" scalar
            // (C#:268-274) — the no7-era models; NOT speedup/depth.
            let v = ort::value::Tensor::from_array((
                [1usize],
                vec![DEFAULT_STEPS].into_boxed_slice(),
            ))
            .map_err(|e| format!("steps tensor: {e}"))?;
            acoustic_values.push(("steps".into(), v.into()));
        } else if has_speedup {
            let speedup = compute_speedup(DEFAULT_STEPS);
            let v = ort::value::Tensor::from_array(([1usize], vec![speedup].into_boxed_slice()))
                .map_err(|e| format!("speedup tensor: {e}"))?;
            acoustic_values.push(("speedup".into(), v.into()));
        } else if has_depth {
            let depth = (DEFAULT_STEPS as f64 * 1000.0).round() as i64;
            let v = ort::value::Tensor::from_array(([1usize], vec![depth].into_boxed_slice()))
                .map_err(|e| format!("depth tensor: {e}"))?;
            acoustic_values.push(("depth".into(), v.into()));
        }
        // Speaker embed (C#:306-311): the vector repeated per frame.
        if has_spk {
            let embed = self.spk_embed.as_ref().ok_or_else(|| {
                "acoustic model wants spk_embed but the bank has no speaker embed".to_string()
            })?;
            let hidden = embed.len();
            let mut spk = Vec::with_capacity(total * hidden);
            for _ in 0..total {
                spk.extend_from_slice(embed);
            }
            let v = ort::value::Tensor::from_array((
                [1usize, total, hidden],
                spk.into_boxed_slice(),
            ))
            .map_err(|e| format!("spk_embed tensor: {e}"))?;
            acoustic_values.push(("spk_embed".into(), v.into()));
        }
        // Gender (C#:313-325): the gender curve sampled per frame × the
        // key-shift scale. Our feed does not expose the gender curve yet,
        // so this is the C# default (curve 0 → all-zero tensor). The
        // curve sampling arrives with the variance work (M3).
        if has_gender {
            let gender = vec![0.0f32; total];
            let v = ort::value::Tensor::from_array(([1usize, total], gender.into_boxed_slice()))
                .map_err(|e| format!("gender tensor: {e}"))?;
            acoustic_values.push(("gender".into(), v.into()));
        }
        // Velocity (C#:327-344): the VELC curve (we have none — the
        // C# default Repeat(1f, totalFrames)).
        if has_velocity {
            let velocity = vec![1.0f32; total];
            let v = ort::value::Tensor::from_array(([1usize, total], velocity.into_boxed_slice()))
                .map_err(|e| format!("velocity tensor: {e}"))?;
            acoustic_values.push(("velocity".into(), v.into()));
        }
        let mut acoustic = self
            .acoustic
            .lock()
            .map_err(|e| format!("acoustic lock: {e}"))?;
        let a_out = acoustic
            .run(acoustic_values)
            .map_err(|e| format!("acoustic run: {e}"))?;
        // Output: mel [1, T, C] (first output — C#:463).
        let (mel_shape, mel_slice) = a_out[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("mel extract: {e}"))?;
        let shape: Vec<usize> = mel_shape.iter().map(|&d| d as usize).collect();
        if shape.len() != 3 || shape[0] != 1 {
            return Err(format!(
                "acoustic output shape should be (1, T, C), got {:?}",
                shape
            ));
        }
        let n_frames = shape[1];
        let mel_bins = shape[2];
        let mel_slice: Vec<f32> = mel_slice.to_vec();

        // --- mel base transform (C#:465-483) ---
        let mel = if self.vocoder_config.mel_base != self.config.mel_base {
            let k = match (
                self.vocoder_config.mel_base.as_str(),
                self.config.mel_base.as_str(),
            ) {
                ("e", "10") => 2.30259f32,   // ×ln(10)
                ("10", "e") => 0.434294f32,  // ÷ln(10)
                _ => {
                    return Err("This should never happen".to_string());
                }
            };
            mel_slice.iter().map(|&v| v * k).collect()
        } else {
            mel_slice
        };

        // --- vocoder: mel + ORIGINAL f0 → samples [1, L] ---
        let mel_val = ort::value::Tensor::from_array((
            [1usize, n_frames, mel_bins],
            mel.into_boxed_slice(),
        ))
        .map_err(|e| format!("mel tensor: {e}"))?;
        let f0_orig_val = ort::value::Tensor::from_array((
            [1usize, total],
            input.f0.clone().into_boxed_slice(),
        ))
        .map_err(|e| format!("f0 tensor: {e}"))?;
        let v_inputs = ort::inputs![mel_val, f0_orig_val];
        let mut vocoder = self
            .vocoder
            .lock()
            .map_err(|e| format!("vocoder lock: {e}"))?;
        let v_out = vocoder
            .run(v_inputs)
            .map_err(|e| format!("vocoder run: {e}"))?;
        let (samples_shape, samples_slice) = v_out[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("samples extract: {e}"))?;
        let s_shape: Vec<usize> = samples_shape.iter().map(|&d| d as usize).collect();
        if s_shape.len() != 2 || s_shape[0] != 1 {
            return Err(format!(
                "vocoder output shape should be (1, length), got {:?}",
                s_shape
            ));
        }
        Ok(samples_slice.to_vec())
    }
}

/// Where the model files live, relative to the voicebank dir — mirrors
/// the C# bank layout for tooling that copies banks around.
pub fn acoustic_path(model_dir: &Path, config: &DsConfig) -> PathBuf {
    model_dir.join(config.acoustic.as_deref().unwrap_or(""))
}
