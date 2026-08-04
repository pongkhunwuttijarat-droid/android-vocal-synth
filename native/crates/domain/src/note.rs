//! Note model: `UNote`, `UPitch`, `PitchPoint`, `UVibrato`, `UPhonemeOverride`.
//!
//! Mirrors `OpenUtau.Core/Ustx/UNote.cs`. Serialized YAML keys match the
//! OpenUtau `UstxYamlTest` expectations exactly:
//!
//! ```yaml
//! position: 120
//! duration: 60
//! tone: 42
//! lyric: あ
//! pitch:
//!   data:
//!   - {x: -5, y: 0, shape: io}
//!   snap_first: true
//! vibrato: {length: 0, period: 175, depth: 25, in: 10, out: 10, shift: 0, drift: 0, vol_link: 0}
//! tuning: 0
//! phoneme_expressions:
//! - {index: 0, abbr: vel, value: 123}
//! phoneme_overrides: []
//! ```
//!
//! Notes do not carry a `tone_shift`/`flags` field in the OpenUtau format —
//! tone shift is the `shft` expression and flags are derived per-phoneme
//! (see [`crate::phoneme::UPhoneme`]).

use serde::{Deserialize, Serialize};

/// Interpolation shape of a pitch point (`PitchPointShape`). Written to
/// YAML as the lowercase C# enum name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PitchPointShape {
    /// Sine in-out.
    #[default]
    Io,
    /// Linear.
    L,
    /// Sine in.
    I,
    /// Sine out.
    O,
    /// Spline.
    Sp,
}

/// A pitch point (`PitchPoint`): `x` is milliseconds from the note start,
/// `y` is pitch offset in 0.1 semitones, `shape` is the interpolation shape.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PitchPoint {
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
    #[serde(default)]
    pub shape: PitchPointShape,
}

impl PitchPoint {
    pub fn new(x: f32, y: f32, shape: PitchPointShape) -> Self {
        PitchPoint { x, y, shape }
    }
}

impl Default for PitchPoint {
    fn default() -> Self {
        PitchPoint { x: 0.0, y: 0.0, shape: PitchPointShape::Io }
    }
}

/// Pitch curve of a note (`UPitch`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UPitch {
    #[serde(default)]
    pub data: Vec<PitchPoint>,
    #[serde(default)]
    pub snap_first: bool,
}

impl Default for UPitch {
    fn default() -> Self {
        UPitch { data: Vec::new(), snap_first: true }
    }
}

impl UPitch {
    /// Add a point, keeping `data` sorted by `x` (C# `UPitch.AddPoint`).
    pub fn add_point(&mut self, point: PitchPoint) {
        self.data.push(point);
        self.data.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
    }

    /// Remove a point (C# `UPitch.RemovePoint`).
    pub fn remove_point(&mut self, point: &PitchPoint) {
        self.data.retain(|p| p != point);
    }
}

/// Vibrato parameters of a note (`UVibrato`). Defaults are the OpenUtau
/// "Standard" note preset: length 0, period 175 ms, depth 25 ct, in/out 10%.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UVibrato {
    /// Vibrato percentage of note length (0..100).
    #[serde(default)]
    pub length: f32,
    /// Period in milliseconds (5..500).
    #[serde(default)]
    pub period: f32,
    /// Depth in cents, 1 semitone = 100 cents (5..200).
    #[serde(default)]
    pub depth: f32,
    /// Fade-in percentage of vibrato length (0..100).
    #[serde(default, rename = "in")]
    pub r#in: f32,
    /// Fade-out percentage of vibrato length (0..100).
    #[serde(default, rename = "out")]
    pub out: f32,
    /// Shift percentage of period length (0..100).
    #[serde(default)]
    pub shift: f32,
    /// Shift the whole vibrato up and down (-100..100).
    #[serde(default)]
    pub drift: f32,
    /// Percentage of volume reduction in linkage with vibrato (-100..100).
    #[serde(default, rename = "vol_link")]
    pub vol_link: f32,
}

impl Default for UVibrato {
    fn default() -> Self {
        UVibrato {
            length: 0.0,
            period: 175.0,
            depth: 25.0,
            r#in: 10.0,
            out: 10.0,
            shift: 0.0,
            drift: 0.0,
            vol_link: 0.0,
        }
    }
}

