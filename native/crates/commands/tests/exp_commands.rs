//! Round-trip tests for the expression commands (curves, note expressions,
//! expression descriptors).

mod common;

use commands::{
    Command, SetCurveCommand, SetCurvePointsCommand, SetExpDescriptorCommand, SetExpressionCommand,
};
use domain::{UCurve, UExpression, UExpressionDescriptor, UExpressionType, UPart, UProject};

/// Project with a `dyn` curve on the part.
fn project_with_curve() -> UProject {
    let mut project = common::project_with_notes();
    if let UPart::Voice(vp) = &mut project.parts[0] {
        vp.curves.push(UCurve {
            abbr: "dyn".into(),
            xs: vec![0, 480],
            ys: vec![0, 100],
        });
    }
    project
}

fn curve<'a>(project: &'a UProject, abbr: &str) -> &'a UCurve {
    match &project.parts[0] {
        UPart::Voice(vp) => vp.curves.iter().find(|c| c.abbr == abbr).unwrap(),
        UPart::Wave(_) => panic!("expected voice part"),
    }
}

#[test]
fn set_curve_roundtrip() {
    let mut project = project_with_curve();
    let before = project.clone();
    let mut cmd = SetCurveCommand::new(&project, 0, "dyn", 240, 50, 0, 0).unwrap();
    assert_eq!(cmd.name(), "Set curve");

    cmd.execute(&mut project).unwrap();
    // UCurve::set keeps the curve continuous: it inserts the edited point
    // and a sampled neighbor at x + INTERVAL (OpenUtau behavior).
    let c = curve(&project, "dyn");
    assert_eq!(c.xs, vec![0, 240, 245, 480]);
    assert_eq!(c.ys, vec![0, 50, 51, 100]);

    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn set_curve_creates_and_removes_curve() {
    let mut project = common::project_with_notes();
    let before = project.clone();
    let mut cmd = SetCurveCommand::new(&project, 0, "pitd", 120, -50, 0, 0).unwrap();

    cmd.execute(&mut project).unwrap();
    assert!(curve(&project, "pitd").xs.contains(&120));

    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before); // curve gone again
}

#[test]
fn set_curve_clamps_to_descriptor_bounds() {
    let mut project = project_with_curve();
    // dyn descriptor is [-240, 120]: 500 clamps to 120, -500 clamps to -240.
    let mut cmd = SetCurveCommand::new(&project, 0, "dyn", 240, 500, 0, -500).unwrap();
    cmd.execute(&mut project).unwrap();
    let c = curve(&project, "dyn");
    assert_eq!(c.ys[1], 120); // clamped
    assert!(c.ys.iter().all(|&y| (-240..=120).contains(&y)));
}

