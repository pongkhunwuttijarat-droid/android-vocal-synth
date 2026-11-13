//! T11: phrase segmentation + phonemizer parity vs OpenUtau for
//! test/golden/sakura-test.ustx with the Teto English bank
//! (test/golden/teto-english/library).
//!
//! Reference (OpenUtau Classic/Worldline, r1 and r2) segments that project into
//! THREE phrases:
//!   posMs=0.0    leading=135.7 dur=1250 phones: sa ku sa ku sa
//!   posMs=1500   leading=70.6  dur=500  phones: ku no
//!   posMs=2250   leading=85.5  dur=1250 phones: ma mo sa to ni
//! Our default path (Universal) produces TWO phrases; our japanese path produces
//! ONE phrase plus ONE skipped phrase. This test records why with per-phoneme
//! evidence and guards the recommended path against regressions.
//!
//! Print the tables:  cargo test -p synth-cli --test phrase_segmentation_parity -- --nocapture

use std::path::{Path, PathBuf};

use domain::UPart;
use phrase::PhraseGrouping;
use synth_cli::pipeline::{self, PhonemizerKind};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}
fn sakura_path() -> PathBuf {
    repo_root().join("test/golden/sakura-test.ustx")
}
fn bank_path() -> PathBuf {
    repo_root().join("test/golden/teto-english/library")
}

struct Ph {
    idx: usize,
    note: i64,
    lyric: String,
    raw: String,
    resolved: String,
    has_oto: bool,
    pos_ms: f64,
    dur_ms: f64,
    end_ms: f64,
}

