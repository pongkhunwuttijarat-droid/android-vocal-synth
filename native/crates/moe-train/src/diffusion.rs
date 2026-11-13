//! Conditional residual diffusion for a frozen single-singer acoustic base.
//!
//! The base model is trained separately from scratch and frozen here. This
//! module only learns the teacher-minus-base mel residual, conditioned on
//! the frozen base mel and score-side features.

use burn::module::Module;
use burn::nn::{Dropout, DropoutConfig, Embedding, EmbeddingConfig, Linear, LinearConfig};
use burn::optim::decay::WeightDecayConfig;
use burn::optim::{AdamConfig, GradientsParams, Optimizer};
use burn::record::{FullPrecisionSettings, NamedMpkFileRecorder};
use burn::tensor::backend::{AutodiffBackend, Backend};
use burn::tensor::{ElementConversion, Int, Tensor, TensorData};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::baseline::{build_batch, ContextAcoustic, EMBED_SLOTS, HALF, WINDOW};
use crate::data::{FrameSample, TrainSet};

pub const DIFFUSION_STEPS: usize = 64;
pub const VALIDATION_SAMPLE_STEPS: usize = 16;
// Chosen so (base + denoiser) is close to the existing general + five
// group-expert package size at the same 4096-slot vocabulary.
pub const DEFAULT_HIDDEN: usize = 1440;
pub const DEFAULT_DEPTH: usize = 2;

/// Epsilon-predicting denoiser. Conditioning includes phoneme context,
/// score features, and the output of the frozen base singer.
#[derive(Module, Debug)]
pub struct ResidualDenoiser<B: Backend> {
    emb: Embedding<B>,
    fc_in: Linear<B>,
    hiddens: Vec<Linear<B>>,
    out: Linear<B>,
    dropout: Dropout,
}

impl<B: Backend> ResidualDenoiser<B> {
    pub fn new(hidden: usize, depth: usize, device: &B::Device) -> Self {
        // WINDOW*16 + (f0,pos) + base mel + noisy residual + time.
        let emb = EmbeddingConfig::new(EMBED_SLOTS, 16).init(device);
        let fc_in = LinearConfig::new(WINDOW * 16 + 2 + 128 + 128 + 1, hidden).init(device);
        let hiddens = (0..depth)
            .map(|_| LinearConfig::new(hidden, hidden).init(device))
            .collect();
        let out = LinearConfig::new(hidden, 128).init(device);
        Self {
            emb,
            fc_in,
            hiddens,
            out,
            dropout: DropoutConfig::new(0.1).init(),
        }
    }

    pub fn forward(
        &self,
        ids: Tensor<B, 2, Int>,
        feats: Tensor<B, 2>,
        base: Tensor<B, 2>,
        noisy_residual: Tensor<B, 2>,
        timestep: Tensor<B, 2>,
    ) -> Tensor<B, 2> {
        let x = self.emb.forward(ids).flatten(1, 2);
        let x = Tensor::cat(vec![x, feats, base, noisy_residual, timestep], 1);
        let mut x = self
            .dropout
            .forward(burn::tensor::activation::relu(self.fc_in.forward(x)));
        for layer in &self.hiddens {
            x = self
                .dropout
                .forward(burn::tensor::activation::relu(layer.forward(x)));
        }
        self.out.forward(x)
    }

    /// Disable dropout for deterministic inference with a loaded checkpoint.
    pub fn disable_dropout(&mut self) {
        self.dropout.prob = 0.0;
    }
}

#[derive(Clone, Debug)]
pub struct TrainReport {
    pub epochs: usize,
    pub train_epsilon_mse: f64,
    pub val_epsilon_mse: f64,
    pub base_val_mel_mse: f64,
    pub corrected_val_mel_mse: f64,
    pub base_seconds: f64,
    pub diffusion_seconds: f64,
}

