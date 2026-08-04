//! TimeAxis tests. Acceptance: at 120 BPM 4/4, one quarter note
//! (480 ticks) equals 500 ms.

use domain::{UTempo, UTimeSignature, UProject};

fn axis_120() -> UProject {
    UProject::new()
}

#[test]
fn quarter_note_is_500ms_at_120bpm() {
    let p = axis_120();
    let axis = &p.time_axis;
    assert!((axis.tick_to_ms(480.0) - 500.0).abs() < 1e-9);
    assert!((axis.tick_to_ms(960.0) - 1000.0).abs() < 1e-9);
    assert_eq!(axis.tick_to_ms(0.0), 0.0);
    assert_eq!(axis.ms_to_tick(500.0), 480);
    assert_eq!(axis.ms_to_tick(0.0), 0);
    assert_eq!(axis.ms_to_tick(1000.0), 960);
    assert_eq!(axis.ms_to_tick(499.0), 479);
    assert_eq!(axis.ms_to_tick(501.0), 481);
    assert_eq!(axis.ms_to_non_exact_tick(500.0), 480.0);
    assert_eq!(axis.bpm_at_tick(0), 120.0);
    assert_eq!(axis.bpm_at_tick(100_000), 120.0);
}

#[test]
fn bar_beat_conversion_44() {
    // OpenUtau semantics: bar is 0-indexed (barPos=0), 4/4 at 120 BPM
    // → ticksPerBar = 4 beats × 480 ticks = 1920.
    let p = axis_120();
    let axis = &p.time_axis;
    assert_eq!(axis.tick_to_bar_beat(0), (0, 0, 0));
    assert_eq!(axis.tick_to_bar_beat(479), (0, 0, 479));
    assert_eq!(axis.tick_to_bar_beat(480), (0, 1, 0));
    assert_eq!(axis.tick_to_bar_beat(960), (0, 2, 0));
    assert_eq!(axis.tick_to_bar_beat(1920), (1, 0, 0));
    assert_eq!(axis.tick_to_bar_beat(2400), (1, 1, 0));
    assert_eq!(axis.tick_to_bar_beat(1920 + 480 + 240), (1, 1, 240));
    assert_eq!(axis.bar_beat_to_tick(0, 0), 0);
    assert_eq!(axis.bar_beat_to_tick(1, 0), 1920);
    assert_eq!(axis.bar_beat_to_tick(4, 0), 7680);
    assert_eq!(axis.bar_beat_to_tick(4, 2), 7680 + 960);
    assert_eq!(axis.next_bar_beat(0, 2), (0, 3));
    assert_eq!(axis.next_bar_beat(0, 3), (1, 0));
    assert_eq!(axis.next_bar_beat(4, 3), (5, 0));

    let ts = axis.time_signature_at_tick(0);
    assert_eq!((ts.bar_position, ts.beat_per_bar, ts.beat_unit), (0, 4, 4));
    assert_eq!(axis.time_signature_at_bar(10), ts);

    assert_eq!(axis.tempos_between_ticks(0, 100), vec![UTempo::new(0, 120.0)]);
    assert_eq!(axis.tempos_between_ticks(100, 5000), vec![UTempo::new(0, 120.0)]);
    // OpenUtau filter: start < tickEnd && tickPos < end → zero-length range
    // at 480 still matches the single tempo (0 < 480, 480 < maxValue).
    assert_eq!(axis.tempos_between_ticks(480, 480), vec![UTempo::new(0, 120.0)]);
}

#[test]
fn ms_helpers() {
    let p = axis_120();
    let axis = &p.time_axis;
    // 500 ms after tick 0 is 480 ticks; 500 ms before tick 480 is -480.
    assert_eq!(axis.ms_to_tick_at(500.0, 0), 480);
    assert_eq!(axis.ms_to_tick_at(-500.0, 480), -480);
    assert_eq!(axis.ticks_between_ms(0.0, 500.0), 480);
    assert!((axis.ms_between_ticks(0.0, 480.0) - 500.0).abs() < 1e-9);
}

