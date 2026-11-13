//! DiffSinger timing utilities — port of `DiffSingerUtils.cs` (C#):
//! head/tail frame padding, phone durations → frames, duration fitting.
//! Constants match the C#: `headFrames = 8`, `tailFrames = 8`.

pub const HEAD_FRAMES: usize = 8;
pub const TAIL_FRAMES: usize = 8;

/// `Math.Round(x, MidpointRounding.ToEven)` — .NET's banker's rounding,
/// NOT Rust's `f64::round()` (half-away-from-zero). DiffSinger durations
/// depend on this exact rounding (C# `DurationsMsToFrames`).
fn round_half_even(x: f64) -> i64 {
    let floor = x.floor();
    let frac = x - floor;
    if frac > 0.5 {
        (floor + 1.0) as i64
    } else if frac < 0.5 {
        floor as i64
    } else {
        let f = floor as i64;
        if f % 2 == 0 {
            f
        } else {
            f + 1
        }
    }
}

/// `DiffSingerUtils.DurationsMsToFrames` — convert a list of durations
/// (ms) into per-phone frame counts. Frames accumulate with
/// `Round(acc/frameMs + 0.5, ToEven)` — banker's rounding (see
/// [round_half_even]), matching the C# bit for bit.
pub fn durations_ms_to_frames(durations_ms: &[f64], frame_ms: f64) -> Result<Vec<i64>, String> {
    let mut result = Vec::with_capacity(durations_ms.len());
    let mut accumulated_ms = 0.0f64;
    let mut previous_frame = 0i64;
    for &duration_ms in durations_ms {
        if duration_ms < 0.0 {
            return Err(format!("Negative DiffSinger duration: {duration_ms} ms."));
        }
        accumulated_ms += duration_ms;
        let frame = round_half_even(accumulated_ms / frame_ms + 0.5);
        result.push(frame - previous_frame);
        previous_frame = frame;
    }
    Ok(result)
}

/// `DiffSingerUtils.PaddedPhoneDurations` — phone durations (ms) padded
/// with the head/tail frame paddings, converted to frame counts. The
/// result length = phones.len() + 2 (matching the padded tokens).
pub fn padded_phone_durations(
    phone_durations_ms: &[f64],
    frame_ms: f64,
    head_frames: usize,
    tail_frames: usize,
) -> Result<Vec<i64>, String> {
    let mut padded = Vec::with_capacity(phone_durations_ms.len() + 2);
    padded.push(head_frames as f64 * frame_ms);
    padded.extend_from_slice(phone_durations_ms);
    padded.push(tail_frames as f64 * frame_ms);
    durations_ms_to_frames(&padded, frame_ms)
}

/// `DiffSingerUtils.FitDurationSum` — adjust the LAST duration so the sum
/// matches `total_frames`; when the last goes negative, drain backwards
/// (C#:124-145).
pub fn fit_duration_sum(mut durations: Vec<i64>, total_frames: i64) -> Result<Vec<i64>, String> {
    if durations.is_empty() {
        return Ok(durations);
    }
    let sum: i64 = durations.iter().sum();
    let mut delta = total_frames - sum;
    let last = durations.len() - 1;
    durations[last] += delta;
    if durations[last] < 0 {
        let mut deficit = -durations[last];
        durations[last] = 0;
        let mut i = last as isize - 1;
        while i >= 0 && deficit > 0 {
            let take = durations[i as usize].min(deficit);
            durations[i as usize] -= take;
            deficit -= take;
            i -= 1;
        }
        if deficit > 0 {
            return Err(format!(
                "Cannot fit DiffSinger durations to {total_frames} frames."
            ));
        }
    }
    Ok(durations)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_to_frames_rounds_half_even() {
        // C# semantics: Round(x/frameMs + 0.5, ToEven) — banker's.
        let d = vec![100.0, 100.0, 100.0];
        let frames = durations_ms_to_frames(&d, 11.61).unwrap();
        // acc 100/11.61+0.5 = 9.11 -> 9; acc 17.73 -> 18 (delta 9);
        // acc 26.34 -> 26 (delta 8)
        assert_eq!(frames, vec![9, 9, 8]);
    }

    #[test]
    fn padded_durations_length() {
        let d = vec![500.0, 500.0];
        let frames = padded_phone_durations(&d, 11.61, 8, 8).unwrap();
        assert_eq!(frames.len(), 4); // head + 2 phones + tail
                                     // 8 * 11.61 = 92.88ms -> 92.88/11.61 + 0.5 = 8.5 -> half-even -> 8
        assert_eq!(frames[0], 8);
    }

    #[test]
    fn fit_sum_adjusts_last() {
        let d = vec![10, 10, 10];
        let fitted = fit_duration_sum(d, 35).unwrap();
        assert_eq!(fitted, vec![10, 10, 15]);
        assert_eq!(fitted.iter().sum::<i64>(), 35);
    }

    #[test]
    fn fit_sum_drains_backwards() {
        let d = vec![10, 10, 2];
        let fitted = fit_duration_sum(d, 12).unwrap();
        assert_eq!(fitted.iter().sum::<i64>(), 12);
        assert!(fitted.iter().all(|&x| x >= 0));
    }
}
