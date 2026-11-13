//! MoeEngine (engine #5) — the MoE student acoustic as a selectable
//! engine. This is the distillation path: a per-frame acoustic learned
//! from the R2 teacher's (mel, f0) via moe-train. It is a rendering
//! engine like r1/r2/classic/diffsinger — added, never replacing them.
//!
//! Data-flow (RenderInput → PCM):
//!   [`RenderInput`] phonemes  ── timing + group routing ──▶ per-frame
//!     (phoneme_id, group_of(phoneme), f0_norm, pos_frac)
//!   general(x) + expert[group(x)](x)  ──▶ mel [frames,128]
//!   mel + f0 ── pc-nsf-hifigan ──▶ PCM (44.1 kHz)
//!
//! The engine is trained-set-free: it loads `{prefix}-general.mpk` +
//! `{prefix}-g{0..4}.mpk` and a `{prefix}-phonemes.txt` vocab sidecar
//! (written by moe-train at train time) — a live `TrainSet` is not
//! required at render.

use std::path::{Path, PathBuf};

use domain::capabilities::EngineCapabilities;
use domain::UProject;
use feed::render_input::RenderInput;
use moe_train::baseline::{EMBED_SLOTS, HALF, WINDOW};
use moe_train::phoneme_groups::group_of;
use moe_train::render::{run_vocoder_once, LoadedMoe};
use voicebank::Voicebank;
use worldline_plugin::WorldlineCapabilities;

use crate::engine::Engine;
use crate::pipeline::{self, PhonemizerKind, RenderReport};

/// The wgpu backend drives the burn acoustic on the GPU. The f16 flavour
/// matches the trained checkpoints (moe-train `--backend wgpu-f16`).
pub type MoeBackend = burn::backend::Wgpu<burn::tensor::f16, i32>;

/// MoE student engine: `RenderInput` → mel (via group-routed experts) →
/// pc-nsf-hifigan → PCM.
pub struct MoeEngine {
    moe: LoadedMoe<MoeBackend>,
    /// phoneme symbol → embedding id (the vocab the checkpoints were
    /// trained on). Loaded from `{prefix}-phonemes.txt`.
    vocab: std::collections::HashMap<String, usize>,
    device: burn::backend::wgpu::WgpuDevice,
    vocoder: PathBuf,
    verbose: bool,
}

impl MoeEngine {
    /// Open the MoE engine from a checkpoint prefix plus the vocab
    /// sidecar and a pc-nsf-hifigan vocoder. `hidden`/`depth` are the
    /// general-expert sizes used at train time (default from moe-train).
    pub fn open(
        prefix: impl AsRef<Path>,
        vocoder: impl AsRef<Path>,
        hidden: usize,
        depth: usize,
        verbose: bool,
    ) -> Result<Self, String> {
        let prefix = prefix.as_ref().to_path_buf();
        let device = burn::backend::wgpu::WgpuDevice::DefaultDevice;
        // Load checkpoints without a live TrainSet (set-free path).
        let general = moe_train::render::load_general::<MoeBackend>(
            &prefix.to_string_lossy(),
            hidden,
            depth,
            &device,
        )?;
        let mut experts = Vec::new();
        for g in 0..moe_train::phoneme_groups::N_GROUPS {
            let path = format!("{}-g{g}", prefix.to_string_lossy());
            experts.push(moe_train::render::load_expert::<MoeBackend>(&path, &device).ok());
        }
        let moe = LoadedMoe { general, experts };
        let vocab = load_vocab_sidecar(&prefix)?;
        Ok(MoeEngine {
            moe,
            vocab,
            device,
            vocoder: vocoder.as_ref().to_path_buf(),
            verbose,
        })
    }

    fn phonemizer_kind(&self, project: &UProject, track_no: i32) -> Result<PhonemizerKind, String> {
        let track = project
            .tracks
            .get(track_no as usize)
            .ok_or_else(|| format!("track {track_no} not found"))?;
        // Universal (IPA-first) default, like DsEngine — not the legacy
        // English/Japanese hard-match the WorldlineEngine uses.
        Ok(PhonemizerKind::from_track(track))
    }
}

