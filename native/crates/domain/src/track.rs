//! Track model: `UTrack`, `URenderSettings`, `UMixFx`.
//!
//! Mirrors `OpenUtau.Core/Ustx/UTrack.cs` and `UMixFx.cs`. Runtime-only
//! members (loaded `USinger`, `Phonemizer` instances, `VoiceColorExp`
//! subbank resolution) are not modeled; voice-color subbank resolution
//! belongs to the voicebank crate.

use serde::{Deserialize, Serialize};

use crate::expression::UExpressionDescriptor;
use crate::project::UProject;

/// Renderer/resampler/wavtool selection of a track (`URenderSettings`).
/// All fields are optional and omitted when unset, like OpenUtau's null
/// handling.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct URenderSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renderer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resampler: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wavtool: Option<String>,
}

/// Per-track post-processing FX state (`UMixFx`). `None` on the track means
/// no FX configured (bypass), matching OpenUtau's nullable `MixFx`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UMixFx {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_eq_preset")]
    pub eq_preset: String,
    #[serde(default = "default_comp_preset")]
    pub comp_preset: String,
    #[serde(default = "default_reverb_preset")]
    pub reverb_preset: String,
    #[serde(default)]
    pub eq_low_db: f64,
    #[serde(default = "default_eq_mid_freq")]
    pub eq_mid_freq: f64,
    #[serde(default = "default_eq_mid_db")]
    pub eq_mid_db: f64,
    #[serde(default = "default_eq_high_db")]
    pub eq_high_db: f64,
    #[serde(default = "default_comp_threshold_db")]
    pub comp_threshold_db: f64,
    #[serde(default = "default_comp_ratio")]
    pub comp_ratio: f64,
    #[serde(default = "default_comp_makeup_db")]
    pub comp_makeup_db: f64,
    #[serde(default = "default_reverb_size")]
    pub reverb_size: f64,
    #[serde(default = "default_reverb_damp")]
    pub reverb_damp: f64,
    #[serde(default)]
    pub reverb_wet: f64,
    #[serde(default)]
    pub reverb_pre_delay_ms: f64,
}

fn default_eq_preset() -> String {
    "vocal_air".into()
}
fn default_comp_preset() -> String {
    "gentle".into()
}
fn default_reverb_preset() -> String {
    "small_room".into()
}
fn default_eq_mid_freq() -> f64 {
    3000.0
}
fn default_eq_mid_db() -> f64 {
    1.5
}
fn default_eq_high_db() -> f64 {
    3.0
}
fn default_comp_threshold_db() -> f64 {
    -18.0
}
fn default_comp_ratio() -> f64 {
    2.0
}
fn default_comp_makeup_db() -> f64 {
    2.5
}
fn default_reverb_size() -> f64 {
    0.30
}
fn default_reverb_damp() -> f64 {
    0.7
}

impl Default for UMixFx {
    fn default() -> Self {
        UMixFx {
            enabled: false,
            eq_preset: default_eq_preset(),
            comp_preset: default_comp_preset(),
            reverb_preset: default_reverb_preset(),
            eq_low_db: 0.0,
            eq_mid_freq: default_eq_mid_freq(),
            eq_mid_db: default_eq_mid_db(),
            eq_high_db: default_eq_high_db(),
            comp_threshold_db: default_comp_threshold_db(),
            comp_ratio: default_comp_ratio(),
            comp_makeup_db: default_comp_makeup_db(),
            reverb_size: default_reverb_size(),
            reverb_damp: default_reverb_damp(),
            reverb_wet: 1.0,
            reverb_pre_delay_ms: 0.0,
        }
    }
}

/// A track (`UTrack`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UTrack {
    /// Singer id; omitted when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub singer: Option<String>,
    /// Phonemizer type name; omitted when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phonemizer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renderer_settings: Option<URenderSettings>,
    #[serde(default = "default_track_name")]
    pub track_name: String,
    #[serde(default = "default_track_color")]
    pub track_color: String,
    #[serde(default)]
    pub mute: bool,
    #[serde(default)]
    pub solo: bool,
    /// `None` = no FX configured (bypass), like OpenUtau's nullable `MixFx`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mix_fx: Option<UMixFx>,
    #[serde(default)]
    pub volume: f64,
    #[serde(default)]
    pub pan: f64,
    #[serde(default)]
    pub track_expressions: Vec<UExpressionDescriptor>,
    #[serde(default = "default_voice_color_names")]
    pub voice_color_names: Vec<String>,
}

