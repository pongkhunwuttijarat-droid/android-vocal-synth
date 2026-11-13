//! Real voicebank end-to-end through pure-Rust world-rs (no C++, no .so).
//! Reads one real voicebank wav, runs analyze_world_audio + synthesize, and
//! reports RMS/peak so the caller can confirm it is non-silent output.
fn maxf(a: f32, b: f32) -> f32 {
    if a > b {
        a
    } else {
        b
    }
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: cargo run -p world-rs --example real_voicebank <wav>");
    let wav = voicebank::read_wav(std::path::Path::new(&path)).expect("read wav");
    let mono = if wav.channels > 1 {
        wav.to_mono()
    } else {
        wav.samples.clone()
    };

    let config = world_rs::analysis::AnalysisConfig {
        sample_rate: 44100,
        frame_period_ms: 5.238_322, // ~1/191 s ≈ 5 ms hop (real bank is real audio)
        fft_size: 2048,
        min_f0_hz: 64.69, // GetF0FloorForCheapTrick(44100,2048)
        max_f0_hz: 2000.0,
    };

    let features = world_rs::analysis::analyze_world_audio(&mono, config, &()).expect("analysis");
    println!(
        "frames={} bins={}",
        features.frame_count, features.bin_count
    );

    let synth = world_rs::synthesis::SynthesisConfig {
        sample_rate: 44100,
        fft_size: 2048,
        ..Default::default()
    };
    let out = world_rs::synthesis::synthesize(&features, synth, &()).expect("synthesis");

    let rms = (out.iter().map(|x| x * x).sum::<f32>() / out.len().max(1) as f32).sqrt();
    let peak: f32 = out.iter().fold(0.0, |acc, x| maxf(acc, x.abs()));
    println!("out={} rms={:.6} peak={:.6}", out.len(), rms, peak);
    assert!(
        rms > 1e-4 && peak > 1e-3,
        "real voicebank render must be non-silent"
    );
}
