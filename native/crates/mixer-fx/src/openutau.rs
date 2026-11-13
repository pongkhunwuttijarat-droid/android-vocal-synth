//! Stateful stereo implementation of OpenUtau's per-track `UMixFx` chain.
//!
//! This is distinct from [`crate::MixerChain`], which implements the legacy
//! optional final-mix POC. Track FX process an interleaved stereo track bus in
//! EQ → stereo-linked compressor → Freeverb order and must live for the whole
//! rendered bus, not be recreated for worker chunks.

use domain::UMixFx;

const REFERENCE_SAMPLE_RATE: f64 = 44_100.0;

#[derive(Debug, Clone, Copy, Default)]
struct TrackBiquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl TrackBiquad {
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

// OpenUtau's BiquadEQ uses the S=1 RBJ shelf form from its C# reference.
// The shared MixerChain shelf helpers use a different `alpha` expression;
// preserve that chain's behavior and keep this track-FX port formula-matched.
fn openutau_low_shelf(fs: f64, gain_db: f64) -> TrackBiquad {
    let a = 10f64.powf(gain_db / 40.0);
    let w0 = 2.0 * std::f64::consts::PI * 200.0 / fs;
    let cosw = w0.cos();
    let sinw = w0.sin();
    let alpha = sinw / 2.0 * 2.0_f64.sqrt();
    let beta = 2.0 * a.sqrt() * alpha;
    let a0 = (a + 1.0) + (a - 1.0) * cosw + beta;
    TrackBiquad {
        b0: (a * ((a + 1.0) - (a - 1.0) * cosw + beta) / a0) as f32,
        b1: (2.0 * a * ((a - 1.0) - (a + 1.0) * cosw) / a0) as f32,
        b2: (a * ((a + 1.0) - (a - 1.0) * cosw - beta) / a0) as f32,
        a1: (-2.0 * ((a - 1.0) + (a + 1.0) * cosw) / a0) as f32,
        a2: (((a + 1.0) + (a - 1.0) * cosw - beta) / a0) as f32,
        ..TrackBiquad::default()
    }
}

fn openutau_peak(fs: f64, freq: f64, q: f64, gain_db: f64) -> TrackBiquad {
    let a = 10f64.powf(gain_db / 40.0);
    let w0 = 2.0 * std::f64::consts::PI * freq / fs;
    let cosw = w0.cos();
    let alpha = w0.sin() / (2.0 * q);
    let a0 = 1.0 + alpha / a;
    TrackBiquad {
        b0: ((1.0 + alpha * a) / a0) as f32,
        b1: ((-2.0 * cosw) / a0) as f32,
        b2: ((1.0 - alpha * a) / a0) as f32,
        a1: ((-2.0 * cosw) / a0) as f32,
        a2: ((1.0 - alpha / a) / a0) as f32,
        ..TrackBiquad::default()
    }
}

fn openutau_high_shelf(fs: f64, gain_db: f64) -> TrackBiquad {
    let a = 10f64.powf(gain_db / 40.0);
    let w0 = 2.0 * std::f64::consts::PI * 8000.0 / fs;
    let cosw = w0.cos();
    let sinw = w0.sin();
    let alpha = sinw / 2.0 * 2.0_f64.sqrt();
    let beta = 2.0 * a.sqrt() * alpha;
    let a0 = (a + 1.0) - (a - 1.0) * cosw + beta;
    TrackBiquad {
        b0: (a * ((a + 1.0) + (a - 1.0) * cosw + beta) / a0) as f32,
        b1: (-2.0 * a * ((a - 1.0) + (a + 1.0) * cosw) / a0) as f32,
        b2: (a * ((a + 1.0) + (a - 1.0) * cosw - beta) / a0) as f32,
        a1: (2.0 * ((a - 1.0) - (a + 1.0) * cosw) / a0) as f32,
        a2: (((a + 1.0) - (a - 1.0) * cosw - beta) / a0) as f32,
        ..TrackBiquad::default()
    }
}

#[derive(Debug, Clone)]
struct Freeverb {
    comb_l: Vec<Comb>,
    comb_r: Vec<Comb>,
    allpass_l: Vec<Allpass>,
    allpass_r: Vec<Allpass>,
    predelay: Vec<f32>,
    predelay_at: usize,
    predelay_frames: usize,
    room_feedback: f32,
    damp: f32,
    width: f32,
    wet: f32,
    dry: f32,
}

impl Freeverb {
    fn new(
        size: f64,
        damp: f64,
        width: f64,
        wet: f64,
        dry: f64,
        predelay_ms: f64,
        sample_rate: f64,
    ) -> Self {
        const COMB: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
        const ALLPASS: [usize; 4] = [556, 441, 341, 225];
        let scale = sample_rate / REFERENCE_SAMPLE_RATE;
        let make_combs = |spread: usize| {
            COMB.iter()
                .map(|&n| Comb::new(((n + spread) as f64 * scale) as usize))
                .collect::<Vec<_>>()
        };
        let make_allpass = |spread: usize| {
            ALLPASS
                .iter()
                .map(|&n| Allpass::new(((n + spread) as f64 * scale) as usize))
                .collect::<Vec<_>>()
        };
        let predelay_len = ((sample_rate * 200.0 / 1000.0) as usize).max(1);
        let mut fx = Self {
            comb_l: make_combs(0),
            comb_r: make_combs(23),
            allpass_l: make_allpass(0),
            allpass_r: make_allpass(23),
            predelay: vec![0.0; predelay_len],
            predelay_at: 0,
            // C# Math.Round uses midpoint-to-even; Rust's round() uses
            // midpoint-away-from-zero, so use the matching rounding mode.
            predelay_frames: (predelay_ms.clamp(0.0, 200.0) * sample_rate / 1000.0)
                .round_ties_even() as usize,
            room_feedback: (size * 0.28 + 0.7) as f32,
            damp: (damp * 0.4) as f32,
            width: width.clamp(0.0, 1.0) as f32,
            wet: wet.max(0.0) as f32,
            dry: dry.max(0.0) as f32,
        };
        for comb in fx.comb_l.iter_mut().chain(fx.comb_r.iter_mut()) {
            comb.feedback = fx.room_feedback;
            comb.set_damp(fx.damp);
        }
        for allpass in fx.allpass_l.iter_mut().chain(fx.allpass_r.iter_mut()) {
            allpass.feedback = 0.5;
        }
        fx.predelay_frames = fx.predelay_frames.min(fx.predelay.len().saturating_sub(1));
        fx
    }

