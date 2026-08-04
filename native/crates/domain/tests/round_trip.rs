//! YAML round-trip tests: serialize → deserialize → equal, plus
//! compatibility with a real-world `.ustx` file (UtaFormatix's template,
//! which OpenUtau itself can load).

use domain::{
    UCurve, UExpression, UExpressionDescriptor, UPart, UPhonemeOverride, UProject, UTempo,
    UTimeSignature, UMixFx, UNote, URenderSettings, UTrack, UstxVersion, UVoicePart, UWavePart,
    ATK, DYN, GEN, K_USTX_VERSION, VEL,
};

fn assert_projects_equal(a: &UProject, b: &UProject) {
    assert_eq!(a.name, b.name);
    assert_eq!(a.comment, b.comment);
    assert_eq!(a.output_dir, b.output_dir);
    assert_eq!(a.cache_dir, b.cache_dir);
    assert_eq!(a.ustx_version, b.ustx_version);
    assert_eq!(a.bpm, b.bpm);
    assert_eq!(a.beat_per_bar, b.beat_per_bar);
    assert_eq!(a.beat_unit, b.beat_unit);
    assert_eq!(a.expressions, b.expressions);
    assert_eq!(a.exp_selectors, b.exp_selectors);
    assert_eq!(a.exp_primary, b.exp_primary);
    assert_eq!(a.exp_secondary, b.exp_secondary);
    assert_eq!(a.key, b.key);
    assert_eq!(a.time_signatures, b.time_signatures);
    assert_eq!(a.tempos, b.tempos);
    assert_eq!(a.tracks, b.tracks);
    assert_eq!(a.parts, b.parts);
}

/// Round-trip a project through its serialized form: `before_save` →
/// serialize → deserialize → `after_load`, then compare every field.
fn round_trip(mut project: UProject) -> (UProject, UProject, String) {
    project.before_save();
    let yaml = serde_yaml::to_string(&project).expect("serialize");
    let mut loaded: UProject = serde_yaml::from_str(&yaml).expect("deserialize");
    // The serialized form must match the saved lists exactly.
    assert_eq!(project.voice_parts, loaded.voice_parts);
    assert_eq!(project.wave_parts, loaded.wave_parts);
    loaded.after_load().expect("after_load");
    assert_projects_equal(&project, &loaded);
    (project, loaded, yaml)
}

#[test]
fn default_project_round_trip() {
    let (p, q, yaml) = round_trip(UProject::create());
    assert_eq!(q.ustx_version, Some(K_USTX_VERSION));
    assert!(yaml.contains("ustx_version"));
    assert!(yaml.contains("voice_parts: []"));
    assert!(yaml.contains("wave_parts: []"));
    assert!(yaml.contains("exp_primary: 0"));
    assert!(yaml.contains("key: 0"));
    assert!(yaml.contains("bpm: 120.0"));
    assert!(yaml.contains("beat_per_bar: 4"));
    assert!(yaml.contains("beat_unit: 4"));
    assert!(yaml.contains("track_name: Track1"));
    // Fresh project without before_save does not write the transient lists.
    let fresh = UProject::create();
    let yaml2 = serde_yaml::to_string(&fresh).unwrap();
    assert!(!yaml2.contains("voice_parts"));
    let _ = p;
}

