//! Stable identity: reconciliation, sidecar round-trip, repair, byte safety.
//!
//! These tests are the step-1 evidence for `docs/CONTRACT-v1.md` §6.2: IDs are
//! opaque, Rust-minted, survive reorder/edit, never appear in `.ustx`, and a
//! damaged sidecar costs at most one entity's identity.

use domain::{UCurve, UNote, UPart, UProject, UVoicePart};
use project_session::identity::{
    fork_for_save_as, reconcile, table_from_json, table_to_json, EntityId, IdGenerator,
    IdentityTable, IdentityWarning, ID_SCHEMA,
};

const DEMO: &str = include_str!("../../../tools/synth-cli/tests/data/demo-song.ustx");

fn load(yaml: &str) -> UProject {
    let mut project: UProject = serde_yaml::from_str(yaml).expect("parse fixture");
    project.after_load().expect("after_load");
    project
}

fn demo() -> UProject {
    load(DEMO)
}

fn voice_part(project: &UProject) -> &UVoicePart {
    project
        .parts
        .iter()
        .find_map(|p| match p {
            UPart::Voice(vp) => Some(vp),
            UPart::Wave(_) => None,
        })
        .expect("fixture has a voice part")
}

fn voice_part_mut(project: &mut UProject) -> &mut UVoicePart {
    project
        .parts
        .iter_mut()
        .find_map(|p| match p {
            UPart::Voice(vp) => Some(vp),
            UPart::Wave(_) => None,
        })
        .expect("fixture has a voice part")
}

fn note_ids(table: &IdentityTable) -> Vec<String> {
    table.tracks[0].parts[0]
        .notes
        .iter()
        .map(|n| n.id.as_str().to_string())
        .collect()
}

fn yaml_of(project: &UProject) -> String {
    let mut copy = project.clone();
    copy.before_save();
    serde_yaml::to_string(&copy).expect("serialize project")
}

/// Re-reconciling an unchanged document keeps every id and reports nothing.
#[test]
fn reconcile_is_stable_for_an_unchanged_document() {
    let project = demo();
    let mut gen = IdGenerator::deterministic();

    let (first, warnings) = reconcile(&project, None, &mut gen);
    assert_eq!(warnings, vec![IdentityWarning::IdsRegenerated]);
    assert_eq!(first.schema, ID_SCHEMA);
    assert_eq!(first.tracks.len(), 1);
    assert_eq!(first.tracks[0].parts[0].notes.len(), 4);
    assert_eq!(first.len(), 1 + 1 + 4);

    let (second, warnings) = reconcile(&project, Some(&first), &mut gen);
    assert_eq!(second, first, "identity churned on an unchanged document");
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
}

/// Reordering entities keeps ids attached to their content.
#[test]
fn ids_survive_reorder() {
    let mut project = demo();
    let mut gen = IdGenerator::deterministic();
    let (before, _) = reconcile(&project, None, &mut gen);
    let before_notes = note_ids(&before);

    voice_part_mut(&mut project).notes.reverse();

    let (after, warnings) = reconcile(&project, Some(&before), &mut gen);
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    let after_notes = note_ids(&after);

    // Content moved, ids travelled with it: the reversed list is the reversed
    // id list.
    let mut expect = before_notes.clone();
    expect.reverse();
    assert_eq!(after_notes, expect);
}

/// Editing content in place keeps the id (positional fallback).
#[test]
fn ids_survive_an_in_place_edit() {
    let mut project = demo();
    let mut gen = IdGenerator::deterministic();
    let (before, _) = reconcile(&project, None, &mut gen);

    voice_part_mut(&mut project).notes[1].tone += 3;

    let (after, warnings) = reconcile(&project, Some(&before), &mut gen);
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    assert_eq!(note_ids(&after), note_ids(&before));
}

/// Duplicating an entity gives the copy a fresh id and keeps the original's.
#[test]
fn duplicate_entities_get_new_ids() {
    let mut project = demo();
    let mut gen = IdGenerator::deterministic();
    let (before, _) = reconcile(&project, None, &mut gen);
    let before_notes = note_ids(&before);

    let copy = voice_part(&project).notes[0].clone();
    voice_part_mut(&mut project).notes.push(copy);

    let (after, warnings) = reconcile(&project, Some(&before), &mut gen);
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    let after_notes = note_ids(&after);
    assert_eq!(after_notes.len(), 5);
    assert_eq!(&after_notes[..4], &before_notes[..], "originals changed id");
    assert_ne!(
        after_notes[4], before_notes[0],
        "copy reused the original id"
    );
    assert!(!before_notes.contains(&after_notes[4]));
}

