//! Load-time repair (`domain::LoadRepair`): malformed curves and missing tracks.
//!
//! `UCurve::sample` indexes `xs` and `ys` together without checking their
//! lengths, so a malformed `.ustx` used to panic on the first render. Loading
//! must repair that deterministically instead — and must never delete curve
//! data the user might still want.

use domain::{LoadRepair, UCurve, UPart, UProject};

/// One voice part with one curve, whose point arrays can be malformed.
///
/// Written as a raw string: `\`-continuations in a normal literal strip the
/// next line's indentation, which silently flattens nested YAML.
fn yaml_with_curve(xs: &str, ys: &str, abbr: &str) -> String {
    format!(
        r#"ustx_version: 0.9
name: Curve Repair
bpm: 120
beat_per_bar: 4
beat_unit: 4
time_signatures:
- bar_position: 0
  beat_per_bar: 4
  beat_unit: 4
tempos:
- position: 0
  bpm: 120.0
expressions:
  dyn:
    name: dynamics (curve)
    abbr: dyn
    type: Curve
    min: -240.0
    max: 120.0
    default_value: 0.0
tracks:
- track_name: Track1
voice_parts:
- name: Part1
  track_no: 0
  position: 0
  duration: 1920
  notes: []
  curves:
  - abbr: {abbr}
    xs: [{xs}]
    ys: [{ys}]
"#
    )
}

fn load(yaml: &str) -> (UProject, Vec<LoadRepair>) {
    let mut project: UProject = serde_yaml::from_str(yaml).expect("parse");
    let warnings = project.after_load_with_warnings().expect("after_load");
    (project, warnings)
}

fn first_curve(project: &UProject) -> &UCurve {
    project
        .parts
        .iter()
        .find_map(|p| match p {
            UPart::Voice(vp) => vp.curves.first(),
            UPart::Wave(_) => None,
        })
        .expect("curve present")
}

fn curves(project: &UProject) -> &[UCurve] {
    match &project.parts[0] {
        UPart::Voice(vp) => &vp.curves,
        UPart::Wave(_) => unreachable!("fixture has a voice part"),
    }
}

#[test]
fn mismatched_arrays_are_truncated_and_reported_instead_of_panicking() {
    let (project, warnings) = load(&yaml_with_curve("0, 480, 960", "5", "dyn"));

    assert!(
        warnings.contains(&LoadRepair::ArraysTruncated {
            part: 0,
            curve: 0,
            xs: 3,
            ys: 1
        }),
        "expected truncation report, got {warnings:?}"
    );

    let curve = first_curve(&project);
    assert_eq!(curve.xs.len(), curve.ys.len());
    assert_eq!(curve.xs, vec![0]);
    assert_eq!(curve.ys, vec![5]);
    // The old code panicked here; sampling must now be in bounds.
    assert_eq!(curve.sample(0), Some(5));
    assert_eq!(curve.sample(480), None);
}

#[test]
fn mismatched_arrays_do_not_lose_the_shorter_side_of_valid_data() {
    // `ys` longer than `xs`: the survivors are the paired prefix.
    let (project, warnings) = load(&yaml_with_curve("0, 480", "1, 2, 3", "dyn"));
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, LoadRepair::ArraysTruncated { .. })),
        "expected truncation: {warnings:?}"
    );
    let curve = first_curve(&project);
    assert_eq!(curve.xs, vec![0, 480]);
    assert_eq!(curve.ys, vec![1, 2]);
}

#[test]
fn matching_arrays_report_nothing() {
    let (_, warnings) = load(&yaml_with_curve("0, 480", "1, 2", "dyn"));
    assert!(
        !warnings
            .iter()
            .any(|w| matches!(w, LoadRepair::ArraysTruncated { .. })),
        "unexpected truncation: {warnings:?}"
    );
}

#[test]
fn unregistered_abbr_is_reported_but_preserved() {
    let (project, warnings) = load(&yaml_with_curve("0, 480", "1, 2", "nope"));
    assert!(
        warnings.contains(&LoadRepair::UnregisteredAbbr {
            part: 0,
            abbr: "nope".into()
        }),
        "expected unregistered report, got {warnings:?}"
    );
    // Never deleted: the user may still register the descriptor.
    assert_eq!(first_curve(&project).abbr, "nope");
}

#[test]
fn duplicate_abbr_in_one_part_is_reported_but_preserved() {
    let yaml = yaml_with_curve("0, 480", "1, 2", "dyn").replace(
        "  - abbr: dyn\n    xs: [0, 480]\n    ys: [1, 2]\n",
        "  - abbr: dyn\n    xs: [0, 480]\n    ys: [1, 2]\n  - abbr: dyn\n    xs: [960]\n    ys: [3]\n",
    );
    let (project, warnings) = load(&yaml);
    assert!(
        warnings.contains(&LoadRepair::DuplicateAbbr {
            part: 0,
            abbr: "dyn".into()
        }),
        "expected duplicate report, got {warnings:?}"
    );
    assert_eq!(
        curves(&project).len(),
        2,
        "duplicate curves must not be deleted"
    );
}

