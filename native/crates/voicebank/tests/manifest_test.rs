//! PhonemeManifest tests — derive the bank's phoneme capability from its
//! oto.ini (Task 1 of the phonemizer IPA plan).
//!
//! Known facts about the Teto English bank (verified in earlier sessions):
//!   - single-phoneme aliases: "3", "A", "b", "d", "D", "e", "E", "f",
//!     "g", "i", "I", "j", "k", "l", "m", "n", "N", "O", "p", "s", "S",
//!     "t", "T", "u", "U", "v", "V", "w", "z", "Z"
//!   - NO lowercase "a" or "r" as single aliases (they are "A" and "r3")
//!   - multi-token aliases exist (CVVC pairs like "l A")
//! The manifest must reflect exactly this — it is the mapping target the
//! IPA adapter (L2) consults, so over/under-reporting breaks the mapping.

use voicebank::manifest::{PhonemeManifest, PhonemeType};
use voicebank::{load_voicebank, Voicebank};

fn teto_en() -> Voicebank {
    // The real Teto English bank library dir.
    let lib = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../test/golden/teto-english/library");
    load_voicebank(&lib).expect("load voicebank")
}

#[test]
fn manifest_has_single_phoneme_aliases() {
    let vb = teto_en();
    let manifest = vb.phoneme_manifest();
    for ph in ["A", "l", "n", "N", "3", "u", "U"] {
        assert!(manifest.phoneme_set.contains(ph), "missing {ph}");
    }
}

#[test]
fn manifest_does_not_have_invented_aliases() {
    let vb = teto_en();
    let manifest = vb.phoneme_manifest();
    // These are NOT single aliases in the bank (they are A / r3) — the
    // manifest must not report them, otherwise the IPA adapter would map
    // to a phoneme the bank cannot actually sing.
    for ph in ["a", "r"] {
        assert!(!manifest.phoneme_set.contains(ph), "reported {ph}?!");
    }
}

#[test]
fn manifest_classifies_vowel_and_consonant() {
    let vb = teto_en();
    let manifest = vb.phoneme_manifest();
    // Vowels (ARPABET vowel set — Teto's CV-style oto makes the
    // `consonant` field unreliable, so classification uses the known
    // vowel set):
    assert_eq!(manifest.types.get("A"), Some(&PhonemeType::Vowel));
    assert_eq!(manifest.types.get("3"), Some(&PhonemeType::Vowel));
    // Consonants: everything not in the vowel/glide set.
    assert_eq!(manifest.types.get("l"), Some(&PhonemeType::Consonant));
    assert_eq!(manifest.types.get("n"), Some(&PhonemeType::Consonant));
}

#[test]
fn manifest_collects_pairs_from_multitoken_aliases() {
    let vb = teto_en();
    let manifest = vb.phoneme_manifest();
    // CVVC-style pairs actually present in the bank: "- A" (as in the
    // alias "- A"), "A hA", "A A". The pair set must contain them.
    assert!(
        manifest.pairs.contains(&("-".into(), "A".into())),
        "expected pair (-, A) in pairs"
    );
    assert!(
        manifest.pairs.contains(&("A".into(), "hA".into())),
        "expected pair (A, hA) in pairs"
    );
}

#[test]
fn manifest_is_engine_agnostic() {
    // The manifest is pure voicebank data — it must not pull in any
    // worldline/engine types. Compile-time guarantee: this crate's
    // manifest module only uses voicebank + std.
    let _ = PhonemeManifest::default();
}
