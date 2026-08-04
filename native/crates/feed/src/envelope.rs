//! Envelope building: p1..p5 → ms-space envelope → sample-space envelope.
//!
//! Ports `UPhoneme.ValidateEnvelope` (`Ustx/UPhoneme.cs`) plus the parts
//! of `ValidateOverlap` that feed it (adjacency, tail intrude/overlap),
//! and `ResamplerItem.EnvelopeMsToSamples` (`Classic/ResamplerItem.cs`):
//! points are shifted so p0 sits at 0, scaled `× sample_rate / 1000` and
//! offset by the skip-over (preutter-derived) samples; amplitudes are
//! normalized from 0..100 to 0..1.

use domain::{UNote, UPhoneme, UProject, UTrack, ATK, DEC, VOL};

use crate::render_input::EnvelopePoint;

/// Default sample rate of the sample-based renderers.
pub const DEFAULT_SAMPLE_RATE: f64 = 44100.0;

/// Builds the 5-point envelope of phonemes.
pub struct EnvelopeBuilder;

impl EnvelopeBuilder {
    /// Port of `UPhoneme.ValidateOverlap`: fills adjacency fields and caps
    /// `leading_ms` (preutter) / `overlap_ms` against the previous phoneme,
    /// exactly like the reference. Phonemes without an oto entry (zero
    /// auto preutter/overlap) are left untouched.
    pub fn compute_adjacency(phonemes: &mut [UPhoneme]) {
        for i in 1..phonemes.len() {
            let prev = &phonemes[i - 1];
            if prev.auto_preutter <= 0.0 && prev.auto_overlap == 0.0 {
                continue; // prev unmapped; C# ValidateOverlap returns early
            }
            let (prev_dur, prev_leading, prev_end) =
                (prev.duration_ms, prev.leading_ms, prev.position_ms + prev.duration_ms);
            let cur = &mut phonemes[i];
            let gap_ms = cur.position_ms - prev_end;
            let mut max_preutter = cur.auto_preutter;
            let mut adjacent = false;
            if gap_ms <= 0.0 {
                adjacent = true;
                if cur.auto_overlap > 0.0 {
                    if cur.auto_preutter - cur.auto_overlap > prev_dur * 0.5 {
                        max_preutter =
                            prev_dur * 0.5 / (cur.auto_preutter - cur.auto_overlap)
                                * cur.auto_preutter;
                    }
                } else {
                    // Plosive consonants.
                    max_preutter = max_preutter.min(prev_dur * 0.9);
                }
                max_preutter = max_preutter.min(prev_dur);
                if prev_leading < 5.0 {
                    max_preutter = max_preutter.min(prev_dur + prev_leading - 5.0);
                }
            } else if gap_ms < cur.auto_preutter {
                max_preutter = gap_ms;
            }
            if cur.auto_preutter > max_preutter {
                let ratio = max_preutter / cur.auto_preutter;
                cur.auto_preutter = max_preutter;
                cur.auto_overlap *= ratio;
            }
            if cur.auto_overlap < 0.0 {
                cur.auto_overlap = cur
                    .auto_overlap
                    .max((35.0 - prev_dur + cur.auto_preutter).min(0.0));
            }
            cur.leading_ms = (cur.auto_preutter + cur.preutter_delta.unwrap_or(0.0)).max(0.0);
            cur.overlap_ms = cur.auto_overlap + cur.overlap_delta.unwrap_or(0.0);
            cur.adjacent = adjacent;
            cur.overlapped = adjacent && cur.overlap_ms > 0.0;
            // The reference stores tail intrude/overlap on the *previous*
            // phoneme, which owns the envelope tail.
            let (tail_intrude, tail_overlap) = if adjacent {
                (
                    (cur.leading_ms - cur.overlap_ms).max(cur.leading_ms),
                    cur.overlap_ms.max(0.0),
                )
            } else {
                (0.0, 0.0)
            };
            phonemes[i - 1].tail_intrude = tail_intrude;
            phonemes[i - 1].tail_overlap = tail_overlap;
        }
    }

