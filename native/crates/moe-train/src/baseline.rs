//! Baseline acoustic with context (POC — the MoE design depends on
//! context helping coarticulation): per-frame regression where each
//! frame sees its phoneme's ±2 neighbors (window 5) + f0 + in-phoneme
//! position. Trained on the R2 teacher distillation set with burn 0.17.

use burn::module::{AutodiffModule, Module};
use burn::nn::{Dropout, DropoutConfig, Embedding, EmbeddingConfig, Linear, LinearConfig};
use burn::optim::decay::WeightDecayConfig;
use burn::optim::{AdamConfig, GradientsParams, Optimizer};
use burn::record::{FullPrecisionSettings, NamedMpkFileRecorder};
use burn::tensor::backend::{AutodiffBackend, Backend};
use burn::tensor::{ElementConversion, Int, Tensor, TensorData};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::data::{FrameSample, TrainSet};
use crate::phoneme_groups::N_GROUPS;

/// Context window (phonemes): current ±2.
pub const WINDOW: usize = 5;
pub(crate) const HALF: usize = 2;

/// Fixed embedding table size: phoneme vocab grows as banks are added,
/// but checkpoints must stay loadable (resume/incremental training) —
/// so the table is always this big and ids are clamped into it.
pub const EMBED_SLOTS: usize = 512;

/// General-expert default size (legacy runs); the 140M-class general
/// uses `--hidden 7680 --depth 2`.
pub const DEFAULT_HIDDEN: usize = 1024;
pub const DEFAULT_DEPTH: usize = 1;
/// Per-group residual experts stay small (they learn the residual only).
pub const EXPERT_HIDDEN: usize = 1024;
pub const EXPERT_DEPTH: usize = 1;

/// Context-aware frame-wise acoustic: 5 phoneme embeddings + f0 +
/// position → MLP → 128 mel bins. Width/depth are configurable — the
/// 140M-class general expert is `hidden≈7680, depth=2`; the per-group
/// residual experts stay small (hidden 1024, depth 1).
#[derive(Module, Debug)]
pub struct ContextAcoustic<B: Backend> {
    emb: Embedding<B>,
    fc_in: Linear<B>,
    hiddens: Vec<Linear<B>>,
    out: Linear<B>,
    dropout: Dropout,
}

impl<B: Backend> ContextAcoustic<B> {
    /// `_vocab_size` is accepted for call-site compatibility, but the
    /// embedding is always `EMBED_SLOTS` — fixed so checkpoints stay
    /// loadable when the phoneme vocab grows (resume/incremental).
    pub fn new(_vocab_size: usize, hidden: usize, depth: usize, device: &B::Device) -> Self {
        let emb = EmbeddingConfig::new(EMBED_SLOTS, 64).init(device);
        // 5*64 emb + 1 f0 + 1 position = 322 inputs.
        let fc_in = LinearConfig::new(WINDOW * 64 + 2, hidden).init(device);
        let mut hiddens = Vec::with_capacity(depth);
        for _ in 0..depth {
            hiddens.push(LinearConfig::new(hidden, hidden).init(device));
        }
        let out = LinearConfig::new(hidden, 128).init(device);
        let dropout = DropoutConfig::new(0.2).init();
        Self {
            emb,
            fc_in,
            hiddens,
            out,
            dropout,
        }
    }

    /// `ids`: [B, WINDOW] int phoneme ids; `feats`: [B,2] (f0, pos);
    /// returns [B,128] mel.
    pub fn forward(&self, ids: Tensor<B, 2, Int>, feats: Tensor<B, 2>) -> Tensor<B, 2> {
        let x = self.emb.forward(ids).flatten(1, 2); // [B, W*64]
        let x = Tensor::cat(vec![x, feats], 1); // [B, W*64+2]
        let mut x = self.dropout.forward(burn::tensor::activation::relu(self.fc_in.forward(x)));
        for h in &self.hiddens {
            x = self.dropout.forward(burn::tensor::activation::relu(h.forward(x)));
        }
        self.out.forward(x)
    }
}

