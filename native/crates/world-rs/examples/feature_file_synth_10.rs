use std::{convert::TryInto, fs::File, io::{Read, Write}};
use world_rs::{synthesis::{synthesize, SynthesisConfig}, WorldFeatures};

fn main() {
    let path = std::env::args().nth(1).expect("feature file");
    let mut bytes = Vec::new();
    File::open(path).unwrap().read_to_end(&mut bytes).unwrap();
    let n = i32::from_ne_bytes(bytes[0..4].try_into().unwrap()) as usize;
    let bins = i32::from_ne_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let mut at = 8;
    let mut f = vec![0.0; n];
    for x in &mut f { *x = f64::from_le_bytes(bytes[at..at + 8].try_into().unwrap()); at += 8; }
    let mut sp = vec![0.0; n * bins];
    for x in &mut sp { *x = f64::from_le_bytes(bytes[at..at + 8].try_into().unwrap()); at += 8; }
    let mut ap = vec![0.0; n * bins];
    for x in &mut ap { *x = f64::from_le_bytes(bytes[at..at + 8].try_into().unwrap()); at += 8; }
    let features = WorldFeatures { frame_count: n, bin_count: bins, f0: f, spectral: sp, aperiodicity: ap };
    let out = synthesize(&features, SynthesisConfig { sample_rate: 44100, frame_period_ms: 10.0, fft_size: 2048, output_length: Some((n - 1) * 441 + 1), ..Default::default() }, &()).unwrap();
    let mut file = File::create("/tmp/rust-final-features.f32").unwrap();
    for x in out { file.write_all(&x.to_le_bytes()).unwrap(); }
}
