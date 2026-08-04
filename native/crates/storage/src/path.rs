//! Logical-path validation shared by all storage backends.
//!
//! A *logical path* is the portable, `/`-separated path the feed pipeline
//! uses to address files (e.g. `"voicebanks/teto/oto.ini"`). Backends map
//! logical paths onto their real filesystem. Every backend MUST reject any
//! logical path that could escape its root:
//!
//! * absolute paths (`/etc/passwd`, `C:\...`, UNC prefixes),
//! * parent-directory components (`..`),
//! * current-directory components (`.`),
//! * empty paths and paths containing NUL bytes.
//!
//! `..` is rejected even when it does not actually escape the root
//! (`voicebanks/../teto` is still refused), and the check is purely lexical
//! so behaviour is predictable and immune to symlink tricks. Repeated or
//! trailing slashes (`voicebanks//teto/`) are normalized away.

use std::io;
use std::path::{Component, Path, PathBuf};

/// Validate a logical path and return it as a root-relative `PathBuf`.
///
/// Rejects empty paths, NUL bytes, absolute paths (including drive prefixes
/// on Windows), and any `.` / `..` component. `.` components are rejected
/// even where `std::path` would silently drop them (e.g. `a/./b`), so the
/// accepted grammar is exactly: one or more non-empty segments joined by
/// `/`, none of which are `.` or `..`.
pub(crate) fn sanitize_logical(logical: &str) -> io::Result<PathBuf> {
    if logical.is_empty() {
        return Err(invalid("path is empty"));
    }
    if logical.contains('\0') {
        return Err(invalid("NUL byte is not allowed"));
    }
    // `std::path::components` silently drops "." components in the middle of
    // a path (`a/./b` yields no CurDir), so scan the `/` segments ourselves
    // for a uniform grammar on every platform.
    for segment in logical.split('/') {
        if segment == "." {
            return Err(invalid("'.' component is not allowed"));
        }
        if segment == ".." {
            return Err(invalid("'..' component is not allowed"));
        }
    }
    let mut out = PathBuf::new();
    for component in Path::new(logical).components() {
        match component {
            Component::Prefix(_) => return Err(invalid("drive prefix is not allowed")),
            Component::RootDir => return Err(invalid("absolute path is not allowed")),
            // Backstops: std yields CurDir for a leading "." (already caught
            // above) and ParentDir for ".." after a Windows `\` separator.
            Component::CurDir => return Err(invalid("'.' component is not allowed")),
            Component::ParentDir => return Err(invalid("'..' component is not allowed")),
            Component::Normal(segment) => out.push(segment),
        }
    }
    Ok(out)
}

/// Validate a cache key: a single plain file name (no separators, no
/// `.` / `..`).
pub(crate) fn sanitize_key(key: &str) -> io::Result<PathBuf> {
    if key.is_empty() {
        return Err(invalid("cache key is empty"));
    }
    if key.contains('\0') {
        return Err(invalid("NUL byte is not allowed"));
    }
    let mut components = Path::new(key).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(PathBuf::from(key)),
        _ => Err(invalid("cache key must be a single file name, not a path")),
    }
}

fn invalid(reason: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, reason.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_invalid(logical: &str) {
        let err = sanitize_logical(logical).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{logical:?}");
    }

    #[test]
    fn accepts_plain_relative_paths() {
        assert_eq!(
            sanitize_logical("voicebanks/teto/oto.ini").unwrap(),
            PathBuf::from("voicebanks/teto/oto.ini")
        );
        assert_eq!(sanitize_logical("a").unwrap(), PathBuf::from("a"));
        // Repeated separators collapse; trailing separator is fine.
        assert_eq!(
            sanitize_logical("voicebanks//teto/").unwrap(),
            PathBuf::from("voicebanks/teto")
        );
    }

    #[test]
    fn rejects_traversal_and_absolute() {
        for path in [
            "",
            "..",
            "../x",
            "a/../../b",
            "voicebanks/../teto",
            "a/..",
            "/etc/passwd",
            "./a",
            "a/./b",
            "a/.",
            ".",
            "a\0b",
        ] {
            assert_invalid(path);
        }
    }

    #[test]
    fn rejects_key_paths() {
        let keys = ["", ".", "..", "a/b", "a/../b", "/abs", "cache/x.wav"];
        for key in keys {
            let err = sanitize_key(key).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{key:?}");
        }
        // "a\0b" is never a usable file name.
        assert_eq!(
            sanitize_key("a\0b").unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        // On Windows a backslash is a path separator, so "a\b" is a sub-path
        // and must be rejected; on Unix it is a plain file-name character
        // and is fine.
        #[cfg(target_os = "windows")]
        assert_eq!(
            sanitize_key("a\\b").unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        #[cfg(not(target_os = "windows"))]
        assert_eq!(sanitize_key("a\\b").unwrap(), PathBuf::from("a\\b"));
    }

    #[test]
    fn accepts_plain_keys() {
        assert_eq!(
            sanitize_key("res-abc123.wav").unwrap(),
            PathBuf::from("res-abc123.wav")
        );
        assert_eq!(sanitize_key("oto.ini").unwrap(), PathBuf::from("oto.ini"));
    }
}
