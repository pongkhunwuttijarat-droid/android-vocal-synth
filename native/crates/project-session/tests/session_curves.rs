//! `curve.apply_stroke` through the session (CONTRACT-v1 §6.6) plus the
//! capability/limit surface (§6.15).
//!
//! The command-level tests live in `commands::curve_commands`; these cover the
//! v2 contract: stable IDs, explicit rejection reasons, canonical patches,
//! revision rules and the declared payload limits.

use project_session::v2::{ERR_NOT_FOUND, ERR_PAYLOAD_TOO_LARGE, MAX_STROKE_POINTS};
use project_session::Session;
use serde_json::{json, Value};

const DEMO: &str = include_str!("../../../tools/synth-cli/tests/data/demo-song.ustx");

/// One track, one part, one note, one `dyn` curve with points at 0 and 480.
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

fn request(revision: u64, command: Value) -> String {
    json!({"expected_revision": revision, "command": command}).to_string()
}

fn part_id(session: &Session) -> String {
    session.identity().tracks[0].parts[0]
        .id
        .as_str()
        .to_string()
}

fn curve_id(session: &Session, index: usize) -> String {
    session.identity().tracks[0].parts[0]
        .curves
        .get(index)
        .expect("curve id")
        .id
        .as_str()
        .to_string()
}

fn curve_points(session: &Session, index: usize) -> (Vec<i64>, Vec<i64>) {
    let value: Value =
        serde_json::from_str(&session.snapshot_json().expect("snapshot")).expect("json");
    let points = value["project"]["tracks"][0]["parts"][0]["curves"][index]["points"]
        .as_array()
        .expect("points");
    (
        points.iter().map(|p| p["tick"].as_i64().unwrap()).collect(),
        points
            .iter()
            .map(|p| p["value"].as_i64().unwrap())
            .collect(),
    )
}

// ---------------------------------------------------------------------------
// Strokes
// ---------------------------------------------------------------------------

#[test]
fn a_stroke_replaces_the_interval_and_reports_canonical_points() {
    let mut session = Session::open(CURVE_PROJECT, None, 1).expect("open");
    let id = curve_id(&session, 0);

    let result = session
        .apply_v2(&request(
            0,
            json!({"type":"curve.apply_stroke","part_id":part_id(&session),"curve_id":id,
                   "mode":"replace","start_tick":480,"end_tick":960,
                   "points":[{"tick":612,"value":20},{"tick":700,"value":5}]}),
        ))
        .expect("stroke");

    assert_eq!(result.revision, 1);
    assert!(result.changes_are_well_formed());
    let change = result
        .changes
        .iter()
        .find(|c| c.kind == "curve.changed")
        .expect("curve.changed");
    assert_eq!(change.curve_id.as_deref(), Some(id.as_str()));
    assert_eq!(change.abbr.as_deref(), Some("dyn"));
    assert_eq!(change.authority, "patch");

    // Canonical points, snapped to the 5-tick grid, with the right-hand edge
    // anchored at the descriptor default.
    let (xs, ys) = curve_points(&session, 0);
    assert_eq!(xs, vec![0, 480, 610, 700, 960]);
    assert_eq!(ys, vec![0, 10, 20, 5, 0]);
    // The patch payload carries exactly what the snapshot says.
    let data = change.data.as_ref().expect("patch data");
    assert_eq!(data["points"][2]["tick"], 610);
    assert_eq!(data["points"][2]["value"], 20);
}

