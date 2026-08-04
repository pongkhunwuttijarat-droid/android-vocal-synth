//! Integration tests for [`storage::FsStorage`], the desktop/test backend.
//!
//! These exercise the public [`storage::Storage`] contract: resolution,
//! read/write round-trips, parent-directory creation, cache paths, and —
//! critically — path-traversal rejection.

use std::fs;

use storage::{FsStorage, Storage};
use tempfile::tempdir;

/// A storage rooted at a fresh temporary directory.
fn storage() -> (tempfile::TempDir, FsStorage) {
    let dir = tempdir().expect("tempdir");
    let fs = FsStorage::new(dir.path());
    (dir, fs)
}

// --- resolve -------------------------------------------------------------

#[test]
fn resolve_joins_logical_paths_under_root() {
    let (dir, fs) = storage();
    assert_eq!(
        fs.resolve("voicebanks/teto/oto.ini"),
        dir.path().join("voicebanks/teto/oto.ini")
    );
    // Valid logical paths never resolve outside the root.
    assert!(fs.resolve("a/b").starts_with(dir.path()));
}

#[test]
#[should_panic(expected = "..")]
fn resolve_rejects_parent_dir_traversal() {
    let (_dir, fs) = storage();
    fs.resolve("../escape.txt");
}

#[test]
#[should_panic(expected = "..")]
fn resolve_rejects_traversal_inside_path() {
    let (_dir, fs) = storage();
    fs.resolve("voicebanks/../teto");
}

#[test]
#[should_panic(expected = "absolute path")]
fn resolve_rejects_absolute_path() {
    let (_dir, fs) = storage();
    fs.resolve("/etc/passwd");
}

#[test]
#[should_panic(expected = "'.' component")]
fn resolve_rejects_cur_dir_component() {
    let (_dir, fs) = storage();
    fs.resolve("./voicebanks/teto");
}

#[test]
#[should_panic(expected = "empty")]
fn resolve_rejects_empty_path() {
    let (_dir, fs) = storage();
    fs.resolve("");
}

// --- read / write --------------------------------------------------------

#[test]
fn write_read_roundtrip() {
    let (dir, fs) = storage();
    let logical = "voicebanks/teto/sample.wav";
    fs.write_bytes(logical, b"RIFF....WAVE").unwrap();
    assert_eq!(fs.read_bytes(logical).unwrap(), b"RIFF....WAVE");
    // The file really landed under root/voicebanks/teto/.
    assert_eq!(fs::read(dir.path().join(logical)).unwrap(), b"RIFF....WAVE");
}

#[test]
fn write_creates_parent_directories() {
    let (dir, fs) = storage();
    fs.write_bytes("voicebanks/teto/oto/extra.ini", b"x")
        .unwrap();
    assert!(dir.path().join("voicebanks/teto/oto").is_dir());
    assert!(dir.path().join("voicebanks/teto/oto/extra.ini").is_file());
}

#[test]
fn write_creates_root_dir_lazily() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("fresh/app/root");
    let fs = FsStorage::new(&root);
    fs.write_bytes("voicebanks/teto/oto.ini", b"[]").unwrap();
    assert!(root.join("voicebanks/teto/oto.ini").is_file());
}

#[test]
fn read_missing_file_is_not_found() {
    let (_dir, fs) = storage();
    let err = fs.read_bytes("voicebanks/ghost.wav").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn write_and_read_reject_traversal_with_error() {
    let (dir, fs) = storage();
    let err = fs.write_bytes("../evil.txt", b"x").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    // Nothing was written outside the root.
    assert!(!dir.path().parent().unwrap().join("evil.txt").exists());

    let err = fs.read_bytes("a/../../b").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn write_rejects_absolute_path_with_error() {
    let (_dir, fs) = storage();
    let err = fs.write_bytes("/tmp/evil.txt", b"x").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
}

// --- exists --------------------------------------------------------------

#[test]
fn exists_reflects_written_files() {
    let (_dir, fs) = storage();
    assert!(!fs.exists("voicebanks/teto/oto.ini"));
    fs.write_bytes("voicebanks/teto/oto.ini", b"[]").unwrap();
    assert!(fs.exists("voicebanks/teto/oto.ini"));
}

#[test]
fn exists_is_false_for_directories() {
    let (_dir, fs) = storage();
    fs.write_bytes("voicebanks/teto/oto.ini", b"[]").unwrap();
    assert!(!fs.exists("voicebanks/teto"));
    assert!(!fs.exists("voicebanks"));
}

#[test]
#[should_panic(expected = "..")]
fn exists_rejects_traversal() {
    let (_dir, fs) = storage();
    fs.exists("../etc/passwd");
}

// --- cache_path ----------------------------------------------------------

#[test]
fn cache_path_points_under_cache_dir() {
    let (dir, fs) = storage();
    let path = fs.cache_path("res-abc123.wav");
    assert_eq!(path, dir.path().join("cache/res-abc123.wav"));
    assert_eq!(path.parent().unwrap(), dir.path().join("cache"));
}

#[test]
fn cache_path_is_unique_per_key() {
    let (_dir, fs) = storage();
    assert_ne!(
        fs.cache_path("res-abc123.wav"),
        fs.cache_path("res-def456.wav")
    );
    assert_eq!(
        fs.cache_path("res-abc123.wav"),
        fs.cache_path("res-abc123.wav")
    );
}

#[test]
#[should_panic(expected = "single file name")]
fn cache_path_rejects_subdirectories() {
    let (_dir, fs) = storage();
    fs.cache_path("cache/res-abc123.wav");
}

#[test]
#[should_panic(expected = "single file name")]
fn cache_path_rejects_traversal() {
    let (_dir, fs) = storage();
    fs.cache_path("../res-abc123.wav");
}

// --- cache via the logical-path API --------------------------------------

#[test]
fn cache_roundtrip_via_logical_paths() {
    let (_dir, fs) = storage();
    // Feed writes caches through the same logical-path API; cache/ is just
    // a plain subdirectory from the logical namespace's point of view.
    fs.write_bytes("cache/res-abc123.wav", b"audio").unwrap();
    assert_eq!(fs.read_bytes("cache/res-abc123.wav").unwrap(), b"audio");
    assert!(fs.exists("cache/res-abc123.wav"));
}
