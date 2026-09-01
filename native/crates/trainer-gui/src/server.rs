//! axum server: shared training state + routes.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Json};
use axum::routing::{get, post};
use axum::Router;
use moe_train::baseline::{save_model, train_context};
use moe_train::data::load_train_set;
use serde::{Deserialize, Serialize};

/// MCP-like status payload (GET /api/status).
#[derive(Clone, Serialize, Debug)]
pub struct Status {
    pub state: String, // idle | loading | running | done | stopped | error
    pub mode: String,  // general | moe
    pub backend: String,
    pub epoch: usize,
    pub epochs_total: usize,
    pub train_loss: f64,
    pub val_loss: f64,
    /// Per-epoch [train, val] pairs.
    pub history: Vec<Vec<f64>>,
    /// MoE per-group report: [group, residual_before, residual_after].
    pub moe_report: Vec<Vec<f64>>,
    pub model_path: String,
    pub elapsed_s: f64,
    pub error: Option<String>,
}

impl Default for Status {
    fn default() -> Self {
        Status {
            state: "idle".into(),
            mode: "general".into(),
            backend: "wgpu".into(),
            epoch: 0,
            epochs_total: 0,
            train_loss: f64::NAN,
            val_loss: f64::NAN,
            history: Vec::new(),
            moe_report: Vec::new(),
            model_path: String::new(),
            elapsed_s: 0.0,
            error: None,
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub status: Arc<RwLock<Status>>,
    pub stop: Arc<AtomicBool>,
    pub train_set: String,
    pub vocoder: String,
    /// Lazily-loaded training set (shared — the JSONL can be ~200MB
    /// with long chains; load once, reuse for phrases/render).
    pub train_cache: Arc<RwLock<Option<Arc<moe_train::data::TrainSet>>>>,
    /// Training log ring buffer (also mirrored to `{train_set}.log`).
    pub logs: Arc<RwLock<Vec<String>>>,
    /// Last CPU sample for delta-based utilization.
    pub last_cpu: Arc<RwLock<(u64, u64)>>,
    /// Per-pid jiffies for per-process CPU% deltas.
    pub last_proc: Arc<RwLock<std::collections::HashMap<i32, u64>>>,
}

impl AppState {
    fn get_train_set(&self) -> Result<Arc<moe_train::data::TrainSet>, String> {
        {
            let c = self.train_cache.read().unwrap();
            if let Some(set) = c.as_ref() {
                return Ok(set.clone());
            }
        }
        let set = Arc::new(moe_train::data::load_train_set(&self.train_set)?);
        *self.train_cache.write().unwrap() = Some(set.clone());
        Ok(set)
    }

    /// Append a timestamped log line (ring buffer + mirror file).
    fn log(&self, line: &str) {
        let ts = {
            let s = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let h = (s / 3600) % 24;
            let m = (s / 60) % 60;
            let sec = s % 60;
            format!("{h:02}:{m:02}:{sec:02}")
        };
        let entry = format!("[{ts}] {line}");
        let path = log_path(&self.train_set); // before the lock (borrow order)
        let mut l = self.logs.write().unwrap();
        l.push(entry.clone());
        let over = l.len().saturating_sub(1000);
        if over > 0 {
            l.drain(..over);
        }
        // Durable mirror next to the train set.
        if let Some(path) = path {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                let _ = writeln!(f, "{entry}");
            }
        }
    }
}

fn log_path(train_set: &str) -> Option<String> {
    let mut p = train_set.to_string();
    if let Some(idx) = p.rfind('.') {
        p.truncate(idx);
    }
    Some(format!("{p}.log"))
}

/// POST /api/start body.
#[derive(Deserialize)]
pub struct StartParams {
    #[serde(default = "d_epochs")]
    pub epochs: usize,
    #[serde(default = "d_expert_epochs")]
    pub expert_epochs: usize,
    #[serde(default = "d_batch")]
    pub batch: usize,
    #[serde(default = "d_lr")]
    pub lr: f64,
    #[serde(default = "d_eval")]
    pub eval_frac: f64,
    #[serde(default = "d_backend")]
    pub backend: String,
    #[serde(default = "d_mode")]
    pub mode: String,
    #[serde(default)]
    pub resume: bool,
    #[serde(default = "d_hidden")]
    pub hidden: usize,
    #[serde(default = "d_depth")]
    pub depth: usize,
    #[serde(default)]
    pub gpu_cap: Option<usize>,
}
fn d_epochs() -> usize { 8 }
fn d_expert_epochs() -> usize { 10 }
fn d_batch() -> usize { 1024 }
fn d_lr() -> f64 { 1e-3 }
fn d_eval() -> f64 { 0.1 }
fn d_backend() -> String { "wgpu".into() }
fn d_hidden() -> usize { moe_train::baseline::DEFAULT_HIDDEN }
fn d_depth() -> usize { moe_train::baseline::DEFAULT_DEPTH }
fn d_mode() -> String { "general".into() }

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/status", get(api_status))
        .route("/api/phrases", get(api_phrases))
        .route("/api/perf", get(api_perf))
        .route("/api/logs", get(api_logs))
        .route("/api/start", post(api_start))
        .route("/api/stop", post(api_stop))
        .route("/api/render", post(api_render))
        .with_state(state)
}

