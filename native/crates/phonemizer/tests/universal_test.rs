//! UniversalPhonemizer (L1+L2 wired into the Phonemizer trait) — Task 5.
//!
//! Path per note:
//!   lyric with [hint]      → hint tokens (authoritative, unchanged path)
//!   lyric without hint     → detect language → IPA (universal) →
//!                            IpaAdapter maps IPA → bank aliases via the
//!                            manifest; unknown phonemes fall back to
//!                            the lyric verbatim.
//!
//! The existing English/Japanese phonemizers keep working (regression
//! gate): `process` on hinted lyrics must be identical.

use phonemizer::adapter::IpaAdapter;
use phonemizer::ipa::IpaDictionary;
use phonemizer::universal::UniversalPhonemizer;
use phonemizer::Phonemizer;
use voicebank::load_voicebank;
use voicebank::manifest::PhonemeManifest;

fn teto() -> (voicebank::Voicebank, PhonemeManifest) {
    let lib = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../test/golden/teto-english/library");
    let vb = load_voicebank(&lib).expect("load voicebank");
    let manifest = vb.phoneme_manifest();
    (vb, manifest)
}

fn note(lyric: &str, tone: i32) -> domain::UNote {
    let mut n = domain::UNote::default();
    n.lyric = lyric.to_string();
    n.tone = tone;
    n.position = 0;
    n.duration = 480;
    n
}

#[test]
fn hinted_lyric_keeps_existing_path() {
    let (vb, manifest) = teto();
    let dict = IpaDictionary::en_subset();
    let adapter = IpaAdapter::new(&manifest, &dict);
    let ph = UniversalPhonemizer::new(adapter);

    // "la[l A]" — hint authoritative: two phonemes l / A resolved to
    // aliases via the oto table.
    let notes = [note("la[l A]", 60)];
    let out = ph.process(&notes, Some(&vb));
    assert_eq!(out.len(), 2);
    // The aliases must be renderable by the bank.
    for p in &out {
        assert!(
            vb.lookup_plain(&p.phoneme).is_some() || vb.lookup(&p.phoneme, p.tone).is_some(),
            "unresolved phoneme: {}",
            p.phoneme
        );
    }
}

#[test]
fn unhinted_english_lyric_goes_through_ipa() {
    let (vb, manifest) = teto();
    let dict = IpaDictionary::en_subset();
    let adapter = IpaAdapter::new(&manifest, &dict);
    let ph = UniversalPhonemizer::new(adapter);

    // "na" (no hint): the Teto bank HAS a single `na` alias, so the
    // direct-alias lookup wins — ONE phoneme, exactly like golden
    // (C# OpenUtau) renders it. This keeps the phoneme boundary
    // identical to the reference (waveform correlation depends on it).
    let notes = [note("na", 60)];
    let out = ph.process(&notes, Some(&vb));
    assert!(!out.is_empty());
    let phonemes: Vec<&str> = out.iter().map(|p| p.phoneme.as_str()).collect();
    assert_eq!(
        phonemes,
        ["na"],
        "bank has direct alias 'na' — use it, not l+A split: {phonemes:?}"
    );
}

#[test]
fn unhinted_japanese_lyric_detects_and_runs() {
    let (vb, manifest) = teto();
    let dict = IpaDictionary::en_subset();
    let adapter = IpaAdapter::new(&manifest, &dict);
    let ph = UniversalPhonemizer::new(adapter);

    // Kana lyric → Japanese path (detect) → falls back to the lyric
    // verbatim when the English subset dict cannot map it (no panic, no
    // empty output).
    let notes = [note("ら", 60)];
    let out = ph.process(&notes, Some(&vb));
    assert!(
        !out.is_empty(),
        "kana lyric must produce at least one phoneme"
    );
}

#[test]
fn unknown_word_falls_back_to_lyric() {
    let (vb, manifest) = teto();
    let dict = IpaDictionary::en_subset();
    let adapter = IpaAdapter::new(&manifest, &dict);
    let ph = UniversalPhonemizer::new(adapter);

    // A word whose letters are NOT resolvable ARPABET symbols ("q" has
    // no single-phoneme alias in the bank) → lyric verbatim.
    let notes = [note("qq", 60)];
    let out = ph.process(&notes, Some(&vb));
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].phoneme, "qq");
}

#[test]
fn letter_split_resolves_zzz_to_phonemes() {
    // "zzz": every letter is a known symbol → letter-split path produces
    // three phonemes (each z resolves to the bank's z alias).
    let (vb, manifest) = teto();
    let dict = IpaDictionary::en_subset();
    let adapter = IpaAdapter::new(&manifest, &dict);
    let ph = UniversalPhonemizer::new(adapter);

    let notes = [note("zzz", 60)];
    let out = ph.process(&notes, Some(&vb));
    assert_eq!(out.len(), 3);
    for p in &out {
        assert!(
            vb.lookup_plain(&p.phoneme).is_some(),
            "unresolved: {}",
            p.phoneme
        );
    }
}
