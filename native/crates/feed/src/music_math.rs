//! Port of `OpenUtau.Core/Util/MusicMath.cs` pieces needed by the feed:
//! easing/interpolation shapes and tone↔frequency conversion.
//!
//! The reference defines `ep = 0.001`; interpolation with an interval
//! shorter than that returns the segment end value.

const EP: f64 = 0.001;

/// `SinEasingInOut` — sine ease-in-out between `(x0,y0)` and `(x1,y1)`.
pub fn sin_easing_in_out(x0: f64, x1: f64, y0: f64, y1: f64, x: f64) -> f64 {
    if x1 - x0 < EP {
        return y1;
    }
    y0 + (y1 - y0) * (1.0 - ((x - x0) / (x1 - x0) * std::f64::consts::PI).cos()) / 2.0
}

/// `SinEasingIn` — sine ease-in.
pub fn sin_easing_in(x0: f64, x1: f64, y0: f64, y1: f64, x: f64) -> f64 {
    if x1 - x0 < EP {
        return y1;
    }
    y0 + (y1 - y0) * (1.0 - ((x - x0) / (x1 - x0) * std::f64::consts::PI / 2.0).cos())
}

/// `SinEasingOut` — sine ease-out.
pub fn sin_easing_out(x0: f64, x1: f64, y0: f64, y1: f64, x: f64) -> f64 {
    if x1 - x0 < EP {
        return y1;
    }
    y0 + (y1 - y0) * ((x - x0) / (x1 - x0) * std::f64::consts::PI / 2.0).sin()
}

/// `Linear` — linear interpolation.
pub fn linear(x0: f64, x1: f64, y0: f64, y1: f64, x: f64) -> f64 {
    if x1 - x0 < EP {
        return y1;
    }
    y0 + (y1 - y0) * (x - x0) / (x1 - x0)
}

/// `MusicMath.InterpolateShape` — pick the interpolation function for a
/// pitch point shape. `io` and `sp` both use sine ease-in-out; `i` and `o`
/// use the one-sided easings; anything else (`l`) is linear.
pub fn interpolate_shape(
    x0: f64,
    x1: f64,
    y0: f64,
    y1: f64,
    x: f64,
    shape: domain::PitchPointShape,
) -> f64 {
    match shape {
        domain::PitchPointShape::Io | domain::PitchPointShape::Sp => {
            sin_easing_in_out(x0, x1, y0, y1, x)
        }
        domain::PitchPointShape::I => sin_easing_in(x0, x1, y0, y1, x),
        domain::PitchPointShape::O => sin_easing_out(x0, x1, y0, y1, x),
        domain::PitchPointShape::L => linear(x0, x1, y0, y1, x),
    }
}

/// `MusicMath.ToneToFreq(int/double)` — MIDI tone (C4 = 60, A4 = 69) to Hz.
pub fn tone_to_freq(tone: f64) -> f64 {
    440.0 * 2f64.powf((tone - 69.0) / 12.0)
}

/// Cents → Hz: [`tone_to_freq`] with the pitch expressed in cents
/// (`cents / 100` semitones).
pub fn cents_to_freq(cents: f64) -> f64 {
    tone_to_freq(cents / 100.0)
}

/// `FreqToTone` — Hz to MIDI tone.
pub fn freq_to_tone(freq: f64) -> f64 {
    (freq / 440.0).log2() * 12.0 + 69.0
}

/// Shift a frequency by `semitones` (`f0 * 2^(shift/12)`), the key-shift
/// convention shared by the neural renderers (`shiftedF0`).
pub fn shift_freq(freq: f64, semitones: f64) -> f64 {
    freq * 2f64.powf(semitones / 12.0)
}

/// `MusicMath.DecibelToLinear`.
pub fn decibel_to_linear(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::PitchPointShape;

    fn assert_close(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "expected {b}, got {a}");
    }

    #[test]
    fn tone_freq_roundtrip() {
        assert_close(tone_to_freq(69.0), 440.0);
        assert_close(tone_to_freq(60.0), 261.6255653005986);
        assert_close(tone_to_freq(81.0), 880.0);
        assert_close(freq_to_tone(440.0), 69.0);
        assert_close(cents_to_freq(6000.0), tone_to_freq(60.0));
        assert_close(shift_freq(440.0, 12.0), 880.0);
        assert_close(shift_freq(440.0, 0.0), 440.0);
    }

    #[test]
    fn linear_and_easing_endpoints() {
        assert_close(linear(0.0, 10.0, 0.0, 100.0, 5.0), 50.0);
        assert_close(linear(0.0, 10.0, 0.0, 100.0, 10.0), 100.0);
        for f in [sin_easing_in_out, sin_easing_in, sin_easing_out] {
            assert_close(f(0.0, 10.0, 0.0, 100.0, 0.0), 0.0);
            assert_close(f(0.0, 10.0, 0.0, 100.0, 10.0), 100.0);
        }
        // io midpoint is exactly half.
        assert_close(sin_easing_in_out(0.0, 10.0, 0.0, 100.0, 5.0), 50.0);
        // i lags at the start, o leads: at x=2.5/10 (quarter), io is (1-cos(pi/4))/2.
        let q = (1.0 - (std::f64::consts::FRAC_PI_4).cos()) / 2.0;
        assert_close(sin_easing_in_out(0.0, 10.0, 0.0, 100.0, 2.5), q * 100.0);
        assert!(sin_easing_in(0.0, 10.0, 0.0, 100.0, 2.5) < q * 100.0);
        assert!(sin_easing_out(0.0, 10.0, 0.0, 100.0, 2.5) > q * 100.0);
    }

    #[test]
    fn interpolate_shape_routes() {
        // io and sp share the sine-in-out curve.
        let v_io = interpolate_shape(0.0, 10.0, 0.0, 100.0, 3.0, PitchPointShape::Io);
        let v_sp = interpolate_shape(0.0, 10.0, 0.0, 100.0, 3.0, PitchPointShape::Sp);
        let v_l = interpolate_shape(0.0, 10.0, 0.0, 100.0, 3.0, PitchPointShape::L);
        assert_close(v_io, v_sp);
        assert_close(v_l, 30.0);
        assert!(v_io < 30.0); // sine in-out is below linear at the start
    }

    #[test]
    fn tiny_interval_returns_end() {
        assert_close(linear(0.0, 0.0005, 0.0, 100.0, 0.0), 100.0);
        assert_close(sin_easing_in_out(1.0, 1.0, 5.0, 9.0, 1.0), 9.0);
    }
}