/// Load a checkpoint if it exists on disk (resume support).
pub fn try_load_model<B: Backend>(
    path: &str,
    hidden: usize,
    depth: usize,
    device: &B::Device,
) -> Result<Option<ContextAcoustic<B>>, String> {
    let recorder = NamedMpkFileRecorder::<FullPrecisionSettings>::new();
    let mut probe = std::path::PathBuf::from(path);
    probe.set_extension("mpk");
    if !probe.exists() {
        return Ok(None);
    }
    let dummy = ContextAcoustic::<B>::new(0, DEFAULT_HIDDEN, DEFAULT_DEPTH, device);
    match dummy.load_file(path, &recorder, device) {
        Ok(m) => Ok(Some(m)),
        Err(e) => Err(format!("resume load {path}: {e}")),
    }
}


/// Read the GPU busy % from amdgpu sysfs (first card that has the file).
fn gpu_busy_percent() -> Option<f64> {
    if let Ok(rd) = std::fs::read_dir("/sys/class/drm") {
        for e in rd.flatten() {
            let p = e.path().join("device/gpu_busy_percent");
            if let Ok(v) = std::fs::read_to_string(&p) {
                if let Ok(n) = v.trim().parse::<f64>() {
                    return Some(n);
                }
            }
        }
    }
    None
}

/// Pacing helper: duty-cycle sleep. The ratio keeps the TRAINING's
/// contribution at `cap`% of wall time. Note: the sysfs busy readout is
/// the WHOLE GPU (desktop/compositor included — idle already reads
/// ~60% mean with 99% spikes on this machine), so a convergence loop
/// against it stalls training; the duty ratio is the right control.
fn pace_gpu(cap: Option<usize>, batch_secs: f64) {
    if let Some(cap) = cap {
        let duty = batch_secs * (100.0 / cap as f64 - 1.0);
        if duty > 0.0 {
            std::thread::sleep(std::time::Duration::from_secs_f64(duty));
        }
    }
}

/// MSE with chunked reduction (row-mean first, then batch-mean): a
/// single-pass full reduction overflows f16 (131k elements × ~30 avg
/// > 65504 → Inf/NaN). Identical math in f32, safe in f16.
fn mse_loss<B: Backend>(pred: Tensor<B, 2>, target: Tensor<B, 2>) -> Tensor<B, 1> {
    let d = pred - target;
    let sq = d.clone() * d;
    sq.mean_dim(1).mean()
}

#[cfg(test)]
mod f16_smoke {
    use super::*;
    use burn::tensor::Int;

    /// Isolate: does a from_data → forward → mse chain produce NaN on
    /// the f16 wgpu backend (no real data involved)?
    #[test]
    fn f16_forward_is_finite() {
        type B = burn::backend::Autodiff<burn::backend::Wgpu<burn::tensor::f16, i32>>;
        let device = burn::backend::wgpu::WgpuDevice::IntegratedGpu(0);
        let m = ContextAcoustic::<B>::new(10, DEFAULT_HIDDEN, DEFAULT_DEPTH, &device);
        let ids = Tensor::<B, 2, Int>::from_data(
            TensorData::new(vec![1i64; 8 * WINDOW], vec![8, WINDOW]),
            &device,
        );
        let feats = Tensor::<B, 2>::from_data(
            TensorData::new(vec![0.5f32; 16], vec![8, 2]),
            &device,
        );
        let pred = m.forward(ids, feats);
        let target = Tensor::<B, 2>::from_data(
            TensorData::new(vec![1.0f32; 8 * 128], vec![8, 128]),
            &device,
        );
        let loss = mse_loss(pred, target.clone());
        let v: f32 = loss.clone().into_scalar().elem();
        assert!(
            v.is_finite() && v > 0.0,
            "f16 forward produced non-finite loss: {v}"
        );
        // Backward + Adam step — the training-loop half the smoke above
        // doesn't exercise. f16 needs a larger epsilon (1e-5 underflows
        // f16's min normal 6.1e-5 → division by ~0 → NaN).
        let mut model = m;
        let mut optimizer = AdamConfig::new().with_epsilon(1e-3).init();
        let grads = loss.backward();
        let grads = GradientsParams::from_grads(grads, &model);
        model = optimizer.step(1e-3, model, grads);
        let ids2 = Tensor::<B, 2, Int>::from_data(
            TensorData::new(vec![1i64; 8 * WINDOW], vec![8, WINDOW]),
            &device,
        );
        let feats2 = Tensor::<B, 2>::from_data(
            TensorData::new(vec![0.5f32; 16], vec![8, 2]),
            &device,
        );
        let pred2 = model.forward(ids2, feats2);
        let loss2 = mse_loss(pred2, target);
        let v2: f32 = loss2.into_scalar().elem();
        assert!(
            v2.is_finite(),
            "f16 backward+step produced non-finite loss: {v2} (was {v})"
        );
    }
}