    fn process(&mut self, stereo: &mut [f32]) {
        if self.wet < 1e-4 || stereo.len() < 2 {
            return;
        }
        let wet_l = self.wet * (self.width / 2.0 + 0.5);
        let wet_cross = self.wet * ((1.0 - self.width) / 2.0);
        for frame in stereo.chunks_exact_mut(2) {
            let in_l = frame[0];
            let in_r = frame[1];
            let mut input = (in_l + in_r) * 0.015;
            if self.predelay_frames > 0 {
                self.predelay[self.predelay_at] = input;
                let read = (self.predelay_at + self.predelay.len() - self.predelay_frames)
                    % self.predelay.len();
                input = self.predelay[read];
                self.predelay_at = (self.predelay_at + 1) % self.predelay.len();
            }
            let mut out_l = 0.0;
            let mut out_r = 0.0;
            for comb in &mut self.comb_l {
                out_l += comb.process(input);
            }
            for comb in &mut self.comb_r {
                out_r += comb.process(input);
            }
            for allpass in &mut self.allpass_l {
                out_l = allpass.process(out_l);
            }
            for allpass in &mut self.allpass_r {
                out_r = allpass.process(out_r);
            }
            frame[0] = out_l * wet_l + out_r * wet_cross + in_l * self.dry;
            frame[1] = out_r * wet_l + out_l * wet_cross + in_r * self.dry;
        }
    }