/// Performance snapshot (GET /api/perf) — GPU busy %, CPU %, memory,
/// GPU temperature, read from sysfs/proc (AMD amdgpu / Mesa), plus
/// per-process stats for the AI's OWN binaries (trainer-gui, moe-train,
/// train-prep) so you can see exactly what the training is doing.
#[derive(Clone, Serialize, Debug, Default)]
pub struct PerfInfo {
    pub gpu_percent: f64,
    pub gpu_temp_c: f64,
    pub gpu_mem_used_gb: f64,
    pub gpu_mem_total_gb: f64,
    pub gpu_mem_percent: f64,
    pub cpu_percent: f64,
    pub mem_used_gb: f64,
    pub mem_total_gb: f64,
    pub processes: Vec<ProcInfo>,
}

#[derive(Clone, Serialize, Debug, Default)]
pub struct ProcInfo {
    pub pid: i32,
    pub name: String,
    pub cmd: String,
    pub state: String,
    pub threads: usize,
    pub cpu_percent: f64,
    pub rss_mb: f64,
}

/// Binary-name markers for "our" processes.
const AI_PROC_MARKERS: [&str; 5] = ["trainer-gui", "moe-train", "train-prep", "moe-train-gui", "synth-cli"];

/// Scan /proc for our own processes: (pid → jiffies) delta gives CPU%.
fn scan_ai_processes(last: &mut std::collections::HashMap<i32, u64>) -> Vec<ProcInfo> {
    let mut out = Vec::new();
    let page_kb = 4.0; // assume 4 KiB pages
    let ok = std::fs::read_dir("/proc");
    let dir = match ok {
        Ok(d) => d,
        Err(_) => return out,
    };
    for entry in dir.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|s| s.parse::<i32>().ok()) else {
            continue;
        };
        let stat = match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(s) => s,
            Err(_) => continue,
        };
        // comm may contain ')' — split after the last ')'.
        let Some(close) = stat.rfind(')') else { continue };
        let comm = stat[..close].rsplit('(').next().unwrap_or("?").to_string();
        let rest: Vec<&str> = stat[close + 1..].split_whitespace().collect();
        if rest.len() < 12 {
            continue;
        }
        let state = rest[0].to_string(); // field 3
        let threads: usize = rest.get(17).and_then(|v| v.parse().ok()).unwrap_or(0); // field 20
        let utime: u64 = rest[10].parse().unwrap_or(0); // field 14
        let stime: u64 = rest[11].parse().unwrap_or(0); // field 15
        let jiffies = utime + stime;

        let is_ours = AI_PROC_MARKERS.iter().any(|m| comm.contains(m));
        if !is_ours {
            continue;
        }

        // cmdline (truncated) + RSS.
        let cmd = std::fs::read_to_string(format!("/proc/{pid}/cmdline"))
            .unwrap_or_default()
            .replace('\0', " ");
        let cmd = cmd.trim().chars().take(80).collect::<String>();
        let rss_mb = std::fs::read_to_string(format!("/proc/{pid}/statm"))
            .ok()
            .and_then(|s| s.split_whitespace().nth(1).and_then(|v| v.parse::<f64>().ok()))
            .map(|pages| pages * page_kb / 1024.0)
            .unwrap_or(0.0);

        let cpu = if let Some(&prev) = last.get(&pid) {
            let d = jiffies.saturating_sub(prev);
            // delta per second over one sample interval (100 Hz ticks).
            d as f64 / 100.0
        } else {
            0.0
        };
        last.insert(pid, jiffies);

        out.push(ProcInfo {
            pid,
            name: comm,
            cmd,
            state,
            threads,
            cpu_percent: cpu.min(8000.0),
            rss_mb,
        });
    }
    out
}

