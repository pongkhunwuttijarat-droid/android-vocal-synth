//! Timing engine: converts note-relative phonemes to part-relative ticks
//! and milliseconds.
//!
//! Phonemizers ([`crate::Phonemizer`]) emit tick positions **relative to
//! the parent note**. [`TimingEngine`] resolves them against the part:
//!
//! 1. adds the parent note's part-relative position, giving part-relative
//!    ticks (`position` / `raw_position`), exactly like
//!    `OpenUtau.Core/Ustx/UPhoneme.cs` where
//!    `rawPosition = phonemizerOutput.position`;
//! 2. converts to milliseconds with the project [`TimeAxis`]
//!    (`PositionMs = timeAxis.TickPosToMsPos(part.position + position)`,
//!    `DurationMs = EndMs - PositionMs`);
//! 3. fills effective `leading_ms` / `overlap_ms` from the oto entry,
//!    velocity stretch and OpenUtau's adjacent-phoneme constraints
//!    (`UPhoneme.ValidateOverlap`).

use domain::{TimeAxis, UNote, UPhoneme};
use voicebank::Voicebank;

/// Applies part-relative tick and millisecond timing to phonemes.
pub struct TimingEngine;

impl TimingEngine {
    /// Resolve `phonemes` in place.
    ///
    /// * `notes` — the notes the phonemes were derived from (same slice
    ///   passed to [`crate::Phonemizer::process`]); each phoneme's
    ///   `parent` indexes into it.
    /// * `part_position` — the containing part's position in project
    ///   ticks (the part's `position` field).
    /// * `time_axis` — the project time axis (already built).
    /// * `singer` — voicebank used to look up each resolved phoneme's oto
    ///   entry for preutter/overlap.
    ///
    /// Phonemes without a valid `parent` are left untouched.
    pub fn process(
        &self,
        notes: &[UNote],
        part_position: i32,
        phonemes: &mut [UPhoneme],
        time_axis: &TimeAxis,
        singer: Option<&Voicebank>,
    ) {
        for phoneme_index in 0..phonemes.len() {
            let (before, current_and_after) = phonemes.split_at_mut(phoneme_index);
            let ph = &mut current_and_after[0];
            let Some(parent) = ph.parent else { continue };
            let Some(note) = notes.get(parent) else {
                continue;
            };

            // Note-relative -> part-relative ticks.
            let part_tick = note.position + ph.position;
            ph.raw_position = part_tick;
            ph.position = part_tick;

            // Ticks -> milliseconds (position relative to the project).
            let start_tick = part_position + part_tick;
            let end_tick = start_tick + ph.duration;
            ph.position_ms = time_axis.tick_to_ms(start_tick as f64);
            ph.duration_ms = time_axis.ms_between_ticks(start_tick as f64, end_tick as f64);

            // OpenUtau UPhoneme.ValidateOverlap: scale OTO timing by VEL,
            // then constrain it against the preceding phoneme before storing
            // the effective preutter used by phrase placement.
            if let Some(vb) = singer {
                if let Some(oto) = vb.lookup(&ph.phoneme, note.tone) {
                    let velocity = note
                        .phoneme_expressions
                        .iter()
                        .find(|expression| {
                            expression.abbr == domain::expression::VEL
                                && expression.index == Some(ph.index)
                        })
                        .map(|expression| expression.value)
                        .unwrap_or(100.0);
                    let consonant_stretch = 2.0_f64.powf(f64::from(1.0_f32 - velocity / 100.0_f32));
                    let mut auto_preutter = oto.preutter * consonant_stretch;
                    let mut auto_overlap = oto.overlap * consonant_stretch;
                    ph.max_oto_preutter = auto_preutter;
                    ph.adjacent = false;
                    ph.tail_intrude = 0.0;
                    ph.tail_overlap = 0.0;

                    if let Some(previous) = before.last() {
                        let gap_ms = ph.position_ms - (previous.position_ms + previous.duration_ms);
                        let previous_duration_ms = previous.duration_ms;
                        let mut max_preutter = auto_preutter;
                        if gap_ms <= 0.0 {
                            ph.adjacent = true;
                            if auto_overlap > 0.0 {
                                if auto_preutter - auto_overlap > previous_duration_ms * 0.5 {
                                    max_preutter = previous_duration_ms * 0.5
                                        / (auto_preutter - auto_overlap)
                                        * auto_preutter;
                                }
                            } else {
                                max_preutter = max_preutter.min(previous_duration_ms * 0.9);
                            }
                            max_preutter = max_preutter.min(previous_duration_ms);
                            if previous.leading_ms < 5.0 {
                                max_preutter = max_preutter
                                    .min(previous_duration_ms + previous.leading_ms - 5.0);
                            }
                        } else if gap_ms < auto_preutter {
                            max_preutter = gap_ms;
                        }
                        if auto_preutter > max_preutter {
                            let ratio = max_preutter / auto_preutter;
                            auto_preutter = max_preutter;
                            auto_overlap *= ratio;
                        }
                        if auto_overlap < 0.0 {
                            auto_overlap = auto_overlap
                                .max((35.0 - previous_duration_ms + auto_preutter).min(0.0));
                        }
                    }

                    ph.auto_preutter = auto_preutter;
                    ph.auto_overlap = auto_overlap;
                    ph.leading_ms = (auto_preutter + ph.preutter_delta.unwrap_or(0.0)).max(0.0);
                    ph.overlap_ms = auto_overlap + ph.overlap_delta.unwrap_or(0.0);
                    ph.overlapped = ph.adjacent && ph.overlap_ms > 0.0;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::english::fixtures::en_vccv_voicebank;
    use domain::{UTempo, UTimeSignature};

    /// 120 bpm time axis: a quarter note (480 ticks) is exactly 500 ms.
    fn axis_120bpm() -> TimeAxis {
        let mut axis = TimeAxis::default();
        axis.build_segments(&[UTimeSignature::new(0, 4, 4)], &[UTempo::new(0, 120.0)])
            .expect("build segments");
        axis
    }

    fn note(position: i32, duration: i32, tone: i32) -> UNote {
        UNote {
            position,
            duration,
            tone,
            ..Default::default()
        }
    }

    fn phoneme(alias: &str, position: i32, duration: i32, parent: usize) -> UPhoneme {
        UPhoneme {
            raw_phoneme: alias.to_string(),
            phoneme: alias.to_string(),
            position,
            raw_position: position,
            duration,
            parent: Some(parent),
            ..Default::default()
        }
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn quarter_note_at_120bpm_is_500ms() {
        let axis = axis_120bpm();
        let notes = [note(0, 480, 60)];
        let mut phs = [phoneme("a", 0, 480, 0)];
        TimingEngine.process(&notes, 0, &mut phs, &axis, None);
        assert_eq!(phs[0].position, 0);
        assert_eq!(phs[0].raw_position, 0);
        assert_close(phs[0].position_ms, 0.0);
        assert_close(phs[0].duration_ms, 500.0);
    }

    #[test]
    fn phoneme_position_is_note_relative_plus_part() {
        let axis = axis_120bpm();
        // Note starts at part tick 480; phoneme at note-relative tick 240
        // with 240 ticks duration -> part tick 720, 750 ms in.
        let notes = [note(480, 480, 60)];
        let mut phs = [phoneme("a", 240, 240, 0)];
        TimingEngine.process(&notes, 0, &mut phs, &axis, None);
        assert_eq!(phs[0].position, 720);
        assert_eq!(phs[0].raw_position, 720);
        assert_close(phs[0].position_ms, 750.0);
        assert_close(phs[0].duration_ms, 250.0);
    }

    #[test]
    fn part_position_offsets_project_ms() {
        let axis = axis_120bpm();
        let notes = [note(0, 480, 60)];
        let mut phs = [phoneme("a", 0, 480, 0)];
        // Part starts at project tick 960 (= 1000 ms at 120 bpm).
        TimingEngine.process(&notes, 960, &mut phs, &axis, None);
        assert_close(phs[0].position_ms, 1000.0);
        assert_close(phs[0].duration_ms, 500.0);
    }

    #[test]
    fn leading_and_overlap_come_from_oto() {
        let axis = axis_120bpm();
        let vb = en_vccv_voicebank();
        let notes = [note(0, 480, 60)];
        // "-h@" in the en_vccv CV set: preutter 90, overlap 45.
        let mut phs = [phoneme("-h@", 0, 480, 0)];
        TimingEngine.process(&notes, 0, &mut phs, &axis, Some(&vb));
        assert_close(phs[0].leading_ms, 90.0);
        assert_close(phs[0].overlap_ms, 45.0);
        assert_close(phs[0].auto_preutter, 90.0);
        assert_close(phs[0].auto_overlap, 45.0);
        assert_close(phs[0].max_oto_preutter, 90.0);
    }

    #[test]
    fn velocity_stretches_effective_preutter_and_overlap() {
        let axis = axis_120bpm();
        let vb = en_vccv_voicebank();
        let mut note = note(0, 480, 60);
        note.phoneme_expressions.push(domain::UExpression {
            index: Some(0),
            abbr: domain::expression::VEL.to_string(),
            value: 150.0,
        });
        let notes = [note];
        // -h@ has OTO preutter 90 / overlap 45. OpenUtau applies
        // 2^(1 - VEL/100) before constructing the phrase origin.
        let mut phs = [phoneme("-h@", 0, 480, 0)];
        TimingEngine.process(&notes, 0, &mut phs, &axis, Some(&vb));
        let scale = 2.0_f64.powf(-0.5);
        assert_close(phs[0].leading_ms, 90.0 * scale);
        assert_close(phs[0].overlap_ms, 45.0 * scale);
        assert_close(phs[0].max_oto_preutter, 90.0 * scale);
    }

    #[test]
    fn leading_overlap_use_tone_mapped_alias() {
        let axis = axis_120bpm();
        let vb = en_vccv_voicebank();
        // Tone 72 (C5) maps "a r" to the high-bank alias "a r_H", whose
        // oto entry is preutter 300 / overlap 150.
        let notes = [note(0, 480, 72)];
        let mut phs = [phoneme("a r", 0, 480, 0)];
        TimingEngine.process(&notes, 0, &mut phs, &axis, Some(&vb));
        assert_eq!(vb.lookup("a r", 72).unwrap().alias, "a r_H");
        assert_close(phs[0].leading_ms, 300.0);
        assert_close(phs[0].overlap_ms, 150.0);
    }

    #[test]
    fn unknown_alias_has_zero_leading_overlap() {
        let axis = axis_120bpm();
        let vb = en_vccv_voicebank();
        let notes = [note(0, 480, 60)];
        let mut phs = [phoneme("zz", 0, 480, 0)];
        TimingEngine.process(&notes, 0, &mut phs, &axis, Some(&vb));
        assert_close(phs[0].leading_ms, 0.0);
        assert_close(phs[0].overlap_ms, 0.0);
        // Tick/ms fields still computed without an oto entry.
        assert_close(phs[0].duration_ms, 500.0);
    }

    #[test]
    fn phonemes_without_parent_are_skipped() {
        let axis = axis_120bpm();
        let notes = [note(0, 480, 60)];
        let mut phs = [UPhoneme {
            phoneme: "a".to_string(),
            position: 42,
            duration: 100,
            parent: None,
            ..Default::default()
        }];
        TimingEngine.process(&notes, 0, &mut phs, &axis, None);
        assert_eq!(phs[0].position, 42);
        assert_close(phs[0].position_ms, 0.0);

        let mut phs = [UPhoneme {
            phoneme: "a".to_string(),
            position: 42,
            duration: 100,
            parent: Some(5), // out of range
            ..Default::default()
        }];
        TimingEngine.process(&notes, 0, &mut phs, &axis, None);
        assert_eq!(phs[0].position, 42);
    }

    #[test]
    fn multi_phoneme_note_timing() {
        let axis = axis_120bpm();
        let notes = [note(0, 480, 60)];
        // Two phonemes of one note: "a t" at 0..240, "d" at 240..480.
        let mut phs = [phoneme("a t", 0, 240, 0), phoneme("d", 240, 240, 0)];
        TimingEngine.process(&notes, 0, &mut phs, &axis, None);
        assert_close(phs[0].position_ms, 0.0);
        assert_close(phs[0].duration_ms, 250.0);
        assert_close(phs[1].position_ms, 250.0);
        assert_close(phs[1].duration_ms, 250.0);
        assert_eq!(phs[1].position, 240);
    }
}