/// Manual training loop with phrase-level holdout: `eval_frac` of the
/// phrases are held out and their mel MSE is reported per epoch via the
/// `progress` callback (and returned). `stop` is checked between epochs
/// for cooperative cancellation (returns `stopped = true`). `init`
/// resumes from an existing model instead of fresh weights.
pub fn train_context<B: AutodiffBackend>(
    set: &TrainSet,
    epochs: usize,
    batch_size: usize,
    lr: f64,
    eval_frac: f64,
    hidden: usize,
    depth: usize,
    gpu_cap: Option<usize>,
    device: &B::Device,
    init: Option<ContextAcoustic<B>>,
    progress: &mut dyn FnMut(usize, f64, f64),
    stop: &AtomicBool,
) -> (ContextAcoustic<B>, f64, f64, bool) {
    // +1 for the pad token used by context windows at phrase edges.
    let mut model = init.unwrap_or_else(|| ContextAcoustic::<B>::new(set.vocab.len() + 1, hidden, depth, device));
    // epsilon 1e-3 (not 1e-5): 1e-5 underflows f16's min normal (6.1e-5)
    // → division by ~0 → NaN on the wgpu-f16 backend.
    let mut optimizer = AdamConfig::new()
        .with_weight_decay(Some(WeightDecayConfig::new(1e-4)))
        .with_epsilon(1e-3)
        .init();
    let mut last_train = f64::INFINITY;
    let mut last_val = f64::INFINITY;

    // Phrase-level split: shuffle phrase ids (deterministic seed) and
    // take the last eval_frac as validation — a random split, not the
    // bank-order tail (which is systematically the exotic aliases).
    let n_phrases = set.samples.last().map(|s| s.phrase_id + 1).unwrap_or(0);
    let n_val = ((n_phrases as f64) * eval_frac).round() as usize;
    let mut phrase_order: Vec<usize> = (0..n_phrases).collect();
    let mut rng = 0x9E3779B97F4A7C15u64;
    for i in (1..n_phrases).rev() {
        rng = xorshift(rng);
        let j = (rng % (i as u64 + 1)) as usize;
        phrase_order.swap(i, j);
    }
    let val_from = n_phrases.saturating_sub(n_val);
    let val_phrases: std::collections::HashSet<usize> =
        phrase_order[val_from..].iter().copied().collect();
    let train_samples: Vec<&FrameSample> = set
        .samples
        .iter()
        .filter(|s| !val_phrases.contains(&s.phrase_id))
        .collect();
    let val_samples: Vec<&FrameSample> = set
        .samples
        .iter()
        .filter(|s| val_phrases.contains(&s.phrase_id))
        .collect();
    eprintln!(
        "split: {} phrases ({} train / {} val), {} train frames / {} val frames",
        n_phrases,
        n_phrases - n_val,
        n_val,
        train_samples.len(),
        val_samples.len()
    );

    let n = train_samples.len();
    let mut stopped = false;
    for epoch in 0..epochs {
        if stop.load(Ordering::Relaxed) {
            stopped = true;
            break;
        }
        let mut indices: Vec<usize> = (0..n).collect();
        for i in (1..n).rev() {
            rng = xorshift(rng);
            let j = (rng % (i as u64 + 1)) as usize;
            indices.swap(i, j);
        }
        let mut epoch_loss = 0.0f64;
        let mut batches = 0usize;
        let mut sampled = 0usize;
        // Loss scalar readback syncs GPU→CPU every call — sample it every
        // 20 batches instead (the backward/step still runs every batch).
        for (bi, chunk) in indices.chunks(batch_size).enumerate() {
            let start = std::time::Instant::now();
            let (ids, feats, target) = build_batch(set, &train_samples, chunk, device);
            let pred = model.forward(ids, feats);
            let loss = mse_loss(pred, target);
            batches += 1;
            if bi % 20 == 0 {
                epoch_loss += loss.clone().into_scalar().elem::<f64>();
                sampled += 1;
            }

            let grads = loss.backward();
            let grads = GradientsParams::from_grads(grads, &model);
            model = optimizer.step(lr, model, grads);
            // GPU utilization cap: duty-cycle + adaptive busy correction.
            pace_gpu(gpu_cap, start.elapsed().as_secs_f64());
        }
        last_train = if sampled > 0 { epoch_loss / sampled as f64 } else { 0.0 };

        // Validation pass (no grad, dropout off).
        let mut val_loss = 0.0f64;
        if !val_samples.is_empty() {
            let dropout_prob = model.dropout.prob;
            model.dropout.prob = 0.0;
            let val_indices: Vec<usize> = (0..val_samples.len()).collect();
            let mut vbatches = 0usize;
            for chunk in val_indices.chunks(batch_size) {
                let (ids, feats, target) = build_batch(set, &val_samples, chunk, device);
                let pred = model.forward(ids, feats);
                let loss = mse_loss(pred, target);
                val_loss += loss.clone().into_scalar().elem::<f64>();
                vbatches += 1;
                pace_gpu(gpu_cap, 0.0);
            }
            model.dropout.prob = dropout_prob;
            last_val = val_loss / vbatches as f64;
        }
        progress(epoch, last_train, last_val);
        println!("epoch {epoch}: train = {last_train:.6}  val = {last_val:.6}");
    }
    (model, last_train, last_val, stopped)
}

