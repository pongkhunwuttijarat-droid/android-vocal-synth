//! `audiocompare` — compare two wav files and report similarity.
//!
//! Golden-test workhorse: loads both files via `voicebank`, computes
//! per-file stats and difference metrics, and issues a PASS/FAIL verdict
//! against relative tolerances.
//!
//! Exit codes: 0 = PASS, 1 = FAIL, 2 = error (bad args, unreadable file).

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use voicebank::read_wav;

use audiocompare::{compare, Tolerances};

/// Compare two wav files: stats, difference metrics, PASS/FAIL verdict.
#[derive(Debug, Parser)]
#[command(name = "audiocompare", version, about)]
struct Args {
    /// Path to the file under test.
    #[arg(long)]
    actual: PathBuf,

    /// Path to the reference/golden file.
    #[arg(long)]
    reference: PathBuf,

    /// Max allowed RMS difference as a fraction of reference RMS (0.01 = 1%).
    #[arg(long, default_value_t = 0.01)]
    rms_tol: f64,

    /// Max allowed duration difference as a fraction of reference duration.
    #[arg(long, default_value_t = 0.05)]
    dur_tol: f64,

    /// Emit machine-readable JSON on stdout instead of the human report.
    #[arg(long)]
    json: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();
    match run(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(message) => {
            eprintln!("audiocompare: error: {message}");
            ExitCode::from(2)
        }
    }
}

fn run(args: &Args) -> Result<bool, String> {
    let actual = read_wav(&args.actual)
        .map_err(|e| format!("cannot read '{}': {e}", args.actual.display()))?;
    let reference = read_wav(&args.reference)
        .map_err(|e| format!("cannot read '{}': {e}", args.reference.display()))?;

    if actual.sample_rate != reference.sample_rate {
        eprintln!(
            "audiocompare: warning: sample rates differ ({} Hz vs {} Hz); \
             stats use each file's native rate",
            actual.sample_rate, reference.sample_rate
        );
    }

    let tolerances = Tolerances {
        rms: args.rms_tol,
        duration: args.dur_tol,
    };
    let actual_path = args.actual.to_string_lossy();
    let reference_path = args.reference.to_string_lossy();
    let report = compare(
        &actual_path,
        &reference_path,
        &actual,
        &reference,
        &tolerances,
    );

    if args.json {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_human());
    }
    Ok(report.result.pass)
}
