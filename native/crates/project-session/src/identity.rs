//! Stable opaque entity identity for a project document.
//!
//! # Why identity is inferred instead of stored on the domain structs
//!
//! Commands locate domain objects by *value* (`commands/src/locate.rs`
//! compares `UNote == UNote`). Adding an `id` field to `UNote`/`UTrack`/… and
//! keeping the derived `PartialEq` would silently break relocation, and
//! removing the field from equality would require hand-written `PartialEq`
//! for every model struct. Instead the domain stays untouched and the session
//! owns an [`IdentityTable`] that is reconciled against the document:
//!
//! * **Pass A** adopts a previous entry whose *content fingerprint* matches
//!   (within the same parent) — survives reorder/move.
//! * **Pass B** adopts the next unconsumed previous entry by position —
//!   survives edits that change the fingerprint.
//! * Anything left over mints a fresh ID; unused previous entries become
//!   [`IdentityTable::orphans`] tombstones so an external tool's save does not
//!   churn identities.
//!
//! Consequences, documented in `docs/CONTRACT-v1.md` §6.2:
//! entity IDs are *inferred* from content+position, so two structurally
//! identical siblings are interchangeable and are matched in list order.
//! IDs are opaque, minted only in Rust, and are never written into `.ustx`.
//!
//! # Persistence
//!
//! The table is the only ID store. `ProjectDoc`/host adapters serialize it to
//! the sidecar `<project-file-name>.rusted-utau-ids.json` via [`table_to_json`] and
//! restore it with [`table_from_json`]. `.ustx` bytes are unaffected by this
//! module — see the module tests for the byte-stability guarantee.

use std::collections::HashSet;

use domain::{UNote, UPart, UProject, UTrack, UVoicePart, UWavePart};
use serde::{Deserialize, Serialize};

/// Sidecar schema identifier. A sidecar with an unknown schema is ignored
/// (the caller regenerates IDs) rather than partially interpreted.
pub const ID_SCHEMA: &str = "rusted-utau.ids/1";

/// Bumped whenever a fingerprint's input fields change. Recorded in the
/// sidecar so a future change can be distinguished from corruption.
pub const ID_SCHEMA_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// EntityId
// ---------------------------------------------------------------------------

/// An opaque, stable entity identifier.
///
/// Rendered as an RFC 4122 v4 UUID in lowercase hyphenated form. The inner
/// value is private: the only ways to obtain one are [`IdGenerator::next_id`]
/// (minting, Rust-only) and [`EntityId::parse`] (loading a sidecar).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntityId(String);

impl EntityId {
    /// Parse a serialized id, validating its shape.
    ///
    /// Shape validation is deliberately strict so a hand-edited or foreign
    /// sidecar cannot inject arbitrary strings as identities.
    pub fn parse(raw: &str) -> Result<EntityId, String> {
        if !is_valid_id_shape(raw) {
            return Err(format!(
                "invalid entity id {raw:?} (expected a lowercase UUID)"
            ));
        }
        Ok(EntityId(raw.to_string()))
    }

