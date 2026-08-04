//! Core comparison logic for the `audiocompare` CLI (Sprint 2.3).
//!
//! Loads two wav files via [`voicebank`], computes per-file statistics
//! (duration, RMS, peak, non-silence ratio) and sample-aligned difference
//! metrics, then issues a PASS/FAIL verdict against relative tolerances.
//! This is the workhorse behind the golden-test harness.

#![forbid(unsafe_code)]

use std::fmt::Write as _;

use voicebank::WavData;

/// Samples at or below this magnitude count as silence (for `nonzero_ratio`).
pub const SILENCE_EPS: f32 = 1e-6;

/// Per-file statistics over a mono sample stream.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioStats {
    /// Length of the (mono) stream in milliseconds.
    pub duration_ms: f64,
    /// Number of samples in the (mono) stream.
    pub sample_count: usize,
    /// Root-mean-square amplitude.
    pub rms: f64,
    /// Maximum absolute sample value.
    pub peak: f32,
    /// Fraction of samples with magnitude > [`SILENCE_EPS`].
    pub nonzero_ratio: f64,
}

/// Difference metrics over the region where both files overlap.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiffMetrics {
    /// Number of samples compared (min of the two lengths).
    pub aligned_samples: usize,
    /// RMS of `actual - reference` over the aligned region.
    pub rms_diff: f64,
    /// Maximum absolute sample difference over the aligned region.
    pub max_abs_diff: f32,
    /// `actual.duration_ms - reference.duration_ms`.
    pub duration_diff_ms: f64,
}

/// Relative tolerances for the PASS/FAIL verdict.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerances {
    /// Max RMS difference as a fraction of reference RMS (0.01 = 1%).
    pub rms: f64,
    /// Max |duration difference| as a fraction of reference duration (0.05 = 5%).
    pub duration: f64,
}

impl Default for Tolerances {
    fn default() -> Self {
        Self {
            rms: 0.01,
            duration: 0.05,
        }
    }
}

/// PASS/FAIL verdict with human-readable failure reasons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompareResult {
    pub pass: bool,
    /// Empty when passing; otherwise one entry per violated check.
    pub reasons: Vec<String>,
}

/// Everything the CLI needs to print: inputs, stats, metrics, verdict.
#[derive(Debug, Clone, PartialEq)]
pub struct CompareReport {
    pub actual_path: String,
    pub reference_path: String,
    pub actual_stats: AudioStats,
    pub reference_stats: AudioStats,
    pub actual_sample_rate: u32,
    pub reference_sample_rate: u32,
    pub actual_channels: u16,
    pub reference_channels: u16,
    pub difference: DiffMetrics,
    pub tolerances: Tolerances,
    pub result: CompareResult,
}

/// Compute per-file statistics over a mono sample stream.
pub fn compute_stats(samples: &[f32], sample_rate: u32) -> AudioStats {
    let n = samples.len();
    let mut sum_sq = 0.0f64;
    let mut peak = 0.0f32;
    let mut nonzero = 0usize;
    for &s in samples {
        let magnitude = s.abs();
        sum_sq += (s as f64) * (s as f64);
        peak = peak.max(magnitude);
        if magnitude > SILENCE_EPS {
            nonzero += 1;
        }
    }
    AudioStats {
        duration_ms: if sample_rate == 0 {
            0.0
        } else {
            n as f64 / sample_rate as f64 * 1000.0
        },
        sample_count: n,
        rms: if n == 0 {
            0.0
        } else {
            (sum_sq / n as f64).sqrt()
        },
        peak,
        nonzero_ratio: if n == 0 {
            0.0
        } else {
            nonzero as f64 / n as f64
        },
    }
}

/// Compare two mono streams sample-by-sample, aligned at sample 0
/// (comparison covers the shorter of the two).
pub fn compute_difference(actual: &[f32], reference: &[f32]) -> DiffMetrics {
    let n = actual.len().min(reference.len());
    let mut sum_sq = 0.0f64;
    let mut max_abs = 0.0f32;
    for (&x, &y) in actual.iter().zip(reference).take(n) {
        let d = x - y;
        sum_sq += (d as f64) * (d as f64);
        max_abs = max_abs.max(d.abs());
    }
    DiffMetrics {
        aligned_samples: n,
        rms_diff: if n == 0 {
            0.0
        } else {
            (sum_sq / n as f64).sqrt()
        },
        max_abs_diff: max_abs,
        duration_diff_ms: 0.0, // filled in by `compare`
    }
}