    /// Port of `UPhoneme.ValidateEnvelope`: the 5 points p0..p4 in ms
    /// space, amplitudes normalized to 0..1. When the phoneme has no oto
    /// entry (all-zero preutter/overlap), the default all-zero-points
    /// envelope is returned (reference behavior for unmapped phonemes).
    pub fn build_ms(
        phoneme: &UPhoneme,
        note: &UNote,
        project: &UProject,
        track: &UTrack,
    ) -> Vec<EnvelopePoint> {
        if phoneme.auto_preutter <= 0.0 && phoneme.auto_overlap == 0.0 {
            return vec![
                EnvelopePoint::new(0.0, 0.0),
                EnvelopePoint::new(0.0, 1.0),
                EnvelopePoint::new(0.0, 1.0),
                EnvelopePoint::new(0.0, 1.0),
                EnvelopePoint::new(0.0, 0.0),
            ];
        }
        let vol = phoneme
            .get_expression(note, project, track, VOL)
            .map(|(v, _)| v)
            .unwrap_or(100.0);
        let atk = phoneme
            .get_expression(note, project, track, ATK)
            .map(|(v, _)| v)
            .unwrap_or(100.0);
        let dec = phoneme
            .get_expression(note, project, track, DEC)
            .map(|(v, _)| v)
            .unwrap_or(0.0);

        let preutter = phoneme.leading_ms;
        let fade_in = if !phoneme.crossfade || !phoneme.overlapped {
            5.0
        } else {
            phoneme.overlap_ms
        };
        let fade_out = if phoneme.tail_overlap > 0.0 && phoneme.crossfade {
            phoneme.tail_overlap
        } else {
            35.0
        };

        let p0x = -preutter;
        let p1x = (p0x + 5.0).max(p0x + fade_in + phoneme.attack_time_delta.unwrap_or(0.0));
        let p2x = p1x.max(0.0);
        let p4x = phoneme.duration_ms - phoneme.tail_intrude + phoneme.tail_overlap;
        let p3x = p2x.max(p4x - fade_out - phoneme.release_time_delta.unwrap_or(0.0));

        let p1y = atk * vol / 100.0;
        let p2y = vol;
        let p3y = vol * (1.0 - dec / 100.0);

        vec![
            EnvelopePoint::new(p0x as f32, 0.0),
            EnvelopePoint::new(p1x as f32, p1y / 100.0),
            EnvelopePoint::new(p2x as f32, p2y / 100.0),
            EnvelopePoint::new(p3x as f32, p3y / 100.0),
            EnvelopePoint::new(p4x as f32, 0.0),
        ]
    }

