//! Server-side project sessions (`docs/CONTRACT-v1.md` §6, §6.12).
//!
//! Sessions are **process-local and not durable**: a restart loses them and the
//! client reopens from the persisted `.ustx` + sidecar. The registry owns the
//! lifecycle rules decided on 2026-09-29:
//!
//! * idle TTL defaults to 30 minutes and **every logical operation resets it**;
//! * a session with a render job in `queued` or `running` **never expires**;
//! * expiry closes the session exactly like an explicit close — unsaved
//!   committed state is lost.
//!
//! Time is passed in as an `Instant` so the policy is testable without sleeping.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use project_session::v2::{Capabilities, RenderCapabilities};
use project_session::Session;
use serde::Serialize;

/// Idle timeout before a session is closed (§6.12).
pub const DEFAULT_IDLE_TTL: Duration = Duration::from_secs(30 * 60);

/// One live session plus its lifecycle bookkeeping.
pub struct SessionEntry {
    pub session: Session,
    /// Browser-local key used only for server-side crash recovery.
    pub recovery_id: Option<String>,
    /// Named project association, when this session was opened from or saved
    /// as a stored project. An imported unsaved document keeps this unset.
    pub store_name: Option<String>,
    /// Last committed revision written to recovery storage.
    pub checkpointed_revision: Option<u64>,
    /// When the idle TTL started counting (reset by activity).
    idle_since: Instant,
    /// Render jobs in `queued`/`running` state. While this is non-zero the
    /// session is exempt from expiry.
    active_render_jobs: usize,
}

impl SessionEntry {
    fn new(
        session: Session,
        now: Instant,
        recovery_id: Option<String>,
        store_name: Option<String>,
    ) -> Self {
        Self {
            session,
            recovery_id,
            store_name,
            checkpointed_revision: None,
            idle_since: now,
            active_render_jobs: 0,
        }
    }

    /// Record activity, restarting the idle countdown.
    pub fn touch(&mut self, now: Instant) {
        self.idle_since = now;
    }

    /// Register a render job that is queued or running.
    pub fn begin_render_job(&mut self) {
        self.active_render_jobs += 1;
    }

    /// Register that a render job finished or was cancelled.
    pub fn end_render_job(&mut self) {
        self.active_render_jobs = self.active_render_jobs.saturating_sub(1);
    }

    pub fn active_render_jobs(&self) -> usize {
        self.active_render_jobs
    }

    /// Whether this entry is past `ttl` at `now` and may be closed.
    fn is_expired(&self, now: Instant, ttl: Duration) -> bool {
        self.active_render_jobs == 0 && now.saturating_duration_since(self.idle_since) >= ttl
    }
}

/// What a session can be opened from (§6.11).
pub enum SessionSource {
    /// A brand-new document.
    New {
        name: Option<String>,
        bpm: Option<f64>,
    },
    /// A stored project: its `.ustx` plus an optional sidecar.
    Stored {
        name: String,
        yaml: String,
        sidecar: Option<String>,
        pronunciation_sidecar: Option<String>,
    },
}

/// A live session table.
pub struct SessionRegistry {
    ttl: Duration,
    sessions: HashMap<String, SessionEntry>,
}

