// Empirical test: does an existing Rust FFT crate (rustfft) match Ooura C++ ground truth?
use std::fs;

fn main() {
    let n = 512usize;
    // Same clean integer-bin signal used to generate ground truth.
    let mut sig = vec![0.0f64; n];
    for f in [40i32, 200] {
        for i in 0..n {
            sig[i] += (f as f64 * std::f64::consts::PI / (n as f64) * (2.0 * i as f64)).sin();
        }
    }

    // Load Ooura ground truth (512 bins, DC + positive freqs)
    let gt_text = fs::read_to_string("/tmp/fft_truth.txt").unwrap();
    let mut gt: Vec<f64> = Vec::new();
    for line in gt_text.lines() {
        if !line.trim().is_empty() && !line.starts_with("n=") {
            gt.push(line.trim().parse::<f64>().unwrap());
        }
    }
    println!("Ground-truth bins: {}", gt.len());

    // Use the same compatibility layer as production code. Ground truth is
    // DC followed by positive-bin magnitudes.
    let mut fft = world_rs::fft::RealFft::new(n).expect("valid FFT length");
    let mut packed = vec![0.0f64; n];
    fft.forward_packed(&sig, &mut packed).expect("FFT");

    // Compare against ground truth bit-for-bit (DC + positive bins)
    let mut maxdiff = 0.0f64;
    for i in 0..gt.len() {
        if i == 0 {
            maxdiff = maxdiff.max((gt[i] - packed[0]).abs()); // DC = sum(x)
        } else {
            let mag = (packed[2*i]*packed[2*i] + packed[2*i+1]*packed[2*i+1]).sqrt();
            maxdiff = maxdiff.max((gt[i] - mag).abs());
        }
    }
    println!("\nMax abs diff vs Ooura ground truth: {:.3e}", maxdiff);

    if maxdiff < 1e-9 {
        println!("RESULT: BIT-EXACT MATCH ✓ — transcription risk LOW");
    } else if maxdiff < 1e-6 {
        println!("RESULT: FLOAT-EPSILON CLOSE (acceptable) — transcription risk LOW");
    } else {
        println!("RESULT: DIVERGENCE DETECTED ✗ — needs investigation");
    }

    // Show worst bins
    let mut worst = Vec::new();
    for i in 0..gt.len() {
        let ours = if i == 0 { packed[0] } else {
            (packed[2*i]*packed[2*i] + packed[2*i+1]*packed[2*i+1]).sqrt()
        };
        if (gt[i]-ours).abs() > maxdiff * 0.1 || i == 0 {
            worst.push((i, gt[i], ours));
        }
    }
    println!("\nWorst bins (idx, gt, ours):");
    for w in &worst[..std::cmp::min(5, worst.len())] {
        println!("  bin {}: gt={:.12e} ours={:.12e}", w.0, w.1, w.2);
    }
}