#[test]
fn set_curve_unregistered_abbr_is_noop() {
    let mut project = common::project_with_notes();
    let before = project.clone();
    let mut cmd = SetCurveCommand::new(&project, 0, "zzz", 240, 50, 0, 0).unwrap();
    cmd.execute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn set_curve_points_roundtrip() {
    let mut project = project_with_curve();
    let before = project.clone();
    let mut cmd =
        SetCurvePointsCommand::new(&project, 0, "dyn", vec![0, 240, 960], vec![10, 20, 30])
            .unwrap();
    assert_eq!(cmd.name(), "Set curve points");

    cmd.execute(&mut project).unwrap();
    assert_eq!(curve(&project, "dyn").xs, vec![0, 240, 960]);
    assert_eq!(curve(&project, "dyn").ys, vec![10, 20, 30]);

    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn set_curve_points_mismatched_lengths_rejected() {
    let project = project_with_curve();
    assert!(SetCurvePointsCommand::new(&project, 0, "dyn", vec![0, 1], vec![0]).is_err());
}

#[test]
fn set_expression_roundtrip() {
    let mut project = common::project_with_notes();
    if let UPart::Voice(vp) = &mut project.parts[0] {
        vp.notes[0].phoneme_expressions.push(UExpression {
            index: Some(0),
            abbr: "vel".into(),
            value: 100.0,
        });
    }
    let before = project.clone();
    let mut cmd = SetExpressionCommand::new(&project, 0, 0, "vel", Some(120.0)).unwrap();
    assert_eq!(cmd.name(), "Set expression");

    cmd.execute(&mut project).unwrap();
    let exps = match &project.parts[0] {
        UPart::Voice(vp) => &vp.notes[0].phoneme_expressions,
        UPart::Wave(_) => panic!(),
    };
    assert_eq!(exps.len(), 1);
    assert_eq!(exps[0].abbr, "vel");
    assert_eq!(exps[0].value, 120.0);

    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn set_expression_clamps_value() {
    let mut project = common::project_with_notes();
    // vel descriptor is [0, 200].
    let mut cmd = SetExpressionCommand::new(&project, 0, 0, "vel", Some(999.0)).unwrap();
    cmd.execute(&mut project).unwrap();
    let exps = match &project.parts[0] {
        UPart::Voice(vp) => &vp.notes[0].phoneme_expressions,
        UPart::Wave(_) => panic!(),
    };
    assert_eq!(exps[0].value, 200.0);
}

#[test]
fn set_expression_none_clears_and_roundtrips() {
    let mut project = common::project_with_notes();
    if let UPart::Voice(vp) = &mut project.parts[0] {
        vp.notes[0].phoneme_expressions.push(UExpression {
            index: Some(0),
            abbr: "vel".into(),
            value: 80.0,
        });
    }
    let before = project.clone();
    let mut cmd = SetExpressionCommand::new(&project, 0, 0, "vel", None).unwrap();

    cmd.execute(&mut project).unwrap();
    let exps = match &project.parts[0] {
        UPart::Voice(vp) => &vp.notes[0].phoneme_expressions,
        UPart::Wave(_) => panic!(),
    };
    assert!(exps.is_empty());

    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn set_expression_unregistered_abbr_is_noop() {
    let mut project = common::project_with_notes();
    let before = project.clone();
    let mut cmd = SetExpressionCommand::new(&project, 0, 0, "zzz", Some(50.0)).unwrap();
    cmd.execute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn set_exp_descriptor_add_roundtrip() {
    let mut project = common::voice_project();
    let before = project.clone();
    let descriptor = UExpressionDescriptor::numerical("tension", "ten", -100.0, 100.0, 0.0, None);
    let mut cmd = SetExpDescriptorCommand::new(&project, descriptor);
    assert_eq!(cmd.name(), "Set expression descriptor");

    cmd.execute(&mut project).unwrap();
    assert!(project.expressions.contains_key("ten"));

    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn set_exp_descriptor_replace_roundtrip() {
    let mut project = common::voice_project();
    let before = project.clone();
    let descriptor = UExpressionDescriptor::numerical("dynamics", "dyn", -100.0, 100.0, 5.0, None);
    let mut cmd = SetExpDescriptorCommand::new(&project, descriptor);

    cmd.execute(&mut project).unwrap();
    let replaced = project.expressions.get("dyn").unwrap();
    assert_eq!(replaced.name, "dynamics");
    assert_eq!(replaced.max, 100.0);

    cmd.unexecute(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn set_expression_uses_track_descriptor_override() {
    let mut project = common::project_with_notes();
    // Track 0 overrides `vel` with a [0, 50] range.
    project.tracks[0]
        .track_expressions
        .push(UExpressionDescriptor {
            name: "velocity".into(),
            abbr: "vel".into(),
            r#type: UExpressionType::Numerical,
            min: 0.0,
            max: 50.0,
            ..Default::default()
        });
    let mut cmd = SetExpressionCommand::new(&project, 0, 0, "vel", Some(80.0)).unwrap();
    cmd.execute(&mut project).unwrap();
    let exps = match &project.parts[0] {
        UPart::Voice(vp) => &vp.notes[0].phoneme_expressions,
        UPart::Wave(_) => panic!(),
    };
    assert_eq!(exps[0].value, 50.0); // clamped by the track descriptor
}
