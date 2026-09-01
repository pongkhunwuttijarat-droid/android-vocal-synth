//! moe-train CLI — POC baseline training.
//!
//! Usage:
//!   moe-train <train-set.jsonl> <out-model.burn> [--epochs N] [--batch M]
//!            [--lr X] [--eval-frac F] [--backend wgpu|ndarray]
//!   moe-train --moe <train-set.jsonl> <out-prefix> [--epochs N] [--expert-epochs M]
//!            [--batch B] [--lr X] [--eval-frac F] [--backend wgpu|ndarray]

// burn-wgpu's wgpu dep chain nests associated types past the default
// recursion limit (E0275) — the documented fix (burn-wgpu README).
#![recursion_limit = "256"]

use moe_train::baseline::{save_model, train_context, train_moe};
use moe_train::data::load_train_set;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!(
            "usage: {} <train-set.jsonl> <out-model.burn> [opts] | {} --moe <train-set> <prefix> [opts]",
            args[0], args[0]
        );
        std::process::exit(2);
    }

    if args[1] == "--moe" {
        let code = run_moe_cli(&args);
        std::process::exit(code);
    }

    if args[1] == "--render" {
        let code = run_render_cli(&args);
        std::process::exit(code);
    }

    if args[1] == "--render-all" {
        let code = run_render_all_cli(&args);
        std::process::exit(code);
    }

    if args.len() < 3 {
        eprintln!(
            "usage: {} <train-set.jsonl> <out-model.burn> [--epochs N] [--batch M] \
             [--lr X] [--eval-frac F] [--backend wgpu|ndarray]",
            args[0]
        );
        std::process::exit(2);
    }
    let data_path = &args[1];
    let out_path = &args[2];

    let mut epochs = 8usize;
    let mut batch = 1024usize;
    let mut lr = 1e-3f64;
    let mut eval_frac = 0.1f64;
    let mut backend = "wgpu".to_string();
    let mut resume = false;
    let mut hidden = moe_train::baseline::DEFAULT_HIDDEN;
    let mut depth = moe_train::baseline::DEFAULT_DEPTH;
    let mut gpu_cap: Option<usize> = None;
    let mut i = 3;
    while i < args.len() {
        match args[i].as_str() {
            "--epochs" => {
                i += 1;
                epochs = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(epochs);
            }
            "--batch" => {
                i += 1;
                batch = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(batch);
            }
            "--lr" => {
                i += 1;
                lr = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(lr);
            }
            "--eval-frac" => {
                i += 1;
                eval_frac = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(eval_frac);
            }
            "--backend" => {
                i += 1;
                backend = args.get(i).cloned().unwrap_or(backend);
            }
            "--resume" => resume = true,
            other => {
                eprintln!("unknown arg: {other}");
                std::process::exit(2);
            }
        }
        i += 1;
    }

    match run(data_path, out_path, epochs, batch, lr, eval_frac, hidden, depth, gpu_cap, &backend, resume) {
        Ok((train, val)) => {
            println!("OK backend={backend} train_loss={train:.6} val_loss={val:.6} -> {out_path}");
        }
        Err(e) => {
            eprintln!("ERROR: {e}");
            std::process::exit(1);
        }
    }
}