    fn heap_payload_bytes(&self) -> usize {
        let combs = self.comb_l.iter().chain(self.comb_r.iter()).fold(
            vec_payload_bytes(&self.comb_l).saturating_add(vec_payload_bytes(&self.comb_r)),
            |sum, comb| sum.saturating_add(vec_payload_bytes(&comb.buffer)),
        );
        let allpasses = self.allpass_l.iter().chain(self.allpass_r.iter()).fold(
            vec_payload_bytes(&self.allpass_l).saturating_add(vec_payload_bytes(&self.allpass_r)),
            |sum, allpass| sum.saturating_add(vec_payload_bytes(&allpass.buffer)),
        );
        combs
            .saturating_add(allpasses)
            .saturating_add(vec_payload_bytes(&self.predelay))
    }
}

fn vec_payload_bytes<T>(values: &Vec<T>) -> usize {
    values.capacity().saturating_mul(std::mem::size_of::<T>())
}

fn config_payload_bytes(configuration: &UMixFx) -> usize {
    configuration
        .eq_preset
        .capacity()
        .saturating_add(configuration.comp_preset.capacity())
        .saturating_add(configuration.reverb_preset.capacity())
}

#[derive(Debug, Clone)]
struct Comb {
    buffer: Vec<f32>,
    at: usize,
    filter: f32,
    feedback: f32,
    damp1: f32,
}
impl Comb {
    fn new(size: usize) -> Self {
        Self {
            buffer: vec![0.0; size.max(1)],
            at: 0,
            filter: 0.0,
            feedback: 0.5,
            damp1: 0.0,
        }
    }
    fn set_damp(&mut self, damp: f32) {
        self.damp1 = damp;
    }
    fn process(&mut self, x: f32) -> f32 {
        let y = self.buffer[self.at];
        self.filter = y * (1.0 - self.damp1) + self.filter * self.damp1;
        self.buffer[self.at] = x + self.filter * self.feedback;
        self.at = (self.at + 1) % self.buffer.len();
        y
    }
}

#[derive(Debug, Clone)]
struct Allpass {
    buffer: Vec<f32>,
    at: usize,
    feedback: f32,
}
impl Allpass {
    fn new(size: usize) -> Self {
        Self {
            buffer: vec![0.0; size.max(1)],
            at: 0,
            feedback: 0.5,
        }
    }
    fn process(&mut self, x: f32) -> f32 {
        let buffered = self.buffer[self.at];
        let y = -x + buffered;
        self.buffer[self.at] = x + buffered * self.feedback;
        self.at = (self.at + 1) % self.buffer.len();
        y
    }
}

#[derive(Debug, Clone)]
pub struct OpenUtauTrackFx {
    configuration: UMixFx,
    sample_rate: f64,
    enabled: bool,
    eq: [[TrackBiquad; 3]; 2],
    eq_enabled: bool,
    comp_enabled: bool,
    comp_threshold_db: f64,
    comp_ratio: f64,
    comp_makeup: f64,
    attack_coef: f64,
    release_coef: f64,
    comp_env_db: f64,
    reverb: Freeverb,
}

/// Process-local checkpoint of a stateful USTX track FX processor.
///
/// This is intentionally not a serialized project or transport DTO. A stream
/// may keep it in a bounded in-memory cache and restore it only into a
/// processor built from the same USTX FX configuration and sample rate.
#[derive(Debug, Clone)]
pub struct OpenUtauTrackFxSnapshot {
    version: u32,
    configuration: UMixFx,
    sample_rate_bits: u64,
    state: Box<OpenUtauTrackFx>,
}

impl OpenUtauTrackFxSnapshot {
    /// Version of the in-memory snapshot representation and restore semantics.
    pub const VERSION: u32 = 1;

    /// Read the snapshot version without exposing its processor internals.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Estimate the snapshot's retained payload for a bounded in-process
    /// cache. This includes the snapshot shell, boxed processor state, and
    /// owned `String`/`Vec` capacities (especially Freeverb delay buffers).
    /// It excludes allocator metadata and any containing cache/map overhead.
    pub fn heap_payload_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            .saturating_add(config_payload_bytes(&self.configuration))
            .saturating_add(self.state.heap_payload_bytes())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotRestoreError {
    UnsupportedVersion,
    ConfigurationMismatch,
    SampleRateMismatch,
}

impl OpenUtauTrackFx {
    /// Build from project-owned USTX values. Preset keys select compressor
    /// timing and reverb width/wet/dry; stored numeric values remain the DSP
    /// source of truth, matching OpenUtau's `MixFxSource.WrapWith`.
    pub fn new(fx: &UMixFx) -> Self {
        Self::with_sample_rate(fx, REFERENCE_SAMPLE_RATE)
    }