fn read_first(dir: &str, file: &str) -> Option<f64> {
    let ok = std::fs::read_dir(dir).ok()?;
    for entry in ok.flatten() {
        let p = entry.path().join(file);
        // Try every entry — the first card (e.g. card0) may not have the
        // file; the AMD one (card1) does.
        if let Ok(v) = std::fs::read_to_string(p) {
            if let Ok(n) = v.trim().parse::<f64>() {
                return Some(n);
            }
        }
    }
    None
}

async fn api_perf(State(state): State<AppState>) -> Json<PerfInfo> {
    let mut info = PerfInfo::default();

    // GPU busy % + junction temp via amdgpu sysfs (card0..cardN).
    if let Some(v) = read_first("/sys/class/drm", "device/gpu_busy_percent") {
        info.gpu_percent = v;
    }
    if let Some(t) = read_first("/sys/class/drm", "device/hwmon/hwmon0/temp1_input") {
        info.gpu_temp_c = t / 1000.0;
    }
    // GPU memory (bytes): VRAM on dGPUs; on UMA iGPUs (760M) VRAM is 0
    // and the real pool is GTT (system RAM mapped for the GPU).
    let vram_u = read_first("/sys/class/drm", "device/mem_info_vram_used").unwrap_or(0.0);
    let vram_t = read_first("/sys/class/drm", "device/mem_info_vram_total").unwrap_or(0.0);
    let (used, total) = if vram_t > 0.0 {
        (vram_u, vram_t)
    } else {
        let gtt_u = read_first("/sys/class/drm", "device/mem_info_gtt_used").unwrap_or(0.0);
        let gtt_t = read_first("/sys/class/drm", "device/mem_info_gtt_total").unwrap_or(0.0);
        (gtt_u, gtt_t)
    };
    if total > 0.0 {
        info.gpu_mem_used_gb = used / 1e9;
        info.gpu_mem_total_gb = total / 1e9;
        info.gpu_mem_percent = used / total * 100.0;
    }

    // CPU % via /proc/stat delta.
    if let Ok(text) = std::fs::read_to_string("/proc/stat") {
        let line = text.lines().next().unwrap_or("");
        let nums: Vec<u64> = line
            .split_whitespace()
            .skip(1)
            .filter_map(|v| v.parse().ok())
            .collect();
        if nums.len() >= 4 {
            let idle = nums[3] + nums.get(4).copied().unwrap_or(0);
            let total: u64 = nums.iter().sum();
            let mut last = state.last_cpu.write().unwrap();
            if *last != (0, 0) {
                let (lt, li) = *last;
                let dt = total.saturating_sub(lt).max(1);
                let di = idle.saturating_sub(li);
                info.cpu_percent = (1.0 - di as f64 / dt as f64) * 100.0;
            }
            *last = (total, idle);
        }
    }

    // Memory via /proc/meminfo.
    if let Ok(text) = std::fs::read_to_string("/proc/meminfo") {
        let get = |k: &str| -> f64 {
            text.lines()
                .find(|l| l.starts_with(k))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse::<f64>().ok())
                .map(|kb| kb / 1024.0 / 1024.0) // kB -> GB
                .unwrap_or(0.0)
        };
        let total = get("MemTotal:");
        let avail = get("MemAvailable:");
        info.mem_total_gb = total;
        info.mem_used_gb = (total - avail).max(0.0);
    }

    // Per-process stats for our own binaries.
    let mut last_proc = state.last_proc.write().unwrap();
    info.processes = scan_ai_processes(&mut last_proc);

    Json(info)
}