    /// The id as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EntityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// `8-4-4-4-12` lowercase hex with hyphens.
fn is_valid_id_shape(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 36 {
        return false;
    }
    for (i, c) in b.iter().enumerate() {
        match i {
            8 | 13 | 18 | 23 => {
                if *c != b'-' {
                    return false;
                }
            }
            _ => {
                if !c.is_ascii_digit() && !(b'a'..=b'f').contains(c) {
                    return false;
                }
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// IdGenerator
// ---------------------------------------------------------------------------

/// Minting source for [`EntityId`]s.
///
/// The owner of the session supplies the seed: a host must use a fresh random
/// value per session (native: OS randomness; browser: `crypto.getRandomValues`
/// passed in from JS). Tests use [`IdGenerator::deterministic`].
///
/// Uniqueness inside one document never depends on the seed quality:
/// [`reconcile`] skips any candidate already claimed by the table.
#[derive(Debug, Clone)]
pub struct IdGenerator {
    state: u64,
    counter: u64,
}

impl IdGenerator {
    /// Deterministic generator: same seed ⇒ same id sequence.
    pub fn from_seed(seed: u64) -> Self {
        let state = if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        };
        IdGenerator { state, counter: 0 }
    }

    /// Generator for tests and conformance vectors.
    pub fn deterministic() -> Self {
        Self::from_seed(0)
    }

    /// splitmix64 — a small, dependency-free, well-distributed generator.
    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Mint the next id (never returns an already-minted value for one
    /// generator instance).
    pub fn next_id(&mut self) -> EntityId {
        let a = self.next_u64();
        let b = self.next_u64() ^ self.counter;
        self.counter = self.counter.wrapping_add(1);
        let mut bytes = [0u8; 16];
        bytes[..8].copy_from_slice(&a.to_be_bytes());
        bytes[8..].copy_from_slice(&b.to_be_bytes());
        EntityId(format_uuid_v4(bytes))
    }
}

fn format_uuid_v4(mut bytes: [u8; 16]) -> String {
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // RFC 4122 variant
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

// ---------------------------------------------------------------------------
// Fingerprints (id_schema_version 1)
// ---------------------------------------------------------------------------

/// Hash a sequence of fields unambiguously: length-prefixed, unit-separated.
fn hash_fields(fields: &[&str]) -> String {
    let mut buf = String::new();
    for f in fields {
        buf.push_str(&f.len().to_string());
        buf.push(':');
        buf.push_str(f);
        buf.push('\u{1f}');
    }
    hash_bytes(buf.as_bytes())
}

fn hash_bytes(bytes: &[u8]) -> String {
    format!("{:016x}", xxhash_rust::xxh64::xxh64(bytes, 0))
}

/// Identity-relevant fields of a track: name, singer, phonemizer.
///
/// Mix state (`mute`/`solo`/`volume`/`pan`) and cosmetics (color) are
/// deliberately excluded: they change often and must never move identity.
pub fn fingerprint_track(track: &UTrack) -> String {
    hash_fields(&[
        &track.track_name,
        track.singer.as_deref().unwrap_or(""),
        track.phonemizer.as_deref().unwrap_or(""),
    ])
}

/// Identity-relevant fields of a voice part: name, track, position.
///
/// `duration` is excluded because OpenUtau recomputes it during its own load
/// (`UVoicePart.AfterLoad`), which would otherwise churn every part ID after
/// an external save.
pub fn fingerprint_part(part: &UVoicePart) -> String {
    hash_fields(&[
        &part.name,
        &part.track_no.to_string(),
        &part.position.to_string(),
    ])
}

/// Wave-part identity follows the same stable fields as voice parts. Prefix
/// the payload so a voice and wave part with matching name/track/position can
/// never exchange IDs during reconciliation.
pub fn fingerprint_wave_part(part: &UWavePart) -> String {
    hash_fields(&[
        "wave",
        &part.name,
        &part.track_no.to_string(),
        &part.position.to_string(),
    ])
}

/// Identity-relevant fields of a note: position, duration, tone, lyric.
///
/// The `id`-free subset is intentional: an edit to any of these still matches
/// by position (Pass B), so the fingerprint only needs to discriminate
/// *reordering* and *duplication*.
pub fn fingerprint_note(note: &UNote) -> String {
    hash_fields(&[
        &note.position.to_string(),
        &note.duration.to_string(),
        &note.tone.to_string(),
        &note.lyric,
    ])
}

/// Identity-relevant fields of a curve: `abbr` plus its points.
///
/// Curves have no other identity in the domain (`UVoicePart.curves` is a
/// `Vec<UCurve>`), so the points are part of the fingerprint; matching falls
/// back to `abbr`, then to position, when a stroke changes them.
pub fn fingerprint_curve(curve: &domain::UCurve) -> String {
    let mut bytes = Vec::with_capacity(curve.abbr.len() + (curve.xs.len() + curve.ys.len()) * 4);
    bytes.extend_from_slice(curve.abbr.as_bytes());
    bytes.push(0x1f);
    for x in &curve.xs {
        bytes.extend_from_slice(&x.to_le_bytes());
    }
    for y in &curve.ys {
        bytes.extend_from_slice(&y.to_le_bytes());
    }
    hash_bytes(&bytes)
}

// ---------------------------------------------------------------------------
// Table
// ---------------------------------------------------------------------------

/// The session's ID store, mirroring the document structure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IdentityTable {
    pub schema: String,
    pub project_id: EntityId,
    /// Hash of the canonical USTX text the table was last reconciled with.
    /// Optional: it is only needed when persisting (see
    /// [`set_document_fingerprint`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_fingerprint: Option<String>,
    pub tracks: Vec<TrackIds>,
    /// Entries that no longer match any entity. Kept so an external tool's
    /// save/restore round-trip does not churn identities.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub orphans: Vec<OrphanIds>,
}

/// IDs of one track and its parts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackIds {
    pub id: EntityId,
    #[serde(default)]
    pub fingerprint: String,
    #[serde(default)]
    pub parts: Vec<PartIds>,
}

/// IDs of one voice part and its children.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PartIds {
    pub id: EntityId,
    #[serde(default)]
    pub fingerprint: String,
    #[serde(default)]
    pub notes: Vec<EntityIds>,
    #[serde(default)]
    pub curves: Vec<CurveIds>,
}

/// A note's id and fingerprint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityIds {
    pub id: EntityId,
    #[serde(default)]
    pub fingerprint: String,
}

/// A curve's id, abbreviation and fingerprint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CurveIds {
    pub id: EntityId,
    pub abbr: String,
    #[serde(default)]
    pub fingerprint: String,
}

/// An entry kept after its entity disappeared.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrphanIds {
    /// `track` | `part` | `note` | `curve`
    pub kind: String,
    pub id: EntityId,
    #[serde(default)]
    pub fingerprint: String,
}

