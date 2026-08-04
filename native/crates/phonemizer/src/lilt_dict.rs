//! Built-in English G2p dictionary for the Lilt demo voicebank (Teto
//! English CVVC). Maps the words the editor's demo song uses to phoneme
//! sequences that EXIST in the Teto oto table (vowel `aI` not `ay`, `D`
//! for th, `N` for ng, etc.).
//!
//! This is a POC vocabulary, not a full dictionary — words not listed here
//! fall back to the lyric verbatim (which then fails to map to an oto
//! alias unless it carries a phonetic hint). A real dictionary (CMUdict
//! subset) is a later milestone.

use crate::g2p::{G2pDictionary, G2pFallbacks};

/// The Lilt demo vocabulary: word → phoneme sequence (Teto symbols).
/// Order matters: `add_entry` overwrites, so list longer words first when
/// they share prefixes (none do here).
pub fn lilt_demo_g2p() -> G2pFallbacks {
    let mut dict = G2pDictionary::new();
    // Register symbols used by the vocabulary (vowel / glide / other).
    // Teto English CVVC symbol set (from the library's oto.ini).
    for v in ["3", "E", "A", "I", "O", "U", "a", "aI", "aU", "e", "eI", "i",
              "o", "oU", "u", "{", "@"] {
        dict.add_symbol(v, true, false);
    }
    for g in ["j", "w", "l", "r", "hi"] {
        dict.add_symbol(g, false, true);
    }
    for c in ["b", "d", "D", "f", "g", "h", "k", "m", "n", "N", "p", "s",
              "t", "v", "z", "-"] {
        dict.add_symbol(c, false, false);
    }
    // Words used by the editor's demo song (Lead Vocal + Harmony tracks).
    dict.add_entry("mi", &["m", "i"]);
    dict.add_entry("dnight", &["d", "n", "aI", "t"]);
    dict.add_entry("in", &["i", "n"]);
    dict.add_entry("the", &["D", "3"]);
    dict.add_entry("city", &["s", "i", "t", "i"]);
    dict.add_entry("lights", &["l", "aI", "t", "s"]);
    dict.add_entry("glow", &["g", "l", "oU"]);
    dict.add_entry("ooh", &["u"]);
    dict.add_entry("ah", &["A"]);
    dict.add_entry("ding", &["d", "i", "N"]);
    // "hi" is a single 2-token alias in Teto (no standalone `h`); stretched
    // variants repeat the vowel.
    dict.add_entry("hi", &["hi"]);
    dict.add_entry("hiiiii", &["hi", "i", "i", "i", "i"]);
    // Machine Love (Jamie Paige) — chorus vocabulary. No `h` in Teto, so
    // "heart" becomes the A t pair (ɑɹt-ish approximation).
    dict.add_entry("wander", &["w", "A", "n", "d", "3", "r3"]);
    dict.add_entry("spell", &["s", "p", "e", "l"]);
    dict.add_entry("parallel", &["p", "A", "r3", "A", "l", "e", "l"]);
    dict.add_entry("true", &["t", "r3", "u"]);
    dict.add_entry("like", &["l", "aI", "k"]);
    dict.add_entry("heart", &["A", "t"]);
    dict.add_entry("sings", &["s", "i", "N", "z"]);
    dict.add_entry("chorus", &["k", "O", "r3", "u", "s"]);
    dict.add_entry("tune", &["t", "u", "n"]);
    dict.add_entry("shelf", &["s", "e", "l", "f"]);
    dict.add_entry("myself", &["m", "aI", "s", "e", "l", "f"]);
    dict.add_entry("feel", &["f", "i", "l"]);
    dict.add_entry("real", &["r3", "i", "l"]);
    dict.add_entry("teach", &["t", "i", "t"]);
    dict.add_entry("love", &["l", "u", "v"]);
    dict.add_entry("me", &["m", "i"]);
    dict.add_entry("be", &["b", "i"]);
    // Function words + remaining chorus vocabulary (Machine Love). Teto
    // English has no standalone `a` — use `A` (ɑ) everywhere.
    dict.add_entry("a", &["A"]);
    dict.add_entry("and", &["A", "n", "d"]);
    dict.add_entry("can", &["k", "A", "n"]);
    dict.add_entry("could", &["k", "u", "d"]);
    dict.add_entry("for", &["f", "O", "r3"]);
    dict.add_entry("it", &["i", "t"]);
    dict.add_entry("leave", &["l", "i", "v"]);
    dict.add_entry("live", &["l", "i", "v"]);
    dict.add_entry("my", &["m", "aI"]);
    dict.add_entry("of", &["u", "v"]);
    dict.add_entry("on", &["O", "n"]);
    dict.add_entry("out", &["aU", "t"]);
    dict.add_entry("want", &["w", "A", "n", "t"]);
    dict.add_entry("you", &["j", "u"]);
    dict.add_entry("so", &["s", "oU"]);
    dict.add_entry("we", &["w", "i"]);
    G2pFallbacks::new(vec![Box::new(dict)])
}
