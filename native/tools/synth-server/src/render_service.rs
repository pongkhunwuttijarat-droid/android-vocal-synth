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
}

/// Handle to the render worker thread.
#[derive(Clone)]
pub struct RenderService {
    tx: mpsc::Sender<RenderJob>,
}

impl RenderService {
    /// Spawn the worker thread and open `so_path` on it. Blocks until the
    /// `.so` is loaded (or fails), so a returned `Ok` guarantees the
    /// renderer is ready.
    pub fn spawn(so_path: PathBuf) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel::<RenderJob>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        std::thread::Builder::new()
            .name("synth-render".to_string())
            .spawn(move || run_worker(so_path, rx, ready_tx))
            .map_err(|e| format!("spawn render worker thread: {e}"))?;
        ready_rx
            .recv()
            .map_err(|_| "render worker died during startup".to_string())??;
        Ok(RenderService { tx })
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
}

fn run_worker(
    so_path: PathBuf,
    rx: mpsc::Receiver<RenderJob>,
    ready: mpsc::Sender<Result<(), String>>,
) {
    // Open the engine here, on the worker thread: the WorldlineEngine
    // wraps a WorldlineRenderer, which is !Send, so it must never cross a
    // thread boundary. The engine adapter owns the render cache across
    // requests (previously rebuilt per request).
    let mut engine: Box<dyn Engine> = match WorldlineEngine::open(&so_path, false) {
        Ok(engine) => {
            let _ = ready.send(Ok(()));
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
        }
    }
}


