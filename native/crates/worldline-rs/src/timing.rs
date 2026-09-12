//! Port of `worldline/classic/timing.cpp` — the per-phoneme time mapping
//! that turns an oto region into the resampled model frames.

use crate::model::Model;
use worldline_plugin::PhonemeRequest;

/// Padding frames added on both ends of the time mapping (`phrase_synth.cpp`).
pub const PADDING: usize = 2;

/// `GetInTotalMs(model, request)`.
pub fn get_in_total_ms(model: &Model, request: &PhonemeRequest) -> f64 {
    if request.cut_off < 0.0 {
        -request.cut_off
    } else {
        model.total_ms() - request.offset - request.cut_off
    }
}

/// `GetTimeMapping(model, request)`.
pub fn get_time_mapping(model: &Model, request: &PhonemeRequest) -> Vec<f64> {
    let mut mapping: Vec<f64> = Vec::new();
    let frame_ms = model.frame_ms();
    let con_speed = 0.5f64.powf(1.0 - request.con_vel / 100.0);
    let in_total_ms = get_in_total_ms(model, request);
    let in_con_ms = 1.0f64.max(request.consonant);
    let in_vow_ms = 1.0f64.max(in_total_ms - in_con_ms);
    let out_total_ms = request.required_length;
    let out_con_ms = in_con_ms / con_speed;
    let out_vow_ms = (out_total_ms - out_con_ms).max(0.0) + frame_ms;
    let vow_speed = (in_vow_ms / out_vow_ms).max(0.0).min(1.0);

    let mut pos_ms = request.offset;
    while mapping.len() as f64 * frame_ms <= out_con_ms {
        mapping.push(pos_ms);
        pos_ms += con_speed * frame_ms;
    }
    while mapping.len() as f64 * frame_ms <= out_total_ms {
        mapping.push(pos_ms);
        pos_ms += vow_speed * frame_ms;
    }
    mapping
}

/// `ShiftTimeMapping`.
pub fn shift_time_mapping(mapping: &mut [f64], shift: f64) {
    for value in mapping.iter_mut() {
        *value += shift;
    }
}

/// `PadTimeMapping` — `frames` copies of the first/last entry on each end.
pub fn pad_time_mapping(mapping: &mut Vec<f64>, frames: usize) {
    if mapping.is_empty() {
        return;
    }
    let first = mapping[0];
    let last = *mapping.last().expect("non-empty");
    for _ in 0..frames {
        mapping.insert(0, first);
    }
    for _ in 0..frames {
        mapping.push(last);
    }
}

/// `ModelTiming` (phrase_synth.h).
#[derive(Debug, Clone, Copy, Default)]
pub struct ModelTiming {
    pub left_extra: usize,
    pub skip: i32,
    pub p0: i32,
    pub p1: i32,
    pub p3: i32,
    pub p4: i32,
}

/// Build the C++ `ModelTiming` from the request and frame size.
pub fn model_timing(request: &PhonemeRequest, frame_ms: f64, padding: usize) -> ModelTiming {
    let mut timing = ModelTiming {
        left_extra: padding,
        skip: (request.skip_ms / frame_ms).round() as i32,
        p0: (request.pos_ms / frame_ms).round() as i32,
        p1: ((request.pos_ms + request.fade_in_ms) / frame_ms).round() as i32,
        p3: ((request.pos_ms + request.length_ms - request.fade_out_ms) / frame_ms).round() as i32,
        p4: ((request.pos_ms + request.length_ms) / frame_ms).round() as i32,
    };
    timing.p0 = timing.p0.max(0);
    timing.p1 = (timing.p0 + 1).max(timing.p1);
    timing.p3 = (timing.p4 - 1).min(timing.p3);
    timing
}
