//! v2 render jobs (`docs/CONTRACT-v1.md` §6.13).
//!
//! A render job owns an **immutable snapshot** of the session at the revision
//! it was started from, its own cancel flag, and its own chunk counters. This
//! module is the job *table* only — the state machine and the data a client
//! reads back. Running the render happens in [`crate::render_service`], on the
//! single worker thread the engine requires.
//!
//! Rules this table enforces (frozen in §6.13):
//!
//! * a job's revision never changes after `begin`, so a mutation that lands
//!   while it renders cannot alter its input;
//! * cancel is **per job** — stopping one render never touches another;
//! * the first terminal outcome wins: a job cancelled while queued cannot
//!   later publish its audio as a fresh result (stale audio never becomes
//!   current);
//! * the audio is addressed by `render_id`, never by "the latest render", so a
//!   client cannot accidentally play an older take.
//!
//! Process-local and not durable, like the sessions it belongs to (§6.12).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::render_service::ProgressCounters;

/// Terminal jobs are kept this long so a client can still fetch the wav after
/// a slow poll (§6.13 item 4).
pub const RETAIN_TERMINAL: Duration = Duration::from_secs(10 * 60);

/// Upper bound on retained terminal jobs, so a long-lived server cannot grow
/// without limit. Oldest terminal jobs are dropped first.
pub const MAX_TERMINAL_JOBS: usize = 64;

/// Lifecycle of one render job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    /// Accepted, not yet picked up by the render worker.
    Queued,
    /// The worker has started this job (the pipeline has reported its plan).
    Running,
    /// Finished; audio is available.
    Done,
    /// Finished with an error; `render_error` in the status payload.
    Failed,
    /// Stopped on request; no audio will ever be published for it.
    Cancelled,
}

impl JobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            JobStatus::Queued => "queued",
            JobStatus::Running => "running",
            JobStatus::Done => "done",
            JobStatus::Failed => "failed",
            JobStatus::Cancelled => "cancelled",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            JobStatus::Done | JobStatus::Failed | JobStatus::Cancelled
        )
    }
}

/// One job as stored by the table.
pub struct RenderJobEntry {
    pub render_id: String,
    /// The session this job was started from (§6.13 item 1).
    pub session_id: String,
    /// The revision whose snapshot this job renders. Fixed for the job's life.
    pub revision: u64,
    pub engine: String,
    pub track: i32,
    /// This job's cancel flag — shared with the worker, unique to the job.
    pub cancel: Arc<AtomicBool>,
    /// Chunk counters written by the worker.
    pub progress: Arc<ProgressCounters>,
    created: Instant,
    /// `None` while the job is queued or running.
    terminal: Option<JobStatus>,
    finished: Option<Instant>,
    /// `(code, message)` from §6.14 for a `Failed` job.
    error: Option<(String, String)>,
    /// Rendered samples, published only by a `Done` transition.
    audio: Option<Arc<Vec<f32>>>,
    /// Audio metadata (filled only when audio is published).
    frames: Option<usize>,
    sample_rate: Option<u32>,
    channels: Option<u16>,
    elapsed_ms: Option<u64>,
}

impl RenderJobEntry {
    /// Queued until the worker reports its plan, then running; a terminal
    /// status always wins.
    pub fn status(&self) -> JobStatus {
        match self.terminal {
            Some(status) => status,
            None if self.progress.is_started() => JobStatus::Running,
            None => JobStatus::Queued,
        }
    }

    pub fn created_at(&self) -> Instant {
        self.created
    }

    fn is_terminal(&self) -> bool {
        self.terminal.is_some()
    }
}

/// The live job table.
#[derive(Default)]
pub struct RenderJobTable {
    jobs: Mutex<HashMap<String, RenderJobEntry>>,
    next_id: AtomicU64,
}

impl RenderJobTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, RenderJobEntry>> {
        // A panic in one handler must not take the whole server down; the
        // table holds only data, so the guard is safe to recover.
        self.jobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Register a job. Returns its opaque id and the per-job handles the
    /// worker needs. The caller must already have captured the immutable
    /// snapshot at `revision`.
    pub fn begin(&self, session_id: &str, revision: u64, engine: &str, track: i32) -> RenderJob {
        let seq = self.next_id.fetch_add(1, Ordering::Relaxed);
        let render_id = new_render_id(session_id, seq);
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(ProgressCounters::default());
        let entry = RenderJobEntry {
            render_id: render_id.clone(),
            session_id: session_id.to_string(),
            revision,
            engine: engine.to_string(),
            track,
            cancel: cancel.clone(),
            progress: progress.clone(),
            created: Instant::now(),
            terminal: None,
            finished: None,
            error: None,
            audio: None,
            frames: None,
            sample_rate: None,
            channels: None,
            elapsed_ms: None,
        };
        {
            let mut jobs = self.lock();
            jobs.insert(render_id.clone(), entry);
        }
        // Retention is enforced on insert: a client that never polls again
        // cannot make the table grow.
        self.sweep_terminal(RETAIN_TERMINAL, MAX_TERMINAL_JOBS);
        RenderJob {
            render_id,
            cancel,
            progress,
        }
    }