impl UVibrato {
    /// Clamped setter for `length` (C# property clamping).
    pub fn set_length(&mut self, value: f32) {
        self.length = value.clamp(0.0, 100.0);
    }

    pub fn set_period(&mut self, value: f32) {
        self.period = value.clamp(5.0, 500.0);
    }

    pub fn set_depth(&mut self, value: f32) {
        self.depth = value.clamp(5.0, 200.0);
    }

    /// Setter for `in`; also shrinks `out` so `in + out <= 100`
    /// (C# `UVibrato.in` setter).
    pub fn set_in(&mut self, value: f32) {
        self.r#in = value.clamp(0.0, 100.0);
        self.out = self.out.min(100.0 - self.r#in);
    }

    /// Setter for `out`; also shrinks `in` so `in + out <= 100`
    /// (C# `UVibrato.out` setter).
    pub fn set_out(&mut self, value: f32) {
        self.out = value.clamp(0.0, 100.0);
        self.r#in = self.r#in.min(100.0 - self.out);
    }

    pub fn set_shift(&mut self, value: f32) {
        self.shift = value.clamp(0.0, 100.0);
    }

    pub fn set_drift(&mut self, value: f32) {
        self.drift = value.clamp(-100.0, 100.0);
    }

    pub fn set_vol_link(&mut self, value: f32) {
        self.vol_link = value.clamp(-100.0, 100.0);
    }

    /// Normalized position where the vibrato starts (`UVibrato.NormalizedStart`).
    pub fn normalized_start(&self) -> f32 {
        1.0 - self.length / 100.0
    }
}

/// A note (`UNote`). Positions and durations are in ticks relative to the
/// containing part.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UNote {
    #[serde(default)]
    pub position: i32,
    #[serde(default)]
    pub duration: i32,
    #[serde(default)]
    pub tone: i32,
    #[serde(default = "default_lyric")]
    pub lyric: String,
    #[serde(default)]
    pub pitch: UPitch,
    #[serde(default)]
    pub vibrato: UVibrato,
    #[serde(default)]
    pub tuning: i32,
    /// Per-note phonemizer override. Serialized under the key `phonemizer`
    /// (C# `[YamlMember(Alias = "phonemizer")]`), omitted when unset.
    #[serde(default, rename = "phonemizer", skip_serializing_if = "Option::is_none")]
    pub phonemizer_override: Option<String>,
    #[serde(default)]
    pub phoneme_expressions: Vec<crate::expression::UExpression>,
    #[serde(default)]
    pub phoneme_overrides: Vec<UPhonemeOverride>,
}

fn default_lyric() -> String {
    "a".to_string()
}

impl Default for UNote {
    fn default() -> Self {
        UNote {
            position: 0,
            duration: 0,
            tone: 0,
            lyric: default_lyric(),
            pitch: UPitch::default(),
            vibrato: UVibrato::default(),
            tuning: 0,
            phonemizer_override: None,
            phoneme_expressions: Vec::new(),
            phoneme_overrides: Vec::new(),
        }
    }
}

impl UNote {
    /// `UNote.End` — note end tick (exclusive).
    pub fn end(&self) -> i32 {
        self.position + self.duration
    }

    /// `UNote.AdjustedTone` — tone with `tuning` (in cents) applied.
    pub fn adjusted_tone(&self) -> f32 {
        self.tone as f32 + self.tuning as f32 / 100.0
    }

    /// Split the phonetic hint out of the lyric, mirroring
    /// `UNote.ToPhonemizerNote`: `"a [e]"` → `("a", Some("e"))`.
    /// The cleaned lyric is trimmed.
    pub fn phonetic_hint(&self) -> (String, Option<String>) {
        split_phonetic_hint(&self.lyric)
    }
}

