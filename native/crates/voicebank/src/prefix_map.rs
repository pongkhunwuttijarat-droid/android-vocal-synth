//! `prefix.map` parsing.
//!
//! Format (tab-separated, one mapping per line):
//!
//! ```text
//! <tone name>\t<prefix>\t<suffix>
//! C3\tP_\t_S
//! C4\tP_\t_S
//! ```
//!
//! Lines are grouped by (prefix, suffix); each group becomes a
//! [`Subbank`](crate::config::Subbank) whose tone ranges are the contiguous
//! runs of covered tones (C1..=C8, mirroring OpenUtau's `LoadMap`).
//!
//! A root `prefix.map` yields subbanks with empty color. A `prefix/*.map`
//! file yields subbanks whose color is the file stem and whose suffix is
//! prefixed with the color (OpenUtau's presamp convention).

use std::collections::{BTreeMap, BTreeSet};

use crate::config::Subbank;
use crate::text;
use crate::tone;

/// Parse a prefix.map. `color` is `""` for the root prefix.map, or the file
/// stem for `prefix/<name>.map`. `declared` is the voicebank text encoding.
pub fn parse_prefix_map(bytes: &[u8], color: &str, declared: Option<&str>) -> Vec<Subbank> {
    // BTreeMap keeps deterministic (prefix, suffix) order.
    let mut groups: BTreeMap<(String, String), BTreeSet<i32>> = BTreeMap::new();
    for line in text::decode_lines(bytes, declared) {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() != 3 {
            continue;
        }
        let Some(tone) = tone::name_to_tone(parts[0]) else {
            continue;
        };
        groups
            .entry((parts[1].to_string(), parts[2].to_string()))
            .or_default()
            .insert(tone);
    }
    groups
        .into_iter()
        .map(|((prefix, suffix), tones)| {
            let tone_ranges = build_tone_ranges(&tones);
            Subbank {
                color: color.to_string(),
                prefix,
                suffix: format!("{color}{suffix}"),
                tone_ranges,
                tones,
            }
        })
        .collect()
}

/// Convert a set of tones to contiguous range strings over C1..=C8
/// (note 24..=108), exactly like OpenUtau's `LoadMap` loop.
fn build_tone_ranges(tones: &std::collections::BTreeSet<i32>) -> Vec<String> {
    let mut ranges = Vec::new();
    let mut range_start: Option<i32> = None;
    let mut range_end: Option<i32> = None;
    for i in 24..=108 {
        if tones.contains(&i) && i < 108 {
            if range_start.is_none() {
                range_start = Some(i);
            } else {
                range_end = Some(i);
            }
        } else if let Some(start) = range_start {
            match range_end {
                Some(end) => ranges.push(format!(
                    "{}-{}",
                    tone::tone_to_name(start),
                    tone::tone_to_name(end)
                )),
                None => ranges.push(tone::tone_to_name(start)),
            }
            range_start = None;
            range_end = None;
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_empty_map() {
        assert!(parse_prefix_map(b"", "", None).is_empty());
        assert!(parse_prefix_map(b"\r\n# comment\n", "", None).is_empty());
    }

    #[test]
    fn groups_and_ranges() {
        // C3 and C4 are NOT contiguous (C#3..B3 gap) -> separate ranges;
        // C#3 merges with C3 into one range.
        let map = b"C3\tP_\t_S\nC4\tP_\t_S\nC#3\tP_\t_S\nD4\tQ_\t_S\nC5\tP_\t_T\n";
        let subs = parse_prefix_map(map, "", None);
        assert_eq!(subs.len(), 3);
        let p_s = subs.iter().find(|s| s.prefix == "P_" && s.suffix == "_S").unwrap();
        assert_eq!(p_s.color, "");
        assert_eq!(p_s.tone_ranges, vec!["C3-C#3", "C4"]);
        assert!(p_s.contains_tone(48) && p_s.contains_tone(49) && !p_s.contains_tone(50));
        let q = subs.iter().find(|s| s.prefix == "Q_").unwrap();
        assert_eq!(q.tone_ranges, vec!["D4"]);
        assert!(q.contains_tone(62));
    }

    #[test]
    fn split_ranges_and_color_suffix() {
        let map = b"C3\tP_\t_S\nC5\tP_\t_S\nC#5\tP_\t_S\nD5\tP_\t_S\n";
        let subs = parse_prefix_map(map, "power", None);
        assert_eq!(subs.len(), 1);
        let sb = &subs[0];
        assert_eq!(sb.color, "power");
        assert_eq!(sb.suffix, "power_S");
        assert_eq!(sb.tone_ranges, vec!["C3", "C5-D5"]);
    }

    #[test]
    fn garbage_lines_skipped() {
        let map = b"not a map\nC4\tP_\t_S\nX9\tA\tB\n";
        let subs = parse_prefix_map(map, "", None);
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].tone_ranges, vec!["C4"]);
    }

    #[test]
    fn ignores_extra_fields() {
        let map = b"C4\tP_\t_S\textra\n";
        assert!(parse_prefix_map(map, "", None).is_empty());
    }
}