    pub fn with_sample_rate(fx: &UMixFx, sample_rate: f64) -> Self {
        // OpenUtau scales Freeverb's delay-line lengths from its 44.1 kHz
        // tuning and allocates maximum pre-delay in milliseconds. Use one
        // valid rate for every stage of this public constructor.
        let sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            REFERENCE_SAMPLE_RATE
        };
        // OpenUtau stores preset names for the editor; the current numeric
        // values are what its signal-chain constructor actually applies.
        let low_db = fx.eq_low_db;
        let mid_freq = fx.eq_mid_freq;
        let mid_db = fx.eq_mid_db;
        let high_db = fx.eq_high_db;
        let make_eq = || {
            [
                openutau_low_shelf(sample_rate, low_db),
                openutau_peak(sample_rate, mid_freq, 0.707, mid_db),
                openutau_high_shelf(sample_rate, high_db),
            ]
        };
        let (attack_ms, release_ms): (f64, f64) = match fx.comp_preset.as_str() {
            "pop" => (5.0, 80.0),
            "limit" => (1.0, 50.0),
            "gentle" => (10.0, 120.0),
            _ => (10.0, 100.0),
        };
        let threshold = fx.comp_threshold_db;
        let ratio = fx.comp_ratio;
        let makeup = fx.comp_makeup_db;
        let (width, wet, dry) = match fx.reverb_preset.as_str() {
            "vocal_plate" => (1.0, 0.22, 0.85),
            "hall" => (1.0, 0.28, 0.80),
            "ambient" => (1.0, 0.35, 0.70),
            "small_room" => (0.8, 0.18, 0.85),
            _ => (1.0, 0.0, 1.0),
        };
        let wet = wet * fx.reverb_wet.clamp(0.0, 2.0);
        Self {
            configuration: fx.clone(),
            sample_rate,
            enabled: fx.enabled,
            eq: [make_eq(), make_eq()],
            eq_enabled: fx.enabled && [low_db, mid_db, high_db].iter().any(|v| v.abs() >= 0.01),
            comp_enabled: fx.enabled && (ratio > 1.0001 || makeup.abs() >= 0.01),
            comp_threshold_db: threshold,
            comp_ratio: ratio.max(1.0),
            comp_makeup: 10f64.powf(makeup / 20.0),
            attack_coef: (-1.0 / (attack_ms.max(0.05) * 0.001 * sample_rate)).exp(),
            release_coef: (-1.0 / (release_ms.max(0.05) * 0.001 * sample_rate)).exp(),
            comp_env_db: 0.0,
            reverb: Freeverb::new(
                fx.reverb_size,
                fx.reverb_damp,
                width,
                wet,
                dry,
                fx.reverb_pre_delay_ms,
                sample_rate,
            ),
        }
    }

    /// Process interleaved stereo samples in place. Call once on a complete
    /// per-track bus so filter, compressor and reverb state cross chunks.
    pub fn process(&mut self, stereo: &mut [f32]) {
        if !self.enabled || stereo.len() < 2 {
            return;
        }
        if self.eq_enabled {
            for frame in stereo.chunks_exact_mut(2) {
                for (channel, sample) in frame.iter_mut().enumerate() {
                    let mut x = *sample;
                    for band in &mut self.eq[channel] {
                        x = band.process(x);
                    }
                    *sample = x;
                }
            }
        }
        if self.comp_enabled {
            let slope = 1.0 / self.comp_ratio - 1.0;
            let mut env = self.comp_env_db;
            for frame in stereo.chunks_exact_mut(2) {
                let peak = frame[0].abs().max(frame[1].abs()).max(1e-12) as f64;
                let above = 20.0 * peak.log10() - self.comp_threshold_db;
                let gain_db = if above <= -3.0 {
                    0.0
                } else if above >= 3.0 {
                    slope * above
                } else {
                    slope * (above + 3.0).powi(2) / 12.0
                };
                let coef = if gain_db < env {
                    self.attack_coef
                } else {
                    self.release_coef
                };
                env = coef * env + (1.0 - coef) * gain_db;
                let gain = (10f64.powf(env / 20.0) * self.comp_makeup) as f32;
                frame[0] *= gain;
                frame[1] *= gain;
            }
            self.comp_env_db = env;
        }
        self.reverb.process(stereo);
    }