    pub fn get(&self, render_id: &str) -> Option<StatusView> {
        self.lock().get(render_id).map(StatusView::from)
    }

    /// Audio for a finished job: the samples plus the revision they render, so
    /// a client can tell whether the take still matches its session. `None`
    /// while running, and also for a job that failed or was cancelled — those
    /// never publish audio.
    pub fn audio(&self, render_id: &str) -> Option<(Arc<Vec<f32>>, u64, u16)> {
        let jobs = self.lock();
        let entry = jobs.get(render_id)?;
        let audio = entry.audio.clone()?;
        Some((audio, entry.revision, entry.channels.unwrap_or(1)))
    }

    /// The job's own revision, whether or not it finished.
    pub fn revision_of(&self, render_id: &str) -> Option<u64> {
        self.lock().get(render_id).map(|e| e.revision)
    }

    /// Publish a successful result. Returns `false` when the job is already
    /// terminal (cancelled or failed): the audio is then dropped instead of
    /// overwriting the outcome — this is what keeps a cancelled render from
    /// becoming the current audio.
    pub fn finish_ok(&self, render_id: &str, samples: Vec<f32>, channels: u16) -> bool {
        let mut jobs = self.lock();
        let Some(entry) = jobs.get_mut(render_id) else {
            return false;
        };
        if entry.is_terminal() {
            return false;
        }
        entry.terminal = Some(JobStatus::Done);
        entry.finished = Some(Instant::now());
        entry.elapsed_ms = Some(elapsed_ms(entry.created));
        let channels = channels.max(1);
        entry.frames = Some(samples.len() / channels as usize);
        entry.sample_rate = Some(synth_cli::pipeline::SAMPLE_RATE);
        entry.channels = Some(channels);
        entry.audio = Some(Arc::new(samples));
        true
    }

    /// Publish a failure. Returns `false` when the job is already terminal.
    pub fn finish_err(&self, render_id: &str, code: &str, message: &str) -> bool {
        let mut jobs = self.lock();
        let Some(entry) = jobs.get_mut(render_id) else {
            return false;
        };
        if entry.is_terminal() {
            return false;
        }
        entry.terminal = Some(JobStatus::Failed);
        entry.finished = Some(Instant::now());
        entry.elapsed_ms = Some(elapsed_ms(entry.created));
        entry.error = Some((code.to_string(), message.to_string()));
        true
    }

    /// Cancel one job. Returns `None` when the id is unknown; `Some(status)`
    /// with the resulting status otherwise.
    ///
    /// Cancelling an already-terminal job is a **no-op that reports the
    /// existing outcome** — there is nothing to stop, and pretending the job
    /// became `cancelled` would misreport a finished render.
    pub fn cancel(&self, render_id: &str) -> Option<JobStatus> {
        let mut jobs = self.lock();
        let entry = jobs.get_mut(render_id)?;
        if entry.is_terminal() {
            return Some(entry.status());
        }
        entry.cancel.store(true, Ordering::Relaxed);
        entry.terminal = Some(JobStatus::Cancelled);
        entry.finished = Some(Instant::now());
        entry.elapsed_ms = Some(elapsed_ms(entry.created));
        entry.error = Some((
            "render_cancelled".to_string(),
            "render cancelled".to_string(),
        ));
        Some(JobStatus::Cancelled)
    }

    /// Cancel every job of `session_id` (explicit close, §6.12). Returns the
    /// ids that were newly cancelled — already-terminal jobs are left as they
    /// are.
    pub fn cancel_session(&self, session_id: &str) -> Vec<String> {
        let mut jobs = self.lock();
        let ids: Vec<String> = jobs
            .iter()
            .filter(|(_, entry)| entry.session_id == session_id && !entry.is_terminal())
            .map(|(id, _)| id.clone())
            .collect();
        let now = Instant::now();
        for id in &ids {
            if let Some(entry) = jobs.get_mut(id) {
                entry.cancel.store(true, Ordering::Relaxed);
                entry.terminal = Some(JobStatus::Cancelled);
                entry.finished = Some(now);
                entry.elapsed_ms = Some(elapsed_ms(entry.created));
                entry.error = Some((
                    "render_cancelled".to_string(),
                    "render cancelled because the session was closed".to_string(),
                ));
            }
        }
        ids
    }

    /// Whether `session_id` still has a job that has not reached a terminal
    /// state (used by the TTL policy, §6.12).
    pub fn active_for_session(&self, session_id: &str) -> usize {
        self.lock()
            .values()
            .filter(|entry| entry.session_id == session_id && !entry.is_terminal())
            .count()
    }

