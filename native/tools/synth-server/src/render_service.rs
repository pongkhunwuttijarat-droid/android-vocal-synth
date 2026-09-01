//! The render worker: owns the `!Send + !Sync` [`WorldlineRenderer`] on
//! a dedicated thread and answers render jobs from the async handlers.
//!
//! The C++ `PhraseSynth` behind `worldline-plugin` must never be shared
//! across threads, and `WorldlineRenderer` is not even `Send` — so the
//! renderer is created *inside* the worker thread (from the `.so` path)
//! and never crosses a thread boundary. Requests are sent over an mpsc
//! channel and replies come back over tokio oneshots. Renders are
//! serialized on the worker, which the library requires anyway.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::Arc;

use synth_cli::pipeline::{self, RenderReport};
use tokio::sync::oneshot;
use voicebank::Voicebank;

/// Chunk render cache capacity (same as the synth-cli CLI default). The
/// pipeline caches rendered PCM chunks keyed by chunk hash + engine salt,
/// so unchanged edits / engine re-picks round-trip in milliseconds
/// instead of re-synthesizing the whole song (a 14 s demo ≈ 6.6 s of
/// mel→vocoder per fresh render).
const RENDER_CACHE_BYTES: usize = 64 << 20;

/// One unit of work for the render worker.
enum RenderJob {
    /// `synth-note`: render a single phoneme/note.
    SynthNote {
        voicebank: Arc<Voicebank>,
        alias: String,
        tone: i32,
        duration_ms: f64,
        reply: oneshot::Sender<Result<Vec<f32>, String>>,
    },
    /// `render`: render every voice part of a track of a `.ustx` project.
    /// The project is loaded on the worker (pure Rust, no .so needed).
    RenderProject {
        project_path: PathBuf,
        voicebank: Arc<Voicebank>,
        track: i32,
        /// Engine for the whole render ("r1"/"r2"/"classic").
        engine: String,
        /// Optional per-phrase engines (one id per phrase in render
        /// order) — "ระหว่างท่อน". Empty = use `engine` for everything.
        engines: Vec<String>,
        reply: oneshot::Sender<Result<RenderReport, String>>,
    },
    /// Hard Stop: abort the in-flight render. Sets the shared cancel flag
    /// the pipeline checks between chunks; the worker also drains queued
    /// render jobs so a Stop lands even while earlier requests queue.
    Cancel,
    /// Hot-swap the mixer FX params (fader/EQ/comp changes from the app).
    SetMixerParams(String),
    /// Post-synth FX: apply the mixer chain (with `params`) to an already
    /// rendered wav's samples — NO re-synthesis. This is the fast path for
    /// mixer fader/EQ drags: synth once, then re-FX in milliseconds.
    PostFx {
        params: String,
        samples: Vec<f32>,
        reply: oneshot::Sender<Result<Vec<f32>, String>>,
    },
}

/// Handle to the render worker thread.
#[derive(Clone)]
pub struct RenderService {
    tx: mpsc::Sender<RenderJob>,
    /// Shared cancel flag: set directly by [`RenderService::cancel`]
    /// (NOT via the channel — the worker is blocked inside a render when
    /// Stop is pressed, so a queued Cancel job would only land after the
    /// render finished). The pipeline checks it between chunks.
    cancel: Arc<AtomicBool>,
}