/// Build one batch: for every frame gather its ±2 phoneme context
/// (padded at phrase edges), f0, position, and the teacher mel.
fn build_batch<B: Backend>(
    set: &TrainSet,
    samples: &[&FrameSample],
    chunk: &[usize],
    device: &B::Device,
) -> (Tensor<B, 2, Int>, Tensor<B, 2>, Tensor<B, 2>) {
    let b = chunk.len();
    let pad = set.vocab.len().min(EMBED_SLOTS - 1) as i64; // pad token id
    let mut ids: Vec<i64> = Vec::with_capacity(b * WINDOW);
    let mut feats: Vec<f32> = Vec::with_capacity(b * 2);
    let mut target: Vec<f32> = Vec::with_capacity(b * 128);
    for &si in chunk {
        let s = samples[si];
        let phrase = &set.phrases[s.phrase_id];
        for k in 0..WINDOW {
            let di = k as i64 - HALF as i64;
            let pi = s.phoneme_pos as i64 + di;
            let id = if pi < 0 || pi >= phrase.phoneme_ids.len() as i64 {
                pad
            } else {
                (phrase.phoneme_ids[pi as usize] as i64).min(EMBED_SLOTS as i64 - 1)
            };
            ids.push(id);
        }
        feats.push(s.f0_norm);
        feats.push(s.pos_frac);
        target.extend_from_slice(&s.mel);
    }
    (
        Tensor::<B, 2, Int>::from_data(TensorData::new(ids, vec![b, WINDOW]), device),
        Tensor::<B, 2>::from_data(TensorData::new(feats, vec![b, 2]), device),
        Tensor::<B, 2>::from_data(TensorData::new(target, vec![b, 128]), device),
    )
}

/// Like `build_batch` but with caller-provided targets (e.g. residual
/// mels for the group experts). `targets` is FULL-LENGTH, parallel to
/// `samples` (indexed by the chunk element, i.e. `samples[si]`).
fn build_batch_residual<B: Backend>(
    set: &TrainSet,
    samples: &[&FrameSample],
    targets: &[[f32; 128]],
    chunk: &[usize],
    device: &B::Device,
) -> (Tensor<B, 2, Int>, Tensor<B, 2>, Tensor<B, 2>) {
    let b = chunk.len();
    let pad = set.vocab.len().min(EMBED_SLOTS - 1) as i64; // pad token id
    let mut ids: Vec<i64> = Vec::with_capacity(b * WINDOW);
    let mut feats: Vec<f32> = Vec::with_capacity(b * 2);
    let mut target: Vec<f32> = Vec::with_capacity(b * 128);
    for &si in chunk {
        let s = samples[si];
        let phrase = &set.phrases[s.phrase_id];
        for k in 0..WINDOW {
            let di = k as i64 - HALF as i64;
            let pi = s.phoneme_pos as i64 + di;
            let id = if pi < 0 || pi >= phrase.phoneme_ids.len() as i64 {
                pad
            } else {
                (phrase.phoneme_ids[pi as usize] as i64).min(EMBED_SLOTS as i64 - 1)
            };
            ids.push(id);
        }
        feats.push(s.f0_norm);
        feats.push(s.pos_frac);
        target.extend_from_slice(&targets[si]);
    }
    (
        Tensor::<B, 2, Int>::from_data(TensorData::new(ids, vec![b, WINDOW]), device),
        Tensor::<B, 2>::from_data(TensorData::new(feats, vec![b, 2]), device),
        Tensor::<B, 2>::from_data(TensorData::new(target, vec![b, 128]), device),
    )
}

