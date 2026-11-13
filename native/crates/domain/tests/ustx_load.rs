//! Loading real OpenUtau `.ustx` files.
//!
//! Every project OpenUtau writes starts with a UTF-8 BOM. `serde_yaml` does
//! not skip it, so the first key becomes unreadable and the document parses as
//! two documents:
//!
//! ```text
//! deserializing from YAML containing more than one document is not supported
//! ```
//!
//! That message reads like a multi-document problem, but the fixtures contain
//! ZERO `---` separators. It is the BOM. Before [`UProject::parse_yaml`], no
//! real OpenUtau project could be opened at all — including the multi-track
//! ones, which is where it hurts most.

use domain::{UPart, UProject};
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// A valid `.ustx` body, produced by the domain's OWN serializer.
///
/// Hand-written YAML drifted from the model already once (a `mute: 0` where the
/// field is a bool), so the fixture is generated instead — it cannot disagree
/// with `UProject`'s shape.
fn minimal_yaml() -> String {
    serde_yaml::to_string(&UProject::create()).expect("the domain serializes its own default")
}

#[test]
fn a_utf8_bom_does_not_break_loading() {
    let body = minimal_yaml();
    let plain = UProject::parse_yaml(&body).expect("plain YAML loads");

    let with_bom = format!("\u{feff}{body}");
    let loaded = UProject::parse_yaml(&with_bom).expect("BOM-prefixed YAML loads");
    assert_eq!(
        loaded.name, plain.name,
        "a BOM must not change the parsed document"
    );
    assert_eq!(
        loaded.tracks.len(),
        plain.tracks.len(),
        "a BOM must not change how many tracks load"
    );
}

#[test]
fn the_bom_failure_mode_really_was_a_bom_and_not_a_multi_document_file() {
    // Pin the diagnosis itself: raw serde rejects the BOM, and the fixture
    // contains no document separators. Without this, someone could "fix" a
    // regression by deleting the BOM handling and this would still pass.
    let body = minimal_yaml();
    let with_bom = format!("\u{feff}{body}");
    let raw = serde_yaml::from_str::<UProject>(&with_bom);
    assert!(
        raw.is_err(),
        "raw serde_yaml is expected to reject the BOM; if it now accepts it, \
         the BOM handling in parse_yaml can be revisited"
    );
    assert!(
        !body.contains("\n---"),
        "this fixture is single-document by construction"
    );
}

#[test]
fn leading_whitespace_before_a_bom_is_also_tolerated() {
    let messy = format!("\n\n  \u{feff}{}", minimal_yaml());
    let loaded = UProject::parse_yaml(&messy).expect("whitespace + BOM loads");
    assert_eq!(loaded.name, UProject::create().name);
}

#[test]
fn from_yaml_text_parses_without_running_after_load() {
    // `after_load` mutates: it merges default expressions, revalidates timing,
    // and bumps the version to 0.9. A caller that needs `LoadRepair` warnings
    // uses `from_yaml_text` and then runs `after_load_with_warnings` itself —
    // exactly once. This test pins that the split is a real difference in
    // behaviour, not two names for the same thing, AND that both read the same
    // document.
    let text = format!("\u{feff}{}", minimal_yaml());
    let bare = UProject::from_yaml_text(&text).expect("parse without repair");

    let mut full = UProject::from_yaml_text(&text).expect("parse");
    full.after_load().expect("repair");
    assert_eq!(
        bare.name, full.name,
        "both paths must read the same document"
    );
    assert_eq!(
        bare.tracks.len(),
        full.tracks.len(),
        "both paths must see the same tracks"
    );
    // Prove the repair step is what normalises the document, by showing it
    // changes state the bare parse leaves alone. A 0.6 file is bumped to 0.9 by
    // `after_load` — exactly the OpenUtau version migration.
    //
    // The version line is INSERTED because the serializer omits
    // `ustx_version` when it is already 0.9, so there is nothing to replace.
    let legacy = minimal_yaml().replacen("bpm:", "ustx_version: '0.6'\nbpm:", 1);
    assert!(
        legacy.contains("ustx_version: '0.6'"),
        "the legacy fixture must actually carry the old version"
    );
    let legacy = format!("\u{feff}{legacy}");
    let mut repaired = UProject::from_yaml_text(&legacy).expect("parse legacy");
    assert_eq!(
        repaired.ustx_version,
        Some(domain::UstxVersion::new(0, 6)),
        "before after_load the legacy version must still be as written"
    );
    repaired.after_load().expect("migrate legacy version");
    assert_eq!(
        repaired.ustx_version,
        Some(domain::UstxVersion::new(0, 9)),
        "after_load must migrate 0.6 to 0.9 — this is what a caller would miss \
         if it used from_yaml_text and forgot to run the repair"
    );
}

#[test]
fn a_real_multitrack_fixture_opens_with_all_its_tracks() {
    // `king.ustx` is a genuine 5-track project. This is the case the BOM bug
    // made impossible: it could not be opened at all.
    let path = repo_root().join("test-ustx-example/king.ustx");
    if !path.is_file() {
        eprintln!("skipping: test-ustx-example/king.ustx not present (gitignored)");
        return;
    }
    let yaml = std::fs::read_to_string(&path).expect("read the real project");
    let project = UProject::parse_yaml(&yaml).expect("load the real 5-track project");
    assert_eq!(
        project.tracks.len(),
        5,
        "the fixture is a 5-track project; loading fewer means parts were dropped"
    );
    let voice_parts = project
        .parts
        .iter()
        .filter(|part| matches!(part, UPart::Voice(_)))
        .count();
    assert_eq!(
        voice_parts, 5,
        "every track must keep its voice part — a partial load would render silence"
    );
    // Each part must belong to a track that exists, otherwise the track index
    // is dangling and render planning cannot route it.
    for part in &project.parts {
        let track_no = part.track_no();
        assert!(
            track_no >= 0 && (track_no as usize) < project.tracks.len(),
            "part points at track {track_no}, but only {} tracks exist",
            project.tracks.len()
        );
    }
}
