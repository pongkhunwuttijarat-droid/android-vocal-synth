use world_rs::synthesis::minimum_phase_spectrum;

fn main() {
    let n = 1024;
    let mut log = vec![0.0; n];
    for i in 0..=n / 2 {
        log[i] = (0.1 + 0.0002 * i as f64).ln() / 2.0;
    }
    for i in n / 2 + 1..n {
        log[i] = log[n - i];
    }
    let s = minimum_phase_spectrum(&log);
    for z in s.iter().take(8) {
        println!("{:.17} {:.17}", z.re, z.im);
    }
}
