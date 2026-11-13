//! Guard: production code must load `.ustx` project text through
//! [`UProject::parse_yaml`], never straight through `serde_yaml`.
//!
//! ## Why this needs a machine check
//!
//! Every `.ustx` OpenUtau writes starts with a UTF-8 BOM
//! (`core/OpenUtau.Core/Format/USTx.cs` saves with
//! `File.WriteAllText(..., Encoding.UTF8)`, and .NET's `Encoding.UTF8` emits a
//! BOM preamble). `serde_yaml` does not skip it, so the document parses as two:
//!
//! ```text
//! deserializing from YAML containing more than one document is not supported
//! ```
//!
//! That error is actively misleading — real projects contain **zero** `---`
//! separators. One call site doing it the obvious way
//! (`serde_yaml::from_str::<UProject>(yaml)`) rejects every real OpenUtau file
//! while passing every hand-written test fixture, because nobody writes a BOM
//! into a test string by hand.
//!
//! A doc comment is not enough: the next call site will look like every other
//! example in the codebase. This test fails instead.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Rust source roots that make up production code for this crate graph.
fn production_roots() -> Vec<PathBuf> {
    let root = workspace_root();
    let mut roots = vec![root.join("crates"), root.join("tools")];
    roots.retain(|path| path.is_dir());
    roots
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // `target/` is build output, not source we govern.
            if path.file_name().and_then(|n| n.to_str()) != Some("target") {
                collect_rs_files(&path, out);
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// The `#[cfg(test)] mod tests { … }` span of a file, if any.
///
/// A crude but honest approximation: from the first `mod tests` to the end of
/// the file. Rust test modules are conventionally last, and a false negative
/// here (missing a test-only hit) would only make the test stricter, never
/// weaker — the failure mode we care about is a real production bypass.
fn test_module_start(source: &str) -> Option<usize> {
    source
        .lines()
        .position(|line| line.trim_start().starts_with("mod tests"))
}

/// Lines inside `mod tests`, or inside a `#[test]`-annotated function.
fn is_test_code(source: &str, index: usize) -> bool {
    if let Some(start) = test_module_start(source) {
        if index >= start {
            return true;
        }
    }
    let lines: Vec<&str> = source.lines().collect();
    lines.iter().take(index).rev().take(40).any(|line| {
        let trimmed = line.trim();
        trimmed.starts_with("#[test]") || trimmed.starts_with("#[cfg(test)]")
    })
}

#[test]
fn no_production_call_site_parses_ustx_text_with_serde_yaml() {
    let mut files = Vec::new();
    for root in production_roots() {
        collect_rs_files(&root, &mut files);
    }
    assert!(
        !files.is_empty(),
        "found no Rust sources — the scan path is wrong, so this test would \
         pass without checking anything"
    );

    let mut violations: Vec<String> = Vec::new();
    for file in &files {
        let Ok(source) = std::fs::read_to_string(file) else {
            continue;
        };
        // The canonical loader itself must deserialize; it is the one allowed
        // direct call, and it is inside `domain`'s own source.
        let is_loader = file.ends_with("domain/src/project.rs");

        // Integration tests and doc comments are not production bypasses: a
        // test parsing a hand-written fixture without a BOM is fine, and this
        // file's own doc comment names the anti-pattern on purpose.
        let relative = file
            .strip_prefix(workspace_root())
            .unwrap_or(file)
            .display()
            .to_string();
        if relative.contains("/tests/") || relative.contains("/examples/") {
            continue;
        }

        for (index, line) in source.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!")
            {
                continue;
            }
            if !trimmed.contains("serde_yaml::from_str") {
                continue;
            }
            // Only project-shaped deserialization matters. A voicebank config
            // or a DiffSinger config is a different format entirely.
            let parses_project = trimmed.contains("UProject")
                || trimmed.contains("<UProject>")
                || (trimmed.contains("from_str(yaml)")
                    && !file.ends_with("voicebank/src/config.rs")
                    && !file.ends_with("diffsinger/src/config.rs"));
            if !parses_project {
                continue;
            }
            if is_test_code(&source, index) {
                continue;
            }
            violations.push(format!("{relative}:{}: {trimmed}", index + 1));
        }
    }

    assert!(
        violations.is_empty(),
        "Production code must load `.ustx` text through \
         `UProject::parse_yaml` (BOM-tolerant) or \
         `UProject::from_yaml_text`, not `serde_yaml` directly — a BOM makes \
         serde_yaml read the project as two documents and every real OpenUtau \
         file fails to open.\n\nOffending lines:\n  {}\n\n\
         Fix: call `UProject::parse_yaml(yaml)` (which also runs `after_load`), \
         or `UProject::from_yaml_text(yaml)` when the caller runs the repair \
         step itself.",
        violations.join("\n  ")
    );
}

#[test]
fn the_canonical_loader_is_where_the_fix_actually_lives() {
    // If someone "simplifies" `parse_yaml` back into a bare `serde_yaml`
    // call, the scan above would still pass (the loader is allow-listed) while
    // the actual defect returns. This pins the behaviour instead.
    let yaml = format!(
        "\u{feff}{}",
        serde_yaml::to_string(&domain::UProject::create()).unwrap()
    );
    assert!(
        domain::UProject::parse_yaml(&yaml).is_ok(),
        "the allow-listed canonical loader MUST stay BOM-tolerant"
    );
    assert!(
        serde_yaml::from_str::<domain::UProject>(&yaml).is_err(),
        "raw serde_yaml is expected to reject a BOM; if it stops doing so, \
         `parse_yaml` and this whole guard can be simplified"
    );
}
