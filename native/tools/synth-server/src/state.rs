//! Shared application state: the scanned voicebanks, the render worker
//! handle and the render counters. Handlers extract it as
//! `State<Arc<AppState>>`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use voicebank::Voicebank;

use crate::render_service::RenderService;
use crate::stats::Stats;
use crate::voicebanks::VoicebankEntry;

/// Shared application state.
pub struct AppState {
    /// The configured `--voicebanks` root (informational).
    pub voicebanks_root: PathBuf,
    /// Voicebanks discovered at startup.
    pub entries: Vec<VoicebankEntry>,
    /// Render worker handle; `None` when the server runs without `--so`.
    pub renderer: Option<RenderService>,
    /// Cumulative render counters.
    pub stats: Stats,
}

impl AppState {
    pub fn new(
        voicebanks_root: PathBuf,
        entries: Vec<VoicebankEntry>,
        renderer: Option<RenderService>,
    ) -> Self {
        AppState {
            voicebanks_root,
            entries,
            renderer,
            stats: Stats::default(),
        }
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
}
