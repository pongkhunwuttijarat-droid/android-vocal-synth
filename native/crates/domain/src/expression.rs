//! Expression registry: `UExpressionDescriptor`, `UExpression` and the
//! OpenUtau default expressions (`Format.Ustx.AddDefaultExpressions`).
//!
//! Mirrors `OpenUtau.Core/Ustx/UExpression.cs` and `OpenUtau.Core/Format/USTx.cs`.

use std::fmt;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::project::UProject;

// ---------------------------------------------------------------------------
// Expression abbreviation constants (Format.Ustx)
// ---------------------------------------------------------------------------

pub const DYN: &str = "dyn";
pub const PITD: &str = "pitd";
pub const CLR: &str = "clr";
pub const ENG: &str = "eng";
pub const VEL: &str = "vel";
pub const VOL: &str = "vol";
pub const ATK: &str = "atk";
pub const DEC: &str = "dec";
pub const GEN: &str = "gen";
pub const GENC: &str = "genc";
pub const BRE: &str = "bre";
pub const BREC: &str = "brec";
pub const LPF: &str = "lpf";
pub const NORM: &str = "norm";
pub const MOD: &str = "mod";
pub const MODP: &str = "mod+";
pub const ALT: &str = "alt";
pub const DIR: &str = "dir";
pub const SHFT: &str = "shft";
pub const SHFC: &str = "shfc";
pub const TENC: &str = "tenc";
pub const VOIC: &str = "voic";

/// Expressions that must exist in every valid project (`Format.Ustx.required`).
pub const REQUIRED: [&str; 8] = [DYN, PITD, CLR, ENG, VEL, VOL, ATK, DEC];

/// Default `exp_selectors` list (`UProject.expSelectors`).
pub const EXP_SELECTORS_DEFAULT: [&str; 10] = [DYN, PITD, CLR, ENG, VEL, VOL, ATK, DEC, GEN, BRE];

/// Resampler engine option for `eng` (`WorldlineResampler.name` in the
/// reference implementation).
pub const ENG_OPTIONS: [&str; 2] = ["", "worldline"];

// ---------------------------------------------------------------------------
// UExpressionType
// ---------------------------------------------------------------------------

/// `UExpressionType` — written to YAML as the C# enum name (e.g. `type: Curve`),
/// accepted on read as a name (case-insensitive) or an integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UExpressionType {
    /// Numerical expression with a min/max range (`type: Numerical`).
    #[default]
    Numerical = 0,
    /// Option-list expression (`type: Options`).
    Options = 1,
    /// Part-level curve expression (`type: Curve`).
    Curve = 2,
}

impl Serialize for UExpressionType {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let name = match self {
            UExpressionType::Numerical => "Numerical",
            UExpressionType::Options => "Options",
            UExpressionType::Curve => "Curve",
        };
        serializer.serialize_str(name)
    }
}

impl<'de> Deserialize<'de> for UExpressionType {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct TypeVisitor;

        impl<'de> Visitor<'de> for TypeVisitor {
            type Value = UExpressionType;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "an expression type name (Numerical/Options/Curve) or 0/1/2")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                match v.to_ascii_lowercase().as_str() {
                    "numerical" | "0" => Ok(UExpressionType::Numerical),
                    "options" | "1" => Ok(UExpressionType::Options),
                    "curve" | "2" => Ok(UExpressionType::Curve),
                    _ => Err(E::custom(format!("invalid expression type {v:?}"))),
                }
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                match v {
                    0 => Ok(UExpressionType::Numerical),
                    1 => Ok(UExpressionType::Options),
                    2 => Ok(UExpressionType::Curve),
                    _ => Err(E::custom(format!("invalid expression type {v}"))),
                }
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                if v < 0 {
                    return Err(E::custom(format!("invalid expression type {v}")));
                }
                self.visit_u64(v as u64)
            }
        }

        deserializer.deserialize_any(TypeVisitor)
    }
}

// ---------------------------------------------------------------------------
// UExpressionDescriptor
// ---------------------------------------------------------------------------

/// Specifications of expressions managed by projects and tracks
/// (`UExpressionDescriptor`).
///
/// Serialized YAML keys match OpenUtau exactly, including the literal
/// `_custom_default_value` key (YamlDotNet underscored naming of the C#
/// field `_customDefaultValue`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UExpressionDescriptor {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub abbr: String,
    #[serde(default, rename = "type")]
    pub r#type: UExpressionType,
    #[serde(default)]
    pub min: f32,
    #[serde(default)]
    pub max: f32,
    #[serde(default)]
    pub default_value: f32,
    #[serde(default, rename = "_custom_default_value", skip_serializing_if = "Option::is_none")]
    pub custom_default_value: Option<f32>,
    #[serde(default)]
    pub is_flag: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
    #[serde(default)]
    pub skip_output_if_default: bool,
}

