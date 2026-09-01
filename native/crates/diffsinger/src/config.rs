//! DiffSinger model configuration — port of the C# `DiffSingerConfig.cs`
//! (`DsConfig`) and `DiffSingerVocoder.cs` (`DsVocoderConfig`).
//!
//! The DiffSinger voicebank layout: `dsconfig.yaml` (acoustic model +
//! phoneme table + mel spec + diffusion flags) and a separate vocoder dir
//! with its own `vocoder.yaml`. Mel specs must MATCH between the two
//! (the C# renderer validates this before running — `InvokeDiffsinger`).

use serde::Deserialize;

/// `dsconfig.yaml` of a DiffSinger acoustic model (C# `DsConfig`).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct DsConfig {
    /// Phoneme table file (default "phonemes.txt"; .json also accepted).
    #[serde(default = "default_phonemes")]
    pub phonemes: String,
    pub languages: Option<String>,
    /// Relative path of the acoustic onnx model.
    pub acoustic: Option<String>,
    /// Vocoder directory (or path) — must be non-empty (C#:141-148).
    pub vocoder: Option<String>,
    #[serde(default)]
    pub speakers: Vec<String>,
    #[serde(default = "default_hidden_size")]
    pub hidden_size: i32,
    #[serde(default)]
    pub use_key_shift_embed: bool,
    #[serde(default)]
    pub use_speed_embed: bool,
    #[serde(default)]
    pub use_energy_embed: bool,
    #[serde(default)]
    pub use_breathiness_embed: bool,
    #[serde(default)]
    pub use_voicing_embed: bool,
    #[serde(default)]
    pub use_tension_embed: bool,
    #[serde(default)]
    pub use_continuous_acceleration: bool,
    #[serde(default)]
    pub use_lang_id: bool,
    #[serde(alias = "use_shallow_diffusion")]
    pub use_variable_depth: Option<bool>,
    #[serde(default)]
    pub max_depth: f64,
    // Variance predictor paths (dspitch / dsvariance subdirs).
    pub dur: Option<String>,
    pub linguistic: Option<String>,
    pub pitch: Option<String>,
    pub variance: Option<String>,
    #[serde(default = "default_true")]
    pub predict_dur: bool,
    #[serde(default = "default_true")]
    pub predict_energy: bool,
    #[serde(default = "default_true")]
    pub predict_breathiness: bool,
    #[serde(default)]
    pub predict_voicing: bool,
    #[serde(default)]
    pub predict_tension: bool,
    #[serde(default)]
    pub use_expr: bool,
    #[serde(default)]
    pub use_note_rest: bool,
    #[serde(default = "default_44100")]
    pub sample_rate: i32,
    #[serde(default = "default_512")]
    pub hop_size: i32,
    #[serde(default = "default_2048")]
    pub win_size: i32,
    #[serde(default = "default_2048")]
    pub fft_size: i32,
    #[serde(default = "default_128")]
    pub num_mel_bins: i32,
    #[serde(default = "default_40")]
    pub mel_fmin: f64,
    #[serde(default = "default_16000")]
    pub mel_fmax: f64,
    #[serde(default = "default_mel_base")]
    pub mel_base: String, // "10" or "e"
    #[serde(default = "default_mel_scale")]
    pub mel_scale: String, // "slaney" or "htk"
}

fn default_phonemes() -> String {
    "phonemes.txt".to_string()
}
fn default_hidden_size() -> i32 {
    256
}
fn default_true() -> bool {
    true
}
fn default_44100() -> i32 {
    44100
}
fn default_512() -> i32 {
    512
}
fn default_2048() -> i32 {
    2048
}
fn default_128() -> i32 {
    128
}
fn default_40() -> f64 {
    40.0
}
fn default_16000() -> f64 {
    16000.0
}
fn default_mel_base() -> String {
    "10".to_string()
}
fn default_mel_scale() -> String {
    "slaney".to_string()
}

impl DsConfig {
    /// `useVariableDepth` — coalesces `use_variable_depth` and the legacy
    /// `use_shallow_diffusion` alias (C# `DsConfig.useVariableDepth`).
    pub fn use_variable_depth(&self) -> bool {
        self.use_variable_depth.unwrap_or(false)
    }

