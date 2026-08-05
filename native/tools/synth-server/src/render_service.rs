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

use synth_cli::engine::{Engine, WorldlineEngine};
use synth_cli::pipeline::{self, RenderReport};
use tokio::sync::oneshot;
use voicebank::Voicebank;

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
    ) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel::<RenderJob>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_for_worker = cancel.clone();
        std::thread::Builder::new()
            .name("synth-render".to_string())
            .spawn(move || run_worker(so_path, mixer_so, mixer_params, rx, ready_tx, cancel_for_worker))
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
    pub async fn render_project(
        &self,
        project_path: PathBuf,
        voicebank: Arc<Voicebank>,
        track: i32,
    ) -> Result<RenderReport, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(RenderJob::RenderProject {
                project_path,
                voicebank,
                track,
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
    rx: mpsc::Receiver<RenderJob>,
    ready: mpsc::Sender<Result<(), String>>,
    cancel: Arc<AtomicBool>,
) {
    // Cooperative-cancel flag: shared with the engine (pipeline checks it
    // between chunks) and set directly by RenderService::cancel.
    let _ = &cancel;
    // Open the engine here, on the worker thread: the WorldlineEngine
    // wraps a WorldlineRenderer, which is !Send, so it must never cross a
    // thread boundary. The engine adapter owns the render cache across
    // requests (previously rebuilt per request).
    let mut engine: Box<dyn Engine> = match WorldlineEngine::open(&so_path, false) {
        Ok(mut engine) => {
            // Optional mixer FX plugin — processed on the final mixed
            // samples inside the engine (WorldlineEngine.mixer).
            if let Some(mixer_path) = &mixer_so {
                match mixer_fx::MixerFx::open(mixer_path, &mixer_params) {
                    Ok(mixer) => {
                        engine.set_mixer(mixer);
                        // Share the cancel flag with the engine so the
                        // pipeline can abort between chunks.
                        engine.set_cancel(cancel.clone());
                        let _ = ready.send(Ok(()));
                    }
                    Err(e) => {
                        let _ = ready.send(Err(format!(
                            "mixer fx open {}: {e}",
                            mixer_path.display()
                        )));
                        return;
                    }
                }
            } else {
                engine.set_cancel(cancel.clone());
                let _ = ready.send(Ok(()));
            }
            Box::new(engine)
        }
        Err(e) => {
            let _ = ready.send(Err(format!("open {}: {e}", so_path.display())));
            return;
        }
    };
    while let Ok(job) = rx.recv() {
        match job {
            RenderJob::SynthNote {
                voicebank,
                alias,
                tone,
                duration_ms,
                reply,
            } => {
                let result =
                    engine.synth_note(&voicebank, &alias, tone, duration_ms);
                let _ = reply.send(result);
            }
            RenderJob::RenderProject {
                project_path,
                voicebank,
                track,
                reply,
            } => {
                // Fresh render: clear any stale cancel flag first.
                engine.clear_cancel();
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
                    engine.set_cache_dir(&cache_dir, 64 << 20)?;
                    engine.render_project(&project, &voicebank, track)
                })();
                let _ = reply.send(result);
            }
            RenderJob::Cancel => {
                // Hard Stop: set the flag — the pipeline bails between
                // chunks, and the queued render job's reply comes back as
                // "render cancelled".
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            RenderJob::SetMixerParams(params) => {
                // Hot-swap mixer params: recreate the FX instance so the
                // next render uses the new fader/EQ/comp settings. The
                // C++ MixerFx reads params only at Create.
                let p = params.clone();
                let res = mixer_so.as_ref().map(|mixer_path| {
                    mixer_fx::MixerFx::open(mixer_path, &p)
                });
                match res {
                    Some(Ok(mixer)) => {
                        engine.set_mixer(mixer);
                    }
                    Some(Err(e)) => {
                        eprintln!("mixer fx reload {e}");
                    }
                    None => {
                        eprintln!("mixer fx reload: no mixer .so configured");
                    }
                }
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