// ---------------------------------------------------------------------------
// MoE (plan Task 4): general expert on ALL frames + one group expert per
// phoneme natural class, trained on the residual (teacher − general).
// ---------------------------------------------------------------------------

/// Compute the general model's residual (teacher mel − general mel) for
/// every sample in `samples`, using the plain (non-autodiff) backend.
fn compute_residuals<B: Backend>(
    general_inner: &ContextAcoustic<B>,
    set: &TrainSet,
    samples: &[&FrameSample],
    batch_size: usize,
    device: &B::Device,
) -> Result<Vec<[f32; 128]>, String> {
    let mut residuals = Vec::with_capacity(samples.len());
    let idx: Vec<usize> = (0..samples.len()).collect();
    for chunk in idx.chunks(batch_size) {
        let (ids, feats, _t) = build_batch(set, samples, chunk, device);
        let pred = general_inner.forward(ids, feats); // [B,128]
        let mut flat = Vec::with_capacity(chunk.len() * 128);
        for &i in chunk {
            flat.extend_from_slice(&samples[i].mel);
        }
        let target =
            Tensor::<B, 2>::from_data(TensorData::new(flat, vec![chunk.len(), 128]), device);
        let res = target - pred;
        let data: TensorData = res.into_data();
        // convert::<f32> first — to_vec::<f32>() alone fails on f16 data
        // (dtype mismatch → swallowed Err → empty vec → panic).
        let vals: Vec<f32> = data.convert::<f32>().to_vec::<f32>().unwrap_or_default();
        for row in 0..chunk.len() {
            let mut r = [0f32; 128];
            r.copy_from_slice(&vals[row * 128..row * 128 + 128]);
            residuals.push(r);
        }
    }
    Ok(residuals)
}

fn mean_mse(residuals: &[[f32; 128]]) -> f64 {
    if residuals.is_empty() {
        return f64::NAN;
    }
    let sum: f64 = residuals
        .iter()
        .map(|r| r.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>())
        .sum();
    sum / (residuals.len() * 128) as f64
}

/// Train ONE group expert on residual targets (its group's frames only).
/// `init` resumes from an existing expert checkpoint.
fn train_group_expert<B: AutodiffBackend>(
    set: &TrainSet,
    samples: &[&FrameSample],
    residuals: &[[f32; 128]],
    epochs: usize,
    batch_size: usize,
    lr: f64,
    gpu_cap: Option<usize>,
    device: &B::Device,
    init: Option<ContextAcoustic<B>>,
    stop: &AtomicBool,
) -> (ContextAcoustic<B>, f64) {
    let mut model = init.unwrap_or_else(|| ContextAcoustic::<B>::new(set.vocab.len() + 1, EXPERT_HIDDEN, EXPERT_DEPTH, device));
    // epsilon 1e-3 (not 1e-5): 1e-5 underflows f16's min normal (6.1e-5)
    // → division by ~0 → NaN on the wgpu-f16 backend.
    let mut optimizer = AdamConfig::new()
        .with_weight_decay(Some(WeightDecayConfig::new(1e-4)))
        .with_epsilon(1e-3)
        .init();
    let mut last = f64::INFINITY;
    let n = samples.len();
    let mut rng = 0xDEADBEEFCAFEF00Du64;
    for _ in 0..epochs {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let mut indices: Vec<usize> = (0..n).collect();
        for i in (1..n).rev() {
            rng = xorshift(rng);
            let j = (rng % (i as u64 + 1)) as usize;
            indices.swap(i, j);
        }
        let mut acc = 0.0f64;
        let mut batches = 0usize;
        let mut sampled = 0usize;
        for (bi, chunk) in indices.chunks(batch_size).enumerate() {
            let start = std::time::Instant::now();
            let (ids, feats, target) = build_batch_residual(set, samples, residuals, chunk, device);
            let pred = model.forward(ids, feats);
            let loss = mse_loss(pred, target);
            batches += 1;
            if bi % 20 == 0 {
                acc += loss.clone().into_scalar().elem::<f64>();
                sampled += 1;
            }
            let grads = loss.backward();
            let grads = GradientsParams::from_grads(grads, &model);
            model = optimizer.step(lr, model, grads);
            // GPU utilization cap: duty-cycle + adaptive busy correction.
            pace_gpu(gpu_cap, start.elapsed().as_secs_f64());
        }
        last = if sampled > 0 { acc / sampled as f64 } else { 0.0 };
    }
    (model, last)
}

