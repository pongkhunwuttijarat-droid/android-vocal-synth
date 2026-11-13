//! Shared application state: the scanned voicebanks, the render worker
//! handle and the render counters. Handlers extract it as
//! `State<Arc<AppState>>`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use voicebank::Voicebank;

use crate::projects::ProjectStore;
use crate::render_jobs::RenderJobTable;
use crate::render_service::RenderService;
use crate::sessions::{SessionRegistry, SharedRegistry, DEFAULT_IDLE_TTL};
use crate::stats::Stats;
use crate::stream_ws::StreamAuth;
use crate::voicebanks::VoicebankEntry;
use domain::{UPart, UProject};

/// Shared application state.
pub struct AppState {
    /// The configured `--voicebanks` root (informational).
    pub voicebanks_root: PathBuf,
    /// Server-side project storage (all project data lives here).
    pub projects: ProjectStore,
    /// Voicebanks discovered at startup.
    pub entries: Vec<VoicebankEntry>,
    /// Render worker handle; `None` when the server runs without `--so`.
    pub renderer: Option<RenderService>,
    /// Cumulative render counters.
    pub stats: Stats,
    /// Live project sessions (§6.12). Process-local and not durable; the idle
    /// TTL and render-job exemption live in [`SessionRegistry`].
    pub sessions: SharedRegistry,
    /// v2 render jobs (§6.13): immutable snapshot + per-job cancel + progress.
    pub render_jobs: RenderJobTable,
    /// Optional launch credential or browser-grant authority plus exact origin
    /// allowlist for MK3 WebSocket access. Disabled unless configured by host.
    pub stream_auth: Option<StreamAuth>,
}

impl AppState {
    pub fn new(
        voicebanks_root: PathBuf,
        projects_root: PathBuf,
        entries: Vec<VoicebankEntry>,
        renderer: Option<RenderService>,
    ) -> Self {
        let projects = ProjectStore::new(projects_root);
        if let Some(renderer) = renderer.as_ref() {
            renderer.set_uploaded_media_root(projects.media_root());
        }
        AppState {
            voicebanks_root,
            projects,
            entries,
            renderer,
            stats: Stats::default(),
            sessions: std::sync::Mutex::new(SessionRegistry::new(DEFAULT_IDLE_TTL)),
            render_jobs: RenderJobTable::new(),
            stream_auth: None,
        }
    }

    pub fn with_stream_auth(mut self, auth: StreamAuth) -> Self {
        self.stream_auth = Some(auth);
        self
    }

    /// Override the session idle TTL (host policy; default 30 minutes).
    pub fn with_session_ttl(mut self, ttl: std::time::Duration) -> Self {
        self.sessions = std::sync::Mutex::new(SessionRegistry::new(ttl));
        self
    }

    /// Whether the native `.so` renderer is loaded (what the server can
    /// honestly advertise as an available engine).
    pub fn so_loaded(&self) -> bool {
        self.renderer.is_some()
    }

    /// Crate version reported by `/health`.
    pub fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    /// Find a scanned voicebank by display name, dir id, or absolute
    /// library path.
    pub fn find_voicebank(&self, name: &str) -> Option<Arc<Voicebank>> {
        for entry in &self.entries {
            if entry.info.name == name || entry.info.dir == name {
                return Some(entry.bank.clone());
            }
        }
        let path = Path::new(name);
        if path.is_absolute() {
            for entry in &self.entries {
                if entry.info.path == path {
                    return Some(entry.bank.clone());
                }
            }
        }
        None
    }

    /// Resolve the immutable voicebank binding for every project track.
    /// Voice tracks must name an installed singer; non-voice tracks keep a
    /// positional placeholder because the stream planner has one slot per
    /// track. Missing singer bindings never fall back to the first bank.
    pub fn voicebanks_for_project(
        &self,
        project: &UProject,
    ) -> Result<Vec<Option<Arc<Voicebank>>>, String> {
        project
            .tracks
            .iter()
            .enumerate()
            .map(|(track_no, track)| {
                let has_voice = project.parts.iter().any(
                    |part| matches!(part, UPart::Voice(voice) if voice.track_no == track_no as i32),
                );
                match (has_voice, track.singer.as_deref()) {
                    (true, Some(singer)) => {
                        self.find_voicebank(singer).map(Some).ok_or_else(|| {
                            format!("track {track_no} singer '{singer}' is unavailable")
                        })
                    }
                    (true, None) => Err(format!(
                        "track {track_no} contains voice parts but has no committed singer"
                    )),
                    (false, Some(singer)) => Ok(self.find_voicebank(singer)),
                    (false, None) => Ok(None),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod voicebank_binding_tests {
    use super::*;
    use crate::voicebanks::{VoicebankEntry, VoicebankInfo};
    use domain::{UTrack, UVoicePart};

    fn entry(name: &str) -> VoicebankEntry {
        let bank = Arc::new(Voicebank::from_oto_bytes(
            name,
            name,
            "voice",
            "voice/oto.ini",
            b"a.wav=a,0,100,0,0,0\n",
        ));
        VoicebankEntry {
            info: VoicebankInfo {
                name: name.to_string(),
                dir: name.to_string(),
                aliases_count: 1,
                wav_count: 1,
                samples_rate: None,
                path: PathBuf::from(name),
            },
            bank,
        }
    }

    fn state(entries: Vec<VoicebankEntry>) -> AppState {
        AppState::new(
            PathBuf::new(),
            PathBuf::from("/tmp/rusted-utau-stream-binding-tests"),
            entries,
            None,
        )
    }

    #[test]
    fn project_voicebanks_follow_track_singers_and_allow_wave_only_slots() {
        let mut project = UProject::create();
        project.tracks = vec![
            UTrack {
                singer: Some("A".into()),
                ..UTrack::default()
            },
            UTrack::default(),
        ];
        let mut voice = UVoicePart::new("voice");
        voice.track_no = 0;
        project.parts.push(UPart::Voice(voice));
        let state = state(vec![entry("A")]);

        let resolved = state
            .voicebanks_for_project(&project)
            .expect("committed singer should resolve");
        assert!(resolved[0].is_some());
        assert!(resolved[1].is_none());
    }

    #[test]
    fn project_voicebank_resolution_rejects_missing_voice_singer() {
        let mut project = UProject::create();
        project.tracks = vec![UTrack::default()];
        let mut voice = UVoicePart::new("voice");
        voice.track_no = 0;
        project.parts.push(UPart::Voice(voice));
        let state = state(vec![entry("A")]);

        let error = state
            .voicebanks_for_project(&project)
            .expect_err("voice part without committed singer must fail preflight");
        assert!(error.contains("no committed singer"));
    }
}
