//! Derived phoneme model: `UPhoneme`, `UEnvelope`.
//!
//! Mirrors `OpenUtau.Core/Ustx/UPhoneme.cs`. Phonemes are derived from the
//! phonemizer + oto data at render time and are **not serialized**. Full
//! oto-based validation (`ValidateDuration`/`ValidateOto`/`ValidateOverlap`/
//! `ValidateEnvelope`) depends on the voicebank crate's oto lookup and is
//! intentionally not implemented here; this module provides the data holder
//! plus the expression/flag helpers that do not need oto data.

use crate::expression::{UExpressionDescriptor, UExpressionType};
use crate::note::UNote;
use crate::project::UProject;
use crate::track::UTrack;

/// The five-point volume envelope of a phoneme (`UEnvelope`), as
/// `(x_ms, volume)` pairs relative to the phoneme start.
#[derive(Debug, Clone, PartialEq)]
pub struct UEnvelope {
    pub data: Vec<(f32, f32)>,
}

impl Default for UEnvelope {
    fn default() -> Self {
        UEnvelope {
            data: vec![
                (0.0, 0.0),
                (0.0, 100.0),
                (0.0, 100.0),
                (0.0, 100.0),
                (0.0, 0.0),
            ],
        }
    }
}

/// A derived phoneme (`UPhoneme`). Not serialized.
///
/// `parent` is the index of the owning note in `UVoicePart::notes`
/// (OpenUtau holds a direct `UNote` reference; an index keeps the model
/// `Clone`-able and serialization-free).
#[derive(Debug, Clone, PartialEq)]
pub struct UPhoneme {
    /// Position in ticks within the part, as produced by the phonemizer.
    pub raw_position: i32,
    /// Phoneme symbol as produced by the phonemizer.
    pub raw_phoneme: String,
    /// Phoneme index within its note.
    pub index: i32,
    /// Position in ticks within the part after overrides/validation.
    pub position: i32,
    /// Phoneme symbol after overrides.
    pub phoneme: String,
    /// Duration in ticks after validation.
    pub duration: i32,
    /// Position in milliseconds relative to the project start.
    pub position_ms: f64,
    /// Duration in milliseconds.
    pub duration_ms: f64,
    /// Leading (preutter) time in milliseconds.
    pub leading_ms: f64,
    /// Overlap time in milliseconds.
    pub overlap_ms: f64,
    /// Computed preutter from oto, before deltas (`autoPreutter`).
    pub auto_preutter: f64,
    /// Computed overlap from oto, before deltas (`autoOverlap`).
    pub auto_overlap: f64,
    /// `maxOtoPreutter` — oto preutter capped for adjacency.
    pub max_oto_preutter: f64,
    pub adjacent: bool,
    pub overlapped: bool,
    pub tail_intrude: f64,
    pub tail_overlap: f64,
    pub preutter_delta: Option<f64>,
    pub overlap_delta: Option<f64>,
    pub attack_time_delta: Option<f64>,
    pub release_time_delta: Option<f64>,
    pub crossfade: bool,
    /// Tone of the parent note (copied at derivation time).
    pub tone: i32,
    /// Per-phoneme tone shift in semitones (from the `shft` expression),
    /// filled by the render pipeline.
    pub tone_shift: Option<i32>,
    /// Resampler flags as strings, filled by the render pipeline.
    pub flags: Vec<String>,
    /// Index of the parent note in `UVoicePart::notes`.
    pub parent: Option<usize>,
}

impl Default for UPhoneme {
    fn default() -> Self {
        UPhoneme {
            raw_position: 0,
            raw_phoneme: "a".into(),
            index: 0,
            position: 0,
            phoneme: "a".into(),
            duration: 0,
            position_ms: 0.0,
            duration_ms: 0.0,
            leading_ms: 0.0,
            overlap_ms: 0.0,
            auto_preutter: 0.0,
            auto_overlap: 0.0,
            max_oto_preutter: 0.0,
            adjacent: false,
            overlapped: false,
            tail_intrude: 0.0,
            tail_overlap: 0.0,
            preutter_delta: None,
            overlap_delta: None,
            attack_time_delta: None,
            release_time_delta: None,
            crossfade: true,
            tone: 0,
            tone_shift: None,
            flags: Vec::new(),
            parent: None,
        }
    }
}

impl UPhoneme {
    pub fn new(phoneme: impl Into<String>) -> Self {
        UPhoneme { phoneme: phoneme.into(), ..Default::default() }
    }