#[test]
fn full_project_round_trip() {
    let mut p = UProject::create();
    p.name = "Round Trip Song".into();
    p.comment = "demo comment".into();
    p.output_dir = "out".into();
    p.cache_dir = "cache".into();
    p.bpm = 138.0;
    p.beat_per_bar = 3;
    p.beat_unit = 8;
    p.key = 5;
    p.exp_primary = 2;
    p.exp_secondary = 3;
    p.time_signatures = vec![
        UTimeSignature::new(0, 4, 4),
        UTimeSignature::new(4, 3, 4),
        UTimeSignature::new(8, 6, 8),
    ];
    p.tempos = vec![UTempo::new(0, 138.0), UTempo::new(1920, 90.5), UTempo::new(5760, 200.0)];
    p.exp_selectors = vec![DYN.into(), "pitd".into(), VEL.into()];

    // Track 0: full configuration.
    p.tracks[0].track_name = "Lead".into();
    p.tracks[0].track_color = "Red".into();
    p.tracks[0].singer = Some("dummy-id".into());
    p.tracks[0].phonemizer = Some("Builtin.DefaultPhonemizer".into());
    p.tracks[0].renderer_settings = Some(URenderSettings {
        renderer: Some("classic".into()),
        resampler: Some("worldline".into()),
        wavtool: Some("wavtool".into()),
    });
    p.tracks[0].mute = true;
    p.tracks[0].volume = -6.5;
    p.tracks[0].pan = 0.25;
    p.tracks[0].voice_color_names = vec!["".into(), "power".into(), "soft".into()];
    p.tracks[0].track_expressions.push(UExpressionDescriptor::numerical(
        "gender",
        GEN,
        -100.0,
        100.0,
        0.0,
        Some("g"),
    ));

    // Track 1: solo + FX.
    let mut t2 = UTrack::new("Harmony");
    t2.solo = true;
    t2.mix_fx = Some(UMixFx { enabled: true, ..Default::default() });
    p.tracks.push(t2);

    // Voice part with notes and curves.
    let mut part = UVoicePart::new("Verse");
    part.track_no = 0;
    part.position = 0;
    part.duration = 3840;
    part.comment = "first verse".into();

    let mut n1 = p.create_note_at(60, 0, 480);
    n1.lyric = "あ".into();
    n1.tuning = -10;
    n1.phonemizer_override = Some("custom.Phonemizer".into());
    n1.phoneme_expressions.push(UExpression { index: Some(0), abbr: VEL.into(), value: 80.0 });
    n1.phoneme_expressions.push(UExpression { index: Some(0), abbr: GEN.into(), value: 12.0 });
    n1.phoneme_overrides.push(UPhonemeOverride {
        index: 0,
        phoneme: Some("a".into()),
        offset: Some(5),
        preutter_delta: Some(1.5),
        ..Default::default()
    });

    let mut n2 = p.create_note_at(62, 480, 960);
    n2.lyric = "a [e]".into();
    n2.vibrato.set_length(50.0);
    n2.vibrato.set_period(180.0);
    n2.vibrato.set_in(20.0);
    n2.vibrato.set_drift(5.0);
    n2.vibrato.set_vol_link(-30.0);
    n2.pitch.data = vec![
        domain::PitchPoint::new(-40.0, 0.0, domain::PitchPointShape::Io),
        domain::PitchPoint::new(0.0, 10.0, domain::PitchPointShape::L),
        domain::PitchPoint::new(40.0, -5.0, domain::PitchPointShape::Sp),
    ];
    n2.pitch.snap_first = false;
    n2.phoneme_expressions.push(UExpression { index: Some(1), abbr: VEL.into(), value: 200.0 });

    let mut n3 = p.create_note_at(64, 1440, 480);
    n3.lyric = "R".into();

    part.notes = vec![n1, n2, n3];
    part.curves.push(UCurve {
        abbr: DYN.into(),
        xs: vec![0, 480, 960, 1440],
        ys: vec![-240, -120, 0, 120],
    });
    part.curves.push(UCurve { abbr: "pitd".into(), xs: vec![0, 1920], ys: vec![0, 0] });
    p.parts.push(UPart::Voice(part));

    // Wave part on track 1.
    let wave = UWavePart {
        name: "audio".into(),
        track_no: 1,
        position: 1920,
        relative_path: Some("audio/clip.wav".into()),
        file_duration_ms: 5000.0,
        skip: 15,
        trim: 30,
        fadein: 45,
        fadeout: 60,
        ..Default::default()
    };
    p.parts.push(UPart::Wave(wave));

    let (saved, loaded, yaml) = round_trip(p);

    // Spot-check the serialized content.
    assert!(yaml.contains("ustx_version"));
    assert!(yaml.contains("lyric: あ"));
    assert!(yaml.contains("phonemizer: custom.Phonemizer"));
    assert!(yaml.contains("vol_link: -30.0"));
    assert!(yaml.contains("relative_path: audio/clip.wav"));

    // After load, parts are merged back from voice_parts/wave_parts.
    assert_eq!(loaded.parts.len(), 2);
    match &loaded.parts[0] {
        UPart::Voice(vp) => {
            assert_eq!(vp.notes.len(), 3);
            assert_eq!(vp.notes[0].phoneme_expressions.len(), 2);
            assert_eq!(vp.notes[1].pitch.data.len(), 3);
            assert!(!vp.notes[1].pitch.snap_first);
            assert_eq!(vp.curves.len(), 2);
            assert_eq!(vp.duration, 3840);
        }
        UPart::Wave(_) => panic!("expected voice part first"),
    }
    match &loaded.parts[1] {
        UPart::Wave(wp) => {
            assert_eq!(wp.relative_path.as_deref(), Some("audio/clip.wav"));
            assert_eq!(wp.file_duration_ms, 5000.0);
        }
        UPart::Voice(_) => panic!("expected wave part second"),
    }
    assert_eq!(saved.tracks, loaded.tracks);
    assert_eq!(loaded.tracks[0].mix_fx, None);
    assert!(loaded.tracks[1].mix_fx.is_some());
    assert_eq!(loaded.tracks[0].renderer_settings.as_ref().unwrap().resampler.as_deref(), Some("worldline"));
    // Time axis is rebuilt after load.
    assert_eq!(loaded.time_axis.bpm_at_tick(0), 138.0);
    assert_eq!(loaded.time_axis.bpm_at_tick(2000), 90.5);
    assert_eq!(loaded.time_axis.bpm_at_tick(6000), 200.0);
}

