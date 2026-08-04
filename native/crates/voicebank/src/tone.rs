//! Tone (note number) <-> tone name conversion, mirroring OpenUtau's
//! `MusicMath.NameToTone` / `MusicMath.GetToneName`.
//!
//! Note numbers use the UTAU/OpenUtau convention: C4 = 60, A4 = 69.

/// Pitch class name -> index within the octave (C = 0 ... B = 11).
/// Both sharp and flat spellings map to the same index, like OpenUtau's
/// `NameInOctave` dictionary.
const NAME_TO_TONE: &[(&str, i32)] = &[
    ("C", 0),
    ("C#", 1),
    ("Db", 1),
    ("D", 2),
    ("D#", 3),
    ("Eb", 3),
    ("E", 4),
    ("F", 5),
    ("F#", 6),
    ("Gb", 6),
    ("G", 7),
    ("G#", 8),
    ("Ab", 8),
    ("A", 9),
    ("A#", 10),
    ("Bb", 10),
    ("B", 11),
];

const OCTAVE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// Parse a tone name such as `"C4"`, `"C#4"` or `"Bb3"` into a note number
/// (C4 = 60). Returns `None` for unparseable input.
pub fn name_to_tone(name: &str) -> Option<i32> {
    let name = name.trim();
    if name.len() < 2 || !name.is_ascii() {
        return None;
    }
    let bytes = name.as_bytes();
    let (pitch, octave_str) = if bytes[1] == b'#' || bytes[1] == b'b' {
        (&name[..2], &name[2..])
    } else {
        (&name[..1], &name[1..])
    };
    let octave: i32 = octave_str.trim().parse().ok()?;
    let in_octave = NAME_TO_TONE
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(pitch))?
        .1;
    Some(12 * (octave + 1) + in_octave)
}

/// Format a note number as a tone name (`60` -> `"C4"`). Returns an empty
/// string for negative note numbers, mirroring `GetToneName`.
pub fn tone_to_name(note: i32) -> String {
    if note < 0 {
        return String::new();
    }
    format!("{}{}", OCTAVE_NAMES[(note % 12) as usize], note / 12 - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        assert_eq!(name_to_tone("C4"), Some(60));
        assert_eq!(name_to_tone("C#4"), Some(61));
        assert_eq!(name_to_tone("Db4"), Some(61));
        assert_eq!(name_to_tone("Bb3"), Some(58));
        assert_eq!(name_to_tone("B3"), Some(59));
        assert_eq!(name_to_tone("C1"), Some(24));
        assert_eq!(name_to_tone("C8"), Some(108));
        assert_eq!(tone_to_name(60), "C4");
        assert_eq!(tone_to_name(61), "C#4");
        // Names always render with sharps (OpenUtau behavior).
        assert_eq!(tone_to_name(58), "A#3");
        assert_eq!(tone_to_name(108), "C8");
        assert_eq!(tone_to_name(-1), "");
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(name_to_tone(""), None);
        assert_eq!(name_to_tone("C"), None);
        assert_eq!(name_to_tone("C#"), None);
        assert_eq!(name_to_tone("H4"), None);
        assert_eq!(name_to_tone("CX4"), None);
        assert_eq!(name_to_tone("4C"), None);
        assert_eq!(name_to_tone("Ｃ４"), None); // fullwidth, non-ASCII
    }
}