impl SessionRegistry {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            sessions: HashMap::new(),
        }
    }

    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// Open a session from `source` and store it. Returns its id.
    pub fn open(
        &mut self,
        source: SessionSource,
        seed: u64,
        now: Instant,
    ) -> Result<String, String> {
        self.open_recoverable(source, seed, now, None, None)
    }

    /// Open a session and associate it with an optional durable recovery slot.
    pub fn open_recoverable(
        &mut self,
        source: SessionSource,
        seed: u64,
        now: Instant,
        recovery_id: Option<String>,
        store_name: Option<String>,
    ) -> Result<String, String> {
        let session = match source {
            SessionSource::New { name, bpm } => {
                Session::new_with_metadata(name.as_deref(), bpm, seed)?
            }
            SessionSource::Stored {
                name: _,
                yaml,
                sidecar,
                pronunciation_sidecar,
            } => Session::open_with_pronunciation_sidecar(
                &yaml,
                sidecar.as_deref(),
                pronunciation_sidecar.as_deref(),
                seed,
            )?,
        };
        let id = session.session_id().to_string();
        self.sessions.insert(
            id.clone(),
            SessionEntry::new(session, now, recovery_id, store_name),
        );
        Ok(id)
    }

    /// Look up a session by **opaque id only** — never by project name, so two
    /// clients editing the same file cannot share a mutable document.
    pub fn get(&self, id: &str) -> Option<&SessionEntry> {
        self.sessions.get(id)
    }

    /// Mutable access without touching the idle clock.
    ///
    /// Render-job bookkeeping uses this: registering a job is not user
    /// activity, so it must not extend the TTL by itself.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut SessionEntry> {
        self.sessions.get_mut(id)
    }

    /// Look up a session and mark it active.
    pub fn touch(&mut self, id: &str, now: Instant) -> Option<&mut SessionEntry> {
        let entry = self.sessions.get_mut(id)?;
        entry.touch(now);
        Some(entry)
    }

    /// Close one session (`DELETE`).
    pub fn close(&mut self, id: &str) -> Option<SessionEntry> {
        self.sessions.remove(id)
    }

    /// Close every session past its idle deadline, skipping those with active
    /// render jobs. Returns the ids that were closed.
    pub fn sweep(&mut self, now: Instant) -> Vec<String> {
        let ttl = self.ttl;
        let expired: Vec<String> = self
            .sessions
            .iter()
            .filter(|(_, entry)| entry.is_expired(now, ttl))
            .map(|(id, _)| id.clone())
            .collect();
        for id in &expired {
            self.sessions.remove(id);
        }
        expired
    }
}

/// A server-level capability descriptor: the session contract (§6.15) plus the
/// host facts a client needs before opening a session.
#[derive(Debug, Clone, Serialize)]
pub struct ServerCapabilities {
    #[serde(flatten)]
    pub session: Capabilities,
    /// Effective idle TTL, in seconds (§6.12).
    pub session_idle_ttl_seconds: u64,
    /// Engines this process can actually serve today — never a static list.
    pub engines: Vec<EngineInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EngineInfo {
    pub id: &'static str,
    pub available: bool,
}

/// The engines a server instance can honestly advertise.
///
/// `r1-rs` is the pure-Rust path the pipeline always has. The `.so`-backed
/// `r1` is only advertised when the server was started with `--so`; neural
/// engines stay unadvertised until a render proves them.
pub fn engines(so_loaded: bool) -> Vec<EngineInfo> {
    let mut out = vec![EngineInfo {
        id: "r1-rs",
        available: true,
    }];
    if so_loaded {
        out.push(EngineInfo {
            id: "r1",
            available: true,
        });
    }
    out
}

/// Assemble the descriptor for a running server.
///
/// The session core advertises the *logical* contract; the render half is
/// host-specific (§6.15), so the native server overrides it with what this
/// process actually serves today: asynchronous jobs with per-job cancel and
/// polling for progress. The WASM adapter keeps `async_jobs: false` until it
/// grows an equivalent worker path.
pub fn capabilities(ttl: Duration, so_loaded: bool) -> ServerCapabilities {
    let mut session = project_session::v2::capabilities();
    session.render = RenderCapabilities {
        async_jobs: true,
        per_job_cancel: true,
        progress_transports: vec!["poll"],
    };
    ServerCapabilities {
        session,
        session_idle_ttl_seconds: ttl.as_secs(),
        engines: engines(so_loaded),
    }
}

/// The registry as stored in `AppState`.
pub type SharedRegistry = Mutex<SessionRegistry>;

#[cfg(test)]
mod tests {
    use super::*;

    const DEMO: &str = include_str!("../../../tools/synth-cli/tests/data/demo-song.ustx");

    fn registry() -> SessionRegistry {
        SessionRegistry::new(DEFAULT_IDLE_TTL)
    }

    fn stored() -> SessionSource {
        SessionSource::Stored {
            name: "scale-demo".to_string(),
            yaml: DEMO.to_string(),
            sidecar: None,
            pronunciation_sidecar: None,
        }
    }

    #[test]
    fn default_ttl_is_thirty_minutes() {
        assert_eq!(DEFAULT_IDLE_TTL, Duration::from_secs(30 * 60));
        assert_eq!(registry().ttl(), DEFAULT_IDLE_TTL);
    }

