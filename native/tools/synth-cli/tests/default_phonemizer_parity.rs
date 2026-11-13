//! T12: OpenUtau DefaultPhonemizer parity at the segmentation level.
//!
//! Reference: OpenUtau picks DefaultPhonemizer for a track with no phonemizer
//! (UTrack.cs:222). It emits ONE phoneme per note = the lyric verbatim (using the
//! oto alias the bank reports), marks a phoneme whose oto is missing as Error, and
//! RenderPhrase.FromPart drops errored phonemes BEFORE grouping. For
//! test/golden/sakura-test.ustx that yields exactly THREE phrases:
//!   @0 ms dur 1250 (sa ku sa ku sa) | @1500 dur 500 (ku no) | @2250 dur 1250 (ma mo sa to ni)
//! because the bank has no oto for the lyric "ya" (verified: no alias contains
//! "ya" anywhere in voice/oto.ini).
//!
//! Run: cargo test -p synth-cli --test default_phonemizer_parity -- --nocapture

use std::path::{Path, PathBuf};

use domain::UPart;
use phonemizer::{DefaultPhonemizer, TimingEngine};
use phrase::PhraseGrouping;
use synth_cli::pipeline;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1.0
}

#[test]
fn default_phonemizer_reproduces_openutau_segmentation_on_sakura() {
    let project = pipeline::load_project(&repo_root().join("test/golden/sakura-test.ustx"))
        .expect("load sakura-test.ustx");
    let vb = pipeline::load_voicebank(&repo_root().join("test/golden/teto-english/library"))
        .expect("load teto-english bank");
    let part = match &project.parts[0] {
        UPart::Voice(p) => p,
        other => panic!("expected a voice part, got {other:?}"),
    };

    let (mut phs, dropped) = DefaultPhonemizer.process_report(&part.notes, Some(&vb));
    println!("dropped {} note(s): {:?}", dropped.len(), dropped);
    TimingEngine.process(
        &part.notes,
        part.position,
        &mut phs,
        &project.time_axis,
        Some(&vb),
    );
    let groups = PhraseGrouping::group(&phs);
    for g in &groups {
        println!(
            "  @{:.1} ms leading {:.1} dur {:.1} ms -> {}",
            g.position_ms,
            g.leading_ms,
            g.duration_ms,
            g.phonemes
                .iter()
                .map(|p| p.phoneme.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        );
    }

    // 1. exactly the "ya" note is dropped (no oto alias anywhere in the bank)
    assert_eq!(
        dropped.len(),
        1,
        "only the note without an oto alias is dropped: {dropped:?}"
    );
    assert!(
        dropped[0].contains("ya"),
        "the dropped note must be 'ya': {dropped:?}"
    );

    // 2. three phrases at the reference positions/durations
    assert_eq!(groups.len(), 3, "OpenUtau segments sakura into 3 phrases");
    let want = [(0.0, 1250.0), (1500.0, 500.0), (2250.0, 1250.0)];
    for (g, (p, d)) in groups.iter().zip(want.iter()) {
        assert!(
            close(g.position_ms, *p),
            "position {:.1} != {:.1}",
            g.position_ms,
            p
        );
        assert!(
            close(g.duration_ms, *d),
            "duration {:.1} != {:.1}",
            g.duration_ms,
            d
        );
    }

    // 3. the aliases are the reference's phones, one per note
    let phones: Vec<Vec<&str>> = groups
        .iter()
        .map(|g| g.phonemes.iter().map(|p| p.phoneme.as_str()).collect())
        .collect();
    assert_eq!(phones[0], vec!["sa", "ku", "sa", "ku", "sa"]);
    assert_eq!(phones[1], vec!["ku", "no"]);
    assert_eq!(phones[2], vec!["ma", "mo", "sa", "to", "ni"]);

    // 4. every emitted phoneme has an oto, so the pipeline renders all 3 phrases
    //    (no phrase is skipped by the sample-based gate)
    for p in &phs {
        assert!(
            vb.lookup(&p.phoneme, p.tone).is_some(),
            "phoneme {:?} has no oto",
            p.phoneme
        );
    }
}

#[test]
fn every_note_maps_to_at_most_one_phoneme() {
    let project = pipeline::load_project(&repo_root().join("test/golden/sakura-test.ustx"))
        .expect("load project");
    let vb = pipeline::load_voicebank(&repo_root().join("test/golden/teto-english/library"))
        .expect("load bank");
    let part = match &project.parts[0] {
        UPart::Voice(p) => p,
        other => panic!("expected a voice part, got {other:?}"),
    };
    let (phs, _dropped) = DefaultPhonemizer.process_report(&part.notes, Some(&vb));
    for (i, _note) in part.notes.iter().enumerate() {
        let n = phs.iter().filter(|p| p.parent == Some(i)).count();
        assert!(
            n <= 1,
            "note {i} produced {n} phonemes (default phonemizer is 1:1)"
        );
    }
    assert_eq!(
        phs.len(),
        part.notes
            .iter()
            .filter(|n| !n.lyric.trim().is_empty())
            .count()
            - 1,
        "one phoneme per note minus the one dropped note"
    );
}

#[test]
fn default_phonemizer_strips_the_phonetic_hint() {
    // A hinted lyric must resolve on the lyric only: OpenUtau stores the hint in
    // a separate field, so DefaultPhonemizer sees "sa", not "sa [ku]".
    let vb = pipeline::load_voicebank(&repo_root().join("test/golden/teto-english/library"))
        .expect("load bank");
    let mut noted = domain::UNote::default();
    noted.lyric = "sa [ku]".to_string();
    noted.tone = 69;
    noted.duration = 240;
    let (phs, dropped) = DefaultPhonemizer.process_report(&[noted], Some(&vb));
    assert!(
        dropped.is_empty(),
        "hint must not break the lookup: {dropped:?}"
    );
    assert_eq!(phs.len(), 1);
    assert_eq!(phs[0].phoneme, "sa");
}
#[test]
fn from_track_maps_default_explicitly_and_keeps_universal_as_fallback() {
    // The applied pipeline hunks must never change the silent default: a track with
    // no phonemizer stays Universal; "default" is an explicit opt-in.
    let mut project = pipeline::load_project(&repo_root().join("test/golden/sakura-test.ustx"))
        .expect("load project");
    project.tracks[0].phonemizer = None;
    assert_eq!(
        pipeline::PhonemizerKind::from_track(&project.tracks[0]),
        pipeline::PhonemizerKind::Universal,
        "no phonemizer on the track must stay Universal"
    );
    project.tracks[0].phonemizer = Some("default".to_string());
    assert_eq!(
        pipeline::PhonemizerKind::from_track(&project.tracks[0]),
        pipeline::PhonemizerKind::Default,
        "'default' must map to the OpenUtau-parity mode"
    );
    project.tracks[0].phonemizer = Some("japanese".to_string());
    assert_eq!(
        pipeline::PhonemizerKind::from_track(&project.tracks[0]),
        pipeline::PhonemizerKind::Japanese
    );
}
