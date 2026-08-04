//! `RenderInput` building for one phrase group.
//!
//! [`PhraseBuilder`] is the port of the `RenderPhrase` constructor
//! (`OpenUtau.Core/Render/RenderPhrase.cs`), producing the feed crate's
//! superset [`feed::RenderInput`] from a [`crate::PhraseGroup`] plus its
//! part / project / track:
//!
//! * notes — the phrase's parent notes plus one adjacent note on either
//!   side when it touches the phrase (the `uNotes` walk of the reference);
//! * pitch — [`feed::PitchComputer`] (flat pitches → vibrato → pitch
//!   points → PITD), sampled every 5 ticks starting `leading` ticks before
//!   the first phoneme;
//! * curves — every `Curve`-type expression of the project sampled at the
//!   same 5-tick grid ([`feed::sample_curve`]), with the `dyn` dB→linear
//!   conversion and per-expression defaults for missing curves;
//! * phonemes — timing plus per-phoneme `shft` tone shift;
//! * sample-based inputs — oto entries via [`feed::OtoMapper`], the 5-point
//!   envelope via [`feed::EnvelopeBuilder`] (after
//!   [`feed::EnvelopeBuilder::compute_adjacency`], the `ValidateOverlap`
//!   pass of the reference), and the resampler flags string;
//! * neural inputs are left `None` — tokenization is renderer-specific and
//!   out of scope here.

use domain::{UNote, UPhoneme, UProject, UTrack, UVoicePart, CLR, DYN, PITD, SHFT};
use feed::render_input::{Curves, NamedCurve, RenderInput, RenderNote, RenderPhoneme, SampleBased};
use feed::{decibel_to_linear, sample_curve, EnvelopeBuilder, FeedError, OtoMapper, PitchComputer};
use voicebank::Voicebank;

use crate::grouping::PhraseGroup;

/// Errors produced by [`PhraseBuilder::build`].
#[derive(Debug, Clone, PartialEq)]
pub enum PhraseError {
    /// The group contains no phonemes.
    Empty,
    /// A phoneme has no parent note index, or the index is out of range
    /// for the part's notes.
    MissingParent(usize),
    /// The part's `track_no` does not address a track of the project.
    TrackNotFound(i32),
    /// A feed-level failure (e.g. no voice part).
    Feed(FeedError),
}

impl std::fmt::Display for PhraseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PhraseError::Empty => write!(f, "phrase group has no phonemes"),
            PhraseError::MissingParent(i) => {
                write!(f, "phoneme {i} has no valid parent note")
            }
            PhraseError::TrackNotFound(t) => write!(f, "track {t} not found"),
            PhraseError::Feed(e) => write!(f, "feed error: {e}"),
        }
    }
}

impl std::error::Error for PhraseError {}

/// Builds a [`feed::RenderInput`] for one phrase group.
///
/// The project's time axis must be built (`after_load`/`validate`), like
/// everywhere else in the pipeline.
pub struct PhraseBuilder<'a> {
    project: &'a UProject,
    track: &'a UTrack,
    part: &'a UVoicePart,
    voicebank: Option<&'a Voicebank>,
}

impl<'a> PhraseBuilder<'a> {
    /// `project` / `track` / `part` — the phrase's containing structures;
    /// `voicebank` — the singer, used to fill the sample-based section
    /// (`None` leaves `sample_based` empty).
    pub fn new(
        project: &'a UProject,
        track: &'a UTrack,
        part: &'a UVoicePart,
        voicebank: Option<&'a Voicebank>,
    ) -> Self {
        PhraseBuilder {
            project,
            track,
            part,
            voicebank,
        }
    }

