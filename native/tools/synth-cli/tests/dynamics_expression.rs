//! The `dyn` expression must be AUDIBLE.
//!
//! `dyn` is the one descriptor the engine does not apply itself:
//! `PhraseSynthRs::set_curves` takes f0 / gender / tension / breathiness /
//! voicing, and `render_input::Curves::dynamics` was only carried through the
//! chunk hash — `runtime::mixer::apply_dynamics` existed but had no call site.
//! So a volume expression saved to the document (and accepted by
//! `curve.apply_stroke`) changed nothing audible.
//!
//! Each test drives it the way a session would: put a curve on the part in the
//! document, re-plan, render, mix.
//!
//! Reference semantics kept in mind: like `Renderers.ApplyDynamics`, samples
//! AFTER the curve's last segment are left untouched, so the render's tail past
//! the note keeps its gain. The comparison therefore covers the span the curve
//! was written over.
//!
//! ```sh
//! WORLDLINE_SO=.../native/build/build-linux/libworldline.so \
//!     cargo test -p synth-cli --test dynamics_expression -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};

use domain::{UCurve, UPart, UProject};
use runtime::mixer::{mix, AudioChunk, MixInput, TrackSpec, SAMPLE_RATE};
use synth_cli::pipeline::{self, PhonemizerKind};
use voicebank::Voicebank;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn one_note_project(root: &Path) -> UProject {
    pipeline::load_project(&root.join("test/golden/one-note.ustx")).expect("load one-note.ustx")
}

fn voicebank(root: &Path) -> Voicebank {
    pipeline::load_voicebank(&root.join("test/golden/teto-english/library")).expect("load bank")
}

fn renderer(root: &Path) -> Option<worldline_plugin::WorldlineRenderer> {
    let so = std::env::var("WORLDLINE_SO")
        .map(PathBuf::from)
        .unwrap_or_else(|_| root.join("native/build/build-linux/libworldline.so"));
    if !so.is_file() {
        eprintln!("libworldline.so not found at {}; skipping", so.display());
        return None;
    }
    worldline_plugin::WorldlineRenderer::open(&so).ok()
}

/// Puts a flat `dyn` curve on every voice part, in descriptor units (dB).
fn set_flat_dyn(project: &mut UProject, db: i32) {
    let last = project
        .parts
        .iter()
        .filter_map(|part| match part {
            UPart::Voice(voice) => voice.notes.last().map(|note| note.position + note.duration),
            _ => None,
        })
        .max()
        .unwrap_or(480);
    // The span runs well past the last note: `sample_curve` falls back to the
    // descriptor default OUTSIDE a curve's own range, and the pitch grid
    // extends past `last_phoneme_end`, so a curve ending at the note would
    // hand the tail back its default gain.
    let end = last + 480 * 100;
    for part in &mut project.parts {
        if let UPart::Voice(voice) = part {
            voice.curves.retain(|curve| curve.abbr != "dyn");
            voice.curves.push(UCurve {
                xs: vec![0, end],
                ys: vec![db, db],
                abbr: "dyn".to_string(),
            });
        }
    }
}

/// Plan + render + mix the whole track the way a host does it, and report how
/// many samples the expression grid actually covers.
///
/// `phrase::builder` samples every expression on the PITCH grid
/// (`pitch_start() .. last_phoneme_end`, `feed/src/pitch.rs`), and
/// `apply_dynamics` — like `Renderers.ApplyDynamics` — leaves samples past the
/// curve's last segment untouched. So the render's tail after the last note
/// keeps its gain; the covered span is what the assertions compare.
fn render(
    project: &UProject,
    bank: &Voicebank,
    renderer: &worldline_plugin::WorldlineRenderer,
) -> (Vec<f32>, usize) {
    let (phrases, planned, _) =
        pipeline::plan_track_chunks(project, bank, 0, PhonemizerKind::Universal).expect("plan");
    assert!(!planned.is_empty(), "fixture must plan at least one chunk");
    let inputs: Vec<MixInput> = planned
        .iter()
        .map(|chunk| MixInput {
            chunk: AudioChunk {
                samples: pipeline::render_planned_chunk(&phrases, chunk, renderer).expect("chunk"),
                position_ms: chunk.position_ms,
                leading_ms: chunk.leading_ms,
                hash: 0,
            },
            track: TrackSpec::default(),
        })
        .collect();
    let samples = mix(&inputs).samples;
    let length = samples.len();
    // Single-chunk fixture: the grid's frame count times its interval is the
    // span the curve was sampled over.
    let frames = phrases
        .iter()
        .map(|p| p.input.curves.dynamics.len())
        .max()
        .unwrap_or(0);
    let interval_ms = worldline_plugin::ms_per_interval(&phrases[0].input);
    let covered = ((frames as f64 * interval_ms / 1000.0) * f64::from(SAMPLE_RATE)) as usize;
    (samples, covered.min(length))
}

