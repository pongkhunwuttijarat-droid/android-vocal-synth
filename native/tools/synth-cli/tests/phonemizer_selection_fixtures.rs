//! Track phonemizer selection against REAL projects, not synthetic strings.
//!
//! `selection.rs` unit-tests the mapping with hand-written names. This file
//! checks the names that actually ship: the `.ustx` fixtures in this checkout
//! carry OpenUtau's registered phonemizer keys verbatim, so they are the only
//! place a wrong-but-plausible mapping would show up.
//!
//! `test-ustx-example/` is user-provided and gitignored, so every case that
//! needs it returns early when it is absent rather than failing a fresh
//! checkout. `test/golden/` is checked in and always runs.

use std::path::{Path, PathBuf};

use synth_cli::pipeline::{self, PhonemizerKind};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// The keys these projects really contain, and what they must resolve to.
///
/// Taken from the checked-in fixtures, not from the mapping table, so the two
/// cannot be edited into agreement by accident. Every key in the file must map
/// to `expected`; a fixture that stores none is simply not checked.
const GOLDEN: &[(&str, PhonemizerKind)] = &[
    // `test-ustx-example/` projects: the Japanese VCV algorithm
    // ("OpenUtau.Plugin.Builtin.JapanesePresampPhonemizer").
    ("king.ustx", PhonemizerKind::Japanese),
    (
        "golden-the-end-of-hutsune-miku.ustx",
        PhonemizerKind::Japanese,
    ),
    ("envybaby.ustx", PhonemizerKind::Japanese),
    ("incomplete-intensevoice.ustx", PhonemizerKind::Japanese),
];

/// Read the `phonemizer:` keys straight out of the `.ustx` text.
///
/// Deliberately NOT `pipeline::load_project`: these fixtures are multi-document
/// YAML, which `load_project` rejects ("more than one document is not
/// supported"). That is a pre-existing loader limitation and out of scope here —
/// but reading the key directly means this test still pins the selection rule
/// against real project text.
fn phonemizer_keys_in(path: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(path).expect("read fixture");
    text.lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("phonemizer:")?;
            let value = rest.trim().trim_matches('"').trim_matches('\'').trim();
            (!value.is_empty()).then(|| value.to_string())
        })
        .collect()
}

#[test]
fn real_ustx_fixtures_resolve_to_the_phonemizer_they_name() {
    let root = repo_root().join("test-ustx-example");
    if !root.is_dir() {
        eprintln!("skipping: test-ustx-example/ not present (gitignored fixture)");
        return;
    }
    let mut checked = 0usize;
    for (file, expected) in GOLDEN {
        let path = root.join(file);
        if !path.is_file() {
            eprintln!("skipping {file}: not present");
            continue;
        }
        for key in phonemizer_keys_in(&path) {
            let mut track = domain::UTrack::new("fixture");
            track.phonemizer = Some(key.clone());
            assert_eq!(
                PhonemizerKind::from_track(&track),
                *expected,
                "{file}: the key {key:?} resolved wrong"
            );
            checked += 1;
        }
    }
    assert!(
        checked > 0,
        "no fixture key was actually checked — the test proved nothing"
    );
    eprintln!("checked {checked} real fixture phonemizer keys");
}

#[test]
fn a_checked_in_golden_project_resolves_through_the_same_path() {
    // `test/golden/` is in the repo, so this always runs and keeps the golden
    // render corpus on the same selection rule as the user fixtures.
    let path = repo_root().join("test/golden/sakura-test.ustx");
    assert!(
        path.is_file(),
        "{} must exist: this fixture is checked in, and a silent skip here \
         would make the whole file prove nothing",
        path.display()
    );
    let mut project = pipeline::load_project(&path).expect("load project");
    project.tracks[0].phonemizer = None;
    assert_eq!(
        PhonemizerKind::from_track(&project.tracks[0]),
        PhonemizerKind::Universal,
        "a track with no phonemizer must stay Universal — changing this moves \
         every golden render"
    );
    project.tracks[0].phonemizer = Some("OpenUtau.Core.DefaultPhonemizer".into());
    assert_eq!(
        PhonemizerKind::from_track(&project.tracks[0]),
        PhonemizerKind::Default
    );
    project.tracks[0].phonemizer = Some("OpenUtau.Plugin.Builtin.JapanesePresampPhonemizer".into());
    assert_eq!(
        PhonemizerKind::from_track(&project.tracks[0]),
        PhonemizerKind::Japanese
    );
}

#[test]
fn the_golden_corpus_does_not_silently_change_phonemizer() {
    // Walks every checked-in `.ustx` under `test/golden/` and asserts each track
    // lands on a kind this engine implements. The point is not the specific kind
    // but that no track falls into a hole: every name a golden project carries
    // must resolve to something concrete rather than depending on substring luck.
    let dir = repo_root().join("test/golden");
    let entries = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{} must be readable: {e}", dir.display()));
    let mut checked = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("ustx") {
            continue;
        }
        let Ok(project) = pipeline::load_project(&path) else {
            continue;
        };
        for track in &project.tracks {
            let kind = PhonemizerKind::from_track(track);
            assert!(
                matches!(
                    kind,
                    PhonemizerKind::English
                        | PhonemizerKind::Japanese
                        | PhonemizerKind::Universal
                        | PhonemizerKind::Default
                ),
                "{}: track {:?} named {:?} resolved to nothing",
                path.display(),
                track.track_name,
                track.phonemizer
            );
            checked += 1;
        }
    }
    assert!(
        checked > 0,
        "no golden track was checked — the corpus walk proved nothing"
    );
    eprintln!("checked {checked} golden tracks");
}
