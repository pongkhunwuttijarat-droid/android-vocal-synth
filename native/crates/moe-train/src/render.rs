//! Rendering: MoE (general + routed group experts) → mel → pc-nsf-hifigan
//! vocoder → wav. The teacher-comparison path for the POC: same phrase
//! rendered by R2 (train-prep --dump-wav) and by the student, then
//! compared (RMS/correlation) — the golden-standard check.

use burn::tensor::backend::Backend;
use burn::tensor::{Int, Tensor, TensorData};

use crate::baseline::{ContextAcoustic, WINDOW, HALF};
use crate::data::TrainSet;

/// Loaded MoE: general expert + one optional expert per group.
pub struct LoadedMoe<B: Backend> {
    pub general: ContextAcoustic<B>,
    pub experts: Vec<Option<ContextAcoustic<B>>>,
}

/// Load `{prefix}-general.mpk` + `{prefix}-g{g}.mpk` (missing experts
/// become None — their frames use the general alone).
pub fn load_moe<B: Backend>(
    set: &TrainSet,
    prefix: &str,
    hidden: usize,
    depth: usize,
    device: &B::Device,
) -> Result<LoadedMoe<B>, String> {
    let general = load_general::<B>(prefix, hidden, depth, device)?;
    let mut experts = Vec::new();
    for g in 0..crate::phoneme_groups::N_GROUPS {
        let path = format!("{prefix}-g{g}");
        experts.push(
            load_expert::<B>(&path, device).ok(),
        ); // missing expert → None
    }
    Ok(LoadedMoe { general, experts })
}
// Prevent dead-code warning when only the set-free path is used.
#[allow(dead_code)]
fn _moe_uses_load_general(_: &TrainSet) {}

/// Load the general expert with a caller-specified size. `hidden`/`depth`
/// come from the training run (the big general may be `--hidden 7680
/// --depth 2`); the `.mpk` record carries the shapes, so a mismatch here
/// fails at load.
pub fn load_general<B: Backend>(
    prefix: &str,
    hidden: usize,
    depth: usize,
    device: &B::Device,
) -> Result<ContextAcoustic<B>, String> {
    use burn::module::Module;
    use burn::record::{FullPrecisionSettings, NamedMpkFileRecorder};
    let recorder = NamedMpkFileRecorder::<FullPrecisionSettings>::new();
    // `ContextAcoustic::new` ignores `_vocab_size` (always EMBED_SLOTS) —
    // a set-free load needs only the shapes (hidden/depth) to build the
    // dummy. This is the POC engine path: the .mpk is loaded against a
    // dummy whose size matches the TRAIN run, not any live train-set.
    let dummy = ContextAcoustic::<B>::new(0, hidden, depth, device);
    let path = format!("{prefix}-general");
    dummy
        .load_file(&path, &recorder, device)
        .map_err(|e| format!("load {path}: {e}"))
}

/// Load one group expert (small, always EXPERT_HIDDEN/EXPERT_DEPTH).
pub fn load_expert<B: Backend>(
    path: &str,
    device: &B::Device,
) -> Result<ContextAcoustic<B>, String> {
    use burn::module::Module;
    use burn::record::{FullPrecisionSettings, NamedMpkFileRecorder};
    let recorder = NamedMpkFileRecorder::<FullPrecisionSettings>::new();
    let dummy = ContextAcoustic::<B>::new(
        0,
        crate::baseline::EXPERT_HIDDEN,
        crate::baseline::EXPERT_DEPTH,
        device,
    );
    dummy
        .load_file(path, &recorder, device)
        .map_err(|e| format!("load {path}: {e}"))
}