    /// `ResamplerItem.EnvelopeMsToSamples`: convert an ms-space envelope
    /// (from [`build_ms`](Self::build_ms)) to sample space at
    /// `sample_rate`. The first point is shifted to 0 and `skip_over_ms`
    /// (preutter-derived leading samples) is added.
    pub fn to_samples(
        envelope: &[EnvelopePoint],
        skip_over_ms: f64,
        sample_rate: f64,
    ) -> Vec<(f64, f64)> {
        let skip_over_samples = (skip_over_ms * sample_rate / 1000.0) as i64 as f64;
        let shift = -envelope.first().map(|p| p.x_ms as f64).unwrap_or(0.0);
        envelope
            .iter()
            .map(|p| {
                (
                    ((p.x_ms as f64 + shift) * sample_rate / 1000.0) + skip_over_samples,
                    p.y as f64,
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::UEnvelope;

    fn project_with_expressions() -> UProject {
        let mut p = UProject::new();
        domain::add_default_expressions(&mut p);
        p.time_axis
            .build_segments(&p.time_signatures, &p.tempos)
            .expect("build segments");
        p
    }

    fn phoneme_with_oto(preutter: f64, overlap: f64, duration_ms: f64) -> UPhoneme {
        let mut ph = UPhoneme::new("3");
        ph.auto_preutter = preutter;
        ph.auto_overlap = overlap;
        ph.leading_ms = preutter;
        ph.overlap_ms = overlap;
        ph.duration_ms = duration_ms;
        ph.position_ms = 500.0;
        ph
    }

    fn assert_point(p: &EnvelopePoint, x: f32, y: f32) {
        assert!((p.x_ms - x).abs() < 1e-3, "x: expected {x}, got {}", p.x_ms);
        assert!((p.y - y).abs() < 1e-6, "y: expected {y}, got {}", p.y);
    }

    #[test]
    fn envelope_shape_with_default_expressions() {
        let project = project_with_expressions();
        let track = &project.tracks[0];
        let note = UNote::default();
        // preutter 20, duration 500, vol/atk 100, dec 0, not overlapped.
        let ph = phoneme_with_oto(20.0, 0.0, 500.0);
        let env = EnvelopeBuilder::build_ms(&ph, &note, &project, track);
        assert_eq!(env.len(), 5);
        assert_point(&env[0], -20.0, 0.0);
        assert_point(&env[1], -15.0, 1.0); // max(p0+5, p0+fadeIn)
        assert_point(&env[2], 0.0, 1.0);
        assert_point(&env[3], 465.0, 1.0); // p4 - fadeOut(35)
        assert_point(&env[4], 500.0, 0.0);
    }

    #[test]
    fn unmapped_phoneme_gets_default_envelope() {
        let project = project_with_expressions();
        let track = &project.tracks[0];
        let note = UNote::default();
        let ph = UPhoneme::new("zz"); // no oto -> auto fields are 0
        let env = EnvelopeBuilder::build_ms(&ph, &note, &project, track);
        assert_eq!(env, UEnvelope::default().data.iter().map(|&(x, y)| EnvelopePoint::new(x, y / 100.0)).collect::<Vec<_>>());
    }

    #[test]
    fn adjacency_caps_preutter_and_sets_tail() {
        let project = project_with_expressions();
        let track = &project.tracks[0];
        let note = UNote::default();
        let mut phs = vec![
            phoneme_with_oto(20.0, 0.0, 250.0),
            phoneme_with_oto(250.0, 83.333, 250.0),
        ];
        // Adjacent: second starts exactly where the first ends.
        phs[0].position_ms = 500.0;
        phs[1].position_ms = 750.0;
        EnvelopeBuilder::compute_adjacency(&mut phs);
        // gap 0, autoOverlap 83.333 > 0 → overlap branch (C# ValidateOverlap
        // lines 125-128): 250-83.333=166.667 > 250×0.5=125 →
        // maxPreutter = 125/166.667×250 ≈ 187.4996 (float rounding — C# same).
        assert!((phs[1].leading_ms - 187.5).abs() < 1e-3, "got {}", phs[1].leading_ms);
        assert!(phs[1].adjacent);
        assert!(phs[1].overlapped); // overlap after ratio scaling ≈ 62.5 > 0
        // tailIntrude = max(preutter, preutter - overlap) ≈ 187.5
        assert!((phs[0].tail_intrude - 187.5).abs() < 1e-3);
        // tailOverlap = max(overlap, 0) ≈ 62.5 (scaled by ratio)
        assert!((phs[0].tail_overlap - 62.5).abs() < 1e-3);
        // Envelope tail of the previous phoneme reaches into the next one.
        let env = EnvelopeBuilder::build_ms(&phs[0], &note, &project, track);
        // p4.x = DurationMs - tailIntrude + tailOverlap ≈ 125
        assert_point(&env[4], 125.0, 0.0);
    }

    #[test]
    fn to_samples_shifts_and_scales() {
        let env = vec![
            EnvelopePoint::new(-20.0, 0.0),
            EnvelopePoint::new(-15.0, 1.0),
            EnvelopePoint::new(0.0, 1.0),
            EnvelopePoint::new(465.0, 1.0),
            EnvelopePoint::new(500.0, 0.0),
        ];
        let samples = EnvelopeBuilder::to_samples(&env, 250.0, 44100.0);
        // skipOver 250 ms -> 11025 samples; shift = +20 ms.
        assert_eq!(samples[0].0, 11025.0);
        assert!((samples[1].0 - (5.0 * 44.1 + 11025.0)).abs() < 1e-9);
        assert!((samples[4].0 - (520.0 * 44.1 + 11025.0)).abs() < 1e-9);
        assert_eq!(samples[0].1, 0.0);
        assert_eq!(samples[1].1, 1.0);
    }
}