impl RenderService {
    /// Spawn the worker thread and open `so_path` on it. Blocks until the
    /// `.so` is loaded (or fails), so a returned `Ok` guarantees the
    /// renderer is ready. `mixer_so`/`mixer_params` optionally load the
    /// mixer FX plugin (libmixerfx.so) on the same worker thread.
    pub fn spawn(
        so_path: PathBuf,
        mixer_so: Option<PathBuf>,
        mixer_params: String,
        mel_onnx: Option<PathBuf>,
        vocoder_onnx: Option<PathBuf>,
        ds_model: Option<PathBuf>,
    ) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel::<RenderJob>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_for_worker = cancel.clone();
        std::thread::Builder::new()
            .name("synth-render".to_string())
            .spawn(move || {
                run_worker(
                    so_path,
                    mixer_so,
                    mixer_params,
                    mel_onnx,
                    vocoder_onnx,
                    ds_model,
                    rx,
                    ready_tx,
                    cancel_for_worker,
                )
            })
            .map_err(|e| format!("spawn render worker thread: {e}"))?;
        ready_rx
            .recv()
            .map_err(|_| "render worker died during startup".to_string())??;
        Ok(RenderService { tx, cancel })
    }

    /// Render one note with a single phoneme (see `pipeline::synth_note`).
    pub async fn synth_note(
        &self,
        voicebank: Arc<Voicebank>,
        alias: String,
        tone: i32,
        duration_ms: f64,
    ) -> Result<Vec<f32>, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(RenderJob::SynthNote {
                voicebank,
                alias,
                tone,
                duration_ms,
                reply: reply_tx,
            })
            .map_err(|e| format!("render worker is gone: {e}"))?;
        reply_rx
            .await
            .map_err(|_| "render worker dropped the reply".to_string())?
    }

    /// Render a project track (see `pipeline::render_project`). The
    /// phonemizer is picked from the track's setting inside the worker.
    /// `engine` selects the synthesis engine: "r1" (WORLDLINE-R),
    /// "r2" (WORLDLINE-R2 neural), "classic" (WORLDLINE-R Classic).
    pub async fn render_project(
        &self,
        project_path: PathBuf,
        voicebank: Arc<Voicebank>,
        track: i32,
        engine: &str,
        engines: Vec<String>,
    ) -> Result<RenderReport, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(RenderJob::RenderProject {
                project_path,
                voicebank,
                track,
                engine: engine.to_string(),
                engines,
                reply: reply_tx,
            })
            .map_err(|e| format!("render worker is gone: {e}"))?;
        reply_rx
            .await
            .map_err(|_| "render worker dropped the reply".to_string())?
    }

    /// Hard Stop: abort the in-flight render. Sets the shared cancel flag
    /// DIRECTLY (not via the channel — the worker is blocked inside the
    /// render when Stop is pressed, so a queued Cancel job would only be
    /// processed after the render finished). The pipeline checks the flag
    /// between chunks and the in-flight reply comes back as an error.
    pub fn cancel(&self) -> Result<(), String> {
        self.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    /// Hot-swap the mixer FX params (recreates the plugin on the worker,
    /// so the next render uses the new fader/EQ/comp settings).
    pub fn set_mixer_params(&self, params: String) -> Result<(), String> {
        self.tx
            .send(RenderJob::SetMixerParams(params))
            .map_err(|e| format!("render worker is gone: {e}"))
    }

    /// Post-synth FX: apply the mixer chain (`params`) to raw samples —
    /// no synthesis. Returns the FX'd samples.
    pub async fn post_fx(
        &self,
        params: String,
        samples: Vec<f32>,
    ) -> Result<Vec<f32>, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(RenderJob::PostFx {
                params,
                samples,
                reply: reply_tx,
            })
            .map_err(|e| format!("render worker is gone: {e}"))?;
        reply_rx
            .await
            .map_err(|_| "render worker dropped the reply".to_string())?
    }
}

