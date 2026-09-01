//! Phoneme → group mapping (plan Task 4): the MoE routes each frame to
//! a group expert by its phoneme's natural class. English symbols are
//! the Teto-english oto/ARPABET-ish set; anything unmapped lands in
//! "other" (O).
//!
//! Groups: 0 = vowel, 1 = plosive, 2 = fricative, 3 = nasal, 4 = other.

pub const N_GROUPS: usize = 5;
pub const G_VOWEL: usize = 0;
pub const G_PLOSIVE: usize = 1;
pub const G_FRICATIVE: usize = 2;
pub const G_NASAL: usize = 3;
pub const G_OTHER: usize = 4;

pub const GROUP_NAMES: [&str; N_GROUPS] = ["vowel", "plosive", "fricative", "nasal", "other"];

/// Vowel nuclei (single symbols and diphthongs).
const VOWELS: &[&str] = &[
    "a", "e", "i", "o", "u", "3", "@", "I", "E", "O", "U", "A", "V",
    "aI", "aU", "OI", "i3", "u@", "eI", "oU",
];
/// Plosives (stops).
const PLOSIVES: &[&str] = &["p", "b", "t", "d", "k", "g"];
/// Fricatives.
const FRICATIVES: &[&str] = &["f", "v", "s", "z", "S", "Z", "T", "D", "h", "C"];
/// Nasals.
const NASALS: &[&str] = &["m", "n", "N"];

/// Map a phoneme symbol to its group id. Teto CVVC aliases carry onset/
/// coda markers ("- n@", "@ l-") — strip them first so the first SOUND
/// classifies (the "-" prefix was sending every onset alias to "other").
pub fn group_of(phoneme: &str) -> usize {
    let p = phoneme.trim_start_matches("- ").trim_end_matches(" -");
    if VOWELS.contains(&p) {
        return G_VOWEL;
    }
    if PLOSIVES.contains(&p) {
        return G_PLOSIVE;
    }
    if FRICATIVES.contains(&p) {
        return G_FRICATIVE;
    }
    if NASALS.contains(&p) {
        return G_NASAL;
    }
    // Fallback: first char (handles "tS", "dZ", "aI"-style tokens).
    if let Some(c) = p.chars().next() {
        let s = c.to_string();
        if VOWELS.contains(&s.as_str()) {
            return G_VOWEL;
        }
        if PLOSIVES.contains(&s.as_str()) {
            return G_PLOSIVE;
        }
        if FRICATIVES.contains(&s.as_str()) {
            return G_FRICATIVE;
        }
        if NASALS.contains(&s.as_str()) {
            return G_NASAL;
        }
    }
    G_OTHER
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_symbols() {
        assert_eq!(group_of("a"), G_VOWEL);
        assert_eq!(group_of("3"), G_VOWEL);
        assert_eq!(group_of("k"), G_PLOSIVE);
        assert_eq!(group_of("s"), G_FRICATIVE);
        assert_eq!(group_of("m"), G_NASAL);
        assert_eq!(group_of("r"), G_OTHER); // liquid → other
        assert_eq!(group_of("w"), G_OTHER); // glide → other
    }

    /// Teto CVVC onset/coda markers must not break classification
    /// (regression: "- n@" was routing to OTHER because of the "- ").
    #[test]
    fn strips_cvvc_markers() {
        assert_eq!(group_of("- n@"), G_NASAL);
        assert_eq!(group_of("- Z"), G_FRICATIVE);
        assert_eq!(group_of("@ l-"), G_VOWEL);
        assert_eq!(group_of("- mi"), G_NASAL);
        assert_eq!(group_of("k l-"), G_PLOSIVE);
        assert_eq!(group_of("T@"), G_FRICATIVE);
    }

    #[test]
    fn falls_back_to_first_char() {
        assert_eq!(group_of("tS"), G_PLOSIVE); // 't' → plosive
        assert_eq!(group_of("dZ"), G_PLOSIVE);
    }

    #[test]
    fn five_groups() {
        assert_eq!(N_GROUPS, 5);
        assert_eq!(GROUP_NAMES.len(), N_GROUPS);
    }
}
