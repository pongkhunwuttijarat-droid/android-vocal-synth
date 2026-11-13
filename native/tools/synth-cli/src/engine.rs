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
use std::sync::Mutex;

use domain::capabilities::EngineCapabilities;
use domain::UProject;
#[cfg(feature = "ffi")]
use runtime::RenderCache;
use voicebank::Voicebank;
use worldline_plugin::WorldlineCapabilities;
#[cfg(feature = "ffi")]
use worldline_plugin::WorldlineRenderer;

use crate::pipeline::{self, PhonemizerKind, RenderReport};

/// Outcome of one engine render (host-agnostic).
pub trait Engine {
    /// Stable engine identifier, e.g. `"worldline-v2"`.
    fn name(&self) -> &'static str;
    /// What the engine can and cannot do (host negotiates input
    /// sections). Engine-agnostic: engines expose their capabilities
    /// through the domain trait — the host never depends on the
    /// concrete engine's capability type.
    fn capabilities(&self) -> &dyn EngineCapabilities;
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
    #[cfg(feature = "ffi")]
    fn set_mixer(&mut self, _mixer: mixer_fx::MixerFx) {}
}

/// Worldline engine adapter: wraps a loaded `WorldlineRenderer` plus the
/// render cache, exposing the [`Engine`] interface. The renderer is
/// `!Send` (owns C++ handles), so the adapter is created and used on the
/// render worker thread — exactly like the previous `RenderService`.
///
/// Native-only (needs the dlopen'd `.so`); the wasm build uses
/// [`WorldlineRsEngine`].
#[cfg(feature = "ffi")]
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

#[cfg(feature = "ffi")]
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
        self.cancel
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

/// R2 (WORLDLINE-R2) neural engine adapter: feature extraction + ONNX
/// mel → pc-nsf-hifigan vocoder. Selectable independently of the v1
/// WORLD engine ("ให้เลือกได้").
#[cfg(feature = "r2")]
pub struct WorldlineR2Engine {
    engine: worldline_plugin::r2_engine::R2Engine,
    verbose: bool,
}

#[cfg(feature = "r2")]
impl WorldlineR2Engine {
    /// `mel_path` / `vocoder_path` are the ONNX models (host bundles them);
    /// `worldline_so` is the same libworldline.so the v1 engine uses (its
    /// F0 / WORLD analysis functions drive the feature extraction).
    pub fn open(
        worldline_so: impl AsRef<Path>,
        mel_path: impl AsRef<Path>,
        vocoder_path: impl AsRef<Path>,
        verbose: bool,
    ) -> Result<Self, String> {
        // Deterministic ort: 1 intra-op thread per session so concurrent
        // chunk renders can't shift the float reductions (see DsEngine).
        let _ = ort::init()
            .with_global_thread_pool(
                ort::environment::GlobalThreadPoolOptions::default()
                    .with_intra_threads(1)
                    .expect("ort thread config"),
            )
            .commit();
        let engine = worldline_plugin::r2_engine::R2Engine::new(
            worldline_plugin::r2_engine::R2Models {
                mel_path: mel_path.as_ref().to_path_buf(),
                vocoder_path: vocoder_path.as_ref().to_path_buf(),
            },
            worldline_so.as_ref(),
        )
        .map_err(|e| format!("open r2 engine: {e}"))?;
        Ok(WorldlineR2Engine { engine, verbose })
    }

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
}

#[cfg(feature = "r2")]
impl pipeline::PhraseRenderer for WorldlineR2Engine {
    fn render_phrase(&self, input: &feed::render_input::RenderInput) -> Result<Vec<f32>, String> {
        self.engine
            .render_phrase(input, 44100, 2048, 512, 44100)
            .map_err(|e| e.to_string())
    }
    fn cache_salt(&self) -> u64 {
        // "worldline-r2" — distinct from the r1 WORLD engine so the
        // neural path never reuses WORLD-rendered chunks.
        0x726c32_0000_0002
    }
    fn curve_frame_ms(&self) -> f64 {
        11.6
    }
    fn parallel_safe(&self) -> bool {
        true
    }
}