/// Full MoE training: general expert (all frames) + one residual expert
/// per phoneme group. Saves `{prefix}-general.mpk` and `{prefix}-g{g}.mpk`.
/// Returns per-group (group_id, residual_mse_before, residual_mse_after).
pub fn train_moe<B: AutodiffBackend>(
    set: &TrainSet,
    epochs: usize,
    batch_size: usize,
    lr: f64,
    eval_frac: f64,
    expert_epochs: usize,
    prefix: &str,
    hidden: usize,
    depth: usize,
    gpu_cap: Option<usize>,
    device: &B::Device,
    resume: bool,
    stop: &AtomicBool,
    progress: &mut dyn FnMut(usize, f64, f64),
) -> Result<Vec<(usize, f64, f64)>, String> {
    // 1. General expert — trained on ALL frames (existing loop).
    let general_init = if resume {
        try_load_model::<B>(&format!("{prefix}-general"), hidden, depth, device)?
    } else {
        None
    };
    if general_init.is_some() {
        println!("[moe] resume: general checkpoint found");
    }
    let (general, train_loss, val_loss, _stopped) = train_context::<B>(
        set,
        epochs,
        batch_size,
        lr,
        eval_frac,
        hidden,
        depth,
        gpu_cap,
        device,
        general_init,
        progress,
        stop,
    );
    println!("[moe] general done: train={train_loss:.4} val={val_loss:.4}");
    save_model(&general, &format!("{prefix}-general"))?;

    // 2. Per-group residual experts.
    let general_inner: ContextAcoustic<B::InnerBackend> = general.valid();
    let mut report: Vec<(usize, f64, f64)> = Vec::new();
    for g in 0..N_GROUPS {
        let frames: Vec<&FrameSample> = set
            .samples
            .iter()
            .filter(|s| s.group_id == g)
            .collect();
        if frames.len() < 200 {
            println!("[moe] group {g}: skip (only {} frames)", frames.len());
            continue;
        }
        let residuals = compute_residuals(&general_inner, set, &frames, batch_size, device)?;
        let before = mean_mse(&residuals);
        let expert_init = if resume {
            try_load_model::<B>(&format!("{prefix}-g{g}"), EXPERT_HIDDEN, EXPERT_DEPTH, device)?
        } else {
            None
        };
        let (expert, after) = train_group_expert::<B>(
            set,
            &frames,
            &residuals,
            expert_epochs,
            batch_size,
            lr,
            gpu_cap,
            &device,
            expert_init,
            stop,
        );
        save_model(&expert, &format!("{prefix}-g{g}"))?;
        println!(
            "[moe] group {g}: {} frames, residual {before:.4} -> {after:.4}",
            frames.len()
        );
        report.push((g, before, after));
    }
    Ok(report)
}

/// Serialize the trained module to a burn checkpoint file
/// (NamedMpk recorder; the extension is appended automatically).
pub fn save_model<B: Backend>(
    model: &ContextAcoustic<B>,
    path: &str,
) -> Result<(), String> {
    let recorder = NamedMpkFileRecorder::<FullPrecisionSettings>::new();
    model
        .clone()
        .save_file(path, &recorder)
        .map_err(|e| format!("save {path}: {e}"))
}

/// Load a burn checkpoint into a fresh module.
pub fn load_model<B: Backend>(
    vocab_size: usize,
    path: &str,
    hidden: usize,
    depth: usize,
    device: &B::Device,
) -> Result<ContextAcoustic<B>, String> {
    let recorder = NamedMpkFileRecorder::<FullPrecisionSettings>::new();
    // Record-only loader: `Module::load_file` needs an instance (self is
    // only used for its record type); build a dummy on the same device.
    let dummy = ContextAcoustic::<B>::new(vocab_size, hidden, depth, device);
    dummy
        .load_file(path, &recorder, device)
        .map_err(|e| format!("load {path}: {e}"))
}

fn xorshift(mut x: u64) -> u64 {
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    x.wrapping_mul(0x2545F4914F6CDD1D)
}