fn table(kind: PhonemizerKind) -> (Vec<Ph>, Vec<String>, usize) {
    let project = pipeline::load_project(&sakura_path()).expect("load sakura-test.ustx");
    let vb = pipeline::load_voicebank(&bank_path()).expect("load teto-english bank");
    let track = &project.tracks[0];
    let part = match &project.parts[0] {
        UPart::Voice(p) => p,
        other => panic!("expected a voice part, got {other:?}"),
    };
    let phonemes = pipeline::derive_phonemes(&project, part, &vb, kind);
    let mut rows = Vec::new();
    for (i, p) in phonemes.iter().enumerate() {
        let lyric = p
            .parent
            .and_then(|n| part.notes.get(n))
            .map(|n| n.lyric.clone())
            .unwrap_or_default();
        rows.push(Ph {
            idx: i,
            note: p.parent.map(|n| n as i64).unwrap_or(-1),
            lyric,
            raw: p.raw_phoneme.clone(),
            resolved: p.phoneme.clone(),
            has_oto: vb.lookup(&p.phoneme, p.tone).is_some(),
            pos_ms: p.position_ms,
            dur_ms: p.duration_ms,
            end_ms: p.position_ms + p.duration_ms,
        });
    }
    let inputs =
        pipeline::build_phrase_inputs(&project, track, part, &vb, kind).expect("phrase inputs");
    let mut summary = Vec::new();
    let mut skipped = 0;
    for (i, ph) in inputs.iter().enumerate() {
        let mapped = ph
            .input
            .sample_based
            .as_ref()
            .map(|s| s.oto.len())
            .unwrap_or(0);
        let total = ph.input.phonemes.len();
        if mapped != total {
            skipped += 1;
        }
        summary.push(format!(
            "phrase {}: @{:.1} ms leading {:.1} dur {:.1} ms phonemes {} mapped {} -> {} ({})",
            i,
            ph.group.position_ms,
            ph.group.leading_ms,
            ph.group.duration_ms,
            total,
            mapped,
            if mapped == total {
                "would render"
            } else {
                "SKIPPED by pipeline"
            },
            ph.input
                .phonemes
                .iter()
                .map(|p| p.phoneme.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        ));
    }
    (rows, summary, skipped)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1.0
}

#[test]
fn universal_is_the_recommended_path_and_skips_nothing() {
    let (rows, summary, skipped) = table(PhonemizerKind::Universal);
    for line in &summary {
        println!("{line}");
    }
    let missing: Vec<&Ph> = rows.iter().filter(|r| !r.has_oto).collect();
    for m in &missing {
        println!(
            "  no oto: note {} {:?} raw {:?} resolved {:?}",
            m.note, m.lyric, m.raw, m.resolved
        );
    }
    assert_eq!(
        skipped, 0,
        "the universal path must not skip a phrase on sakura"
    );
    assert!(
        missing.is_empty(),
        "every universal phoneme must map to an oto alias"
    );
    assert_eq!(summary.len(), 2, "universal groups sakura into 2 phrases");
}

#[test]
fn japanese_path_pins_the_missing_oto_alias() {
    let (rows, summary, skipped) = table(PhonemizerKind::Japanese);
    println!("japanese per-phoneme table:");
    for r in &rows {
        println!(
            "  idx {:3} note {:>3} lyric {:>5} pos {:>8.1} dur {:>6.1} raw {:>12} resolved {:>12} oto {}",
            r.idx, r.note, r.lyric, r.pos_ms, r.dur_ms, r.raw, r.resolved, r.has_oto
        );
    }
    for line in &summary {
        println!("{line}");
    }
    let missing: Vec<&Ph> = rows.iter().filter(|r| !r.has_oto).collect();
    println!("phonemes without an oto alias: {}", missing.len());
    for m in &missing {
        println!(
            "  MISSING note {} lyric {:?} raw {:?} resolved {:?} pos {:.1}",
            m.note, m.lyric, m.raw, m.resolved, m.pos_ms
        );
    }
    assert_eq!(
        missing.len(),
        1,
        "exactly one phoneme of the japanese path has no oto alias"
    );
    assert_eq!(
        skipped, 1,
        "that single missing alias is what skips the whole 2nd phrase"
    );
    assert_eq!(
        summary.len(),
        2,
        "japanese still groups sakura into 2 phrases"
    );
}

#[test]
fn openutau_segmentation_is_our_rule_plus_error_dropping() {
    // OpenUtau's DefaultPhonemizer emits ONE phoneme per note and marks the whole
    // phoneme Error when the note has no oto alias; RenderPhrase.FromPart then drops
    // it BEFORE grouping, so the full note slot becomes a gap. Our japanese path
    // emits two phonemes per mora (onset + vowel), so dropping only the errored
    // onset would leave a half-note gap and split at the wrong ms. To compare like
    // for like, drop EVERY phoneme of a note that has any unmapped phoneme.
    let project = pipeline::load_project(&sakura_path()).expect("load project");
    let vb = pipeline::load_voicebank(&bank_path()).expect("load bank");
    let part = match &project.parts[0] {
        UPart::Voice(p) => p,
        other => panic!("expected a voice part, got {other:?}"),
    };
    let all = pipeline::derive_phonemes(&project, part, &vb, PhonemizerKind::Japanese);
    let mut broken_notes = std::collections::HashSet::new();
    for p in &all {
        if vb.lookup(&p.phoneme, p.tone).is_none() {
            if let Some(n) = p.parent {
                broken_notes.insert(n);
            }
        }
    }
    let kept: Vec<domain::UPhoneme> = all
        .iter()
        .filter(|p| !p.parent.map(|n| broken_notes.contains(&n)).unwrap_or(false))
        .cloned()
        .collect();
    let groups = PhraseGrouping::group(&kept);
    println!(
        "reference model: {} note(s) without any oto alias -> {} phonemes dropped, {} phrases left",
        broken_notes.len(),
        all.len() - kept.len(),
        groups.len()
    );
    let reference_leading = [135.7, 70.6, 85.5];
    for (i, g) in groups.iter().enumerate() {
        println!(
            "  @{:.1} ms leading {:.1} (reference {:.1}) dur {:.1} ms ({} phonemes)",
            g.position_ms,
            g.leading_ms,
            reference_leading[i],
            g.duration_ms,
            g.phonemes.len()
        );
    }
    assert_eq!(groups.len(), 3, "reference segmentation has 3 phrases");
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
}
