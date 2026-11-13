//! Thai orthographic-to-VCCV conversion ported from OpenUtau's
//! `ThaiVCCVPhonemizer`. This module only converts a single lyric into the
//! phonetic tokens used by the composition layer; note-neighbour context and
//! timing remain the responsibility of a future phonemizer adapter.

use regex::Regex;
use std::sync::OnceLock;

const CONSONANT: &str = r"([ก-ฮ][ลรว]?|อ[ย]?|ห[ก-ฮ]?)";

const VOWELS: &[(&str, &str)] = &[
    ("เcือะ", "6"),
    ("เcือx", "6"),
    ("แcะ", "@"),
    ("แcx", "@"),
    ("เcอะ", "3"),
    ("เcอ", "3"),
    ("ไc", "I"),
    ("ใc", "I"),
    ("เcาะ", "Q"),
    ("cอx", "Q"),
    ("cืx", "1"),
    ("cึx", "1"),
    ("cือ", "1"),
    ("cะ", "a"),
    ("cัx", "a"),
    ("cาx", "a"),
    ("เcา", "8"),
    ("เcะ", "e"),
    ("เcx", "e"),
    ("cิx", "i"),
    ("cีx", "i"),
    ("เcียะ", "ia"),
    ("เcียx", "ia"),
    ("โcะ", "o"),
    ("โcx", "o"),
    ("cุx", "u"),
    ("cูx", "u"),
    ("cัวะ", "ua"),
    ("cัว", "ua"),
    ("cำ", "am"),
    ("เcิx", "3"),
    ("เcิ", "3"),
];

const C_MAP: &[(char, &str)] = &[
    ('ก', "k"),
    ('ข', "kh"),
    ('ค', "kh"),
    ('ฆ', "kh"),
    ('ฅ', "kh"),
    ('ฃ', "kh"),
    ('จ', "j"),
    ('ฉ', "ch"),
    ('ช', "ch"),
    ('ฌ', "ch"),
    ('ฎ', "d"),
    ('ด', "d"),
    ('ต', "t"),
    ('ฏ', "t"),
    ('ถ', "th"),
    ('ฐ', "th"),
    ('ฑ', "th"),
    ('ธ', "th"),
    ('ท', "th"),
    ('บ', "b"),
    ('ป', "p"),
    ('พ', "ph"),
    ('ผ', "ph"),
    ('ภ', "ph"),
    ('ฟ', "f"),
    ('ฝ', "f"),
    ('ห', "h"),
    ('ฮ', "h"),
    ('ม', "m"),
    ('น', "n"),
    ('ณ', "n"),
    ('ร', "r"),
    ('ล', "l"),
    ('ฤ', "r"),
    ('ส', "s"),
    ('ศ', "s"),
    ('ษ', "s"),
    ('ซ', "s"),
    ('ง', "g"),
    ('ย', "y"),
    ('ญ', "y"),
    ('ว', "w"),
    ('ฬ', "r"),
];

const X_MAP: &[(char, &str)] = &[
    ('บ', "b"),
    ('ป', "b"),
    ('พ', "b"),
    ('ฟ', "b"),
    ('ภ', "b"),
    ('ด', "d"),
    ('จ', "d"),
    ('ช', "d"),
    ('ซ', "d"),
    ('ฎ', "d"),
    ('ฏ', "d"),
    ('ฐ', "d"),
    ('ฑ', "d"),
    ('ฒ', "d"),
    ('ต', "d"),
    ('ถ', "d"),
    ('ท', "d"),
    ('ธ', "d"),
    ('ศ', "d"),
    ('ษ', "d"),
    ('ส', "d"),
    ('ก', "k"),
    ('ข', "k"),
    ('ค', "k"),
    ('ฆ', "k"),
    ('ว', "w"),
    ('ย', "y"),
    ('น', "n"),
    ('ญ', "n"),
    ('ณ', "n"),
    ('ร', "n"),
    ('ล', "n"),
    ('ฬ', "n"),
    ('ง', "g"),
    ('ม', "m"),
];

const CONSONANTS: &[&str] = &[
    "kh", "ch", "ph", "th", "b", "d", "f", "g", "h", "j", "k", "l", "m", "n", "p", "r", "s", "t",
    "w", "y",
];
const GLIDES: &[&str] = &["r", "l", "w"];
const PHONE_VOWELS: &[&str] = &[
    "ia", "ua", "am", "a", "i", "u", "e", "o", "@", "Q", "3", "6", "1", "8",
];

/// Parsed VCCV components. Tokens are returned in the sequence used for the
/// universal-phone view; aliases are separately derived against the bank.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThaiPhones {
    pub onset: Option<String>,
    pub glide: Option<String>,
    pub vowel: Option<String>,
    pub coda: Option<String>,
}

