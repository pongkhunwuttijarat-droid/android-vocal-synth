//! Project model: `UProject`, `UTempo`, `UTimeSignature`, `UstxVersion`.
//!
//! Mirrors `OpenUtau.Core/Ustx/UProject.cs` and the save/load flow in
//! `OpenUtau.Core/Format/USTx.cs`. Serialized YAML field order and keys
//! match OpenUtau's output:
//!
//! ```yaml
//! name: New Project
//! comment: ''
//! output_dir: Vocal
//! cache_dir: UCache
//! ustx_version: '0.9'
//! bpm: 120
//! beat_per_bar: 4
//! beat_unit: 4
//! expressions: { ... }
//! exp_selectors: [dyn, pitd, clr, eng, vel, vol, atk, dec, gen, bre]
//! exp_primary: 0
//! exp_secondary: 1
//! key: 0
//! time_signatures: [...]
//! tempos: [...]
//! tracks: [...]
//! voice_parts: [...]
//! wave_parts: [...]
//! ```

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::expression::{
    add_default_expressions, EXP_SELECTORS_DEFAULT, ATK,
};
use crate::note::{PitchPoint, PitchPointShape, UNote};
use crate::part::{UPart, UVoicePart, UWavePart};
use crate::time_axis::TimeAxis;
use crate::track::UTrack;
use crate::{K_USTX_VERSION, RESOLUTION};

/// A tempo change (`UTempo`): `position` in ticks, `bpm`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct UTempo {
    #[serde(default)]
    pub position: i32,
    #[serde(default)]
    pub bpm: f64,
}

impl Default for UTempo {
    fn default() -> Self {
        UTempo { position: 0, bpm: 0.0 }
    }
}

impl UTempo {
    pub fn new(position: i32, bpm: f64) -> Self {
        UTempo { position, bpm }
    }
}

/// A time signature change (`UTimeSignature`): `bar_position` in bars,
/// `beat_per_bar` beats per bar, `beat_unit` beat unit (4 = quarter note).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct UTimeSignature {
    #[serde(default)]
    pub bar_position: i32,
    #[serde(default)]
    pub beat_per_bar: i32,
    #[serde(default)]
    pub beat_unit: i32,
}

impl Default for UTimeSignature {
    fn default() -> Self {
        UTimeSignature { bar_position: 0, beat_per_bar: 4, beat_unit: 4 }
    }
}

impl UTimeSignature {
    pub fn new(bar_position: i32, beat_per_bar: i32, beat_unit: i32) -> Self {
        UTimeSignature { bar_position, beat_per_bar, beat_unit }
    }
}

/// A `major.minor` ustx format version (`System.Version` subset).
///
/// Serialized as a string (`ustx_version: '0.9'`, like OpenUtau's
/// `[YamlMember(SerializeAs = typeof(string))]`), and accepted on read as a
/// string or a bare number (some writers emit `ustx_version: 0.6`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct UstxVersion {
    pub major: u32,
    pub minor: u32,
}

impl UstxVersion {
    pub const fn new(major: u32, minor: u32) -> Self {
        UstxVersion { major, minor }
    }

    /// Parse `"0.9"`, `"0.9.0"`, `"0.9.0.0"` or `"0"`. Build/revision
    /// components are ignored (OpenUtau only ever writes `major.minor`).
    pub fn parse(s: &str) -> Option<Self> {
        let mut parts = s.trim().split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next().map(|p| p.parse().ok()).unwrap_or(Some(0))?;
        Some(UstxVersion { major, minor })
    }
}

impl fmt::Display for UstxVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

impl FromStr for UstxVersion {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        UstxVersion::parse(s).ok_or_else(|| format!("invalid ustx version: {s:?}"))
    }
}

impl Serialize for UstxVersion {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for UstxVersion {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct VersionVisitor;

