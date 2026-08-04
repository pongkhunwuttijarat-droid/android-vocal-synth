//! Tick ↔ time conversion engine (`TimeAxis`).
//!
//! Mirrors `OpenUtau.Core/Util/TimeAxis.cs` including segment construction
//! order, zero-bpm forward fill, and banker's rounding in `ms_to_tick`.
//! All segment lookups use the same `first segment where
//! `pos == value || end > value`` linear scan as the reference.

use crate::csharp_round;
use crate::project::{UTempo, UTimeSignature};

/// Time-signature segment: a span of bars/ticks sharing one time signature.
#[derive(Debug, Clone, PartialEq)]
struct TimeSigSegment {
    bar_pos: i32,
    bar_end: i32,
    tick_pos: i32,
    tick_end: i32,
    beat_per_bar: i32,
    beat_unit: i32,
    ticks_per_bar: i32,
    ticks_per_beat: i32,
}

/// Tempo segment: a span of ticks sharing one tempo.
#[derive(Debug, Clone, PartialEq)]
struct TempoSegment {
    tick_pos: i32,
    tick_end: i32,
    bpm: f64,
    beat_per_bar: i32,
    beat_unit: i32,
    ms_pos: f64,
    ms_end: f64,
    ms_per_tick: f64,
    ticks_per_ms: f64,
}

impl TempoSegment {
    fn ticks(&self) -> i32 {
        self.tick_end - self.tick_pos
    }
}

/// Converts between ticks and milliseconds given a project's tempo and time
/// signature lists.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TimeAxis {
    time_sig_segments: Vec<TimeSigSegment>,
    tempo_segments: Vec<TempoSegment>,
}

impl TimeAxis {
    /// Rebuild the segment lists from `time_signatures` and `tempos`. The
    /// caller is responsible for keeping the lists sorted (see
    /// `UProject::validate`). Fails when the first time signature is not
    /// at bar 0 or the first tempo is not at tick 0, exactly like the
    /// reference implementation.
    pub fn build_segments(
        &mut self,
        time_signatures: &[UTimeSignature],
        tempos: &[UTempo],
    ) -> Result<(), String> {
        const RES: i32 = crate::RESOLUTION;

        self.time_sig_segments.clear();
        for (i, timesig) in time_signatures.iter().enumerate() {
            let pos_tick = if i > 0 {
                let last = self.time_sig_segments.last().expect("segment exists");
                let last_bar_pos = time_signatures[i - 1].bar_position;
                last.tick_pos + last.ticks_per_bar * (timesig.bar_position - last_bar_pos)
            } else {
                if timesig.bar_position != 0 {
                    return Err("First time signature must be at bar 0.".to_string());
                }
                0
            };
            self.time_sig_segments.push(TimeSigSegment {
                bar_pos: timesig.bar_position,
                bar_end: i32::MAX,
                tick_pos: pos_tick,
                tick_end: i32::MAX,
                beat_per_bar: timesig.beat_per_bar,
                beat_unit: timesig.beat_unit,
                ticks_per_bar: RES * 4 * timesig.beat_per_bar / timesig.beat_unit,
                ticks_per_beat: RES * 4 / timesig.beat_unit,
            });
        }
        for i in 0..self.time_sig_segments.len().saturating_sub(1) {
            self.time_sig_segments[i].bar_end = self.time_sig_segments[i + 1].bar_pos;
            self.time_sig_segments[i].tick_end = self.time_sig_segments[i + 1].tick_pos;
        }

        self.tempo_segments.clear();
        self.tempo_segments.extend(self.time_sig_segments.iter().map(|sigseg| TempoSegment {
            tick_pos: sigseg.tick_pos,
            tick_end: i32::MAX,
            bpm: 0.0,
            beat_per_bar: sigseg.beat_per_bar,
            beat_unit: sigseg.beat_unit,
            ms_pos: 0.0,
            ms_end: f64::MAX,
            ms_per_tick: 0.0,
            ticks_per_ms: 0.0,
        }));
        for (i, tempo) in tempos.iter().enumerate() {
            if i == 0 && tempo.position != 0 {
                return Err("First tempo must be at tick 0.".to_string());
            }
            let index = self
                .tempo_segments
                .iter()
                .position(|seg| seg.tick_pos >= tempo.position);
            match index {
                None => {
                    let last = self.tempo_segments.last().expect("segment exists");
                    self.tempo_segments.push(TempoSegment {
                        tick_pos: tempo.position,
                        tick_end: i32::MAX,
                        bpm: tempo.bpm,
                        beat_per_bar: last.beat_per_bar,
                        beat_unit: last.beat_unit,
                        ms_pos: 0.0,
                        ms_end: f64::MAX,
                        ms_per_tick: 0.0,
                        ticks_per_ms: 0.0,
                    });
                }
                Some(index) if self.tempo_segments[index].tick_pos == tempo.position => {
                    self.tempo_segments[index].bpm = tempo.bpm;
                }
                Some(index) => {
                    let prev = &self.tempo_segments[index - 1];
                    self.tempo_segments.insert(
                        index,
                        TempoSegment {
                            tick_pos: tempo.position,
                            tick_end: i32::MAX,
                            bpm: tempo.bpm,
                            beat_per_bar: prev.beat_per_bar,
                            beat_unit: prev.beat_unit,
                            ms_pos: 0.0,
                            ms_end: f64::MAX,
                            ms_per_tick: 0.0,
                            ticks_per_ms: 0.0,
                        },
                    );
                }
            }
        }
        for i in 0..self.tempo_segments.len().saturating_sub(1) {
            if self.tempo_segments[i + 1].bpm == 0.0 {
                self.tempo_segments[i + 1].bpm = self.tempo_segments[i].bpm;
            }
            self.tempo_segments[i].tick_end = self.tempo_segments[i + 1].tick_pos;
        }
        for i in 0..self.tempo_segments.len() {
            if i > 0 {
                let prev = self.tempo_segments[i - 1].clone();
                let ms_pos = prev.ms_pos + prev.ticks() as f64 * prev.ms_per_tick;
                self.tempo_segments[i].ms_pos = ms_pos;
            }
            let seg = &mut self.tempo_segments[i];
            seg.ms_per_tick = 60.0 * 1000.0 / (seg.bpm * crate::RESOLUTION as f64);
            seg.ticks_per_ms = seg.bpm * crate::RESOLUTION as f64 / (60.0 * 1000.0);
        }
        for i in 0..self.tempo_segments.len().saturating_sub(1) {
            let ms_pos = self.tempo_segments[i + 1].ms_pos;
            self.tempo_segments[i].ms_end = ms_pos;
        }
        Ok(())
    }