    /// `maxDepth` — the C# divides by 1000 unless continuous acceleration
    /// (C#: `useContinuousAcceleration ? _maxDepth : _maxDepth / 1000.0`).
    pub fn max_depth(&self) -> f64 {
        if self.use_continuous_acceleration {
            self.max_depth
        } else {
            self.max_depth / 1000.0
        }
    }

    /// Frame length in ms (C# `frameMs()`).
    pub fn frame_ms(&self) -> f64 {
        1000.0 * self.hop_size as f64 / self.sample_rate as f64
    }

    /// Validate the keys the renderer requires (C#:141-148).
    pub fn validate(&self) -> Result<(), String> {
        if self.acoustic.as_deref().unwrap_or("").is_empty()
            || self.vocoder.as_deref().unwrap_or("").is_empty()
            || self.phonemes.trim().is_empty()
        {
            return Err(
                "Invalid dsconfig.yaml. Please ensure that dsconfig.yaml contains keys \
                 \"vocoder\", \"acoustic\" and \"phonemes\"."
                    .to_string(),
            );
        }
        Ok(())
    }
}

/// `vocoder.yaml` of a DiffSinger vocoder (C# `DsVocoderConfig`).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct DsVocoderConfig {
    #[serde(default = "default_vocoder_name")]
    pub name: String,
    #[serde(default = "default_model_onnx")]
    pub model: String,
    #[serde(default = "default_44100")]
    pub sample_rate: i32,
    #[serde(default = "default_512")]
    pub hop_size: i32,
    #[serde(default = "default_2048")]
    pub win_size: i32,
    #[serde(default = "default_2048")]
    pub fft_size: i32,
    #[serde(default = "default_128")]
    pub num_mel_bins: i32,
    #[serde(default = "default_40")]
    pub mel_fmin: f64,
    #[serde(default = "default_16000")]
    pub mel_fmax: f64,
    #[serde(default = "default_mel_base")]
    pub mel_base: String,
    #[serde(default = "default_mel_scale")]
    pub mel_scale: String,
    /// Whether the vocoder accepts the shifted f0 as input.
    #[serde(default)]
    pub pitch_controllable: bool,
}

fn default_vocoder_name() -> String {
    "vocoder".to_string()
}
fn default_model_onnx() -> String {
    "model.onnx".to_string()
}

/// Upper bound for `num_mel_bins` (C# `DsVocoderConfig.MaxMelBins`).
pub const MAX_MEL_BINS: i32 = 255;

impl DsVocoderConfig {
    pub fn frame_ms(&self) -> f64 {
        1000.0 * self.hop_size as f64 / self.sample_rate as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dsconfig_yaml() {
        let yaml = r#"
phonemes: phonemes.txt
acoustic: acoustic.onnx
vocoder: nsf-hifigan
hidden_size: 256
use_variable_depth: true
max_depth: 700
num_mel_bins: 128
mel_base: "10"
mel_scale: slaney
"#;
        let cfg: DsConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.phonemes, "phonemes.txt");
        assert_eq!(cfg.acoustic.as_deref(), Some("acoustic.onnx"));
        assert_eq!(cfg.use_variable_depth(), true);
        assert_eq!(cfg.max_depth(), 0.7); // /1000
        assert_eq!(cfg.frame_ms(), 11.609977324263039);
        cfg.validate().unwrap();
    }

    #[test]
    fn validate_rejects_missing_keys() {
        let cfg = DsConfig::default();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn parses_vocoder_yaml() {
        let yaml = r#"
name: vocoder
model: model.onnx
pitch_controllable: true
"#;
        let v: DsVocoderConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(v.model, "model.onnx");
        assert!(v.pitch_controllable);
        assert_eq!(v.num_mel_bins, 128); // defaults
        assert_eq!(v.sample_rate, 44100);
    }

    #[test]
    fn coalesces_shallow_diffusion_alias() {
        let yaml = "use_shallow_diffusion: true\n";
        let cfg: DsConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(cfg.use_variable_depth());
    }
}