/// Train the denoiser while treating `base` as immutable. Base predictions
/// are computed once; only the denoiser participates in autodiff/optimizer.
pub fn train<B: AutodiffBackend>(
    set: &TrainSet,
    base: &ContextAcoustic<B::InnerBackend>,
    epochs: usize,
    batch_size: usize,
    lr: f64,
    eval_frac: f64,
    hidden: usize,
    depth: usize,
    out_prefix: &str,
    device: &B::Device,
    stop: &AtomicBool,
    progress: &mut dyn FnMut(usize, f64, f64),
) -> Result<TrainReport, String> {
    if set.samples.is_empty() {
        return Err("empty train set".into());
    }
    if batch_size == 0 {
        return Err("batch size must be > 0".into());
    }
    if set.vocab.len() >= EMBED_SLOTS {
        return Err(format!(
            "phoneme vocab {} exceeds {}",
            set.vocab.len(),
            EMBED_SLOTS
        ));
    }

    let base_started = std::time::Instant::now();
    let mut frozen_base = base.clone();
    frozen_base.dropout.prob = 0.0;
    let (base_mels, base_seconds) = infer_base(&frozen_base, set, device)?;
    let _base_prepare = base_started.elapsed();
    let residuals: Vec<[f32; 128]> = set
        .samples
        .iter()
        .zip(&base_mels)
        .map(|(s, b)| {
            let mut row = [0.0; 128];
            for j in 0..128 {
                row[j] = s.mel[j] - b[j];
            }
            row
        })
        .collect();

    let (train_ids, val_ids) = split_samples(set, eval_frac);
    if train_ids.is_empty() || val_ids.is_empty() {
        return Err("train/validation split is empty".into());
    }
    eprintln!(
        "diffusion split: {} train frames / {} val frames ({} phrases)",
        train_ids.len(),
        val_ids.len(),
        set.phrases.len()
    );

    let mut model = ResidualDenoiser::<B>::new(hidden, depth, device);
    let mut optimizer = AdamConfig::new()
        .with_weight_decay(Some(WeightDecayConfig::new(1e-4)))
        .with_epsilon(1e-3)
        .init();
    let schedule = alpha_bars();
    let mut rng = 0x51A6_1E5D_2026_0926u64;
    let started = std::time::Instant::now();
    let mut last_train = f64::NAN;
    let mut last_val = f64::NAN;

    for epoch in 0..epochs {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let mut order = train_ids.clone();
        shuffle(&mut rng, &mut order);
        let mut loss_total = 0.0;
        let mut loss_batches = 0;
        for chunk in order.chunks(batch_size) {
            let batch = make_batch(
                set, chunk, &base_mels, &residuals, &schedule, &mut rng, device,
            );
            let pred = model.forward(
                batch.ids,
                batch.feats,
                batch.base,
                batch.noisy,
                batch.timestep,
            );
            let loss = mse(pred, batch.noise);
            let value: f64 = loss.clone().into_scalar().elem();
            if value.is_finite() {
                loss_total += value;
                loss_batches += 1;
            }
            let grads = GradientsParams::from_grads(loss.backward(), &model);
            model = optimizer.step(lr, model, grads);
        }
        last_train = if loss_batches > 0 {
            loss_total / loss_batches as f64
        } else {
            f64::NAN
        };
        let dropout_prob = model.dropout.prob;
        model.dropout.prob = 0.0;
        last_val = validation_epsilon_mse(
            &model, set, &val_ids, &base_mels, &residuals, &schedule, device, 4096,
        );
        model.dropout.prob = dropout_prob;
        progress(epoch, last_train, last_val);
    }

    let diffusion_seconds = started.elapsed().as_secs_f64();
    let dropout_prob = model.dropout.prob;
    model.dropout.prob = 0.0;
    let corrected_val_mel_mse = validation_ddim_mel_mse(
        &model,
        set,
        &val_ids,
        &base_mels,
        &schedule,
        device,
        4096,
        VALIDATION_SAMPLE_STEPS,
    );
    model.dropout.prob = dropout_prob;
    let recorder = NamedMpkFileRecorder::<FullPrecisionSettings>::new();
    model
        .clone()
        .save_file(out_prefix, &recorder)
        .map_err(|e| format!("save diffusion {out_prefix}: {e}"))?;
    Ok(TrainReport {
        epochs,
        train_epsilon_mse: last_train,
        val_epsilon_mse: last_val,
        base_val_mel_mse: mean_residual_mse(&residuals, &val_ids),
        corrected_val_mel_mse,
        base_seconds,
        diffusion_seconds,
    })
}