/// Something repaired or reported while reconciling identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityWarning {
    /// No usable previous table: every entity received a fresh ID.
    /// `ids_stable` is `false` for this session.
    IdsRegenerated,
    /// An id in the sidecar was reused; the later entry was repaired by
    /// minting a fresh id (the first occurrence keeps its id).
    DuplicateIdRepaired { count: usize },
    /// An id in the sidecar did not have the expected shape and was dropped.
    MalformedIdEntry { count: usize },
    /// Entries that matched no entity were moved to `orphans`.
    OrphanedEntries { count: usize },
    /// A curve's `xs`/`ys` lengths disagreed; truncated to the shorter one.
    RepairedCurveArrays,
    /// A part curve's `abbr` is not a registered project expression.
    UnregisteredCurveAbbr { abbr: String },
    /// More than one curve in a part shares an `abbr`.
    DuplicateCurveAbbr { abbr: String },
    /// A Rusted UTAU pronunciation sidecar was present but could not be safely
    /// adopted; the USTX remains open with those choices dropped.
    PronunciationSidecarIgnored,
    /// The project had no `tracks`; a default `Track1` was created.
    MissingTracksRepaired { created: usize },
    /// A voice part references a track index that does not exist.
    PartReferencesMissingTrack { part: usize, track_no: i32 },
}