/// Per-frame features: the ±2 phoneme context ids, f0/pos and group per
/// frame. Mirrors `moe-train`'s `build_batch` but driven by a
/// [`RenderInput`] (app phrase) instead of a [`TrainSet`].
pub(crate) struct FrameFeats {
    /// WINDOW ids per frame ([frames, WINDOW] flattened).
    pub(crate) ids: Vec<i64>,
    /// `[f0_norm, pos_frac]` per frame ([frames, 2] flattened).
    pub(crate) feats: Vec<f32>,
    /// group id per frame (routing key).
    pub(crate) groups: Vec<usize>,
    /// f0 denormalized (Hz) per frame — passed to the vocoder.
    pub(crate) f0_hz: Vec<f32>,
    /// Universal phonemes that are absent from the checkpoint vocabulary.
    /// These currently fall back to the pad embedding, so expose them in
    /// verbose mode instead of making the mismatch invisible.
    pub(crate) missing_symbols: Vec<String>,
}

/// Build per-frame features (ids/feats/groups/f0) from a `RenderInput`.
/// Frame grid: `ceil((duration + leading) / frame_ms)` with `frame_ms`
/// matched to the train-prep distillation (11.61 ms, the v2 fork frame).
pub(crate) fn build_frame_features(
    input: &RenderInput,
    vocab: &std::collections::HashMap<String, usize>,
) -> FrameFeats {
    use worldline_plugin::convert::sample_phrase_curves;
    let frame_ms = 11.61;
    let curves = sample_phrase_curves(input, frame_ms);
    let n = curves.f0.len();
    let pad = vocab.len().min(EMBED_SLOTS - 1) as i64;
    let mut ids = Vec::with_capacity(n * WINDOW);
    let mut feats = Vec::with_capacity(n * 2);
    let mut groups = Vec::with_capacity(n);
    let mut f0_hz = Vec::with_capacity(n);
    let mut missing_symbols = Vec::new();

    for f in 0..n {
        let ms = input.phrase.position_ms - input.phrase.leading_ms + f as f64 * frame_ms;
        // Which phoneme does this frame fall in?
        let pidx = locate_phoneme_index(input, ms);
        let (group, pos_ms, dur_ms) = match pidx {
            Some(i) => {
                let ph = &input.phonemes[i];
                (group_of(&ph.phoneme), ph.position_ms, ph.duration_ms)
            }
            None => (4 /* other */, 0.0, 0.0),
        };
        // Context window: the frame's phoneme ±2 (padded at edges), like
        // moe-train's `build_batch`.
        for k in 0..WINDOW {
            let di = k as i64 - HALF as i64;
            let pi = pidx.map(|i| i as i64 + di).unwrap_or(-1);
            let ctx_id = if pi < 0 || pi >= input.phonemes.len() as i64 {
                pad
            } else {
                let ph = &input.phonemes[pi as usize];
                match vocab.get(&ph.phoneme).copied() {
                    Some(id) => id.min(EMBED_SLOTS - 1) as i64,
                    None => {
                        if !missing_symbols.contains(&ph.phoneme) {
                            missing_symbols.push(ph.phoneme.clone());
                        }
                        pad
                    }
                }
            };
            ids.push(ctx_id);
        }
        groups.push(group);
        let pos_frac = if dur_ms > 0.0 {
            ((ms - pos_ms) / dur_ms).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        // f0_norm denormalizes to Hz for the vocoder.
        let f0_norm = if curves.f0[f] > 0.0 {
            (curves.f0[f] / 500.0).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        feats.push(f0_norm);
        feats.push(pos_frac);
        f0_hz.push(curves.f0[f] as f32);
    }
    FrameFeats {
        ids,
        feats,
        groups,
        f0_hz,
        missing_symbols,
    }
}

/// Index into `input.phonemes` of the phoneme containing `ms`, else None.
fn locate_phoneme_index(input: &RenderInput, ms: f64) -> Option<usize> {
    for (i, ph) in input.phonemes.iter().enumerate() {
        let start = ph.position_ms;
        let end = ph.position_ms + ph.duration_ms;
        if ms >= start && ms < end {
            return Some(i);
        }
    }
    None
}

/// Load `{prefix}-phonemes.txt` (one symbol per line, index = id).
pub(crate) fn load_vocab_sidecar(
    prefix: &Path,
) -> Result<std::collections::HashMap<String, usize>, String> {
    let path = format!("{}-phonemes.txt", prefix.to_string_lossy());
    let text = std::fs::read_to_string(&path).map_err(|e| format!("read {path}: {e}"))?;
    let mut map = std::collections::HashMap::new();
    for (i, line) in text.lines().enumerate() {
        let sym = line.trim();
        if !sym.is_empty() {
            map.insert(sym.to_string(), i);
        }
    }
    Ok(map)
}

impl pipeline::PhraseRenderer for MoeEngine {
    fn render_phrase(&self, input: &RenderInput) -> Result<Vec<f32>, String> {
        use burn::tensor::{Int, Tensor, TensorData};
        let ff = build_frame_features(input, &self.vocab);
        if self.verbose && !ff.missing_symbols.is_empty() {
            eprintln!(
                "moe: {} input symbol(s) absent from checkpoint vocab; mapped to PAD: {:?}",
                ff.missing_symbols.len(),
                ff.missing_symbols
            );
        }
        let n = ff.ids.len() / WINDOW;
        if n == 0 {
            return Ok(Vec::new());
        }
        let ids = Tensor::<MoeBackend, 2, Int>::from_data(
            TensorData::new(ff.ids, vec![n, WINDOW]),
            &self.device,
        );
        let feats =
            Tensor::<MoeBackend, 2>::from_data(TensorData::new(ff.feats, vec![n, 2]), &self.device);
        // General expert on all frames.
        let general_mel = self.moe.general.forward(ids.clone(), feats.clone());
        // Routed group experts: one forward per frame whose group has an
        // expert (missing expert → zeros, the general covers them).
        let mut add: Option<Tensor<MoeBackend, 2>> = None;
        for pos in 0..n {
            if let Some(exp) = &self.moe.experts[ff.groups[pos]] {
                let one_ids = ids.clone().narrow(0, pos, 1);
                let one_feats = feats.clone().narrow(0, pos, 1);
                let e_mel = exp.forward(one_ids, one_feats);
                add = Some(match add {
                    None => {
                        let mut z = Tensor::<MoeBackend, 2>::zeros([n, 128], &self.device);
                        z = z.slice_assign([pos..pos + 1, 0..128], e_mel);
                        z
                    }
                    Some(acc) => acc.slice_assign([pos..pos + 1, 0..128], e_mel),
                });
            }
        }
        let pred = match add {
            Some(a) => general_mel + a,
            None => general_mel,
        };
        let data = pred.into_data();
        let mel: Vec<f32> = data.convert::<f32>().to_vec::<f32>().unwrap_or_default();
        let mel2: Vec<[f32; 128]> = mel
            .chunks(128)
            .map(|c| {
                let mut r = [0f32; 128];
                r.copy_from_slice(c);
                r
            })
            .collect();
        run_vocoder_once(&self.vocoder.to_string_lossy(), &mel2, &ff.f0_hz)
    }
    fn curve_frame_ms(&self) -> f64 {
        11.61
    }

    fn cache_salt(&self) -> u64 {
        // "moe" — distinct from r1/r2/classic/diffsinger caches.
        0x6d6f65_0000_0005
    }

    fn parallel_safe(&self) -> bool {
        false
    }
}

impl Engine for MoeEngine {
    fn name(&self) -> &'static str {
        "moe"
    }

    fn capabilities(&self) -> &dyn EngineCapabilities {
        WorldlineCapabilities::get()
    }

    fn render_project(
        &mut self,
        project: &UProject,
        voicebank: &Voicebank,
        track_no: i32,
    ) -> Result<RenderReport, String> {
        let kind = self.phonemizer_kind(project, track_no)?;
        pipeline::render_project(
            project,
            voicebank,
            self,
            track_no,
            kind,
            self.verbose,
            &mut None,
            None,
            None,
        )
    }

    fn synth_note(
        &self,
        voicebank: &Voicebank,
        alias: &str,
        tone: i32,
        duration_ms: f64,
    ) -> Result<Vec<f32>, String> {
        pipeline::synth_note(voicebank, self, alias, tone, duration_ms)
    }

    fn set_cache_dir(&mut self, _dir: &Path, _max_bytes: usize) -> Result<(), String> {
        Ok(()) // no res-{hash} cache yet (same as the neural path)
    }
}
