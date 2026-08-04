//! Integration tests: phonemize `test-data/mock-song.ustx`, group the
//! phonemes like `RenderPhrase.FromPart`, and build a full `RenderInput`
//! with the phrase builder.

use std::path::Path;

use domain::{UPart, UProject, UVoicePart};
use phonemizer::{EnglishCvvcPhonemizer, Phonemizer, TimingEngine};
use phrase::{PhraseBuilder, PhraseError, PhraseGroup, PhraseGrouping};
use voicebank::{load_voicebank, Voicebank};

fn mock_song() -> UProject {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/mock-song.ustx");
    let yaml = std::fs::read_to_string(&path).expect("read mock-song.ustx");
    let mut project: UProject = serde_yaml::from_str(&yaml).expect("parse mock-song.ustx");
    project.after_load().expect("after_load");
    project
}

fn mock_voicebank() -> Voicebank {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/mock-voicebank");
    assert!(
        dir.is_dir(),
        "mock voicebank not found at {}",
        dir.display()
    );
    load_voicebank(&dir).expect("load mock voicebank")
}

fn voice_part(project: &UProject) -> &UVoicePart {
    match &project.parts[0] {
        UPart::Voice(part) => part,
        UPart::Wave(_) => panic!("expected a voice part"),
    }
}

/// Phonemize + time the mock song's notes (the standard derivation path).
fn derive_phonemes(project: &UProject, part: &UVoicePart, vb: &Voicebank) -> Vec<domain::UPhoneme> {
    let mut phonemes = EnglishCvvcPhonemizer::new().process(&part.notes, Some(vb));
    TimingEngine.process(
        &part.notes,
        part.position,
        &mut phonemes,
        &project.time_axis,
        Some(vb),
    );
    phonemes
}

#[test]
fn mock_song_groups_into_one_phrase() {
    let project = mock_song();
    let vb = mock_voicebank();
    let part = voice_part(&project);
    let phonemes = derive_phonemes(&project, part, &vb);

    // 9 phonemes: read[r iy d] red[r eh d] x la[l ae]. (`x` keeps the
    // phonemes unmappable in the mock bank — the G2P dict maps `a` → A,
    // which the mock bank DOES alias, breaking the "no oto mapping" intent.)
    assert_eq!(phonemes.len(), 9);
    let names: Vec<&str> = phonemes.iter().map(|p| p.phoneme.as_str()).collect();
    assert_eq!(names, ["r", "iy", "d", "r", "eh", "d", "x", "l", "ae"]);

    // Every phoneme touches the next one (End == next.position): the
    // whole part is one phrase, like RenderPhrase.FromPart would decide.
    let groups = PhraseGrouping::group(&phonemes);
    assert_eq!(groups.len(), 1);
    let group = &groups[0];
    assert_eq!(group.phonemes.len(), 9);
    // First phoneme starts at project ms 500 (part tick 480 @120bpm);
    // last phoneme ends at 3000 ms.
    assert!(
        (group.position_ms - 500.0).abs() < 1e-9,
        "position_ms {}",
        group.position_ms
    );
    assert!(
        (group.duration_ms - 2500.0).abs() < 1e-9,
        "duration_ms {}",
        group.duration_ms
    );
    assert_eq!(group.leading_ms, 0.0);
}

#[test]
fn phrase_builder_builds_render_input_from_mock_song() {
    let project = mock_song();
    let vb = mock_voicebank();
    let track = &project.tracks[0];
    let part = voice_part(&project);
    let phonemes = derive_phonemes(&project, part, &vb);
    let groups = PhraseGrouping::group(&phonemes);
    assert_eq!(groups.len(), 1);

    let input = PhraseBuilder::new(&project, track, part, Some(&vb))
        .build(&groups[0])
        .expect("build RenderInput");

    // Phrase timing.
    assert!((input.phrase.position_ms - 500.0).abs() < 1e-9);
    assert!((input.phrase.duration_ms - 2500.0).abs() < 1e-9);
    assert_eq!(input.phrase.leading_ms, 0.0);
    assert_eq!(input.phrase.leading_ticks, 0);
    assert_eq!(input.phrase.time_axis_hint.as_deref(), Some("120bpm 4/4"));

    // Notes: all four, with ms timing.
    assert_eq!(input.notes.len(), 4);
    let note_positions: Vec<f64> = input.notes.iter().map(|n| n.position_ms).collect();
    let note_durations: Vec<f64> = input.notes.iter().map(|n| n.duration_ms).collect();
    for (got, want) in note_positions.iter().zip([500.0, 1000.0, 1500.0, 2000.0]) {
        assert!((got - want).abs() < 1e-9, "position {got} != {want}");
    }
    for (got, want) in note_durations.iter().zip([500.0, 500.0, 500.0, 1000.0]) {
        assert!((got - want).abs() < 1e-9, "duration {got} != {want}");
    }
    assert_eq!(
        input.notes.iter().map(|n| n.tone).collect::<Vec<_>>(),
        vec![60, 62, 64, 65]
    );

    // Phonemes: 9, adjacent, parent_note pointing into input.notes.
    assert_eq!(input.phonemes.len(), 9);
    assert!((input.phonemes[0].position_ms - 500.0).abs() < 1e-9);
    assert!((input.phonemes[0].duration_ms - 500.0 / 3.0).abs() < 1e-6);
    assert_eq!(input.phonemes[0].parent_note, 0);
    assert_eq!(input.phonemes[6].parent_note, 2);
    assert_eq!(input.phonemes[7].parent_note, 3);
    assert_eq!(input.phonemes[8].parent_note, 3);
    assert!((input.phonemes[8].position_ms - 2500.0).abs() < 1e-9);
    assert!((input.phonemes[8].duration_ms - 500.0).abs() < 1e-9);

    // Pitch grid: 481 samples (tick 480..2880 every 5 ticks), flat per
    // note with the default portamento points pulling the boundary sample
    // to the next note's pitch (reference behavior).
    assert_eq!(input.pitches_cents.len(), 481);
    assert!(input.pitches_cents[..95].iter().all(|&p| p == 6000));
    assert_eq!(input.pitches_cents[95], 6200); // 5 ticks before note 2
    assert!(input.pitches_cents[96..191].iter().all(|&p| p == 6200));
    assert_eq!(input.pitches_cents[191], 6400);
    assert!(input.pitches_cents[192..287].iter().all(|&p| p == 6400));
    assert_eq!(input.pitches_cents[287], 6500);
    assert!(input.pitches_cents[288..].iter().all(|&p| p == 6500));

    // Curves: defaults of the registered expressions, aligned with the
    // pitch grid; shfc lands in extra (no tone_shift bucket in Curves).
    assert_eq!(input.curves.dynamics.len(), 481);
    assert!(input
        .curves
        .dynamics
        .iter()
        .all(|&v| (v - 1.0).abs() < 1e-6));
    assert!(input.curves.gender.iter().all(|&v| v == 0.0));
    assert!(input.curves.breathiness.iter().all(|&v| v == 0.0));
    assert!(input.curves.tension.iter().all(|&v| v == 0.0));
    assert!(input.curves.voicing.iter().all(|&v| v == 100.0));
    assert_eq!(input.curves.extra.len(), 1);
    assert_eq!(input.curves.extra[0].abbr, "shfc");
    assert!(input.curves.extra[0].values.iter().all(|&v| v == 0.0));

    // No phoneme of the mock song maps to an oto alias ("r", "iy", ...),
    // so the sample-based section stays empty.
    assert!(input.sample_based.is_none());
    assert!(input.neural.is_none());
}