#[test]
fn special_lyrics_round_trip() {
    // Mirrors OpenUtau's UstxYamlTest.SpecialLyric.
    for lyric in ["-@", "-&", "null", "true", "-,", "\t- asdf", "あ", "a [e]", "+", "...あ"] {
        let note = UNote { lyric: lyric.into(), ..Default::default() };
        let yaml = serde_yaml::to_string(&note).unwrap();
        let back: UNote = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(back.lyric, lyric, "lyric {lyric:?} failed round trip");
    }
}

#[test]
fn unknown_keys_are_ignored() {
    // Files written by other tools may carry extra keys (e.g. `resolution`)
    // that OpenUtau ignores; serde must do the same.
    let yaml = r#"
name: X
comment: ''
output_dir: Vocal
cache_dir: UCache
ustx_version: '0.9'
resolution: 480
bpm: 120
beat_per_bar: 4
beat_unit: 4
expressions: {}
time_signatures:
- bar_position: 0
  beat_per_bar: 4
  beat_unit: 4
tempos:
- position: 0
  bpm: 120
tracks: []
"#;
    let p: UProject = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(p.name, "X");
    assert_eq!(p.ustx_version, Some(UstxVersion::new(0, 9)));
}

#[test]
fn legacy_v06_timing_migration() {
    // ustx 0.5 files have no time_signatures/tempos; they are rebuilt from
    // the legacy bpm/beat_per_bar/beat_unit fields on load.
    let yaml = r#"
name: Old
comment: ''
output_dir: Vocal
cache_dir: UCache
ustx_version: '0.5'
bpm: 96
beat_per_bar: 3
beat_unit: 8
expressions: {}
time_signatures: []
tempos: []
tracks:
- track_name: Track1
voice_parts: []
"#;
    let mut p: UProject = serde_yaml::from_str(yaml).unwrap();
    p.after_load().unwrap();
    assert_eq!(p.ustx_version, Some(K_USTX_VERSION));
    assert_eq!(p.tempos, vec![UTempo::new(0, 96.0)]);
    assert_eq!(p.time_signatures, vec![UTimeSignature::new(0, 3, 8)]);
    // 3/8 time: one eighth note = 480*4/8 = 240 ticks; at 96 bpm that is
    // 60000/96/2 = 312.5 ms.
    let ms = p.time_axis.tick_to_ms(240.0);
    assert!((ms - 312.5).abs() < 1e-9, "got {ms}");
}

