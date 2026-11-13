//! Single-singer residual-diffusion engine: frozen base mel + DDIM detail
//! correction → pc-nsf-hifigan → PCM.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use burn::tensor::{Int, Tensor, TensorData};
use domain::capabilities::EngineCapabilities;
use domain::UProject;
use feed::render_input::RenderInput;
use moe_train::baseline::{ContextAcoustic, WINDOW};
use moe_train::diffusion::ResidualDenoiser;
use voicebank::Voicebank;
use worldline_plugin::WorldlineCapabilities;

use crate::engine::Engine;
use crate::moe::{build_frame_features, load_vocab_sidecar};
use crate::pipeline::{self, PhonemizerKind, RenderReport};

type DiffusionBackend = burn::backend::Wgpu;

pub struct DiffusionEngine {
    base: ContextAcoustic<DiffusionBackend>,
    denoiser: ResidualDenoiser<DiffusionBackend>,
    vocab: std::collections::HashMap<String, usize>,
    device: burn::backend::wgpu::WgpuDevice,
    vocoder: PathBuf,
    sample_steps: usize,
    verbose: bool,
    cache_salt: u64,
}

impl DiffusionEngine {
    pub fn open(
        base_prefix: impl AsRef<Path>,
        diffusion_prefix: impl AsRef<Path>,
        vocoder: impl AsRef<Path>,
        base_hidden: usize,
        base_depth: usize,
        sample_steps: usize,
        verbose: bool,
    ) -> Result<Self, String> {
        let base_prefix = base_prefix.as_ref();
        let diffusion_prefix = diffusion_prefix.as_ref();
        let device = burn::backend::wgpu::WgpuDevice::DefaultDevice;
        let mut base = moe_train::baseline::load_model::<DiffusionBackend>(
            0,
            &base_prefix.to_string_lossy(),
            base_hidden,
            base_depth,
            &device,
        )?;
        base.disable_dropout();
        let mut denoiser = moe_train::diffusion::load_model::<DiffusionBackend>(
            &diffusion_prefix.to_string_lossy(),
            moe_train::diffusion::DEFAULT_HIDDEN,
            moe_train::diffusion::DEFAULT_DEPTH,
            &device,
        )?;
        denoiser.disable_dropout();
        let vocab = load_vocab_sidecar(diffusion_prefix)?;
        // Include the checkpoint identities in phrase cache keys so replacing
        // either model at a stable prefix cannot serve stale audio.
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        checkpoint_identity(base_prefix).hash(&mut hasher);
        checkpoint_identity(diffusion_prefix).hash(&mut hasher);
        let cache_salt = 0x6469_6666_0000_0001 ^ hasher.finish();
        Ok(Self {
            base,
            denoiser,
            vocab,
            device,
            vocoder: vocoder.as_ref().to_path_buf(),
            sample_steps: sample_steps.clamp(2, moe_train::diffusion::DIFFUSION_STEPS),
            verbose,
            cache_salt,
        })
    }

    fn phonemizer_kind(&self, project: &UProject, track_no: i32) -> Result<PhonemizerKind, String> {
        let track = project
            .tracks
            .get(track_no as usize)
            .ok_or_else(|| format!("track {track_no} not found"))?;
        Ok(PhonemizerKind::from_track(track))
    }
}

impl pipeline::PhraseRenderer for DiffusionEngine {
    fn render_phrase(&self, input: &RenderInput) -> Result<Vec<f32>, String> {
        let ff = build_frame_features(input, &self.vocab);
        if self.verbose && !ff.missing_symbols.is_empty() {
            eprintln!(
                "diffusion: {} input symbol(s) absent from checkpoint vocab; mapped to PAD: {:?}",
                ff.missing_symbols.len(),
                ff.missing_symbols
            );
        }
        let n = ff.ids.len() / WINDOW;
        if n == 0 {
            return Ok(Vec::new());
        }
        let ids = Tensor::<DiffusionBackend, 2, Int>::from_data(
            TensorData::new(ff.ids.clone(), vec![n, WINDOW]),
            &self.device,
        );
        let feats = Tensor::<DiffusionBackend, 2>::from_data(
            TensorData::new(ff.feats.clone(), vec![n, 2]),
            &self.device,
        );
        let base_values: Vec<f32> = self
            .base
            .forward(ids, feats)
            .into_data()
            .convert::<f32>()
            .to_vec::<f32>()
            .map_err(|e| format!("diffusion base mel conversion: {e}"))?;
        if base_values.len() != n * 128 {
            return Err("diffusion base mel shape mismatch".into());
        }
        let base_mel: Vec<[f32; 128]> = base_values
            .chunks_exact(128)
            .map(|row| {
                let mut mel = [0.0; 128];
                mel.copy_from_slice(row);
                mel
            })
            .collect();
        let residual = moe_train::diffusion::sample_residual_values(
            &self.denoiser,
            &ff.ids,
            &ff.feats,
            &base_mel,
            self.sample_steps,
            0xD1FF_0510_2026u64,
            &self.device,
        )?;
        let corrected: Vec<[f32; 128]> = base_mel
            .iter()
            .zip(residual)
            .map(|(b, r)| {
                let mut mel = [0.0; 128];
                for i in 0..128 {
                    mel[i] = b[i] + r[i];
                }
                mel
            })
            .collect();
        moe_train::render::run_vocoder_once(&self.vocoder.to_string_lossy(), &corrected, &ff.f0_hz)
    }
    fn curve_frame_ms(&self) -> f64 {
        11.61
    }

    fn cache_salt(&self) -> u64 {
        self.cache_salt
    }
    fn parallel_safe(&self) -> bool {
        false
    }
}

fn checkpoint_identity(prefix: &Path) -> (PathBuf, u64, u128) {
    let mut file = prefix.to_path_buf();
    file.set_extension("mpk");
    let metadata = std::fs::metadata(&file).ok();
    let len = metadata.as_ref().map(|m| m.len()).unwrap_or_default();
    let modified = metadata
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    (file, len, modified)
}

impl Engine for DiffusionEngine {
    fn name(&self) -> &'static str {
        "single-singer-diffusion"
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
        Ok(())
    }
}
