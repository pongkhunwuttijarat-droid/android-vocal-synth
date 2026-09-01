//! Language detection (L1) — pick the phonemization path for a lyric.
//!
//! Detection is deliberately simple (POC): hiragana/katakana present →
//! Japanese, otherwise latin → English. Mirrors the existing
//! `translit.rs` kana-range logic so the two stay consistent.

/// Which phonemization path a lyric takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    English,
    Japanese,
}

/// Detect the language of a lyric from its script.
///
/// Kana (hiragana 3040-309F, katakana 30A0-30FF) marks Japanese — even a
/// mixed lyric ("ド dō") goes down the Japanese path. Everything else
/// (latin, digits, spaces, punctuation) is English.
pub fn detect_language(lyric: &str) -> Language {
    if lyric.chars().any(|c| {
        matches!(c as u32, 0x3040..=0x309F | 0x30A0..=0x30FF | 0x31F0..=0x31FF)
    }) {
        Language::Japanese
    } else {
        Language::English
    }
}
