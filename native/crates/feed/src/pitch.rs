//! Pitch computation: `note.tone + tuning + vibrato + pitch points + PITD`
//! → `pitches[]` in cents, sampled every 5 ticks.
//!
//! This is a faithful port of the pitch block of
//! `OpenUtau.Core/Render/RenderPhrase.cs` (flat pitches → vibrato → pitch
//! points → PITD), with `UVibrato.Evaluate` from `Ustx/UNote.cs`,
//! `MusicMath.InterpolateShape` and the Catmull-Rom `CubicSplineSegment`
//! from `Util/SplineInterpolate.cs`.
//!
//! The pitch grid starts `leading` ticks before the first phoneme of the
//! phrase and covers the whole phrase, one sample per 5 ticks
//! (`pitchInterval = 5`, the same constant as `UCurve.INTERVAL`).

use domain::{PitchPointShape, TimeAxis, UNote, UPhoneme, UCurve};

use crate::music_math::interpolate_shape;

/// Sampling interval of the pitch grid, in ticks (OpenUtau `pitchInterval`).
pub const PITCH_INTERVAL: i32 = 5;

/// Port of `SplineInterpolate.CubicSplineSegment` (Catmull-Rom).
struct CubicSplineSegment {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    a: f64,
    b: f64,
    c: f64,
    d: f64,
}

impl CubicSplineSegment {
    #[allow(clippy::too_many_arguments)] // mirrors C# SplineInterpolate.cs ctor
    fn new(
        x_1: f64,
        y_1: f64,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
    ) -> Self {
        let m0 = (y1 - y_1) * (x1 - x0) / (x1 - x_1);
        let m1 = (y2 - y0) * (x1 - x0) / (x2 - x0);
        CubicSplineSegment {
            x0,
            y0,
            x1,
            y1,
            a: 2.0 * y0 - 2.0 * y1 + m0 + m1,
            b: -3.0 * y0 + 3.0 * y1 - 2.0 * m0 - m1,
            c: m0,
            d: y0,
        }
    }

    fn get_y(&self, x: f64) -> f64 {
        if x <= self.x0 {
            return self.y0;
        }
        if x >= self.x1 {
            return self.y1;
        }
        let t = (x - self.x0) / (self.x1 - self.x0);
        ((self.a * t + self.b) * t + self.c) * t + self.d
    }
}

/// A pitch point converted to the phrase coordinate system: `x` in
/// part-relative ticks, `y` in cents, plus whether the point was
/// auto-completed by the reference algorithm (leading/trailing anchors).
#[derive(Debug, Clone, Copy)]
struct PhrasePitchPoint {
    x: i32,
    y: f32,
    shape: PitchPointShape,
    auto_completed: bool,
}

/// Port of `UVibrato.Evaluate` (`UNote.cs`): pitch offset in *notes* at a
/// normalized position `n_pos` of the note length.
fn vibrato_evaluate(
    vibrato: &domain::UVibrato,
    n_pos: f32,
    n_period: f32,
    adjusted_tone: f32,
) -> f32 {
    let n_start = vibrato.normalized_start();
    let n_in = vibrato.length / 100.0 * vibrato.r#in / 100.0;
    let n_in_pos = n_start + n_in;
    let n_out = vibrato.length / 100.0 * vibrato.out / 100.0;
    let n_out_pos = 1.0 - n_out;
    let t = (n_pos - n_start) / n_period + vibrato.shift / 100.0;
    let mut y = (2.0 * std::f32::consts::PI * t).sin() * vibrato.depth
        + (vibrato.depth / 100.0 * vibrato.drift);
    if n_pos < n_start {
        y = 0.0;
    } else if n_pos < n_in_pos {
        y *= (n_pos - n_start) / n_in;
    } else if n_pos > n_out_pos {
        y *= (1.0 - n_pos) / n_out;
    }
    adjusted_tone + y / 100.0
}

/// Computes the per-5-tick pitch array (cents) for one phrase.
///
/// * `notes` — all notes of the phrase (part-relative ticks).
/// * `part_position` — the containing part's position in project ticks.
/// * `phonemes` — the derived phonemes (part-relative ticks; first/last
///   define the phrase span).
/// * `leading` — leading ticks of the phrase (first phoneme preutter).
/// * `time_axis` — project time axis (must be built).
/// * `pitd` — the part's `pitd` curve, if any (applied as a deviation).
pub struct PitchComputer<'a> {
    notes: &'a [UNote],
    part_position: i32,
    first_phoneme_position: i32,
    last_phoneme_end: i32,
    leading: i32,
    time_axis: &'a TimeAxis,
    pitd: Option<&'a UCurve>,
}

