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
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use media::{FileMediaSource, MediaSource, MemoryMediaSource};
use synth_cli::pipeline::{self, MixdownReport, RenderReport, StreamMixdownReport};
use tokio::sync::oneshot;
use voicebank::Voicebank;

/// Host asset resolver for wave (audio) parts.
///
/// `None` = this host serves no media, so every wave part is reported as
/// skipped instead of silently dropped. It is the host's job to resolve a
/// logical `media_ref` to bytes; the engine owns everything after that.
pub type SharedMedia = Arc<dyn MediaSource + Send + Sync>;

struct UploadedMediaResolver {
    uploaded: MemoryMediaSource,
    files: Option<FileMediaSource>,
    uploaded_root: Arc<std::sync::RwLock<Option<PathBuf>>>,
}

impl MediaSource for UploadedMediaResolver {
    fn load(&self, media_ref: &str) -> Result<Vec<u8>, String> {
        self.uploaded.load(media_ref).or_else(|_| {
            if let Some(id) = media_ref.strip_prefix("asset:") {
                let root = self
                    .uploaded_root
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone();
                if let Some(root) = root {
                    if !id.is_empty()
                        && id
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                    {
                        if let Ok(bytes) = std::fs::read(root.join(format!("{id}.media"))) {
                            return Ok(bytes);
                        }
                    }
                }
            }
            self.files
                .as_ref()
                .ok_or_else(|| format!("no uploaded media registered for '{media_ref}'"))?
                .load(media_ref)
        })
    }
}

/// Chunk counters shared with the owner of a render job (§6.13 item 3).
///
/// The pipeline writes `done`/`total` at chunk boundaries only; the counters
/// are therefore the *coarsest* honest progress a client can show, and a
/// reader must treat a missing update as "no chunk finished yet", never as a
/// stall verdict.
#[derive(Debug, Default)]
pub struct ProgressCounters {
    done: AtomicUsize,
    total: AtomicUsize,
    /// Set by the first pipeline callback. This is how a job's owner can tell
    /// "still queued behind another render" from "the worker picked it up" —
    /// the worker has a single serial queue and reports nothing else.
    started: AtomicBool,
}

impl ProgressCounters {
    /// `(done_chunks, total_chunks)`; `total == 0` means the plan is not known.
    pub fn snapshot(&self) -> (usize, usize) {
        (
            self.done.load(Ordering::Relaxed),
            self.total.load(Ordering::Relaxed),
        )
    }

    /// Whether the render worker has begun this job.
    pub fn is_started(&self) -> bool {
        self.started.load(Ordering::Acquire)
    }

    /// Record the chunk counters the pipeline reported.
    pub fn report(&self, done: usize, total: usize) {
        // The pipeline can render chunks on Rayon workers, so callbacks may
        // arrive out of order. Never let a slower callback move the UI's
        // progress bar backwards.
        self.total.store(total, Ordering::Relaxed);
        self.done.fetch_max(done, Ordering::Relaxed);
        self.started.store(true, Ordering::Release);
    }
}

/// Where a render job reads its project from.
enum ProjectSource {
    /// Legacy: the worker re-reads the file (preserved behavior, §6.13 item 8).
    Path(PathBuf),
    /// v2: an immutable snapshot serialized at revision R by the session
    /// (§6.13 item 9 — the worker must never re-read a file that may have
    /// changed since the job started).
    Yaml(String),
}

/// Chunk render cache capacity (same as the synth-cli CLI default). The
/// pipeline caches rendered PCM chunks keyed by chunk hash + engine salt,
/// so unchanged edits / engine re-picks round-trip in milliseconds
/// instead of re-synthesizing the whole song (a 14 s demo ≈ 6.6 s of
/// mel→vocoder per fresh render).
const RENDER_CACHE_BYTES: usize = 64 << 20;
/// At most one complete PCM block may wait for a transport consumer.
pub const STREAM_QUEUE_BLOCKS: usize = 1;
/// Do not let an abandoned transport pin the serial render worker forever.
pub const STREAM_QUEUE_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