        impl<'de> Visitor<'de> for VersionVisitor {
            type Value = UstxVersion;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "a ustx version string like \"0.9\" or a number like 0.9")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                UstxVersion::parse(v).ok_or_else(|| E::custom(format!("invalid ustx version {v:?}")))
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                UstxVersion::parse(&v.to_string())
                    .ok_or_else(|| E::custom(format!("invalid ustx version {v}")))
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                if v < 0 {
                    return Err(E::custom(format!("invalid ustx version {v}")));
                }
                self.visit_u64(v as u64)
            }

            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
                // "0.6" written unquoted arrives here as the float 0.6.
                UstxVersion::parse(&v.to_string())
                    .ok_or_else(|| E::custom(format!("invalid ustx version {v}")))
            }
        }

        deserializer.deserialize_any(VersionVisitor)
    }
}

/// An OpenUtau-compatible project (`UProject`).
///
/// # Save/load lifecycle
///
/// * [`UProject::before_save`] — mirror of `UProject.BeforeSave`: sets
///   `ustx_version`, sorts notes/expressions/parts, and fills the
///   serialized `voice_parts`/`wave_parts` lists.
/// * [`UProject::after_load`] — mirror of `Ustx.Load`: merges default
///   expressions, merges `voice_parts`/`wave_parts` into `parts`, drops
///   note expressions with unknown abbreviations, applies legacy version
///   migrations, revalidates timing, and bumps the version to 0.9.
///
/// The `time_axis` is rebuilt by `validate`/`after_load`; a freshly
/// deserialized project has an empty axis until one of those runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UProject {
    #[serde(default = "default_project_name")]
    pub name: String,
    #[serde(default)]
    pub comment: String,
    #[serde(default = "default_output_dir")]
    pub output_dir: String,
    #[serde(default = "default_cache_dir")]
    pub cache_dir: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ustx_version: Option<UstxVersion>,
    /// Legacy tempo (deprecated since ustx v0.6, still written by OpenUtau).
    #[serde(default = "default_bpm")]
    pub bpm: f64,
    /// Legacy beats per bar (deprecated since ustx v0.6).
    #[serde(default = "default_beat_per_bar")]
    pub beat_per_bar: i32,
    /// Legacy beat unit (deprecated since ustx v0.6).
    #[serde(default = "default_beat_unit")]
    pub beat_unit: i32,
    #[serde(default)]
    pub expressions: HashMap<String, crate::expression::UExpressionDescriptor>,
    #[serde(default = "default_exp_selectors")]
    pub exp_selectors: Vec<String>,
    #[serde(default)]
    pub exp_primary: i32,
    #[serde(default = "default_exp_secondary")]
    pub exp_secondary: i32,
    /// Music key of the project: 0 = C, 1 = C#, ..., 11 = B.
    #[serde(default)]
    pub key: i32,
    #[serde(default)]
    pub time_signatures: Vec<UTimeSignature>,
    #[serde(default)]
    pub tempos: Vec<UTempo>,
    #[serde(default)]
    pub tracks: Vec<UTrack>,
    /// Serialized voice parts (transient in OpenUtau: filled by
    /// `BeforeSave`, merged into `parts` by `AfterLoad`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice_parts: Option<Vec<UVoicePart>>,
    /// Serialized wave parts (same transient lifecycle).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wave_parts: Option<Vec<UWavePart>>,
    /// In-memory part list (`[YamlIgnore]` in OpenUtau).
    #[serde(skip)]
    pub parts: Vec<UPart>,
    /// Tick/time conversion engine (`[YamlIgnore]` in OpenUtau); rebuilt by
    /// [`validate`](Self::validate)/[`after_load`](Self::after_load).
    #[serde(skip)]
    pub time_axis: TimeAxis,
}

fn default_project_name() -> String {
    "New Project".into()
}
fn default_output_dir() -> String {
    "Vocal".into()
}
fn default_cache_dir() -> String {
    "UCache".into()
}
fn default_bpm() -> f64 {
    120.0
}
fn default_beat_per_bar() -> i32 {
    4
}
fn default_beat_unit() -> i32 {
    4
}
fn default_exp_selectors() -> Vec<String> {
    EXP_SELECTORS_DEFAULT.map(str::to_string).to_vec()
}
fn default_exp_secondary() -> i32 {
    1
}

