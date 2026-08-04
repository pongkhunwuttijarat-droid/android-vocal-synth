//! Mixing: align rendered chunks in time, sum with per-track gain, apply
//! dynamics envelopes and fades.
//!
//! Faithful to the OpenUtau reference where it matters:
//!
//! * a chunk's sample `0` lands at `position_ms - leading_ms` (see
//!   `Renderers.ApplyDynamics`: `startMs = result.positionMs - result.leadingMs`);
//! * ms→sample conversion truncates, matching C# `(int)` casts;
//! * [`apply_dynamics`] is a direct port of `Renderers.ApplyDynamics`
//!   (per-segment `endSample = (int)((endMs - startMs) / 1000 * 44100)`,
//!   linear interpolation between neighbouring dynamics values, last value
//!   repeated).
//!
//! Output is mono at 44100 Hz, per the data contracts. Stereo panning is
//! folded down to mono with the equal-power law (see [`pan_gain`]).

/// Output sample rate, fixed by the data contracts (44100 Hz).
pub const SAMPLE_RATE: u32 = 44100;

/// A rendered audio chunk, as produced by one chunk of render work.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioChunk {
    /// Mono samples at [`SAMPLE_RATE`]. Sample `0` corresponds to
    /// `position_ms - leading_ms`.
    pub samples: Vec<f32>,
    /// Where the phrase starts, ms.
    pub position_ms: f64,
    /// Leading (preutter) time that precedes the phrase, ms. Subtracted
    /// from `position_ms` for placement, like OpenUtau.
    pub leading_ms: f64,
    /// The chunk's render-cache key (see [`crate::Chunker`]).
    pub hash: u64,
}

/// Final mixed audio handed to the playback service.
#[derive(Clone, Debug, PartialEq)]
pub struct FinalAudio {
    /// Mono samples at `sample_rate`.
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub duration_ms: f64,
}

/// Per-track mix parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackSpec {
    /// Track volume in dB (0 dB = unity).
    pub volume_db: f32,
    /// Stereo pan, `-1.0` (hard left) .. `0.0` (center) .. `1.0` (hard
    /// right). Applied to the mono output as an equal-power fold-down
    /// gain: `cos(pan * pi/4)` — unity at center, `-3 dB` hard-panned.
    pub pan: f32,
}

impl TrackSpec {
    pub fn new(volume_db: f32, pan: f32) -> Self {
        Self { volume_db, pan }
    }
}

impl Default for TrackSpec {
    fn default() -> Self {
        Self {
            volume_db: 0.0,
            pan: 0.0,
        }
    }
}

/// A chunk plus the track parameters it is mixed with.
#[derive(Clone, Debug)]
pub struct MixInput {
    pub chunk: AudioChunk,
    pub track: TrackSpec,
}

/// Convert ms to samples at `sample_rate`, truncating like the C#
/// reference (`(int)(ms / 1000 * 44100)`).
pub fn ms_to_samples(ms: f64, sample_rate: u32) -> usize {
    (ms * f64::from(sample_rate) / 1000.0) as usize
}