    fn tempo_segment_at(&self, tick: i32) -> &TempoSegment {
        self.tempo_segments
            .iter()
            .find(|seg| seg.tick_pos == tick || seg.tick_end > tick)
            .unwrap_or_else(|| {
                panic!("time axis has no tempo segments; call build_segments first")
            })
    }

    fn tempo_segment_at_ms(&self, ms: f64) -> &TempoSegment {
        self.tempo_segments
            .iter()
            .find(|seg| seg.ms_pos == ms || seg.ms_end > ms)
            .unwrap_or_else(|| {
                panic!("time axis has no tempo segments; call build_segments first")
            })
    }

    fn time_sig_segment_at_tick(&self, tick: i32) -> &TimeSigSegment {
        self.time_sig_segments
            .iter()
            .find(|seg| seg.tick_pos == tick || seg.tick_end > tick)
            .unwrap_or_else(|| {
                panic!("time axis has no time signature segments; call build_segments first")
            })
    }

    fn time_sig_segment_at_bar(&self, bar: i32) -> &TimeSigSegment {
        self.time_sig_segments
            .iter()
            .find(|seg| seg.bar_pos == bar || seg.bar_end > bar)
            .unwrap_or_else(|| {
                panic!("time axis has no time signature segments; call build_segments first")
            })
    }

    /// `GetBpmAtTick`.
    pub fn bpm_at_tick(&self, tick: i32) -> f64 {
        self.tempo_segment_at(tick).bpm
    }

    /// `TickPosToMsPos`.
    pub fn tick_to_ms(&self, tick: f64) -> f64 {
        let seg = self.tempo_segment_at(tick as i32);
        seg.ms_pos + seg.ms_per_tick * (tick - seg.tick_pos as f64)
    }

    /// `MsPosToNonExactTickPos`.
    pub fn ms_to_non_exact_tick(&self, ms: f64) -> f64 {
        let seg = self.tempo_segment_at_ms(ms);
        seg.tick_pos as f64 + (ms - seg.ms_pos) * seg.ticks_per_ms
    }

    /// `MsPosToTickPos` — rounds with C# `Math.Round` (banker's rounding).
    pub fn ms_to_tick(&self, ms: f64) -> i32 {
        csharp_round(self.ms_to_non_exact_tick(ms))
    }

