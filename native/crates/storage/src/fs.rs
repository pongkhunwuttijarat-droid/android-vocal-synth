//! Directory-backed [`Storage`] implementation (desktop / tests / Linux).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::path::{sanitize_key, sanitize_logical};
use crate::Storage;

/// [`Storage`] backed by a plain directory on the local filesystem.
///
/// The root directory is fixed at construction; every logical path is
/// validated and resolved against it, so `..` can never escape the root.
/// The render cache lives in `{root}/cache/`.
#[derive(Clone, Debug)]
pub struct FsStorage {
    root: PathBuf,
}

impl FsStorage {
    /// Create a storage rooted at `root`.
    ///
    /// No directory is created here — the root (and any missing parents)
    /// are created lazily by [`Storage::write_bytes`].
    pub fn new(root: impl Into<PathBuf>) -> Self {
        FsStorage { root: root.into() }
    }

    /// The root directory all logical paths resolve against.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The directory render-cache files live in: `{root}/cache/`.
    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }
}

impl Storage for FsStorage {
    fn resolve(&self, logical: &str) -> PathBuf {
        let relative = sanitize_logical(logical)
            .unwrap_or_else(|e| panic!("invalid logical path {logical:?}: {e}"));
        self.root.join(relative)
    }

    fn read_bytes(&self, path: &str) -> io::Result<Vec<u8>> {
        fs::read(self.resolve_checked(path)?)
    }

    fn write_bytes(&self, path: &str, data: &[u8]) -> io::Result<()> {
        let absolute = self.resolve_checked(path)?;
        if let Some(parent) = absolute.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(absolute, data)
    }

    fn exists(&self, path: &str) -> bool {
        self.resolve(path).is_file()
    }

    fn cache_path(&self, key: &str) -> PathBuf {
        let name = sanitize_key(key).unwrap_or_else(|e| panic!("invalid cache key {key:?}: {e}"));
        self.cache_dir().join(name)
    }
}

impl FsStorage {
    /// [`Storage::resolve`] with an error channel for the IO methods.
    fn resolve_checked(&self, logical: &str) -> io::Result<PathBuf> {
        Ok(self.root.join(sanitize_logical(logical)?))
    }
}