impl<'a> PitchComputer<'a> {
    pub fn new(
        notes: &'a [UNote],
        part_position: i32,
        phonemes: &[UPhoneme],
        leading: i32,
        time_axis: &'a TimeAxis,
        pitd: Option<&'a UCurve>,
    ) -> Self {
        let first = phonemes.first().expect("phrase must have phonemes");
        let last = phonemes.last().expect("phrase must have phonemes");
        PitchComputer {
            notes,
            part_position,
            first_phoneme_position: first.position,
            last_phoneme_end: last.end(),
            leading,
            time_axis,
            pitd,
        }
    }

    /// Start tick of the pitch grid, relative to the part.
    pub fn pitch_start(&self) -> i32 {
        self.first_phoneme_position - self.leading
    }

    /// Number of samples of the pitch grid.
    pub fn length(&self) -> usize {
        ((self.last_phoneme_end - self.pitch_start()) / PITCH_INTERVAL + 1).max(0) as usize
    }

    /// The full pitch array in cents.
    pub fn compute(&self) -> Vec<f32> {
        let pitch_start = self.pitch_start();
        let len = self.length();
        let mut pitches = vec![0.0f32; len];

        // 1. Flat pitches: each note's adjusted tone (cents) across its span.
        let mut index = 0usize;
        for note in self.notes {
            while pitch_start + (index as i32) * PITCH_INTERVAL < note.end()
                && index < pitches.len()
            {
                pitches[index] = note.adjusted_tone() * 100.0;
                index += 1;
            }
        }
        index = index.max(1);
        while index < pitches.len() {
            pitches[index] = pitches[index - 1];
            index += 1;
        }

        // 2. Vibrato (skipped when length is 0, like the reference).
        for note in self.notes {
            if note.vibrato.length <= 0.0 {
                continue;
            }
            let start_index = ((note.position - pitch_start) as f32 / PITCH_INTERVAL as f32)
                .ceil()
                .max(0.0) as usize;
            let end_index = (((note.end() - pitch_start) / PITCH_INTERVAL) as usize).min(pitches.len());
            // Use the tempo at note start to calculate the vibrato period.
            let note_duration_ms = self
                .time_axis
                .ms_between_ticks(
                    (self.part_position + note.position) as f64,
                    (self.part_position + note.end()) as f64,
                );
            let n_period = note.vibrato.period / note_duration_ms as f32;
            for (i, pitch) in pitches.iter_mut().enumerate().take(end_index).skip(start_index) {
                let n_pos = (pitch_start + i as i32 * PITCH_INTERVAL - note.position) as f32
                    / note.duration as f32;
                let point = vibrato_evaluate(&note.vibrato, n_pos, n_period, note.adjusted_tone());
                *pitch = point * 100.0;
            }
        }

        // 3. Pitch points (portamento / manual pitch), shape-interpolated.
        for (note_idx, note) in self.notes.iter().enumerate() {
            let prev_note = note_idx.checked_sub(1).and_then(|i| self.notes.get(i));
            self.apply_pitch_points(note, prev_note, &mut pitches);
        }

        // 4. PITD curve deviation.
        if let Some(curve) = self.pitd {
            if !curve.is_empty() {
                for (i, pitch) in pitches.iter_mut().enumerate() {
                    *pitch += curve.sample(pitch_start + i as i32 * PITCH_INTERVAL).unwrap_or(0) as f32;
                }
            }
        }

        pitches
    }

