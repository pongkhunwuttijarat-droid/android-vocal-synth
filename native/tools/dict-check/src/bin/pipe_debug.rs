//! Debug: พิมพ์ phonemes หลัง pairing สำหรับ Machine Love chorus —
//! หา alias ที่ OtoMapper map ไม่ได้ (pipeline fail 60/65)
use domain::UPart;
use phrase::{PhraseBuilder, PhraseGrouping};
use synth_cli::pipeline::{self, PhonemizerKind};
use voicebank::load_voicebank as load_vb;

fn main() {
    let project = pipeline::load_project(std::path::Path::new("/tmp/ml_demo2.ustx"))
        .expect("load project");
    let vb = load_vb(std::path::Path::new(
        "/home/seal/project/android-voice-synth/test/golden/teto-english/library",
    ))
    .expect("load vb");
    let track = &project.tracks[0];
    let part = match &project.parts[0] {
        UPart::Voice(v) => v,
        UPart::Wave(_) => panic!(),
    };
    let phonemes = pipeline::derive_phonemes(&project, part, &vb, PhonemizerKind::English);
    let groups = PhraseGrouping::group(&phonemes);
    let builder = PhraseBuilder::new(&project, track, part, Some(&vb));
    for g in &groups {
        let input = builder.build(g).expect("build");
        let mapped: Vec<&str> = input
            .sample_based
            .as_ref()
            .map(|sb| sb.oto.iter().map(|o| o.alias.as_str()).collect())
            .unwrap_or_default();
        println!("phonemes ({}):", input.phonemes.len());
        for p in &input.phonemes {
            let ok = input
                .sample_based
                .as_ref()
                .map(|sb| sb.oto.iter().any(|o| o.alias == p.phoneme))
                .unwrap_or(false);
            println!("  {} {}", if ok { "✓" } else { "✗" }, p.phoneme);
        }
        println!("oto entries ({}): {:?}", mapped.len(), mapped);
    }
}