impl Default for UExpressionDescriptor {
    fn default() -> Self {
        UExpressionDescriptor {
            name: String::new(),
            abbr: String::new(),
            r#type: UExpressionType::Numerical,
            min: 0.0,
            max: 0.0,
            default_value: 0.0,
            custom_default_value: None,
            is_flag: false,
            flag: None,
            options: None,
            skip_output_if_default: false,
        }
    }
}

impl UExpressionDescriptor {
    /// Constructor for Numerical/Curve expressions, mirroring the C#
    /// `(name, abbr, min, max, defaultValue, flag, customDefaultValue,
    /// skipOutputIfDefault)` overload. Defaults are clamped into `[min, max]`.
    #[allow(clippy::too_many_arguments)] // mirrors the C# constructor arity
    pub fn new(
        name: impl Into<String>,
        abbr: impl Into<String>,
        r#type: UExpressionType,
        min: f32,
        max: f32,
        default_value: f32,
        flag: Option<&str>,
        custom_default_value: Option<f32>,
        skip_output_if_default: bool,
    ) -> Self {
        let abbr = abbr.into().to_ascii_lowercase();
        let default_value = default_value.clamp(min, max);
        let custom_default_value = custom_default_value.map(|v| v.clamp(min, max));
        let is_flag = flag.is_some();
        UExpressionDescriptor {
            name: name.into(),
            abbr,
            r#type,
            min,
            max,
            default_value,
            custom_default_value: match custom_default_value {
                Some(v) if v == default_value => None,
                other => other,
            },
            is_flag,
            flag: flag.map(str::to_string),
            options: None,
            skip_output_if_default,
        }
    }

    /// Numerical expression constructor (no custom default).
    pub fn numerical(
        name: impl Into<String>,
        abbr: impl Into<String>,
        min: f32,
        max: f32,
        default_value: f32,
        flag: Option<&str>,
    ) -> Self {
        Self::new(name, abbr, UExpressionType::Numerical, min, max, default_value, flag, None, false)
    }

    /// Curve expression constructor.
    pub fn curve(
        name: impl Into<String>,
        abbr: impl Into<String>,
        min: f32,
        max: f32,
        default_value: f32,
    ) -> Self {
        Self::new(name, abbr, UExpressionType::Curve, min, max, default_value, None, None, false)
    }

    /// Options expression constructor: `type = Options`, `min = 0`,
    /// `max = options.len() - 1` (which is `-1` for an empty option list,
    /// exactly like OpenUtau's `clr` descriptor).
    pub fn options(
        name: impl Into<String>,
        abbr: impl Into<String>,
        is_flag: bool,
        options: Vec<String>,
    ) -> Self {
        let max = options.len() as f32 - 1.0;
        UExpressionDescriptor {
            name: name.into(),
            abbr: abbr.into().to_ascii_lowercase(),
            r#type: UExpressionType::Options,
            min: 0.0,
            max,
            default_value: 0.0,
            custom_default_value: None,
            is_flag,
            flag: None,
            options: Some(options),
            skip_output_if_default: false,
        }
    }

    /// The effective default value: `custom_default_value` if set,
    /// otherwise `default_value` (C# `CustomDefaultValue`).
    pub fn custom_default_value(&self) -> f32 {
        self.custom_default_value.unwrap_or(self.default_value)
    }

    /// Set the custom default value; values equal to `default_value` reset
    /// it to `None` (C# `CustomDefaultValue` setter).
    pub fn set_custom_default_value(&mut self, value: f32) {
        self.custom_default_value = if value == self.default_value { None } else { Some(value) };
    }

    /// Create a `UExpression` initialized to the default value.
    pub fn create(&self) -> UExpression {
        UExpression { index: None, abbr: self.abbr.clone(), value: self.default_value }
    }
}

// ---------------------------------------------------------------------------
// UExpression
// ---------------------------------------------------------------------------

/// Value for each phoneme (`UExpression`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UExpression {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<i32>,
    #[serde(default)]
    pub abbr: String,
    #[serde(default)]
    pub value: f32,
}

impl UExpression {
    /// Clamp `value` into `[min, max]` like the C# `value` setter, except
    /// for `clr` which is never clamped.
    pub fn clamp_value(&mut self, min: f32, max: f32) {
        if self.abbr != CLR {
            self.value = self.value.clamp(min, max);
        }
    }

