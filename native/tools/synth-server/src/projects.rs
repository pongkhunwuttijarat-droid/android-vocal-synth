//! Server-side project storage.
//!
//! ALL project data lives on the server ("ทุก project data เลย"): the
//! app saves/loads/lists/deletes projects through HTTP endpoints, and
//! the server owns the `.ustx` files on disk (using the same `ustx-codec`
//! that the app's FFI wrapper calls — one codec, both sides Rust).
//!
//! Layout: `<projects_root>/<name>.ustx`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Errors from the project store.
#[derive(Debug)]
pub enum ProjectError {
    NotFound(String),
    InvalidName(String),
    Io(String),
}

impl ProjectError {
    pub fn message(&self) -> String {
        match self {
            ProjectError::NotFound(name) => format!("project '{name}' not found"),
            ProjectError::InvalidName(name) => {
                format!("invalid project name '{name}' (allowed: letters, digits, - _ space)")
            }
            ProjectError::Io(e) => format!("project storage: {e}"),
        }
    }
}

/// Filesystem-backed project storage rooted at `root`.
#[derive(Debug, Clone)]
pub struct ProjectStore {
    root: PathBuf,
}

/// Server-side crash-recovery snapshot for one browser client. This is kept
/// separate from named projects so autosave never overwrites a user's file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoverySnapshot {
    pub store_name: Option<String>,
    pub document_name: Option<String>,
    pub ustx: String,
    pub sidecar: String,
    pub pronunciation_sidecar: String,
}

impl ProjectStore {
    pub fn new(root: PathBuf) -> Self {
        ProjectStore { root }
    }