/// Split a `[hint]` suffix out of a lyric string.
pub fn split_phonetic_hint(lyric: &str) -> (String, Option<String>) {
    if let Some(start) = lyric.find('[') {
        if let Some(rel_end) = lyric[start + 1..].find(']') {
            let end = start + 1 + rel_end;
            let hint = lyric[start + 1..end].trim().to_string();
            let mut cleaned = String::with_capacity(lyric.len());
            cleaned.push_str(&lyric[..start]);
            cleaned.push_str(&lyric[end + 1..]);
            return (cleaned.trim().to_string(), Some(hint));
        }
    }
    (lyric.trim().to_string(), None)
}

/// Per-phoneme override of the phonemizer output (`UPhonemeOverride`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct UPhonemeOverride {
    #[serde(default)]
    pub index: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phoneme: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preutter_delta: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlap_delta: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attack_time_delta: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_time_delta: Option<f32>,
}

impl UPhonemeOverride {
    /// `UPhonemeOverride.IsEmpty` — no field is set.
    pub fn is_empty(&self) -> bool {
        self.phoneme.is_none()
            && self.offset.is_none()
            && self.preutter_delta.is_none()
            && self.overlap_delta.is_none()
            && self.attack_time_delta.is_none()
            && self.release_time_delta.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_yaml_matches_openutau_keys() {
        let note = UNote {
            position: 120,
            duration: 60,
            tone: 42,
            lyric: "あ".into(),
            ..Default::default()
        };
        let yaml = serde_yaml::to_string(&note).unwrap();
        assert!(yaml.contains("position: 120"));
        assert!(yaml.contains("duration: 60"));
        assert!(yaml.contains("tone: 42"));
        assert!(yaml.contains("lyric: あ"));
        assert!(yaml.contains("snap_first: true"));
        assert!(yaml.contains("phoneme_expressions: []"));
        assert!(yaml.contains("phoneme_overrides: []"));
        assert!(!yaml.contains("phonemizer"));
    }

    #[test]
    fn phonemizer_override_key_is_phonemizer() {
        let note = UNote {
            phonemizer_override: Some("custom".into()),
            ..Default::default()
        };
        let yaml = serde_yaml::to_string(&note).unwrap();
        assert!(yaml.contains("phonemizer: custom"));
        assert!(!yaml.contains("phonemizer_override"));
        let back: UNote = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(back.phonemizer_override.as_deref(), Some("custom"));
    }

    #[test]
    fn default_vibrato_matches_note_presets() {
        let v = UVibrato::default();
        assert_eq!((v.length, v.period, v.depth, v.r#in, v.out, v.shift, v.drift, v.vol_link),
                   (0.0, 175.0, 25.0, 10.0, 10.0, 0.0, 0.0, 0.0));
        let mut v2 = v.clone();
        v2.set_in(80.0);
        assert_eq!(v2.r#in, 80.0);
        assert_eq!(v2.out, 10.0); // unchanged: 10 <= 100 - 80
        v2.set_in(95.0);
        assert_eq!(v2.out, 5.0); // shrunk so in + out <= 100
        v2.set_length(150.0);
        assert_eq!(v2.length, 100.0);
    }

    #[test]
    fn pitch_add_point_sorts() {
        let mut pitch = UPitch::default();
        pitch.add_point(PitchPoint::new(40.0, 0.0, PitchPointShape::Io));
        pitch.add_point(PitchPoint::new(-40.0, 0.0, PitchPointShape::Io));
        assert_eq!(pitch.data[0].x, -40.0);
        assert_eq!(pitch.data[1].x, 40.0);
    }

    #[test]
    fn phonetic_hint_splitting() {
        assert_eq!(split_phonetic_hint("a"), ("a".to_string(), None));
        assert_eq!(split_phonetic_hint("a [e]"), ("a".to_string(), Some("e".to_string())));
        assert_eq!(split_phonetic_hint(" あ [a] "), ("あ".to_string(), Some("a".to_string())));
        assert_eq!(split_phonetic_hint("[do]"), ("".to_string(), Some("do".to_string())));
        assert_eq!(split_phonetic_hint("a [unclosed"), ("a [unclosed".to_string(), None));
    }

    #[test]
    fn phoneme_override_empty() {
        assert!(UPhonemeOverride::default().is_empty());
        let o = UPhonemeOverride { index: 0, phoneme: Some("a".into()), ..Default::default() };
        assert!(!o.is_empty());
    }
}
