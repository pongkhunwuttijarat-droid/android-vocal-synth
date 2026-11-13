use world_rs::cheaptrick::{analyze, CheapTrickOption};

fn main() {
    let fs = 44100u32;
    let n = 44100usize;
    let f0 = 220.0f64;
    let samples: Vec<f32> = (0..n)
        .map(|i| (2.0 * std::f64::consts::PI * f0 * i as f64 / fs as f64).sin() as f32)
        .collect();
    let positions = [0.25, 0.50, 0.75];
    let f0s = [f0; 3];
    let option = CheapTrickOption {
        q1: -0.15,
        f0_floor: 71.0,
        fft_size: 2048,
    };
    let out = analyze(&samples, fs, &positions, &f0s, option).unwrap();
    for i in 0..3 {
        let row = &out[i * 1025..(i + 1) * 1025];
        println!(
            "{:.17e} {:.17e} {:.17e} {:.17e}",
            row[0], row[1], row[2], row[100]
        );
    }
}