/// `moe-train --render <train-set> <model-prefix> <vocoder.onnx> <phrase-idx>
/// <out.wav> [--backend wgpu|ndarray]` — MoE → mel → vocoder → wav.
fn run_render_cli(args: &[String]) -> i32 {
    if args.len() < 7 {
        eprintln!(
            "usage: {} --render <train-set.jsonl> <model-prefix> <vocoder.onnx> \
             <phrase-idx> <out.wav> [--backend wgpu|ndarray]",
            args[0]
        );
        return 2;
    }
    let data_path = &args[2];
    let prefix = &args[3];
    let vocoder = &args[4];
    let phrase_idx: usize = match args[5].parse() {
        Ok(v) => v,
        Err(_) => {
            eprintln!("phrase-idx must be a number: {}", args[5]);
            return 2;
        }
    };
    let out_wav = &args[6];
    let mut backend = "wgpu".to_string();
    let mut gain = 1.0f32;
    let mut hidden = moe_train::baseline::DEFAULT_HIDDEN;
    let mut depth = moe_train::baseline::DEFAULT_DEPTH;
    let mut gpu_cap: Option<usize> = None;
    let mut i = 7;
    while i < args.len() {
        match args[i].as_str() {
            "--backend" => {
                i += 1;
                backend = args.get(i).cloned().unwrap_or(backend);
            }
            "--gain" => {
                i += 1;
                gain = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(gain);
            }
            "--hidden" => {
                i += 1;
                hidden = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(hidden);
            }
            "--depth" => {
                i += 1;
                depth = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(depth);
            }
            "--gpu-cap" => {
                i += 1;
                gpu_cap = args.get(i).and_then(|v| v.parse().ok());
            }
            other => {
                eprintln!("unknown arg: {other}");
                return 2;
            }
        }
        i += 1;
    }

    let set = match load_train_set(data_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ERROR: {e}");
            return 1;
        }
    };
    if phrase_idx >= set.phrases.len() {
        eprintln!(
            "ERROR: phrase {phrase_idx} out of range (0..{}). Available: {:?}",
            set.phrases.len(),
            set.phrases
                .iter()
                .take(5)
                .map(|p| p.aliases.join(" "))
                .collect::<Vec<_>>()
        );
        return 1;
    }
    eprintln!(
        "phrase {phrase_idx}: aliases = {:?}",
        set.phrases[phrase_idx].aliases
    );

    let result = match backend.as_str() {
        "wgpu" => {
            type Backend = burn::backend::Wgpu;
            let device = burn::backend::wgpu::WgpuDevice::IntegratedGpu(0);
            render_on::<Backend>(&set, prefix, vocoder, phrase_idx, gain, hidden, depth, &device)
        }
        "wgpu-f16" => {
            type Backend = burn::backend::Wgpu<burn::tensor::f16, i32>;
            let device = burn::backend::wgpu::WgpuDevice::IntegratedGpu(0);
            render_on::<Backend>(&set, prefix, vocoder, phrase_idx, gain, hidden, depth, &device)
        }
        "ndarray" => {
            type Backend = burn::backend::NdArray;
            let device = Default::default();
            render_on::<Backend>(&set, prefix, vocoder, phrase_idx, gain, hidden, depth, &device)
        }
        other => {
            eprintln!("ERROR: unknown backend: {other}");
            return 1;
        }
    };

    match result {
        Ok(bytes) => match std::fs::write(out_wav, &bytes) {
            Ok(()) => {
                println!("OK rendered -> {out_wav} ({} bytes)", bytes.len());
                0
            }
            Err(e) => {
                eprintln!("ERROR: write {out_wav}: {e}");
                1
            }
        },
        Err(e) => {
            eprintln!("ERROR: {e}");
            1
        }
    }
}

fn render_on<B: burn::tensor::backend::Backend>(
    set: &moe_train::data::TrainSet,
    prefix: &str,
    vocoder: &str,
    phrase_idx: usize,
    gain: f32,
    hidden: usize,
    depth: usize,
    device: &B::Device,
) -> Result<Vec<u8>, String> {
    let moe = moe_train::render::load_moe::<B>(set, prefix, hidden, depth, device)?;
    moe_train::render::render_phrase_wav::<B>(&moe, set, phrase_idx, vocoder, gain, device)
}

