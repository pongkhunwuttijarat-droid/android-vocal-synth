//! Voicebank metadata: `character.txt` and `character.yaml`.
//!
//! `character.txt` is the legacy UTAU format, typically Shift-JIS encoded,
//! with `key=value` / `key:value` / `key：value` lines. `character.yaml` is
//! the modern OpenUtau format (UTF-8); its non-empty fields override
//! character.txt, mirroring `VoicebankLoader.ApplyConfig`.

use std::collections::{BTreeSet, HashMap};

use serde::Deserialize;

use crate::text;
use crate::tone;

/// A voice color / tone-range subbank (from character.yaml `subbanks` or
/// prefix.map). `tones` is the expanded set of note numbers covered by
/// `tone_ranges`, used for tone-based lookup.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Subbank {
    /// Voice color, e.g. "power", "whisper". Empty for the main bank.
    pub color: String,
    /// Alias prefix prepended to the phoneme (e.g. `P_`).
    pub prefix: String,
    /// Alias suffix appended to the phoneme (e.g. `_P`).
    pub suffix: String,
    /// Tone ranges as strings, e.g. `["C1-C4", "C#4"]`.
    pub tone_ranges: Vec<String>,
    /// Expanded note numbers covered by `tone_ranges`.
    pub tones: BTreeSet<i32>,
}

impl Subbank {
    /// Build a subbank from tone-range strings like `"C1-C4"` or `"C4"`.
    pub fn from_ranges(
        color: impl Into<String>,
        prefix: impl Into<String>,
        suffix: impl Into<String>,
        tone_ranges: Vec<String>,
    ) -> Subbank {
        let tones = tone_ranges
            .iter()
            .filter_map(|r| parse_range(r))
            .flatten()
            .collect();
        Subbank {
            color: color.into(),
            prefix: prefix.into(),
            suffix: suffix.into(),
            tone_ranges,
            tones,
        }
    }

    pub fn contains_tone(&self, tone: i32) -> bool {
        self.tones.contains(&tone)
    }
}

/// Parse `"C1-C4"` or `"C4"` into note numbers (inclusive).
fn parse_range(range: &str) -> Option<Vec<i32>> {
    let range = range.trim();
    if range.is_empty() {
        return None;
    }
    if let Some((lo, hi)) = range.split_once('-') {
        let lo = tone::name_to_tone(lo)?;
        let hi = tone::name_to_tone(hi)?;
        if hi < lo {
            return None;
        }
        Some((lo..=hi).collect())
    } else {
        tone::name_to_tone(range).map(|t| vec![t])
    }
}

/// Parsed `character.txt`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CharacterTxt {
    pub name: Option<String>,
    pub image: Option<String>,
    pub author: Option<String>,
    pub voice: Option<String>,
    pub sample: Option<String>,
    pub web: Option<String>,
    pub version: Option<String>,
    /// Lines that matched no known key, joined with `\n`.
    pub other_info: String,
}

/// Parse `character.txt` bytes. `declared` is the encoding label from
/// character.yaml's `text_file_encoding` (falls back to sniffing / Shift-JIS).
pub fn parse_character_txt(bytes: &[u8], declared: Option<&str>) -> CharacterTxt {
    let text = text::decode(bytes, declared);
    let mut cfg = CharacterTxt::default();
    let mut other: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r').trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = split_kv(line) else {
            other.push(line.to_string());
            continue;
        };
        match key.as_str() {
            "name" => cfg.name = Some(value),
            // Japanese legacy key; only used when no English "name" yet.
            "名前" => {
                if cfg.name.is_none() {
                    cfg.name = Some(value);
                }
            }
            "image" => cfg.image = Some(value),
            "author" | "created by" => cfg.author = Some(value),
            "cv" => cfg.voice = Some(value),
            k if k.starts_with("voice") => cfg.voice = Some(value),
            "sample" => cfg.sample = Some(value),
            "web" => cfg.web = Some(value),
            "version" => cfg.version = Some(value),
            _ => other.push(line.to_string()),
        }
    }
    cfg.other_info = other.join("\n");
    cfg
}

/// Split a character.txt line into (lowercased key, value) on the first of
/// `=`, `:`, or fullwidth `：` (OpenUtau's separator order).
fn split_kv(line: &str) -> Option<(String, String)> {
    for sep in ['=', ':', '：'] {
        if let Some(i) = line.find(sep) {
            let key = line[..i].trim().to_ascii_lowercase();
            let value = line[i + sep.len_utf8()..].trim().to_string();
            return Some((key, value));
        }
    }
    None
}

/// `character.yaml`, the OpenUtau metadata format.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct VoicebankConfigYaml {
    pub name: Option<String>,
    pub localized_names: HashMap<String, String>,
    pub image: Option<String>,
    pub portrait: Option<String>,
    pub portrait_opacity: Option<f32>,
    pub portrait_height: Option<i32>,
    pub author: Option<String>,
    pub voice: Option<String>,
    pub web: Option<String>,
    pub version: Option<String>,
    pub sample: Option<String>,
    pub default_phonemizer: Option<String>,
    pub text_file_encoding: Option<String>,
    pub singer_type: Option<String>,
    pub use_filename_as_alias: Option<bool>,
    pub subbanks: Vec<SubbankYaml>,
}

