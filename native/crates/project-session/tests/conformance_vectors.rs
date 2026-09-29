//! Conformance vector runner (CONTRACT-v1 §6.17).
//!
//! The vectors in `tests/vectors/session_vectors.json` are transport-neutral:
//! they name entities through symbolic references (`@note:0`, `@part:0`,
//! `@last_note:`), so the same file can be replayed against the native HTTP
//! adapter or the WASM facade without rewrites. This runner drives
//! `project-session` directly, which makes it the reference implementation of
//! the expected outcomes.

use std::fs;
use std::path::PathBuf;

use project_session::Session;
use serde::Deserialize;
use serde_json::{json, Value};

const DEMO: &str = include_str!("../../../tools/synth-cli/tests/data/demo-song.ustx");

const CURVE_PROJECT: &str = r#"ustx_version: 0.9
name: Curve Session
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
- track_name: Lead
  singer: teto
voice_parts:
- name: Part1
  track_no: 0
  position: 0
  duration: 1920
  notes:
  - position: 0
    duration: 480
    tone: 60
    lyric: la
  curves:
  - abbr: dyn
    xs: [0, 480]
    ys: [0, 10]
"#;

#[derive(Debug, Deserialize)]
struct Suite {
    vectors: Vec<Vector>,
}

#[derive(Debug, Deserialize)]
struct Vector {
    name: String,
    project: String,
    #[serde(default)]
    seed: u64,
    /// Present while the area is not implemented: the vector is reported as
    /// not-run instead of silently passing (§6.17 acceptance rule).
    #[serde(default)]
    skip: Option<String>,
    #[serde(default)]
    steps: Vec<Step>,
}

#[derive(Debug, Deserialize)]
struct Step {
    op: String,
    #[serde(default)]
    command: Option<Value>,
    #[serde(default)]
    name: Option<String>,
    /// `"current"` (default), `"stale"`, or an explicit revision number.
    #[serde(default)]
    revision: Option<Value>,
    #[serde(default)]
    expect: Option<Expect>,
}

#[derive(Debug, Default, Deserialize)]
struct Expect {
    label: Option<String>,
    revision: Option<u64>,
    changed: Option<bool>,
    /// Expected error code; absent means "this step must succeed".
    code: Option<String>,
    /// Expected `details.reason` for a validation error.
    reason: Option<String>,
    /// Expected change kinds (compared as a sorted set).
    changes: Option<Vec<String>>,
    ids_stable: Option<bool>,
    note_count: Option<usize>,
    track_count: Option<usize>,
    point_count: Option<usize>,
    abbr: Option<String>,
}

fn fixture(name: &str) -> &'static str {
    match name {
        "demo" => DEMO,
        "curve" => CURVE_PROJECT,
        other => panic!("unknown vector fixture '{other}'"),
    }
}

fn vectors_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/vectors/session_vectors.json")
}

fn suite() -> Suite {
    let raw = fs::read_to_string(vectors_path()).expect("read vectors");
    serde_json::from_str(&raw).expect("parse vectors")
}

/// Outcome of one step, normalized for assertions.
#[derive(Debug, Default)]
struct Outcome {
    revision: u64,
    changed: Option<bool>,
    code: Option<String>,
    reason: Option<String>,
    changes: Vec<String>,
    ids_stable: Option<bool>,
    note_count: Option<usize>,
    track_count: Option<usize>,
    point_count: Option<usize>,
    abbr: Option<String>,
}

struct Runner {
    session: Session,
    group_id: Option<String>,
    last_note_id: Option<String>,
    last_curve_id: Option<String>,
}

impl Runner {
    fn new(project: &str, seed: u64) -> Self {
        Self {
            session: Session::open(fixture(project), None, seed).expect("open fixture"),
            group_id: None,
            last_note_id: None,
            last_curve_id: None,
        }
    }