/// Deleting an entity leaves a tombstone instead of churning the survivors.
#[test]
fn deleted_entities_become_orphans() {
    let mut project = demo();
    let mut gen = IdGenerator::deterministic();
    let (before, _) = reconcile(&project, None, &mut gen);

    voice_part_mut(&mut project).notes.remove(2);

    let (after, warnings) = reconcile(&project, Some(&before), &mut gen);
    assert!(
        warnings.contains(&IdentityWarning::OrphanedEntries { count: 1 }),
        "expected one orphan, got {warnings:?}"
    );
    assert_eq!(after.orphans.len(), 1);
    assert_eq!(after.orphans[0].kind, "note");
    assert_eq!(after.orphans[0].id.as_str(), note_ids(&before)[2]);
    let survivors = note_ids(&after);
    assert_eq!(survivors.len(), 3);
    assert_eq!(survivors[0], note_ids(&before)[0]);
    assert_eq!(survivors[1], note_ids(&before)[1]);
    assert_eq!(survivors[2], note_ids(&before)[3]);
}

/// The sidecar is the only persistence path and round-trips exactly.
#[test]
fn sidecar_round_trip_preserves_ids() {
    let project = demo();
    let mut gen = IdGenerator::deterministic();
    let (table, _) = reconcile(&project, None, &mut gen);

    let json = table_to_json(&table).expect("serialize sidecar");
    let restored = table_from_json(&json).expect("parse sidecar");
    assert_eq!(restored, table);

    let (again, warnings) = reconcile(&project, Some(&restored), &mut gen);
    assert_eq!(again, table);
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
}

/// A future sidecar schema is refused rather than partially interpreted.
#[test]
fn unknown_sidecar_schema_is_rejected() {
    let project = demo();
    let mut gen = IdGenerator::deterministic();
    let (table, _) = reconcile(&project, None, &mut gen);
    let json = table_to_json(&table)
        .unwrap()
        .replace(ID_SCHEMA, "lilt.ids/99");

    let err = table_from_json(&json).expect_err("schema must be checked");
    assert!(err.contains("lilt.ids/99"), "unexpected error: {err}");
}

/// A duplicated id costs the later entity its identity, not the project's.
#[test]
fn duplicate_id_in_sidecar_is_repaired() {
    let project = demo();
    let mut gen = IdGenerator::deterministic();
    let (table, _) = reconcile(&project, None, &mut gen);

    let mut damaged = table.clone();
    let first = damaged.tracks[0].parts[0].notes[0].id.clone();
    damaged.tracks[0].parts[0].notes[1].id = first.clone();

    let (after, warnings) = reconcile(&project, Some(&damaged), &mut gen);
    assert!(
        warnings.contains(&IdentityWarning::DuplicateIdRepaired { count: 1 }),
        "expected duplicate repair, got {warnings:?}"
    );
    assert_eq!(
        after.tracks[0].parts[0].notes[0].id, first,
        "first claim lost"
    );
    assert_ne!(after.tracks[0].parts[0].notes[1].id, first);
    assert_eq!(after.tracks[0].parts[0].notes.len(), 4);
}

/// A malformed id costs one entity's identity only.
#[test]
fn malformed_id_in_sidecar_is_repaired() {
    let project = demo();
    let mut gen = IdGenerator::deterministic();
    let (table, _) = reconcile(&project, None, &mut gen);

    let victim = table.tracks[0].parts[0].notes[2].id.as_str().to_string();
    let json = table_to_json(&table)
        .unwrap()
        .replace(&victim, "not-a-uuid");
    let damaged = table_from_json(&json).expect("lenient parse");

    let (after, warnings) = reconcile(&project, Some(&damaged), &mut gen);
    assert!(
        warnings.contains(&IdentityWarning::MalformedIdEntry { count: 1 }),
        "expected malformed repair, got {warnings:?}"
    );
    let repaired = after.tracks[0].parts[0].notes[2].id.as_str();
    assert_ne!(repaired, "not-a-uuid");
    assert!(
        EntityId::parse(repaired).is_ok(),
        "repaired id is not valid"
    );
    // Untouched siblings keep their ids.
    assert_eq!(
        after.tracks[0].parts[0].notes[0].id.as_str(),
        table.tracks[0].parts[0].notes[0].id.as_str()
    );
}

