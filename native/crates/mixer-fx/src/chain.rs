//! Pure-Rust mixer FX chain — port of `plugins/mixer-fx/mixer_fx.cpp`.
//!
//! Chain order (OpenUtau `MixFxSource`): gain → 3-band EQ → compressor →
//! soft clip.
//!
//! Why this exists next to the `.so` path: the browser (wasm) build has no
//! `dlopen`, so the FX chain must be available as plain Rust. The
//! coefficients and the process order are identical to the C++ version, so
//! both produce the same samples.

/// One biquad section in Transposed Direct Form II.
#[derive(Debug, Clone, Copy, Default)]
pub struct Biquad {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
    s1: f64,
    s2: f64,
}

impl Biquad {
    /// Reset the DF2T state (coefficients are kept).
    pub fn reset(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
    }

    /// One sample, in place.
    #[inline]
    pub fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.s1;
        self.s1 = self.b1 * x - self.a1 * y + self.s2;
        self.s2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// Low shelf (RBJ Audio EQ Cookbook). `f0` = corner, `gain_db` = shelf gain,
/// `s` = shelf slope (1 = max).
pub fn low_shelf(f0: f64, gain_db: f64, s: f64, fs: f64) -> Biquad {
    let a = 10f64.powf(gain_db / 40.0);
    let w0 = 2.0 * std::f64::consts::PI * f0 / fs;
    let cs = w0.cos();
    let alpha = w0.sin() / 2.0 * (2.0 * a * s).sqrt();
    let sq_a = 2.0 * a.sqrt() * alpha;
    let a0 = (a + 1.0) + (a - 1.0) * cs + sq_a;
    Biquad {
        b0: a * ((a + 1.0) - (a - 1.0) * cs + sq_a) / a0,
        b1: 2.0 * a * ((a - 1.0) - (a + 1.0) * cs) / a0,
        b2: a * ((a + 1.0) - (a - 1.0) * cs - sq_a) / a0,
        a1: -2.0 * ((a - 1.0) + (a + 1.0) * cs) / a0,
        a2: ((a + 1.0) + (a - 1.0) * cs - sq_a) / a0,
        ..Default::default()
    }
}

/// High shelf (RBJ).
pub fn high_shelf(f0: f64, gain_db: f64, s: f64, fs: f64) -> Biquad {
    let a = 10f64.powf(gain_db / 40.0);
    let w0 = 2.0 * std::f64::consts::PI * f0 / fs;
    let cs = w0.cos();
    let alpha = w0.sin() / 2.0 * (2.0 * a * s).sqrt();
    let sq_a = 2.0 * a.sqrt() * alpha;
    let a0 = (a + 1.0) - (a - 1.0) * cs + sq_a;
    Biquad {
        b0: a * ((a + 1.0) + (a - 1.0) * cs + sq_a) / a0,
        b1: -2.0 * a * ((a - 1.0) + (a + 1.0) * cs) / a0,
        b2: a * ((a + 1.0) + (a - 1.0) * cs - sq_a) / a0,
        a1: 2.0 * ((a - 1.0) - (a + 1.0) * cs) / a0,
        a2: ((a + 1.0) - (a - 1.0) * cs - sq_a) / a0,
        ..Default::default()
    }
}

/// Peak / parametric (RBJ). `f0` = center, `q` = bandwidth.
pub fn peak(f0: f64, gain_db: f64, q: f64, fs: f64) -> Biquad {
    let a = 10f64.powf(gain_db / 40.0);
    let w0 = 2.0 * std::f64::consts::PI * f0 / fs;
    let cs = w0.cos();
    let alpha = w0.sin() / (2.0 * q);
    let a0 = 1.0 + alpha / a;
    Biquad {
        b0: (1.0 + alpha * a) / a0,
        b1: (-2.0 * cs) / a0,
        b2: (1.0 - alpha * a) / a0,
        a1: (-2.0 * cs) / a0,
        a2: (1.0 - alpha / a) / a0,
        ..Default::default()
    }
}

fn db_to_lin(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

/// Minimal `"key":value` scan — same contract as the C++ `parse_param`
/// (enough for the JSON params the host sends).
pub fn parse_param(json: &str, key: &str, default: f64) -> f64 {
    let needle = format!("\"{key}\"");
    let Some(pos) = json.find(&needle) else {
        return default;
    };
    let Some(colon) = json[pos..].find(':') else {
        return default;
    };
    let rest = &json[pos + colon + 1..];
    let rest = rest.trim_start();
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')))
        .unwrap_or(rest.len());
    rest[..end].parse::<f64>().unwrap_or(default)
}

/// The pure-Rust FX chain.
#[derive(Debug, Clone)]
pub struct MixerChain {
    sample_rate: f64,
    gain: f64,
    comp_threshold: f64,
    comp_ratio: f64,
    comp_attack: f64,
    comp_release: f64,
    comp_makeup: f64,
    comp_env: f64,
    eq_enabled: bool,
    comp_enabled: bool,
    clip_enabled: bool,
    low: Biquad,
    mid: Biquad,
    high: Biquad,
}

impl MixerChain {
    /// Build the chain from a params JSON string (same keys as the `.so`).
    pub fn from_params_json(sample_rate: f64, params_json: &str) -> Self {
        let p = params_json;
        let s = parse_param(p, "eq_slope", 1.0);
        let mut chain = MixerChain {
            sample_rate,
            gain: parse_param(p, "gain", 1.0),
            comp_threshold: parse_param(p, "comp_threshold", 0.5),
            comp_ratio: parse_param(p, "comp_ratio", 4.0),
            comp_attack: parse_param(p, "comp_attack", 0.005),
            comp_release: parse_param(p, "comp_release", 0.1),
            comp_makeup: parse_param(p, "comp_makeup", 0.0),
            comp_env: 0.0,
            eq_enabled: parse_param(p, "eq_enabled", 0.0) > 0.5,
            comp_enabled: parse_param(p, "comp_enabled", 0.0) > 0.5,
            clip_enabled: parse_param(p, "clip_enabled", 1.0) > 0.5,
            low: Biquad::default(),
            mid: Biquad::default(),
            high: Biquad::default(),
        };
        // Bands are always configured (valid coefficients even when EQ is
        // off and no processing runs), exactly like the C++ ctor.
        chain.low = low_shelf(200.0, parse_param(p, "low_gain", 0.0), s, sample_rate);
        chain.mid = peak(
            1000.0,
            parse_param(p, "mid_gain", 0.0),
            parse_param(p, "eq_mid_q", 1.0),
            sample_rate,
        );
        chain.high = high_shelf(4000.0, parse_param(p, "high_gain", 0.0), s, sample_rate);
        chain
    }

    /// Process one block in place (`MxFxProcess`).
    pub fn process(&mut self, samples: &mut [f32], _pos_ms: f64) {
        if self.gain != 1.0 {
            for sample in samples.iter_mut() {
                *sample = (*sample as f64 * self.gain) as f32;
            }
        }
        if self.eq_enabled {
            for sample in samples.iter_mut() {
                let mut x = *sample as f64;
                x = self.low.process(x);
                x = self.mid.process(x);
                x = self.high.process(x);
                *sample = x as f32;
            }
        }
        if self.comp_enabled {
            self.compress(samples);
        }
        if self.clip_enabled {
            for sample in samples.iter_mut() {
                *sample = (*sample as f64).tanh() as f32;
            }
        }
    }

    /// One-pole compressor: envelope follower + ratio reduction + makeup.
    fn compress(&mut self, samples: &mut [f32]) {
        let th = self.comp_threshold;
        let ratio = self.comp_ratio;
        let makeup = db_to_lin(self.comp_makeup);
        let atk = 1.0 - (-1.0 / (self.comp_attack * self.sample_rate + 1e-9)).exp();
        let rel = 1.0 - (-1.0 / (self.comp_release * self.sample_rate + 1e-9)).exp();
        for sample in samples.iter_mut() {
            let x = (*sample as f64).abs();
            let coeff = if x > self.comp_env { atk } else { rel };
            self.comp_env += coeff * (x - self.comp_env);
            let gain = if self.comp_env > th {
                let over = self.comp_env - th;
                th / (th + over / ratio)
            } else {
                1.0
            };
            *sample = (*sample as f64 * gain * makeup) as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_param_reads_json_values() {
        let j = r#"{"gain":0.5,"mid_gain":-3.5,"clip_enabled":1}"#;
        assert_eq!(parse_param(j, "gain", 9.0), 0.5);
        assert_eq!(parse_param(j, "mid_gain", 9.0), -3.5);
        assert_eq!(parse_param(j, "missing", 7.0), 7.0);
        assert_eq!(parse_param(j, "clip_enabled", 0.0), 1.0);
    }

    #[test]
    fn flat_params_are_a_noop_when_bands_are_off() {
        // gain 1, no EQ, no comp, clip off -> identity.
        let mut chain = MixerChain::from_params_json(44100.0, r#"{"clip_enabled":0}"#);
        let mut samples = vec![0.25f32, -0.5, 0.75];
        let before = samples.clone();
        chain.process(&mut samples, 0.0);
        assert_eq!(samples, before);
    }

    #[test]
    fn clip_bounds_the_peak() {
        let mut chain = MixerChain::from_params_json(44100.0, r#"{"clip_enabled":1}"#);
        let mut samples = vec![5.0f32, -5.0];
        chain.process(&mut samples, 0.0);
        assert!(samples.iter().all(|s| s.abs() < 1.0));
    }

    #[test]
    fn eq_band_changes_the_signal() {
        let mut chain = MixerChain::from_params_json(
            44100.0,
            r#"{"eq_enabled":1,"clip_enabled":0,"low_gain":12.0}"#,
        );
        // A 100 Hz tone sits inside the low shelf -> boosted.
        let mut samples: Vec<f32> = (0..4410)
            .map(|i| (2.0 * std::f64::consts::PI * 100.0 * i as f64 / 44100.0).sin() as f32)
            .collect();
        let peak_before = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        chain.process(&mut samples, 0.0);
        let peak_after = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            peak_after > peak_before * 1.5,
            "{peak_before} -> {peak_after}"
        );
    }
}