/// True when `rms_diff` is within `tolerance` (relative to `reference_rms`).
fn rms_within_tolerance(rms_diff: f64, reference_rms: f64, tolerance: f64) -> bool {
    if rms_diff <= 0.0 {
        return true; // identical signals
    }
    if reference_rms <= 0.0 {
        return false; // any difference against silence exceeds any relative tolerance
    }
    rms_diff / reference_rms <= tolerance
}

/// Evaluate stats/metrics against tolerances into a PASS/FAIL verdict.
pub fn evaluate(
    actual: &AudioStats,
    reference: &AudioStats,
    difference: &DiffMetrics,
    tolerances: &Tolerances,
) -> CompareResult {
    let mut reasons = Vec::new();

    let duration_diff = (actual.duration_ms - reference.duration_ms).abs();
    let duration_ok =
        reference.duration_ms > 0.0 && duration_diff / reference.duration_ms <= tolerances.duration;
    if !duration_ok {
        let relative = if reference.duration_ms > 0.0 {
            duration_diff / reference.duration_ms * 100.0
        } else {
            f64::INFINITY
        };
        reasons.push(format!(
            "duration differs by {dur_diff:.2} ms ({relative:.2}% of reference; tolerance {tol:.2}%)",
            dur_diff = duration_diff,
            tol = tolerances.duration * 100.0,
        ));
    }

    if !rms_within_tolerance(difference.rms_diff, reference.rms, tolerances.rms) {
        reasons.push(format!(
            "RMS difference {rms_diff:.6} exceeds tolerance (reference RMS {ref_rms:.6} × {tol:.4})",
            rms_diff = difference.rms_diff,
            ref_rms = reference.rms,
            tol = tolerances.rms,
        ));
    }

    CompareResult {
        pass: reasons.is_empty(),
        reasons,
    }
}

/// Full comparison of two decoded wavs (stereo files are downmixed to mono).
pub fn compare(
    actual_path: &str,
    reference_path: &str,
    actual: &WavData,
    reference: &WavData,
    tolerances: &Tolerances,
) -> CompareReport {
    let actual_mono = actual.to_mono();
    let reference_mono = reference.to_mono();
    let actual_stats = compute_stats(&actual_mono, actual.sample_rate);
    let reference_stats = compute_stats(&reference_mono, reference.sample_rate);
    let mut difference = compute_difference(&actual_mono, &reference_mono);
    difference.duration_diff_ms = actual_stats.duration_ms - reference_stats.duration_ms;
    let result = evaluate(&actual_stats, &reference_stats, &difference, tolerances);
    CompareReport {
        actual_path: actual_path.to_string(),
        reference_path: reference_path.to_string(),
        actual_stats,
        reference_stats,
        actual_sample_rate: actual.sample_rate,
        reference_sample_rate: reference.sample_rate,
        actual_channels: actual.channels,
        reference_channels: reference.channels,
        difference,
        tolerances: *tolerances,
        result,
    }
}

/// Escape a string for inclusion in a JSON string literal.
pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

