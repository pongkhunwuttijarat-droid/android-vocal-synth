//! IPA layer (L1) tests — universal IPA conversion + language detection.
//!
//! Task 2: ARPABET→IPA dictionary (subset that Teto English uses).
//! Task 3: detect the lyric's language (hiragana/katakana → Japanese,
//!         latin → English) so the right phonemization path runs.

use phonemizer::detect::{detect_language, Language};
use phonemizer::ipa::{ipa_to_symbols, IpaDictionary};

#[test]
fn ipa_maps_arpabet_symbols_to_ipa() {
    let dict = IpaDictionary::en_subset();
    // Known-good mappings from the plan:
    assert_eq!(dict.ipa("l"), Some("/l/"));
    assert_eq!(dict.ipa("A"), Some("/ɑ/"));
    assert_eq!(dict.ipa("aI"), Some("/aɪ/"));
    assert_eq!(dict.ipa("3"), Some("/ə/"));
}

#[test]
fn ipa_unknown_symbol_returns_none() {
    let dict = IpaDictionary::en_subset();
    assert_eq!(dict.ipa("zzzz"), None);
    assert_eq!(dict.ipa("R"), None); // not in Teto's subset
}

#[test]
fn ipa_sequence_builds_token_list() {
    let dict = IpaDictionary::en_subset();
    // "la[l A]" → /l/ /ɑ/
    let toks = ipa_to_symbols(&dict, &["l", "A"]);
    assert_eq!(toks, vec!["/l/", "/ɑ/"]);
}

#[test]
fn detect_latin_is_english() {
    assert_eq!(detect_language("hello"), Language::English);
    assert_eq!(detect_language("la"), Language::English);
    assert_eq!(detect_language("midnight"), Language::English);
}

#[test]
fn detect_hiragana_is_japanese() {
    assert_eq!(detect_language("ら"), Language::Japanese);
    assert_eq!(detect_language("さくら"), Language::Japanese);
    // Katakana too:
    assert_eq!(detect_language("サクラ"), Language::Japanese);
}

#[test]
fn detect_mixed_prefers_japanese_when_kana_present() {
    // A lyric containing kana is Japanese even if it also has latin
    // (e.g. "ド dō") — the phonemizer path is chosen by kana presence.
    assert_eq!(detect_language("ド"), Language::Japanese);
}