    /// Consuming variant of [`clamp_value`](Self::clamp_value).
    pub fn clamped(mut self, min: f32, max: f32) -> Self {
        self.clamp_value(min, max);
        self
    }
}

// ---------------------------------------------------------------------------
// AddDefaultExpressions
// ---------------------------------------------------------------------------

/// Register OpenUtau's default expressions on a project
/// (`Format.Ustx.AddDefaultExpressions`). Existing expressions with the same
/// abbreviation are preserved (see [`UProject::register_expression`]).
pub fn add_default_expressions(project: &mut UProject) {
    project.register_expression(UExpressionDescriptor::curve("dynamics (curve)", DYN, -240.0, 120.0, 0.0));
    project.register_expression(UExpressionDescriptor::curve("pitch deviation (curve)", PITD, -1200.0, 1200.0, 0.0));
    project.register_expression(UExpressionDescriptor::options("voice color", CLR, false, vec![]));
    project.register_expression(UExpressionDescriptor::options("resampler engine", ENG, false, ENG_OPTIONS.map(str::to_string).to_vec()));
    project.register_expression(UExpressionDescriptor::numerical("velocity", VEL, 0.0, 200.0, 100.0, None));
    project.register_expression(UExpressionDescriptor::numerical("volume", VOL, 0.0, 200.0, 100.0, None));
    project.register_expression(UExpressionDescriptor::numerical("attack", ATK, 0.0, 200.0, 100.0, None));
    project.register_expression(UExpressionDescriptor::numerical("decay", DEC, 0.0, 100.0, 0.0, None));
    project.register_expression(UExpressionDescriptor::numerical("gender", GEN, -100.0, 100.0, 0.0, Some("g")));
    project.register_expression(UExpressionDescriptor::curve("gender (curve)", GENC, -100.0, 100.0, 0.0));
    project.register_expression(UExpressionDescriptor::numerical("breath", BRE, 0.0, 100.0, 0.0, Some("B")));
    project.register_expression(UExpressionDescriptor::curve("breathiness (curve)", BREC, -100.0, 100.0, 0.0));
    project.register_expression(UExpressionDescriptor::numerical("lowpass", LPF, 0.0, 100.0, 0.0, Some("H")));
    project.register_expression(UExpressionDescriptor::numerical("normalize", NORM, 0.0, 100.0, 86.0, Some("P")));
    project.register_expression(UExpressionDescriptor::numerical("modulation", MOD, 0.0, 100.0, 0.0, None));
    project.register_expression(UExpressionDescriptor::numerical("modulation plus", MODP, 0.0, 100.0, 0.0, None));
    project.register_expression(UExpressionDescriptor::numerical("alternate", ALT, 0.0, 16.0, 0.0, None));
    project.register_expression(UExpressionDescriptor::options("direct", DIR, false, vec!["off".into(), "on".into()]));
    project.register_expression(UExpressionDescriptor::numerical("tone shift", SHFT, -36.0, 36.0, 0.0, None));
    project.register_expression(UExpressionDescriptor::curve("tone shift (curve)", SHFC, -1200.0, 1200.0, 0.0));
    project.register_expression(UExpressionDescriptor::curve("tension (curve)", TENC, -100.0, 100.0, 0.0));
    project.register_expression(UExpressionDescriptor::curve("voicing (curve)", VOIC, 0.0, 100.0, 100.0));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_serde_names_and_ints() {
        assert_eq!(serde_yaml::to_string(&UExpressionType::Curve).unwrap().trim(), "Curve");
        assert_eq!(serde_yaml::from_str::<UExpressionType>("Curve").unwrap(), UExpressionType::Curve);
        assert_eq!(serde_yaml::from_str::<UExpressionType>("curve").unwrap(), UExpressionType::Curve);
        assert_eq!(serde_yaml::from_str::<UExpressionType>("2").unwrap(), UExpressionType::Curve);
        assert_eq!(serde_yaml::from_str::<UExpressionType>("Numerical").unwrap(), UExpressionType::Numerical);
        assert!(serde_yaml::from_str::<UExpressionType>("nope").is_err());
    }

    #[test]
    fn descriptor_yaml_shape() {
        let d = UExpressionDescriptor::numerical("gender", "GEN", -100.0, 100.0, 0.0, Some("g"));
        let yaml = serde_yaml::to_string(&d).unwrap();
        assert!(yaml.contains("abbr: gen"));
        assert!(yaml.contains("flag: g"));
        assert!(yaml.contains("is_flag: true"));
        assert!(yaml.contains("type: Numerical"));
        assert!(!yaml.contains("_custom_default_value"));
        let back: UExpressionDescriptor = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(back, d);
    }
}