fn send_stream_event(
    events: &mpsc::SyncSender<StreamRenderEvent>,
    mut event: StreamRenderEvent,
    cancel: Option<&AtomicBool>,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        if cancel.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            return Err("render cancelled".to_string());
        }
        match events.try_send(event) {
            Ok(()) => return Ok(()),
            Err(mpsc::TrySendError::Disconnected(_)) => {
                return Err("stream consumer disconnected".to_string());
            }
            Err(mpsc::TrySendError::Full(returned)) => {
                event = returned;
                if Instant::now() >= deadline {
                    return Err("stream consumer idle timeout".to_string());
                }
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }
}

/// One bounded producer event from a pinned mixdown stream.
#[derive(Debug)]
pub enum StreamRenderEvent {
    Data { frame_start: u64, samples: Vec<f32> },
    Finished(Result<StreamMixdownReport, String>),
}

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
        source: ProjectSource,
        voicebank: Arc<Voicebank>,
        track: i32,
        /// Engine for the whole render ("r1"/"r2"/"classic").
        engine: String,
        /// Optional per-phrase engines (one id per phrase in render
        /// order) — "ระหว่างท่อน". Empty = use `engine` for everything.
        engines: Vec<String>,
        /// Stable-session choices aligned to voice parts/notes, if the render
        /// was requested from a live project session.
        pronunciations: Option<Vec<Vec<Option<domain::UPronunciation>>>>,
        /// Resolver for this track's wave (audio) parts. `None` when the host
        /// serves no media; wave parts are then reported as skipped.
        media: Option<SharedMedia>,
        /// Per-job cancel flag. `None` = the legacy shared flag (`/cancel`),
        /// which is what v1 clients still drive.
        cancel: Option<Arc<AtomicBool>>,
        /// Per-job chunk counters, when the caller wants progress.
        progress: Option<Arc<ProgressCounters>>,
        reply: oneshot::Sender<Result<RenderReport, String>>,
    },
    /// Render every track plus wave parts into OpenUtau's stereo mixdown.
    RenderMixdown {
        yaml: String,
        voicebank: Arc<Voicebank>,
        engine: String,
        media: Option<SharedMedia>,
        pronunciations: Vec<Vec<Vec<Option<domain::UPronunciation>>>>,
        cancel: Arc<AtomicBool>,
        progress: Arc<ProgressCounters>,
        reply: oneshot::Sender<Result<MixdownReport, String>>,
    },
    /// Incremental full-project mixdown. The bounded synchronous channel
    /// applies backpressure directly to the render worker.
    RenderMixdownStream {
        yaml: String,
        voicebanks: Vec<Option<Arc<Voicebank>>>,
        engine: String,
        media: Option<SharedMedia>,
        pronunciations: Vec<Vec<Vec<Option<domain::UPronunciation>>>>,
        cancel: Arc<AtomicBool>,
        progress: Arc<ProgressCounters>,
        events: mpsc::SyncSender<StreamRenderEvent>,
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
    /// Resolver handed to every render job so a track's wave (audio) parts can
    /// be mixed in. `None` when the host was started without a media root.
    media: Option<SharedMedia>,
    uploaded_media: MemoryMediaSource,
    uploaded_media_root: Arc<std::sync::RwLock<Option<PathBuf>>>,
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
        moe_prefix: Option<PathBuf>,
        moe_vocoder: Option<PathBuf>,
        media_root: Option<PathBuf>,
    ) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel::<RenderJob>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_for_worker = cancel.clone();
        // Resolution follows OpenUtau's `UWavePart.AfterLoad`: the reference is
        // combined with the project folder, and real `.ustx` files do point
        // outside it, so this is `new` (not `restricted`) — the same trust
        // decision `FileMediaSource` documents.
        let uploaded_media = MemoryMediaSource::new();
        let uploaded_media_root = Arc::new(std::sync::RwLock::new(None));
        let files = media_root.clone().map(FileMediaSource::new);
        let media: Option<SharedMedia> = Some(Arc::new(UploadedMediaResolver {
            uploaded: uploaded_media.clone(),
            files,
            uploaded_root: uploaded_media_root.clone(),
        }));
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
                    moe_prefix,
                    moe_vocoder,
                    rx,
                    ready_tx,
                    cancel_for_worker,
                )
            })
            .map_err(|e| format!("spawn render worker thread: {e}"))?;
        ready_rx
            .recv()
            .map_err(|_| "render worker died during startup".to_string())??;
        Ok(RenderService {
            tx,
            media,
            uploaded_media,
            uploaded_media_root,
            cancel,
        })
    }

    /// Register one host-uploaded audio asset under its opaque logical key.
    pub fn register_media(&self, media_ref: impl Into<String>, bytes: Vec<u8>) {
        self.uploaded_media.insert(media_ref, bytes);
    }

    /// Configure persistent storage for `asset:<id>` uploads. Reads are lazy,
    /// so a restart does not preload large media files into memory.
    pub fn set_uploaded_media_root(&self, root: PathBuf) {
        *self
            .uploaded_media_root
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(root);
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
                source: ProjectSource::Path(project_path),
                voicebank,
                track,
                engine: engine.to_string(),
                engines,
                pronunciations: None,
                media: self.media.clone(),
                // Legacy path: the shared `/cancel` flag drives it, as before.
                cancel: None,
                progress: None,
                reply: reply_tx,
            })
            .map_err(|e| format!("render worker is gone: {e}"))?;
        reply_rx
            .await
            .map_err(|_| "render worker dropped the reply".to_string())?
    }

    /// Render a project from an **immutable snapshot** with its own cancel flag
    /// and progress counters (§6.13 items 1, 3, 9).
    ///
    /// `cancel` belongs to this job alone: setting it stops this render and
    /// leaves other queued jobs untouched. `progress` is written at chunk
    /// boundaries by the worker.
    #[allow(clippy::too_many_arguments)]
    pub async fn render_project_snapshot(
        &self,
        yaml: String,
        voicebank: Arc<Voicebank>,
        track: i32,
        engine: &str,
        engines: Vec<String>,
        cancel: Arc<AtomicBool>,
        progress: Arc<ProgressCounters>,
    ) -> Result<RenderReport, String> {
        self.render_project_snapshot_with_pronunciations(
            yaml, voicebank, track, engine, engines, None, cancel, progress,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn render_project_snapshot_with_pronunciations(
        &self,
        yaml: String,
        voicebank: Arc<Voicebank>,
        track: i32,
        engine: &str,
        engines: Vec<String>,
        pronunciations: Option<Vec<Vec<Option<domain::UPronunciation>>>>,
        cancel: Arc<AtomicBool>,
        progress: Arc<ProgressCounters>,
    ) -> Result<RenderReport, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(RenderJob::RenderProject {
                source: ProjectSource::Yaml(yaml),
                voicebank,
                track,
                engine: engine.to_string(),
                engines,
                pronunciations,
                media: self.media.clone(),
                cancel: Some(cancel),
                progress: Some(progress),
                reply: reply_tx,
            })
            .map_err(|e| format!("render worker is gone: {e}"))?;
        reply_rx
            .await
            .map_err(|_| "render worker dropped the reply".to_string())?
    }

    /// Render a whole-project stereo mixdown from an immutable session snapshot.
    pub async fn render_mixdown_snapshot(
        &self,
        yaml: String,
        voicebank: Arc<Voicebank>,
        engine: &str,
        pronunciations: Vec<Vec<Vec<Option<domain::UPronunciation>>>>,
        cancel: Arc<AtomicBool>,
        progress: Arc<ProgressCounters>,
    ) -> Result<MixdownReport, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(RenderJob::RenderMixdown {
                yaml,
                voicebank,
                engine: engine.to_string(),
                media: self.media.clone(),
                pronunciations,
                cancel,
                progress,
                reply: reply_tx,
            })
            .map_err(|e| format!("render worker is gone: {e}"))?;
        reply_rx
            .await
            .map_err(|_| "render worker dropped the reply".to_string())?
    }

    /// Start incremental whole-project rendering from an immutable snapshot.
    /// The returned receiver has a one-block capacity; if the consumer stops
    /// reading, rendering blocks instead of accumulating the final mix.
    pub fn render_mixdown_stream_snapshot(
        &self,
        yaml: String,
        voicebanks: Vec<Option<Arc<Voicebank>>>,
        engine: &str,
        pronunciations: Vec<Vec<Vec<Option<domain::UPronunciation>>>>,
        cancel: Arc<AtomicBool>,
        progress: Arc<ProgressCounters>,
    ) -> Result<mpsc::Receiver<StreamRenderEvent>, String> {
        let (events_tx, events_rx) = mpsc::sync_channel(STREAM_QUEUE_BLOCKS);
        self.tx
            .send(RenderJob::RenderMixdownStream {
                yaml,
                voicebanks,
                engine: engine.to_string(),
                media: self.media.clone(),
                pronunciations,
                cancel,
                progress,
                events: events_tx,
            })
            .map_err(|e| format!("render worker is gone: {e}"))?;
        Ok(events_rx)
    }

    /// Hard Stop: abort the in-flight render. Sets the shared cancel flag
    /// DIRECTLY (not via the channel — the worker is blocked inside the
    /// render when Stop is pressed, so a queued Cancel job would only be
    /// processed after the render finished). The pipeline checks the flag
    /// between chunks and the in-flight reply comes back as an error.
    pub fn cancel(&self) -> Result<(), String> {
        self.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
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
    pub async fn post_fx(&self, params: String, samples: Vec<f32>) -> Result<Vec<f32>, String> {
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
    moe_prefix: Option<PathBuf>,
    moe_vocoder: Option<PathBuf>,
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

    // r1-rs: the SAME WORLDLINE-R phrase path on the pure-Rust kernel
    // (worldline-rs) — no dlopen'd .so for the DSP. Selectable via
    // `engine: "r1-rs"`; the .so path stays the default.
    match synth_cli::engine::WorldlineRsEngine::open(false) {
        Ok(e) => {
            engines.insert("r1-rs".to_string(), Box::new(e));
        }
        Err(e) => {
            let _ = ready.send(Err(format!("open r1-rs engine: {e}")));
            return;
        }
    }

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

    // moe: MoE student acoustic (only when --moe-prefix given). The
    // vocoder is the same pc-nsf-hifigan onnx the r2 chain uses; the
    // checkpoint prefix decides the model. hidden/depth are the
    // general-expert sizes from the train run (defaults from moe-train).
    if let (Some(prefix), Some(vocoder)) = (moe_prefix, moe_vocoder) {
        match synth_cli::moe::MoeEngine::open(
            &prefix,
            &vocoder,
            moe_train::baseline::DEFAULT_HIDDEN,
            moe_train::baseline::DEFAULT_DEPTH,
            false,
        ) {
            Ok(e) => {
                engines.insert("moe".to_string(), Box::new(e));
            }
            Err(e) => {
                let _ = ready.send(Err(format!("open moe engine: {e}")));
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
                source,
                voicebank,
                track,
                engine: engine_name,
                engines: per_phrase,
                pronunciations,
                media,
                cancel: job_cancel,
                progress: job_progress,
                reply,
            } => {
                // Engine select: "r2"/"classic" route to the matching
                // engine; anything else = r1 (WORLDLINE-R). When
                // `per_phrase` is given (one engine id per phrase in
                // render order) each phrase renders with its own engine
                // ("ระหว่างท่อน").
                let resolve = |name: &str| -> Option<&dyn pipeline::PhraseRenderer> {
                    match name {
                        "r1-rs" if engines.contains_key("r1-rs") => engines.get("r1-rs"),
                        "r2" if engines.contains_key("r2") => engines.get("r2"),
                        "classic" if engines.contains_key("classic") => engines.get("classic"),
                        "diffsinger" if engines.contains_key("diffsinger") => {
                            engines.get("diffsinger")
                        }
                        "moe" if engines.contains_key("moe") => engines.get("moe"),
                        _ => engines.get("r1"),
                    }
                    .map(|b| b.as_ref() as &dyn pipeline::PhraseRenderer)
                };
                let default_renderer = resolve(&engine_name)
                    .unwrap_or_else(|| engines.get("r1").expect("r1 always opened").as_ref());
                // Per-phrase map: index (in render order) → engine id.
                let phrase_map: Vec<&str> = per_phrase.iter().map(|s| s.as_str()).collect();
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
                let engine_by_phrase_ref: Option<
                    &dyn Fn(usize) -> &'static dyn pipeline::PhraseRenderer,
                > = if resolved.is_empty() {
                    None
                } else {
                    Some(
                        &move |idx: usize| -> &'static dyn pipeline::PhraseRenderer {
                            resolved
                                .get(idx)
                                .copied()
                                .unwrap_or(unsafe { std::mem::transmute(default_renderer) })
                        },
                    )
                };
                // Cancel: a per-job flag when the caller supplied one (v2),
                // otherwise the legacy shared flag. The shared flag is cleared
                // at the start of a fresh render as it always was; a per-job
                // flag is fresh by construction, so clearing it would only
                // race with a cancel that arrived while the job was queued.
                let cancel_ref: &AtomicBool = match job_cancel.as_deref() {
                    Some(flag) => flag,
                    None => {
                        cancel.store(false, Ordering::Relaxed);
                        &cancel
                    }
                };
                // Chunk counters, when the job asked for progress.
                let progress_cb: Option<Box<dyn Fn(usize, usize) + Sync>> =
                    job_progress.as_ref().map(|counters| {
                        let counters = counters.clone();
                        Box::new(move |done: usize, total: usize| {
                            counters.report(done, total);
                        }) as Box<dyn Fn(usize, usize) + Sync>
                    });
                let result = (|| {
                    let project = match &source {
                        ProjectSource::Path(path) => pipeline::load_project(path)?,
                        ProjectSource::Yaml(yaml) => pipeline::parse_project(yaml)?,
                    };
                    // Render cache next to the voicebank (OpenUtau-style
                    // res-{hash} files). On Android this lands in
                    // filesDir/engine/cache — persists across Play/Export.
                    let cache_dir = voicebank
                        .path
                        .parent()
                        .map(|p| p.join("cache"))
                        .unwrap_or_else(|| std::path::PathBuf::from("/tmp/rusted-utau-cache"));
                    let mut cache = Some(runtime::RenderCache::new_in_dir(
                        &cache_dir,
                        RENDER_CACHE_BYTES,
                    ));
                    let kind = pipeline::PhonemizerKind::from_track(
                        project
                            .tracks
                            .get(track as usize)
                            .ok_or_else(|| format!("track {track} not found"))?,
                    );
                    // A host without a media root still renders: the voice
                    // path is unchanged and any wave part is reported as
                    // skipped with the reason.
                    let empty_media =
                        FileMediaSource::new(std::path::PathBuf::from("/nonexistent-media-root"));
                    let media = media.as_ref().map(|m| m.as_ref()).unwrap_or(&empty_media);
                    pipeline::render_track_with_media(
                        &project,
                        &voicebank,
                        default_renderer,
                        track,
                        kind,
                        false,
                        &mut cache,
                        media,
                        None,
                        Some(cancel_ref),
                        engine_by_phrase_ref,
                        progress_cb.as_deref(),
                        pronunciations.as_deref(),
                    )
                })();
                let _ = reply.send(result);
            }
            RenderJob::RenderMixdown {
                yaml,
                voicebank,
                engine: engine_name,
                media: wave_media,
                pronunciations,
                cancel: job_cancel,
                progress: job_progress,
                reply,
            } => {
                let renderer = engines
                    .get(&engine_name)
                    .or_else(|| engines.get("r1"))
                    .expect("r1 always opened");
                let progress_cb: Option<Box<dyn Fn(usize, usize) + Sync>> = Some(Box::new({
                    let counters = job_progress.clone();
                    move |done: usize, total: usize| counters.report(done, total)
                }));
                let result = (|| {
                    let project = pipeline::parse_project(&yaml)?;
                    let cache_dir = voicebank
                        .path
                        .parent()
                        .map(|p| p.join("cache"))
                        .unwrap_or_else(|| std::path::PathBuf::from("/tmp/rusted-utau-cache"));
                    let mut cache = Some(runtime::RenderCache::new_in_dir(
                        &cache_dir,
                        RENDER_CACHE_BYTES,
                    ));
                    let empty_media =
                        FileMediaSource::new(std::path::PathBuf::from("/nonexistent-media-root"));
                    let media = wave_media
                        .as_ref()
                        .map(|m| m.as_ref())
                        .unwrap_or(&empty_media);
                    pipeline::render_mixdown_with_pronunciations(
                        &project,
                        &voicebank,
                        renderer.as_ref(),
                        None,
                        false,
                        &mut cache,
                        media,
                        None,
                        Some(&job_cancel),
                        progress_cb.as_deref(),
                        Some(&pronunciations),
                    )
                })();
                let _ = reply.send(result);
            }
            RenderJob::RenderMixdownStream {
                yaml,
                voicebanks,
                engine: engine_name,
                media: wave_media,
                pronunciations,
                cancel: job_cancel,
                progress: job_progress,
                events,
            } => {
                let renderer = engines
                    .get(&engine_name)
                    .or_else(|| engines.get("r1"))
                    .expect("r1 always opened");
                let progress_cb: Option<Box<dyn Fn(usize, usize) + Sync>> = Some(Box::new({
                    let counters = job_progress.clone();
                    move |done: usize, total: usize| counters.report(done, total)
                }));
                let result = (|| {
                    let project = pipeline::parse_project(&yaml)?;
                    let cache_dir = voicebanks
                        .iter()
                        .flatten()
                        .next()
                        .and_then(|bank| bank.path.parent())
                        .map(|p| p.join("cache"))
                        .unwrap_or_else(|| std::path::PathBuf::from("/tmp/rusted-utau-cache"));
                    let mut cache = Some(runtime::RenderCache::new_in_dir(
                        &cache_dir,
                        RENDER_CACHE_BYTES,
                    ));
                    let empty_media =
                        FileMediaSource::new(std::path::PathBuf::from("/nonexistent-media-root"));
                    let media = wave_media
                        .as_ref()
                        .map(|m| m.as_ref())
                        .unwrap_or(&empty_media);
                    let voicebank_refs = voicebanks
                        .iter()
                        .map(|bank| bank.as_deref())
                        .collect::<Vec<_>>();
                    pipeline::render_mixdown_stream_by_track_with_pronunciations(
                        &project,
                        &voicebank_refs,
                        renderer.as_ref(),
                        None,
                        &mut cache,
                        media,
                        Some(&job_cancel),
                        progress_cb.as_deref(),
                        Some(&pronunciations),
                        pipeline::STREAM_BLOCK_FRAMES,
                        |frame_start, samples| {
                            send_stream_event(
                                &events,
                                StreamRenderEvent::Data {
                                    frame_start,
                                    samples: samples.to_vec(),
                                },
                                Some(&job_cancel),
                                STREAM_QUEUE_IDLE_TIMEOUT,
                            )
                        },
                    )
                })();
                let _ = send_stream_event(
                    &events,
                    StreamRenderEvent::Finished(result),
                    None,
                    STREAM_QUEUE_IDLE_TIMEOUT,
                );
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
                    Some(mixer_path) => match mixer_fx::MixerFx::open(mixer_path, &params) {
                        Ok(mut fx) => {
                            let mut out = samples;
                            match fx.process(&mut out, 0.0) {
                                Ok(()) => Ok(out),
                                Err(e) => Err(format!("mixer fx process: {e}")),
                            }
                        }
                        Err(e) => Err(format!("mixer fx open {e}")),
                    },
                    None => Err("no mixer .so configured (--mixer-so)".to_string()),
                };
                let _ = reply.send(result);
            }
        }
    }
}

#[cfg(test)]
mod stream_queue_tests {
    use super::*;

    #[test]
    fn bounded_stream_send_times_out_when_consumer_stops_reading() {
        let (tx, _rx) = mpsc::sync_channel(0);
        let start = Instant::now();
        let result = send_stream_event(
            &tx,
            StreamRenderEvent::Data {
                frame_start: 0,
                samples: vec![0.0; 8],
            },
            None,
            Duration::from_millis(10),
        );
        assert_eq!(result.unwrap_err(), "stream consumer idle timeout");
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn bounded_stream_send_observes_cancel_while_waiting_for_capacity() {
        let (tx, _rx) = mpsc::sync_channel(0);
        let cancel = AtomicBool::new(true);
        let result = send_stream_event(
            &tx,
            StreamRenderEvent::Data {
                frame_start: 0,
                samples: vec![0.0; 8],
            },
            Some(&cancel),
            Duration::from_secs(1),
        );
        assert_eq!(result.unwrap_err(), "render cancelled");
    }
}