#[test]
fn phrase_builder_fills_sample_based_for_mapped_phonemes() {
    let project = domain::UProject::create();
    let vb = mock_voicebank();
    let track = &project.tracks[0];

    // One note "a[A]" — "A" maps in the mock bank (preutter 20, overlap 40).
    let mut note = project.create_note_at(60, 0, 480);
    note.lyric = "a[A]".into();
    let part = UVoicePart {
        position: 0,
        notes: vec![note],
        ..Default::default()
    };

    let mut phonemes = EnglishCvvcPhonemizer::new().process(&part.notes, Some(&vb));
    TimingEngine.process(
        &part.notes,
        part.position,
        &mut phonemes,
        &project.time_axis,
        Some(&vb),
    );
    assert_eq!(phonemes.len(), 1);
    assert_eq!(phonemes[0].phoneme, "A");
    let groups = PhraseGrouping::group(&phonemes);
    assert_eq!(groups.len(), 1);

    let input = PhraseBuilder::new(&project, track, &part, Some(&vb))
        .build(&groups[0])
        .expect("build RenderInput");

    let sb = input.sample_based.expect("sample_based filled");
    assert_eq!(sb.oto.len(), 1);
    let entry = &sb.oto[0];
    assert_eq!(entry.alias, "A");
    assert_eq!(entry.file, "_a+_ha+_a+_a+_a+-.wav");
    assert!(entry.wav_path.ends_with("voice/_a+_ha+_a+_a+_a+-.wav"));
    assert_eq!(entry.preutter, 20.0);
    assert_eq!(entry.overlap, 40.0);
    // Envelope: preutter 20 ms, duration 500 ms, defaults vol/atk 100 dec 0.
    assert_eq!(entry.envelope.len(), 5);
    let pts: Vec<(f32, f32)> = entry.envelope.iter().map(|p| (p.x_ms, p.y)).collect();
    assert_eq!(
        pts,
        vec![
            (-20.0, 0.0),
            (-15.0, 1.0),
            (0.0, 1.0),
            (465.0, 1.0),
            (500.0, 0.0)
        ]
    );
    // Resampler flags of the default expressions, sorted (g0B0H0P86).
    assert_eq!(entry.flags, "B0H0P86g0");
    // Mirror fields of the first mapped phoneme.
    assert_eq!(sb.envelope, entry.envelope);
    assert_eq!(sb.flags, entry.flags);
    assert_eq!(sb.wav_path.as_deref(), Some(entry.wav_path.as_str()));
    // Phoneme timing picked up the oto preutter/overlap.
    assert_eq!(input.phonemes[0].leading_ms, 20.0);
    assert_eq!(input.phonemes[0].overlap_ms, 40.0);
    // Phrase leading is the first phoneme's preutter.
    assert_eq!(input.phrase.leading_ms, 20.0);
    assert_eq!(input.phrase.leading_ticks, 19); // 20 ms ≈ 19.2 ticks, banker-rounded
}

#[test]
fn empty_group_is_rejected() {
    let project = mock_song();
    let track = &project.tracks[0];
    let part = voice_part(&project);
    let group = PhraseGroup {
        phonemes: vec![],
        position_ms: 0.0,
        duration_ms: 0.0,
        leading_ms: 0.0,
    };
    let err = PhraseBuilder::new(&project, track, part, None)
        .build(&group)
        .unwrap_err();
    assert_eq!(err, PhraseError::Empty);
}
