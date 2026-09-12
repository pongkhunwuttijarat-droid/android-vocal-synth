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

    // The production Universal path may emit any valid bank alias.  Keep
    // every alias by default so the student's vocab cannot miss a valid
    // project symbol; --max-singles remains an explicit smoke-test knob.
    let mut max_singles: Option<usize> = None;
    let mut chain = 20usize;
    let mut chain_max = 12usize;
    let mut tones = vec![60i32];
    let mut durs = vec![500.0f64];
    let mut dump_wav: Option<PathBuf> = None;
    let mut only_aliases: Option<Vec<String>> = None;
    let mut project_path: Option<PathBuf> = None;
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
            "--only-aliases" => {
                i += 1;
                if let Some(raw) = args.get(i) {
                    only_aliases = Some(
                        raw.split(',')
                            .map(|v| v.trim().to_string())
                            .filter(|v| !v.is_empty())
                            .collect(),
                    );
                }
            }
            "--project" => {
                i += 1;
                project_path = args.get(i).map(PathBuf::from);
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
        only_aliases.as_deref(),
        project_path.as_deref(),
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
    only_aliases: Option<&[String]>,
    project_path: Option<&Path>,
) -> Result<(usize, usize, usize), String> {
    use std::io::Write;
    let vb = load_voicebank(bank_dir).map_err(|e| format!("load bank: {e}"))?;
    // Real-project mode: extract phrases from a composed .ustx project.
    if let Some(proj) = project_path {
        let dump_dir = std::env::temp_dir().join("train-prep-dump");
        let pairs = train_prep::project_extract::extract_project(
            // Training must use the same Universal phonemizer as production,
            // regardless of the track's legacy phonemizer label.
            proj,
            so,
            mel,
            vocoder,
            &vb,
            Some(synth_cli::pipeline::PhonemizerKind::Universal),
            &dump_dir,
            dump_wav,
        )?;
        let entries: Vec<train_prep::TrainEntry> =
            pairs.iter().map(|(e, _)| e.clone()).collect();
        let (passed_entries, stats) = train_prep::filter::apply(&entries);
        eprintln!(
            "[filter] total={} passed={} too_short={} unvoiced={} flat_mel={}",
            stats.total, stats.passed, stats.too_short, stats.unvoiced,
            stats.flat_mel
        );
        let mut file = std::io::BufWriter::new(
            std::fs::File::create(out)
                .map_err(|e| format!("create {}: {e}", out.display()))?,
        );
        let mut k = 0usize;
        for (entry, audio) in pairs.iter() {
            if !train_prep::filter::passes(entry) {
                continue;
            }
            if let Some(w) = dump_wav {
                let wav = wavwriter::write_wav_16(audio, 44100);
                let p = w.join(format!("{k:05}.wav"));
                wavwriter::write_wav_file(&p, &wav)
                    .map_err(|e| format!("write {}: {e}", p.display()))?;
            }
            let line = serde_json::to_string(entry)
                .map_err(|e| format!("json: {e}"))?;
            file.write_all(line.as_bytes())
                .and_then(|_| file.write_all(b"\n"))
                .map_err(|e| format!("write: {e}"))?;
            k += 1;
        }
        file.flush().map_err(|e| format!("flush: {e}"))?;
        train_prep::extract::write_tsbin(
            &out.with_extension("tsbin"),
            &passed_entries,
        )?;
        return Ok((passed_entries.len(), stats.total - passed_entries.len(), 0));
    }
    let specs = phrases_for_bank(&vb, tones, durs, max_singles, chain, chain_max, only_aliases);
    eprintln!(
        "bank {}: {} oto entries -> {} phrase specs",
        bank_dir.display(),
        vb.otos.len(),
        specs.len()
    );

    let dump_dir = std::env::temp_dir().join("train-prep-dump");
    extract_bank(so, mel, vocoder, &vb, &specs, out, &dump_dir, dump_wav)
}