fn infer_base<B: Backend>(
    base: &ContextAcoustic<B>,
    set: &TrainSet,
    device: &B::Device,
) -> Result<(Vec<[f32; 128]>, f64), String> {
    let start = std::time::Instant::now();
    let samples: Vec<&FrameSample> = set.samples.iter().collect();
    let idx: Vec<usize> = (0..samples.len()).collect();
    let mut out = Vec::with_capacity(samples.len());
    for chunk in idx.chunks(1024) {
        let (ids, feats, _) = build_batch(set, &samples, chunk, device);
        let values: Vec<f32> = base
            .forward(ids, feats)
            .into_data()
            .convert::<f32>()
            .to_vec::<f32>()
            .map_err(|e| format!("base output conversion: {e}"))?;
        if values.len() != chunk.len() * 128 {
            return Err("base output shape mismatch".into());
        }
        for row in values.chunks_exact(128) {
            let mut mel = [0.0; 128];
            mel.copy_from_slice(row);
            out.push(mel);
        }
    }
    Ok((out, start.elapsed().as_secs_f64()))
}

struct Batch<B: Backend> {
    ids: Tensor<B, 2, Int>,
    feats: Tensor<B, 2>,
    base: Tensor<B, 2>,
    noisy: Tensor<B, 2>,
    timestep: Tensor<B, 2>,
    noise: Tensor<B, 2>,
}

fn make_batch<B: Backend>(
    set: &TrainSet,
    chunk: &[usize],
    base: &[[f32; 128]],
    residuals: &[[f32; 128]],
    schedule: &[f32],
    rng: &mut u64,
    device: &B::Device,
) -> Batch<B> {
    let n = chunk.len();
    let pad = set.vocab.len() as i64;
    let mut ids = Vec::with_capacity(n * WINDOW);
    let mut feats = Vec::with_capacity(n * 2);
    let mut base_flat = Vec::with_capacity(n * 128);
    let mut noisy_flat = Vec::with_capacity(n * 128);
    let mut noise_flat = Vec::with_capacity(n * 128);
    let mut timestep = Vec::with_capacity(n);
    for &si in chunk {
        let s = &set.samples[si];
        let phrase = &set.phrases[s.phrase_id];
        for k in 0..WINDOW {
            let pi = s.phoneme_pos as isize + k as isize - HALF as isize;
            ids.push(if pi < 0 || pi >= phrase.phoneme_ids.len() as isize {
                pad
            } else {
                phrase.phoneme_ids[pi as usize] as i64
            });
        }
        feats.push(s.f0_norm);
        feats.push(s.pos_frac);
        base_flat.extend_from_slice(&base[si]);
        let t = (next_u64(rng) as usize) % DIFFUSION_STEPS;
        timestep.push(t as f32 / (DIFFUSION_STEPS - 1) as f32);
        let a = schedule[t];
        let sa = a.sqrt();
        let so = (1.0 - a).sqrt();
        for j in 0..128 {
            let z = normal(rng);
            noise_flat.push(z);
            noisy_flat.push(sa * residuals[si][j] + so * z);
        }
    }
    Batch {
        ids: Tensor::from_data(TensorData::new(ids, vec![n, WINDOW]), device),
        feats: Tensor::from_data(TensorData::new(feats, vec![n, 2]), device),
        base: Tensor::from_data(TensorData::new(base_flat, vec![n, 128]), device),
        noisy: Tensor::from_data(TensorData::new(noisy_flat, vec![n, 128]), device),
        timestep: Tensor::from_data(TensorData::new(timestep, vec![n, 1]), device),
        noise: Tensor::from_data(TensorData::new(noise_flat, vec![n, 128]), device),
    }
}

fn validation_epsilon_mse<B: Backend>(
    model: &ResidualDenoiser<B>,
    set: &TrainSet,
    ids: &[usize],
    base: &[[f32; 128]],
    residuals: &[[f32; 128]],
    schedule: &[f32],
    device: &B::Device,
    limit: usize,
) -> f64 {
    let stride = (ids.len() / limit.max(1)).max(1);
    let chosen: Vec<usize> = ids.iter().step_by(stride).take(limit).copied().collect();
    let mut rng = 0xBADC_0FFE_2026u64;
    let mut sum = 0.0;
    let mut n = 0;
    for chunk in chosen.chunks(512) {
        let batch = make_batch(set, chunk, base, residuals, schedule, &mut rng, device);
        let pred = model.forward(
            batch.ids,
            batch.feats,
            batch.base,
            batch.noisy,
            batch.timestep,
        );
        let value: f64 = mse(pred, batch.noise).into_scalar().elem();
        sum += value;
        n += 1;
    }
    if n == 0 {
        f64::NAN
    } else {
        sum / n as f64
    }
}