/// `10^(x/20)` — the same dB→linear conversion `phrase::builder` applies,
/// which scales the descriptor value by 0.1 first: the `dyn` descriptor is in
/// tenths of a dB (`-240` = −24 dB, `+120` = +12 dB).
fn linear(descriptor_value: f64) -> f32 {
    (10f64.powf(descriptor_value * 0.1 / 20.0)) as f32
}

/// A flat `dyn` curve must scale the rendered audio over the span it covers.
#[test]
#[ignore = "needs libworldline.so (run with --ignored)"]
fn a_flat_dynamics_curve_scales_the_rendered_audio() {
    let root = repo_root();
    let Some(renderer) = renderer(&root) else {
        return;
    };
    let bank = voicebank(&root);
    let plain = one_note_project(&root);
    let (baseline, covered) = render(&plain, &bank, &renderer);
    assert!(
        baseline.iter().any(|s| s.abs() > 1e-4),
        "fixture must actually produce audio"
    );

    // 0 dB is the descriptor default, so this must equal the untouched render.
    let mut unity = one_note_project(&root);
    set_flat_dyn(&mut unity, 0);
    let (unity_samples, _) = render(&unity, &bank, &renderer);
    assert_eq!(
        baseline, unity_samples,
        "an explicit 0 dB curve must not change a single sample (golden parity)"
    );

    // -120 in descriptor units is -12 dB.
    let mut quiet = one_note_project(&root);
    set_flat_dyn(&mut quiet, -120);
    let (quiet_samples, _) = render(&quiet, &bank, &renderer);
    assert_eq!(
        baseline.len(),
        quiet_samples.len(),
        "the gain must not change the buffer length"
    );
    assert_ne!(
        baseline, quiet_samples,
        "before the fix the dyn curve was hashed but never applied, so both renders were identical"
    );

    // The expression grid is sampled over the notes (pitch grid = pitch_start ..
    // last_phoneme_end), and `apply_dynamics` — like `Renderers.ApplyDynamics` —
    // leaves everything past the curve's last segment untouched. So the SUNG
    // span scales and the renderer's own tail keeps its gain. 80% of the buffer
    // is safely inside the covered span on this fixture; the tail is asserted
    // separately so both halves of the behaviour are pinned.
    let inside = baseline.len() * 4 / 5;
    let expected = linear(-120.0);
    let mut worst = 0.0f32;
    let mut checked = 0usize;
    for (a, b) in baseline[..inside]
        .iter()
        .zip(quiet_samples[..inside].iter())
    {
        if a.abs() < 1e-3 {
            continue;
        }
        worst = worst.max((b / a - expected).abs());
        checked += 1;
    }
    assert!(
        checked > 1000,
        "too few audible samples to judge ({checked})"
    );
    assert!(
        worst < 0.02,
        "a flat -12 dB curve must scale the sung span by {expected}; worst deviation {worst}"
    );

    // Nothing may get louder, and the render tail must be untouched.
    for (a, b) in baseline.iter().zip(quiet_samples.iter()) {
        assert!(
            b.abs() <= a.abs() + 1e-6,
            "a quieter curve must never increase a sample"
        );
    }
    // Measured on this fixture the grid covers ~91.5% of the buffer, so 95% is
    // safely past it.
    let past_grid = baseline.len() * 95 / 100;
    assert_eq!(
        &baseline[past_grid..],
        &quiet_samples[past_grid..],
        "past the expression grid the curve must not touch the render"
    );
}

/// Without a volume expression the render is unchanged — the regression guard
/// for every golden WAV that has no `dyn` curve.
#[test]
#[ignore = "needs libworldline.so (run with --ignored)"]
fn an_explicit_default_curve_matches_the_curve_less_render() {
    let root = repo_root();
    let Some(renderer) = renderer(&root) else {
        return;
    };
    let bank = voicebank(&root);
    let plain = one_note_project(&root);
    let (before, _) = render(&plain, &bank, &renderer);

    let mut with_curve = one_note_project(&root);
    set_flat_dyn(&mut with_curve, 0);
    let (after, _) = render(&with_curve, &bank, &renderer);
    assert_eq!(before, after);
}