/// `moe-train --render-all <train-set> <prefix> <vocoder.onnx> <out-dir>
/// [--gain X] [--max N] [--backend ...]` — render every phrase in ONE
/// process (models loaded once) for batch evaluation vs the teacher.
fn run_render_all_cli(args: &[String]) -> i32 {
    if args.len() < 6 {
        eprintln!(
            "usage: {} --render-all <train-set.jsonl> <model-prefix> <vocoder.onnx> <out-dir> \
             [--gain X] [--max N] [--backend wgpu|ndarray]",
            args[0]
        );
        return 2;
    }
    let data_path = &args[2];
    let prefix = &args[3];
    let vocoder = &args[4];
    let out_dir = std::path::PathBuf::from(&args[5]);
    let mut gain = 1.0f32;
    let mut max: Option<usize> = None;
    let mut backend = "wgpu".to_string();
    let mut workers = 8usize;
    let mut hidden = moe_train::baseline::DEFAULT_HIDDEN;
    let mut depth = moe_train::baseline::DEFAULT_DEPTH;
    let mut gpu_cap: Option<usize> = None;
    let mut i = 6;
    while i < args.len() {
        match args[i].as_str() {
            "--gain" => {
                i += 1;
                gain = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(gain);
            }
            "--max" => {
                i += 1;
                max = args.get(i).and_then(|v| v.parse().ok());
            }
            "--backend" => {
                i += 1;
                backend = args.get(i).cloned().unwrap_or(backend);
            }
            "--workers" => {
                i += 1;
                workers = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(workers);
            }
            "--hidden" => {
                i += 1;
                hidden = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(hidden);
            }
            "--depth" => {
                i += 1;
                depth = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(depth);
            }
            "--gpu-cap" => {
                i += 1;
                gpu_cap = args.get(i).and_then(|v| v.parse().ok());
            }
            other => {
                eprintln!("unknown arg: {other}");
                return 2;
            }
        }
        i += 1;
    }

    let set = match load_train_set(data_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ERROR: {e}");
            return 1;
        }
    };
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("mkdir: {e}")).unwrap();
    let n = max.unwrap_or(set.phrases.len()).min(set.phrases.len());

    let result = match backend.as_str() {
        "wgpu" => {
            type Backend = burn::backend::Wgpu;
            let device = burn::backend::wgpu::WgpuDevice::IntegratedGpu(0);
            render_all_on::<Backend>(&set, prefix, vocoder, &out_dir, n, gain, workers, hidden, depth, &device)
        }
        "wgpu-f16" => {
            type Backend = burn::backend::Wgpu<burn::tensor::f16, i32>;
            let device = burn::backend::wgpu::WgpuDevice::IntegratedGpu(0);
            render_all_on::<Backend>(&set, prefix, vocoder, &out_dir, n, gain, workers, hidden, depth, &device)
        }
        "ndarray" => {
            type Backend = burn::backend::NdArray;
            let device = Default::default();
            render_all_on::<Backend>(&set, prefix, vocoder, &out_dir, n, gain, workers, hidden, depth, &device)
        }
        other => {
            eprintln!("ERROR: unknown backend: {other}");
            return 1;
        }
    };

    match result {
        Ok(done) => {
            println!("OK rendered {done}/{n} phrases -> {}", out_dir.display());
            0
        }
        Err(e) => {
            eprintln!("ERROR: {e}");
            1
        }
    }
}

