//! Mora → English-symbol transliteration for cross-language synthesis:
//! Japanese lyrics (せんぼんざくら) sung with an ENGLISH voicebank (Teto
//! English). JapaneseVcvPhonemizer normally emits mora symbols (せ, ん, ...)
//! that only Japanese banks alias; when the bank is English, each mora is
//! transliterated to English phoneme symbols that the bank DOES alias —
//! e.g. せ → [s, e], ぼ → [b, O]. The symbol set mirrors Teto English's
//! oto (vowels a e i o u O 3 A ...; consonants b d D f g h k m n N p s t v z;
//! glides j w l r).

/// Transliterate a Japanese mora to English symbols, or None if the mora is
/// already English-safe (pure vowel / single consonant the bank aliases).
pub fn mora_to_english(mora: &str) -> Option<&'static [&'static str]> {
    match mora {
        // Vowels pass through as-is.
        "a" | "i" | "u" | "e" | "o" | "A" | "O" | "3" => None,
        "ka" => Some(&["k", "A"]),
        "ki" => Some(&["k", "i"]),
        "ku" => Some(&["k", "u"]),
        "ke" => Some(&["k", "e"]),
        "ko" => Some(&["k", "O"]),
        "sa" => Some(&["s", "A"]),
        "shi" => Some(&["s", "i"]),
        "su" => Some(&["s", "u"]),
        "se" => Some(&["s", "e"]),
        "so" => Some(&["s", "O"]),
        "ta" => Some(&["t", "A"]),
        "chi" => Some(&["t", "i"]),
        "tsu" => Some(&["t", "s", "u"]),
        "te" => Some(&["t", "e"]),
        "to" => Some(&["t", "O"]),
        "na" => Some(&["n", "A"]),
        "ni" => Some(&["n", "i"]),
        "nu" => Some(&["n", "u"]),
        "ne" => Some(&["n", "e"]),
        "no" => Some(&["n", "O"]),
        "ha" => Some(&["h", "A"]),
        "hi" => Some(&["h", "i"]),
        "fu" => Some(&["f", "u"]),
        "he" => Some(&["h", "e"]),
        "ho" => Some(&["h", "O"]),
        "ma" => Some(&["m", "A"]),
        "mi" => Some(&["m", "i"]),
        "mu" => Some(&["m", "u"]),
        "me" => Some(&["m", "e"]),
        "mo" => Some(&["m", "O"]),
        "ya" => Some(&["j", "A"]),
        "yu" => Some(&["j", "u"]),
        "yo" => Some(&["j", "O"]),
        "ra" => Some(&["r3", "A"]),
        "ri" => Some(&["r3", "i"]),
        "ru" => Some(&["r3", "u"]),
        "re" => Some(&["r3", "e"]),
        "ro" => Some(&["r3", "O"]),
        "wa" => Some(&["w", "A"]),
        "wo" => Some(&["w", "O"]),
        "ga" => Some(&["g", "A"]),
        "gi" => Some(&["g", "i"]),
        "gu" => Some(&["g", "u"]),
        "ge" => Some(&["g", "e"]),
        "go" => Some(&["g", "O"]),
        "za" => Some(&["z", "A"]),
        "ji" => Some(&["z", "i"]),
        "zu" => Some(&["z", "u"]),
        "ze" => Some(&["z", "e"]),
        "zo" => Some(&["z", "O"]),
        "da" => Some(&["d", "A"]),
        "de" => Some(&["d", "e"]),
        "do" => Some(&["d", "O"]),
        "ba" => Some(&["b", "A"]),
        "bi" => Some(&["b", "i"]),
        "bu" => Some(&["b", "u"]),
        "be" => Some(&["b", "e"]),
        "bo" => Some(&["b", "O"]),
        "pa" => Some(&["p", "A"]),
        "pi" => Some(&["p", "i"]),
        "pu" => Some(&["p", "u"]),
        "pe" => Some(&["p", "e"]),
        "po" => Some(&["p", "O"]),
        // Syllabic n and glides.
        "n" | "N" => Some(&["n"]),
        // Small-tsu (gemination) → double consonant is handled by caller
        // repeating the following symbol; fall through as consonant hold.
        "Q" => Some(&["-"]),
        // Everything else: let the normal path try the bank as-is.
        _ => None,
    }
}

/// True when [symbol] is an English vowel (used to decide bridging).
pub fn is_english_vowel(symbol: &str) -> bool {
    matches!(
        symbol,
        "a" | "e" | "i" | "o" | "u" | "A" | "E" | "I" | "O" | "U" | "3"
            | "aI" | "aU" | "eI" | "oU" | "@" | "{"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn senbonzakura_transliterates() {
        // せ ん ぼ ん ざ く ら → s e / n / b O / n / z A / k u / r3 A
        let expected: Vec<&[&str]> = vec![
            &["s", "e"],
            &["n"],
            &["b", "O"],
            &["n"],
            &["z", "A"],
            &["k", "u"],
            &["r3", "A"],
        ];
        let morae = ["se", "n", "bo", "n", "za", "ku", "ra"];
        for (m, exp) in morae.iter().zip(expected.iter()) {
            assert_eq!(mora_to_english(m).unwrap(), *exp, "mora {m}");
        }
    }

    #[test]
    fn vowels_pass_through() {
        for v in ["a", "i", "u", "e", "o", "A", "O", "3"] {
            assert_eq!(mora_to_english(v), None, "vowel {v}");
        }
        // Syllabic n (ん) is a consonant mora → maps to [n].
        assert_eq!(mora_to_english("N"), Some(&["n"][..]));
    }
}
