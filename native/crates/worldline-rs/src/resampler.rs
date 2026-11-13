//! Port of `worldline/classic/resampler.cpp` — the CLASSIC (UTAU-compatible)
//! per-phoneme resampler: `Resample()` plus its pitch and effects stages.
//!
//! This is the third synthesis path in the reference (the other two are
//! `PhraseSynth` in `phrase.rs` and the neural R2 chain). It differs from
//! `PhraseSynth` in that it is **single-phoneme**: it pitch-shifts one
//! sample to a note, applies the UTAU flags, trims to `required_length`, and
//! applies auto-gain AFTER trimming — the caller concatenates the results
//! (the C++ `worldline::Resampler`, exposed as the `Resample` C entry point).
//!
//! Fixed at a 10 ms model frame and 2 frames of padding, exactly like the
//! reference (`const double frame_ms = 10; const int padding = 2;`).

use worldline_plugin::PhonemeRequest;

use crate::effects::{auto_gain, get_tension_coefficients, vec_maxabs};
use crate::f0::F0Estimator;
use crate::model::Model;
use crate::timing::{
    get_in_total_ms, get_time_mapping, pad_time_mapping, shift_time_mapping, PADDING,
};

/// The classic path's fixed model frame, in ms (`resampler.cpp`).
pub const CLASSIC_FRAME_MS: f64 = 10.0;

/// WORLD `kFloorF0` (used by `ApplyPitch` to detect unvoiced frames).
const FLOOR_F0: f64 = 71.0;

/// Semitone ratio `2^(1/12)`; the reference uses a truncated literal.
const A: f64 = 1.05946309436;

/// The classic resampler.
pub struct ResamplerRs {
    request: PhonemeRequest,
    model: Model,
}

impl ResamplerRs {
    /// Build from a request and decoded mono samples.
    ///
    /// `frq` is the raw `<wav>_wav.frq` payload when the bank ships one
    /// (the reference prefers it over estimation).
    pub fn new(
        request: &PhonemeRequest,
        samples: Vec<f64>,
        sample_fs: i32,
        frq: Option<&[u8]>,
    ) -> Self {
        let estimator = match frq {
            Some(bytes) => match voicebank::parse_frq(bytes) {
                Ok(data) => F0Estimator::Frq(data),
                Err(_) => F0Estimator::Pyin,
            },
            None => F0Estimator::Pyin,
        };
        ResamplerRs {
            request: request.clone(),
            model: Model::new(samples, sample_fs, CLASSIC_FRAME_MS, estimator),
        }
    }

    /// `Resampler::Resample()` — the full classic chain.
    pub fn resample(&mut self) -> Result<Vec<f32>, String> {
        let frame_ms = CLASSIC_FRAME_MS;
        let src_max = vec_maxabs(self.model.samples());

        self.model.build_f0();
        if self.model.f0().is_empty() {
            return Err(format!("F0 estimation failed for {}", self.request.phoneme));
        }

        let mut mapping = get_time_mapping(&self.model, &self.request);

        // --- trim to the input region -------------------------------------
        let start_ms = self.request.offset;
        let length_ms = get_in_total_ms(&self.model, &self.request);
        let start_frame = (start_ms / frame_ms) as i64;
        let length_frame = ((start_ms + length_ms).ceil() / frame_ms) as i64 - start_frame;
        if start_frame < 0 || length_frame <= 0 {
            return Err(format!(
                "invalid input region for {} (start {start_frame}, length {length_frame})",
                self.request.phoneme
            ));
        }
        let left_trimmed = start_frame as f64 * frame_ms;
        // The reference keeps the sub-frame remainder as extra output to trim.
        let mut left_extra = start_ms - left_trimmed;

        self.model.trim(start_frame as usize, length_frame as usize);
        if self.model.f0().is_empty() {
            return Err(format!(
                "trimmed model is empty for {}",
                self.request.phoneme
            ));
        }
        shift_time_mapping(&mut mapping, -left_trimmed);

        self.model.build_sp()?;
        self.model.build_ap()?;

        pad_time_mapping(&mut mapping, PADDING);
        left_extra += frame_ms * PADDING as f64;

        self.model.remap(&mapping);

        // --- pitch + effects ----------------------------------------------
        self.apply_pitch();
        let (tension, breathiness, voicing) = self.apply_effects();
        self.model.synth(&tension, &breathiness, &voicing)?;

        // --- trim the padding back off ------------------------------------
        let mut samples: Vec<f64> = self.model.samples().to_vec();
        let left_extra_samples = self.model.ms_to_samples(left_extra).max(0) as usize;
        let length_samples = self
            .model
            .ms_to_samples(self.request.required_length)
            .max(0) as usize;
        let start = left_extra_samples.min(samples.len());
        samples = samples[start..].to_vec();
        if samples.len() > length_samples {
            samples.truncate(length_samples);
        }

        // Auto-gain is measured on the TRIMMED audio (`out_max`), unlike the
        // phrase path where it runs mid-chain.
        let out_max = vec_maxabs(&samples);
        auto_gain(
            &mut samples,
            src_max,
            out_max,
            self.model.get_voiced_ratio(),
            self.request.volume.round() as i32,
            self.request.flag_P,
        );
        Ok(samples.into_iter().map(|s| s as f32).collect())
    }