/// GET /api/logs — the training log (ring buffer, newest last).
async fn api_logs(State(state): State<AppState>) -> Json<Vec<String>> {
    let l = state.logs.read().unwrap();
    let n = l.len();
    let start = n.saturating_sub(300);
    Json(l[start..].to_vec())
}

/// Phrase lookup table entry (GET /api/phrases).
#[derive(Clone, Serialize, Debug)]
pub struct PhraseInfo {
    pub idx: usize,
    pub aliases: Vec<String>,
    pub tone: i32,
    pub duration_ms: f64,
    pub frames: usize,
    pub n_phonemes: usize,
}

/// GET /api/phrases — the phrase index → aliases lookup table so the
/// GUI (and agents) can pick a phrase instead of guessing indices.
async fn api_phrases(State(state): State<AppState>) -> Result<Json<Vec<PhraseInfo>>, (StatusCode, String)> {
    let set = state.get_train_set().map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let mut out = Vec::with_capacity(set.phrases.len());
    let mut frames_by_phrase: Vec<usize> = vec![0; set.phrases.len()];
    for s in &set.samples {
        frames_by_phrase[s.phrase_id] += 1;
    }
    for (idx, p) in set.phrases.iter().enumerate() {
        out.push(PhraseInfo {
            idx,
            aliases: p.aliases.clone(),
            tone: p.tone,
            duration_ms: p.duration_ms,
            frames: frames_by_phrase[idx],
            n_phonemes: p.phoneme_ids.len(),
        });
    }
    Ok(Json(out))
}

async fn index() -> Html<&'static str> {
    Html(include_str!("gui.html"))
}

async fn api_status(State(state): State<AppState>) -> Json<Status> {
    Json(state.status.read().unwrap().clone())
}

async fn api_start(
    State(state): State<AppState>,
    Json(params): Json<StartParams>,
) -> Result<Json<Status>, (StatusCode, String)> {
    {
        let st = state.status.read().unwrap();
        if st.state == "running" || st.state == "loading" {
            return Err((
                StatusCode::CONFLICT,
                format!("already {}", st.state),
            ));
        }
    }
    // Reset + mark loading, then spawn the blocking training thread.
    {
        let mut st = state.status.write().unwrap();
        *st = Status {
            mode: params.mode.clone(),
            backend: params.backend.clone(),
            epochs_total: params.epochs,
            model_path: state.model_path(),
            ..Default::default()
        };
        st.state = "loading".into();
    }
    state.stop.store(false, Ordering::Relaxed);

    let inner = state.clone();
    std::thread::spawn(move || run_training(inner, params));
    Ok(Json(state.status.read().unwrap().clone()))
}

async fn api_stop(State(state): State<AppState>) -> Json<Status> {
    state.stop.store(true, Ordering::Relaxed);
    Json(state.status.read().unwrap().clone())
}

/// POST /api/render body.
#[derive(Deserialize)]
pub struct RenderParams {
    pub phrase_idx: usize,
    #[serde(default = "d_gain")]
    pub gain: f32,
    #[serde(default = "d_backend")]
    pub backend: String,
    #[serde(default = "d_hidden")]
    pub hidden: usize,
    #[serde(default = "d_depth")]
    pub depth: usize,
    #[serde(default)]
    pub gpu_cap: Option<usize>,
}
fn d_gain() -> f32 { 1.0 }