    /// `UPhoneme.End` — end tick within the part.
    pub fn end(&self) -> i32 {
        self.position + self.duration
    }

    /// `UPhoneme.GetExpression`: the value of `abbr` for this phoneme.
    /// Returns `(value, true)` when the value comes from a per-phoneme
    /// expression on the note, `(descriptor custom default, false)`
    /// otherwise. Returns `None` when the abbreviation is not registered.
    pub fn get_expression(
        &self,
        note: &UNote,
        project: &UProject,
        track: &UTrack,
        abbr: &str,
    ) -> Option<(f32, bool)> {
        let descriptor = track.try_get_exp_descriptor(project, abbr)?;
        if let Some(exp) = note
            .phoneme_expressions
            .iter()
            .find(|e| e.abbr == abbr && e.index == Some(self.index))
        {
            return Some((exp.value, true));
        }
        Some((descriptor.custom_default_value(), false))
    }

    /// `UPhoneme.GetResamplerFlags`: `(flag, value, abbr)` tuples for all
    /// registered expressions, mirroring the C# implementation (track
    /// expressions override project expressions with the same abbreviation;
    /// `skip_output_if_default` suppresses numerical flags at their default).
    pub fn get_resampler_flags(
        &self,
        note: &UNote,
        project: &UProject,
        track: &UTrack,
    ) -> Vec<(String, Option<i32>, String)> {
        let mut descriptors: Vec<UExpressionDescriptor> = Vec::new();
        for d in project.expressions.values() {
            if !track.track_expressions.iter().any(|te| te.abbr == d.abbr) {
                descriptors.push(d.clone());
            }
        }
        descriptors.extend(track.track_expressions.iter().cloned());

        let mut flags = Vec::new();
        for d in &descriptors {
            let Some((value, _)) = self.get_expression(note, project, track, &d.abbr) else {
                continue;
            };
            match d.r#type {
                UExpressionType::Numerical => {
                    if let Some(flag) = &d.flag {
                        let v = value as i32;
                        if d.skip_output_if_default && v == d.default_value as i32 {
                            continue;
                        }
                        flags.push((flag.clone(), Some(v), d.abbr.clone()));
                    }
                }
                UExpressionType::Options => {
                    if d.is_flag {
                        if let Some(options) = &d.options {
                            if options.is_empty() {
                                continue;
                            }
                            let idx = (value as i32).clamp(0, options.len() as i32 - 1) as usize;
                            flags.push((options[idx].clone(), None, d.abbr.clone()));
                        }
                    }
                }
                UExpressionType::Curve => {}
            }
        }
        flags
    }

    /// Resampler flags stringified the way OpenUtau's classic resampler does
    /// (`FlagsToString`): `flag + value` for numerical flags (`g5`), the
    /// option string itself for option flags (`on`).
    pub fn flags_as_strings(
        &self,
        note: &UNote,
        project: &UProject,
        track: &UTrack,
    ) -> Vec<String> {
        self.get_resampler_flags(note, project, track)
            .iter()
            .map(|(flag, value, _)| match value {
                Some(v) => format!("{flag}{v}"),
                None => flag.clone(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{UProject, UPhonemeOverride};

    #[test]
    fn envelope_defaults() {
        let e = UEnvelope::default();
        assert_eq!(e.data.len(), 5);
        assert_eq!(e.data[0], (0.0, 0.0));
        assert_eq!(e.data[1], (0.0, 100.0));
        assert_eq!(e.data[4], (0.0, 0.0));
    }

    #[test]
    fn phoneme_expression_lookup() {
        let p = UProject::create();
        let track = &p.tracks[0];
        let mut note = p.create_note_at(60, 0, 480);
        note.phoneme_expressions.push(crate::UExpression {
            index: Some(0),
            abbr: crate::VEL.into(),
            value: 123.0,
        });
        note.phoneme_overrides.push(UPhonemeOverride::default());
        let ph = UPhoneme { index: 0, parent: Some(0), ..Default::default() };
        assert_eq!(ph.get_expression(&note, &p, track, crate::VEL), Some((123.0, true)));
        assert_eq!(ph.get_expression(&note, &p, track, crate::VOL), Some((100.0, false)));
        assert_eq!(ph.get_expression(&note, &p, track, "nope"), None);
        // index 1 has no expression -> falls back to the default
        let ph1 = UPhoneme { index: 1, parent: Some(0), ..Default::default() };
        assert_eq!(ph1.get_expression(&note, &p, track, crate::VEL), Some((100.0, false)));
    }
}
