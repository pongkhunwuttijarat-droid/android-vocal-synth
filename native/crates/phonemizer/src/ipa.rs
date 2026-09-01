//! IPA layer (L1) — universal phoneme representation.
//!
//! [`IpaDictionary`] maps ARPABET-style symbols (what the existing
//! English CVVC phonemizer and the Teto English bank use) to IPA
//! strings. The IPA token is the UNIVERSAL middle layer: lyrics of any
//! language first convert to IPA, then the L2 adapter maps IPA tokens to
//! the target voicebank's aliases through its manifest.
//!
//! YAGNI: only the subset the Teto banks use is implemented now (see
//! `en_subset`); extend per bank as needed.

use std::collections::HashMap;

/// ARPABET → IPA dictionary (universal middle layer, L1).
#[derive(Debug, Default)]
pub struct IpaDictionary {
    map: HashMap<String, &'static str>,
    /// Reverse: IPA string → ARPABET symbol (the bank's alias space).
    /// L2 needs this to find which alias an IPA token maps to.
    reverse: HashMap<&'static str, &'static str>,
}

impl IpaDictionary {
    /// The known-good subset for Teto English (ARPABET symbols that the
    /// bank + lilt_dict actually use).
    pub fn en_subset() -> Self {
        let mut d = IpaDictionary::default();
        // Vowels (monophthongs + diphthongs)
        for (arpabet, ipa) in [
            ("A", "/ɑ/"),
            ("E", "/ɛ/"),
            ("I", "/ɪ/"),
            ("O", "/ɔ/"),
            ("U", "/ʊ/"),
            ("3", "/ə/"),
            ("i", "/i/"),
            ("u", "/u/"),
            ("e", "/e/"),
            ("o", "/o/"),
            ("a", "/a/"),
            ("aI", "/aɪ/"),
            ("aU", "/aʊ/"),
            ("eI", "/eɪ/"),
            ("oU", "/oʊ/"),
        ] {
            d.map.insert(arpabet.to_string(), ipa);
            d.reverse.insert(ipa, arpabet);
            // Also register the bare IPA (no slashes) so both lookup
            // shapes work.
            d.reverse.insert(ipa.trim_matches('/'), arpabet);
        }
        // Consonants (subset Teto uses)
        for (arpabet, ipa) in [
            ("b", "/b/"),
            ("d", "/d/"),
            ("D", "/ð/"),
            ("f", "/f/"),
            ("g", "/g/"),
            ("h", "/h/"),
            ("j", "/j/"),
            ("k", "/k/"),
            ("l", "/l/"),
            ("m", "/m/"),
            ("n", "/n/"),
            ("N", "/ŋ/"),
            ("p", "/p/"),
            ("r", "/r/"),
            ("s", "/s/"),
            ("S", "/ʃ/"),
            ("t", "/t/"),
            ("T", "/θ/"),
            ("v", "/v/"),
            ("w", "/w/"),
            ("z", "/z/"),
            ("Z", "/ʒ/"),
        ] {
            d.map.insert(arpabet.to_string(), ipa);
            d.reverse.insert(ipa, arpabet);
            d.reverse.insert(ipa.trim_matches('/'), arpabet);
        }
        d
    }

    /// IPA string for an ARPABET symbol, or `None` when unknown.
    pub fn ipa(&self, arpabet: &str) -> Option<&'static str> {
        self.map.get(arpabet).copied()
    }

    /// ARPABET symbol for an IPA token (with or without `/` slashes),
    /// or `None` when the dict does not know it. L2's bridge from the
    /// universal IPA layer back into the bank's alias space.
    pub fn arpabet(&self, ipa_token: &str) -> Option<&'static str> {
        self.reverse.get(ipa_token).copied()
    }
}

/// Convert a sequence of ARPABET symbols to IPA tokens. Unknown symbols
/// pass through verbatim (they may be hint tokens like `SP`/`AP`, or a
/// future symbol the dict does not know yet).
pub fn ipa_to_symbols(dict: &IpaDictionary, symbols: &[&str]) -> Vec<String> {
    symbols
        .iter()
        .map(|s| dict.ipa(s).map(str::to_string).unwrap_or_else(|| (*s).to_string()))
        .collect()
}