#[test]
fn tempo_change_at_960() {
    let mut p = axis_120();
    p.tempos = vec![UTempo::new(0, 120.0), UTempo::new(960, 240.0)];
    p.validate().unwrap();
    let axis = &p.time_axis;
    assert_eq!(axis.bpm_at_tick(959), 120.0);
    assert_eq!(axis.bpm_at_tick(960), 240.0);
    assert_eq!(axis.bpm_at_tick(100_000), 240.0);
    // 960 ticks at 120 bpm = 1000 ms; then 480 ticks at 240 bpm = 250 ms.
    assert!((axis.tick_to_ms(960.0) - 1000.0).abs() < 1e-9);
    assert!((axis.tick_to_ms(1440.0) - 1250.0).abs() < 1e-9);
    assert_eq!(axis.ms_to_tick(1000.0), 960);
    assert_eq!(axis.ms_to_tick(1250.0), 1440);
    // Segment boundary tempo list.
    assert_eq!(
        axis.tempos_between_ticks(0, 2000),
        vec![UTempo::new(0, 120.0), UTempo::new(960, 240.0)]
    );
}

#[test]
fn time_signature_change_at_bar_4() {
    let mut p = axis_120();
    p.time_signatures = vec![
        UTimeSignature::new(0, 4, 4),
        UTimeSignature::new(4, 3, 4),
        UTimeSignature::new(8, 6, 8),
    ];
    p.validate().unwrap();
    let axis = &p.time_axis;

    // Bar 4 starts at 4 bars x 1920 ticks = 7680.
    assert_eq!(axis.bar_beat_to_tick(4, 0), 7680);
    assert_eq!(axis.tick_to_bar_beat(7680), (4, 0, 0));
    // 3/4: 480*4*3/4 = 1440 ticks per bar.
    assert_eq!(axis.bar_beat_to_tick(5, 0), 7680 + 1440);
    assert_eq!(axis.tick_to_bar_beat(7680 + 1440), (5, 0, 0));
    assert_eq!(axis.tick_to_bar_beat(7680 + 480), (4, 1, 0));
    assert_eq!(axis.next_bar_beat(4, 2), (5, 0));
    let ts = axis.time_signature_at_tick(7680);
    assert_eq!((ts.beat_per_bar, ts.beat_unit), (3, 4));
    let ts = axis.time_signature_at_tick(7680 + 1440);
    assert_eq!((ts.beat_per_bar, ts.beat_unit), (3, 4));
    // 6/8 at bar 8: ticks per bar = 480*4*6/8 = 1440.
    let bar8 = 7680 + 4 * 1440;
    assert_eq!(axis.bar_beat_to_tick(8, 0), bar8);
    assert_eq!(axis.tick_to_bar_beat(bar8), (8, 0, 0));
    let ts = axis.time_signature_at_bar(8);
    assert_eq!((ts.beat_per_bar, ts.beat_unit), (6, 8));
    // 6/8: ticks per beat = 480*4/8 = 240.
    assert_eq!(axis.tick_to_bar_beat(bar8 + 240), (8, 1, 0));
    // Tempo still 120 throughout (tempos unchanged).
    assert_eq!(axis.bpm_at_tick(bar8), 120.0);
}

#[test]
fn unsorted_lists_are_sorted_by_validate() {
    let mut p = axis_120();
    p.tempos = vec![UTempo::new(960, 240.0), UTempo::new(0, 120.0)];
    p.time_signatures = vec![UTimeSignature::new(4, 3, 4), UTimeSignature::new(0, 4, 4)];
    p.validate().unwrap();
    assert_eq!(p.time_axis.bpm_at_tick(500), 120.0);
    assert_eq!(p.time_axis.bpm_at_tick(1000), 240.0);
    assert_eq!(p.time_axis.tick_to_bar_beat(7680), (4, 0, 0));
}

#[test]
fn zero_bpm_tempo_inherits_previous() {
    let mut p = axis_120();
    p.tempos = vec![UTempo::new(0, 120.0), UTempo::new(480, 0.0)];
    p.validate().unwrap();
    assert_eq!(p.time_axis.bpm_at_tick(1000), 120.0);
}
