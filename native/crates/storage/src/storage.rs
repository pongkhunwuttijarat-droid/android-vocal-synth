//! The [`Storage`] trait: the single way the feed pipeline touches files.

use std::io;
use std::path::PathBuf;

/// Filesystem access for the feed pipeline.
///
/// Every method addresses files by *logical path* — a portable,
/// `/`-separated, relative path such as `"voicebanks/teto/oto.ini"` — never
/// by absolute filesystem paths. Each backend maps logical paths onto its
/// own storage (a directory on desktop, the Kotlin `StorageService` sandbox
/// on Android) and MUST refuse logical paths that escape its root.
///
/// # Path safety
///
/// Logical paths containing `..` or `.` components, absolute paths, empty
/// paths, and NUL bytes are invalid and must be rejected by every
/// implementation:
///
/// * [`Storage::resolve`], [`Storage::exists`] and [`Storage::cache_path`]
///   have no error channel, so they treat an invalid logical path as a
///   programming error and **panic**.
/// * [`Storage::read_bytes`] and [`Storage::write_bytes`] report it as
///   `io::ErrorKind::InvalidInput`.
///
/// The `..` check is purely lexical: `"voicebanks/../teto"` is refused even
/// though it would not actually escape the root, so behaviour is predictable
/// and immune to symlink tricks.
pub trait Storage {
    /// Map a logical path to its absolute filesystem path under this
    /// storage's root.
    ///
    /// # Panics
    ///
    /// If `logical` is empty, absolute, or contains `.` / `..` components.
    fn resolve(&self, logical: &str) -> PathBuf;

    /// Read the full contents of the file at `path`.
    ///
    /// # Errors
    ///
    /// Returns `InvalidInput` if `path` is not a valid logical path, and
    /// `NotFound` if the file does not exist.
    fn read_bytes(&self, path: &str) -> io::Result<Vec<u8>>;

    /// Write `data` to the file at `path`, creating any missing parent
    /// directories (including the storage root itself).
    ///
    /// # Errors
    ///
    /// Returns `InvalidInput` if `path` is not a valid logical path, or the
    /// underlying filesystem error if the write fails.
    fn write_bytes(&self, path: &str, data: &[u8]) -> io::Result<()>;

    /// Whether a regular file exists at `path` (directories do not count).
    ///
    /// # Panics
    ///
    /// If `path` is not a valid logical path.
    fn exists(&self, path: &str) -> bool;

    /// Absolute path for a render-cache file identified by `key`.
    ///
    /// Keys are plain file names such as `"res-abc123.wav"`; sub-paths and
    /// `.` / `..` are rejected. All cache files live under the storage's
    /// `cache/` directory.
    ///
    /// # Panics
    ///
    /// If `key` is empty or contains path separators or `.` / `..`
    /// components.
    fn cache_path(&self, key: &str) -> PathBuf;
}