    /// Drop terminal jobs older than `max_age`, then the oldest terminal jobs
    /// beyond `max_kept`. Active jobs are never dropped.
    pub fn sweep_terminal(&self, max_age: Duration, max_kept: usize) {
        let now = Instant::now();
        let mut jobs = self.lock();
        jobs.retain(|_, entry| {
            !entry.is_terminal()
                || entry.finished.map_or(true, |finished| {
                    now.saturating_duration_since(finished) < max_age
                })
        });
        let terminal = jobs.values().filter(|e| e.is_terminal()).count();
        if terminal > max_kept {
            let mut finished: Vec<(String, Instant)> = jobs
                .iter()
                .filter(|(_, entry)| entry.is_terminal())
                .map(|(id, entry)| (id.clone(), entry.finished.unwrap_or(now)))
                .collect();
            finished.sort_by_key(|(_, at)| *at);
            for (id, _) in finished.into_iter().take(terminal - max_kept) {
                jobs.remove(&id);
            }
        }
    }
}

/// A read-only view of one job, so callers cannot hold the table lock while
/// they build a response or touch another lock.
#[derive(Debug, Clone)]
pub struct StatusView {
    pub render_id: String,
    pub session_id: String,
    pub revision: u64,
    pub engine: String,
    pub track: i32,
    pub status: JobStatus,
    pub done_chunks: usize,
    pub total_chunks: usize,
    pub elapsed_ms: u64,
    pub error: Option<(String, String)>,
    /// Present only after successful completion, per §6.13 item 4.
    pub sample_rate: Option<u32>,
    pub frames: Option<usize>,
    pub channels: Option<u16>,
}

impl StatusView {
    pub fn to_json(&self) -> Value {
        json!({
            "render_id": self.render_id,
            "session_id": self.session_id,
            "revision": self.revision,
            "engine": self.engine,
            "track": self.track,
            "status": self.status.as_str(),
            "done_chunks": self.done_chunks,
            "total_chunks": self.total_chunks,
            "elapsed_ms": self.elapsed_ms,
            "sample_rate": self.sample_rate,
            "frames": self.frames,
            "channels": self.channels,
            "error": self.error.as_ref().map(|(code, message)| json!({
                "code": code,
                "message": message,
            })),
        })
    }
}

impl From<&RenderJobEntry> for StatusView {
    fn from(entry: &RenderJobEntry) -> Self {
        let (done_chunks, total_chunks) = entry.progress.snapshot();
        Self {
            render_id: entry.render_id.clone(),
            session_id: entry.session_id.clone(),
            revision: entry.revision,
            engine: entry.engine.clone(),
            track: entry.track,
            status: entry.status(),
            done_chunks,
            total_chunks,
            elapsed_ms: entry
                .elapsed_ms
                .unwrap_or_else(|| elapsed_ms(entry.created)),
            error: entry.error.clone(),
            sample_rate: entry.sample_rate,
            frames: entry.frames,
            channels: entry.channels,
        }
    }
}

/// The handles a caller needs to run a registered job.
pub struct RenderJob {
    pub render_id: String,
    pub cancel: Arc<AtomicBool>,
    pub progress: Arc<ProgressCounters>,
}

fn elapsed_ms(since: Instant) -> u64 {
    since.elapsed().as_millis() as u64
}