    /// `TicksBetweenMsPos`.
    pub fn ticks_between_ms(&self, ms_pos: f64, ms_end: f64) -> i32 {
        self.ms_to_tick(ms_end) - self.ms_to_tick(ms_pos)
    }

    /// `MsBetweenTickPos`.
    pub fn ms_between_ticks(&self, tick_pos: f64, tick_end: f64) -> f64 {
        self.tick_to_ms(tick_end) - self.tick_to_ms(tick_pos)
    }

    /// `MsToTickAt`: convert a millisecond duration (positive = after
    /// `ref_tick_pos`, negative = before it) to ticks.
    pub fn ms_to_tick_at(&self, offset_ms: f64, ref_tick_pos: i32) -> i32 {
        let ref_ms = self.tick_to_ms(ref_tick_pos as f64);
        self.ticks_between_ms(ref_ms, ref_ms + offset_ms)
    }

    /// `TickPosToBarBeat` → `(bar, beat, remaining_ticks)`.
    pub fn tick_to_bar_beat(&self, tick: i32) -> (i32, i32, i32) {
        let seg = self.time_sig_segment_at_tick(tick);
        let bar = seg.bar_pos + (tick - seg.tick_pos) / seg.ticks_per_bar;
        let tick_in_bar = tick - seg.tick_pos - seg.ticks_per_bar * (bar - seg.bar_pos);
        let beat = tick_in_bar / seg.ticks_per_beat;
        let remaining_ticks = tick_in_bar - beat * seg.ticks_per_beat;
        (bar, beat, remaining_ticks)
    }

    /// `BarBeatToTickPos`.
    pub fn bar_beat_to_tick(&self, bar: i32, beat: i32) -> i32 {
        let seg = self.time_sig_segment_at_bar(bar);
        seg.tick_pos + seg.ticks_per_bar * (bar - seg.bar_pos) + seg.ticks_per_beat * beat
    }

    /// `NextBarBeat`.
    pub fn next_bar_beat(&self, bar: i32, beat: i32) -> (i32, i32) {
        let mut next_bar = bar;
        let mut next_beat = beat + 1;
        let seg = self.time_sig_segment_at_bar(bar);
        if next_beat >= seg.beat_per_bar {
            next_bar += 1;
            next_beat = 0;
        }
        (next_bar, next_beat)
    }

    /// `TemposBetweenTicks`: tempo changes whose segment overlaps
    /// `(start, end)`.
    pub fn tempos_between_ticks(&self, start: i32, end: i32) -> Vec<UTempo> {
        self.tempo_segments
            .iter()
            .filter(|seg| start < seg.tick_end && seg.tick_pos < end)
            .map(|seg| UTempo::new(seg.tick_pos, seg.bpm))
            .collect()
    }

    /// `TimeSignatureAtTick`.
    pub fn time_signature_at_tick(&self, tick: i32) -> UTimeSignature {
        let seg = self.time_sig_segment_at_tick(tick);
        UTimeSignature::new(seg.bar_pos, seg.beat_per_bar, seg.beat_unit)
    }

    /// `TimeSignatureAtBar`.
    pub fn time_signature_at_bar(&self, bar: i32) -> UTimeSignature {
        let seg = self.time_sig_segment_at_bar(bar);
        UTimeSignature::new(seg.bar_pos, seg.beat_per_bar, seg.beat_unit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::UProject;

    fn default_project() -> UProject {
        UProject::new()
    }

    #[test]
    fn build_errors() {
        let mut p = default_project();
        p.time_signatures = vec![UTimeSignature::new(1, 4, 4)];
        let err = p
            .time_axis
            .build_segments(&p.time_signatures, &p.tempos)
            .unwrap_err();
        assert_eq!(err, "First time signature must be at bar 0.");
        let mut p = default_project();
        p.tempos = vec![UTempo::new(480, 120.0)];
        let err = p.time_axis.build_segments(&p.time_signatures, &p.tempos).unwrap_err();
        assert_eq!(err, "First tempo must be at tick 0.");
    }

    #[test]
    fn bpm_forward_fill_zero() {
        // A tempo with bpm 0 inherits the previous segment's bpm.
        let mut p = default_project();
        p.tempos = vec![UTempo::new(0, 120.0), UTempo::new(480, 0.0)];
        p.time_axis.build_segments(&p.time_signatures, &p.tempos).unwrap();
        assert_eq!(p.time_axis.bpm_at_tick(1000), 120.0);
    }
}