impl ThaiPhones {
    pub fn tokens(&self) -> Vec<String> {
        [
            self.onset.as_ref(),
            self.glide.as_ref(),
            self.vowel.as_ref(),
            self.coda.as_ref(),
        ]
        .into_iter()
        .flatten()
        .filter(|s| !s.is_empty())
        .cloned()
        .collect()
    }
}

/// Convert Thai spelling (or an already-converted VCCV spelling) into its
/// longest-match onset/glide/vowel/coda decomposition.
pub fn parse(input: &str) -> Option<ThaiPhones> {
    if input.trim().is_empty() {
        return None;
    }
    let phonetic = word_to_phonemes(input);
    let mut rest = phonetic.as_str();
    let onset = take_longest(&mut rest, CONSONANTS);
    let glide = take_longest(&mut rest, GLIDES);
    let vowel = take_longest(&mut rest, PHONE_VOWELS);
    let coda = if rest.is_empty() {
        None
    } else {
        take_longest(&mut rest, CONSONANTS)
    };
    if !rest.is_empty() {
        return None;
    }
    Some(ThaiPhones {
        onset,
        glide,
        vowel,
        coda,
    })
}

/// Apply the ordered OpenUtau vowel-pattern rules and the same length-based
/// fallback for unparsed Thai spellings.
pub fn word_to_phonemes(input: &str) -> String {
    let stripped: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, '่' | '้' | '๊' | '๋' | '็'))
        .collect();
    let chars: Vec<char> = stripped.chars().collect();
    let mut clean = String::new();
    let mut i = 0;
    while i < chars.len() {
        if i + 1 < chars.len() && chars[i + 1] == '์' {
            i += 2;
        } else {
            clean.push(chars[i]);
            i += 1;
        }
    }
    if !clean.chars().any(|c| ('ก'..='ฮ').contains(&c)) {
        return clean;
    }
    let chars: Vec<char> = clean.chars().collect();

    for (re, vowel) in vowel_patterns() {
        let Some(captures) = re.captures(&clean) else {
            continue;
        };
        let onset = captures.get(1).map(|m| m.as_str()).unwrap_or("");
        let ending = captures.get(2).map(|m| m.as_str()).unwrap_or("");
        let onset = if onset.chars().count() >= 2
            && (onset.starts_with('ห') || onset.starts_with('อ'))
        {
            onset.chars().skip(1).collect::<String>()
        } else {
            onset.to_string()
        };
        let c = convert_c(&onset);
        let x = convert_x(ending);
        if *vowel == "a" && clean.contains('ั') && ending == "ว" {
            return format!("{c}ua");
        }
        if *vowel == "e" && ending == "ย" {
            return format!("{c}3{x}");
        }
        return format!("{c}{vowel}{x}");
    }

    let n = chars.len();
    match n {
        1 => format!("{}Q", convert_c(&clean)),
        2 => format!(
            "{}o{}",
            convert_c(&chars[0].to_string()),
            convert_x(&chars[1].to_string())
        ),
        3 if chars[1] == 'ว' => format!(
            "{}ua{}",
            convert_c(&chars[0].to_string()),
            convert_x(&chars[2].to_string())
        ),
        3 => format!(
            "{}o{}",
            convert_c(&chars[..2].iter().collect::<String>()),
            convert_x(&chars[1].to_string())
        ),
        4 if chars[2] == 'ว' => format!(
            "{}ua{}",
            convert_c(&chars[..2].iter().collect::<String>()),
            convert_x(&chars[3].to_string())
        ),
        _ => clean,
    }
}

fn vowel_patterns() -> &'static [(Regex, &'static str)] {
    static PATTERNS: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        VOWELS
            .iter()
            .filter_map(|(pattern, vowel)| {
                let expression = pattern.replace("c", CONSONANT).replace("x", "([ก-ฮ]?)");
                Regex::new(&format!("^{expression}$"))
                    .ok()
                    .map(|re| (re, *vowel))
            })
            .collect()
    })
}

fn convert_c(input: &str) -> String {
    let mut chars = input.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let Some((_, mapped)) = C_MAP.iter().find(|(ch, _)| *ch == first) else {
        return input.to_string();
    };
    let mut result = (*mapped).to_string();
    if let Some(second) = chars.next() {
        if let Some((_, mapped)) = C_MAP.iter().find(|(ch, _)| *ch == second) {
            result.push_str(mapped);
        }
    }
    result
}

fn convert_x(input: &str) -> String {
    let Some(first) = input.chars().next() else {
        return String::new();
    };
    X_MAP
        .iter()
        .find(|(ch, _)| *ch == first)
        .map(|(_, mapped)| (*mapped).to_string())
        .unwrap_or_else(|| input.to_string())
}

fn take_longest(rest: &mut &str, choices: &[&str]) -> Option<String> {
    let choice = choices.iter().find(|choice| rest.starts_with(**choice))?;
    *rest = &rest[choice.len()..];
    Some((*choice).to_string())
}
