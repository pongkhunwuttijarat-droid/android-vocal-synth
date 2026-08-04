//! Expression registry tests: default expressions, descriptor construction,
//! and per-phoneme resampler flags.

use domain::{
    UExpression, UExpressionDescriptor, UExpressionType, UPhoneme, UProject, UTrack, ALT, ATK,
    BRE, BREC, CLR, DEC, DIR, DYN, ENG, EXP_SELECTORS_DEFAULT, GEN, GENC, LPF, MOD, MODP, NORM,
    PITD, REQUIRED, SHFC, SHFT, TENC, VEL, VOIC, VOL,
};

#[test]
fn all_default_expressions_registered() {
    let p = UProject::create();
    let e = &p.expressions;
    assert_eq!(e.len(), 22);
    for abbr in REQUIRED {
        assert!(e.contains_key(abbr), "missing required expression {abbr}");
    }

    // Curve expressions.
    let dyn_ = &e[DYN];
    assert_eq!(dyn_.r#type, UExpressionType::Curve);
    assert_eq!((dyn_.min, dyn_.max, dyn_.default_value), (-240.0, 120.0, 0.0));
    let pitd = &e[PITD];
    assert_eq!(pitd.r#type, UExpressionType::Curve);
    assert_eq!((pitd.min, pitd.max), (-1200.0, 1200.0));
    for abbr in [GENC, BREC, SHFC, TENC] {
        assert_eq!(e[abbr].r#type, UExpressionType::Curve);
    }
    let voic = &e[VOIC];
    assert_eq!(voic.r#type, UExpressionType::Curve);
    assert_eq!(voic.default_value, 100.0);

    // Options expressions.
    let clr = &e[CLR];
    assert_eq!(clr.r#type, UExpressionType::Options);
    assert_eq!(clr.options.as_deref(), Some(&[][..]));
    assert_eq!(clr.max, -1.0); // empty options -> max = len - 1 = -1 (OpenUtau)
    let eng = &e[ENG];
    assert_eq!(eng.r#type, UExpressionType::Options);
    assert_eq!(eng.options.as_deref(), Some(&["".to_string(), "worldline".to_string()][..]));
    assert_eq!(eng.max, 1.0);
    let dir = &e[DIR];
    assert_eq!(dir.r#type, UExpressionType::Options);
    assert_eq!(dir.options.as_deref(), Some(&["off".to_string(), "on".to_string()][..]));

    // Numerical expressions with ranges and flags.
    let vel = &e[VEL];
    assert_eq!(vel.r#type, UExpressionType::Numerical);
    assert_eq!((vel.min, vel.max, vel.default_value), (0.0, 200.0, 100.0));
    assert_eq!((e[VOL].min, e[VOL].max), (0.0, 200.0));
    assert_eq!((e[ATK].min, e[ATK].max), (0.0, 200.0));
    assert_eq!((e[DEC].min, e[DEC].max), (0.0, 100.0));
    assert_eq!((e[BRE].min, e[BRE].max), (0.0, 100.0));
    assert_eq!((e[LPF].min, e[LPF].max), (0.0, 100.0));
    assert_eq!((e[MOD].min, e[MOD].max), (0.0, 100.0));
    assert_eq!((e[MODP].min, e[MODP].max), (0.0, 100.0));
    assert_eq!((e[ALT].min, e[ALT].max), (0.0, 16.0));
    assert_eq!((e[SHFT].min, e[SHFT].max), (-36.0, 36.0));
    assert_eq!((e[GEN].min, e[GEN].max), (-100.0, 100.0));

    assert_eq!(e[GEN].flag.as_deref(), Some("g"));
    assert!(e[GEN].is_flag);
    assert_eq!(e[BRE].flag.as_deref(), Some("B"));
    assert_eq!(e[LPF].flag.as_deref(), Some("H"));
    let norm = &e[NORM];
    assert_eq!(norm.flag.as_deref(), Some("P"));
    assert_eq!(norm.default_value, 86.0);
    assert_eq!(e[MODP].abbr, "mod+");
    assert_eq!(e[SHFT].flag, None);
    assert!(!e[SHFT].is_flag);

    // Default selectors and primary/secondary.
    assert_eq!(p.exp_selectors, EXP_SELECTORS_DEFAULT.map(str::to_string).to_vec());
    assert_eq!(p.exp_primary, 0);
    assert_eq!(p.exp_secondary, 1);
}

#[test]
fn register_expression_preserves_existing() {
    // Register a custom "dyn" first; add_default_expressions must not
    // clobber it (OpenUtau's RegisterExpression keeps existing entries).
    let mut p = UProject::new();
    let custom = UExpressionDescriptor::curve("my dynamics", "dyn", -1000.0, 1000.0, 5.0);
    p.register_expression(custom.clone());
    domain::add_default_expressions(&mut p);
    assert_eq!(p.expressions["dyn"].name, "my dynamics");
    assert_eq!(p.expressions["dyn"].max, 1000.0);
    assert_eq!(p.expressions.len(), 22);
    // And registering a second time keeps the first.
    p.register_expression(UExpressionDescriptor::curve("other", "dyn", 0.0, 1.0, 0.0));
    assert_eq!(p.expressions["dyn"].name, "my dynamics");
}

#[test]
fn descriptor_constructors_clamp_and_lowercase() {
    let d = UExpressionDescriptor::numerical("Test", "TEST", 0.0, 100.0, 150.0, None);
    assert_eq!(d.abbr, "test");
    assert_eq!(d.default_value, 100.0); // clamped into [min, max]

    let d = UExpressionDescriptor::numerical("gen", "gen", -100.0, 100.0, 0.0, Some("g"));
    assert_eq!(d.r#type, UExpressionType::Numerical);
    assert!(d.is_flag);
    assert_eq!(d.flag.as_deref(), Some("g"));

    let d = UExpressionDescriptor::curve("c", "c", -10.0, 10.0, 0.0);
    assert_eq!(d.r#type, UExpressionType::Curve);
    assert!(!d.is_flag);

    let d = UExpressionDescriptor::options("o", "o", false, vec!["x".into(), "y".into()]);
    assert_eq!(d.r#type, UExpressionType::Options);
    assert_eq!((d.min, d.max, d.default_value), (0.0, 1.0, 0.0));

    // Custom default value semantics (C# CustomDefaultValue property).
    let mut d = UExpressionDescriptor::curve("c", "c", -10.0, 10.0, 0.0);
    assert_eq!(d.custom_default_value(), 0.0);
    d.set_custom_default_value(3.0);
    assert_eq!(d.custom_default_value(), 3.0);
    assert_eq!(d.custom_default_value, Some(3.0));
    d.set_custom_default_value(0.0); // back to default -> None
    assert_eq!(d.custom_default_value, None);
}

#[test]
fn expression_value_clamping() {
    let mut e = UExpression { index: Some(0), abbr: VEL.into(), value: 999.0 };
    e.clamp_value(0.0, 200.0);
    assert_eq!(e.value, 200.0);
    let mut c = UExpression { index: Some(0), abbr: CLR.into(), value: 999.0 };
    c.clamp_value(0.0, 2.0);
    assert_eq!(c.value, 999.0); // clr is never clamped
}

#[test]
fn descriptor_create_expression() {
    let d = UExpressionDescriptor::numerical("velocity", VEL, 0.0, 200.0, 100.0, None);
    let e = d.create();
    assert_eq!(e.abbr, "vel");
    assert_eq!(e.value, 100.0);
    assert_eq!(e.index, None);
}

#[test]
fn phoneme_resampler_flags() {
    let p = UProject::create();
    let track = &p.tracks[0];
    let mut note = p.create_note_at(60, 0, 480);
    note.phoneme_expressions.push(UExpression { index: Some(0), abbr: GEN.into(), value: 5.0 });
    note.phoneme_expressions.push(UExpression { index: Some(0), abbr: DIR.into(), value: 1.0 });
    let ph = UPhoneme { index: 0, parent: Some(0), ..Default::default() };

    let flags = ph.get_resampler_flags(&note, &p, track);
    assert!(flags.contains(&("g".to_string(), Some(5), "gen".to_string())));
    assert!(flags.contains(&("B".to_string(), Some(0), "bre".to_string())));
    assert!(flags.contains(&("P".to_string(), Some(86), "norm".to_string())));
    // dir is Options with is_flag=false: resolved as an expression value
    // but never emitted as a resampler flag (C# `if (descriptor.isFlag)`).
    assert_eq!(ph.get_expression(&note, &p, track, DIR), Some((1.0, true)));
    assert!(!flags.iter().any(|(f, _, _)| f == "on"));

    let strings = ph.flags_as_strings(&note, &p, track);
    assert!(strings.contains(&"g5".to_string()));
    assert!(strings.contains(&"B0".to_string()));

    // Default expression values still produce flags (value 0 included).
    let note2 = p.create_note_at(60, 0, 480);
    let flags2 = ph.get_resampler_flags(&note2, &p, track);
    assert!(flags2.contains(&("g".to_string(), Some(0), "gen".to_string())));
}

#[test]
fn options_flag_emitted_for_custom_descriptor() {
    let mut p = UProject::create();
    p.register_expression(UExpressionDescriptor::options(
        "my option",
        "mop",
        true,
        vec!["a".into(), "b".into()],
    ));
    let track = &p.tracks[0];
    let ph = UPhoneme { index: 0, parent: Some(0), ..Default::default() };

    // Default value 0 -> option "a".
    let note = p.create_note_at(60, 0, 480);
    let flags = ph.get_resampler_flags(&note, &p, track);
    assert!(flags.contains(&("a".to_string(), None, "mop".to_string())));
    // Value 1 -> option "b".
    let mut note2 = p.create_note_at(60, 0, 480);
    note2.phoneme_expressions.push(UExpression { index: Some(0), abbr: "mop".into(), value: 1.0 });
    let flags2 = ph.get_resampler_flags(&note2, &p, track);
    assert!(flags2.contains(&("b".to_string(), None, "mop".to_string())));
    assert!(ph.flags_as_strings(&note2, &p, track).iter().any(|s| s == "b"));
}

#[test]
fn skip_output_if_default_suppresses_flag() {
    let mut p = UProject::create();
    let mut d = UExpressionDescriptor::numerical("test flag", "tst", 0.0, 10.0, 5.0, Some("T"));
    d.skip_output_if_default = true;
    p.register_expression(d);
    let track = &p.tracks[0];
    let note = p.create_note_at(60, 0, 480);
    let ph = UPhoneme { index: 0, parent: Some(0), ..Default::default() };

    // At the default value the flag is suppressed...
    let flags = ph.get_resampler_flags(&note, &p, track);
    assert!(!flags.iter().any(|(f, _, _)| f == "T"), "flag should be suppressed at default");
    // ...but emitted otherwise.
    let mut note2 = p.create_note_at(60, 0, 480);
    note2.phoneme_expressions.push(UExpression { index: Some(0), abbr: "tst".into(), value: 7.0 });
    let flags2 = ph.get_resampler_flags(&note2, &p, track);
    assert!(flags2.contains(&("T".to_string(), Some(7), "tst".to_string())));
}

#[test]
fn track_expression_overrides_project() {
    let mut p = UProject::create();
    // A track-level descriptor with the same abbr shadows the project one
    // in TryGetExpDescriptor and in flag generation.
    let d = UExpressionDescriptor::numerical("track gender", "gen", -100.0, 100.0, 42.0, Some("g"));
    p.tracks[0].track_expressions.push(d.clone());
    let found = p.tracks[0].try_get_exp_descriptor(&p, "gen").unwrap();
    assert_eq!(found.name, "track gender");
    assert_eq!(found.default_value, 42.0);
    // Custom track-only expression resolves too.
    p.tracks[0]
        .track_expressions
        .push(UExpressionDescriptor::numerical("myexp", "myexp", 0.0, 10.0, 1.0, None));
    assert!(p.tracks[0].try_get_exp_descriptor(&p, "myexp").is_some());
    assert_eq!(p.tracks[0].try_get_exp_descriptor(&p, "unknown"), None);

    // Project-level "gen" descriptor is shadowed: no duplicate flag pair.
    let track = &p.tracks[0];
    let note = p.create_note_at(60, 0, 480);
    let ph = UPhoneme { index: 0, parent: Some(0), ..Default::default() };
    let flags = ph.get_resampler_flags(&note, &p, track);
    let gen_flags: Vec<_> = flags.iter().filter(|(f, _, _)| f == "g").collect();
    assert_eq!(gen_flags.len(), 1);
}

#[test]
fn unknown_expression_dropped_on_load() {
    // OpenUtau's UNote.AfterLoad drops note expressions whose abbr is not
    // registered; after_load must do the same.
    let mut p = UProject::new();
    p.tracks.push(UTrack::new("T2"));
    let mut part = domain::UVoicePart::new("P");
    part.track_no = 1;
    let mut note = p.create_note_at(60, 0, 480);
    note.phoneme_expressions.push(UExpression { index: Some(0), abbr: "ghost".into(), value: 1.0 });
    note.phoneme_expressions.push(UExpression { index: Some(0), abbr: "vel".into(), value: 90.0 });
    part.notes.push(note);
    p.parts.push(domain::UPart::Voice(part));

    p.before_save();
    let yaml = serde_yaml::to_string(&p).unwrap();
    let mut q: UProject = serde_yaml::from_str(&yaml).unwrap();
    q.after_load().unwrap();
    let domain::UPart::Voice(vp) = &q.parts[0] else { panic!() };
    // "vel" resolves through the default expressions added on load; "ghost" does not.
    assert_eq!(vp.notes[0].phoneme_expressions.len(), 1);
    assert_eq!(vp.notes[0].phoneme_expressions[0].abbr, "vel");
}
