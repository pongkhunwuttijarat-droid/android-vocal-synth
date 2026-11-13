use std::{fs::File, io::Read};
use world_rs::{
    synthesis::{synthesize, SynthesisConfig},
    WorldFeatures,
};
fn main() {
    let p = std::env::args().nth(1).unwrap();
    let mut b = Vec::new();
    File::open(p).unwrap().read_to_end(&mut b).unwrap();
    let n = i32::from_ne_bytes(b[0..4].try_into().unwrap()) as usize;
    let bins = i32::from_ne_bytes(b[4..8].try_into().unwrap()) as usize;
    let mut o = WorldFeatures::try_new(n, bins).unwrap();
    let mut at = 8;
    for x in &mut o.f0 {
        *x = f64::from_le_bytes(b[at..at + 8].try_into().unwrap());
        at += 8;
    }
    for x in o.spectral.iter_mut().chain(o.aperiodicity.iter_mut()) {
        *x = f64::from_le_bytes(b[at..at + 8].try_into().unwrap());
        at += 8;
    }
    let y = synthesize(
        &o,
        SynthesisConfig {
            sample_rate: 44100,
            frame_period_ms: 512.0 * 1000.0 / 44100.0,
            fft_size: 2048,
            output_length: Some((n - 1) * 512 + 1),
            ..Default::default()
        },
        &(),
    )
    .unwrap();
    let q: Vec<u8> = y.iter().flat_map(|x| x.to_le_bytes()).collect();
    std::fs::write("/tmp/rust-feature-synth.f32", q).unwrap();
}
