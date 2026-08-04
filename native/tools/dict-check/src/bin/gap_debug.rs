//! Debug: dump SynthRequests สำหรับ ml_demo2 — หา gap ระหว่าง phoneme
use synth_cli::pipeline::{self, PhonemizerKind};
use worldline_plugin::convert::build_requests;
use voicebank::load_voicebank as load_vb;
use domain::UPart;

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
    let inputs = pipeline::build_phrase_inputs(&project, track, part, &vb, PhonemizerKind::English)
        .expect("build phrase inputs");
    for pi in &inputs {
        let reqs = build_requests(&pi.input).expect("convert");
        println!("phrase @{}ms — {} requests:", pi.input.phrase.position_ms as i64, reqs.len());
        for (i, r) in reqs.iter().enumerate() {
            let end = r.pos_ms + r.length_ms;
            if (r.pos_ms > 5300.0 && r.pos_ms < 5700.0) || (r.pos_ms > 7100.0 && r.pos_ms < 7450.0) || (r.pos_ms > 8100.0 && r.pos_ms < 8500.0) { println!("    [{}] {} pos={:.0} len={:.0} end={:.0} skip={:.0}", i, r.phoneme, r.pos_ms, r.length_ms, end, r.skip_ms); }
            println!(
                "  {:2} {:<8} pos={:7.1} len={:6.1} end={:7.1} skip={:5.1}",
                i, r.phoneme, r.pos_ms, r.length_ms, end, r.skip_ms
            );
        }
        // gap check
        for i in 1..reqs.len() {
            let prev_end = reqs[i-1].pos_ms + reqs[i-1].length_ms;
            let gap = reqs[i].pos_ms - prev_end;
            if gap > 1.0 {
                println!("  >>> GAP {}ms ก่อน phoneme {} (end {:.1} → start {:.1})",
                    gap, reqs[i].phoneme, prev_end, reqs[i].pos_ms);
            }
        }
    }
}