/// dB to linear amplitude (`10^(db/20)`).
pub fn db_to_linear(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Equal-power stereo pan folded down to a mono gain.
///
/// Stereo gains are `L = cos((pan+1)·π/4)`, `R = sin((pan+1)·π/4)`; the
/// mono fold-down `(L+R)/√2` simplifies to `cos(pan·π/4)`: unity at
/// center, `-3 dB` at hard left/right.
pub fn pan_gain(pan: f32) -> f32 {
    (pan.clamp(-1.0, 1.0) * std::f32::consts::FRAC_PI_4).cos()
}

/// Multiply `samples` by a dynamics envelope, ported from
/// `OpenUtau.Core.Render.Renderers.ApplyDynamics`.
///
/// `dynamics[i]` is the curve value at time `start_ms + i * interval_ms`;
/// consecutive values are linearly interpolated, and the last value
/// repeats within its own segment. Like the C# original, samples *after*
/// the curve's final segment end are left untouched. `start_ms` is the
/// buffer's time origin (`position_ms - leading_ms` in OpenUtau terms).
pub fn apply_dynamics(
    samples: &mut [f32],
    dynamics: &[f32],
    start_ms: f64,
    interval_ms: f64,
    sample_rate: u32,
) {
    if samples.is_empty() || dynamics.is_empty() {
        return;
    }
    let end_sample_of =
        |i: usize| ms_to_samples(start_ms + (i as f64 + 1.0) * interval_ms, sample_rate);
    let mut seg_start = 0usize;
    for (i, _) in dynamics.iter().enumerate() {
        let seg_end = end_sample_of(i).min(samples.len());
        if seg_end > seg_start {
            let a = dynamics[i];
            let b = if i + 1 < dynamics.len() {
                dynamics[i + 1]
            } else {
                dynamics[i]
            };
            let span = (seg_end - seg_start) as f32;
            for (offset, sample) in samples[seg_start..seg_end].iter_mut().enumerate() {
                let t = offset as f32 / span;
                *sample *= a + (b - a) * t;
            }
        }
        seg_start = seg_end;
    }
}

/// Dynamics variant for tick-based timelines with a tempo-aware time axis.
///
/// The `interval_ticks`/`start_tick` values and the `tick_to_ms` closure
/// replicate OpenUtau's `phrase.position - phrase.leading` / `TimeAxis`
/// usage; each segment's end sample is computed from the *actual* ms of
/// its end tick, so tempo changes are respected.
pub fn apply_dynamics_ticks(
    samples: &mut [f32],
    dynamics: &[f32],
    start_tick: i32,
    interval_ticks: i32,
    tick_to_ms: impl Fn(i32) -> f64,
    sample_rate: u32,
) {
    if samples.is_empty() || dynamics.is_empty() {
        return;
    }
    let start_ms = tick_to_ms(start_tick);
    let mut seg_start = 0usize;
    for (i, _) in dynamics.iter().enumerate() {
        let end_ms = tick_to_ms(start_tick + (i as i32 + 1) * interval_ticks) - start_ms;
        let seg_end = ms_to_samples(end_ms, sample_rate).min(samples.len());
        if seg_end > seg_start {
            let a = dynamics[i];
            let b = if i + 1 < dynamics.len() {
                dynamics[i + 1]
            } else {
                dynamics[i]
            };
            let span = (seg_end - seg_start) as f32;
            for (offset, sample) in samples[seg_start..seg_end].iter_mut().enumerate() {
                let t = offset as f32 / span;
                *sample *= a + (b - a) * t;
            }
        }
        seg_start = seg_end;
    }
}

/// Linear fade-in over the first `fade_samples` samples (first → 0,
/// `fade_samples`-th → 1). No-op when `fade_samples` is 0.
pub fn fade_in(samples: &mut [f32], fade_samples: usize) {
    let n = fade_samples.min(samples.len());
    for (j, sample) in samples[..n].iter_mut().enumerate() {
        *sample *= j as f32 / n as f32;
    }
}

/// Linear fade-out over the last `fade_samples` samples (first faded
/// sample → 1, last sample → 0). No-op when `fade_samples` is 0.
pub fn fade_out(samples: &mut [f32], fade_samples: usize) {
    let n = fade_samples.min(samples.len());
    if n == 0 {
        return;
    }
    let start = samples.len() - n;
    for j in 0..n {
        let factor = if n == 1 {
            0.0
        } else {
            (n - 1 - j) as f32 / (n - 1) as f32
        };
        samples[start + j] *= factor;
    }
}

/// Apply both fades (ms-based convenience over [`fade_in`]/[`fade_out`]).
pub fn apply_fades(samples: &mut [f32], fade_in_ms: f64, fade_out_ms: f64, sample_rate: u32) {
    let fade_in_samples = ms_to_samples(fade_in_ms, sample_rate);
    let fade_out_samples = ms_to_samples(fade_out_ms, sample_rate);
    fade_in(samples, fade_in_samples);
    fade_out(samples, fade_out_samples);
}

/// Align every chunk at `position_ms - leading_ms` and sum them with
/// per-track gain (`volume_db` + pan fold-down).
///
/// Output length covers the longest chunk; `duration_ms` follows.
pub fn mix(inputs: &[MixInput]) -> FinalAudio {
    let length = inputs
        .iter()
        .map(|input| {
            let start = ms_to_samples(input.chunk.position_ms - input.chunk.leading_ms, SAMPLE_RATE);
            start + input.chunk.samples.len()
        })
        .max()
        .unwrap_or(0);
    let mut out = vec![0f32; length];
    // Track written samples so overlapping chunks (chunk-boundary leading,
    // e.g. incremental re-render) are crossfaded over the overlap zone
    // instead of summed — matches the worldline weight blend.
    let mut written = vec![0usize; length];
    for input in inputs {
        let start = ms_to_samples(input.chunk.position_ms - input.chunk.leading_ms, SAMPLE_RATE);
        // Crossfade zone = the chunk's leading (it overlaps the previous
        // chunk's tail); beyond it the chunk writes at full gain. Only
        // chunks with leading (incremental re-render) crossfade — plain
        // track overlaps (leading == 0) still sum.
        let overlap = ms_to_samples(input.chunk.leading_ms, SAMPLE_RATE).max(1);
        let crossfade = input.chunk.leading_ms > 0.0;
        let gain = db_to_linear(input.track.volume_db) * pan_gain(input.track.pan);
        for (i, sample) in input.chunk.samples.iter().enumerate() {
            let target = start + i;
            if target >= out.len() {
                break;
            }
            if written[target] == 0 || !crossfade {
                out[target] += sample * gain;
            } else {
                let prev = out[target];
                let a = if i < overlap {
                    // 1 → 0 across the leading zone: previous fades out,
                    // this chunk fades in.
                    1.0 - (i as f32 / overlap as f32)
                } else {
                    0.0
                };
                out[target] = prev * a + sample * gain * (1.0 - a);
            }
            written[target] += 1;
        }
    }
    let duration_ms = length as f64 / f64::from(SAMPLE_RATE) * 1000.0;
    FinalAudio {
        samples: out,
        sample_rate: SAMPLE_RATE,
        duration_ms,
    }
}

/// Like [`mix`] but the output is at least `min_duration_ms` long
/// (silence-padded), matching the architecture doc's `combine(chunks,
/// range)` contract.
pub fn mix_min_duration(inputs: &[MixInput], min_duration_ms: f64) -> FinalAudio {
    let mut audio = mix(inputs);
    let min_len = ms_to_samples(min_duration_ms, SAMPLE_RATE);
    if audio.samples.len() < min_len {
        audio.samples.resize(min_len, 0.0);
        audio.duration_ms = min_len as f64 / f64::from(SAMPLE_RATE) * 1000.0;
    }
    audio
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(samples: Vec<f32>, position_ms: f64, leading_ms: f64) -> AudioChunk {
        AudioChunk {
            samples,
            position_ms,
            leading_ms,
            hash: 0,
        }
    }

    fn input(chunk: AudioChunk, volume_db: f32, pan: f32) -> MixInput {
        MixInput {
            chunk,
            track: TrackSpec::new(volume_db, pan),
        }
    }

    fn assert_close(a: f32, b: f32, eps: f32) {
        assert!((a - b).abs() <= eps, "|{a} - {b}| > {eps}");
    }

    #[test]
    fn ms_conversion_truncates() {
        assert_eq!(ms_to_samples(0.0, 44100), 0);
        assert_eq!(ms_to_samples(1000.0, 44100), 44100);
        assert_eq!(ms_to_samples(1.0, 44100), 44); // 44.1 truncates
        assert_eq!(ms_to_samples(-5.0, 44100), 0); // negative → 0 (cast)
    }

    #[test]
    fn alignment_uses_position_minus_leading() {
        // 100 ms @ 44100 = 4410 samples; leading 10 ms shifts left by 441,
        // so the chunk lands at sample (100-10) ms = 3969.
        let a = chunk(vec![1.0; 100], 100.0, 10.0);
        let audio = mix(&[input(a, 0.0, 0.0)]);
        assert_eq!(audio.samples.len(), 4410 - 441 + 100);
        assert_eq!(audio.samples[0], 0.0, "silence before the chunk");
        assert_eq!(audio.samples[3968], 0.0, "silence before the chunk");
        assert_eq!(audio.samples[3969], 1.0, "chunk starts at pos-leading");
        assert_eq!(audio.samples[4068], 1.0, "chunk end");
    }

    #[test]
    fn two_chunks_sum_at_overlap() {
        // First chunk: samples [1,1,1] at 0 ms. Second: [2,2,2] at 1 ms
        // (44 samples later, truncation) — no overlap for tiny buffers.
        let a = chunk(vec![1.0; 100], 0.0, 0.0);
        let b = chunk(vec![2.0; 100], 1.0, 0.0);
        let audio = mix(&[input(a, 0.0, 0.0), input(b, 0.0, 0.0)]);
        assert_eq!(audio.samples.len(), 44 + 100);
        assert_eq!(audio.samples[0], 1.0);
        assert_eq!(audio.samples[44], 1.0 + 2.0);
        assert_eq!(audio.samples[99], 1.0 + 2.0);
        assert_eq!(audio.samples[100], 2.0);
        assert_eq!(audio.samples[143], 2.0);
    }

    #[test]
    fn volume_db_scales_linearly() {
        let a = chunk(vec![1.0; 10], 0.0, 0.0);
        // -6 dB ≈ 0.501; +6 dB ≈ 1.995.
        let quiet = mix(&[input(a.clone(), -6.0, 0.0)]);
        assert_close(quiet.samples[0], 0.501, 0.001);
        let loud = mix(&[input(a, 6.0, 0.0)]);
        assert_close(loud.samples[0], 1.995, 0.001);
    }

    #[test]
    fn pan_folds_down_equal_power() {
        // Center: unity. Hard left/right: -3 dB = 1/√2.
        assert_close(pan_gain(0.0), 1.0, 1e-6);
        assert_close(pan_gain(1.0), std::f32::consts::FRAC_1_SQRT_2, 1e-4);
        assert_close(pan_gain(-1.0), std::f32::consts::FRAC_1_SQRT_2, 1e-4);
        assert_close(pan_gain(0.5), 0.9239, 1e-4);
        // Out-of-range clamps instead of panning past the ends.
        assert_close(pan_gain(2.0), pan_gain(1.0), 1e-6);
    }

    #[test]
    fn final_audio_metadata() {
        let a = chunk(vec![0.0; 44100], 0.0, 0.0); // exactly 1 s
        let audio = mix(&[input(a, 0.0, 0.0)]);
        assert_eq!(audio.sample_rate, 44100);
        assert_close(audio.duration_ms as f32, 1000.0, 0.001);
    }

    #[test]
    fn mix_min_duration_pads_with_silence() {
        let a = chunk(vec![1.0; 100], 0.0, 0.0);
        let audio = mix_min_duration(&[input(a, 0.0, 0.0)], 2000.0);
        assert_eq!(audio.samples.len(), 88200);
        assert_close(audio.duration_ms as f32, 2000.0, 0.001);
        assert_eq!(audio.samples[0], 1.0);
        assert_eq!(audio.samples[100], 0.0);
    }

    #[test]
    fn empty_input_yields_empty_audio() {
        let audio = mix(&[]);
        assert!(audio.samples.is_empty());
        assert_eq!(audio.duration_ms, 0.0);
    }

    #[test]
    fn dynamics_constant_curve_scales_uniformly() {
        // Constant 0.5 over the whole buffer.
        let mut samples = vec![1.0; 4410]; // 100 ms
        apply_dynamics(&mut samples, &[0.5, 0.5], 0.0, 50.0, 44100);
        for s in &samples {
            assert_close(*s, 0.5, 1e-6);
        }
    }

    #[test]
    fn dynamics_ramp_interpolates() {
        // Ramp 0.0 -> 1.0 over two 50 ms segments (2205 samples each).
        let mut samples = vec![1.0; 4410];
        apply_dynamics(&mut samples, &[0.0, 1.0], 0.0, 50.0, 44100);
        assert_close(samples[0], 0.0, 1e-6);
        assert_close(samples[2205], 1.0, 1e-6);
        // Midpoint of the first segment ≈ 0.5.
        assert_close(samples[1102], 0.5, 0.001);
        // Last value repeats beyond the curve.
        assert_close(samples[4409], 1.0, 1e-6);
    }

    #[test]
    fn dynamics_shorter_than_buffer_leaves_tail_untouched() {
        // Like OpenUtau, only the curve's own span is scaled: one value at
        // interval 10 ms covers [0, 441) samples; the tail is untouched.
        let mut samples = vec![1.0; 44100];
        apply_dynamics(&mut samples, &[0.25], 0.0, 10.0, 44100);
        assert_close(samples[0], 0.25, 1e-6);
        assert_close(samples[440], 0.25, 1e-6);
        assert_eq!(samples[441], 1.0, "tail beyond the curve is untouched");
        assert_eq!(samples[44099], 1.0);
    }

    #[test]
    fn dynamics_ticks_uses_tempo_aware_axis() {
        // Linear 120 BPM axis: 1 tick = 0.5 ms. Interval 10 ticks = 5 ms.
        let mut samples = vec![1.0; 4410];
        let axis = |tick: i32| tick as f64 * 0.5;
        apply_dynamics_ticks(&mut samples, &[0.0, 1.0], 0, 10, axis, 44100);
        // 5 ms = 220 samples per segment.
        assert_close(samples[0], 0.0, 1e-6);
        assert_close(samples[220], 1.0, 1e-6);
        assert_close(samples[110], 0.5, 0.01);
    }

    #[test]
    fn dynamics_noop_on_empty() {
        let mut samples = vec![1.0; 10];
        apply_dynamics(&mut samples, &[], 0.0, 5.0, 44100);
        assert_eq!(samples, vec![1.0; 10]);
        let mut empty: Vec<f32> = vec![];
        apply_dynamics(&mut empty, &[0.5], 0.0, 5.0, 44100);
        assert!(empty.is_empty());
    }

    #[test]
    fn fade_in_out_shapes_edges() {
        let mut samples = vec![1.0; 100];
        fade_in(&mut samples, 10);
        assert_close(samples[0], 0.0, 1e-6);
        assert_close(samples[5], 0.5, 1e-6);
        assert_close(samples[9], 0.9, 1e-6);
        assert_eq!(samples[10], 1.0, "fade must not touch the rest");

        // Fade-out: first faded sample keeps full gain, last → 0.
        let mut samples = vec![1.0; 100];
        fade_out(&mut samples, 10);
        assert_eq!(samples[89], 1.0, "fade must not touch the rest");
        assert_close(samples[90], 1.0, 1e-6);
        assert_close(samples[94], 5.0 / 9.0, 1e-6);
        assert_close(samples[99], 0.0, 1e-6);
    }

    #[test]
    fn fades_are_noop_for_zero_or_full_length() {
        let mut samples = vec![1.0; 10];
        fade_in(&mut samples, 0);
        fade_out(&mut samples, 0);
        assert_eq!(samples, vec![1.0; 10]);

        // Fade longer than the buffer fades everything.
        let mut samples = vec![1.0; 3];
        fade_in(&mut samples, 10);
        assert_close(samples[0], 0.0, 1e-6);
        assert_close(samples[2], 2.0 / 3.0, 1e-6);
    }

    #[test]
    fn apply_fades_ms_based() {
        let mut samples = vec![1.0; 44100];
        apply_fades(&mut samples, 10.0, 10.0, 44100);
        // Fade-in: 441 samples, linear 0 → 440/441.
        assert_close(samples[0], 0.0, 1e-6);
        assert_close(samples[440], 440.0 / 441.0, 1e-6);
        assert_eq!(samples[441], 1.0, "middle untouched");
        // Fade-out starts at 44100 - 441 = 43659.
        assert_eq!(samples[43658], 1.0, "middle untouched");
        assert_close(samples[43659], 1.0, 1e-6);
        assert_close(samples[44099], 0.0, 1e-6);
        assert_eq!(samples[22050], 1.0, "middle untouched");
    }
}
