use world_rs::d4c::{analyze, D4cOption};
fn main() {
    let fs = 44100u32; let n = 44100usize; let f0 = 220.0;
    let x: Vec<f32> = (0..n).map(|i| (2.0 * std::f64::consts::PI * f0 * i as f64 / fs as f64).sin() as f32).collect();
    let out = analyze(&x, fs, &[0.25, 0.5, 0.75], &[f0; 3], 2048, D4cOption { threshold: 0.0, floor_f0: 47.0 }, &()).unwrap();
    for i in 0..3 { let r = &out[i * 1025..(i + 1) * 1025]; println!("{:.17e} {:.17e} {:.17e} {:.17e}", r[0], r[1], r[2], r[100]); }
}
