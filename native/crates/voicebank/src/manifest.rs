//! Voicebank phoneme manifest (L3) — what phonemes this bank can sing.
//!
//! Derived from the bank's oto.ini: the manifest is the mapping target
//! the IPA adapter (L2) consults. Engine-agnostic by design — this
//! module only depends on `voicebank` + `std`; the engine's own
//! capabilities live behind the `EngineCapabilities` trait (domain).

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::Voicebank;

/// Broad articulation class used to constrain nearest-phoneme mapping
/// (vowel → vowel, consonant → consonant).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PhonemeType {
    Vowel,
    Consonant,
    /// e.g. `w`, `j` — glide/semivowel, treated as consonant for mapping.
    Glide,
}

/// What a voicebank can sing, derived from its oto.ini aliases.
///
/// - `phoneme_set`: every single-token alias (e.g. "A", "l", "r3").
/// - `types`: per-phoneme articulation class (oto consonant>0 →
///   consonant; glide list → glide; else vowel).
/// - `pairs`: every 2-token alias (e.g. "l A") — the CV/CVVC/VCV
///   structure the adapter uses to assemble sequences.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct PhonemeManifest {
    /// Single-token aliases, sorted for stable diffs.
    pub phoneme_set: BTreeSet<String>,
    /// Phoneme → articulation class.
    pub types: HashMap<String, PhonemeType>,
    /// 2-token alias pairs (token1, token2).
    pub pairs: HashSet<(String, String)>,
}

impl PhonemeManifest {
    /// Derive the manifest from a loaded voicebank's oto aliases.
    ///
    /// Classification rules:
    /// - alias tokens split on whitespace (OpenUtau aliases are
    ///   whitespace-separated phoneme sequences, e.g. "l A")
    /// - one token → single phoneme; two tokens → pair
    /// - type: `consonant > 0.0` in the oto line → Consonant; else if
    ///   the token is a known glide (w/j/y) → Glide; else Vowel.
    ///   (A CV sample like "la" has a consonant onset; a pure V sample
    ///   like "A" has consonant == 0.)
    pub fn from_voicebank(vb: &Voicebank) -> Self {
        let mut set = BTreeSet::new();
        let mut types: HashMap<String, PhonemeType> = HashMap::new();
        let mut pairs = HashSet::new();

        for oto in vb.otos.iter().filter(|o| o.is_valid) {
            let tokens: Vec<&str> = oto.alias.split_whitespace().collect();
            match tokens.len() {
                1 => {
                    let tok = tokens[0];
                    set.insert(tok.to_string());
                    types.entry(tok.to_string()).or_insert_with(|| classify(tok));
                }
                2 => {
                    pairs.insert((tokens[0].to_string(), tokens[1].to_string()));
                    // NOTE: pair tokens do NOT join `phoneme_set` — a
                    // token like `r` (only in "r b" aliases) has no
                    // single-phoneme sample, so the bank cannot sing it
                    // standalone. phoneme_set = single-token aliases ONLY
                    // (the mapping target must not over-report).
                    types
                        .entry(tokens[0].to_string())
                        .or_insert_with(|| classify(tokens[0]));
                    types
                        .entry(tokens[1].to_string())
                        .or_insert_with(|| classify(tokens[1]));
                }
                _ => {
                    // 3+ token aliases (rare): chain adjacent tokens.
                    for w in tokens.windows(2) {
                        pairs.insert((w[0].to_string(), w[1].to_string()));
                    }
                }
            }
        }

        PhonemeManifest { phoneme_set: set, types, pairs }
    }

    /// Can the bank render [phoneme] (single-token alias exists)?
    pub fn can_render(&self, phoneme: &str) -> bool {
        self.phoneme_set.contains(phoneme)
    }

    /// Phonemes of [phonemes] the bank cannot render.
    pub fn missing<'a>(&self, phonemes: &[&'a str]) -> Vec<&'a str> {
        phonemes
            .iter()
            .copied()
            .filter(|p| !self.can_render(p))
            .collect()
    }
}

fn is_glide(tok: &str) -> bool {
    matches!(tok, "w" | "j" | "y" | "w3" | "j3")
}

/// Known vowel tokens for the Teto English bank (ARPABET-style). The oto
/// `consonant` field is NOT a reliable classifier here — every CV sample
/// carries a consonant onset, so a heuristic "consonant > 0 → consonant"
/// would mislabel vowels like `A` (which appears in "A hA", "A A").
/// Vowel set is the mapping target's known-good subset (YAGNI: extend
/// per bank when needed; Japanese VCV uses its own classifier later).
const VOWELS: &[&str] = &[
    "A", "E", "I", "O", "U", "3", "i", "u", "e", "o", "a", "Q", "aI",
    "aU", "eI", "oU", "e3", "I3", "u3", "o3", "A3", "i3",
];

fn classify(tok: &str) -> PhonemeType {
    if is_glide(tok) {
        PhonemeType::Glide
    } else if VOWELS.contains(&tok) {
        PhonemeType::Vowel
    } else {
        PhonemeType::Consonant
    }
}