impl IdentityWarning {
    /// Stable machine-readable code (frontends branch on this, not on text).
    pub fn code(&self) -> &'static str {
        match self {
            IdentityWarning::IdsRegenerated => "ids_regenerated",
            IdentityWarning::DuplicateIdRepaired { .. } => "duplicate_id_repaired",
            IdentityWarning::MalformedIdEntry { .. } => "malformed_id_entry",
            IdentityWarning::OrphanedEntries { .. } => "orphaned_entries",
            IdentityWarning::RepairedCurveArrays => "repaired_curve_arrays",
            IdentityWarning::UnregisteredCurveAbbr { .. } => "unregistered_curve_abbr",
            IdentityWarning::DuplicateCurveAbbr { .. } => "duplicate_curve_abbr",
            IdentityWarning::PronunciationSidecarIgnored => "pronunciation_sidecar_ignored",
            IdentityWarning::MissingTracksRepaired { .. } => "missing_tracks_repaired",
            IdentityWarning::PartReferencesMissingTrack { .. } => "part_references_missing_track",
        }
    }
}

// ---------------------------------------------------------------------------
// Reconciliation
// ---------------------------------------------------------------------------

/// Previous entry used as a matching candidate.
#[derive(Debug, Clone)]
struct PrevEntry {
    id: Option<EntityId>,
    fingerprint: String,
    abbr: Option<String>,
}

impl IdentityTable {
    /// Build a table for a document with no previous identity information.
    pub fn fresh(
        project: &UProject,
        gen: &mut IdGenerator,
    ) -> (IdentityTable, Vec<IdentityWarning>) {
        reconcile(project, None, gen)
    }

    /// Look up a track id by index.
    pub fn track_id(&self, track: usize) -> Option<&EntityId> {
        self.tracks.get(track).map(|t| &t.id)
    }

    /// Look up a part id by (track, part) index.
    pub fn part_id(&self, track: usize, part: usize) -> Option<&EntityId> {
        self.tracks.get(track)?.parts.get(part).map(|p| &p.id)
    }

    /// Look up a note id by (track, part, note) index.
    pub fn note_id(&self, track: usize, part: usize, note: usize) -> Option<&EntityId> {
        self.tracks
            .get(track)?
            .parts
            .get(part)?
            .notes
            .get(note)
            .map(|n| &n.id)
    }

    /// Look up a curve id by (track, part, curve) index.
    pub fn curve_id(&self, track: usize, part: usize, curve: usize) -> Option<&EntityId> {
        self.tracks
            .get(track)?
            .parts
            .get(part)?
            .curves
            .get(curve)
            .map(|c| &c.id)
    }

    /// Reverse lookup: `(track, part-in-track, note-in-part)` for a note id.
    pub fn find_note(&self, id: &str) -> Option<(usize, usize, usize)> {
        for (t, track) in self.tracks.iter().enumerate() {
            for (p, part) in track.parts.iter().enumerate() {
                for (n, note) in part.notes.iter().enumerate() {
                    if note.id.as_str() == id {
                        return Some((t, p, n));
                    }
                }
            }
        }
        None
    }

    /// Reverse lookup: `(track, part-in-track, curve-in-part)` for a curve id.
    pub fn find_curve(&self, id: &str) -> Option<(usize, usize, usize)> {
        for (t, track) in self.tracks.iter().enumerate() {
            for (p, part) in track.parts.iter().enumerate() {
                for (c, curve) in part.curves.iter().enumerate() {
                    if curve.id.as_str() == id {
                        return Some((t, p, c));
                    }
                }
            }
        }
        None
    }

    /// Reverse lookup: `(track, part-in-track)` for a part id.
    pub fn find_part(&self, id: &str) -> Option<(usize, usize)> {
        for (t, track) in self.tracks.iter().enumerate() {
            for (p, part) in track.parts.iter().enumerate() {
                if part.id.as_str() == id {
                    return Some((t, p));
                }
            }
        }
        None
    }

    /// Reverse lookup: track index for a track id.
    pub fn find_track(&self, id: &str) -> Option<usize> {
        self.tracks.iter().position(|t| t.id.as_str() == id)
    }