    /// Capture the processor's current block-boundary state for process-local
    /// seeking. The caller is responsible for recording the project/render
    /// identity and exact frame boundary alongside this snapshot.
    pub fn snapshot(&self) -> OpenUtauTrackFxSnapshot {
        OpenUtauTrackFxSnapshot {
            version: OpenUtauTrackFxSnapshot::VERSION,
            configuration: self.configuration.clone(),
            sample_rate_bits: self.sample_rate.to_bits(),
            state: Box::new(self.clone()),
        }
    }

    fn heap_payload_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            .saturating_add(config_payload_bytes(&self.configuration))
            .saturating_add(self.reverb.heap_payload_bytes())
    }

    /// Restore a compatible process-local checkpoint atomically. A rejected
    /// snapshot leaves this processor untouched.
    pub fn restore_snapshot(
        &mut self,
        snapshot: &OpenUtauTrackFxSnapshot,
    ) -> Result<(), SnapshotRestoreError> {
        if snapshot.version != OpenUtauTrackFxSnapshot::VERSION {
            return Err(SnapshotRestoreError::UnsupportedVersion);
        }
        if snapshot.configuration != self.configuration {
            return Err(SnapshotRestoreError::ConfigurationMismatch);
        }
        if snapshot.sample_rate_bits != self.sample_rate.to_bits() {
            return Err(SnapshotRestoreError::SampleRateMismatch);
        }
        *self = snapshot.state.as_ref().clone();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Freeverb, OpenUtauTrackFx, SnapshotRestoreError};
    use domain::UMixFx;

    #[test]
    fn freeverb_delay_lines_scale_with_sample_rate_like_openutau() {
        let fx_44k = Freeverb::new(0.3, 0.7, 0.8, 0.18, 0.85, 12.0, 44_100.0);
        assert_eq!(fx_44k.comb_l[0].buffer.len(), 1116);
        assert_eq!(fx_44k.comb_r[0].buffer.len(), 1139);
        assert_eq!(fx_44k.predelay.len(), 8820);
        assert_eq!(fx_44k.predelay_frames, 529);

        let fx_48k = Freeverb::new(0.3, 0.7, 0.8, 0.18, 0.85, 12.0, 48_000.0);
        // OpenUtau multiplies each 44.1 kHz delay tuning by sr/44100 and
        // truncates to an integer; pre-delay remains 12 ms at the new rate.
        assert_eq!(fx_48k.comb_l[0].buffer.len(), 1214);
        assert_eq!(fx_48k.comb_r[0].buffer.len(), 1239);
        assert_eq!(fx_48k.predelay.len(), 9600);
        assert_eq!(fx_48k.predelay_frames, 576);

        let half_frame_ms = 500.0 / 44_100.0;
        let fx_half_frame = Freeverb::new(0.3, 0.7, 0.8, 0.18, 0.85, half_frame_ms, 44_100.0);
        assert_eq!(
            fx_half_frame.predelay_frames, 0,
            "C# Math.Round midpoint-to-even"
        );

        let fx_low_rate = Freeverb::new(0.3, 0.7, 0.8, 0.18, 0.85, 12.0, 1.0);
        assert!(fx_low_rate
            .comb_l
            .iter()
            .all(|comb| !comb.buffer.is_empty()));
        assert!(fx_low_rate
            .allpass_l
            .iter()
            .all(|allpass| !allpass.buffer.is_empty()));
        let mut impulse = [1.0_f32, 1.0_f32];
        fx_low_rate.clone().process(&mut impulse);
    }