    /// Port of the "Pitch points" block of `RenderPhrase` for one note.
    fn apply_pitch_points(&self, note: &UNote, prev_note: Option<&UNote>, pitches: &mut [f32]) {
        let pitch_start = self.pitch_start();
        let part_position = self.part_position;
        let adjusted_cents = note.adjusted_tone() * 100.0;

        // Convert note-relative ms points to part-relative ticks + cents.
        let mut points: Vec<PhrasePitchPoint> = note
            .pitch
            .data
            .iter()
            .map(|point| {
                let node_pos_ms = self.time_axis.tick_to_ms((part_position + note.position) as f64);
                PhrasePitchPoint {
                    x: self.time_axis.ms_to_tick(node_pos_ms + point.x as f64) - part_position,
                    y: point.y * 10.0 + adjusted_cents,
                    shape: point.shape,
                    auto_completed: false,
                }
            })
            .collect();
        if points.is_empty() {
            points.push(PhrasePitchPoint {
                x: note.position,
                y: adjusted_cents,
                shape: PitchPointShape::Io,
                auto_completed: true,
            });
            points.push(PhrasePitchPoint {
                x: note.end(),
                y: adjusted_cents,
                shape: PitchPointShape::Io,
                auto_completed: true,
            });
        }
        let is_first_note = self.notes.first().is_some_and(|n| std::ptr::eq(n, note));
        if is_first_note && points[0].x > pitch_start {
            points.insert(
                0,
                PhrasePitchPoint {
                    x: pitch_start,
                    y: points[0].y,
                    shape: PitchPointShape::Io,
                    auto_completed: true,
                },
            );
        } else if points[0].x > note.position {
            points.insert(
                0,
                PhrasePitchPoint {
                    x: note.position,
                    y: points[0].y,
                    shape: PitchPointShape::Io,
                    auto_completed: true,
                },
            );
        }
        if points.last().is_some_and(|p| p.x < note.end()) {
            let last_y = points.last().expect("points is non-empty").y;
            points.push(PhrasePitchPoint {
                x: note.end(),
                y: last_y,
                shape: PitchPointShape::Io,
                auto_completed: true,
            });
        }

        let mut index = (((points[0].x - pitch_start) as f64) / PITCH_INTERVAL as f64).max(0.0)
            as usize;
        for i in 0..points.len().saturating_sub(1) {
            let point_1 = if i == 0 { points[i] } else { points[i - 1] };
            let point0 = points[i];
            let point1 = points[i + 1];
            let point2 = if i >= points.len() - 2 { points[i + 1] } else { points[i + 2] };
            let mut x = pitch_start + index as i32 * PITCH_INTERVAL;

            let spline = note.pitch.data.len() > 2 && point0.shape == PitchPointShape::Sp
                && !point1.auto_completed;
            while x < point1.x && index < pitches.len() {
                let pitch = if spline {
                    CubicSplineSegment::new(
                        point_1.x as f64,
                        point_1.y as f64,
                        point0.x as f64,
                        point0.y as f64,
                        point1.x as f64,
                        point1.y as f64,
                        point2.x as f64,
                        point2.y as f64,
                    )
                    .get_y(x as f64)
                } else {
                    interpolate_shape(
                        point0.x as f64,
                        point1.x as f64,
                        point0.y as f64,
                        point1.y as f64,
                        x as f64,
                        point0.shape,
                    )
                } as f32;
                let base_pitch = if prev_note.is_some_and(|p| x < p.end()) {
                    prev_note.expect("prev_note is some").adjusted_tone() * 100.0
                } else {
                    adjusted_cents
                };
                pitches[index] += pitch - base_pitch;
                index += 1;
                x += PITCH_INTERVAL;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{UTempo, UTimeSignature, UVibrato, UPitch};

    fn axis_120bpm() -> TimeAxis {
        let mut axis = TimeAxis::default();
        axis.build_segments(&[UTimeSignature::new(0, 4, 4)], &[UTempo::new(0, 120.0)])
            .expect("build segments");
        axis
    }

    fn note(position: i32, duration: i32, tone: i32) -> UNote {
        UNote { position, duration, tone, ..Default::default() }
    }

    fn phoneme(position: i32, duration: i32) -> UPhoneme {
        UPhoneme { position, raw_position: position, duration, ..Default::default() }
    }

    fn computer<'a>(
        notes: &'a [UNote],
        phonemes: &[UPhoneme],
        axis: &'a TimeAxis,
    ) -> PitchComputer<'a> {
        PitchComputer::new(notes, 0, phonemes, 0, axis, None)
    }

    #[test]
    fn flat_pitches_cover_note_span() {
        let axis = axis_120bpm();
        let notes = [note(0, 480, 60)];
        let phs = [phoneme(0, 480)];
        let pitches = computer(&notes, &phs, &axis).compute();
        // 480 ticks / 5 + 1 = 97 samples, all 6000 cents (C4).
        assert_eq!(pitches.len(), 97);
        assert!(pitches.iter().all(|&p| p == 6000.0));
    }

    #[test]
    fn two_notes_step_up_at_boundary() {
        let axis = axis_120bpm();
        let notes = [note(0, 480, 60), note(480, 480, 62)];
        let phs = [phoneme(0, 960)];
        let pitches = computer(&notes, &phs, &axis).compute();
        assert_eq!(pitches.len(), 193); // 960/5 + 1
        assert!(pitches[..96].iter().all(|&p| p == 6000.0));
        assert!(pitches[96..].iter().all(|&p| p == 6200.0));
    }

    #[test]
    fn portamento_starts_five_ticks_before_note() {
        // Note 2 at tick 960 with the default portamento points (-1ms, 1ms,
        // both y=0): the interpolation between the anchor points starts at
        // sample index (959-480)/5 = 95, i.e. tick 955, 5 ticks before the
        // note start, and rises from the previous note's base pitch.
        let axis = axis_120bpm();
        let mut note2 = note(960, 480, 62);
        note2.pitch = UPitch {
            data: vec![
                domain::PitchPoint::new(-1.0, 0.0, PitchPointShape::Io),
                domain::PitchPoint::new(1.0, 0.0, PitchPointShape::Io),
            ],
            snap_first: true,
        };
        let notes = [note(480, 480, 60), note2];
        let phs = [phoneme(480, 960)];
        let pitches = computer(&notes, &phs, &axis).compute();
        assert_eq!(pitches[95], 6200.0); // x=955 >= 959? no: 955 < 959 -> interpolated
        assert_eq!(pitches[94], 6000.0); // still the previous note
        assert!(pitches[96..].iter().all(|&p| p == 6200.0));
    }

    #[test]
    fn vibrato_oscillates_around_adjusted_tone() {
        let axis = axis_120bpm();
        let mut v = UVibrato::default();
        v.set_length(100.0);
        v.set_depth(25.0);
        v.set_period(175.0);
        let mut n = note(0, 480, 60);
        n.vibrato = v;
        let notes = [n];
        let phs = [phoneme(0, 480)];
        let pitches = computer(&notes, &phs, &axis).compute();
        let min = pitches.iter().cloned().fold(f32::INFINITY, f32::min);
        let max = pitches.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        assert!(min < 6000.0 && max > 6000.0, "min {min} max {max}");
        // Depth is 25 ct; the samples land near (not exactly on) the peak
        // because the grid is discrete (5 ticks = 5.2 ms at 120 bpm).
        assert!((max - 6000.0 - 25.0).abs() < 0.1, "peak {max}");
        assert!((6000.0 - min - 25.0).abs() < 0.1, "trough {min}");
        // Fade-in: the first sample is near the base tone.
        assert!((pitches[0] - 6000.0).abs() < 1.0);
        // Fade-out: the last sample is back near the base tone.
        assert!((pitches[96] - 6000.0).abs() < 1.0);
    }

    #[test]
    fn pitd_curve_shifts_pitches() {
        let axis = axis_120bpm();
        let notes = [note(0, 480, 60)];
        let phs = [phoneme(0, 480)];
        let curve = UCurve { abbr: "pitd".into(), xs: vec![0, 480], ys: vec![0, 100] };
        let pitches = PitchComputer::new(&notes, 0, &phs, 0, &axis, Some(&curve)).compute();
        assert_eq!(pitches[0], 6000.0);
        assert_eq!(pitches[96], 6100.0); // +100 cents at the end
        assert_eq!(pitches[48], 6050.0); // linear midpoint
    }

    #[test]
    fn leading_shifts_grid_start() {
        let axis = axis_120bpm();
        let notes = [note(0, 480, 60)];
        let phs = [phoneme(0, 480)];
        // 240 ticks of leading = 250 ms preutter at 120 bpm.
        let pc = PitchComputer::new(&notes, 0, &phs, 240, &axis, None);
        assert_eq!(pc.pitch_start(), -240);
        assert_eq!(pc.length(), 145); // (480+240)/5 + 1
        let pitches = pc.compute();
        assert!(pitches.iter().all(|&p| p == 6000.0));
    }

    #[test]
    fn spline_shape_with_more_than_two_points() {
        let axis = axis_120bpm();
        let mut n = note(0, 960, 60);
        n.pitch = UPitch {
            data: vec![
                domain::PitchPoint::new(-40.0, 0.0, PitchPointShape::Sp),
                domain::PitchPoint::new(200.0, 50.0, PitchPointShape::Sp),
                domain::PitchPoint::new(400.0, 0.0, PitchPointShape::Sp),
                domain::PitchPoint::new(1000.0, 0.0, PitchPointShape::Sp),
            ],
            snap_first: true,
        };
        let notes = [n];
        let phs = [phoneme(0, 960)];
        let pitches = computer(&notes, &phs, &axis).compute();
        // The Catmull-Rom spline raises the pitch toward the 6500 ct peak
        // around x = 200 ms and comes back; it may overshoot slightly.
        // It also dips below the 6000 ct base between the start and the
        // first control point (Catmull-Rom overshoot — same as C#).
        let max = pitches.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let min = pitches.iter().cloned().fold(f32::INFINITY, f32::min);
        assert!(max > 6400.0 && max < 6600.0, "max {max}");
        assert!(min >= 5900.0, "min {min}"); // overshoot dip < 100 ct below base
    }
}