    /// `Resampler::ApplyPitch` — retune every frame from the note's tone plus
    /// the pitch-bend curve (the pitch grid is 5 ticks at the request tempo).
    fn apply_pitch(&mut self) {
        let step_ms = 60000.0 / self.request.tempo.max(1e-9) / 480.0 * 5.0;
        let bend_len = self.request.pitch_bend.len();
        let left_pitch = if bend_len > 0 {
            f64::from(self.request.pitch_bend[0])
        } else {
            0.0
        };
        let right_pitch = if bend_len > 0 {
            f64::from(self.request.pitch_bend[bend_len - 1])
        } else {
            0.0
        };
        let frames = self.model.f0().len();
        let mut time_ms = 0.0f64;
        let mut new_f0 = Vec::with_capacity(frames);
        for i in 0..frames {
            // NOTE (reference quirk): the C++ declares `int t` and then does
            // `int index = (int)t; t -= index;` — so the interpolation
            // fraction is ALWAYS 0 and the bend is sampled as a step function
            // (index - padding, no blend). Porting the math as written with a
            // fractional `t` diverges audibly, so the truncation is preserved
            // deliberately. `pitch` is also an int in the reference, so the
            // bend value is used unscaled (×1) and the interpolation term
            // drops out entirely.
            let index = (time_ms / step_ms) as i64;
            let current = self.model.f0()[i];
            let pitch = if current < FLOOR_F0 {
                0.0
            } else if index - (PADDING as i64) < 0 {
                left_pitch
            } else if (index - PADDING as i64) < bend_len as i64 {
                // `index - padding` is inside the array here; the reference's
                // `+1` term is multiplied by t == 0, so it contributes
                // nothing (and reads out of bounds at the tail — avoided).
                f64::from(self.request.pitch_bend[(index - PADDING as i64) as usize])
            } else {
                right_pitch
            };
            let tone = f64::from(self.request.tone) + pitch * 0.01;
            new_f0.push(440.0 * A.powf(tone - 69.0));
            time_ms += self.model.frame_ms();
        }
        *self.model.f0_mut() = new_f0;
    }

    /// `Resampler::ApplyEffects` — gender (`g`), tension (`Mt`), breathiness
    /// (`Mb`) and voicing (`Mv`) flags.
    fn apply_effects(&mut self) -> (Vec<Vec<f64>>, Vec<f64>, Vec<f64>) {
        if self.request.flag_g != 0 {
            self.model.shift_gender_all(self.request.flag_g);
        }
        let frames = self.model.f0().len();
        let bins = self.model.bins().max(1);

        let mut tension: Vec<Vec<f64>> = Vec::with_capacity(frames);
        let mut breathiness = vec![1.0f64; frames];
        let mut voicing = vec![1.0f64; frames];

        // `SynthParams` seeds every frame, then the flags overwrite them.
        let breathiness_value = if self.request.flag_Mb < 0 {
            1.0 + f64::from(self.request.flag_Mb) * 0.01
        } else {
            1.0 + f64::from(self.request.flag_Mb) * 0.02
        };
        let voicing_value = f64::from(self.request.flag_Mv) * 0.01;

        for i in 0..frames {
            tension.push(get_tension_coefficients(
                self.model.f0()[i],
                self.model.fs(),
                self.request.flag_Mt,
                bins,
            ));
        }
        breathiness.fill(breathiness_value);
        voicing.fill(voicing_value);
        (tension, breathiness, voicing)
    }
}

/// Convenience: run the classic chain for one request over in-memory samples.
pub fn resample(
    request: &PhonemeRequest,
    samples: Vec<f64>,
    sample_fs: i32,
    frq: Option<&[u8]>,
) -> Result<Vec<f32>, String> {
    ResamplerRs::new(request, samples, sample_fs, frq).resample()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f64, n: usize, fs: f64) -> Vec<f64> {
        (0..n)
            .map(|i| 0.5 * (2.0 * std::f64::consts::PI * hz * i as f64 / fs).sin())
            .collect()
    }

    #[test]
    fn classic_frame_and_padding_match_the_reference() {
        assert_eq!(CLASSIC_FRAME_MS, 10.0);
        assert_eq!(PADDING, 2);
    }

    #[test]
    fn pitch_mapping_matches_the_semitone_formula() {
        // tone 69 (A4) with no bend -> 440 Hz.
        let tone = 69.0 + 0.0;
        assert!((440.0 * A.powf(tone - 69.0) - 440.0).abs() < 1e-6);
        // tone 81 (A5) -> one octave up.
        let tone = 81.0;
        assert!((440.0 * A.powf(tone - 69.0) - 880.0).abs() < 0.01);
    }

    #[test]
    fn resamples_a_sine_to_the_requested_length() {
        let fs = 44100.0;
        let samples = sine(220.0, 44100, fs);
        let request = PhonemeRequest {
            phoneme: "a".into(),
            wav_path: "/vb/a.wav".into(),
            pos_ms: 0.0,
            skip_ms: 0.0,
            length_ms: 500.0,
            fade_in_ms: 0.0,
            fade_out_ms: 0.0,
            tone: 60,
            con_vel: 100.0,
            offset: 0.0,
            required_length: 500.0,
            consonant: 0.0,
            cut_off: 0.0,
            volume: 100.0,
            modulation: 0.0,
            tempo: 120.0,
            pitch_bend: vec![],
            flag_g: 0,
            flag_O: 0,
            flag_P: 86,
            flag_Mt: 0,
            flag_Mb: 0,
            flag_Mv: 100,
        };
        let out = resample(&request, samples, 44100, None).expect("resample");
        assert!(!out.is_empty(), "classic resampler produced no samples");
        // ~500 ms at 44.1 kHz (the trim is sample-exact to required_length).
        assert!(
            (out.len() as i64 - 22050).abs() < 500,
            "unexpected length {}",
            out.len()
        );
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 1e-3, "classic output is silent (peak {peak})");
    }
}
