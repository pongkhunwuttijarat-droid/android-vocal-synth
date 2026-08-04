//! Part model: `UPart`, `UVoicePart`, `UWavePart`.
//!
//! Mirrors `OpenUtau.Core/Ustx/UPart.cs`. `UVoicePart` holds notes and
//! curves; `UWavePart` is modeled for `.ustx` round-trip fidelity (its
//! audio-loading behavior is out of scope for this crate). Parts are
//! serialized through `UProject.voice_parts` / `UProject.wave_parts`
//! (`UProject.parts` is `[YamlIgnore]` in OpenUtau).

use serde::{Deserialize, Serialize};

use crate::curve::UCurve;
use crate::note::UNote;
use crate::time_axis::TimeAxis;

/// Base part data shared by voice and wave parts (OpenUtau `UPart` fields).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PartBase {
    pub name: String,
    pub comment: String,
    pub track_no: i32,
    pub position: i32,
}

impl PartBase {
    pub fn new(name: impl Into<String>) -> Self {
        PartBase { name: name.into(), ..Default::default() }
    }
}

/// A voice part (`UVoicePart`) holding notes and expression curves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UVoicePart {
    #[serde(default = "default_part_name")]
    pub name: String,
    #[serde(default)]
    pub comment: String,
    #[serde(default)]
    pub track_no: i32,
    #[serde(default)]
    pub position: i32,
    #[serde(default)]
    pub duration: i32,
    #[serde(default)]
    pub notes: Vec<UNote>,
    #[serde(default)]
    pub curves: Vec<UCurve>,
}

fn default_part_name() -> String {
    "New Part".into()
}

impl Default for UVoicePart {
    fn default() -> Self {
        UVoicePart {
            name: default_part_name(),
            comment: String::new(),
            track_no: 0,
            position: 0,
            duration: 0,
            notes: Vec::new(),
            curves: Vec::new(),
        }
    }
}

impl UVoicePart {
    pub fn new(name: impl Into<String>) -> Self {
        UVoicePart { name: name.into(), ..Default::default() }
    }

    /// Part end tick (`UPart.End`).
    pub fn end(&self) -> i32 {
        self.position + self.duration
    }

    /// `UVoicePart.GetMinDurTick`: the tick of the next bar beat after the
    /// last note ends (or after `position + 1` when there are no notes),
    /// relative to the part position.
    pub fn get_min_dur_tick(&self, axis: &TimeAxis) -> i32 {
        let end_ticks = self.position + self.notes.last().map(UNote::end).unwrap_or(1);
        let (bar, beat, _) = axis.tick_to_bar_beat(end_ticks);
        axis.bar_beat_to_tick(bar, beat + 1) - self.position
    }
}

/// A wave (audio) part (`UWavePart`). Only serialized fields are modeled;
/// audio loading/peaks are out of scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UWavePart {
    #[serde(default = "default_part_name")]
    pub name: String,
    #[serde(default)]
    pub comment: String,
    #[serde(default)]
    pub track_no: i32,
    #[serde(default)]
    pub position: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relative_path: Option<String>,
    #[serde(default)]
    pub file_duration_ms: f64,
    #[serde(default)]
    pub skip: i32,
    #[serde(default)]
    pub trim: i32,
    #[serde(default)]
    pub fadein: i32,
    #[serde(default)]
    pub fadeout: i32,
}

impl Default for UWavePart {
    fn default() -> Self {
        UWavePart {
            name: default_part_name(),
            comment: String::new(),
            track_no: 0,
            position: 0,
            relative_path: None,
            file_duration_ms: 0.0,
            skip: 0,
            trim: 0,
            fadein: 0,
            fadeout: 0,
        }
    }
}

impl UWavePart {
    pub fn new(name: impl Into<String>) -> Self {
        UWavePart { name: name.into(), ..Default::default() }
    }

    /// Part end tick (`UPart.End`); wave part duration is computed from the
    /// audio file at load time, so this returns `position` until a render
    /// pipeline fills it in.
    pub fn end(&self) -> i32 {
        self.position
    }
}

/// A part in a project's in-memory part list (`UProject.parts`).
#[derive(Debug, Clone, PartialEq)]
pub enum UPart {
    Voice(UVoicePart),
    Wave(UWavePart),
}

impl UPart {
    pub fn track_no(&self) -> i32 {
        match self {
            UPart::Voice(p) => p.track_no,
            UPart::Wave(p) => p.track_no,
        }
    }

    pub fn position(&self) -> i32 {
        match self {
            UPart::Voice(p) => p.position,
            UPart::Wave(p) => p.position,
        }
    }

    pub fn end(&self) -> i32 {
        match self {
            UPart::Voice(p) => p.end(),
            UPart::Wave(p) => p.end(),
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            UPart::Voice(p) => &p.name,
            UPart::Wave(p) => &p.name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_part_defaults_and_yaml_keys() {
        let p = UVoicePart::new("My Part");
        assert_eq!(p.name, "My Part");
        let yaml = serde_yaml::to_string(&p).unwrap();
        assert!(yaml.contains("track_no: 0"));
        assert!(yaml.contains("position: 0"));
        assert!(yaml.contains("duration: 0"));
        assert!(yaml.contains("notes: []"));
        assert!(yaml.contains("curves: []"));
        let back: UVoicePart = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn wave_part_yaml_keys() {
        let p = UWavePart {
            relative_path: Some("audio/voice.wav".into()),
            file_duration_ms: 2500.5,
            skip: 10,
            trim: 20,
            fadein: 30,
            fadeout: 40,
            ..Default::default()
        };
        let yaml = serde_yaml::to_string(&p).unwrap();
        assert!(yaml.contains("relative_path: audio/voice.wav"));
        assert!(yaml.contains("file_duration_ms: 2500.5"));
        assert!(yaml.contains("skip: 10"));
        assert!(yaml.contains("trim: 20"));
        assert!(yaml.contains("fadein: 30"));
        assert!(yaml.contains("fadeout: 40"));
        let back: UWavePart = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(back, p);
    }
}