/// Predict the mel for one phrase (all its frames): general(x) +
/// expert[group(x)](x). Returns (mel [frames, 128], f0 [frames]).
pub fn predict_phrase_mel<B: Backend>(
    moe: &LoadedMoe<B>,
    set: &TrainSet,
    phrase_id: usize,
    device: &B::Device,
) -> Result<(Vec<[f32; 128]>, Vec<f32>), String> {
    let samples: Vec<&crate::data::FrameSample> = set
        .samples
        .iter()
        .filter(|s| s.phrase_id == phrase_id)
        .collect();
    if samples.is_empty() {
        return Err(format!("phrase {phrase_id}: no frames"));
    }
    let idx: Vec<usize> = (0..samples.len()).collect();
    let mut mel_out: Vec<[f32; 128]> = Vec::with_capacity(samples.len());
    let mut f0_out: Vec<f32> = Vec::with_capacity(samples.len());
    for chunk in idx.chunks(512) {
        let (ids, feats) = build_render_batch(set, &samples, chunk, device);
        let general_mel = moe.general.forward(ids.clone(), feats.clone()); // [B,128]
        // Routed expert contribution: scatter each frame's group-expert
        // output into a full-batch zeros tensor (groups WITHOUT an
        // expert — e.g. skipped — keep zeros; the general covers them).
        let mut expert_add: Option<Tensor<B, 2>> = None;
        for (pos, &si) in chunk.iter().enumerate() {
            if let Some(exp) = &moe.experts[samples[si].group_id] {
                let one_ids = ids.clone().narrow(0, pos, 1);
                let one_feats = feats.clone().narrow(0, pos, 1);
                let e_mel = exp.forward(one_ids, one_feats); // [1,128]
                expert_add = Some(match expert_add {
                    None => {
                        let mut z = Tensor::<B, 2>::zeros([chunk.len(), 128], device);
                        z = z.slice_assign([pos..pos + 1, 0..128], e_mel);
                        z
                    }
                    Some(acc) => acc.slice_assign([pos..pos + 1, 0..128], e_mel),
                });
            }
        }
        let pred = match expert_add {
            Some(add) => general_mel + add,
            None => general_mel,
        };
        let data: TensorData = pred.into_data();
        // convert::<f32> first (f16 data would fail to_vec::<f32>).
        let vals: Vec<f32> = data.convert::<f32>().to_vec::<f32>().unwrap_or_default();
        for (i, &si) in chunk.iter().enumerate() {
            let mut r = [0f32; 128];
            r.copy_from_slice(&vals[i * 128..i * 128 + 128]);
            mel_out.push(r);
            f0_out.push(samples[si].f0_norm * 500.0); // denormalize
        }
    }
    Ok((mel_out, f0_out))
}

/// Context-window batch builder for rendering (targets unused).
fn build_render_batch<B: Backend>(
    set: &TrainSet,
    samples: &[&crate::data::FrameSample],
    chunk: &[usize],
    device: &B::Device,
) -> (Tensor<B, 2, Int>, Tensor<B, 2>) {
    let b = chunk.len();
    let pad = set.vocab.len() as i64;
    let mut ids: Vec<i64> = Vec::with_capacity(b * WINDOW);
    let mut feats: Vec<f32> = Vec::with_capacity(b * 2);
    for &si in chunk {
        let s = samples[si];
        let phrase = &set.phrases[s.phrase_id];
        for k in 0..WINDOW {
            let di = k as i64 - HALF as i64;
            let pi = s.phoneme_pos as i64 + di;
            let id = if pi < 0 || pi >= phrase.phoneme_ids.len() as i64 {
                pad
            } else {
                phrase.phoneme_ids[pi as usize] as i64
            };
            ids.push(id);
        }
        feats.push(s.f0_norm);
        feats.push(s.pos_frac);
    }
    (
        Tensor::<B, 2, Int>::from_data(TensorData::new(ids, vec![b, WINDOW]), device),
        Tensor::<B, 2>::from_data(TensorData::new(feats, vec![b, 2]), device),
    )
}