    fn recovery_path(&self, recovery_id: &str) -> Result<PathBuf, ProjectError> {
        if recovery_id.len() < 16
            || recovery_id.len() > 128
            || !recovery_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            return Err(ProjectError::InvalidName("invalid recovery id".to_string()));
        }
        Ok(self
            .root
            .join(".recovery")
            .join(format!("{recovery_id}.json")))
    }

    /// Persist a Rust-authored recovery checkpoint without touching named
    /// project files.
    pub fn save_recovery(
        &self,
        recovery_id: &str,
        snapshot: &RecoverySnapshot,
    ) -> Result<(), ProjectError> {
        let path = self.recovery_path(recovery_id)?;
        let parent = path
            .parent()
            .ok_or_else(|| ProjectError::Io("recovery path has no parent".to_string()))?;
        std::fs::create_dir_all(parent).map_err(|error| ProjectError::Io(error.to_string()))?;
        let bytes =
            serde_json::to_vec(snapshot).map_err(|error| ProjectError::Io(error.to_string()))?;
        write_atomic(&path, &bytes)
    }

    /// Read the most recent checkpoint for a browser client.
    pub fn load_recovery(&self, recovery_id: &str) -> Result<RecoverySnapshot, ProjectError> {
        let path = self.recovery_path(recovery_id)?;
        if !path.is_file() {
            return Err(ProjectError::NotFound(recovery_id.to_string()));
        }
        let bytes = std::fs::read(path).map_err(|error| ProjectError::Io(error.to_string()))?;
        serde_json::from_slice(&bytes).map_err(|error| ProjectError::Io(error.to_string()))
    }

    /// Ensure the root directory exists (created at startup).
    pub fn ensure(&self) -> Result<(), ProjectError> {
        std::fs::create_dir_all(&self.root).map_err(|e| ProjectError::Io(e.to_string()))
    }

    fn validate_name(name: &str) -> Result<(), ProjectError> {
        if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
            return Err(ProjectError::InvalidName(name.to_string()));
        }
        Ok(())
    }

    fn path_for(&self, name: &str) -> PathBuf {
        self.root.join(format!("{name}.ustx"))
    }

    /// Root for browser-uploaded audio assets referenced as `asset:<id>`.
    pub fn media_root(&self) -> PathBuf {
        self.root.join(".media")
    }

    /// Persist an uploaded audio asset so a server restart does not leave
    /// recovered wave parts pointing at bytes that existed only in RAM.
    pub fn save_media(&self, id: &str, bytes: &[u8]) -> Result<(), ProjectError> {
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(ProjectError::InvalidName("invalid media id".to_string()));
        }
        let root = self.media_root();
        std::fs::create_dir_all(&root).map_err(|error| ProjectError::Io(error.to_string()))?;
        write_atomic(&root.join(format!("{id}.media")), bytes)
    }

    /// List saved project names, newest first.
    pub fn list(&self) -> Result<Vec<String>, ProjectError> {
        let mut names: Vec<String> = Vec::new();
        for entry in std::fs::read_dir(&self.root).map_err(|e| ProjectError::Io(e.to_string()))? {
            let entry = entry.map_err(|e| ProjectError::Io(e.to_string()))?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "ustx") {
                if let Some(stem) = path.file_stem() {
                    names.push(stem.to_string_lossy().into_owned());
                }
            }
        }
        names.sort();
        names.reverse();
        Ok(names)
    }

    /// Read a project's `.ustx` bytes.
    pub fn load(&self, name: &str) -> Result<Vec<u8>, ProjectError> {
        Self::validate_name(name)?;
        let path = self.path_for(name);
        if !path.is_file() {
            return Err(ProjectError::NotFound(name.to_string()));
        }
        std::fs::read(&path).map_err(|e| ProjectError::Io(e.to_string()))
    }

    /// Write a project's `.ustx` bytes (upsert).
    pub fn save(&self, name: &str, ustx_bytes: &[u8]) -> Result<(), ProjectError> {
        Self::validate_name(name)?;
        let path = self.path_for(name);
        std::fs::write(&path, ustx_bytes).map_err(|e| ProjectError::Io(e.to_string()))
    }

    /// Path of the identity sidecar for `name` (§6.2.4).
    pub fn sidecar_path_for(&self, name: &str) -> PathBuf {
        self.root.join(format!("{name}.ustx.rusted-utau-ids.json"))
    }

    /// Path of the Rusted UTAU pronunciation sidecar.
    pub fn pronunciation_sidecar_path_for(&self, name: &str) -> PathBuf {
        self.root
            .join(format!("{name}.ustx.rusted-utau-pronunciations.json"))
    }

    /// Write `.ustx` **and** its identity sidecar atomically.
    ///
    /// Only the session v2 save path calls this (decision 2026-09-29): the
    /// legacy `POST /project` endpoint keeps its old behavior. Each file is
    /// written to a temporary sibling and renamed, so a crash leaves either the
    /// previous version or the new one — never a half-written project.
    pub fn save_with_sidecar(
        &self,
        name: &str,
        ustx_bytes: &[u8],
        sidecar_json: Option<&str>,
        pronunciation_sidecar_json: Option<&str>,
    ) -> Result<(), ProjectError> {
        Self::validate_name(name)?;
        std::fs::create_dir_all(&self.root).map_err(|e| ProjectError::Io(e.to_string()))?;
        write_atomic(&self.path_for(name), ustx_bytes)?;
        if let Some(json) = sidecar_json {
            write_atomic(&self.sidecar_path_for(name), json.as_bytes())?;
        }
        if let Some(json) = pronunciation_sidecar_json {
            write_atomic(&self.pronunciation_sidecar_path_for(name), json.as_bytes())?;
        }
        Ok(())
    }

    /// Delete a project file.
    pub fn delete(&self, name: &str) -> Result<(), ProjectError> {
        Self::validate_name(name)?;
        let path = self.path_for(name);
        if !path.is_file() {
            return Err(ProjectError::NotFound(name.to_string()));
        }
        std::fs::remove_file(&path).map_err(|e| ProjectError::Io(e.to_string()))
    }
}

/// Resolve a project reference: a bare name → stored project; an
/// existing absolute path → that file (back-compat with the old
/// app-owned path flow). Returns the resolved path.
pub fn resolve_project_path(store: &ProjectStore, ref_: &str) -> Option<PathBuf> {
    if Path::new(ref_).is_file() {
        return Some(PathBuf::from(ref_));
    }
    let name = ref_.trim_end_matches(".ustx");
    let path = store.root.join(format!("{name}.ustx"));
    path.is_file().then_some(path)
}

/// Write `bytes` to `path` via a temporary sibling plus rename.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), ProjectError> {
    let tmp = path.with_extension("tmp-write");
    std::fs::write(&tmp, bytes).map_err(|e| ProjectError::Io(e.to_string()))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        ProjectError::Io(e.to_string())
    })
}