fn render_all_on<B: burn::tensor::backend::Backend>(
    set: &moe_train::data::TrainSet,
    prefix: &str,
    vocoder_path: &str,
    out_dir: &std::path::Path,
    n: usize,
    gain: f32,
    workers: usize,
    hidden: usize,
    depth: usize,
    device: &B::Device,
) -> Result<usize, String> {
    use moe_train::render::{load_moe, open_vocoder_threads, render_phrase_wav_with_session};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    let moe = load_moe::<B>(set, prefix, hidden, depth, device)?;
    // PARALLEL: each worker owns its ort vocoder session (the vocoder is
    // the render bottleneck — ort runs it on CPU — so N sessions run the
    // mel+vocoder chain concurrently). The burn model is shared read-only.
    // Each session caps intra-op threads at 2 (ort's default = all cores
    // PER SESSION → 8 sessions × 16 cores = massive oversubscription/lag).
    let workers = workers.min(8);
    eprintln!("[render-all] {n} phrases, {workers} workers (2 threads/session)");
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let errors: Mutex<Vec<String>> = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..workers {
            // Borrows (not move): thread::scope guarantees the threads
            // finish before these locals drop.
            s.spawn(|| {
                let mut vocoder = match open_vocoder_threads(vocoder_path, 2) {
                    Ok(v) => v,
                    Err(e) => {
                        errors.lock().unwrap().push(e);
                        return;
                    }
                };
                loop {
                    let pid = next.fetch_add(1, Ordering::Relaxed);
                    if pid >= n {
                        break;
                    }
                    match render_phrase_wav_with_session::<B>(&moe, set, pid, &mut vocoder, gain, device) {
                        Ok(bytes) => {
                            if let Err(e) = std::fs::write(out_dir.join(format!("{pid:05}.wav")), &bytes) {
                                errors.lock().unwrap().push(format!("write {pid}: {e}"));
                            }
                        }
                        Err(e) => {
                            errors.lock().unwrap().push(format!("phrase {pid}: {e}"));
                        }
                    }
                    let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                    if d % 200 == 0 {
                        eprintln!("[render-all] {d}/{n}");
                    }
                }
            });
        }
    });
    let errs = errors.lock().unwrap();
    if !errs.is_empty() {
        return Err(format!("{} render errors (first: {})", errs.len(), errs[0]));
    }
    Ok(done.load(Ordering::Relaxed))
}
fn run_moe_cli(args: &[String]) -> i32 {
    if args.len() < 4 {
        eprintln!("usage: {} --moe <train-set.jsonl> <out-prefix> [opts]", args[0]);
        return 2;
    }
    let data_path = &args[2];
    let prefix = &args[3];
    let mut epochs = 15usize;
    let mut expert_epochs = 10usize;
    let mut batch = 1024usize;
    let mut lr = 1e-3f64;
    let mut eval_frac = 0.1f64;
    let mut backend = "wgpu".to_string();
    let mut resume = false;
    let mut hidden = moe_train::baseline::DEFAULT_HIDDEN;
    let mut depth = moe_train::baseline::DEFAULT_DEPTH;
    let mut gpu_cap: Option<usize> = None;
    let mut i = 4;
    while i < args.len() {
        match args[i].as_str() {
            "--epochs" => {
                i += 1;
                epochs = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(epochs);
            }
            "--expert-epochs" => {
                i += 1;
                expert_epochs = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(expert_epochs);
            }
            "--batch" => {
                i += 1;
                batch = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(batch);
            }
            "--lr" => {
                i += 1;
                lr = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(lr);
            }
            "--eval-frac" => {
                i += 1;
                eval_frac = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(eval_frac);
            }
            "--backend" => {
                i += 1;
                backend = args.get(i).cloned().unwrap_or(backend);
            }
            "--hidden" => {
                i += 1;
                hidden = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(hidden);
            }
            "--depth" => {
                i += 1;
                depth = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(depth);
            }
            "--gpu-cap" => {
                i += 1;
                gpu_cap = args.get(i).and_then(|v| v.parse().ok());
            }
            "--resume" => resume = true,
            other => {
                eprintln!("unknown arg: {other}");
                return 2;
            }
        }
        i += 1;
    }

    let set = match load_train_set(data_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ERROR: {e}");
            return 1;
        }
    };
    eprintln!(
        "train-set: {} samples, {} phonemes, {} phrases",
        set.samples.len(),
        set.vocab.len(),
        set.phrases.len()
    );

    let result = match backend.as_str() {
        "wgpu" => {
            type Backend = burn::backend::Wgpu;
            type AB = burn::backend::Autodiff<Backend>;
            let device = burn::backend::wgpu::WgpuDevice::IntegratedGpu(0);
            train_moe::<AB>(
                &set,
                epochs,
                batch,
                lr,
                eval_frac,
                expert_epochs,
                prefix,
                hidden,
                depth,
                gpu_cap,
                &device,
                resume,
                &NEVER_STOP,
                &mut |e, t, v| println!("[general] epoch {e}: train={t:.4} val={v:.4}"),
            )
        }
        "wgpu-f16" => {
            type Backend = burn::backend::Wgpu<burn::tensor::f16, i32>;
            type AB = burn::backend::Autodiff<Backend>;
            let device = burn::backend::wgpu::WgpuDevice::IntegratedGpu(0);
            train_moe::<AB>(
                &set,
                epochs,
                batch,
                lr,
                eval_frac,
                expert_epochs,
                prefix,
                hidden,
                depth,
                gpu_cap,
                &device,
                resume,
                &NEVER_STOP,
                &mut |e, t, v| println!("[general] epoch {e}: train={t:.4} val={v:.4}"),
            )
        }
        "ndarray" => {
            type Backend = burn::backend::NdArray;
            type AB = burn::backend::Autodiff<Backend>;
            let device = Default::default();
            train_moe::<AB>(
                &set,
                epochs,
                batch,
                lr,
                eval_frac,
                expert_epochs,
                prefix,
                hidden,
                depth,
                gpu_cap,
                &device,
                resume,
                &NEVER_STOP,
                &mut |e, t, v| println!("[general] epoch {e}: train={t:.4} val={v:.4}"),
            )
        }
        other => {
            eprintln!("ERROR: unknown backend: {other}");
            return 1;
        }
    };

    match result {
        Ok(report) => {
            println!("OK moe trained -> {prefix}-general.mpk + group experts");
            for (g, before, after) in &report {
                println!("  group {g}: residual {before:.4} -> {after:.4}");
            }
            // Vocab sidecar: the render-time MoeEngine needs the phoneme
            // id mapping the general/experts were trained on (the .mpk
            // carries only weights, no vocab). Write the id-sorted vocab
            // next to the checkpoints so the engine can build id tensors.
            if let Err(e) = write_vocab_sidecar(prefix, &set) {
                eprintln!("WARN: vocab sidecar: {e}");
            }
            0
        }
        Err(e) => {
            eprintln!("ERROR: {e}");
            1
        }
    }
}

