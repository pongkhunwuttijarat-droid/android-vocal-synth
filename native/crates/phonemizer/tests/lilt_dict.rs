//! Tests for the Lilt demo G2p dictionary: words must map to phoneme
//! sequences that EXIST in the Teto oto table (the golden voicebank).

use phonemizer::english::EnglishCvvcPhonemizer;
use phonemizer::lilt_dict::lilt_demo_g2p;
use phonemizer::phonemizer::Phonemizer;

fn note(lyric: &str) -> domain::UNote {
    domain::UNote {
        lyric: lyric.to_string(),
        tone: 60,
        position: 0,
        duration: 480,
        ..Default::default()
    }
}

fn teto() -> voicebank::Voicebank {
    // The golden Teto English library (CVVC aliases like `3`, `A`, `e d`).
    let dir = std::path::Path::new(
        "/home/seal/project/android-voice-synth/test/golden/teto-english/library",
    );
    voicebank::load_voicebank(dir).expect("load Teto library")
}

#[test]
fn demo_words_resolve_to_oto_aliases() {
    let vb = teto();
    let g2p = lilt_demo_g2p();
    let phonemizer = EnglishCvvcPhonemizer::with_g2p(g2p);

    for word in [
        "mi", "dnight", "in", "the", "city", "lights", "glow", "ooh", "ah", "ding", "hi", "hiiiii",
        "wander", "spell", "parallel", "true", "like", "heart", "sings", "chorus", "tune", "shelf",
        "myself", "feel", "real", "teach", "love", "me", "be",
    ] {
        let phs = phonemizer.process(&[note(word)], Some(&vb));
        assert!(!phs.is_empty(), "{word}: phonemizer produced nothing");
        // Every phoneme must resolve to an oto alias in the bank.
        for ph in &phs {
            let ok = vb.lookup(&ph.phoneme, 60).is_some()
                || vb.lookup(&ph.phoneme, 60).is_some();
            assert!(ok, "{word}: phoneme '{}' has no oto alias", ph.phoneme);
        }
    }
}

#[test]
fn unknown_word_falls_back_verbatim() {
    let vb = teto();
    let g2p = lilt_demo_g2p();
    let phonemizer = EnglishCvvcPhonemizer::with_g2p(g2p);
    let phs = phonemizer.process(&[note("zzzznope")], Some(&vb));
    // Verbatim fallback: one phoneme named after the lyric, which then has
    // no oto alias — the caller (pipeline) reports it as skipped.
    assert_eq!(phs.len(), 1);
    assert_eq!(phs[0].phoneme, "zzzznope");
}