#[test]
fn after_load_without_warnings_still_repairs() {
    // The legacy entry point keeps working and applies the same repair.
    let mut project: UProject =
        serde_yaml::from_str(&yaml_with_curve("0, 480, 960", "5", "dyn")).expect("parse");
    project.after_load().expect("after_load");
    let curve = first_curve(&project);
    assert_eq!(curve.xs.len(), curve.ys.len());
    assert_eq!(curve.sample(0), Some(5));
}

/// A file with no `time_signatures`/`tempos` used to panic inside
/// `TimeAxis::build_segments`. OpenUtau's constructor seeds both, so the
/// loader must apply the same defaults.
#[test]
fn missing_timing_sections_get_openutau_defaults() {
    let yaml = yaml_with_curve("0, 480", "1, 2", "dyn")
        .replace(
            "time_signatures:\n- bar_position: 0\n  beat_per_bar: 4\n  beat_unit: 4\n",
            "",
        )
        .replace("tempos:\n- position: 0\n  bpm: 120.0\n", "");
    assert!(
        !yaml.contains("time_signatures:"),
        "fixture still has time signatures"
    );
    assert!(!yaml.contains("tempos:"), "fixture still has tempos");

    let mut project: UProject = serde_yaml::from_str(&yaml).expect("parse");
    project.after_load_with_warnings().expect("after_load");

    assert_eq!(project.time_signatures.len(), 1);
    assert_eq!(project.time_signatures[0].beat_per_bar, 4);
    assert_eq!(project.time_signatures[0].beat_unit, 4);
    assert_eq!(project.tempos.len(), 1);
    assert_eq!(project.tempos[0].bpm, 120.0);
}

// ---------------------------------------------------------------------------
// Tracks (decision 2026-09-29)
// ---------------------------------------------------------------------------

/// The `tracks` block of the shared fixture, so a test can remove or empty it.
fn without_tracks(yaml: &str) -> String {
    let mut out = String::new();
    let mut skipping = false;
    for line in yaml.lines() {
        if line.starts_with("tracks:") {
            skipping = true;
            continue;
        }
        if skipping && (line.starts_with("- ") || line.starts_with("  ")) {
            continue;
        }
        skipping = false;
        out.push_str(line);
        out.push('\n');
    }
    out
}

#[test]
fn a_missing_tracks_key_is_repaired_to_track1() {
    let yaml = without_tracks(&yaml_with_curve("0, 480", "1, 2", "dyn"));
    assert!(!yaml.contains("tracks:"), "fixture still has tracks");

    let (project, warnings) = load(&yaml);

    assert!(
        warnings.contains(&LoadRepair::MissingTracksRepaired { created: 1 }),
        "expected missing_tracks_repaired, got {warnings:?}"
    );
    assert_eq!(project.tracks.len(), 1);
    assert_eq!(project.tracks[0].track_name, "Track1");
    // The repaired project is usable: the part still resolves to a track.
    assert!(
        !warnings
            .iter()
            .any(|w| matches!(w, LoadRepair::PartReferencesMissingTrack { .. })),
        "the part should reference the repaired track: {warnings:?}"
    );
}

#[test]
fn an_empty_tracks_list_is_repaired_the_same_way() {
    // OpenUtau cannot save `tracks: []` at all (its BeforeSave indexes the
    // list), so repairing it is what keeps such a file usable.
    let yaml = format!(
        "tracks: []\n{}",
        without_tracks(&yaml_with_curve("0, 480", "1, 2", "dyn"))
    );
    assert!(yaml.contains("tracks: []"));

    let (project, warnings) = load(&yaml);

    assert!(warnings.contains(&LoadRepair::MissingTracksRepaired { created: 1 }));
    assert_eq!(project.tracks.len(), 1);
    assert_eq!(project.tracks[0].track_name, "Track1");
}

#[test]
fn existing_tracks_are_left_alone() {
    let (project, warnings) = load(&yaml_with_curve("0, 480", "1, 2", "dyn"));
    assert!(
        !warnings
            .iter()
            .any(|w| matches!(w, LoadRepair::MissingTracksRepaired { .. })),
        "unexpected repair: {warnings:?}"
    );
    assert_eq!(project.tracks.len(), 1);
    assert_eq!(project.tracks[0].track_name, "Track1");
}

#[test]
fn a_part_referencing_a_missing_track_is_reported_not_invented() {
    let yaml = yaml_with_curve("0, 480", "1, 2", "dyn").replace("track_no: 0", "track_no: 2");
    assert!(yaml.contains("track_no: 2"));

    let (project, warnings) = load(&yaml);

    assert!(
        warnings.contains(&LoadRepair::PartReferencesMissingTrack {
            part: 0,
            track_no: 2
        }),
        "expected a missing-track report, got {warnings:?}"
    );
    // Never invent tracks the project never had.
    assert_eq!(project.tracks.len(), 1);
    assert!(
        !warnings
            .iter()
            .any(|w| matches!(w, LoadRepair::MissingTracksRepaired { .. })),
        "tracks were not empty, so no default track should be created"
    );
}