    /// Every note id currently bound (used for added/removed diffs).
    pub fn note_id_set(&self) -> HashSet<String> {
        let mut out = HashSet::new();
        for track in &self.tracks {
            for part in &track.parts {
                for note in &part.notes {
                    out.insert(note.id.as_str().to_string());
                }
            }
        }
        out
    }

    /// Total number of IDs carried (tracks + parts + notes + curves).
    pub fn len(&self) -> usize {
        let mut n = self.tracks.len();
        for t in &self.tracks {
            n += t.parts.len();
            for p in &t.parts {
                n += p.notes.len() + p.curves.len();
            }
        }
        n
    }

    /// Whether the table carries no identities at all.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Fork a table for "Save As": the copied document is a new project, but its
/// musical entities keep their ids so history stays continuous.
pub fn fork_for_save_as(table: &mut IdentityTable, gen: &mut IdGenerator) {
    table.project_id = gen.next_id();
    table.document_fingerprint = None;
}

/// Voice parts of `track`, in project order.
///
/// This ordering is the contract between [`reconcile`] (which builds the
/// parallel `IdentityTable` arrays) and any snapshot/view that reports IDs by
/// `(track, part, …)` index. Both sides must use it.
pub fn voice_parts_of_track(project: &UProject, track: usize) -> Vec<&UVoicePart> {
    project
        .parts
        .iter()
        .filter_map(|part| match part {
            UPart::Voice(vp) if vp.track_no == track as i32 => Some(vp),
            _ => None,
        })
        .collect()
}

/// All part kinds of `track`, in the same project ordering used by USTX.
pub fn parts_of_track(project: &UProject, track: usize) -> Vec<&UPart> {
    project
        .parts
        .iter()
        .filter(|part| part.track_no() == track as i32)
        .collect()
}

fn fingerprint_upart(part: &UPart) -> String {
    match part {
        UPart::Voice(part) => fingerprint_part(part),
        UPart::Wave(part) => fingerprint_wave_part(part),
    }
}

/// Reconcile `project` against `previous`, minting IDs where nothing matches.
///
/// `previous == None` means "no usable sidecar": everything is minted and an
/// [`IdentityWarning::IdsRegenerated`] warning is returned.
pub fn reconcile(
    project: &UProject,
    previous: Option<&IdentityTable>,
    gen: &mut IdGenerator,
) -> (IdentityTable, Vec<IdentityWarning>) {
    let mut warnings = Vec::new();
    let mut used: HashSet<String> = HashSet::new();

    if previous.is_none() {
        warnings.push(IdentityWarning::IdsRegenerated);
    }
    let malformed = std::cell::Cell::new(0usize);
    let duplicate = std::cell::Cell::new(0usize);

    let project_id = match previous {
        Some(p) if is_valid_id_shape(p.project_id.as_str()) => {
            used.insert(p.project_id.as_str().to_string());
            p.project_id.clone()
        }
        _ => gen.next_id(),
    };

    let previous_tracks: &[TrackIds] = previous.map(|p| p.tracks.as_slice()).unwrap_or(&[]);
    let mut track_entries: Vec<PrevEntry> = previous_tracks
        .iter()
        .map(|t| PrevEntry {
            id: Some(t.id.clone()),
            fingerprint: t.fingerprint.clone(),
            abbr: None,
        })
        .collect();
    track_entries.extend(orphan_candidates(previous, "track"));
    let track_fps: Vec<String> = project.tracks.iter().map(fingerprint_track).collect();
    let track_match = match_list(
        &track_fps,
        &[],
        &track_entries,
        gen,
        &mut used,
        &malformed,
        &duplicate,
    );

    let mut tracks = Vec::with_capacity(project.tracks.len());
    let mut orphans: Vec<OrphanIds> = Vec::new();

    for track_index in 0..project.tracks.len() {
        let track_parts = parts_of_track(project, track_index);

        let prev_parts: &[PartIds] = previous_tracks
            .get(track_index)
            .map(|t| t.parts.as_slice())
            .unwrap_or(&[]);
        let mut part_entries: Vec<PrevEntry> = prev_parts
            .iter()
            .map(|p| PrevEntry {
                id: Some(p.id.clone()),
                fingerprint: p.fingerprint.clone(),
                abbr: None,
            })
            .collect();
        part_entries.extend(orphan_candidates(previous, "part"));
        let part_fps: Vec<String> = track_parts.iter().map(|p| fingerprint_upart(p)).collect();
        let part_match = match_list(
            &part_fps,
            &[],
            &part_entries,
            gen,
            &mut used,
            &malformed,
            &duplicate,
        );

        let mut parts = Vec::with_capacity(track_parts.len());
        for (part_index, part) in track_parts.iter().enumerate() {
            let prev_part = prev_parts.get(part_index);
            let (note_match, curve_match, note_fps, curve_fps, curve_abbrs) = match part {
                UPart::Voice(vp) => {
                    let mut note_entries: Vec<PrevEntry> = prev_part
                        .map(|p| {
                            p.notes
                                .iter()
                                .map(|n| PrevEntry {
                                    id: Some(n.id.clone()),
                                    fingerprint: n.fingerprint.clone(),
                                    abbr: None,
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    note_entries.extend(orphan_candidates(previous, "note"));
                    let note_fps: Vec<String> = vp.notes.iter().map(fingerprint_note).collect();
                    let note_match = match_list(
                        &note_fps,
                        &[],
                        &note_entries,
                        gen,
                        &mut used,
                        &malformed,
                        &duplicate,
                    );
                    let mut curve_entries: Vec<PrevEntry> = prev_part
                        .map(|p| {
                            p.curves
                                .iter()
                                .map(|c| PrevEntry {
                                    id: Some(c.id.clone()),
                                    fingerprint: c.fingerprint.clone(),
                                    abbr: Some(c.abbr.clone()),
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    curve_entries.extend(orphan_candidates(previous, "curve"));
                    let curve_fps: Vec<String> = vp.curves.iter().map(fingerprint_curve).collect();
                    let curve_abbrs: Vec<String> =
                        vp.curves.iter().map(|c| c.abbr.clone()).collect();
                    let curve_match = match_list(
                        &curve_fps,
                        &curve_abbrs,
                        &curve_entries,
                        gen,
                        &mut used,
                        &malformed,
                        &duplicate,
                    );
                    (note_match, curve_match, note_fps, curve_fps, curve_abbrs)
                }
                UPart::Wave(_) => {
                    let note_orphans: Vec<_> = prev_part
                        .into_iter()
                        .flat_map(|p| p.notes.iter())
                        .map(|n| (n.id.clone(), n.fingerprint.clone()))
                        .collect();
                    let curve_orphans: Vec<_> = prev_part
                        .into_iter()
                        .flat_map(|p| p.curves.iter())
                        .map(|c| (c.id.clone(), c.fingerprint.clone()))
                        .collect();
                    (
                        ListMatch {
                            ids: Vec::new(),
                            orphans: note_orphans,
                        },
                        ListMatch {
                            ids: Vec::new(),
                            orphans: curve_orphans,
                        },
                        Vec::new(),
                        Vec::new(),
                        Vec::new(),
                    )
                }
            };

            for (id, fp) in note_match.orphans.iter() {
                orphans.push(OrphanIds {
                    kind: "note".to_string(),
                    id: id.clone(),
                    fingerprint: fp.clone(),
                });
            }

            for (id, fp) in curve_match.orphans.iter() {
                orphans.push(OrphanIds {
                    kind: "curve".to_string(),
                    id: id.clone(),
                    fingerprint: fp.clone(),
                });
            }

            parts.push(PartIds {
                id: part_match.ids[part_index].clone(),
                fingerprint: part_fps[part_index].clone(),
                notes: note_match
                    .ids
                    .iter()
                    .zip(note_fps.iter())
                    .map(|(id, fp)| EntityIds {
                        id: id.clone(),
                        fingerprint: fp.clone(),
                    })
                    .collect(),
                curves: curve_match
                    .ids
                    .iter()
                    .zip(curve_fps.iter())
                    .zip(curve_abbrs.iter())
                    .map(|((id, fp), abbr)| CurveIds {
                        id: id.clone(),
                        abbr: abbr.clone(),
                        fingerprint: fp.clone(),
                    })
                    .collect(),
            });
        }

        for (id, fp) in part_match.orphans {
            orphans.push(OrphanIds {
                kind: "part".to_string(),
                id,
                fingerprint: fp,
            });
        }

        tracks.push(TrackIds {
            id: track_match.ids[track_index].clone(),
            fingerprint: track_fps[track_index].clone(),
            parts,
        });
    }

    for (id, fp) in track_match.orphans {
        orphans.push(OrphanIds {
            kind: "track".to_string(),
            id,
            fingerprint: fp,
        });
    }

    if let Some(previous) = previous {
        for entry in &previous.orphans {
            if !used.contains(entry.id.as_str()) && is_valid_id_shape(entry.id.as_str()) {
                orphans.push(entry.clone());
            }
        }
    }
    let mut seen_orphans = HashSet::new();
    orphans.retain(|entry| seen_orphans.insert(entry.id.as_str().to_string()));

    if malformed.get() > 0 {
        warnings.push(IdentityWarning::MalformedIdEntry {
            count: malformed.get(),
        });
    }
    if duplicate.get() > 0 {
        warnings.push(IdentityWarning::DuplicateIdRepaired {
            count: duplicate.get(),
        });
    }
    if !orphans.is_empty() {
        warnings.push(IdentityWarning::OrphanedEntries {
            count: orphans.len(),
        });
    }

    let table = IdentityTable {
        schema: ID_SCHEMA.to_string(),
        project_id,
        document_fingerprint: previous.and_then(|p| p.document_fingerprint.clone()),
        tracks,
        orphans,
    };
    (table, warnings)
}

fn orphan_candidates(previous: Option<&IdentityTable>, kind: &str) -> Vec<PrevEntry> {
    previous
        .into_iter()
        .flat_map(|table| table.orphans.iter())
        .filter(|entry| entry.kind == kind)
        .map(|entry| PrevEntry {
            id: Some(entry.id.clone()),
            fingerprint: entry.fingerprint.clone(),
            abbr: None,
        })
        .collect()
}

/// Outcome of matching one flat list of entities against previous entries.
struct ListMatch {
    ids: Vec<EntityId>,
    /// Unmatched previous entries as `(id, fingerprint)` tombstones.
    orphans: Vec<(EntityId, String)>,
}

/// Pass A (fingerprint + optional abbr) → Pass A2 (abbr) → Pass B (position).
fn match_list(
    new_fingerprints: &[String],
    new_abbrs: &[String],
    previous: &[PrevEntry],
    gen: &mut IdGenerator,
    used: &mut HashSet<String>,
    malformed: &std::cell::Cell<usize>,
    duplicate: &std::cell::Cell<usize>,
) -> ListMatch {
    let mut consumed = vec![false; previous.len()];
    let mut ids: Vec<Option<EntityId>> = vec![None; new_fingerprints.len()];

    // Pass A: same fingerprint (and same abbr when the kind has one).
    for i in 0..new_fingerprints.len() {
        let abbr = new_abbrs.get(i);
        if let Some(j) = (0..previous.len()).find(|&j| {
            !consumed[j]
                && previous[j].fingerprint == new_fingerprints[i]
                && match (abbr, previous[j].abbr.as_ref()) {
                    (Some(a), Some(b)) => a == b,
                    (None, None) => true,
                    _ => false,
                }
        }) {
            consumed[j] = true;
            ids[i] = Some(adopt(&previous[j].id, gen, used, malformed, duplicate));
        }
    }

    // Pass A2: same abbr, points changed (curves only).
    if !new_abbrs.is_empty() {
        for i in 0..new_fingerprints.len() {
            if ids[i].is_some() {
                continue;
            }
            let abbr = &new_abbrs[i];
            if let Some(j) = (0..previous.len())
                .find(|&j| !consumed[j] && previous[j].abbr.as_ref() == Some(abbr))
            {
                consumed[j] = true;
                ids[i] = Some(adopt(&previous[j].id, gen, used, malformed, duplicate));
            }
        }
    }

    // Pass B: positional.
    for i in 0..new_fingerprints.len() {
        if ids[i].is_some() {
            continue;
        }
        if let Some(j) = (0..previous.len()).find(|&j| !consumed[j]) {
            consumed[j] = true;
            ids[i] = Some(adopt(&previous[j].id, gen, used, malformed, duplicate));
        }
    }

    // Anything still unmatched mints a fresh id.
    for slot in ids.iter_mut() {
        if slot.is_none() {
            *slot = Some(mint(gen, used));
        }
    }

    // Unused previous entries become tombstones (their ids stay reserved).
    let mut orphans = Vec::new();
    for (j, entry) in previous.iter().enumerate() {
        if consumed[j] {
            continue;
        }
        if let Some(id) = &entry.id {
            if is_valid_id_shape(id.as_str()) {
                used.insert(id.as_str().to_string());
                orphans.push((id.clone(), entry.fingerprint.clone()));
            }
        }
    }

    ListMatch {
        ids: ids.into_iter().flatten().collect(),
        orphans,
    }
}

/// Take a previous id, or mint a fresh one when it is missing/unusable.
fn adopt(
    previous: &Option<EntityId>,
    gen: &mut IdGenerator,
    used: &mut HashSet<String>,
    malformed: &std::cell::Cell<usize>,
    duplicate: &std::cell::Cell<usize>,
) -> EntityId {
    if let Some(id) = previous {
        if is_valid_id_shape(id.as_str()) {
            if used.insert(id.as_str().to_string()) {
                return id.clone();
            }
            duplicate.set(duplicate.get() + 1);
        } else {
            malformed.set(malformed.get() + 1);
        }
    }
    mint(gen, used)
}

fn mint(gen: &mut IdGenerator, used: &mut HashSet<String>) -> EntityId {
    loop {
        let candidate = gen.next_id();
        if used.insert(candidate.as_str().to_string()) {
            return candidate;
        }
    }
}

/// Record the canonical document hash the table was reconciled with.
pub fn set_document_fingerprint(table: &mut IdentityTable, canonical_ustx: &str) {
    table.document_fingerprint = Some(hash_bytes(canonical_ustx.as_bytes()));
}

// ---------------------------------------------------------------------------
// Sidecar (de)serialization
// ---------------------------------------------------------------------------

/// Serialize the table as the sidecar payload.
pub fn table_to_json(table: &IdentityTable) -> Result<String, String> {
    serde_json::to_string_pretty(table).map_err(|e| format!("serialize id table: {e}"))
}

/// Parse a sidecar payload.
///
/// An unknown `schema` is an error (the caller then regenerates IDs) so a
/// future format is never partially interpreted. Duplicate and malformed ids
/// are *not* rejected here — [`reconcile`] repairs them and reports warnings,
/// because a corrupt id should cost one entity's identity, not the project.
pub fn table_from_json(json: &str) -> Result<IdentityTable, String> {
    let table: IdentityTable =
        serde_json::from_str(json).map_err(|e| format!("parse id table: {e}"))?;
    if table.schema != ID_SCHEMA {
        return Err(format!(
            "unsupported id table schema {:?} (expected {ID_SCHEMA})",
            table.schema
        ));
    }
    Ok(table)
}
