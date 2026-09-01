//! Server-side project storage.
//!
//! ALL project data lives on the server ("ทุก project data เลย"): the
//! app saves/loads/lists/deletes projects through HTTP endpoints, and
//! the server owns the `.ustx` files on disk (using the same `ustx-codec`
//! that the app's FFI wrapper calls — one codec, both sides Rust).
//!
//! Layout: `<projects_root>/<name>.ustx`.

use std::path::{Path, PathBuf};

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

impl ProjectStore {
    pub fn new(root: PathBuf) -> Self {
        ProjectStore { root }
    }

    /// Ensure the root directory exists (created at startup).
    pub fn ensure(&self) -> Result<(), ProjectError> {
        std::fs::create_dir_all(&self.root)
            .map_err(|e| ProjectError::Io(e.to_string()))
    }

    fn validate_name(name: &str) -> Result<(), ProjectError> {
        if name.is_empty()
            || name.contains('/')
            || name.contains('\\')
            || name.contains("..")
        {
            return Err(ProjectError::InvalidName(name.to_string()));
        }
        Ok(())
    }

    fn path_for(&self, name: &str) -> PathBuf {
        self.root.join(format!("{name}.ustx"))
    }

    /// List saved project names, newest first.
    pub fn list(&self) -> Result<Vec<String>, ProjectError> {
        let mut names: Vec<String> = Vec::new();
        for entry in std::fs::read_dir(&self.root)
            .map_err(|e| ProjectError::Io(e.to_string()))?
        {
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
