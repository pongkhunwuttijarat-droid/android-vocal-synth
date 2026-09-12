//! Full Teto-English voicebank through pure-Rust world-rs (no C++, no .so).
//!
//! Reads every real `.wav` in the bank, runs world-rs analysis + synthesis on
//! each, and reports an aggregate so we can confirm the whole bank renders.
//! Usage: cargo run -p world-rs --example full_teto_english <bank-dir> [max]

use std::env;
use std::path::PathBuf;

fn maxf(a: f32, b: f32) -> f32 { if a > b { a } else { b } }

fn main() {
    let bank = env::args().nth(1).expect("usage: cargo run -p world-rs --example full_teto_english <bank-dir> [max]");
    let cap: usize = env::args().nth(2).and_then(|s| s.parse::<usize>().ok()).unwrap_or(u32::MAX as usize);

    let mut wavs: Vec<PathBuf> = std::fs::read_dir(&bank)
        .expect("open bank dir")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("wav")))
        .collect();
    wavs.sort();

    let config = world_rs::analysis::AnalysisConfig {
        sample_rate: 44100,
        frame_period_ms: 5.238_322,   // ~1/191 s ≈ 5 ms hop (real bank is real audio)
        fft_size: 2048,
        min_f0_hz: 64.69,             // GetF0FloorForCheapTrick(44100,2048)
        max_f0_hz: 2000.0,
    };

    let mut ok = 0usize;
    let mut silent = 0usize;
    let mut total_samples = 0u64;
    let mut max_rms_seen = 0.0f32;
    for p in wavs.iter().take(cap).map(|p| p.to_str().unwrap_or_default()) {
        let wav = match voicebank::read_wav(std::path::Path::new(&p)) {
            Ok(w) => w,
            Err(e) => { eprintln!("skip {}: {}", p, format_args!("{:?}", e)); continue; }
        };
        let mono = if wav.channels > 1 { wav.to_mono() } else { wav.samples.clone() };
        let features = match world_rs::analysis::analyze_world_audio(&mono, config, &()) {
            Ok(f) => f,
            Err(e) => { eprintln!("analysis fail {} ({:?})", p, e); continue; }
        };
        let synth = world_rs::synthesis::SynthesisConfig { sample_rate: 44100, fft_size: 2048, ..Default::default() };
        let out = match world_rs::synthesis::synthesize(&features, synth, &()) {
            Ok(o) => o,
            Err(e) => { eprintln!("synth fail {}: {:?}", p, e); continue; }
        };
        let rms = (out.iter().map(|x| x * x).sum::<f32>() / out.len().max(1) as f32).sqrt();
        total_samples += out.len() as u64;
        max_rms_seen = maxf(max_rms_seen, rms);
        if rms > 1e-4 { ok += 1 } else { silent += 1 }
    }

    println!("bank wavs rendered: {} (ok={}, silent={}) total_out_samples={} max_rms={:.6}", cap.min(wavs.len()), ok, silent, total_samples, max_rms_seen);
}
