use world_rs::{
    synthesis::{synthesize, SynthesisConfig},
    WorldFeatures,
};

fn main() {
    let mut features = WorldFeatures::try_new(4, 513).unwrap();
    features.f0.fill(220.0);
    features.spectral.fill(0.1);
    let ap = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    features.aperiodicity.fill(ap);
    let sp = std::env::args()
        .nth(3)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.1);
    features.spectral.fill(sp);
    let y = synthesize(
        &features,
        SynthesisConfig {
            sample_rate: 44100,
            frame_period_ms: 5.0,
            fft_size: 1024,
            output_length: Some(662),
            ..Default::default()
        },
        &(),
    )
    .unwrap();
    if let Some(path) = std::env::args().nth(1) {
        let raw: Vec<u8> = y.iter().flat_map(|sample| sample.to_le_bytes()).collect();
        std::fs::write(path, raw).unwrap();
        return;
    }
    println!("{}", y.len());
    for sample in y {
        println!("{sample:.17}");
    }
}