/// "Save As" forks the project identity but keeps musical entity ids.
#[test]
fn save_as_forks_project_id_only() {
    let project = demo();
    let mut gen = IdGenerator::deterministic();
    let (table, _) = reconcile(&project, None, &mut gen);

    let mut forked = table.clone();
    fork_for_save_as(&mut forked, &mut gen);
    assert_ne!(forked.project_id, table.project_id);
    assert_eq!(note_ids(&forked), note_ids(&table));
    assert_eq!(forked.document_fingerprint, None);
}

/// Curve identity follows `abbr` across a stroke edit; a new curve is new.
#[test]
fn curve_ids_survive_strokes_and_are_allocated_for_new_curves() {
    let mut project = demo();
    voice_part_mut(&mut project).curves.push(UCurve {
        abbr: "dyn".into(),
        xs: vec![0, 480],
        ys: vec![0, 10],
    });
    let mut gen = IdGenerator::deterministic();
    let (before, _) = reconcile(&project, None, &mut gen);
    let dyn_id = before.curve_id(0, 0, 0).expect("dyn curve id").clone();

    // A stroke replaces the points (fingerprint changes) but not the abbr.
    let curve = &mut voice_part_mut(&mut project).curves[0];
    curve.xs = vec![0, 120, 480];
    curve.ys = vec![0, 25, 4];

    let (after, warnings) = reconcile(&project, Some(&before), &mut gen);
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    assert_eq!(after.curve_id(0, 0, 0), Some(&dyn_id), "curve id changed");

    // A brand-new descriptor in the same part gets a fresh id.
    voice_part_mut(&mut project).curves.push(UCurve {
        abbr: "pitd".into(),
        xs: vec![0, 480],
        ys: vec![0, 100],
    });
    let (grown, warnings) = reconcile(&project, Some(&after), &mut gen);
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    assert_eq!(grown.curve_id(0, 0, 0), Some(&dyn_id));
    let new_id = grown.curve_id(0, 0, 1).expect("pitd curve id");
    assert_ne!(new_id, &dyn_id);
}

/// Identity never leaks into `.ustx`: the bytes are unchanged by reconciling.
#[test]
fn identity_does_not_change_ustx_bytes() {
    let mut project = demo();
    voice_part_mut(&mut project).curves.push(UCurve {
        abbr: "dyn".into(),
        xs: vec![0, 480],
        ys: vec![0, 10],
    });

    let before = yaml_of(&project);
    let mut gen = IdGenerator::deterministic();
    let (table, _) = reconcile(&project, None, &mut gen);
    let after = yaml_of(&project);

    assert_eq!(before, after, "identity changed the serialized document");
    assert!(!after.contains("lilt"), "sidecar data leaked into .ustx");
    assert!(!after.contains(table.project_id.as_str()));
    assert!(!after.contains(table.tracks[0].id.as_str()));
}

/// IDs are minted only through the generator and are shape-validated.
#[test]
fn generated_ids_are_unique_and_well_formed() {
    let mut gen = IdGenerator::deterministic();
    let mut seen = std::collections::HashSet::new();
    for _ in 0..512 {
        let id = gen.next_id();
        assert!(EntityId::parse(id.as_str()).is_ok(), "bad shape: {id}");
        assert!(seen.insert(id.as_str().to_string()), "duplicate id: {id}");
    }
    assert!(EntityId::parse("NOT-A-UUID").is_err());
    assert!(EntityId::parse("").is_err());
    // Same seed reproduces the same sequence (deterministic tests/vectors).
    let mut a = IdGenerator::from_seed(7);
    let mut b = IdGenerator::from_seed(7);
    assert_eq!(a.next_id(), b.next_id());
}

/// Fingerprints are host-independent (same input ⇒ same hash).
#[test]
fn fingerprints_are_deterministic() {
    let note = UNote {
        position: 480,
        duration: 240,
        tone: 60,
        ..Default::default()
    };
    assert_eq!(
        project_session::identity::fingerprint_note(&note),
        project_session::identity::fingerprint_note(&note.clone())
    );
    let mut edited = note.clone();
    edited.tone = 61;
    assert_ne!(
        project_session::identity::fingerprint_note(&note),
        project_session::identity::fingerprint_note(&edited)
    );
}
