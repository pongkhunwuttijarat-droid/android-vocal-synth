// พิมพ์ phonemes ที่ Machine Love chorus ผลิต — หา mismatch
use phonemizer::english::EnglishCvvcPhonemizer;
use phonemizer::lilt_dict::lilt_demo_g2p;
use phonemizer::phonemizer::Phonemizer;
use std::collections::BTreeSet;
use voicebank::load_voicebank;

fn main() {
    let vb = load_voicebank(std::path::Path::new(
        "/home/seal/project/android-voice-synth/test/golden/teto-english/library",
    ))
    .expect("load");
    let g2p = lilt_demo_g2p();
    let ph = EnglishCvvcPhonemizer::with_g2p(g2p);

    let words: Vec<&str> = "so can we wander for a spell and live in parallel i want it to be true to be like you my heart sings a chorus out of tune and i could leave it on a shelf"
        .split_whitespace()
        .collect();
    let mut bad: BTreeSet<String> = BTreeSet::new();
    let mut total = 0;
    for w in &words {
        for tone in [55, 60, 72] {
        let note = domain::UNote {
            lyric: w.to_string(),
            tone,
            position: 0,
            duration: 480,
            phoneme_expressions: Default::default(),
            phoneme_overrides: Default::default(),
            phonemizer_override: None,
            pitch: Default::default(),
            vibrato: Default::default(),
            tuning: 0,
        };
        let phs = ph.process(&[note], Some(&vb));
        for p in &phs {
            total += 1;
            if vb.lookup(&p.phoneme, tone).is_none() {
                bad.insert(format!("{}@t{tone}", p.phoneme));
            }
        }
        if phs.is_empty() {
            println!("{w}: NO PHONEMES");
        }
        for p in &phs {
            if vb.lookup(&p.phoneme, tone).is_none() {
                println!("  {w}@t{tone}: phoneme '{}' NO ALIAS", p.phoneme);
            }
        }
        }
    }
    println!("total phonemes: {total}, unmapped: {bad:?}");
    // พิมพ์ทุก phoneme ที่ process คืน เพื่อดู alias pairing
    for w in &words {
        let note = domain::UNote {
            lyric: w.to_string(),
            tone: 55,
            position: 0,
            duration: 480,
            phoneme_expressions: Default::default(),
            phoneme_overrides: Default::default(),
            phonemizer_override: None,
            pitch: Default::default(),
            vibrato: Default::default(),
            tuning: 0,
        };
        let phs = ph.process(&[note], Some(&vb));
        if !phs.is_empty() {
            let names: Vec<&str> = phs.iter().map(|p| p.phoneme.as_str()).collect();
            println!("  {w}: {names:?}");
        }
    }
}