/// Render one phrase with the trained MoE and return the WAV bytes
/// (audio/wav) — the GUI plays it inline; an agent can curl it too.
async fn api_render(
    State(state): State<AppState>,
    Json(params): Json<RenderParams>,
) -> Result<axum::response::Response, (StatusCode, String)> {
    use axum::response::IntoResponse;
    use moe_train::render::{load_moe, render_phrase_wav};

    // Rendering mid-training reads half-written checkpoints (general
    // saves at the end, experts one-by-one) → stale/mismatched loads.
    let st = state.status.read().unwrap();
    if st.state == "running" || st.state == "loading" {
        return Err((StatusCode::CONFLICT, "training in progress".into()));
    }
    drop(st);
    let set = state.get_train_set().map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    if params.phrase_idx >= set.phrases.len() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("phrase {0} out of range (0..{1})", params.phrase_idx, set.phrases.len()),
        ));
    }
    let prefix = state.model_path();
    let bytes = match params.backend.as_str() {
        "wgpu-f16" => {
            type Backend = burn::backend::Wgpu<burn::tensor::f16, i32>;
            let device = burn::backend::wgpu::WgpuDevice::IntegratedGpu(0);
            let moe = load_moe::<Backend>(&set, &prefix, params.hidden, params.depth, &device)
                .map_err(|e| (StatusCode::NOT_FOUND, e))?;
            render_phrase_wav::<Backend>(&moe, &set, params.phrase_idx, &state.vocoder_path(), params.gain, &device)
        }
        "wgpu" => {
            type Backend = burn::backend::Wgpu;
            let device = burn::backend::wgpu::WgpuDevice::IntegratedGpu(0);
            let moe = load_moe::<Backend>(&set, &prefix, params.hidden, params.depth, &device)
                .map_err(|e| (StatusCode::NOT_FOUND, e))?;
            render_phrase_wav::<Backend>(&moe, &set, params.phrase_idx, &state.vocoder_path(), params.gain, &device)
        }
        "ndarray" => {
            type Backend = burn::backend::NdArray;
            let device = Default::default();
            let moe = load_moe::<Backend>(&set, &prefix, params.hidden, params.depth, &device)
                .map_err(|e| (StatusCode::NOT_FOUND, e))?;
            render_phrase_wav::<Backend>(&moe, &set, params.phrase_idx, &state.vocoder_path(), params.gain, &device)
        }
        other => return Err((StatusCode::BAD_REQUEST, format!("unknown backend: {other}"))),
    }
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    Ok((
        [(axum::http::header::CONTENT_TYPE, "audio/wav")],
        bytes,
    )
        .into_response())
}

impl AppState {
    fn model_path(&self) -> String {
        // Checkpoint prefix next to the train-set. DASH separator (NOT a
        // dot): burn's NamedMpk recorder does `set_extension("mpk")`,
        // which REPLACES a dot-extension — "X.model" would collide every
        // save into "X.mpk". "X-model" has no dot → "X-model-general.mpk"
        // etc. stay distinct.
        let mut p = self.train_set.clone();
        if let Some(idx) = p.rfind('.') {
            p.truncate(idx);
        }
        format!("{p}-model")
    }

    fn vocoder_path(&self) -> String {
        self.vocoder.clone()
    }
}

/// The blocking training loop (std thread, burn backends need no async).
fn run_training(state: AppState, params: StartParams) {
    let set = match load_train_set(&state.train_set) {
        Ok(s) => s,
        Err(e) => {
            state.log(&format!("ERROR loading train set: {e}"));
            let mut st = state.status.write().unwrap();
            st.state = "error".into();
            st.error = Some(e);
            return;
        }
    };
    state.log(&format!(
        "train-set loaded: {} samples, {} phonemes, {} phrases (mode={}, backend={}, epochs={})",
        set.samples.len(),
        set.vocab.len(),
        set.phrases.len(),
        params.mode,
        params.backend,
        params.epochs
    ));
    {
        let mut st = state.status.write().unwrap();
        st.state = "running".into();
    }
    let started = Instant::now();

    // Progress callback: update shared status + log every epoch.
    let st_state = state.status.clone();
    let log_state = state.clone();
    let mut progress = move |epoch: usize, train: f64, val: f64| {
        let mut st = st_state.write().unwrap();
        st.epoch = epoch + 1;
        st.train_loss = train;
        st.val_loss = val;
        st.history.push(vec![train, val]);
        st.elapsed_s = started.elapsed().as_secs_f64();
        log_state.log(&format!("epoch {epoch}: train={train:.4} val={val:.4}"));
    };

    let result: Result<(f64, f64, bool, Vec<Vec<f64>>), String> =
        match params.backend.as_str() {
            "wgpu" => {
                type Backend = burn::backend::Wgpu;
                type AB = burn::backend::Autodiff<Backend>;
                let device = burn::backend::wgpu::WgpuDevice::IntegratedGpu(0);
                run_training_on::<AB>(
                    &set,
                    &params,
                    &state,
                    &mut progress,
                    &device,
                )
            }
            "wgpu-f16" => {
                type Backend = burn::backend::Wgpu<burn::tensor::f16, i32>;
                type AB = burn::backend::Autodiff<Backend>;
                let device = burn::backend::wgpu::WgpuDevice::IntegratedGpu(0);
                run_training_on::<AB>(
                    &set,
                    &params,
                    &state,
                    &mut progress,
                    &device,
                )
            }
            "ndarray" => {
                type Backend = burn::backend::NdArray;
                type AB = burn::backend::Autodiff<Backend>;
                let device = Default::default();
                run_training_on::<AB>(
                    &set,
                    &params,
                    &state,
                    &mut progress,
                    &device,
                )
            }
            other => Err(format!("unknown backend: {other}")),
        };

    let mut st = state.status.write().unwrap();
    st.elapsed_s = started.elapsed().as_secs_f64();
    match result {
        Ok((train, val, stopped, report)) => {
            st.state = if stopped { "stopped" } else { "done" }.into();
            st.train_loss = train;
            st.val_loss = val;
            st.moe_report = report;
        }
        Err(e) => {
            st.state = "error".into();
            st.error = Some(e);
        }
    }
}

