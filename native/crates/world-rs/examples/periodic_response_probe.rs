use rustfft::{num_complex::Complex, FftPlanner};
use world_rs::synthesis::minimum_phase_spectrum;

fn main() {
    let n = 1024usize;
    let log = vec![0.1f64.ln() / 2.0; n];
    let mut freq = minimum_phase_spectrum(&log);
    let coefficient = 2.0 * std::f64::consts::PI * (0.4 / 44100.0) * 44100.0 / n as f64;
    for k in 0..=n / 2 {
        let z = freq[k];
        let c = (coefficient * k as f64).cos();
        let s = (1.0 - c * c).max(0.0).sqrt();
        freq[k] = Complex {
            re: z.re * c + z.im * s,
            im: z.im * c - z.re * s,
        };
    }
    for k in 1..n / 2 {
        freq[n - k] = freq[k].conj();
    }
    let mut planner = FftPlanner::<f64>::new();
    planner.plan_fft_inverse(n).process(&mut freq);
    freq.rotate_left(n / 2);
    for z in freq.iter().take(8) {
        println!("{:.17}", z.re / n as f64);
    }
}