    /// Resolve `@kind:index` references against the current session.
    fn resolve(&self, value: &Value) -> Value {
        match value {
            Value::String(s) if s.starts_with('@') => {
                Value::String(self.resolve_ref(s).unwrap_or_else(|e| panic!("{e}")))
            }
            Value::Array(items) => Value::Array(items.iter().map(|i| self.resolve(i)).collect()),
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(k, v)| (k.clone(), self.resolve(v)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    fn resolve_ref(&self, reference: &str) -> Result<String, String> {
        // `@kind:N`, or `@kind:` / `@kind` for kinds that carry no index
        // (`@last_note`, `@last_curve`).
        let (kind, raw_index) = match reference.split_once(':') {
            Some((kind, rest)) => (kind, rest),
            None => (reference, "0"),
        };
        let index: usize = if raw_index.is_empty() {
            0
        } else {
            raw_index
                .parse()
                .map_err(|_| format!("bad ref index in '{reference}'"))?
        };
        let ids = self.session.identity();
        let track = ids.tracks.first().ok_or("no track in table")?;
        match kind {
            "@track" => {
                if index == 0 {
                    Ok(track.id.as_str().to_string())
                } else {
                    Err(format!("no track {index}"))
                }
            }
            "@part" => track
                .parts
                .get(index)
                .map(|p| p.id.as_str().to_string())
                .ok_or_else(|| format!("no part {index}")),
            "@note" => track
                .parts
                .first()
                .and_then(|p| p.notes.get(index))
                .map(|n| n.id.as_str().to_string())
                .ok_or_else(|| format!("no note {index}")),
            "@curve" => track
                .parts
                .first()
                .and_then(|p| p.curves.get(index))
                .map(|c| c.id.as_str().to_string())
                .ok_or_else(|| format!("no curve {index}")),
            "@last_note" => self
                .last_note_id
                .clone()
                .ok_or_else(|| "no note.added reported yet".to_string()),
            "@last_curve" => self
                .last_curve_id
                .clone()
                .ok_or_else(|| "no curve.changed reported yet".to_string()),
            other => Err(format!("unknown ref kind '{other}'")),
        }
    }

    fn revision_for(&self, spec: &Option<Value>) -> u64 {
        match spec {
            None => self.session.revision(),
            Some(Value::String(s)) if s == "current" => self.session.revision(),
            Some(Value::String(s)) if s == "stale" => {
                if self.session.revision() == 0 {
                    7
                } else {
                    self.session.revision() - 1
                }
            }
            Some(Value::Number(n)) => n.as_u64().expect("numeric revision"),
            Some(other) => panic!("unsupported revision spec {other}"),
        }
    }

    fn record_changes(&mut self, changes: &[project_session::Change]) {
        if let Some(added) = changes.iter().find(|c| c.kind == "note.added") {
            self.last_note_id = added.note_id.clone();
        }
        if let Some(curve) = changes.iter().find(|c| c.kind == "curve.changed") {
            if let Some(id) = &curve.curve_id {
                self.last_curve_id = Some(id.clone());
            }
        }
    }

    fn step(&mut self, step: &Step) -> Outcome {
        let revision = self.revision_for(&step.revision);
        match step.op.as_str() {
            "apply" => {
                let command = self.resolve(step.command.as_ref().expect("apply needs a command"));
                let request =
                    json!({"expected_revision": revision, "command": command}).to_string();
                match self.session.apply_v2(&request) {
                    Ok(result) => {
                        self.record_changes(&result.changes);
                        // A `curve.changed` patch must carry the canonical
                        // points; surface their count for the vectors.
                        let point_count = result
                            .changes
                            .iter()
                            .find(|c| c.kind == "curve.changed")
                            .and_then(|c| c.data.as_ref())
                            .and_then(|d| d.get("points"))
                            .and_then(Value::as_array)
                            .map(Vec::len);
                        Outcome {
                            revision: result.revision,
                            changed: Some(result.changed),
                            changes: kinds(&result.changes),
                            point_count,
                            ..Default::default()
                        }
                    }
                    Err(err) => error_outcome(&err),
                }
            }
            "group_begin" => {
                let name = step.name.clone().unwrap_or_else(|| "Edit".to_string());
                match self.session.group_begin(revision, &name) {
                    Ok(begin) => {
                        self.group_id = Some(begin.group_id);
                        Outcome {
                            revision: begin.base_revision,
                            ..Default::default()
                        }
                    }
                    Err(err) => error_outcome(&err),
                }
            }
            "group_child" => {
                let group_id = self
                    .group_id
                    .clone()
                    .unwrap_or_else(|| "stale-group".to_string());
                let command = self.resolve(step.command.as_ref().expect("child needs a command"));
                let request = json!({
                    "expected_revision": revision,
                    "group_id": group_id,
                    "command": command,
                })
                .to_string();
                match self.session.apply_v2(&request) {
                    Ok(result) => Outcome {
                        revision: result.revision,
                        changed: Some(result.changed),
                        changes: kinds(&result.changes),
                        ..Default::default()
                    },
                    Err(err) => {
                        let outcome = error_outcome(&err);
                        if outcome.code.as_deref() == Some("group_failed") {
                            // Fail-closed: the group is gone.
                            self.group_id = None;
                        }
                        outcome
                    }
                }
            }
            "group_commit" => {
                let group_id = self
                    .group_id
                    .clone()
                    .unwrap_or_else(|| "stale-group".to_string());
                match self.session.group_commit(&group_id, revision) {
                    Ok(result) => {
                        self.group_id = None;
                        self.record_changes(&result.changes);
                        Outcome {
                            revision: result.revision,
                            changed: Some(result.changed),
                            changes: kinds(&result.changes),
                            ..Default::default()
                        }
                    }
                    Err(err) => {
                        if err.is_conflict() {
                            self.group_id = None;
                        }
                        error_outcome(&err)
                    }
                }
            }
            "group_cancel" => {
                let group_id = self
                    .group_id
                    .clone()
                    .unwrap_or_else(|| "stale-group".to_string());
                match self.session.group_cancel(&group_id) {
                    Ok(()) => {
                        self.group_id = None;
                        Outcome {
                            revision: self.session.revision(),
                            ..Default::default()
                        }
                    }
                    Err(err) => error_outcome(&err),
                }
            }
            "undo" | "redo" => {
                let result = if step.op == "undo" {
                    self.session.undo()
                } else {
                    self.session.redo()
                };
                match result {
                    Ok(Some(outcome)) => Outcome {
                        revision: outcome.revision,
                        changed: Some(true),
                        ..Default::default()
                    },
                    Ok(None) => Outcome {
                        revision: self.session.revision(),
                        changed: Some(false),
                        ..Default::default()
                    },
                    Err(err) => Outcome {
                        revision: self.session.revision(),
                        code: Some("internal_error".to_string()),
                        reason: Some(err),
                        ..Default::default()
                    },
                }
            }
            "snapshot" => {
                let value: Value =
                    serde_json::from_str(&self.session.snapshot_json().expect("snapshot"))
                        .expect("snapshot json");
                Outcome {
                    revision: value["revision"].as_u64().unwrap_or_default(),
                    ids_stable: value["ids_stable"].as_bool(),
                    ..Default::default()
                }
            }
            "read_notes" => {
                let part = self.resolve_ref("@part:0").expect("part ref");
                match self.session.part_notes_json(&part) {
                    Ok(json) => {
                        let value: Value = serde_json::from_str(&json).expect("notes json");
                        Outcome {
                            revision: value["revision"].as_u64().unwrap_or_default(),
                            note_count: value["notes"].as_array().map(Vec::len),
                            ..Default::default()
                        }
                    }
                    Err(err) => error_outcome(&err),
                }
            }
            "read_curve" => {
                let part = self.resolve_ref("@part:0").expect("part ref");
                // A missing curve is a legitimate vector expectation (a `clear`
                // can remove it), so report it instead of panicking.
                let curve = match self.resolve_ref("@curve:0") {
                    Ok(id) => id,
                    Err(_) => {
                        return Outcome {
                            code: Some("not_found".to_string()),
                            ..Default::default()
                        }
                    }
                };
                match self.session.curve_json(&part, &curve) {
                    Ok(json) => {
                        let value: Value = serde_json::from_str(&json).expect("curve json");
                        Outcome {
                            revision: value["revision"].as_u64().unwrap_or_default(),
                            abbr: value["abbr"].as_str().map(str::to_string),
                            point_count: value["points"].as_array().map(Vec::len),
                            ..Default::default()
                        }
                    }
                    Err(err) => error_outcome(&err),
                }
            }
            "read_tracks" => match self.session.tracks_json() {
                Ok(json) => {
                    let value: Value = serde_json::from_str(&json).expect("tracks json");
                    Outcome {
                        revision: value["revision"].as_u64().unwrap_or_default(),
                        track_count: value["tracks"].as_array().map(Vec::len),
                        ..Default::default()
                    }
                }
                Err(err) => error_outcome(&err),
            },
            other => panic!("unknown vector op '{other}'"),
        }
    }
}

fn kinds(changes: &[project_session::Change]) -> Vec<String> {
    let mut out: Vec<String> = changes.iter().map(|c| c.kind.clone()).collect();
    out.sort();
    out.dedup();
    out
}

fn error_outcome(err: &project_session::SessionError) -> Outcome {
    Outcome {
        revision: 0,
        code: Some(err.code().to_string()),
        reason: err
            .details
            .get("reason")
            .and_then(Value::as_str)
            .map(str::to_string),
        ..Default::default()
    }
}

fn check(vector: &str, index: usize, step: &Step, outcome: &Outcome) {
    let default_expect = Expect::default();
    let expect = step.expect.as_ref().unwrap_or(&default_expect);
    let where_ = format!("vector '{vector}' step {index} ({})", step.op);

    match &expect.code {
        Some(code) => {
            assert_eq!(
                outcome.code.as_deref(),
                Some(code.as_str()),
                "{where_}: expected error '{code}', got {:?} (reason {:?})",
                outcome.code,
                outcome.reason
            );
            return;
        }
        None => assert_eq!(
            outcome.code, None,
            "{where_}: expected success, got error {:?} (reason {:?})",
            outcome.code, outcome.reason
        ),
    }

    if let Some(reason) = &expect.reason {
        assert_eq!(outcome.reason.as_deref(), Some(reason.as_str()), "{where_}");
    }
    if let Some(revision) = expect.revision {
        assert_eq!(outcome.revision, revision, "{where_}: revision");
    }
    if let Some(changed) = expect.changed {
        assert_eq!(outcome.changed, Some(changed), "{where_}: changed flag");
    }
    if let Some(expected) = &expect.changes {
        let mut expected = expected.clone();
        expected.sort();
        expected.dedup();
        assert_eq!(outcome.changes, expected, "{where_}: change kinds");
    }
    if let Some(ids_stable) = expect.ids_stable {
        assert_eq!(outcome.ids_stable, Some(ids_stable), "{where_}: ids_stable");
    }
    if let Some(count) = expect.note_count {
        assert_eq!(outcome.note_count, Some(count), "{where_}: note count");
    }
    if let Some(count) = expect.track_count {
        assert_eq!(outcome.track_count, Some(count), "{where_}: track count");
    }
    if let Some(count) = expect.point_count {
        assert_eq!(outcome.point_count, Some(count), "{where_}: point count");
    }
    if let Some(abbr) = &expect.abbr {
        assert_eq!(
            outcome.abbr.as_deref(),
            Some(abbr.as_str()),
            "{where_}: abbr"
        );
    }
}

#[test]
fn session_conformance_vectors() {
    let suite = suite();
    assert!(suite.vectors.len() >= 10, "the suite must stay meaningful");

    let mut ran = 0usize;
    let mut skipped: Vec<(&str, &str)> = Vec::new();

    for vector in &suite.vectors {
        if let Some(reason) = &vector.skip {
            // Not-run, and said so: a skip must never look like a pass.
            assert!(
                vector.steps.is_empty(),
                "a skipped vector must not carry steps: {}",
                vector.name
            );
            assert!(!reason.trim().is_empty(), "a skip needs a reason: {}", vector.name);
            skipped.push((vector.name.as_str(), reason.as_str()));
            continue;
        }
        ran += 1;
        let mut runner = Runner::new(&vector.project, vector.seed);
        for (index, step) in vector.steps.iter().enumerate() {
            let outcome = runner.step(step);
            check(&vector.name, index, step, &outcome);
        }
    }

    eprintln!("conformance vectors: ran {ran}, skipped {}", skipped.len());
    for (name, reason) in &skipped {
        eprintln!("  SKIPPED (not run): {name} — {reason}");
    }
    assert!(ran >= 10, "only {ran} vectors actually ran");
}

#[test]
fn every_vector_names_a_known_fixture_and_op() {
    // Catches typos in the JSON without needing the vector to fail first.
    let suite = suite();
    let ops = [
        "apply",
        "group_begin",
        "group_child",
        "group_commit",
        "group_cancel",
        "undo",
        "redo",
        "snapshot",
        "read_notes",
        "read_curve",
        "read_tracks",
    ];
    for vector in &suite.vectors {
        assert!(!vector.name.is_empty());
        fixture(&vector.project);
        for step in &vector.steps {
            assert!(ops.contains(&step.op.as_str()), "unknown op '{}'", step.op);
        }
        if vector.skip.is_some() {
            assert!(vector.steps.is_empty(), "skipped vectors carry no steps");
        }
    }
}