#[test]
fn legacy_v03_acc_migration() {
    let yaml = r#"
name: Old
comment: ''
output_dir: Vocal
cache_dir: UCache
ustx_version: '0.3'
expressions:
  acc:
    name: accent
    abbr: acc
    type: Numerical
    min: 0
    max: 200
    default_value: 100
time_signatures:
- bar_position: 0
  beat_per_bar: 4
  beat_unit: 4
tempos:
- position: 0
  bpm: 120
tracks:
- track_name: Track1
voice_parts:
- name: Part
  comment: ''
  track_no: 0
  position: 0
  notes:
  - position: 0
    duration: 480
    tone: 60
    lyric: a
    pitch:
      data: []
      snap_first: true
    vibrato: {length: 0.0, period: 175.0, depth: 25.0, in: 10.0, out: 10.0, shift: 0.0, drift: 0.0, vol_link: 0.0}
    phoneme_expressions:
    - {index: 0, abbr: acc, value: 123}
    phoneme_overrides: []
  curves: []
wave_parts: []
"#;
    let mut p: UProject = serde_yaml::from_str(yaml).unwrap();
    p.after_load().unwrap();
    assert!(!p.expressions.contains_key("acc"));
    assert!(p.expressions.contains_key("atk"));
    let atk = &p.expressions[ATK];
    assert_eq!(atk.name, "attack");
    let UPart::Voice(vp) = &p.parts[0] else { panic!("expected voice part") };
    assert_eq!(vp.notes[0].phoneme_expressions[0].abbr, ATK);
    assert_eq!(vp.notes[0].phoneme_expressions[0].value, 123.0);
}

#[test]
fn legacy_v04_lyric_migration() {
    let yaml = r#"
name: Old
comment: ''
output_dir: Vocal
cache_dir: UCache
ustx_version: '0.4'
expressions: {}
time_signatures:
- bar_position: 0
  beat_per_bar: 4
  beat_unit: 4
tempos:
- position: 0
  bpm: 120
tracks:
- track_name: Track1
voice_parts:
- name: Part
  comment: ''
  track_no: 0
  position: 0
  notes:
  - position: 0
    duration: 480
    tone: 60
    lyric: "...あ"
    pitch: {data: [], snap_first: true}
    vibrato: {length: 0.0, period: 175.0, depth: 25.0, in: 10.0, out: 10.0, shift: 0.0, drift: 0.0, vol_link: 0.0}
    phoneme_expressions: []
    phoneme_overrides: []
  curves: []
wave_parts: []
"#;
    let mut p: UProject = serde_yaml::from_str(yaml).unwrap();
    p.after_load().unwrap();
    let UPart::Voice(vp) = &p.parts[0] else { panic!("expected voice part") };
    assert_eq!(vp.notes[0].lyric, "+あ");
}

#[test]
fn legacy_v06_exp_selector_padding() {
    let yaml = r#"
name: Old
comment: ''
output_dir: Vocal
cache_dir: UCache
ustx_version: '0.6'
exp_selectors: [dyn]
expressions: {}
time_signatures:
- bar_position: 0
  beat_per_bar: 4
  beat_unit: 4
tempos:
- position: 0
  bpm: 120
tracks: []
voice_parts: []
"#;
    let mut p: UProject = serde_yaml::from_str(yaml).unwrap();
    p.after_load().unwrap();
    assert_eq!(p.exp_selectors.len(), 10);
    assert_eq!(p.exp_selectors[0], "dyn");
    assert_eq!(p.exp_selectors[1], "pitd");
}

// ---------------------------------------------------------------------------
// Real-world file compatibility: UtaFormatix's template.ustx (ustx 0.6),
// which OpenUtau itself can open. BOM stripped.
// ---------------------------------------------------------------------------

const SAMPLE_USTX: &str = r#"name: New Project
comment: ''
output_dir: Vocal
cache_dir: UCache
ustx_version: 0.6
resolution: 480
bpm: 120
beat_per_bar: 4
beat_unit: 4
time_signatures:
  - bar_position: 0
    beat_per_bar: 4
    beat_unit: 4
