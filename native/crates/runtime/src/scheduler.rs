//! Generic job scheduler: FIFO queue, cancellation, progress, retry.
//!
//! The scheduler is a pure data structure (no threads, no IO) so it is
//! trivially testable and embeddable: a worker loop (or a `Mutex<Scheduler>`
//! shared across threads) drives it by calling [`Scheduler::next_ready`],
//! then [`Scheduler::complete`] / [`Scheduler::fail`]. It is generic over
//! the job input type `T`, so it does not depend on the `feed` crate —
//! `T` will be `feed::RenderInput` once that crate lands.
//!
//! # Retry
//!
//! [`Job::max_attempts`] controls how many times a failing job is
//! re-queued. `1` (the default) means the first failure is final;
//! `3` means two automatic retries. [`Scheduler::fail`] re-queues the job
//! at the back of the FIFO when attempts remain.

use std::collections::{HashMap, VecDeque};

/// Opaque job identifier, handed out sequentially.
pub type JobId = u64;

/// Boxed progress callback: invoked with a `0.0..=1.0` fraction.
pub type ProgressCallback = Box<dyn FnMut(f32) + Send>;

/// Lifecycle state of a job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobStatus {
    /// Enqueued, not yet picked up by a worker.
    Queued,
    /// Picked up by a worker ([`Scheduler::next_ready`]); may be cancelled.
    Running,
    /// [`Scheduler::complete`] was called.
    Completed,
    /// Exhausted its retries; carries the last error message.
    Failed(String),
    /// [`Scheduler::cancel`] was called before completion.
    Cancelled,
}

/// A job submitted to the [`Scheduler`].
///
/// The scheduler keeps `input` inside the job while it is queued and hands
/// it to the worker (by value) on [`Scheduler::next_ready`]; the worker
/// returns it via [`Scheduler::fail`] when a retry is needed.
pub struct Job<T> {
    pub input: T,
    /// Maximum total executions before the job is marked `Failed`.
    /// `1` = no retries (default), `3` = up to three executions
    /// (two automatic retries). [`Scheduler::attempts`] counts failures
    /// so far.
    pub max_attempts: u32,
    on_progress: Option<ProgressCallback>,
}

impl<T> Job<T> {
    /// Create a job with default settings (`max_attempts = 1`, no callback).
    pub fn new(input: T) -> Self {
        Self {
            input,
            max_attempts: 1,
            on_progress: None,
        }
    }

    /// Set the number of attempts (clamped to at least 1).
    pub fn with_max_attempts(mut self, attempts: u32) -> Self {
        self.max_attempts = attempts.max(1);
        self
    }

    /// Attach a progress callback, fired on [`Scheduler::report_progress`]
    /// and [`Scheduler::complete`] (with `1.0`).
    pub fn with_progress(mut self, callback: impl FnMut(f32) + Send + 'static) -> Self {
        self.on_progress = Some(Box::new(callback));
        self
    }
}

struct JobEntry<T> {
    input: Option<T>,
    status: JobStatus,
    attempts: u32,
    max_attempts: u32,
    progress: f32,
    on_progress: Option<ProgressCallback>,
}

/// FIFO job queue with cancellation, per-job progress and retry.
///
/// Invariant: the queue contains exactly the ids of jobs whose status is
/// `Queued`; `next_ready` pops them in FIFO order, `cancel` removes them,
/// `fail` re-appends them (retry).
pub struct Scheduler<T> {
    next_id: JobId,
    jobs: HashMap<JobId, JobEntry<T>>,
    queue: VecDeque<JobId>,
}

impl<T: std::fmt::Debug> std::fmt::Debug for Scheduler<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scheduler")
            .field("next_id", &self.next_id)
            .field("tracked_jobs", &self.jobs.len())
            .field("pending", &self.queue.len())
            .finish()
    }
}

impl<T> Default for Scheduler<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Scheduler<T> {
    /// Create an empty scheduler.
    pub fn new() -> Self {
        Self {
            next_id: 0,
            jobs: HashMap::new(),
            queue: VecDeque::new(),
        }
    }

    /// Enqueue a job; returns its id. The id is valid until the job is
    /// completed, failed or cancelled (after that the id is never reused).
    pub fn enqueue(&mut self, job: Job<T>) -> JobId {
        let id = self.next_id;
        self.next_id += 1;
        self.jobs.insert(
            id,
            JobEntry {
                input: Some(job.input),
                status: JobStatus::Queued,
                attempts: 0,
                max_attempts: job.max_attempts,
                progress: 0.0,
                on_progress: job.on_progress,
            },
        );
        self.queue.push_back(id);
        id
    }