#[cfg(feature = "r2")]
impl Engine for WorldlineR2Engine {
    fn name(&self) -> &'static str {
        "worldline-r2"
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
        Ok(()) // neural path has no res-{hash} cache yet
    }
}

#[cfg(feature = "ffi")]
impl Engine for WorldlineEngine {
    fn name(&self) -> &'static str {
        // This is the WORLD-resynthesis path (PhraseSynth.Synth()) —
        // the reference calls that "WORLDLINE-R" (v1). The neural R2
        // path is a separate engine ("worldline-r2").
        "worldline-r"
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
            &self.renderer,
            track_no,
            kind,
            self.verbose,
            &mut self.cache,
            self.mixer
                .as_mut()
                .map(|m| m as &mut dyn pipeline::PostMixFx),
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
        std::fs::create_dir_all(dir).map_err(|e| format!("cache dir {}: {e}", dir.display()))?;
        self.cache = Some(RenderCache::new_in_dir(dir, max_bytes));
        Ok(())
    }
}

/// `WORLDLINE-R` with the pure-Rust kernel (`worldline-rs`) instead of the
/// dlopen'd `libworldline.so`.
///
/// Same request conversion and curve sampling as [`WorldlineEngine`]; the
/// WORLD analysis + resynthesis run in `world-rs` (no `.so` at all). Wired
/// as a selectable engine — the C++ `.so` path stays the default until the
/// comparison clears it.
pub struct WorldlineRsEngine {
    frame_ms: f64,
    verbose: bool,
    prepared_cache: Mutex<worldline_rs::PreparedModelCache>,
}

impl WorldlineRsEngine {
    /// No `.so` is needed; `verbose` mirrors the pipeline flag.
    pub fn open(verbose: bool) -> Result<Self, String> {
        Ok(WorldlineRsEngine {
            frame_ms: worldline_plugin::DEFAULT_FRAME_MS,
            verbose,
            prepared_cache: Mutex::new(worldline_rs::PreparedModelCache::new(64 * 1024 * 1024)),
        })
    }

    pub fn clear_caches(&self) {
        if let Ok(mut guard) = self.prepared_cache.lock() {
            *guard = worldline_rs::PreparedModelCache::new(64 * 1024 * 1024);
        }
    }

    pub fn prepared_cache_len(&self) -> usize {
        self.prepared_cache.lock().map(|g| g.len()).unwrap_or(0)
    }

    pub fn render_phrase_stream_with_stats(
        &self,
        input: &feed::render_input::RenderInput,
        block_samples: usize,
        callback: impl FnMut(&[f32]) -> Result<(), String>,
    ) -> Result<(usize, worldline_rs::StreamStatsRs), String> {
        worldline_rs::render_phrase_stream_with_source_and_cache(
            input,
            self.frame_ms(),
            worldline_rs::model_frame_ms(),
            &worldline_rs::FileSource,
            Some(&self.prepared_cache),
            block_samples,
            callback,
        )
    }

    /// Frame size the Rust phrase path uses (v2 default, overridable by
    /// `WORLD_FRAME_MS` exactly like the `.so` renderer).
    pub fn frame_ms(&self) -> f64 {
        std::env::var_os("WORLD_FRAME_MS")
            .and_then(|v| v.to_str().and_then(|s| s.parse::<f64>().ok()))
            .unwrap_or(self.frame_ms)
    }

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
}

impl pipeline::PhraseRenderer for WorldlineRsEngine {
    fn render_phrase(&self, input: &feed::render_input::RenderInput) -> Result<Vec<f32>, String> {
        worldline_rs::render_phrase_with_source_and_shared_cache(
            input,
            self.curve_frame_ms(),
            worldline_rs::model_frame_ms(),
            &worldline_rs::FileSource,
            Some(&self.prepared_cache),
        )
    }