/// An opaque render id. Clients must treat it as a token, not parse it; the
/// sequence part only exists so two jobs in one process cannot collide.
fn new_render_id(session_id: &str, seq: u64) -> String {
    let tag: String = session_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect();
    format!("rj-{tag}-{seq:08x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_starts_queued_and_reports_its_revision() {
        let table = RenderJobTable::new();
        let job = table.begin("session-a", 7, "r1", 0);
        let status = table.get(&job.render_id).expect("job");

        assert_eq!(status.status, JobStatus::Queued);
        assert_eq!(
            status.revision, 7,
            "the job renders the revision it began at"
        );
        assert_eq!(status.session_id, "session-a");
        assert_eq!(status.done_chunks, 0);
        assert_eq!(status.total_chunks, 0);
    }

    #[test]
    fn progress_moves_a_job_to_running_and_never_regresses() {
        let table = RenderJobTable::new();
        let job = table.begin("s", 0, "r1", 0);
        job.progress.report(0, 12);
        assert_eq!(
            table.get(&job.render_id).unwrap().status,
            JobStatus::Running
        );
        job.progress.report(5, 12);
        // Concurrent Rayon callbacks can arrive out of order.
        job.progress.report(3, 12);
        let status = table.get(&job.render_id).unwrap();
        assert_eq!(status.done_chunks, 5);
        assert_eq!(status.total_chunks, 12);
    }

    #[test]
    fn a_finished_job_publishes_audio_once() {
        let table = RenderJobTable::new();
        let job = table.begin("s", 3, "r1", 0);

        assert!(table.finish_ok(&job.render_id, vec![0.5, -0.5], 1));
        let status = table.get(&job.render_id).unwrap();
        assert_eq!(status.status, JobStatus::Done);
        assert_eq!(table.audio(&job.render_id).unwrap().0.len(), 2);

        // A second completion (or an error) cannot overwrite the outcome.
        assert!(!table.finish_ok(&job.render_id, vec![9.0], 1));
        assert!(!table.finish_err(&job.render_id, "render_error", "late"));
        assert_eq!(table.audio(&job.render_id).unwrap().0[0], 0.5);
    }

    #[test]
    fn a_cancelled_job_never_publishes_audio() {
        let table = RenderJobTable::new();
        let job = table.begin("s", 1, "r1", 0);

        assert_eq!(table.cancel(&job.render_id), Some(JobStatus::Cancelled));
        assert!(
            job.cancel.load(Ordering::Relaxed),
            "the worker is told to stop"
        );

        // The worker finishes late: the audio must be dropped, not published.
        assert!(!table.finish_ok(&job.render_id, vec![1.0, 2.0], 1));
        assert_eq!(
            table.get(&job.render_id).unwrap().status,
            JobStatus::Cancelled
        );
        assert!(
            table.audio(&job.render_id).is_none(),
            "stale audio must never become current"
        );
    }

    #[test]
    fn cancel_is_per_job() {
        let table = RenderJobTable::new();
        let first = table.begin("s", 0, "r1", 0);
        let second = table.begin("s", 0, "r1", 0);

        table.cancel(&first.render_id);

        assert!(first.cancel.load(Ordering::Relaxed));
        assert!(
            !second.cancel.load(Ordering::Relaxed),
            "cancelling one job must not touch a queued neighbour"
        );
        assert_eq!(
            table.get(&second.render_id).unwrap().status,
            JobStatus::Queued
        );
    }

    #[test]
    fn cancelling_a_terminal_job_reports_its_real_outcome() {
        let table = RenderJobTable::new();
        let job = table.begin("s", 0, "r1", 0);
        table.finish_ok(&job.render_id, vec![0.1], 1);

        assert_eq!(table.cancel(&job.render_id), Some(JobStatus::Done));
        assert_eq!(table.get(&job.render_id).unwrap().status, JobStatus::Done);
        assert!(table.audio(&job.render_id).is_some());
        assert_eq!(table.cancel("rj-nope-0"), None);
    }

    #[test]
    fn closing_a_session_cancels_its_jobs_only() {
        let table = RenderJobTable::new();
        let mine = table.begin("session-a", 0, "r1", 0);
        let other = table.begin("session-b", 0, "r1", 0);

        assert_eq!(
            table.cancel_session("session-a"),
            vec![mine.render_id.clone()]
        );
        assert!(mine.cancel.load(Ordering::Relaxed));
        assert!(!other.cancel.load(Ordering::Relaxed));
        assert_eq!(table.active_for_session("session-a"), 0);
        assert_eq!(table.active_for_session("session-b"), 1);
    }

    #[test]
    fn a_finished_job_is_not_counted_as_active() {
        let table = RenderJobTable::new();
        let job = table.begin("s", 0, "r1", 0);
        assert_eq!(table.active_for_session("s"), 1);
        table.finish_err(&job.render_id, "render_error", "boom");
        assert_eq!(table.active_for_session("s"), 0);
    }

    #[test]
    fn retention_drops_old_terminal_jobs_but_never_active_ones() {
        let table = RenderJobTable::new();
        let active = table.begin("s", 0, "r1", 0);
        let done_a = table.begin("s", 0, "r1", 0);
        let done_b = table.begin("s", 0, "r1", 0);
        table.finish_ok(&done_a.render_id, vec![0.0], 1);
        table.finish_ok(&done_b.render_id, vec![0.0], 1);

        // Keep the retention window wide, but cap terminal results at one.
        table.sweep_terminal(Duration::from_secs(60), 1);

        assert_eq!(table.len(), 2, "the active job plus one retained result");
        assert!(
            table.get(&active.render_id).is_some(),
            "active jobs are kept"
        );
    }

    #[test]
    fn status_payload_carries_session_id_and_revision() {
        let table = RenderJobTable::new();
        let job = table.begin("session-xyz", 42, "r2", 1);
        let value = table.get(&job.render_id).unwrap().to_json();

        assert_eq!(value["session_id"], "session-xyz");
        assert_eq!(value["revision"], 42);
        assert_eq!(value["render_id"], job.render_id);
        assert_eq!(value["status"], "queued");
        assert_eq!(value["track"], 1);
    }
}
