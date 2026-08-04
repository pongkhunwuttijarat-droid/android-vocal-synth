//! `storage` — filesystem access behind one trait (Sprint 1.4.2).
//!
//! The feed pipeline (next sprint) reads voicebank files and writes render
//! caches exclusively through [`Storage`], never through raw `std::fs`:
//!
//! * [`FsStorage`] — desktop / tests / Linux: a plain root directory with
//!   path-traversal protection.
//! * [`JniStorage`] — Android: compile-time stub for the Kotlin
//!   `StorageService` path (JNI wiring lands in a later sprint).
//!
//! All methods address files by *logical path* — a portable, `/`-separated,
//! relative path such as `"voicebanks/teto/oto.ini"`. Backends reject any
//! logical path that could escape their root (`..`, `.` components, absolute
//! paths). See [`Storage`] for the full contract.

mod path;

pub mod fs;
mod storage;

#[cfg(target_os = "android")]
pub mod jni;

pub use fs::FsStorage;
pub use storage::Storage;

#[cfg(target_os = "android")]
pub use jni::JniStorage;