    fn active_fx() -> UMixFx {
        UMixFx {
            enabled: true,
            reverb_preset: "small_room".into(),
            reverb_wet: 1.0,
            reverb_pre_delay_ms: 7.0,
            ..UMixFx::default()
        }
    }

    #[test]
    fn versioned_snapshot_restores_identical_continuation() {
        let config = active_fx();
        let mut uninterrupted = OpenUtauTrackFx::with_sample_rate(&config, 48_000.0);
        let mut prefix = vec![0.25_f32; 4_096];
        uninterrupted.process(&mut prefix);
        let checkpoint = uninterrupted.snapshot();
        assert_eq!(checkpoint.version(), 1);

        let mut expected = vec![0.0_f32; 2_048];
        for (index, sample) in expected.iter_mut().enumerate() {
            *sample = ((index as f32) * 0.017).sin() * 0.2;
        }
        let mut actual = expected.clone();
        uninterrupted.process(&mut expected);

        let mut restored = OpenUtauTrackFx::with_sample_rate(&config, 48_000.0);
        restored.restore_snapshot(&checkpoint).unwrap();
        restored.process(&mut actual);
        assert_eq!(actual, expected);
    }

    #[test]
    fn snapshot_heap_payload_counts_owned_processor_and_delay_buffer_capacities() {
        let config = active_fx();
        let processor = OpenUtauTrackFx::with_sample_rate(&config, 48_000.0);
        let snapshot = processor.snapshot();
        let state = snapshot.state.as_ref();
        let reverb = &state.reverb;
        let mut expected = std::mem::size_of::<super::OpenUtauTrackFxSnapshot>()
            + std::mem::size_of::<OpenUtauTrackFx>();
        for configuration in [&snapshot.configuration, &state.configuration] {
            expected += configuration.eq_preset.capacity();
            expected += configuration.comp_preset.capacity();
            expected += configuration.reverb_preset.capacity();
        }
        expected += (reverb.comb_l.capacity() + reverb.comb_r.capacity())
            * std::mem::size_of::<super::Comb>();
        expected += (reverb.allpass_l.capacity() + reverb.allpass_r.capacity())
            * std::mem::size_of::<super::Allpass>();
        expected += reverb.predelay.capacity() * std::mem::size_of::<f32>();
        expected += reverb
            .comb_l
            .iter()
            .chain(reverb.comb_r.iter())
            .map(|comb| comb.buffer.capacity() * std::mem::size_of::<f32>())
            .sum::<usize>();
        expected += reverb
            .allpass_l
            .iter()
            .chain(reverb.allpass_r.iter())
            .map(|allpass| allpass.buffer.capacity() * std::mem::size_of::<f32>())
            .sum::<usize>();

        assert_eq!(snapshot.heap_payload_bytes(), expected);
        assert!(snapshot.heap_payload_bytes() > 100_000);
    }

    #[test]
    fn incompatible_snapshot_is_rejected_without_mutating_processor() {
        let config = active_fx();
        let mut source = OpenUtauTrackFx::with_sample_rate(&config, 48_000.0);
        let mut prefix = vec![0.25_f32; 4_096];
        source.process(&mut prefix);
        let checkpoint = source.snapshot();

        let mut changed_config = config.clone();
        changed_config.reverb_wet = 0.5;
        let mut destination = OpenUtauTrackFx::with_sample_rate(&changed_config, 48_000.0);
        let mut expected = vec![0.125_f32; 512];
        let mut actual = expected.clone();
        let mut control = destination.clone();

        assert_eq!(
            destination.restore_snapshot(&checkpoint),
            Err(SnapshotRestoreError::ConfigurationMismatch)
        );
        destination.process(&mut actual);
        control.process(&mut expected);
        assert_eq!(actual, expected);

        let mut wrong_rate = OpenUtauTrackFx::with_sample_rate(&config, 44_100.0);
        assert_eq!(
            wrong_rate.restore_snapshot(&checkpoint),
            Err(SnapshotRestoreError::SampleRateMismatch)
        );
    }
}