    fn render_phrase_stream(
        &self,
        input: &feed::render_input::RenderInput,
        block_samples: usize,
        callback: &mut dyn FnMut(&[f32]) -> Result<(), String>,
    ) -> Result<usize, String> {
        self.render_phrase_stream_with_stats(input, block_samples, callback)
            .map(|(samples, _)| samples)
    }
    fn cache_salt(&self) -> u64 {
        // "worldline-r-rs" — never share chunk caches with the .so r1 path.
        0x726c31_7273_0004 ^ self.frame_ms().to_bits()
    }
    fn curve_frame_ms(&self) -> f64 {
        self.frame_ms()
    }
    // Pure Rust, no shared C++ randn state: verified deterministic under
    // concurrent chunk renders (same class as the .so r1 path).
    fn parallel_safe(&self) -> bool {
        true
    }
}

impl Engine for WorldlineRsEngine {
    fn name(&self) -> &'static str {
        "worldline-r-rs"
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

/// Classic (WORLDLINE-R Classic) engine: the SAME feature extraction the
/// R2 neural path uses (`SynthFeatures` port — segments → blended
/// f0/sp_env/ap), but synthesized with the C++ `WorldSynthesis` call
/// instead of mel.onnx → pc-nsf-hifigan. This is the third reference
/// path — C# v1 `PhraseSynthV2.Synth()` (SynthFeatures → WorldSynthesis,
/// no neural vocoder). Selected as `"classic"` alongside `"r1"`/`"r2"`.
#[cfg(feature = "r2")]
pub struct WorldlineClassicEngine {
    /// dlopen'd WORLD analysis (F0 / analysis / WorldSynthesis) — the
    /// same libworldline.so the r1/r2 engines use.
    analysis: worldline_plugin::Analysis,
    verbose: bool,
}

#[cfg(feature = "r2")]
impl WorldlineClassicEngine {
    /// dlopen `so_path` — the same libworldline.so the r1/r2 engines use
    /// (its F0 / WORLD analysis + WorldSynthesis drive this path).
    pub fn open(so_path: impl AsRef<Path>, verbose: bool) -> Result<Self, String> {
        let analysis = worldline_plugin::analysis(so_path.as_ref())
            .map_err(|e| format!("open classic engine: {e}"))?;
        Ok(WorldlineClassicEngine { analysis, verbose })
    }

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
}

#[cfg(feature = "r2")]
impl pipeline::PhraseRenderer for WorldlineClassicEngine {
    fn render_phrase(&self, input: &feed::render_input::RenderInput) -> Result<Vec<f32>, String> {
        // 1. Per-phoneme feature extraction — the SAME port r2 uses.
        let requests = worldline_plugin::build_requests(input)
            .map_err(|e| format!("classic build_requests: {e}"))?;
        if requests.is_empty() {
            return Ok(Vec::new());
        }
        let mut segments = Vec::with_capacity(requests.len());
        for request in &requests {
            let wav = voicebank::read_wav(std::path::Path::new(&request.wav_path))
                .map_err(|e| format!("classic read {}: {e}", request.wav_path))?;
            let samples: Vec<f32> = wav.to_mono().into_iter().map(|s| s as f32).collect();
            let seg = worldline_plugin::r2::analyze_segment(
                &self.analysis,
                request,
                &samples,
                44100,
                512,
                2048,
            )
            .map_err(|e| format!("classic analyze_segment: {e}"))?;
            segments.push(seg);
        }
        let mut features = worldline_plugin::r2::synth_features(&segments, 2048);

        // 2. f0 curve override — C# SynthFeatures: `if (f0Out[i] >
        // config.f0_floor) f0Out[i] = f0Curve[i]` (floor 64.69 @44.1k/2048).
        let mut f0 = features.f0.clone();
        let f0_floor = 3.0 * 44100.0 / (2048.0 - 3.0);
        let mut curve = worldline_plugin::sample_phrase_curves(input, 11.6);
        for (i, f) in f0.iter_mut().enumerate() {
            if *f > f0_floor {
                if let Some(c) = curve.f0.get(i) {
                    if *c > 0.0 {
                        *f = *c;
                    }
                }
            }
        }

        // 3. C++ WorldSynthesis (C# PhraseSynthV2.Synth): f0/sp/ap +
        // per-frame gender/tension/breathiness/voicing curves → PCM.
        let total = f0.len();
        let sp_size = features.sp_size;
        let mut y: *mut f64 = std::ptr::null_mut();
        let n = unsafe {
            self.analysis.world_synthesis(
                &mut f0,
                &mut features.sp_env,
                sp_size as i32,
                &mut features.ap,
                2048,
                11.61,
                44100,
                &mut y,
                &mut curve.gender,
                &mut curve.tension,
                &mut curve.breathiness,
                &mut curve.voicing,
            )
        };
        if n <= 0 || y.is_null() {
            return Err(format!("classic WorldSynthesis returned {n} samples"));
        }
        let out: Vec<f32> = unsafe { std::slice::from_raw_parts(y, n as usize) }
            .iter()
            .map(|&s| s as f32)
            .collect();
        unsafe { worldline_plugin::libc_free(y as *mut std::ffi::c_void) };
        Ok(out)
    }
    fn curve_frame_ms(&self) -> f64 {
        11.6
    }

