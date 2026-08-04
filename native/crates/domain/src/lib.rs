//! Domain model for OpenUtau-compatible `.ustx` project files (v0.9).
//!
//! This crate implements the serializable data model of an OpenUtau project
//! (`UProject` and friends) plus the `TimeAxis` tick/time conversion engine,
//! mirroring the C# reference implementation in `OpenUtau.Core/Ustx` and
//! `OpenUtau.Core/Util/TimeAxis.cs`.
//!
//! # YAML compatibility
//!
//! Field names, defaults and structure match what OpenUtau writes and reads
//! (UnderscoredNamingConvention + `DefaultValuesHandling.OmitNull` in
//! `OpenUtau.Core/Util/Yaml.cs`):
//!
//! * struct field names are snake_case of the C# member (`trackNo` →
//!   `track_no`, `snapFirst` → `snap_first`);
//! * `None`/null members are omitted on write and accepted as missing on
//!   read (the one exception is `_custom_default_value`, whose leading
//!   underscore is preserved exactly like YamlDotNet's underscored naming);
//! * enums are written as their C# names (`shape: io`, `type: Curve`);
//! * `ustx_version` is written as a string (`"0.9"`) and accepted as either
//!   a string or a bare number on read (both appear in the wild);
//! * `resolution` is a compile-time constant (480), exactly like OpenUtau's
//!   `[YamlIgnore] resolution => 480` — a `resolution:` key in a file is
//!   ignored on read and never written.
//!
//! Cosmetic differences from YamlDotNet output (semantically identical,
//! round-trip safe): floats are written with a trailing `.0` (`-240.0`
//! instead of `-240`) and `PitchPoint`/`UVibrato`/`UExpression` are written
//! as block mappings instead of `{...}` flow mappings (serde_yaml has no
//! per-field flow-style control).

pub mod curve;
pub mod expression;
pub mod note;
pub mod part;
pub mod phoneme;
pub mod project;
pub mod time_axis;
pub mod track;

pub use curve::UCurve;
pub use expression::{
    add_default_expressions, UExpression, UExpressionDescriptor, UExpressionType, ALT, ATK, BRE,
    BREC, CLR, DEC, DIR, DYN, ENG, EXP_SELECTORS_DEFAULT, GEN, GENC, LPF, MOD, MODP, NORM, PITD,
    REQUIRED, SHFC, SHFT, TENC, VEL, VOIC, VOL,
};
pub use note::{PitchPoint, PitchPointShape, UNote, UPitch, UPhonemeOverride, UVibrato};
pub use part::{UPart, UVoicePart, UWavePart};
pub use phoneme::{UEnvelope, UPhoneme};
pub use project::{UTempo, UProject, UTimeSignature, UstxVersion};
pub use time_axis::TimeAxis;
pub use track::{UMixFx, URenderSettings, UTrack};

/// Ticks per quarter note. OpenUtau hard-codes this (`UProject.resolution`).
pub const RESOLUTION: i32 = 480;

/// The ustx format version this crate reads and writes (`Format.Ustx.kUstxVersion`).
pub const K_USTX_VERSION: UstxVersion = UstxVersion { major: 0, minor: 9 };

/// `System.Math.Round(double)` — banker's rounding (round half to even).
///
/// OpenUtau relies on this exact behavior in `TimeAxis.MsPosToTickPos` and
/// `UCurve.Set`; Rust's `f64::round` rounds half away from zero, so a
/// dedicated helper is required for bit-compatible conversions.
pub(crate) fn csharp_round(x: f64) -> i32 {
    let f = x.floor();
    let diff = x - f;
    if diff < 0.5 {
        f as i32
    } else if diff > 0.5 {
        (f + 1.0) as i32
    } else if (f as i64) % 2 == 0 {
        f as i32
    } else {
        (f + 1.0) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::csharp_round;

    #[test]
    fn banker_rounding_matches_csharp() {
        assert_eq!(csharp_round(0.5), 0);
        assert_eq!(csharp_round(1.5), 2);
        assert_eq!(csharp_round(2.5), 2);
        assert_eq!(csharp_round(-0.5), 0);
        assert_eq!(csharp_round(-1.5), -2);
        assert_eq!(csharp_round(0.4), 0);
        assert_eq!(csharp_round(0.6), 1);
        assert_eq!(csharp_round(3.0), 3);
        assert_eq!(csharp_round(-2.5), -2);
    }
}