impl Default for UProject {
    fn default() -> Self {
        UProject {
            name: default_project_name(),
            comment: String::new(),
            output_dir: default_output_dir(),
            cache_dir: default_cache_dir(),
            ustx_version: None,
            bpm: default_bpm(),
            beat_per_bar: default_beat_per_bar(),
            beat_unit: default_beat_unit(),
            expressions: HashMap::new(),
            exp_selectors: default_exp_selectors(),
            exp_primary: 0,
            exp_secondary: default_exp_secondary(),
            key: 0,
            time_signatures: Vec::new(),
            tempos: Vec::new(),
            tracks: Vec::new(),
            voice_parts: None,
            wave_parts: None,
            parts: Vec::new(),
            time_axis: TimeAxis::default(),
        }
    }
}

impl UProject {
    /// Constructor mirroring `UProject()`: default time signature 4/4 at
    /// bar 0, tempo 120 BPM at tick 0, one track `Track1`, and a built
    /// time axis. Note: no default expressions — use [`create`](Self::create)
    /// for the full OpenUtau new-project state.
    pub fn new() -> Self {
        let mut project = UProject {
            time_signatures: vec![UTimeSignature::new(0, 4, 4)],
            tempos: vec![UTempo::new(0, 120.0)],
            tracks: vec![UTrack::new("Track1")],
            ..Default::default()
        };
        project
            .time_axis
            .build_segments(&project.time_signatures, &project.tempos)
            .expect("default project timing is valid");
        project
    }

    /// `Ustx.Create()`: a fresh project with default expressions registered.
    pub fn create() -> Self {
        let mut project = UProject::new();
        add_default_expressions(&mut project);
        project
    }

    /// `UProject.RegisterExpression` — add only when the abbreviation is
    /// not already present.
    pub fn register_expression(&mut self, descriptor: crate::expression::UExpressionDescriptor) {
        self.expressions
            .entry(descriptor.abbr.clone())
            .or_insert(descriptor);
    }

    /// `UProject.CreateNote()` with the default portamento pitch points
    /// (at -40 ms and +40 ms, `io` shape).
    pub fn create_note(&self) -> UNote {
        let mut note = UNote::default();
        note.pitch.add_point(PitchPoint::new(-40.0, 0.0, PitchPointShape::Io));
        note.pitch.add_point(PitchPoint::new(40.0, 0.0, PitchPointShape::Io));
        note
    }

    /// `UProject.CreateNote(noteNum, posTick, durTick)`.
    pub fn create_note_at(&self, note_num: i32, pos_tick: i32, dur_tick: i32) -> UNote {
        let mut note = self.create_note();
        note.tone = note_num;
        note.position = pos_tick;
        note.duration = dur_tick;
        note
    }

    /// `UProject.EndTick` — end of the last part, or 0 when there are none.
    pub fn end_tick(&self) -> i32 {
        self.parts.iter().map(UPart::end).max().unwrap_or(0)
    }

    /// `UProject.SoloTrackExist`.
    pub fn solo_track_exists(&self) -> bool {
        self.tracks.iter().any(|t| t.solo)
    }

    /// Mirror of `UProject.BeforeSave`: set the format version, sort
    /// notes and note expressions, and populate the serialized
    /// `voice_parts`/`wave_parts` lists (sorted by track, then position).
    pub fn before_save(&mut self) {
        self.ustx_version = Some(K_USTX_VERSION);
        for part in &mut self.parts {
            if let UPart::Voice(vp) = part {
                vp.notes.sort_by_key(|n| n.position);
                for note in &mut vp.notes {
                    note.phoneme_expressions.sort_by(|a, b| {
                        a.index
                            .unwrap_or(i32::MIN)
                            .cmp(&b.index.unwrap_or(i32::MIN))
                            .then_with(|| a.abbr.cmp(&b.abbr))
                    });
                }
            }
        }
        let mut voices: Vec<UVoicePart> = Vec::new();
        let mut waves: Vec<UWavePart> = Vec::new();
        for part in &self.parts {
            match part {
                UPart::Voice(vp) => voices.push(vp.clone()),
                UPart::Wave(wp) => waves.push(wp.clone()),
            }
        }
        voices.sort_by(|a, b| a.track_no.cmp(&b.track_no).then_with(|| a.position.cmp(&b.position)));
        waves.sort_by(|a, b| a.track_no.cmp(&b.track_no).then_with(|| a.position.cmp(&b.position)));
        self.voice_parts = Some(voices);
        self.wave_parts = Some(waves);
    }