    fn cache_salt(&self) -> u64 {
        // "worldline-classic" — never reuse r1/r2 chunk caches.
        0x726c33_0000_0003
    }
    // The classic path is a pure C++ WorldSynthesis call (no randn) —
    // verified byte-identical under concurrent chunk renders.
    fn parallel_safe(&self) -> bool {
        true
    }
}

#[cfg(feature = "r2")]
impl Engine for WorldlineClassicEngine {
    fn name(&self) -> &'static str {
        "worldline-classic"
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

/// DiffSinger (MS3.3) engine: diffusion acoustic model + vocoder. The
/// model dir holds `dsconfig.yaml` + phonemes table + acoustic.onnx +
/// vocoder dir (the C# DiffSinger voicebank layout). Phonemization reuses
/// the pipeline's Universal phonemizer — the model's phoneme table must
/// contain the produced symbols (the POC uses the same bank's aliases).
#[cfg(feature = "r2")]
pub struct DsEngine {
    model: diffsinger::DsModel,
    verbose: bool,
}

#[cfg(feature = "r2")]
impl DsEngine {
    pub fn open(model_dir: impl AsRef<Path>, verbose: bool) -> Result<Self, String> {
        // Single-threaded ort: the intra-op thread-pool reductions are
        // float-schedule sensitive — concurrent chunk renders would
        // shift the output (golden-break). 1 thread per session makes
        // concurrent renders deterministic; the parallel fan-out replaces
        // the parallelism (the first `commit` wins — all engines agree).
        let _ = ort::init()
            .with_global_thread_pool(
                ort::environment::GlobalThreadPoolOptions::default()
                    .with_intra_threads(1)
                    .expect("ort thread config"),
            )
            .commit();
        let model = diffsinger::DsModel::load(model_dir.as_ref())
            .map_err(|e| format!("open diffsinger model: {e}"))?;
        Ok(DsEngine { model, verbose })
    }