fn validation_ddim_mel_mse<B: Backend>(
    model: &ResidualDenoiser<B>,
    set: &TrainSet,
    ids: &[usize],
    base: &[[f32; 128]],
    schedule: &[f32],
    device: &B::Device,
    limit: usize,
    sample_steps: usize,
) -> f64 {
    // Deterministic DDIM sampler for a bounded held-out quality estimate.
    let stride = (ids.len() / limit.max(1)).max(1);
    let chosen: Vec<usize> = ids.iter().step_by(stride).take(limit).copied().collect();
    let sample_steps = sample_steps.clamp(2, DIFFUSION_STEPS);
    let steps: Vec<usize> = (0..sample_steps)
        .map(|i| DIFFUSION_STEPS - 1 - i * (DIFFUSION_STEPS - 1) / (sample_steps - 1))
        .collect();
    let mut sum = 0.0;
    let mut count = 0usize;
    for chunk in chosen.chunks(512) {
        let n = chunk.len();
        let pad = set.vocab.len() as i64;
        let (mut pids, mut feats, mut bflat, mut state) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut rng = 0xD1FF_0510_2026u64;
        for &si in chunk {
            let s = &set.samples[si];
            let phrase = &set.phrases[s.phrase_id];
            for k in 0..WINDOW {
                let pi = s.phoneme_pos as isize + k as isize - HALF as isize;
                pids.push(if pi < 0 || pi >= phrase.phoneme_ids.len() as isize {
                    pad
                } else {
                    phrase.phoneme_ids[pi as usize] as i64
                });
            }
            feats.extend([s.f0_norm, s.pos_frac]);
            bflat.extend_from_slice(&base[si]);
            for _ in 0..128 {
                state.push(normal(&mut rng));
            }
        }
        for (step_idx, &t) in steps.iter().enumerate() {
            let a = schedule[t];
            let noisy_t =
                Tensor::<B, 2>::from_data(TensorData::new(state.clone(), vec![n, 128]), device);
            let ids_t = Tensor::<B, 2, Int>::from_data(
                TensorData::new(pids.clone(), vec![n, WINDOW]),
                device,
            );
            let feat_t =
                Tensor::<B, 2>::from_data(TensorData::new(feats.clone(), vec![n, 2]), device);
            let base_t =
                Tensor::<B, 2>::from_data(TensorData::new(bflat.clone(), vec![n, 128]), device);
            let time_t = Tensor::<B, 2>::from_data(
                TensorData::new(vec![t as f32 / (DIFFUSION_STEPS - 1) as f32; n], vec![n, 1]),
                device,
            );
            let pred: Vec<f32> = model
                .forward(ids_t, feat_t, base_t, noisy_t, time_t)
                .into_data()
                .convert::<f32>()
                .to_vec::<f32>()
                .unwrap_or_default();
            let prev = if step_idx + 1 < steps.len() {
                schedule[steps[step_idx + 1]]
            } else {
                1.0
            };
            let sa = a.sqrt();
            let so = (1.0 - a).sqrt();
            let sp = prev.sqrt();
            let sop = (1.0 - prev).sqrt();
            for k in 0..state.len() {
                // At the noisiest cosine step alpha_bar is close to zero, so
                // small epsilon errors can create arbitrarily large x0 values.
                // Residual mel corrections are bounded; clipping x0 prevents
                // a single unstable estimate from contaminating every step.
                let clean = ((state[k] - so * pred[k]) / sa).clamp(-1.0, 1.0);
                state[k] = sp * clean + sop * pred[k];
            }
        }
        for row in 0..n {
            for j in 0..128 {
                let e = (base[chunk[row]][j] + state[row * 128 + j]
                    - set.samples[chunk[row]].mel[j]) as f64;
                sum += e * e;
                count += 1;
            }
        }
    }
    if count == 0 {
        f64::NAN
    } else {
        sum / count as f64
    }
}