/// Backend-generic training dispatch: `general` (single context model)
/// or `moe` (general + per-group residual experts).
fn run_training_on<B: burn::tensor::backend::AutodiffBackend>(
    set: &moe_train::data::TrainSet,
    params: &StartParams,
    state: &AppState,
    progress: &mut dyn FnMut(usize, f64, f64),
    device: &B::Device,
) -> Result<(f64, f64, bool, Vec<Vec<f64>>), String> {
    use moe_train::baseline::{save_model, train_context, train_moe};

    if params.mode == "moe" {
        let report = train_moe::<B>(
            set,
            params.epochs,
            params.batch,
            params.lr,
            params.eval_frac,
            params.expert_epochs,
            &state.model_path(),
            params.hidden,
            params.depth,
            params.gpu_cap,
            device,
            params.resume,
            &state.stop,
            progress,
        )?;
        // Vocab sidecar: the render-time MoeEngine needs the phoneme id
        // mapping the checkpoints were trained on. Written even on the
        // GUI train path so the produced .mpk is renderable by an engine.
        if let Err(e) = write_vocab_sidecar_set(&state.model_path(), set) {
            eprintln!("WARN: vocab sidecar: {e}");
        }
        // Final general losses are the last progress values in status.
        let st = state.status.read().unwrap();
        let train = st.train_loss;
        let val = st.val_loss;
        let stopped = state.stop.load(Ordering::Relaxed);
        let report: Vec<Vec<f64>> = report
            .into_iter()
            .map(|(g, b, a)| vec![g as f64, b, a])
            .collect();
        Ok((train, val, stopped, report))
    } else {
        let init = if params.resume {
            moe_train::baseline::try_load_model::<B>(&state.model_path(), params.hidden, params.depth, device)?
        } else {
            None
        };
        let (model, train, val, stopped) = train_context::<B>(
            set,
            params.epochs,
            params.batch,
            params.lr,
            params.eval_frac,
            params.hidden,
            params.depth,
            params.gpu_cap,
            device,
            init,
            progress,
            &state.stop,
        );
        save_model(&model, &state.model_path())?;
        Ok((train, val, stopped, Vec::new()))
    }
}

/// Write `{prefix}-phonemes.txt` — one phoneme per line (index order =
/// `TrainSet.vocab`), so the render-time MoeEngine (set-free) can map a
/// phoneme symbol to the embedding id the checkpoints were trained with.
fn write_vocab_sidecar_set(prefix: &str, set: &moe_train::data::TrainSet) -> Result<(), String> {
    let path = format!("{prefix}-phonemes.txt");
    let mut body = String::new();
    for p in &set.vocab {
        body.push_str(p);
        body.push('\n');
    }
    std::fs::write(&path, body).map_err(|e| format!("write {path}: {e}"))
}