    fn phonemizer_kind(&self, project: &UProject, track_no: i32) -> Result<PhonemizerKind, String> {
        let track = project
            .tracks
            .get(track_no as usize)
            .ok_or_else(|| format!("track {track_no} not found"))?;
        Ok(PhonemizerKind::from_track(track))
    }
}

#[cfg(feature = "r2")]
impl pipeline::PhraseRenderer for DsEngine {
    fn render_phrase(&self, input: &feed::render_input::RenderInput) -> Result<Vec<f32>, String> {
        let frame_ms = self.model.config.frame_ms();
        // The C# DiffSinger phrase's leading = the first phoneme's
        // preutter — the fixed 8-frame DS constant. The pipeline's
        // worldline-style preutter differs (e.g. 133ms vs 92.9ms), which
        // shifts the f0 grid and the dsdur's onset stretch — patch it
        // before building the tensors.
        let mut patched = input.clone();
        patched.phrase.leading_ms = diffsinger::utils::HEAD_FRAMES as f64 * frame_ms;
        patched.phrase.leading_ticks =
            (patched.phrase.leading_ms / (60000.0 / (120.0 * 480.0))) as i32;
        let input = &patched;
        let frame_ms = frame_ms;

        // OpenUtau-identical durations: when the bank ships a dsdur, the
        // duration predictor's per-token output (stretched onto the note
        // bounds) replaces the pipeline's note-timed durations — exactly
        // what the C# backend feeds the acoustic model.
        let override_durs = match &self.model.dsdur {
            Some(predictor) => {
                let tokens = diffsinger::tensors::build_tokens(
                    input,
                    &self.model.phoneme_table,
                    &self.model.dict,
                )?;
                // ph_midi per token: the SP tone + each phoneme's tone
                // repeated per dict/letter-split token (C# SelectMany).
                let counts = diffsinger::tensors::token_counts(
                    input,
                    &self.model.phoneme_table,
                    &self.model.dict,
                )?;
                let first_tone = input.phonemes.first().map(|p| p.tone as i64).unwrap_or(60);
                let mut tones = vec![first_tone];
                for (ph, &n) in input.phonemes.iter().zip(counts.iter()) {
                    tones.extend(std::iter::repeat(ph.tone as i64).take(n));
                }
                Some(predictor.predict(
                    input,
                    &tokens,
                    &tones,
                    &counts,
                    &self.model.phoneme_table,
                    self.model.dict.vowels(),
                    frame_ms,
                )?)
            }
            None => None,
        };

        let ds_input = diffsinger::tensors::build_acoustic_inputs(
            input,
            &self.model.phoneme_table,
            frame_ms,
            &self.model.dict,
            override_durs.as_deref(),
        )?;
        if std::env::var("DIFFSINGER_DEBUG_DUR").is_ok() {
            // Append to a fixed path (the CLI pipes stdout) — the per-token
            // durations for the golden alignment debug.
            use std::io::Write;
            let line = format!(
                "tokens={} durs_frames={:?} ms_per_token={:?}\n",
                ds_input.tokens.len(),
                ds_input.durations,
                override_durs
            );
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open("/tmp/ds-durations.log")
            {
                let _ = f.write_all(line.as_bytes());
            }
        }
        self.model.render(&ds_input)
    }
    fn curve_frame_ms(&self) -> f64 {
        self.model.config.frame_ms()
    }
    fn parallel_safe(&self) -> bool {
        true
    }
}

#[cfg(feature = "r2")]
impl Engine for DsEngine {
    fn name(&self) -> &'static str {
        "diffsinger"
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_are_host_negotiable() {
        // Hosts negotiate through the ENGINE-AGNOSTIC trait now.
        use domain::capabilities::EngineCapabilities as _;
        let caps: &dyn EngineCapabilities = WorldlineCapabilities::get();
        // Worldline needs wav samples + oto, never frq (pyin + f0 curve).
        assert!(caps.needs_wav_samples());
        assert!(caps.needs_oto());
        assert!(!caps.needs_frq());
        assert_eq!(caps.sample_rate(), 44100);
        assert_eq!(caps.channels(), 1);
    }

    #[test]
    fn engine_name_is_stable() {
        // A renderer-less adapter can't be built (needs a .so), but the
        // name/caps contract is static — assert the trait contract here so
        // hosts can rely on it for capability negotiation.
        use domain::capabilities::EngineCapabilities as _;
        let caps: &dyn EngineCapabilities = WorldlineCapabilities::get();
        assert_eq!(caps.id(), "worldline");
        assert!(caps.expressions().len() >= 10);
    }
}