#[cfg(feature = "ort-vocoder")]
/// Open the pc-nsf-hifigan session once (reused across phrases).
/// `intra_threads` caps the session's op thread pool (0 = ort default) —
/// parallel render workers each open a session, and the default (all
/// cores per session!) oversubscribes the CPU badly.
pub fn open_vocoder_threads(vocoder_path: &str, intra_threads: usize) -> Result<ort::session::Session, String> {
    let mut builder = ort::session::Session::builder().map_err(|e| format!("vocoder builder: {e}"))?;
    if intra_threads > 0 {
        builder = builder
            .with_intra_threads(intra_threads)
            .map_err(|e| format!("vocoder threads: {e}"))?;
    }
    builder
        .commit_from_file(vocoder_path)
        .map_err(|e| format!("load vocoder: {e}"))
}

#[cfg(feature = "ort-vocoder")]
/// Open with ort's default thread pool (single-phrase rendering).
pub fn open_vocoder(vocoder_path: &str) -> Result<ort::session::Session, String> {
    open_vocoder_threads(vocoder_path, 0)
}

#[cfg(feature = "ort-vocoder")]
/// pc-nsf-hifigan: mel [1,N,128] + f0 [1,N] → audio [N*hop] (44.1k).
pub fn run_vocoder(
    session: &mut ort::session::Session,
    mel: &[[f32; 128]],
    f0: &[f32],
) -> Result<Vec<f32>, String> {
    let n = mel.len();
    let mel_flat: Vec<f32> = mel.iter().flatten().copied().collect();
    let mel_arr = ndarray::Array3::from_shape_vec((1, n, 128), mel_flat)
        .map_err(|e| format!("mel shape: {e}"))?;
    let f0_arr = ndarray::Array2::from_shape_vec((1, n), f0.to_vec())
        .map_err(|e| format!("f0 shape: {e}"))?;
    let inputs = ort::inputs![
        ort::value::Value::from_array(mel_arr).map_err(|e| format!("mel tensor: {e}"))?,
        ort::value::Value::from_array(f0_arr).map_err(|e| format!("f0 tensor: {e}"))?,
    ];
    let out = session.run(inputs).map_err(|e| format!("vocoder run: {e}"))?;
    let (_, audio) = out[0]
        .try_extract_tensor::<f32>()
        .map_err(|e| format!("audio extract: {e}"))?;
    Ok(audio.to_vec())
}

#[cfg(feature = "ort-vocoder")]
/// One-shot convenience (single-phrase rendering): open + run.
pub fn run_vocoder_once(vocoder_path: &str, mel: &[[f32; 128]], f0: &[f32]) -> Result<Vec<f32>, String> {
    let mut session = open_vocoder(vocoder_path)?;
    run_vocoder(&mut session, mel, f0)
}

/// Render one phrase to a 16-bit PCM WAV (44.1 kHz mono). `gain`
/// scales the audio (volume calibration — the student renders ~4×
/// quieter than the teacher without it).
#[cfg(feature = "ort-vocoder")]
pub fn render_phrase_wav<B: Backend>(
    moe: &LoadedMoe<B>,
    set: &TrainSet,
    phrase_id: usize,
    vocoder_path: &str,
    gain: f32,
    device: &B::Device,
) -> Result<Vec<u8>, String> {
    let (mel, f0) = predict_phrase_mel(moe, set, phrase_id, device)?;
    let mut audio = run_vocoder_once(vocoder_path, &mel, &f0)?;
    if gain != 1.0 {
        for s in &mut audio {
            *s *= gain;
        }
    }
    Ok(wavwriter::write_wav_16(&audio, 44100))
}

/// Render with a shared vocoder session (batch rendering — the session
/// open is the expensive part, so render-all reuses one).
#[cfg(feature = "ort-vocoder")]
pub fn render_phrase_wav_with_session<B: Backend>(
    moe: &LoadedMoe<B>,
    set: &TrainSet,
    phrase_id: usize,
    vocoder: &mut ort::session::Session,
    gain: f32,
    device: &B::Device,
) -> Result<Vec<u8>, String> {
    let (mel, f0) = predict_phrase_mel(moe, set, phrase_id, device)?;
    let mut audio = run_vocoder(vocoder, &mel, &f0)?;
    if gain != 1.0 {
        for s in &mut audio {
            *s *= gain;
        }
    }
    Ok(wavwriter::write_wav_16(&audio, 44100))
}
