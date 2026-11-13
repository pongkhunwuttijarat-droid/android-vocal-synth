//! The mix stage — not the render stage — owns track volume/pan.
//!
//! `Chunk::hash` (`synth-cli/chunking.rs`) deliberately excludes volume/pan/FX,
//! so moving a fader must re-mix cached PCM and never re-render. The WASM
//! `Mixer` wrapper is where the browser hands those values to
//! `runtime::mixer::mix`, and it used to push `TrackSpec::default()` for every
//! chunk — silently dropping the whole mixer from the browser mixdown. These
//! tests are the regression guard for that.

use engine_wasm::Mixer;

const SAMPLE_RATE: usize = 44100;

fn ramp(n: usize, phase: f32) -> Vec<f32> {
    (0..n).map(|i| ((i as f32) * 0.01 + phase).sin()).collect()
}

fn ms(v: f64) -> usize {
    (v * SAMPLE_RATE as f64 / 1000.0) as usize
}

/// A full-scale chunk with a fader yields exactly the fader gain. `add_chunk`
/// stays the unity/centre convenience so nothing already using it changes.
#[test]
fn add_chunk_track_applies_volume_and_pan() {
    let unity_samples = vec![1.0f32; 1000];
    let mut unity = Mixer::new();
    unity.add_chunk(&unity_samples, 0.0, 0.0);
    assert!(
        unity.finish().iter().all(|s| (*s - 1.0).abs() < 1e-6),
        "0 dB / centre must stay unity"
    );

    let mut quiet = Mixer::new();
    // -6.0206 dB ≈ 0.5 linear.
    quiet.add_chunk_track(&unity_samples, 0.0, 0.0, -6.0206, 0.0);
    assert!(
        quiet.finish().iter().all(|s| (*s - 0.5).abs() < 1e-3),
        "volume_db must reach the mix"
    );

    let mut panned = Mixer::new();
    // Hard left folds down by cos(π/4) ≈ 0.7071 in the mono mix.
    panned.add_chunk_track(&unity_samples, 0.0, 0.0, 0.0, -1.0);
    assert!(
        panned
            .finish()
            .iter()
            .all(|s| (*s - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3),
        "pan must reach the mix"
    );
}

/// `finish_from` is the streaming entry point: it MUST stay bit-identical to
/// the tail of `finish()`, track parameters included. Volume/pan live in the
/// mix stage, so a range mixed from a chunk boundary already carries them —
/// which is why a fader change can re-mix without re-rendering.
#[test]
fn finish_from_is_the_tail_of_a_full_mix_with_track_params() {
    let plan = [(0.0f64, 0.0f64), (30.0, 20.0), (60.0, 20.0), (90.0, 0.0)];
    let mut mixer = Mixer::new();
    mixer.add_chunk(&ramp(2000, 0.0), plan[0].0, plan[0].1);
    mixer.add_chunk_track(&ramp(2000, 1.0), plan[1].0, plan[1].1, -3.0, 0.0);
    mixer.add_chunk_track(&ramp(2000, 2.0), plan[2].0, plan[2].1, 2.0, 0.2);
    mixer.add_chunk_track(&ramp(1500, 3.0), plan[3].0, plan[3].1, 0.0, -0.5);

    let full = mixer.finish();
    assert_eq!(mixer.chunk_count(), plan.len());
    for (index, (position, leading)) in plan.iter().enumerate().skip(1) {
        let tail = mixer.finish_from(index);
        let origin = ms(position - leading);
        let expected = &full[origin..];
        assert_eq!(
            tail.len(),
            expected.len(),
            "finish_from({index}) length differs from the full-mix tail"
        );
        assert!(
            tail.iter()
                .zip(expected)
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "finish_from({index}) must be BIT-identical to the full-mix tail"
        );
    }
}