#[test]
fn a_stroke_can_create_a_curve_and_the_new_id_is_addressable() {
    let mut session = Session::open(DEMO, None, 1).expect("open");
    let part = part_id(&session);

    let result = session
        .apply_v2(&request(
            0,
            json!({"type":"curve.apply_stroke","part_id":part,"create":true,"abbr":"dyn",
                   "mode":"replace","start_tick":0,"end_tick":480,
                   "points":[{"tick":120,"value":15}]}),
        ))
        .expect("create curve");

    let change = result
        .changes
        .iter()
        .find(|c| c.kind == "curve.changed")
        .expect("curve.changed");
    let new_id = change
        .curve_id
        .clone()
        .expect("id reported for the new curve");
    assert_eq!(
        result
            .changes
            .iter()
            .filter(|c| c.kind == "curve.changed")
            .count(),
        1
    );

    let (xs, ys) = curve_points(&session, 0);
    assert_eq!(xs, vec![0, 120, 480]);
    assert_eq!(ys, vec![0, 15, 0]);

    // The reported id addresses the curve afterwards.
    session
        .apply_v2(&request(
            1,
            json!({"type":"curve.apply_stroke","part_id":part,"curve_id":new_id,
                   "mode":"replace","start_tick":480,"end_tick":960,
                   "points":[{"tick":600,"value":9}]}),
        ))
        .expect("the reported id must be addressable");
}