fn default_track_name() -> String {
    "New Track".into()
}
fn default_track_color() -> String {
    "Blue".into()
}
fn default_voice_color_names() -> Vec<String> {
    vec!["".into()]
}

impl Default for UTrack {
    fn default() -> Self {
        UTrack {
            singer: None,
            phonemizer: None,
            renderer_settings: None,
            track_name: default_track_name(),
            track_color: default_track_color(),
            mute: false,
            solo: false,
            mix_fx: None,
            volume: 0.0,
            pan: 0.0,
            track_expressions: Vec::new(),
            voice_color_names: default_voice_color_names(),
        }
    }
}

impl UTrack {
    pub fn new(track_name: impl Into<String>) -> Self {
        UTrack { track_name: track_name.into(), ..Default::default() }
    }

    /// `UTrack(UProject)` constructor: derive the next `TrackN` name from
    /// the existing tracks.
    pub fn new_for_project(project: &UProject) -> Self {
        let mut track_count = 0;
        if !project.tracks.is_empty() {
            track_count = project
                .tracks
                .iter()
                .map(|t| {
                    t.track_name
                        .strip_prefix("Track")
                        .and_then(|s| s.parse::<i32>().ok())
                        .unwrap_or(0)
                })
                .max()
                .unwrap_or(0);
            if project.tracks.len() as i32 > track_count {
                track_count = project.tracks.len() as i32;
            }
        }
        UTrack::new(format!("Track{}", track_count + 1))
    }

    /// `UTrack.TryGetExpDescriptor`: track expressions take precedence over
    /// project expressions. The runtime voice-color (subbank) descriptor is
    /// not modeled here — `clr` resolves from the project registry.
    ///
    /// Returns an owned clone so the result does not borrow both the track
    /// and the project (which would fight the borrow checker in
    /// `UProject::after_load`).
    pub fn try_get_exp_descriptor(
        &self,
        project: &UProject,
        abbr: &str,
    ) -> Option<UExpressionDescriptor> {
        self.track_expressions
            .iter()
            .find(|e| e.abbr == abbr)
            .cloned()
            .or_else(|| project.expressions.get(abbr).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UProject;

    #[test]
    fn track_defaults() {
        let t = UTrack::default();
        assert_eq!(t.track_name, "New Track");
        assert_eq!(t.track_color, "Blue");
        assert_eq!(t.voice_color_names, vec!["".to_string()]);
        assert!(!t.mute && !t.solo);
    }

    #[test]
    fn new_for_project_numbering() {
        let mut p = UProject::new();
        assert_eq!(UTrack::new_for_project(&p).track_name, "Track2");
        p.tracks.push(UTrack::new("Track7"));
        assert_eq!(UTrack::new_for_project(&p).track_name, "Track8");
        p.tracks.push(UTrack::new("Vocals"));
        // highest parsed number (7) still wins over len 3
        assert_eq!(UTrack::new_for_project(&p).track_name, "Track8");
        // with 9 tracks, len beats the parsed max
        for _ in 0..6 {
            p.tracks.push(UTrack::new("x"));
        }
        assert_eq!(UTrack::new_for_project(&p).track_name, "Track10");
    }

    #[test]
    fn track_yaml_keys() {
        let t = UTrack { mute: true, volume: -3.0, ..Default::default() };
        let yaml = serde_yaml::to_string(&t).unwrap();
        assert!(yaml.contains("track_name: New Track"));
        assert!(yaml.contains("track_color: Blue"));
        assert!(yaml.contains("mute: true"));
        assert!(yaml.contains("solo: false"));
        assert!(yaml.contains("volume: -3.0"));
        assert!(yaml.contains("pan: 0.0"));
        assert!(yaml.contains("track_expressions: []"));
        assert!(yaml.contains("voice_color_names:"));
        assert!(!yaml.contains("singer:"));
        let back: UTrack = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(back, t);
    }
}
