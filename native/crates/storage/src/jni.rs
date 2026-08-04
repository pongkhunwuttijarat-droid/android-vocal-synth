//! Android [`Storage`] implementation backed by the Kotlin `StorageService`.
//!
//! # Contract (for the Android wiring sprint)
//!
//! On Android the app's files live inside the app sandbox (SAF-picked
//! voicebanks, internal cache). Path resolution is owned by Kotlin:
//!
//! 1. Feed code calls [`Storage`] methods with logical paths, exactly as on
//!    desktop — the feed never learns where files really live.
//! 2. This backend forwards requests to the Kotlin `StorageService` over JNI
//!    (or receives a resolved root dir from it at startup).
//! 3. Kotlin resolves logical paths against its sandbox and performs the IO,
//!    so Android never exposes raw filesystem paths to the Rust side.
//!
//! # Current status (Sprint 1.4.2)
//!
//! This is a compile-time stub so feed code builds for
//! `target_os = "android"`. Path computation (which is pure) falls back to
//! [`FsStorage`] semantics; actual IO returns `io::ErrorKind::Unsupported`
//! until the JNI wiring lands. The `TODO(android-wiring)` markers below are
//! the exact points to replace.

use std::io;
use std::path::PathBuf;

use crate::fs::FsStorage;
use crate::Storage;

/// Placeholder Android backend; see the module docs for the wiring contract.
#[derive(Debug)]
pub struct JniStorage {
    inner: FsStorage,
}

impl JniStorage {
    /// Create the stub around a root directory (Android internal storage).
    ///
    /// TODO(android-wiring): the real implementation will obtain the root /
    /// handle from the Kotlin `StorageService` instead of a caller-supplied
    /// path, and route IO through JNI.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        JniStorage {
            inner: FsStorage::new(root),
        }
    }
}

impl Storage for JniStorage {
    fn resolve(&self, logical: &str) -> PathBuf {
        // Pure path computation is safe and useful for diagnostics.
        self.inner.resolve(logical)
    }

    fn read_bytes(&self, _path: &str) -> io::Result<Vec<u8>> {
        // TODO(android-wiring): forward to Kotlin StorageService.readBytes().
        Err(self.unsupported_error())
    }

    fn write_bytes(&self, _path: &str, _data: &[u8]) -> io::Result<()> {
        // TODO(android-wiring): forward to Kotlin StorageService.writeBytes().
        Err(self.unsupported_error())
    }

    fn exists(&self, path: &str) -> bool {
        // Read-only path check; harmless until real IO lands.
        self.inner.exists(path)
    }

    fn cache_path(&self, key: &str) -> PathBuf {
        self.inner.cache_path(key)
    }
}

impl JniStorage {
    fn unsupported_error(&self) -> io::Error {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "JniStorage is a stub: Kotlin StorageService wiring lands in a later sprint",
        )
    }
}