    #[test]
    fn a_session_is_addressed_by_its_opaque_id() {
        let mut reg = registry();
        let now = Instant::now();
        let id = reg.open(stored(), 1, now).expect("open");

        assert_eq!(reg.len(), 1);
        assert!(reg.get(&id).is_some());
        assert!(
            reg.get("scale-demo").is_none(),
            "a project name must not address a session"
        );
    }

    #[test]
    fn activity_resets_the_idle_clock() {
        let mut reg = registry();
        let start = Instant::now();
        let id = reg.open(stored(), 1, start).expect("open");

        // Almost expired...
        let almost = start + DEFAULT_IDLE_TTL - Duration::from_secs(1);
        assert!(reg.sweep(almost).is_empty());

        // ...then activity at that moment restarts the countdown.
        reg.touch(&id, almost).expect("touch");
        assert!(
            reg.sweep(almost + Duration::from_secs(60)).is_empty(),
            "touching must reset the TTL"
        );
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn an_idle_session_is_swept_and_reported() {
        let mut reg = registry();
        let start = Instant::now();
        let id = reg.open(stored(), 1, start).expect("open");

        let closed = reg.sweep(start + DEFAULT_IDLE_TTL + Duration::from_secs(1));

        assert_eq!(closed, vec![id]);
        assert!(
            reg.is_empty(),
            "an expired session is closed like an explicit close"
        );
    }

    #[test]
    fn a_session_with_a_render_job_never_expires() {
        let mut reg = registry();
        let start = Instant::now();
        let id = reg.open(stored(), 1, start).expect("open");

        reg.get(&id).expect("entry");
        reg.sessions.get_mut(&id).expect("entry").begin_render_job();

        // Far past the TTL: still alive while the job is queued/running.
        assert!(reg.sweep(start + DEFAULT_IDLE_TTL * 5).is_empty());
        assert_eq!(reg.get(&id).expect("entry").active_render_jobs(), 1);

        // Once the job ends and the TTL passes again, it expires.
        let after_job = start + DEFAULT_IDLE_TTL * 5;
        reg.touch(&id, after_job).expect("touch");
        reg.sessions.get_mut(&id).expect("entry").end_render_job();
        assert!(reg
            .sweep(after_job + DEFAULT_IDLE_TTL - Duration::from_secs(1))
            .is_empty());
        assert_eq!(
            reg.sweep(after_job + DEFAULT_IDLE_TTL + Duration::from_secs(1)),
            vec![id]
        );
    }

    #[test]
    fn closing_reports_the_entry_and_is_idempotent() {
        let mut reg = registry();
        let id = reg.open(stored(), 1, Instant::now()).expect("open");

        assert!(reg.close(&id).is_some());
        assert!(reg.close(&id).is_none(), "closing twice is not an error");
        assert!(reg.is_empty());
    }

    #[test]
    fn concurrent_sessions_are_isolated() {
        let mut reg = registry();
        let now = Instant::now();
        let first = reg.open(stored(), 1, now).expect("open");
        let second = reg.open(stored(), 2, now).expect("open");

        assert_ne!(first, second);
        // Editing through one session must not appear in the other.
        reg.get_mut(&first)
            .unwrap()
            .session
            .apply_v2(r#"{"expected_revision":0,"command":{"type":"note.set_tone","note_id":"x","tone":1}}"#)
            .expect_err("unknown id");
        let yaml_second = reg.get(&second).unwrap().session.yaml();
        assert!(
            yaml_second.contains("see[s i]"),
            "second session must be untouched"
        );
        assert_eq!(reg.len(), 2);
    }

    #[test]
    fn capabilities_report_the_session_contract_and_host_facts() {
        let caps = capabilities(DEFAULT_IDLE_TTL, false);
        assert_eq!(caps.session_idle_ttl_seconds, 30 * 60);
        assert_eq!(
            caps.engines,
            vec![EngineInfo {
                id: "r1-rs",
                available: true
            }]
        );
        assert_eq!(caps.session.limits.max_command_bytes, 1_048_576);
        assert!(caps.session.features.contains(&"group_transactions"));
        assert!(
            caps.session.render.async_jobs && caps.session.render.per_job_cancel,
            "the native server serves asynchronous jobs with per-job cancel"
        );
        assert_eq!(caps.session.render.progress_transports, vec!["poll"]);

        // With the native renderer loaded, `r1` joins the list.
        let with_so = capabilities(DEFAULT_IDLE_TTL, true);
        assert!(with_so.engines.iter().any(|e| e.id == "r1"));
    }
}