    /// Build the render input for `group`.
    ///
    /// The group's phonemes must carry `parent` indices into
    /// `part.notes` (as produced by the phonemizer + `TimingEngine`).
    pub fn build(&self, group: &PhraseGroup) -> Result<RenderInput, PhraseError> {
        if group.phonemes.is_empty() {
            return Err(PhraseError::Empty);
        }

        // ValidateOverlap pass on a working copy: caps preutter/overlap
        // against the previous phoneme and fills the tail fields, exactly
        // like the reference does before building envelopes.
        let mut phonemes = group.phonemes.clone();
        EnvelopeBuilder::compute_adjacency(&mut phonemes);

        let first = phonemes.first().expect("non-empty");
        let last = phonemes.last().expect("non-empty");

        // Parent note span of the phrase. The reference walks the linked
        // note list from the first phoneme's note to the last's (through
        // Extends chains, which the flat model does not have), then pulls
        // in one adjacent note on each side when it touches the phrase.
        let first_parent = first.parent.ok_or(PhraseError::MissingParent(0))?;
        let last_parent = last
            .parent
            .ok_or(PhraseError::MissingParent(phonemes.len() - 1))?;
        if first_parent >= self.part.notes.len() || last_parent >= self.part.notes.len() {
            return Err(PhraseError::MissingParent(last_parent));
        }
        let mut note_start = first_parent;
        let mut note_end = last_parent;
        if note_start > 0
            && self.part.notes[note_start - 1].end() == self.part.notes[note_start].position
        {
            note_start -= 1;
        }
        if note_end + 1 < self.part.notes.len()
            && self.part.notes[note_end].end() == self.part.notes[note_end + 1].position
        {
            note_end += 1;
        }
        let phrase_notes = &self.part.notes[note_start..=note_end];

        // Leading ticks: `Math.Max(0, TicksBetweenMsPos(positionMs - leadingMs, positionMs))`.
        let leading_ticks = self
            .project
            .time_axis
            .ticks_between_ms(first.position_ms - first.leading_ms, first.position_ms)
            .max(0);

        // Pitch: flat → vibrato → pitch points → PITD.
        let pitd = self.part.curves.iter().find(|c| c.abbr == PITD);
        let pitch_computer = PitchComputer::new(
            phrase_notes,
            self.part.position,
            &phonemes,
            leading_ticks,
            &self.project.time_axis,
            pitd,
        );
        let pitches = pitch_computer.compute();
        let pitches_cents: Vec<i32> = pitches.iter().map(|&p| p.round() as i32).collect();
        let pitch_start = pitch_computer.pitch_start();
        let length = pitch_computer.length();

        // Curves: every Curve-type expression of the project, sampled on
        // the pitch grid. Missing curves sample to the descriptor default
        // (the reference creates an empty `UCurve(descriptor)`).
        let curves = self.sample_curves(pitch_start, length);

        // Notes (project order, as the reference selects them).
        let mut render_notes = Vec::with_capacity(phrase_notes.len());
        for note in phrase_notes {
            render_notes.push(RenderNote {
                lyric: note.lyric.clone(),
                tone: note.tone,
                position_ms: self
                    .project
                    .time_axis
                    .tick_to_ms((self.part.position + note.position) as f64),
                duration_ms: self.project.time_axis.ms_between_ticks(
                    (self.part.position + note.position) as f64,
                    (self.part.position + note.end()) as f64,
                ),
            });
        }

        // Phonemes: timing + per-phoneme expressions.
        let mut render_phonemes = Vec::with_capacity(phonemes.len());
        for ph in &phonemes {
            let parent = ph
                .parent
                .ok_or(PhraseError::MissingParent(render_phonemes.len()))?;
            if parent >= self.part.notes.len() {
                return Err(PhraseError::MissingParent(parent));
            }
            let note = &self.part.notes[parent];
            let tone_shift = ph
                .get_expression(note, self.project, self.track, SHFT)
                .map(|(v, _)| v as i32)
                .unwrap_or(0);
            render_phonemes.push(RenderPhoneme {
                phoneme: ph.phoneme.clone(),
                position_ms: ph.position_ms,
                duration_ms: ph.duration_ms,
                leading_ms: ph.leading_ms,
                overlap_ms: ph.overlap_ms,
                tone: ph.tone,
                tone_shift,
                index: ph.index,
                parent_note: parent - note_start,
            });
        }

        let sample_based = self.build_sample_based(&phonemes);

        Ok(RenderInput {
            phrase: feed::PhraseInfo {
                position_ms: group.position_ms,
                duration_ms: group.duration_ms,
                leading_ms: first.leading_ms,
                leading_ticks,
                time_axis_hint: self.time_axis_hint(),
            },
            notes: render_notes,
            phonemes: render_phonemes,
            pitches_cents,
            curves,
            sample_based,
            neural: None,
        })
    }