    /// Mirror of `Ustx.Load` + `UProject.AfterLoad` + `ValidateFull`:
    /// merge default expressions, merge serialized parts, drop note
    /// expressions with unknown abbreviations, apply legacy version
    /// migrations, revalidate timing, and bump the version to 0.9.
    pub fn after_load(&mut self) -> Result<(), String> {
        add_default_expressions(self);

        if let Some(voice_parts) = self.voice_parts.take() {
            self.parts.extend(voice_parts.into_iter().map(UPart::Voice));
        }
        if let Some(wave_parts) = self.wave_parts.take() {
            self.parts.extend(wave_parts.into_iter().map(UPart::Wave));
        }

        // UNote.AfterLoad: drop phoneme expressions whose abbreviation is
        // not registered anywhere. The set of valid abbreviations is
        // computed per track without holding cross-field borrows.
        for part in &mut self.parts {
            if let UPart::Voice(vp) = part {
                let valid_abbrs: Option<std::collections::HashSet<&str>> = self
                    .tracks
                    .get(vp.track_no as usize)
                    .map(|track| {
                        let mut set: std::collections::HashSet<&str> =
                            self.expressions.keys().map(String::as_str).collect();
                        for te in &track.track_expressions {
                            set.insert(te.abbr.as_str());
                        }
                        set
                    });
                if let Some(valid) = valid_abbrs {
                    for note in &mut vp.notes {
                        note.phoneme_expressions
                            .retain(|exp| valid.contains(exp.abbr.as_str()));
                    }
                }
                // Track missing (corrupt file): keep expressions rather
                // than silently destroying data.
            }
        }

        // Legacy migrations, in the order of Ustx.Load.
        if let Some(v) = self.ustx_version {
            if v < UstxVersion::new(0, 4) {
                self.migrate_acc_to_atk();
            }
            if v < UstxVersion::new(0, 5) {
                self.migrate_rest_lyrics();
            }
            if v < UstxVersion::new(0, 6) {
                self.migrate_legacy_timing();
            }
            if v < UstxVersion::new(0, 7) {
                self.migrate_exp_selectors();
            }
        }

        self.validate()?;

        // UVoicePart.AfterLoad: `Duration = Math.Max(Duration, GetMinDurTick(...))`.
        for part in &mut self.parts {
            if let UPart::Voice(vp) = part {
                let min_dur = vp.get_min_dur_tick(&self.time_axis);
                vp.duration = vp.duration.max(min_dur);
            }
        }

        self.ustx_version = Some(K_USTX_VERSION);
        Ok(())
    }

    /// `UProject.Validate`: sort timing lists and rebuild the time axis.
    pub fn validate(&mut self) -> Result<(), String> {
        self.time_signatures.sort_by_key(|ts| ts.bar_position);
        self.tempos.sort_by_key(|t| t.position);
        self.time_axis
            .build_segments(&self.time_signatures, &self.tempos)
    }

    /// ustx < 0.4: rename the `acc`/`accent` expression to `atk`/`attack`
    /// and rewrite note expressions (Ustx.Load).
    fn migrate_acc_to_atk(&mut self) {
        let Some(exp) = self.expressions.remove("acc") else { return };
        if exp.name != "accent" {
            self.expressions.insert("acc".to_string(), exp);
            return;
        }
        let mut exp = exp;
        exp.abbr = ATK.to_string();
        exp.name = "attack".to_string();
        self.expressions.insert(ATK.to_string(), exp);
        for part in &mut self.parts {
            if let UPart::Voice(vp) = part {
                for note in &mut vp.notes {
                    for e in &mut note.phoneme_expressions {
                        if e.abbr == "acc" {
                            e.abbr = ATK.to_string();
                        }
                    }
                }
            }
        }
    }

    /// ustx < 0.5: rest lyrics starting with `...` become `+` (Ustx.Load).
    fn migrate_rest_lyrics(&mut self) {
        for part in &mut self.parts {
            if let UPart::Voice(vp) = part {
                for note in &mut vp.notes {
                    if note.lyric.starts_with("...") {
                        note.lyric = note.lyric.replace("...", "+");
                    }
                }
            }
        }
    }