tempos:
  - position: 0
    bpm: 120
expressions:
  dyn:
    name: dynamics (curve)
    abbr: dyn
    type: Curve
    min: -240
    max: 120
    default_value: 0
    is_flag: false
    flag: ''
  pitd:
    name: pitch deviation (curve)
    abbr: pitd
    type: Curve
    min: -1200
    max: 1200
    default_value: 0
    is_flag: false
    flag: ''
  clr:
    name: voice color
    abbr: clr
    type: Options
    min: 0
    max: -1
    default_value: 0
    is_flag: false
    options: []
  eng:
    name: resampler engine
    abbr: eng
    type: Options
    min: 0
    max: 1
    default_value: 0
    is_flag: false
    options:
    - ''
    - worldline
  vel:
    name: velocity
    abbr: vel
    type: Numerical
    min: 0
    max: 200
    default_value: 100
    is_flag: false
    flag: ''
  vol:
    name: volume
    abbr: vol
    type: Numerical
    min: 0
    max: 200
    default_value: 100
    is_flag: false
    flag: ''
  atk:
    name: attack
    abbr: atk
    type: Numerical
    min: 0
    max: 200
    default_value: 100
    is_flag: false
    flag: ''
  dec:
    name: decay
    abbr: dec
    type: Numerical
    min: 0
    max: 100
    default_value: 0
    is_flag: false
    flag: ''
  gen:
    name: gender
    abbr: gen
    type: Numerical
    min: -100
    max: 100
    default_value: 0
    is_flag: true
    flag: g
  genc:
    name: gender (curve)
    abbr: genc
    type: Curve
    min: -100
    max: 100
    default_value: 0
    is_flag: false
    flag: ''
  bre:
    name: breath
    abbr: bre
    type: Numerical
    min: 0
    max: 100
    default_value: 0
    is_flag: true
    flag: B
  brec:
    name: breathiness (curve)
    abbr: brec
    type: Curve
    min: -100
    max: 100
    default_value: 0
    is_flag: false
    flag: ''
  lpf:
    name: lowpass
    abbr: lpf
    type: Numerical
    min: 0
    max: 100
    default_value: 0
    is_flag: true
    flag: H
  mod:
    name: modulation
    abbr: mod
    type: Numerical
    min: 0
    max: 100
    default_value: 0
    is_flag: false
    flag: ''
  alt:
    name: alternate
    abbr: alt
    type: Numerical
    min: 0
    max: 16
    default_value: 0
    is_flag: false
    flag: ''
  shft:
    name: tone shift
    abbr: shft
    type: Numerical
    min: -36
    max: 36
    default_value: 0
    is_flag: false
    flag: ''
  shfc:
    name: tone shift (curve)
    abbr: shfc
    type: Curve
    min: -1200
    max: 1200
    default_value: 0
    is_flag: false
    flag: ''
  tenc:
    name: tension (curve)
    abbr: tenc
    type: Curve
    min: -100
    max: 100
    default_value: 0
    is_flag: false
    flag: ''
  voic:
    name: voicing (curve)
    abbr: voic
    type: Curve
    min: 0
    max: 100
    default_value: 100
    is_flag: false
    flag: ''
tracks:
- phonemizer: OpenUtau.Core.DefaultPhonemizer
  mute: false
  solo: false
  volume: 0