    /// `RenderPhrase` curves block: iterate the project's Curve-type
    /// expressions, sample each on the pitch grid (identity conversion,
    /// except `dyn` → dB to linear), and bucket them into [`Curves`].
    fn sample_curves(&self, pitch_start: i32, length: usize) -> Curves {
        let mut curves = Curves::default();
        let mut extra: Vec<NamedCurve> = Vec::new();
        let mut descriptors: Vec<_> = self.project.expressions.values().collect();
        // HashMap iteration order is random; sort for deterministic output.
        descriptors.sort_by(|a, b| a.abbr.cmp(&b.abbr));
        for descriptor in descriptors {
            if descriptor.r#type != domain::UExpressionType::Curve {
                continue;
            }
            let curve = self.part.curves.iter().find(|c| c.abbr == descriptor.abbr);
            let default_value = descriptor.custom_default_value();
            // dyn converts dB → linear (0 at the descriptor minimum);
            // every other curve is identity.
            let dyn_min = if descriptor.abbr == DYN {
                Some(descriptor.min)
            } else {
                None
            };
            let convert = move |x: f32| match dyn_min {
                Some(min) if x == min => 0.0,
                Some(_) => decibel_to_linear(x as f64 * 0.1) as f32,
                None => x,
            };
            let sampled = match curve {
                Some(c) => sample_curve(c, pitch_start, length, default_value, convert),
                // The reference builds an empty UCurve(descriptor); an
                // empty curve samples to the descriptor default.
                None => sample_curve(
                    &domain::UCurve::new(descriptor.abbr.clone()),
                    pitch_start,
                    length,
                    default_value,
                    convert,
                ),
            };
            match descriptor.abbr.as_str() {
                DYN => curves.dynamics = sampled,
                // PITD is applied inside PitchComputer; excluded here like
                // the reference (`case PITD: break`).
                PITD => {}
                domain::GENC => curves.gender = sampled,
                domain::BREC => curves.breathiness = sampled,
                domain::TENC => curves.tension = sampled,
                domain::VOIC => curves.voicing = sampled,
                _ => extra.push(NamedCurve {
                    abbr: descriptor.abbr.clone(),
                    values: sampled,
                }),
            }
        }
        curves.extra = extra;
        curves
    }

    /// Sample-based section: oto entries (mapped via [`feed::OtoMapper`]),
    /// the first mapped phoneme's envelope and flags. Phonemes without an
    /// oto entry are skipped, like the reference renderer's `oto == null`
    /// handling.
    fn build_sample_based(&self, phonemes: &[UPhoneme]) -> Option<SampleBased> {
        let vb = self.voicebank?;
        let mut oto = Vec::new();
        let mut wav_path = None;
        let mut envelope = Vec::new();
        let mut flags = String::new();
        for ph in phonemes {
            let Some(parent) = ph.parent else { continue };
            let Some(note) = self.part.notes.get(parent) else {
                continue;
            };
            let Some(mut entry) = OtoMapper::map(
                vb,
                &ph.phoneme,
                ph.tone,
                self.voice_color(note, ph).as_deref(),
            ) else {
                continue;
            };
            let env = EnvelopeBuilder::build_ms(ph, note, self.project, self.track);
            let mut flag_parts = ph.flags_as_strings(note, self.project, self.track);
            flag_parts.sort(); // deterministic order; the resampler reads flags positionally
            entry.envelope = env.clone();
            entry.flags = flag_parts.join("");
            if wav_path.is_none() {
                wav_path = Some(entry.wav_path.clone());
                envelope = env;
                flags = entry.flags.clone();
            }
            oto.push(entry);
        }
        if oto.is_empty() {
            return None;
        }
        Some(SampleBased {
            oto,
            wav_path,
            envelope,
            flags,
        })
    }

    /// Voice color (subbank) of a phoneme from the `clr` expression —
    /// the `GetVoiceColor` equivalent. `None` when the descriptor has no
    /// options (the default bank has a single unnamed subbank).
    fn voice_color(&self, note: &UNote, ph: &UPhoneme) -> Option<String> {
        let descriptor = self.track.try_get_exp_descriptor(self.project, CLR)?;
        let options = descriptor.options.as_ref()?;
        if options.is_empty() {
            return None;
        }
        let value = ph.get_expression(note, self.project, self.track, CLR)?.0;
        let index = (value as i32).clamp(0, options.len() as i32 - 1) as usize;
        Some(options[index].clone())
    }

    fn time_axis_hint(&self) -> Option<String> {
        let tempo = self.project.tempos.first()?;
        let sig = self.project.time_signatures.first()?;
        Some(format!(
            "{}bpm {}/{}",
            tempo.bpm, sig.beat_per_bar, sig.beat_unit
        ))
    }
}