fn run_worker(
    so_path: PathBuf,
    mixer_so: Option<PathBuf>,
    mixer_params: String,
    mel_onnx: Option<PathBuf>,
    vocoder_onnx: Option<PathBuf>,
    ds_model: Option<PathBuf>,
    rx: mpsc::Receiver<RenderJob>,
    ready: mpsc::Sender<Result<(), String>>,
    cancel: Arc<AtomicBool>,
) {
    // Cooperative-cancel flag: shared with the engine (pipeline checks it
    // between chunks) and set directly by RenderService::cancel.
    let _ = &cancel;
    // Open ALL engines here, on the worker thread (they wrap !Send
    // renderers — never cross a thread boundary). r1 (WORLDLINE-R) and
    // classic (WORLDLINE-R Classic) always open; r2 (WORLDLINE-R2
    // neural) only when both ONNX models are provided; diffsinger only
    // when --ds-model is given. The request's
    // `engine`/`engines` field picks which one renders (per request or
    // per phrase — "ระหว่างท่อน").
    let mut engines: std::collections::HashMap<String, Box<dyn pipeline::PhraseRenderer>> =
        std::collections::HashMap::new();

    // r1: C++ PhraseSynth — the default (WORLDLINE-R).
    let r1 = match worldline_plugin::WorldlineRenderer::open(&so_path) {
        Ok(e) => e,
        Err(e) => {
            let _ = ready.send(Err(format!("open {}: {e}", so_path.display())));
            return;
        }
    };
    engines.insert("r1".to_string(), Box::new(r1));

    // classic: SynthFeatures → C++ WorldSynthesis (no neural vocoder).
    match synth_cli::engine::WorldlineClassicEngine::open(&so_path, false) {
        Ok(e) => {
            engines.insert("classic".to_string(), Box::new(e));
        }
        Err(e) => {
            let _ = ready.send(Err(format!("open classic engine: {e}")));
            return;
        }
    }

    // r2: neural mel → HiFiGAN (only when the models shipped).
    if let (Some(mel), Some(voc)) = (mel_onnx, vocoder_onnx) {
        match synth_cli::engine::WorldlineR2Engine::open(&so_path, &mel, &voc, false) {
            Ok(e) => {
                engines.insert("r2".to_string(), Box::new(e));
            }
            Err(e) => {
                let _ = ready.send(Err(format!("open r2 engine: {e}")));
                return;
            }
        }
    }

    // diffsinger: diffusion acoustic model (only when --ds-model given).
    if let Some(ds) = ds_model {
        match synth_cli::engine::DsEngine::open(&ds, false) {
            Ok(e) => {
                engines.insert("diffsinger".to_string(), Box::new(e));
            }
            Err(e) => {
                let _ = ready.send(Err(format!("open diffsinger engine: {e}")));
                return;
            }
        }
    }

    let _ = ready.send(Ok(()));

    while let Ok(job) = rx.recv() {
        match job {
            RenderJob::SynthNote {
                voicebank,
                alias,
                tone,
                duration_ms,
                reply,
            } => {
                // synth-note always uses the default (r1) engine.
                let engine = engines.get("r1").expect("r1 always opened");
                let result = synth_cli::pipeline::synth_note(
                    &voicebank,
                    engine.as_ref(),
                    &alias,
                    tone,
                    duration_ms,
                );
                let _ = reply.send(result);
            }
            RenderJob::RenderProject {
                project_path,
                voicebank,
                track,
                engine: engine_name,
                engines: per_phrase,
                reply,
            } => {
                // Engine select: "r2"/"classic" route to the matching
                // engine; anything else = r1 (WORLDLINE-R). When
                // `per_phrase` is given (one engine id per phrase in
                // render order) each phrase renders with its own engine
                // ("ระหว่างท่อน").
                let resolve = |name: &str| -> Option<&dyn pipeline::PhraseRenderer> {
                    match name {
                        "r2" if engines.contains_key("r2") => engines.get("r2"),
                        "classic" if engines.contains_key("classic") => engines.get("classic"),
                        "diffsinger" if engines.contains_key("diffsinger") => {
                            engines.get("diffsinger")
                        }
                        _ => engines.get("r1"),
                    }
                    .map(|b| b.as_ref() as &dyn pipeline::PhraseRenderer)
                };
                let default_renderer = resolve(&engine_name).unwrap_or_else(|| {
                    engines.get("r1").expect("r1 always opened").as_ref()
                });
                // Per-phrase map: index (in render order) → engine id.
                let phrase_map: Vec<&str> = per_phrase
                    .iter()
                    .map(|s| s.as_str())
                    .collect();
                // Resolve every phrase's renderer up front — the engines
                // live for the worker's lifetime in the HashMap, so the
                // borrows are safe to hand out as 'static for the span
                // of this render call.
                let resolved: Vec<&'static dyn pipeline::PhraseRenderer> = phrase_map
                    .iter()
                    .map(|name| {
                        let e = resolve(name).unwrap_or(default_renderer);
                        unsafe {
                            std::mem::transmute::<
                                &dyn pipeline::PhraseRenderer,
                                &'static dyn pipeline::PhraseRenderer,
                            >(e)
                        }
                    })
                    .collect();
                let engine_by_phrase_ref: Option<&dyn Fn(usize) -> &'static dyn pipeline::PhraseRenderer> =
                    if resolved.is_empty() {
                        None
                    } else {
                        Some(&move |idx: usize| -> &'static dyn pipeline::PhraseRenderer {
                            resolved.get(idx).copied().unwrap_or(unsafe {
                                std::mem::transmute(default_renderer)
                            })
                        })
                    };
                // Fresh render: clear any stale cancel flag first.
                cancel.store(false, std::sync::atomic::Ordering::Relaxed);
                let result = (|| {
                    let project = pipeline::load_project(&project_path)?;
                    // Render cache next to the voicebank (OpenUtau-style
                    // res-{hash} files). On Android this lands in
                    // filesDir/engine/cache — persists across Play/Export.
                    let cache_dir = voicebank
                        .path
                        .parent()
                        .map(|p| p.join("cache"))
                        .unwrap_or_else(|| std::path::PathBuf::from("/tmp/lilt-cache"));
                    let mut cache =
                        Some(runtime::RenderCache::new_in_dir(&cache_dir, RENDER_CACHE_BYTES));
                    let kind = pipeline::PhonemizerKind::from_track(
                        project
                            .tracks
                            .get(track as usize)
                            .ok_or_else(|| format!("track {track} not found"))?,
                    );
                    pipeline::render_project_with(
                        &project,
                        &voicebank,
                        default_renderer,
                        track,
                        kind,
                        false,
                        &mut cache,
                        None,
                        Some(&cancel),
                        engine_by_phrase_ref,
                    )
                })();
                let _ = reply.send(result);
            }
            RenderJob::Cancel => {
                // Hard Stop: set the flag — the pipeline bails between
                // chunks, and the queued render job's reply comes back as
                // "render cancelled".
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            RenderJob::SetMixerParams(_params) => {
                // Mixer FX is per-call: the app applies it via /post-fx
                // (PostFx carries the params) — the engine pipeline
                // renders raw, so there is nothing to hot-swap here.
            }
            RenderJob::PostFx {
                params,
                samples,
                reply,
            } => {
                // Post-synth FX: open a fresh mixer with the given params
                // (the render path's instance may have different params),
                // process the raw samples, reply with the FX'd stream.
                // No synthesis — this is the fast fader/EQ drag path.
                let result = match mixer_so.as_ref() {
                    Some(mixer_path) => {
                        match mixer_fx::MixerFx::open(mixer_path, &params) {
                            Ok(mut fx) => {
                                let mut out = samples;
                                match fx.process(&mut out, 0.0) {
                                    Ok(()) => Ok(out),
                                    Err(e) => Err(format!("mixer fx process: {e}")),
                                }
                            }
                            Err(e) => Err(format!("mixer fx open {e}")),
                        }
                    }
                    None => Err("no mixer .so configured (--mixer-so)".to_string()),
                };
                let _ = reply.send(result);
            }
        }
    }
}


