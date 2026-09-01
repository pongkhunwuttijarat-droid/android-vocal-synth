//! train-prep CLI — Phase 0 POC: build a distillation training set
//! from a classic voicebank using the R2 engine as teacher.
//!
//! Usage:
//!   train-prep <bank_library_dir> <worldline.so> <mel.onnx> <vocoder.onnx>
//!              <out.jsonl> [--max-singles N] [--chain M] [--tone T] [--dur-ms D]

use std::path::{Path, PathBuf};

use synth_cli::engine::WorldlineR2Engine;
use train_prep::extract::extract_bank;
use train_prep::phrase_gen::phrases_for_bank;
use voicebank::load_voicebank;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 6 {
        eprintln!(
            "usage: {} <bank_library_dir> <worldline.so> <mel.onnx> <vocoder.onnx> \
             <out.jsonl> [--max-singles N] [--chain M] [--tone T] [--dur-ms D]",
            args[0]
        );
        std::process::exit(2);
    }
    let bank_dir = PathBuf::from(&args[1]);
    let so = PathBuf::from(&args[2]);
    let mel = PathBuf::from(&args[3]);
    let vocoder = PathBuf::from(&args[4]);
    let out = PathBuf::from(&args[5]);

    let mut max_singles: Option<usize> = Some(300);
    let mut chain = 20usize;
    let mut chain_max = 12usize;
    let mut tones = vec![60i32];
    let mut durs = vec![500.0f64];
    let mut dump_wav: Option<PathBuf> = None;
    let mut i = 6;
    while i < args.len() {
        match args[i].as_str() {
            "--max-singles" => {
                i += 1;
                max_singles = args.get(i).and_then(|v| v.parse().ok());
            }
            "--chain" => {
                i += 1;
                chain = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(chain);
            }
            "--chain-max" => {
                i += 1;
                chain_max = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(chain_max);
            }
            "--tones" => {
                i += 1;
                if let Some(raw) = args.get(i) {
                    tones = raw
                        .split(',')
                        .filter_map(|v| v.trim().parse().ok())
                        .collect();
                }
            }
            "--durs" => {
                i += 1;
                if let Some(raw) = args.get(i) {
                    durs = raw
                        .split(',')
                        .filter_map(|v| v.trim().parse().ok())
                        .collect();
                }
            }
            "--tone" => {
                i += 1;
                tones = vec![args.get(i).and_then(|v| v.parse().ok()).unwrap_or(60)];
            }
            "--dur-ms" => {
                i += 1;
                durs = vec![args.get(i).and_then(|v| v.parse().ok()).unwrap_or(500.0)];
            }
            "--dump-wav" => {
                i += 1;
                dump_wav = args.get(i).map(PathBuf::from);
            }
            other => {
                eprintln!("unknown arg: {other}");
                std::process::exit(2);
            }
        }
        i += 1;
    }

    match run(
        &bank_dir,
        &so,
        &mel,
        &vocoder,
        &out,
        max_singles,
        chain,
        chain_max,
        &tones,
        &durs,
        dump_wav.as_deref(),
    ) {
        Ok((passed, filtered, skipped)) => {
            println!(
                "OK passed={passed} filtered={filtered} skipped={skipped} -> {}",
                out.display()
            );
        }
        Err(e) => {
            eprintln!("ERROR: {e}");
            std::process::exit(1);
        }
    }
}

fn run(
    bank_dir: &PathBuf,
    so: &PathBuf,
    mel: &PathBuf,
    vocoder: &PathBuf,
    out: &PathBuf,
    max_singles: Option<usize>,
    chain: usize,
    chain_max: usize,
    tones: &[i32],
    durs: &[f64],
    dump_wav: Option<&Path>,
) -> Result<(usize, usize, usize), String> {
    let vb = load_voicebank(bank_dir).map_err(|e| format!("load bank: {e}"))?;
    let specs = phrases_for_bank(&vb, tones, durs, max_singles, chain, chain_max);
    eprintln!(
        "bank {}: {} oto entries -> {} phrase specs",
        bank_dir.display(),
        vb.otos.len(),
        specs.len()
    );

    let dump_dir = std::env::temp_dir().join("train-prep-dump");
    extract_bank(so, mel, vocoder, &vb, &specs, out, &dump_dir, dump_wav)
}