impl CompareReport {
    /// Machine-readable JSON document (one line).
    pub fn to_json(&self) -> String {
        let reasons = self
            .result
            .reasons
            .iter()
            .map(|r| format!("\"{}\"", json_escape(r)))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"verdict\":\"{verdict}\",\"reasons\":[{reasons}],\"actual\":{actual},\"reference\":{reference},\"difference\":{difference},\"tolerances\":{tolerances}}}",
            verdict = if self.result.pass { "PASS" } else { "FAIL" },
            actual = self.file_json(
                &self.actual_path,
                &self.actual_stats,
                self.actual_sample_rate,
                self.actual_channels
            ),
            reference = self.file_json(
                &self.reference_path,
                &self.reference_stats,
                self.reference_sample_rate,
                self.reference_channels
            ),
            difference = self.diff_json(),
            tolerances = self.tol_json(),
        )
    }

    fn file_json(&self, path: &str, stats: &AudioStats, sample_rate: u32, channels: u16) -> String {
        format!(
            "{{\"path\":\"{escaped}\",\"sample_rate\":{sample_rate},\"channels\":{channels},\"duration_ms\":{dur},\"sample_count\":{count},\"rms\":{rms},\"peak\":{peak},\"nonzero_ratio\":{nonzero}}}",
            escaped = json_escape(path),
            dur = stats.duration_ms,
            count = stats.sample_count,
            rms = stats.rms,
            peak = stats.peak,
            nonzero = stats.nonzero_ratio,
        )
    }

    fn diff_json(&self) -> String {
        format!(
            "{{\"aligned_samples\":{aligned},\"rms_diff\":{rms_diff},\"max_abs_diff\":{max_abs},\"duration_diff_ms\":{dur_diff}}}",
            aligned = self.difference.aligned_samples,
            rms_diff = self.difference.rms_diff,
            max_abs = self.difference.max_abs_diff,
            dur_diff = self.difference.duration_diff_ms,
        )
    }

    fn tol_json(&self) -> String {
        format!(
            "{{\"rms\":{rms},\"duration\":{duration}}}",
            rms = self.tolerances.rms,
            duration = self.tolerances.duration,
        )
    }

    /// Human-readable report.
    pub fn to_human(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "comparing: {actual_path}  vs  {reference_path}",
            actual_path = self.actual_path,
            reference_path = self.reference_path,
        );
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "actual     : duration={dur:.2} ms  samples={count}  rms={rms:.6}  peak={peak:.6}  nonzero={nonzero:.1}%",
            dur = self.actual_stats.duration_ms,
            count = self.actual_stats.sample_count,
            rms = self.actual_stats.rms,
            peak = self.actual_stats.peak,
            nonzero = self.actual_stats.nonzero_ratio * 100.0,
        );
        let _ = writeln!(
            out,
            "reference  : duration={dur:.2} ms  samples={count}  rms={rms:.6}  peak={peak:.6}  nonzero={nonzero:.1}%",
            dur = self.reference_stats.duration_ms,
            count = self.reference_stats.sample_count,
            rms = self.reference_stats.rms,
            peak = self.reference_stats.peak,
            nonzero = self.reference_stats.nonzero_ratio * 100.0,
        );
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "difference : aligned={aligned} samples  rms_diff={rms_diff:.6}  max_abs_diff={max_abs:.6}  duration_diff={dur_diff:.2} ms",
            aligned = self.difference.aligned_samples,
            rms_diff = self.difference.rms_diff,
            max_abs = self.difference.max_abs_diff,
            dur_diff = self.difference.duration_diff_ms,
        );
        let _ = writeln!(
            out,
            "tolerances : rms={rms:.4} (rel. to reference rms)  duration={dur:.4} (rel. to reference duration)",
            rms = self.tolerances.rms,
            dur = self.tolerances.duration,
        );
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "verdict    : {verdict}",
            verdict = if self.result.pass { "PASS" } else { "FAIL" },
        );
        for reason in &self.result.reasons {
            let _ = writeln!(out, "             - {reason}");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use voicebank::parse_wav;

    fn assert_close(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} != {b}");
    }

    // ---------- stats ----------

    #[test]
    fn stats_basic() {
        let s = compute_stats(&[1.0, -1.0, 0.0, 0.5], 44100);
        assert_close(s.rms, 0.75); // sqrt((1+1+0+0.25)/4)
        assert_eq!(s.peak, 1.0);
        assert_close(s.nonzero_ratio, 0.75);
        assert_eq!(s.sample_count, 4);
        assert_close(s.duration_ms, 4.0 / 44100.0 * 1000.0);
    }

    #[test]
    fn stats_silence() {
        let s = compute_stats(&[0.0; 8], 44100);
        assert_eq!(s.rms, 0.0);
        assert_eq!(s.peak, 0.0);
        assert_eq!(s.nonzero_ratio, 0.0);
        assert_eq!(s.sample_count, 8);
    }

    #[test]
    fn stats_duration_scales_with_rate() {
        assert_close(compute_stats(&[0.5; 44100], 44100).duration_ms, 1000.0);
        assert_close(compute_stats(&[0.5; 44100], 22050).duration_ms, 2000.0);
    }

    #[test]
    fn stats_silence_epsilon() {
        // Samples at/below SILENCE_EPS count as silence.
        let s = compute_stats(&[SILENCE_EPS, SILENCE_EPS * 0.5, 1e-3], 44100);
        assert_close(s.nonzero_ratio, 1.0 / 3.0);
    }

    // ---------- difference ----------

    #[test]
    fn difference_identical() {
        let d = compute_difference(&[0.1, 0.2, -0.3], &[0.1, 0.2, -0.3]);
        assert_eq!(d.aligned_samples, 3);
        assert_close(d.rms_diff, 0.0);
        assert_eq!(d.max_abs_diff, 0.0);
    }

    #[test]
    fn difference_aligns_to_min_length() {
        let d = compute_difference(&[1.0, 1.0, 9.0, 9.0], &[0.0, 0.0]);
        assert_eq!(d.aligned_samples, 2);
        assert_close(d.rms_diff, 1.0);
        assert_eq!(d.max_abs_diff, 1.0);
    }

    #[test]
    fn difference_known_values() {
        let d = compute_difference(&[1.0, 0.0], &[0.0, 0.0]);
        assert_close(d.rms_diff, 0.5f64.sqrt());
        assert_eq!(d.max_abs_diff, 1.0);
    }

    // ---------- verdict / tolerances ----------

    #[test]
    fn verdict_identical_passes() {
        let a = compute_stats(&[0.5; 100], 44100);
        let r = compute_stats(&[0.5; 100], 44100);
        let d = compute_difference(&[0.5; 100], &[0.5; 100]);
        let v = evaluate(&a, &r, &d, &Tolerances::default());
        assert!(v.pass);
        assert!(v.reasons.is_empty());
    }

    #[test]
    fn verdict_silence_vs_silence_passes() {
        let a = compute_stats(&[0.0; 100], 44100);
        let r = compute_stats(&[0.0; 100], 44100);
        let d = compute_difference(&[0.0; 100], &[0.0; 100]);
        let v = evaluate(&a, &r, &d, &Tolerances::default());
        assert!(v.pass);
    }

    #[test]
    fn verdict_noise_against_silence_fails() {
        let a = compute_stats(&[0.1; 100], 44100);
        let r = compute_stats(&[0.0; 100], 44100);
        let d = compute_difference(&[0.1; 100], &[0.0; 100]);
        let v = evaluate(&a, &r, &d, &Tolerances::default());
        assert!(!v.pass);
        assert!(v.reasons.iter().any(|x| x.contains("RMS")));
    }

    #[test]
    fn verdict_rms_within_tolerance_passes() {
        let ref_s: Vec<f32> = (0..1000).map(|i| (i as f32 / 1000.0) - 0.5).collect();
        let act_s: Vec<f32> = ref_s.iter().map(|s| s * 1.005).collect();
        let a = compute_stats(&act_s, 44100);
        let r = compute_stats(&ref_s, 44100);
        let d = compute_difference(&act_s, &ref_s);
        assert!(d.rms_diff / r.rms < 0.01);
        assert!(evaluate(&a, &r, &d, &Tolerances::default()).pass);
    }

    #[test]
    fn verdict_rms_outside_tolerance_fails() {
        let ref_s: Vec<f32> = (0..1000).map(|i| (i as f32 / 1000.0) - 0.5).collect();
        let act_s: Vec<f32> = ref_s.iter().map(|s| s * 2.0).collect();
        let a = compute_stats(&act_s, 44100);
        let r = compute_stats(&ref_s, 44100);
        let d = compute_difference(&act_s, &ref_s);
        let v = evaluate(&a, &r, &d, &Tolerances::default());
        assert!(!v.pass);
        assert!(v.reasons.iter().any(|x| x.contains("RMS")));
    }

    #[test]
    fn verdict_duration_within_tolerance_passes() {
        // 1% duration difference vs 5% tolerance; signals identical on overlap.
        let a = compute_stats(&[0.5; 44100 + 441], 44100);
        let r = compute_stats(&[0.5; 44100], 44100);
        let d = compute_difference(&[0.5; 44100 + 441], &[0.5; 44100]);
        assert!(d.rms_diff < 1e-12);
        assert!(evaluate(&a, &r, &d, &Tolerances::default()).pass);
    }

    #[test]
    fn verdict_duration_mismatch_fails() {
        let a = compute_stats(&[0.5; 44100], 44100); // 1000 ms
        let r = compute_stats(&[0.5; 44100 / 2], 44100); // 500 ms
        let d = compute_difference(&[0.5; 44100 / 2], &[0.5; 44100 / 2]);
        let v = evaluate(&a, &r, &d, &Tolerances::default());
        assert!(!v.pass);
        assert!(v.reasons.iter().any(|x| x.contains("duration")));
        // 500 ms diff vs 500 ms reference = 100% of reference.
        assert!(v.reasons.iter().any(|x| x.contains("100.00%")));
    }

    #[test]
    fn verdict_zero_tolerance_passes_identical_only() {
        let tol = Tolerances {
            rms: 0.0,
            duration: 0.0,
        };
        let a = compute_stats(&[0.25; 4], 44100);
        let r = compute_stats(&[0.25; 4], 44100);
        let d = compute_difference(&[0.25; 4], &[0.25; 4]);
        assert!(evaluate(&a, &r, &d, &tol).pass);
        // 1e-6 is above f32 ULP at 0.25 (~3e-8), so the difference is real.
        let d2 = compute_difference(&[0.25; 4], &[0.25 + 1e-6; 4]);
        assert!(!evaluate(&a, &r, &d2, &tol).pass);
    }

    // ---------- end-to-end through voicebank::parse_wav ----------

    /// Minimal 16-bit PCM wav builder (mirrors voicebank's test helper).
    fn wav_bytes(pcm: &[u8], channels: u16, rate: u32, bits: u16) -> Vec<u8> {
        let data_len = pcm.len() as u32;
        let fmt_len = 16u32;
        let riff_len = 4 + (8 + fmt_len) + (8 + data_len);
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&riff_len.to_le_bytes());
        b.extend_from_slice(b"WAVE");
        b.extend_from_slice(b"fmt ");
        b.extend_from_slice(&fmt_len.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes()); // PCM
        b.extend_from_slice(&channels.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        let block_align = channels * bits / 8;
        b.extend_from_slice(&(rate * block_align as u32).to_le_bytes());
        b.extend_from_slice(&block_align.to_le_bytes());
        b.extend_from_slice(&bits.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data_len.to_le_bytes());
        b.extend_from_slice(pcm);
        b
    }

    fn pcm16(samples: &[i16]) -> Vec<u8> {
        samples.iter().flat_map(|s| s.to_le_bytes()).collect()
    }

    fn parse16(pcm: &[i16], channels: u16) -> voicebank::WavData {
        parse_wav(&wav_bytes(&pcm16(pcm), channels, 44100, 16)).unwrap()
    }

    #[test]
    fn compare_end_to_end_identical_passes() {
        let a = parse16(&[0, 8192, -8192, 16384], 1);
        let r = parse16(&[0, 8192, -8192, 16384], 1);
        let report = compare("a.wav", "ref.wav", &a, &r, &Tolerances::default());
        assert!(report.result.pass);
        assert!(report.result.reasons.is_empty());
        assert_eq!(report.actual_stats.sample_count, 4);
        assert_eq!(report.actual_sample_rate, 44100);
    }

    #[test]
    fn compare_end_to_end_different_fails() {
        let a = parse16(&[0, 8192, -8192, 16384], 1);
        let r = parse16(&[0, 0, 0, 0], 1);
        let report = compare("a.wav", "ref.wav", &a, &r, &Tolerances::default());
        assert!(!report.result.pass);
        assert!(!report.result.reasons.is_empty());
    }

    #[test]
    fn compare_stereo_downmixed_before_comparison() {
        // Stereo [L,R] pairs whose mono average equals the reference.
        let a = parse16(&[1000, 1000, -1000, -1000], 2);
        let r = parse16(&[1000, -1000], 1);
        let report = compare("stereo.wav", "mono.wav", &a, &r, &Tolerances::default());
        assert!(report.result.pass);
        assert_eq!(report.actual_stats.sample_count, 2); // downmixed
    }

    // ---------- output formatting ----------

    #[test]
    fn json_escape_quotes_and_backslashes() {
        assert_eq!(json_escape(r#"a"b\c"#), r#"a\"b\\c"#);
        assert_eq!(json_escape("line\nbreak"), "line\\nbreak");
        assert_eq!(json_escape("tab\there"), "tab\\there");
        assert_eq!(json_escape("plain"), "plain");
    }

    #[test]
    fn report_json_shape() {
        let a = parse16(&[0, 8192], 1);
        let r = parse16(&[0, 8192], 1);
        let report = compare("a\"b.wav", "ref.wav", &a, &r, &Tolerances::default());
        let j = report.to_json();
        assert!(j.contains("\"verdict\":\"PASS\""));
        assert!(j.contains("\"path\":\"a\\\"b.wav\""));
        assert!(j.contains("\"rms_diff\":0"));
        assert!(j.contains("\"duration_ms\":"));
        assert!(j.contains("\"aligned_samples\":2"));
    }

    #[test]
    fn report_json_fail_reasons() {
        let a = parse16(&[0, 8192], 1);
        let r = parse16(&[0, 0], 1);
        let report = compare("a.wav", "ref.wav", &a, &r, &Tolerances::default());
        let j = report.to_json();
        assert!(j.contains("\"verdict\":\"FAIL\""));
        assert!(j.contains("RMS difference"));
    }

    #[test]
    fn human_output_contains_verdict() {
        let a = parse16(&[0, 8192], 1);
        let r = parse16(&[0, 8192], 1);
        let report = compare("a.wav", "ref.wav", &a, &r, &Tolerances::default());
        let h = report.to_human();
        assert!(h.contains("verdict    : PASS"));
        assert!(h.contains("comparing: a.wav  vs  ref.wav"));
    }
}
