//! Bounded prefix mixer for the deterministic chunk order used by
//! `runtime::mixer::mix`.
//!
//! A caller renders chunks in canonical plan order and calls `drain_before()`
//! only up to the earliest start sample of every unrendered chunk. At that
//! watermark no future chunk can change emitted samples, so a consumer may
//! play the returned prefix immediately.

use runtime::mixer::{
    db_to_linear, ms_to_samples, ms_to_samples_signed, pan_gain, MixInput, SAMPLE_RATE,
};

/// Incrementally mixes mono voice chunks while retaining only the uncommitted
/// overlap tail. The prefix is bit-identical to [`runtime::mixer::mix`] when
/// inputs are pushed in the same order and drained behind a valid watermark.
#[derive(Debug, Default)]
pub struct IncrementalTrackMixer {
    /// Earliest timeline frame the mixer is configured to emit.
    initial_base_frame: i64,
    /// Absolute sample index represented by `samples[0]`.
    base_frame: i64,
    /// Number of samples in the current buffer already handed to the caller.
    emitted: usize,
    source_end_frame: i64,
    samples: Vec<f32>,
    written: Vec<usize>,
}

impl IncrementalTrackMixer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an incremental track mixer positioned to start emitting at
    /// `start_frame` (skipping any chunk prefix strictly before it while
    /// preserving crossfade phase for straddling chunks).
    pub fn new_at(start_frame: i64) -> Self {
        let base = start_frame.max(0);
        Self {
            initial_base_frame: base,
            base_frame: base,
            emitted: 0,
            source_end_frame: 0,
            samples: Vec::new(),
            written: Vec::new(),
        }
    }

    /// Exclusive end frame of the mix buffer, including pending overlap tail.
    pub fn current_end_frame(&self) -> i64 {
        self.base_frame + self.samples.len() as i64
    }

    /// End frame of the furthest pushed chunk, excluding zero padding added
    /// solely to advance a safe watermark.
    pub fn source_end_frame(&self) -> i64 {
        self.source_end_frame
    }

    /// Add one voice chunk in canonical order. Returns an error if it would
    /// modify audio that the caller has already committed to playback.
    pub fn push(&mut self, input: &MixInput) -> Result<(), String> {
        self.push_fragment(input, 0, &input.chunk.samples)
    }

    /// Add a contiguous slice of one chunk while preserving the chunk's
    /// original timeline offset and crossfade phase. `sample_offset` is the
    /// first sample's index in `input.chunk.samples`; callers may invoke this
    /// repeatedly as a renderer produces finalized PCM blocks.
    pub fn push_fragment(
        &mut self,
        input: &MixInput,
        sample_offset: usize,
        samples: &[f32],
    ) -> Result<(), String> {
        let chunk_start = ms_to_samples_signed(
            input.chunk.position_ms - input.chunk.leading_ms,
            SAMPLE_RATE,
        );
        let start = chunk_start.saturating_add(sample_offset as i64);
        let end = start.saturating_add(samples.len() as i64).max(0);
        self.source_end_frame = self.source_end_frame.max(end);
        if end <= self.base_frame {
            return Ok(());
        }
        let has_committed_audio = self.emitted > 0 || self.base_frame > self.initial_base_frame;
        if start < self.base_frame && has_committed_audio {
            return Err(format!(
                "chunk starts at frame {start} before retained frame {}",
                self.base_frame
            ));
        }
        if start >= 0 && start < self.base_frame.saturating_add(self.emitted as i64) && has_committed_audio {
            return Err(format!(
                "chunk starts at frame {start} before committed frame {}",
                self.base_frame.saturating_add(self.emitted as i64)
            ));
        }

        let required = usize::try_from(end.saturating_sub(self.base_frame))
            .map_err(|_| "incremental mix frame range overflow".to_string())?;
        if self.samples.len() < required {
            self.samples.resize(required, 0.0);
            self.written.resize(required, 0);
        }

        let overlap = ms_to_samples(input.chunk.leading_ms, SAMPLE_RATE).max(1);
        let crossfade = input.chunk.leading_ms > 0.0;
        let gain =
            db_to_linear(input.track.volume_db) * pan_gain(input.track.pan) * input.track.gain;
        for (index, sample) in samples.iter().enumerate() {
            let chunk_index = sample_offset.saturating_add(index);
            let target = start.saturating_add(index as i64);
            if target < 0 || target < self.base_frame {
                continue;
            }
            let target = usize::try_from(target - self.base_frame)
                .map_err(|_| "incremental mix target overflow".to_string())?;
            if target < self.emitted {
                return Err(format!(
                    "chunk writes frame {} before committed frame {}",
                    self.base_frame + target as i64,
                    self.base_frame + self.emitted as i64
                ));
            }
            if target >= self.samples.len() {
                // A negative start can make `end` shorter than the original
                // sample vector; the ordinary mixer also clips at frame 0.
                break;
            }
            if self.written[target] == 0 || !crossfade {
                self.samples[target] += sample * gain;
            } else {
                let t = (chunk_index as f32 / overlap as f32).min(1.0);
                let a = (t * std::f32::consts::FRAC_PI_2).cos();
                let b = (t * std::f32::consts::FRAC_PI_2).sin();
                self.samples[target] = self.samples[target] * a + sample * gain * b;
            }
            self.written[target] += 1;
        }
        Ok(())
    }

    /// Release all samples strictly before `end_frame`. The caller must have
    /// proved that every not-yet-pushed chunk starts at or after this frame.
    /// Missing samples in the prefix are emitted as silence, as in `mix()`.
    pub fn drain_before(&mut self, end_frame: i64) -> Result<Vec<f32>, String> {
        let end_frame = end_frame.max(self.base_frame);
        let required = usize::try_from(end_frame - self.base_frame)
            .map_err(|_| "incremental mix watermark overflow".to_string())?;
        if self.samples.len() < required {
            self.samples.resize(required, 0.0);
            self.written.resize(required, 0);
        }
        let count = required.min(self.samples.len());
        if count < self.emitted {
            return Err("incremental mix watermark moved backwards".to_string());
        }
        let out = self.samples[self.emitted..count].to_vec();
        self.emitted = count;
        self.compact();
        Ok(out)
    }

    /// Release the remaining samples after all planned chunks have been
    /// pushed.
    pub fn finish(&mut self) -> Vec<f32> {
        let out = self.samples[self.emitted..].to_vec();
        self.emitted = self.samples.len();
        self.compact();
        out
    }

    fn compact(&mut self) {
        // Keep the moving window bounded without shifting it on every small
        // audio block. A few seconds of overlap remains cheap even for long
        // projects; the caller's output channel applies its own hard bound.
        if self.emitted >= 16_384 && self.emitted * 2 >= self.samples.len() {
            self.samples.drain(..self.emitted);
            self.written.drain(..self.emitted);
            self.base_frame += self.emitted as i64;
            self.emitted = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime::mixer::{mix, AudioChunk, TrackSpec};

    fn input(samples: Vec<f32>, position_ms: f64, leading_ms: f64) -> MixInput {
        MixInput {
            chunk: AudioChunk {
                samples,
                position_ms,
                leading_ms,
                hash: 0,
            },
            track: TrackSpec::default(),
        }
    }

    #[test]
    fn incremental_mix_matches_full_mix_behind_future_start_watermark() {
        let inputs = vec![
            input(vec![0.25; 4410], 100.0, 20.0),
            input(vec![0.5; 4410], 180.0, 30.0),
            input(vec![-0.125; 2205], 310.0, 0.0),
            input(vec![0.75; 4410], 520.0, 45.0),
        ];
        let expected = mix(&inputs).samples;
        let mut mixer = IncrementalTrackMixer::new();
        let mut actual = Vec::new();
        for (index, item) in inputs.iter().enumerate() {
            mixer.push(item).unwrap();
            let watermark = inputs[index + 1..]
                .iter()
                .map(|next| {
                    ms_to_samples_signed(
                        next.chunk.position_ms - next.chunk.leading_ms,
                        SAMPLE_RATE,
                    )
                })
                .min();
            if let Some(watermark) = watermark {
                actual.extend(mixer.drain_before(watermark).unwrap());
            }
        }
        actual.extend(mixer.finish());
        assert_eq!(actual, expected);
    }

    #[test]
    fn streamed_fragments_preserve_chunk_crossfade_and_timeline_exactly() {
        let inputs = vec![
            input(
                (0..8_820)
                    .map(|i| (i as f32 * 0.0031).sin() * 0.25)
                    .collect(),
                100.0,
                20.0,
            ),
            input(
                (0..8_820)
                    .map(|i| (i as f32 * 0.0017).cos() * 0.5)
                    .collect(),
                180.0,
                30.0,
            ),
            input(vec![-0.125; 2205], 310.0, 0.0),
            input(vec![0.75; 4410], 520.0, 45.0),
        ];
        let expected = mix(&inputs).samples;
        let mut mixer = IncrementalTrackMixer::new();
        let mut actual = Vec::new();
        for (index, item) in inputs.iter().enumerate() {
            for (offset, fragment) in item.chunk.samples.chunks(733).enumerate() {
                mixer.push_fragment(item, offset * 733, fragment).unwrap();
            }
            let watermark = inputs[index + 1..]
                .iter()
                .map(|next| {
                    ms_to_samples_signed(
                        next.chunk.position_ms - next.chunk.leading_ms,
                        SAMPLE_RATE,
                    )
                })
                .min();
            if let Some(watermark) = watermark {
                actual.extend(mixer.drain_before(watermark).unwrap());
            }
        }
        actual.extend(mixer.finish());
        assert_eq!(actual, expected);
    }

    #[test]
    fn incremental_mix_rejects_a_chunk_that_reaches_committed_audio() {
        let mut mixer = IncrementalTrackMixer::new();
        mixer.push(&input(vec![0.25; 4410], 100.0, 0.0)).unwrap();
        let _ = mixer.drain_before(2205).unwrap();
        let error = mixer.push(&input(vec![0.5; 2205], 90.0, 50.0)).unwrap_err();
        assert!(error.contains("before committed frame"));
    }

    #[test]
    fn incremental_mix_compacts_a_long_committed_prefix_without_changing_pcm() {
        let inputs = vec![
            input(vec![0.125; 44_100], 0.0, 0.0),
            input(vec![0.25; 44_100], 1000.0, 20.0),
        ];
        let expected = mix(&inputs).samples;
        let mut mixer = IncrementalTrackMixer::new();
        mixer.push(&inputs[0]).unwrap();
        let mut actual = mixer.drain_before(43_218).unwrap();
        assert_eq!(mixer.base_frame, 43_218, "large committed prefix compacts");
        mixer.push(&inputs[1]).unwrap();
        actual.extend(mixer.finish());
        assert_eq!(actual, expected);
    }

    #[test]
    fn incremental_mix_from_start_frame_matches_offline_tail() {
        let inputs = vec![
            input(vec![0.125; 4410], 50.0, 10.0),
            input(vec![0.25; 8820], 120.0, 20.0),
            input(vec![0.5; 8820], 250.0, 30.0),
        ];
        let expected = mix(&inputs).samples;
        let seek_frame = 6_000usize;
        let mut mixer = IncrementalTrackMixer::new_at(seek_frame as i64);
        for item in &inputs {
            for (offset, fragment) in item.chunk.samples.chunks(511).enumerate() {
                mixer.push_fragment(item, offset * 511, fragment).unwrap();
            }
        }
        let mut actual = mixer.drain_before(10_000).unwrap();
        actual.extend(mixer.finish());
        assert_eq!(actual, expected[seek_frame..]);
    }
}