voice_parts:
- name: New Part
  comment: ''
  track_no: 0
  position: 1920
  notes:
  - position: 480
    duration: 480
    tone: 60
    lyric: a
    pitch:
      data:
      - {x: -1, y: 0, shape: io}
      - {x: 1, y: 0, shape: io}
      snap_first: true
    vibrato: {length: 0, period: 175, depth: 25, in: 10, out: 10, shift: 0, drift: 0}
    phoneme_expressions: []
    phoneme_overrides: []
  - position: 960
    duration: 480
    tone: 62
    lyric: a
    pitch:
      data:
      - {x: -1, y: -20, shape: io}
      - {x: 1, y: 0, shape: io}
      snap_first: true
    vibrato: {length: 0, period: 175, depth: 25, in: 10, out: 10, shift: 0, drift: 0}
    phoneme_expressions: []
    phoneme_overrides: []
  - position: 1440
    duration: 240
    tone: 62
    lyric: a
    pitch:
      data:
      - {x: -1, y: 0, shape: io}
      - {x: 1, y: 0, shape: io}
      snap_first: true
    vibrato: {length: 0, period: 175, depth: 25, in: 10, out: 10, shift: 0, drift: 0}
    phoneme_expressions: []
    phoneme_overrides: []
  curves: []
wave_parts: []
"#;

#[test]
fn openutau_compatible_sample_loads_and_round_trips() {
    let mut p: UProject = serde_yaml::from_str(SAMPLE_USTX).expect("parse real-world ustx");
    assert_eq!(p.ustx_version, Some(UstxVersion::new(0, 6)));
    assert_eq!(p.name, "New Project");
    assert_eq!(p.bpm, 120.0);
    assert_eq!(p.tempos, vec![UTempo::new(0, 120.0)]);
    assert_eq!(p.tracks[0].phonemizer.as_deref(), Some("OpenUtau.Core.DefaultPhonemizer"));
    assert_eq!(p.tracks[0].singer, None);
    assert_eq!(p.tracks[0].track_name, "New Track"); // defaulted

    p.after_load().unwrap();

    // Version bumped, defaults merged (norm/mod+/dir were missing from this file).
    assert_eq!(p.ustx_version, Some(K_USTX_VERSION));
    assert_eq!(p.expressions.len(), 22);
    assert_eq!(p.expressions["eng"].options.as_deref(), Some(&["".to_string(), "worldline".to_string()][..]));
    assert_eq!(p.expressions["clr"].max, -1.0);
    // exp_selectors padded to the default 10 (file had none).
    assert_eq!(p.exp_selectors.len(), 10);
    assert_eq!(p.exp_selectors[0], "dyn");

    // Parts merged from voice_parts; 3 notes; duration normalized to the
    // minimum (last note ends at 1680 within the part, i.e. tick 3600;
    // next bar beat is 3840, so min duration = 3840 - 1920 = 1920).
    assert_eq!(p.parts.len(), 1);
    let UPart::Voice(vp) = &p.parts[0] else { panic!("expected voice part") };
    assert_eq!(vp.name, "New Part");
    assert_eq!(vp.position, 1920);
    assert_eq!(vp.track_no, 0);
    assert_eq!(vp.duration, 1920);
    assert_eq!(vp.notes.len(), 3);
    assert_eq!(vp.notes[0].position, 480);
    assert_eq!(vp.notes[0].tone, 60);
    assert_eq!(vp.notes[0].vibrato.period, 175.0);
    assert_eq!(vp.notes[0].vibrato.vol_link, 0.0); // defaulted
    assert_eq!(vp.notes[1].pitch.data[0].y, -20.0);
    assert_eq!(vp.notes[1].pitch.data[0].shape, domain::PitchPointShape::Io);
    assert!(vp.curves.is_empty());

    // Time axis built from the file timing.
    // OpenUtau semantics: bar is 0-indexed (barPos=0 for first segment),
    // ticksPerBar=1920 (4/4 @120bpm) → tick 1920 = bar 1, beat 0, tick 0.
    assert_eq!(p.time_axis.bpm_at_tick(0), 120.0);
    assert_eq!(p.time_axis.tick_to_bar_beat(1920), (1, 0, 0));

    // Double round trip: serialize the loaded project and load it again.
    // after_load() consumes voice_parts into parts, so before_save() must
    // be called again to rebuild the serialized lists (mirrors OpenUtau:
    // BeforeSave always precedes Save).
    p.before_save();
    let yaml = serde_yaml::to_string(&p).unwrap();
    let mut q: UProject = serde_yaml::from_str(&yaml).unwrap();
    q.after_load().unwrap();
    assert_projects_equal(&p, &q);
}