    /// Pop the next queued job in FIFO order, moving its input out and
    /// marking it `Running`. Returns `None` when the queue is empty.
    ///
    /// The worker checks [`Scheduler::is_cancelled`] while executing and
    /// finishes with [`Scheduler::complete`] or [`Scheduler::fail`].
    pub fn next_ready(&mut self) -> Option<(JobId, T)> {
        let id = self.queue.pop_front()?;
        let entry = self.jobs.get_mut(&id).expect("queue id must exist");
        debug_assert_eq!(entry.status, JobStatus::Queued);
        entry.status = JobStatus::Running;
        // An entry without input can only appear if the same id was
        // dequeued twice; next_ready is not re-entrant per id.
        let input = entry.input.take().expect("queued job must have input");
        Some((id, input))
    }

    /// Mark a job completed (progress `1.0`, callback fired).
    ///
    /// No-op (returns `false`) if the job is already `Cancelled`, `Failed`
    /// or `Completed` — in particular, a worker that finished after
    /// [`Scheduler::cancel`] must not resurrect a cancelled job.
    pub fn complete(&mut self, id: JobId) -> bool {
        let Some(entry) = self.jobs.get_mut(&id) else {
            return false;
        };
        if entry.status == JobStatus::Cancelled || matches!(entry.status, JobStatus::Failed(_)) {
            return false;
        }
        if entry.status == JobStatus::Completed {
            return false;
        }
        entry.status = JobStatus::Completed;
        entry.progress = 1.0;
        Self::fire_progress(entry);
        true
    }

    /// Report a worker failure for a job, handing the input back.
    ///
    /// The input was moved out by [`Scheduler::next_ready`]; return it here
    /// so a retry can re-execute the job. If the job has failures left
    /// (`attempts < max_attempts`) it is re-queued at the back of the FIFO
    /// with status `Queued`; otherwise it becomes `Failed(error)` and
    /// `input` is dropped.
    ///
    /// Returns `true` if the job is still tracked, `false` if `id` is
    /// unknown, already finished, or was cancelled.
    pub fn fail(&mut self, id: JobId, input: T, error: impl Into<String>) -> bool {
        let Some(entry) = self.jobs.get_mut(&id) else {
            return false;
        };
        if entry.status == JobStatus::Cancelled
            || entry.status == JobStatus::Completed
            || matches!(entry.status, JobStatus::Failed(_))
        {
            return false;
        }
        entry.attempts += 1;
        if entry.attempts < entry.max_attempts {
            entry.status = JobStatus::Queued;
            entry.input = Some(input);
            self.queue.push_back(id);
        } else {
            entry.status = JobStatus::Failed(error.into());
        }
        true
    }

    /// Cancel a job.
    ///
    /// * `Queued` — removed from the queue, status `Cancelled`.
    /// * `Running` — status `Cancelled`; the worker is expected to observe
    ///   [`Scheduler::is_cancelled`] and stop (then `complete`/`fail` are
    ///   no-ops).
    /// * otherwise — no-op.
    ///
    /// Returns `true` if the job was actually cancelled.
    pub fn cancel(&mut self, id: JobId) -> bool {
        let Some(entry) = self.jobs.get_mut(&id) else {
            return false;
        };
        match entry.status {
            JobStatus::Queued => {
                // Remove from queue: find and drop the first occurrence.
                if let Some(pos) = self.queue.iter().position(|q| *q == id) {
                    self.queue.remove(pos);
                }
                entry.status = JobStatus::Cancelled;
                true
            }
            JobStatus::Running => {
                entry.status = JobStatus::Cancelled;
                true
            }
            _ => false,
        }
    }

    /// Update a running job's progress (clamped to `0.0..=1.0`) and fire
    /// its callback. Returns `false` if the job is unknown.
    pub fn report_progress(&mut self, id: JobId, progress: f32) -> bool {
        let Some(entry) = self.jobs.get_mut(&id) else {
            return false;
        };
        entry.progress = progress.clamp(0.0, 1.0);
        Self::fire_progress(entry);
        true
    }

    /// Whether a running job was cancelled (worker poll point).
    pub fn is_cancelled(&self, id: JobId) -> bool {
        matches!(self.jobs.get(&id).map(|e| &e.status), Some(JobStatus::Cancelled))
    }

    /// Current status of a job.
    pub fn status(&self, id: JobId) -> Option<&JobStatus> {
        self.jobs.get(&id).map(|e| &e.status)
    }

    /// Current progress of a job (`0.0..=1.0`).
    pub fn progress(&self, id: JobId) -> Option<f32> {
        self.jobs.get(&id).map(|e| e.progress)
    }

