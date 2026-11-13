use std::path::Path;
use world_rs::cheaptrick::{analyze, CheapTrickOption};
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let frq_path = voicebank::frq_path_for_wav(Path::new(&path));
    let wav = voicebank::read_wav(Path::new(&path)).unwrap();
    let frq = voicebank::read_frq(&frq_path).unwrap();
    let fs = wav.sample_rate;
    let hop = 512usize;
    let frames = (wav.samples.len().saturating_sub(1) / hop) + 1;
    let f0: Vec<f64> = (0..frames)
        .map(|i| {
            let a = ((i as f64 * hop as f64 / frq.hop_size as f64).floor() as usize)
                .min(frq.f0.len() - 1);
            frq.f0[a]
        })
        .collect();
    let t: Vec<f64> = (0..frames)
        .map(|i| i as f64 * hop as f64 / fs as f64)
        .collect();
    let out = analyze(
        &wav.to_mono(),
        fs,
        &t,
        &f0,
        CheapTrickOption {
            q1: -0.15,
            f0_floor: 71.0,
            fft_size: 2048,
        },
    )
    .unwrap();
    for i in 0..out.len() / 1025 {
        let r = &out[i * 1025..(i + 1) * 1025];
        println!("{:.17e} {:.17e} {:.17e} {:.17e}", r[0], r[1], r[2], r[100]);
    }
}