/// Write `{prefix}-phonemes.txt` — one phoneme per line (index order =
/// `TrainSet.vocab`), so a render-time engine can map a phoneme symbol
/// to the embedding id the checkpoints were trained with.
fn write_vocab_sidecar(prefix: &str, set: &moe_train::data::TrainSet) -> Result<(), String> {
    let path = format!("{prefix}-phonemes.txt");
    let mut body = String::new();
    for p in &set.vocab {
        body.push_str(p);
        body.push('\n');
    }
    std::fs::write(&path, body).map_err(|e| format!("write {path}: {e}"))
}

fn run(
    data_path: &str,
    out_path: &str,
    epochs: usize,
    batch: usize,
    lr: f64,
    eval_frac: f64,
    hidden: usize,
    depth: usize,
    gpu_cap: Option<usize>,
    backend: &str,
    resume: bool,
) -> Result<(f64, f64), String> {
    let set = load_train_set(data_path)?;
    eprintln!(
        "train-set: {} samples, {} phonemes in vocab",
        set.samples.len(),
        set.vocab.len()
    );

    match backend {
        "wgpu" => {
            // Vulkan (Radeon 760M iGPU / Android Adreno-Mali) — the
            // portable path; same code as the ndarray fallback.
            type Backend = burn::backend::Wgpu;
            type AB = burn::backend::Autodiff<Backend>;
            let device = burn::backend::wgpu::WgpuDevice::IntegratedGpu(0);
            let init = if resume {
                moe_train::baseline::try_load_model::<AB>(out_path, hidden, depth, &device)?
            } else {
                None
            };
            let (model, train_loss, val_loss, _stopped) =
                train_context::<AB>(&set, epochs, batch, lr, eval_frac, hidden, depth, gpu_cap, &device, init, &mut noop(), &NEVER_STOP);
            save_model(&model, out_path)?;
            Ok((train_loss, val_loss))
        }
        "wgpu-f16" => {
            // Half precision on the same Vulkan path — RDNA3 does 2× the
            // FP16 compute vs FP32, and half the memory traffic.
            // (burn 0.21 + cubecl 0.10 — f16 finally supported on wgpu.)
            type Backend = burn::backend::Wgpu<burn::tensor::f16, i32>;
            type AB = burn::backend::Autodiff<Backend>;
            let device = burn::backend::wgpu::WgpuDevice::IntegratedGpu(0);
            let init = if resume {
                moe_train::baseline::try_load_model::<AB>(out_path, hidden, depth, &device)?
            } else {
                None
            };
            let (model, train_loss, val_loss, _stopped) =
                train_context::<AB>(&set, epochs, batch, lr, eval_frac, hidden, depth, gpu_cap, &device, init, &mut noop(), &NEVER_STOP);
            save_model(&model, out_path)?;
            Ok((train_loss, val_loss))
        }
        "ndarray" => {
            type Backend = burn::backend::NdArray;
            type AB = burn::backend::Autodiff<Backend>;
            let device = Default::default();
            let init = if resume {
                moe_train::baseline::try_load_model::<AB>(out_path, hidden, depth, &device)?
            } else {
                None
            };
            let (model, train_loss, val_loss, _stopped) =
                train_context::<AB>(&set, epochs, batch, lr, eval_frac, hidden, depth, gpu_cap, &device, init, &mut noop(), &NEVER_STOP);
            save_model(&model, out_path)?;
            Ok((train_loss, val_loss))
        }
        other => Err(format!("unknown backend: {other} (use wgpu|ndarray)")),
    }
}

/// CLI convenience: no progress reporting, never stop.
fn noop() -> impl FnMut(usize, f64, f64) {
    |_: usize, _: f64, _: f64| {}
}
static NEVER_STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