/// One entry of `character.yaml`'s `subbanks` list.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct SubbankYaml {
    pub color: Option<String>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub tone_ranges: Vec<String>,
}

/// Parse `character.yaml` (UTF-8).
pub fn parse_character_yaml(bytes: &[u8]) -> Result<VoicebankConfigYaml, serde_yaml::Error> {
    serde_yaml::from_slice(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TETO_LIKE: &str = "name=重音テト（かさねてと）音声ライブラリー\r\nimage=teto.bmp\r\n\
        sample=重音テト単独音\\_にゃ.wav\r\n性別:キメラ　年齢:31歳\r\n\
        好きなもの:フランスパン\r\n-------------------------:\r\n\
        web:http://kasaneteto.jp/\r\n重音テト音声ライブラリー:(C) 2008 All Rights Reserved.\r\n";

    #[test]
    fn parses_shift_jis_character_txt() {
        // Re-encode the fixture to Shift-JIS like the real Teto file.
        let bytes = TETO_LIKE
            .encode_shift_jis()
            .expect("fixture must be Shift-JIS encodable");
        let cfg = parse_character_txt(&bytes, None);
        assert_eq!(cfg.name.as_deref(), Some("重音テト（かさねてと）音声ライブラリー"));
        assert_eq!(cfg.image.as_deref(), Some("teto.bmp"));
        assert_eq!(cfg.web.as_deref(), Some("http://kasaneteto.jp/"));
        assert_eq!(cfg.sample.as_deref(), Some("重音テト単独音\\_にゃ.wav"));
        assert!(cfg.other_info.contains("性別"));
        assert!(cfg.other_info.contains("フランスパン"));
        assert!(cfg.other_info.contains("2008"));
    }

    #[test]
    fn parses_utf8_character_txt() {
        let cfg = parse_character_txt(b"Name = Teto English\ncreated by: Someone\n", None);
        assert_eq!(cfg.name.as_deref(), Some("Teto English"));
        assert_eq!(cfg.author.as_deref(), Some("Someone"));
    }

    #[test]
    fn japanese_name_key_only_when_no_english_name() {
        let cfg = parse_character_txt("名前：テト\n".as_bytes(), None);
        assert_eq!(cfg.name.as_deref(), Some("テト"));
        let cfg = parse_character_txt("name=Teto\n名前：テト\n".as_bytes(), None);
        assert_eq!(cfg.name.as_deref(), Some("Teto"));
    }

    #[test]
    fn parses_character_yaml() {
        let yaml = r#"
name: Teto Power
text_file_encoding: shift_jis
use_filename_as_alias: true
subbanks:
  - color: power
    prefix: P_
    suffix: _P
    tone_ranges:
      - C1-C4
      - C#4
  - prefix: ""
    suffix: ""
"#;
        let cfg: VoicebankConfigYaml = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.name.as_deref(), Some("Teto Power"));
        assert_eq!(cfg.text_file_encoding.as_deref(), Some("shift_jis"));
        assert_eq!(cfg.use_filename_as_alias, Some(true));
        assert_eq!(cfg.subbanks.len(), 2);
        assert_eq!(cfg.subbanks[0].color.as_deref(), Some("power"));
        assert_eq!(cfg.subbanks[0].tone_ranges, vec!["C1-C4", "C#4"]);
    }

    #[test]
    fn subbank_ranges_expand() {
        let sb = Subbank::from_ranges("power", "P_", "_P", vec!["C1-C4".into(), "C#4".into()]);
        assert!(sb.contains_tone(24)); // C1
        assert!(sb.contains_tone(60)); // C4
        assert!(sb.contains_tone(61)); // C#4
        assert!(!sb.contains_tone(62)); // D4
        assert!(!sb.contains_tone(23)); // B0
        // C1-C4 = 37 tones + C#4 = 38.
        assert_eq!(sb.tones.len(), 38);
    }

    #[test]
    fn single_tone_range_and_garbage() {
        let sb = Subbank::from_ranges("", "", "", vec!["C4".into(), "nonsense".into()]);
        assert!(sb.contains_tone(60));
        assert!(!sb.contains_tone(61));
        let sb = Subbank::from_ranges("", "", "", vec!["D5-C3".into()]); // inverted
        assert!(sb.tones.is_empty());
    }

    trait ShiftJisExt {
        fn encode_shift_jis(&self) -> Option<Vec<u8>>;
    }
    impl ShiftJisExt for str {
        fn encode_shift_jis(&self) -> Option<Vec<u8>> {
            let (bytes, _, had_errors) = encoding_rs::SHIFT_JIS.encode(self);
            (!had_errors).then(|| bytes.into_owned())
        }
    }
}