    /// Number of failures a job has accumulated so far (0 = no failure
    /// yet; a job with `max_attempts = 3` fails definitively at 3).
    pub fn attempts(&self, id: JobId) -> Option<u32> {
        self.jobs.get(&id).map(|e| e.attempts)
    }

    /// Number of jobs still waiting in the queue.
    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    /// Total number of tracked jobs (all statuses).
    pub fn len(&self) -> usize {
        self.jobs.len()
    }

    /// Whether no jobs are tracked at all.
    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }

    /// Drop all jobs (queued, running and finished). Their inputs are
    /// dropped, callbacks are not fired.
    pub fn clear(&mut self) {
        self.jobs.clear();
        self.queue.clear();
    }

    /// Drain every job synchronously: for each job in FIFO order, call
    /// `executor(input)`; `Ok(())` completes the job, `Err(e)` fails it
    /// (with retry). Cancelled jobs are skipped.
    ///
    /// Requires `T: Clone` because the executor consumes the input per
    /// attempt; a retry re-invokes it with a clone. Returns the outcomes
    /// in completion order. Useful for single-threaded embedding and
    /// tests; parallel workers should drive the scheduler with
    /// `next_ready`/`complete`/`fail` directly (no `Clone` needed there).
    pub fn run_all<E>(&mut self, mut executor: E) -> Vec<(JobId, Result<(), String>)>
    where
        T: Clone,
        E: FnMut(T) -> Result<(), String>,
    {
        let mut results = Vec::new();
        while let Some((id, input)) = self.next_ready() {
            if self.is_cancelled(id) {
                continue;
            }
            match executor(input.clone()) {
                Ok(()) => {
                    self.complete(id);
                    results.push((id, Ok(())));
                }
                Err(error) => {
                    let message = error.clone();
                    self.fail(id, input, error);
                    // A retried job stays in the queue and is picked up by
                    // the next loop iteration; only a definitive failure
                    // produces a result here.
                    if matches!(self.status(id), Some(JobStatus::Failed(_))) {
                        results.push((id, Err(message)));
                    }
                }
            }
        }
        results
    }

    fn fire_progress(entry: &mut JobEntry<T>) {
        let progress = entry.progress;
        if let Some(callback) = entry.on_progress.as_mut() {
            callback(progress);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn collect_progress() -> (Arc<Mutex<Vec<f32>>>, impl FnMut(f32) + Send + 'static) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let log2 = log.clone();
        (log, move |p| log2.lock().unwrap().push(p))
    }

    #[test]
    fn fifo_order() {
        let mut sched: Scheduler<&str> = Scheduler::new();
        let a = sched.enqueue(Job::new("a"));
        let b = sched.enqueue(Job::new("b"));
        let c = sched.enqueue(Job::new("c"));
        assert_eq!(sched.next_ready(), Some((a, "a")));
        assert_eq!(sched.next_ready(), Some((b, "b")));
        assert_eq!(sched.next_ready(), Some((c, "c")));
        assert_eq!(sched.next_ready(), None);
    }

    #[test]
    fn progress_callback_and_complete() {
        let mut sched: Scheduler<u32> = Scheduler::new();
        let (log, cb) = collect_progress();
        let id = sched.enqueue(Job::new(7).with_progress(cb));
        assert!(sched.report_progress(id, 0.25));
        assert!(sched.report_progress(id, 0.6));
        assert_eq!(sched.progress(id), Some(0.6));
        let _ = sched.next_ready();
        assert!(sched.complete(id));
        assert_eq!(sched.status(id), Some(&JobStatus::Completed));
        assert_eq!(sched.progress(id), Some(1.0));
        assert_eq!(*log.lock().unwrap(), vec![0.25, 0.6, 1.0]);
    }

    #[test]
    fn progress_clamped() {
        let mut sched: Scheduler<u32> = Scheduler::new();
        let (log, cb) = collect_progress();
        let id = sched.enqueue(Job::new(1).with_progress(cb));
        sched.report_progress(id, -5.0);
        assert_eq!(sched.progress(id), Some(0.0));
        sched.report_progress(id, 7.0);
        assert_eq!(sched.progress(id), Some(1.0));
        assert_eq!(*log.lock().unwrap(), vec![0.0, 1.0]);
    }

    #[test]
    fn cancel_queued_job() {
        let mut sched: Scheduler<u32> = Scheduler::new();
        let a = sched.enqueue(Job::new(1));
        let b = sched.enqueue(Job::new(2));
        assert!(sched.cancel(a));
        assert_eq!(sched.status(a), Some(&JobStatus::Cancelled));
        // b is now first in line.
        assert_eq!(sched.next_ready(), Some((b, 2)));
        assert_eq!(sched.pending(), 0);
        // Cancelling again is a no-op.
        assert!(!sched.cancel(a));
        // Unknown ids are a no-op.
        assert!(!sched.cancel(999));
    }

    #[test]
    fn cancel_running_job_then_complete_is_noop() {
        let mut sched: Scheduler<u32> = Scheduler::new();
        let id = sched.enqueue(Job::new(1));
        let (dequeued, input) = sched.next_ready().unwrap();
        assert_eq!((dequeued, input), (id, 1));
        assert!(sched.cancel(id));
        assert!(sched.is_cancelled(id));
        // The worker finishing late must not resurrect the job.
        assert!(!sched.complete(id));
        assert_eq!(sched.status(id), Some(&JobStatus::Cancelled));
        assert!(!sched.fail(id, 1, "late failure"));
        assert_eq!(sched.status(id), Some(&JobStatus::Cancelled));
    }

    #[test]
    fn retry_until_max_attempts() {
        let mut sched: Scheduler<u32> = Scheduler::new();
        let id = sched.enqueue(Job::new(42).with_max_attempts(3));
        let mut fail_times = 0;
        let results = sched.run_all(|_| {
            fail_times += 1;
            Err(format!("boom {fail_times}"))
        });
        assert_eq!(fail_times, 3);
        assert_eq!(sched.attempts(id), Some(3));
        assert_eq!(sched.status(id), Some(&JobStatus::Failed("boom 3".into())));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, id);
        assert_eq!(results[0].1, Err("boom 3".to_string()));
        assert_eq!(sched.pending(), 0);
    }

    #[test]
    fn retry_then_succeed() {
        let mut sched: Scheduler<u32> = Scheduler::new();
        let id = sched.enqueue(Job::new(42).with_max_attempts(3));
        let mut attempts = 0;
        let results = sched.run_all(|input| {
            attempts += 1;
            if attempts < 3 {
                Err("transient".into())
            } else {
                assert_eq!(input, 42);
                Ok(())
            }
        });
        assert_eq!(attempts, 3);
        assert_eq!(sched.status(id), Some(&JobStatus::Completed));
        assert_eq!(sched.progress(id), Some(1.0));
        assert_eq!(results, vec![(id, Ok(()))]);
    }

    #[test]
    fn default_is_no_retry() {
        let mut sched: Scheduler<u32> = Scheduler::new();
        let id = sched.enqueue(Job::new(1));
        let mut calls = 0;
        sched.run_all(|_| {
            calls += 1;
            Err("nope".into())
        });
        assert_eq!(calls, 1);
        assert_eq!(sched.attempts(id), Some(1));
        assert_eq!(sched.status(id), Some(&JobStatus::Failed("nope".into())));
    }

    #[test]
    fn run_all_skips_cancelled() {
        let mut sched: Scheduler<u32> = Scheduler::new();
        let doomed = sched.enqueue(Job::new(1));
        let fine = sched.enqueue(Job::new(2));
        sched.cancel(doomed);
        let mut executed = Vec::new();
        let results = sched.run_all(|input| {
            executed.push(input);
            Ok(())
        });
        assert_eq!(executed, vec![2]);
        assert_eq!(results, vec![(fine, Ok(()))]);
        assert_eq!(sched.status(doomed), Some(&JobStatus::Cancelled));
    }

    #[test]
    fn manual_drive_with_retry_and_progress() {
        // Worker-loop style usage: next_ready -> work -> complete/fail.
        let mut sched: Scheduler<u32> = Scheduler::new();
        let (log, cb) = collect_progress();
        let id = sched.enqueue(Job::new(10).with_max_attempts(2).with_progress(cb));

        let (id1, input) = sched.next_ready().unwrap();
        assert_eq!((id1, input), (id, 10));
        sched.report_progress(id, 0.5);
        sched.fail(id, 10, "first attempt failed");
        assert_eq!(sched.status(id), Some(&JobStatus::Queued));

        let (id2, input) = sched.next_ready().unwrap();
        assert_eq!((id2, input), (id, 10));
        sched.report_progress(id, 0.9);
        sched.complete(id);
        assert_eq!(sched.status(id), Some(&JobStatus::Completed));
        assert_eq!(*log.lock().unwrap(), vec![0.5, 0.9, 1.0]);
    }

    #[test]
    fn ids_are_unique_and_monotonic() {
        let mut sched: Scheduler<u32> = Scheduler::new();
        let ids: Vec<JobId> = (0..5).map(|i| sched.enqueue(Job::new(i))).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted);
        assert_eq!(sched.len(), 5);
        assert!(!sched.is_empty());
        sched.clear();
        assert!(sched.is_empty());
        assert_eq!(sched.pending(), 0);
    }
}