    /// ustx < 0.6: rebuild timing from the legacy `bpm`/`beat_per_bar`/
    /// `beat_unit` fields (Ustx.Load).
    fn migrate_legacy_timing(&mut self) {
        self.time_signatures = vec![UTimeSignature::new(0, self.beat_per_bar, self.beat_unit)];
        self.tempos = vec![UTempo::new(0, self.bpm)];
    }

    /// ustx < 0.7: pad `exp_selectors` to the default list, keeping the
    /// project's own entries at the front (Ustx.Load).
    fn migrate_exp_selectors(&mut self) {
        if self.exp_selectors.len() < EXP_SELECTORS_DEFAULT.len() {
            let mut selectors = default_exp_selectors();
            for (i, s) in self.exp_selectors.iter().enumerate() {
                selectors[i] = s.clone();
            }
            self.exp_selectors = selectors;
        }
    }

    /// Ticks per quarter note (`UProject.resolution` — a constant).
    pub const fn resolution(&self) -> i32 {
        RESOLUTION
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parse() {
        assert_eq!(UstxVersion::parse("0.9"), Some(UstxVersion::new(0, 9)));
        assert_eq!(UstxVersion::parse("0.9.0"), Some(UstxVersion::new(0, 9)));
        assert_eq!(UstxVersion::parse("0.9.1.2"), Some(UstxVersion::new(0, 9)));
        assert_eq!(UstxVersion::parse("0"), Some(UstxVersion::new(0, 0)));
        assert_eq!(UstxVersion::parse("1.2"), Some(UstxVersion::new(1, 2)));
        assert_eq!(UstxVersion::parse("abc"), None);
        assert_eq!(UstxVersion::parse(""), None);
    }

    #[test]
    fn version_serde_string_and_number() {
        let v = UstxVersion::new(0, 9);
        let yaml = serde_yaml::to_string(&v).unwrap();
        assert_eq!(serde_yaml::from_str::<UstxVersion>(yaml.trim()).unwrap(), v);
        assert_eq!(serde_yaml::from_str::<UstxVersion>("'0.9'").unwrap(), v);
        assert_eq!(serde_yaml::from_str::<UstxVersion>("\"0.9\"").unwrap(), v);
        assert_eq!(serde_yaml::from_str::<UstxVersion>("0.6").unwrap(), UstxVersion::new(0, 6));
        assert_eq!(serde_yaml::from_str::<UstxVersion>("0").unwrap(), UstxVersion::new(0, 0));
        assert!(serde_yaml::from_str::<UstxVersion>("x.y").is_err());
    }

    #[test]
    fn version_ordering() {
        assert!(UstxVersion::new(0, 5) < UstxVersion::new(0, 6));
        assert!(UstxVersion::new(0, 9) < UstxVersion::new(1, 0));
        assert!(UstxVersion::new(0, 9) == K_USTX_VERSION);
    }

    #[test]
    fn new_project_state() {
        let p = UProject::new();
        assert_eq!(p.name, "New Project");
        assert_eq!(p.time_signatures, vec![UTimeSignature::new(0, 4, 4)]);
        assert_eq!(p.tempos, vec![UTempo::new(0, 120.0)]);
        assert_eq!(p.tracks.len(), 1);
        assert_eq!(p.tracks[0].track_name, "Track1");
        assert!(p.expressions.is_empty());
        assert_eq!(p.time_axis.bpm_at_tick(0), 120.0);
        assert_eq!(p.resolution(), 480);
    }

    #[test]
    fn create_note_default_portamento() {
        let p = UProject::create();
        let n = p.create_note_at(57, 240, 480);
        assert_eq!((n.tone, n.position, n.duration), (57, 240, 480));
        assert_eq!(n.pitch.data.len(), 2);
        assert_eq!(n.pitch.data[0].x, -40.0);
        assert_eq!(n.pitch.data[1].x, 40.0);
        assert_eq!(n.pitch.data[0].shape, PitchPointShape::Io);
    }
}