/// Evaluate a saved base+diffusion pair on the same deterministic phrase
/// holdout used during training. Used to inspect full-step sampling without
/// retraining the denoiser.
pub fn evaluate<B: Backend>(
    set: &TrainSet,
    base: &ContextAcoustic<B>,
    model: &ResidualDenoiser<B>,
    sample_steps: usize,
    device: &B::Device,
) -> Result<(f64, f64), String> {
    let mut base = base.clone();
    base.dropout.prob = 0.0;
    let mut model = model.clone();
    model.dropout.prob = 0.0;
    let (base_mels, _) = infer_base(&base, set, device)?;
    let residuals: Vec<[f32; 128]> = set
        .samples
        .iter()
        .zip(&base_mels)
        .map(|(s, b)| {
            let mut r = [0.0; 128];
            for j in 0..128 {
                r[j] = s.mel[j] - b[j]
            }
            r
        })
        .collect();
    let (_, val_ids) = split_samples(set, 0.1);
    let schedule = alpha_bars();
    let base_mse = mean_residual_mse(&residuals, &val_ids);
    let corrected = validation_ddim_mel_mse(
        &model,
        set,
        &val_ids,
        &base_mels,
        &schedule,
        device,
        4096,
        sample_steps,
    );
    Ok((base_mse, corrected))
}

pub fn load_model<B: Backend>(
    prefix: &str,
    hidden: usize,
    depth: usize,
    device: &B::Device,
) -> Result<ResidualDenoiser<B>, String> {
    let recorder = NamedMpkFileRecorder::<FullPrecisionSettings>::new();
    ResidualDenoiser::<B>::new(hidden, depth, device)
        .load_file(prefix, &recorder, device)
        .map_err(|e| format!("load diffusion {prefix}: {e}"))
}

/// Sample one deterministic DDIM residual batch. Inputs use the same layout
/// as the frame-wise trainer: ids `[N, WINDOW]`, features `[N, 2]`, and base
/// mel `[N, 128]`. The seed lets renderers make phrase output repeatable.
pub fn sample_residual_values<B: Backend>(
    model: &ResidualDenoiser<B>,
    ids: &[i64],
    feats: &[f32],
    base: &[[f32; 128]],
    sample_steps: usize,
    seed: u64,
    device: &B::Device,
) -> Result<Vec<[f32; 128]>, String> {
    let n = base.len();
    if n == 0 {
        return Ok(Vec::new());
    }
    if ids.len() != n * WINDOW || feats.len() != n * 2 {
        return Err("diffusion sampler input shape mismatch".into());
    }
    let sample_steps = sample_steps.clamp(2, DIFFUSION_STEPS);
    let steps: Vec<usize> = (0..sample_steps)
        .map(|i| DIFFUSION_STEPS - 1 - i * (DIFFUSION_STEPS - 1) / (sample_steps - 1))
        .collect();
    let schedule = alpha_bars();
    let mut rng = seed;
    let mut state: Vec<f32> = (0..n * 128).map(|_| normal(&mut rng)).collect();
    let base_flat: Vec<f32> = base.iter().flatten().copied().collect();
    for (step_idx, &t) in steps.iter().enumerate() {
        let noisy = Tensor::<B, 2>::from_data(TensorData::new(state.clone(), vec![n, 128]), device);
        let ids_t =
            Tensor::<B, 2, Int>::from_data(TensorData::new(ids.to_vec(), vec![n, WINDOW]), device);
        let feats_t =
            Tensor::<B, 2>::from_data(TensorData::new(feats.to_vec(), vec![n, 2]), device);
        let base_t =
            Tensor::<B, 2>::from_data(TensorData::new(base_flat.clone(), vec![n, 128]), device);
        let time_t = Tensor::<B, 2>::from_data(
            TensorData::new(vec![t as f32 / (DIFFUSION_STEPS - 1) as f32; n], vec![n, 1]),
            device,
        );
        let pred: Vec<f32> = model
            .forward(ids_t, feats_t, base_t, noisy, time_t)
            .into_data()
            .convert::<f32>()
            .to_vec::<f32>()
            .map_err(|e| format!("diffusion epsilon conversion: {e}"))?;
        if pred.len() != state.len() {
            return Err("diffusion epsilon shape mismatch".into());
        }
        let a = schedule[t];
        let prev = if step_idx + 1 < steps.len() {
            schedule[steps[step_idx + 1]]
        } else {
            1.0
        };
        let sa = a.sqrt();
        let so = (1.0 - a).sqrt();
        let sp = prev.sqrt();
        let sop = (1.0 - prev).sqrt();
        for k in 0..state.len() {
            let clean = ((state[k] - so * pred[k]) / sa).clamp(-1.0, 1.0);
            state[k] = sp * clean + sop * pred[k];
        }
    }
    let mut out = Vec::with_capacity(n);
    for row in state.chunks_exact(128) {
        let mut mel = [0.0; 128];
        mel.copy_from_slice(row);
        out.push(mel);
    }
    Ok(out)
}

