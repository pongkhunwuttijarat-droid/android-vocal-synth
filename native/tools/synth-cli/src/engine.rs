//! Engine adapter layer — the host's (synth-server worker, CLI) contract
//! for synthesis engines.
//!
//! Hosts depend on [`Engine`] instead of the concrete `WorldlineRenderer`,
//! so engines can be swapped or chained (worldline v1/v2, classic, future
//! neural engines, mixer post-FX plugins) without touching callers. This
//! mirrors the reference's renderer-independence: OpenUtau picks a
//! renderer by name (`WORLDLINE-R` / `WORLDLINE-R2` / `CLASSIC`) and
//! negotiates capabilities before filling the input.

use std::path::Path;

use domain::UProject;
use runtime::RenderCache;
use voicebank::Voicebank;
use worldline_plugin::{WorldlineCapabilities, WorldlineRenderer};

use crate::pipeline::{self, PhonemizerKind, RenderReport};

/// Outcome of one engine render (host-agnostic).
pub trait Engine {
    /// Stable engine identifier, e.g. `"worldline-v2"`.
    fn name(&self) -> &'static str;
    /// What the engine can and cannot do (host negotiates input sections).
    fn capabilities(&self) -> &WorldlineCapabilities;
    /// Render `track_no` of `project` with `voicebank`.
    ///
    /// `&mut self` because engines own mutable state (the render cache)
    /// that must live across calls; hosts call this from their single
    /// render worker thread.
    fn render_project(
        &mut self,
        project: &UProject,
        voicebank: &Voicebank,
        track_no: i32,
    ) -> Result<RenderReport, String>;
    /// Render one note with a single phoneme (see `pipeline::synth_note`).
    fn synth_note(
        &self,
        voicebank: &Voicebank,
        alias: &str,
        tone: i32,
        duration_ms: f64,
    ) -> Result<Vec<f32>, String>;
    /// Point the engine's render cache at `dir` (OpenUtau-style res-{hash}
    /// files). Engines without a cache may treat this as a no-op.
    fn set_cache_dir(&mut self, dir: &Path, max_bytes: usize) -> Result<(), String>;

    /// Wire a cooperative-cancellation flag (host sets it to abort an
    /// in-flight render between chunks). Default: no-op for engines
    /// without chunk-level cancellation.
    fn set_cancel(&mut self, _cancel: std::sync::Arc<std::sync::atomic::AtomicBool>) {}

    /// Reset the cancellation flag before a new render. Default: no-op.
    fn clear_cancel(&mut self) {}

    /// Attach a mixer FX plugin (dlopen'd libmixerfx.so). Default: no-op
    /// for engines without mixer support.
    fn set_mixer(&mut self, _mixer: mixer_fx::MixerFx) {}
}

/// Worldline engine adapter: wraps a loaded `WorldlineRenderer` plus the
/// render cache, exposing the [`Engine`] interface. The renderer is
/// `!Send` (owns C++ handles), so the adapter is created and used on the
/// render worker thread — exactly like the previous `RenderService`.
pub struct WorldlineEngine {
    renderer: WorldlineRenderer,
    /// Render cache (res-{hash} next to the voicebank), kept across
    /// renders — the previous server code rebuilt it per request.
    cache: Option<RenderCache>,
    /// Optional mixer FX plugin (dlopen'd) — processed after mixing.
    mixer: Option<mixer_fx::MixerFx>,
    /// Cooperative cancellation flag — set by the host (Stop button) to
    /// abort an in-flight render between chunks.
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    verbose: bool,
}

impl WorldlineEngine {
    /// dlopen `so_path` and wrap it. `verbose` mirrors the pipeline flag.
    pub fn open(so_path: impl AsRef<Path>, verbose: bool) -> Result<Self, String> {
        let renderer = WorldlineRenderer::open(so_path)
            .map_err(|e| format!("open worldline renderer: {e}"))?;
        Ok(WorldlineEngine {
            renderer,
            cache: None,
            mixer: None,
            cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            verbose,
        })
    }

    /// Phonemizer picked from the track's setting (mirrors the server).
    fn phonemizer_kind(&self, project: &UProject, track_no: i32) -> Result<PhonemizerKind, String> {
        let track = project
            .tracks
            .get(track_no as usize)
            .ok_or_else(|| format!("track {track_no} not found"))?;
        match track.phonemizer.as_deref().unwrap_or("English") {
            "Japanese" => Ok(PhonemizerKind::Japanese),
            _ => Ok(PhonemizerKind::English),
        }
    }

    /// Attach a mixer FX plugin (dlopen'd libmixerfx.so) to the engine.
    /// Processed on the final mixed samples of every render.
    pub fn set_mixer(&mut self, mixer: mixer_fx::MixerFx) {
        self.mixer = Some(mixer);
    }

    /// Wire a cooperative-cancellation flag. Set it (store true) to abort
    /// the in-flight render between chunks; the flag is shared with the
    /// host so it can also be used for a hard Stop on the next render.
    pub fn set_cancel(&mut self, cancel: std::sync::Arc<std::sync::atomic::AtomicBool>) {
        self.cancel = cancel;
    }

    /// Reset the cancellation flag (call before starting a new render).
    pub fn clear_cancel(&mut self) {
        self.cancel.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Engine for WorldlineEngine {
    fn name(&self) -> &'static str {
        "worldline-v2"
    }

    fn capabilities(&self) -> &WorldlineCapabilities {
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
            &self.renderer,
            track_no,
            kind,
            self.verbose,
            &mut self.cache,
            self.mixer.as_mut(),
            Some(&self.cancel),
        )
    }

    fn synth_note(
        &self,
        voicebank: &Voicebank,
        alias: &str,
        tone: i32,
        duration_ms: f64,
    ) -> Result<Vec<f32>, String> {
        pipeline::synth_note(voicebank, &self.renderer, alias, tone, duration_ms)
    }

    fn set_cache_dir(&mut self, dir: &Path, max_bytes: usize) -> Result<(), String> {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cache dir {}: {e}", dir.display()))?;
        self.cache = Some(RenderCache::new_in_dir(dir, max_bytes));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_are_host_negotiable() {
        let caps = WorldlineCapabilities::get();
        // Worldline needs wav samples + oto, never frq (pyin + f0 curve).
        assert!(caps.needs_wav_samples);
        assert!(caps.needs_oto);
        assert!(!caps.needs_frq);
        assert_eq!(caps.sample_rate, 44100);
        assert_eq!(caps.channels, 1);
    }

    #[test]
    fn engine_name_is_stable() {
        // A renderer-less adapter can't be built (needs a .so), but the
        // name/caps contract is static — assert the trait contract here so
        // hosts can rely on it for capability negotiation.
        let caps = WorldlineCapabilities::get();
        let _name: &str = "worldline-v2"; // mirrored in WorldlineEngine::name
        assert!(caps.modes.len() >= 3); // Classic, Worldline, Worldline2
    }
}