#[test]
fn descriptor_and_first_stroke_context_are_revision_tagged_reads() {
    let mut session = Session::open(CURVE_PROJECT, None, 1).expect("open");
    let part = part_id(&session);
    let descriptor: Value =
        serde_json::from_str(&session.expression_descriptor_json("dyn").unwrap()).unwrap();
    assert_eq!(descriptor["revision"], 0);
    assert_eq!(descriptor["descriptor"]["abbr"], "dyn");
    assert_eq!(descriptor["descriptor"]["min"], -240.0);

    let curve = curve_id(&session, 0);
    session
        .apply_v2(&request(
            0,
            json!({"type":"curve.apply_stroke","part_id":part,"curve_id":curve,
                   "mode":"clear","start_tick":0,"end_tick":480,"points":[]}),
        ))
        .unwrap();
    let context: Value = serde_json::from_str(
        &session
            .curve_stroke_context_for_abbr_json(&part, "dyn", None)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(context["revision"], 1);
    assert_eq!(
        context["track_id"],
        session.identity().tracks[0].id.as_str()
    );
    assert_eq!(context["part_id"], part);
    assert!(context["curve_id"].is_null());
    assert_eq!(context["points"], json!([]));
    assert_eq!(context["descriptor"]["min_value"], -240);
    assert_eq!(context["descriptor"]["max_value"], 120);
}

#[test]
fn clear_removes_the_interval_and_deletes_an_emptied_curve() {
    let mut session = Session::open(CURVE_PROJECT, None, 1).expect("open");
    let id = curve_id(&session, 0);

    // Clearing the whole part removes the curve itself.
    let result = session
        .apply_v2(&request(
            0,
            json!({"type":"curve.apply_stroke","part_id":part_id(&session),"curve_id":id,
                   "mode":"clear","start_tick":0,"end_tick":1920,"points":[]}),
        ))
        .expect("clear");

    let change = result
        .changes
        .iter()
        .find(|c| c.kind == "curve.changed")
        .expect("curve.changed");
    assert_eq!(
        change.authority, "invalidate",
        "a removed curve has no payload"
    );
    assert!(change.data.is_none());
    assert!(
        session.identity().tracks[0].parts[0].curves.is_empty(),
        "the emptied curve is gone from the identity table"
    );
}

#[test]
fn a_partial_clear_keeps_the_remaining_points() {
    let mut session = Session::open(CURVE_PROJECT, None, 1).expect("open");
    let id = curve_id(&session, 0);

    session
        .apply_v2(&request(
            0,
            json!({"type":"curve.apply_stroke","part_id":part_id(&session),"curve_id":id,
                   "mode":"clear","start_tick":480,"end_tick":1920,"points":[]}),
        ))
        .expect("clear tail");

    let (xs, ys) = curve_points(&session, 0);
    assert_eq!(xs, vec![0]);
    assert_eq!(ys, vec![0]);
}

#[test]
fn one_stroke_is_one_revision_and_one_undo_step() {
    let mut session = Session::open(CURVE_PROJECT, None, 1).expect("open");
    let id = curve_id(&session, 0);
    let before = curve_points(&session, 0);

    session
        .apply_v2(&request(
            0,
            json!({"type":"curve.apply_stroke","part_id":part_id(&session),"curve_id":id,
                   "mode":"replace","start_tick":0,"end_tick":960,
                   "points":[{"tick":240,"value":40},{"tick":480,"value":50}]}),
        ))
        .expect("stroke");
    assert_eq!(session.revision(), 1);

    let undone = session.undo().expect("undo").expect("step");
    assert_eq!(undone.revision, 2);
    assert_eq!(
        curve_points(&session, 0),
        before,
        "undo restores the points"
    );
}

#[test]
fn committed_stroke_simplifies_the_whole_curve_and_undo_restores_exact_points() {
    let mut session = Session::open(CURVE_PROJECT, None, 1).expect("open");
    let id = curve_id(&session, 0);
    let before = curve_points(&session, 0);
    session
        .apply_v2(&request(
            0,
            json!({"type":"curve.apply_stroke","part_id":part_id(&session),"curve_id":id,
                   "mode":"replace","start_tick":0,"end_tick":960,
                   "points":[{"tick":240,"value":5},{"tick":480,"value":10},
                             {"tick":720,"value":5}]}),
        ))
        .expect("commit stroke");
    assert_eq!(session.revision(), 1);
    assert_eq!(
        curve_points(&session, 0),
        (vec![0, 480, 960], vec![0, 10, 0])
    );
    session.undo().expect("undo").expect("one stroke step");
    assert_eq!(
        curve_points(&session, 0),
        before,
        "undo is byte/structure exact"
    );
}

#[test]
fn dense_stroke_is_one_revision_and_one_undo_entry() {
    let mut session = Session::open(CURVE_PROJECT, None, 1).expect("open");
    let id = curve_id(&session, 0);
    let before = curve_points(&session, 0);
    let points: Vec<_> = (0..500)
        .map(|i| json!({"tick": i * 960 / 499, "value": (i % 3) as i32}))
        .collect();
    let result = session
        .apply_v2(&request(
            0,
            json!({"type":"curve.apply_stroke","part_id":part_id(&session),"curve_id":id,
                   "mode":"replace","start_tick":0,"end_tick":960,"points":points}),
        ))
        .expect("dense gesture commits once");
    assert_eq!(result.revision, 1);
    assert_eq!(session.revision(), 1);
    session.undo().expect("undo").expect("one undo entry");
    assert_eq!(session.revision(), 2);
    assert_eq!(curve_points(&session, 0), before);
}

#[test]
fn a_stroke_inside_a_group_is_published_only_on_commit() {
    let mut session = Session::open(CURVE_PROJECT, None, 1).expect("open");
    let id = curve_id(&session, 0);
    let before = curve_points(&session, 0);
    let begin = session.group_begin(0, "Draw curve").expect("begin");

    let child = session
        .apply_v2(
            json!({"expected_revision":0,"group_id":begin.group_id,
                   "command":{"type":"curve.apply_stroke","part_id":part_id(&session),"curve_id":id,
                              "mode":"replace","start_tick":0,"end_tick":960,
                              "points":[{"tick":240,"value":40}]}})
            .to_string()
            .as_str(),
        )
        .expect("child");
    assert!(child.changes.is_empty(), "tentative state is not public");
    assert_eq!(
        curve_points(&session, 0),
        before,
        "committed view is unchanged"
    );

    let commit = session.group_commit(&begin.group_id, 0).expect("commit");
    assert_eq!(commit.revision, 1);
    let change = commit
        .changes
        .iter()
        .find(|c| c.kind == "curve.changed")
        .expect("merged curve change");
    assert_eq!(change.curve_id.as_deref(), Some(id.as_str()));
    assert_ne!(curve_points(&session, 0), before);
}

// ---------------------------------------------------------------------------
// Rejections
// ---------------------------------------------------------------------------

fn expect_reason(err: project_session::SessionError, reason: &str) {
    assert_eq!(err.code(), "validation_error", "wrong code: {err:?}");
    assert_eq!(err.details["reason"], reason, "wrong reason: {err:?}");
}

#[test]
fn stroke_rejections_carry_a_stable_reason() {
    let mut session = Session::open(CURVE_PROJECT, None, 1).expect("open");
    let part = part_id(&session);
    let id = curve_id(&session, 0);

    // Unsupported mode.
    let err = session
        .apply_v2(&request(
            0,
            json!({"type":"curve.apply_stroke","part_id":part,"curve_id":id,
                   "mode":"blend","start_tick":0,"end_tick":480,"points":[]}),
        ))
        .expect_err("mode");
    assert_eq!(err.details["field"], "mode");

    // Interval outside the part.
    let err = session
        .apply_v2(&request(
            0,
            json!({"type":"curve.apply_stroke","part_id":part,"curve_id":id,
                   "mode":"replace","start_tick":0,"end_tick":99999,
                   "points":[{"tick":10,"value":1}]}),
        ))
        .expect_err("interval");
    expect_reason(err, "tick_out_of_range");

    // start >= end.
    let err = session
        .apply_v2(&request(
            0,
            json!({"type":"curve.apply_stroke","part_id":part,"curve_id":id,
                   "mode":"replace","start_tick":480,"end_tick":480,
                   "points":[{"tick":480,"value":1}]}),
        ))
        .expect_err("degenerate interval");
    expect_reason(err, "tick_out_of_range");

    // A point outside the declared interval.
    let err = session
        .apply_v2(&request(
            0,
            json!({"type":"curve.apply_stroke","part_id":part,"curve_id":id,
                   "mode":"replace","start_tick":480,"end_tick":960,
                   "points":[{"tick":1000,"value":1}]}),
        ))
        .expect_err("point outside");
    expect_reason(err, "stroke_point_out_of_range");

    // empty replace / non-empty clear.
    let err = session
        .apply_v2(&request(
            0,
            json!({"type":"curve.apply_stroke","part_id":part,"curve_id":id,
                   "mode":"replace","start_tick":0,"end_tick":480,"points":[]}),
        ))
        .expect_err("empty replace");
    expect_reason(err, "empty_stroke");
    let err = session
        .apply_v2(&request(
            0,
            json!({"type":"curve.apply_stroke","part_id":part,"curve_id":id,
                   "mode":"clear","start_tick":0,"end_tick":480,
                   "points":[{"tick":10,"value":1}]}),
        ))
        .expect_err("clear with points");
    expect_reason(err, "unexpected_points");

    // create + curve_id, and create without an abbr.
    let err = session
        .apply_v2(&request(
            0,
            json!({"type":"curve.apply_stroke","part_id":part,"curve_id":id,"create":true,
                   "mode":"replace","start_tick":0,"end_tick":480,
                   "points":[{"tick":10,"value":1}]}),
        ))
        .expect_err("create + id");
    assert_eq!(err.details["field"], "create");
    let err = session
        .apply_v2(&request(
            0,
            json!({"type":"curve.apply_stroke","part_id":part,"create":true,
                   "mode":"replace","start_tick":0,"end_tick":480,
                   "points":[{"tick":10,"value":1}]}),
        ))
        .expect_err("create without abbr");
    assert_eq!(err.details["field"], "abbr");

    // Duplicate descriptor when creating.
    let err = session
        .apply_v2(&request(
            0,
            json!({"type":"curve.apply_stroke","part_id":part,"create":true,"abbr":"dyn",
                   "mode":"replace","start_tick":0,"end_tick":480,
                   "points":[{"tick":10,"value":1}]}),
        ))
        .expect_err("duplicate descriptor");
    expect_reason(err, "duplicate_curve_descriptor");

    // An expression that is not a Curve descriptor.
    let err = session
        .apply_v2(&request(
            0,
            json!({"type":"curve.apply_stroke","part_id":part,"create":true,"abbr":"vel",
                   "mode":"replace","start_tick":0,"end_tick":480,
                   "points":[{"tick":10,"value":1}]}),
        ))
        .expect_err("not a curve descriptor");
    expect_reason(err, "unsupported_curve_descriptor");

    // abbr mismatch against the addressed curve.
    let err = session
        .apply_v2(&request(
            0,
            json!({"type":"curve.apply_stroke","part_id":part,"curve_id":id,"abbr":"pitd",
                   "mode":"replace","start_tick":0,"end_tick":480,
                   "points":[{"tick":10,"value":1}]}),
        ))
        .expect_err("abbr mismatch");
    expect_reason(err, "abbr_mismatch");

    // Unknown curve id / missing curve_id without create.
    let err = session
        .apply_v2(&request(
            0,
            json!({"type":"curve.apply_stroke","part_id":part,"curve_id":"ghost",
                   "mode":"replace","start_tick":0,"end_tick":480,
                   "points":[{"tick":10,"value":1}]}),
        ))
        .expect_err("unknown curve");
    assert_eq!(err.code(), ERR_NOT_FOUND);
    let err = session
        .apply_v2(&request(
            0,
            json!({"type":"curve.apply_stroke","part_id":part,
                   "mode":"replace","start_tick":0,"end_tick":480,
                   "points":[{"tick":10,"value":1}]}),
        ))
        .expect_err("missing curve_id");
    assert_eq!(err.details["field"], "curve_id");

    // Nothing above may have changed the document.
    assert_eq!(session.revision(), 0);
    assert!(!session.can_undo());
    assert_eq!(curve_points(&session, 0), (vec![0, 480], vec![0, 10]));
}

// ---------------------------------------------------------------------------
// Capabilities and limits
// ---------------------------------------------------------------------------

#[test]
fn capabilities_describe_what_this_build_supports() {
    let session = Session::open(DEMO, None, 1).expect("open");
    let value: Value = serde_json::from_str(&session.capabilities_json().unwrap()).unwrap();

    assert_eq!(value["contract_version"], "2-draft3");
    assert_eq!(value["id_schema_version"], 1);
    for feature in [
        "stable_ids",
        "partial_reads",
        "group_transactions",
        "curve_apply_stroke",
        "project.controls.v1",
    ] {
        assert!(
            value["features"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f == feature),
            "missing feature {feature}"
        );
    }
    assert_eq!(value["limits"]["max_command_bytes"], 1_048_576);
    assert_eq!(value["limits"]["max_stroke_points"], MAX_STROKE_POINTS);
    assert_eq!(value["render"]["async_jobs"], false, "must not over-claim");
}

#[test]
fn too_many_stroke_points_is_a_payload_limit_error() {
    let mut session = Session::open(DEMO, None, 1).expect("open");
    let part = part_id(&session);
    let points: Vec<Value> = (0..MAX_STROKE_POINTS + 1)
        .map(|i| json!({"tick": (i as i32) * 5 % 480, "value": 1}))
        .collect();

    let err = session
        .apply_v2(&request(
            0,
            json!({"type":"curve.apply_stroke","part_id":part,"create":true,"abbr":"dyn",
                   "mode":"replace","start_tick":0,"end_tick":480,"points":points}),
        ))
        .expect_err("over the declared limit");

    assert_eq!(err.code(), ERR_PAYLOAD_TOO_LARGE);
    assert_eq!(err.details["limit"], "max_stroke_points");
    assert_eq!(session.revision(), 0);
}

#[test]
fn an_oversized_request_is_rejected_before_parsing() {
    let mut session = Session::open(DEMO, None, 1).expect("open");
    // 1 MiB + 1 byte of well-formed JSON.
    let filler = " ".repeat(1_048_576);
    let oversized = format!(
        "{filler}{}",
        request(0, json!({"type":"note.set_tone","note_id":"x","tone":1}))
    );

    let err = session.apply_v2(&oversized).expect_err("too large");

    assert_eq!(err.code(), ERR_PAYLOAD_TOO_LARGE);
    assert_eq!(err.details["limit"], "max_command_bytes");
    assert_eq!(session.revision(), 0);
}