fn split_samples(set: &TrainSet, eval_frac: f64) -> (Vec<usize>, Vec<usize>) {
    let n_phrases = set.phrases.len();
    let n_val = ((n_phrases as f64) * eval_frac.clamp(0.0, 0.5)).round() as usize;
    let mut phrases: Vec<usize> = (0..n_phrases).collect();
    let mut rng = 0x9E37_79B9_7F4A_7C15u64;
    shuffle(&mut rng, &mut phrases);
    let hold: std::collections::HashSet<usize> = phrases.into_iter().rev().take(n_val).collect();
    let mut tr = Vec::new();
    let mut va = Vec::new();
    for (i, s) in set.samples.iter().enumerate() {
        if hold.contains(&s.phrase_id) {
            va.push(i)
        } else {
            tr.push(i)
        }
    }
    (tr, va)
}
fn mean_residual_mse(residuals: &[[f32; 128]], ids: &[usize]) -> f64 {
    let sum: f64 = ids
        .iter()
        .flat_map(|&i| residuals[i].iter())
        .map(|&v| (v as f64) * (v as f64))
        .sum();
    sum / (ids.len().max(1) * 128) as f64
}
fn alpha_bars() -> Vec<f32> {
    // Cosine schedule reaches near-pure noise even with only 64 training
    // steps. A conventional 1e-4..0.02 linear beta schedule would leave
    // alpha_bar≈0.52 at t=63, making a Gaussian inference start inconsistent.
    let s = 0.008f64;
    let f0 = (s / (1.0 + s) * std::f64::consts::FRAC_PI_2).cos().powi(2);
    let alpha_bar = |i: usize| {
        let x = (i as f64 / DIFFUSION_STEPS as f64 + s) / (1.0 + s) * std::f64::consts::FRAC_PI_2;
        x.cos().powi(2) / f0
    };
    let mut product = 1.0f64;
    let mut out = Vec::with_capacity(DIFFUSION_STEPS);
    for t in 0..DIFFUSION_STEPS {
        let beta = (1.0 - alpha_bar(t + 1) / alpha_bar(t)).clamp(0.0, 0.999);
        product *= 1.0 - beta;
        out.push(product as f32);
    }
    out
}
fn mse<B: Backend>(pred: Tensor<B, 2>, target: Tensor<B, 2>) -> Tensor<B, 1> {
    let d = pred - target;
    (d.clone() * d).mean_dim(1).mean()
}
fn shuffle(rng: &mut u64, values: &mut [usize]) {
    // Match baseline::train_context's PRNG state transition exactly so all
    // candidates use the same phrase-level train/validation partition.
    for i in (1..values.len()).rev() {
        *rng = xorshift(*rng);
        let j = (*rng % (i as u64 + 1)) as usize;
        values.swap(i, j);
    }
}
fn xorshift(mut x: u64) -> u64 {
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    x.wrapping_mul(0x2545F4914F6CDD1D)
}
fn next_u64(x: &mut u64) -> u64 {
    *x ^= *x >> 12;
    *x ^= *x << 25;
    *x ^= *x >> 27;
    x.wrapping_mul(0x2545F4914F6CDD1D)
}
fn normal(rng: &mut u64) -> f32 {
    let u1 = ((next_u64(rng) >> 11) as f64 + 1.0) / ((1u64 << 53) as f64 + 1.0);
    let u2 = ((next_u64(rng) >> 11) as f64) / ((1u64 << 53) as f64);
    ((-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()) as f32
}
