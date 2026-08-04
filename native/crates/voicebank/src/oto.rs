//! oto.ini parsing.
//!
//! Format per line: `<wav>=<alias>,<offset>,<consonant>,<cutoff>,<preutter>,<overlap>`
//! (`<wav>` may be followed by `=<alias>` where the alias is the phoneme
//! sequence; the wav filename may be used as alias when the alias is empty).
//!
//! Handles:
//! - CRLF / LF line endings
//! - comments (lines starting with `;` or `#` after trimming)
//! - a `#Charset:<encoding>` header in the first 10 lines (UTAU convention)
//! - quoted aliases containing commas: `file.wav="a,b",1,2,3,4,5`
//! - Shift-JIS / legacy encodings via [`crate::text`]
//!
//! Mirrors OpenUtau's `VoicebankLoader.ParseOto` / `ParseOtoSet`.

use std::path::PathBuf;

use crate::text;

/// One oto.ini entry.
///
/// Wav layout (from OpenUtau's `Oto`):
/// `|-offset-|-consonant-(fixed)-|-stretched-|-cutoff-|`
/// `|        |-preutter-----|`
/// `|        |-overlap-|`
#[derive(Debug, Clone, PartialEq)]
pub struct Oto {
    /// Phoneme sequence this entry is triggered by (e.g. `"3 h3"`).
    pub alias: String,
    /// Same as `alias` at load time; kept separate for future edits.
    pub phonetic: String,
    /// Wave file name, relative to the directory containing oto.ini.
    pub wav: String,
    /// Length of left offset (ms).
    pub offset: f64,
    /// Length of unstretched consonant in wav, AKA fixed (ms).
    pub consonant: f64,
    /// Length of right cutoff, AKA end blank (ms). If negative, length of
    /// (consonant + stretched).
    pub cutoff: f64,
    /// Length before note start, usually within consonant range (ms).
    pub preutter: f64,
    /// Length overlap with previous note, usually within consonant range (ms).
    pub overlap: f64,
    /// False if the line failed to parse or its wav file is missing.
    pub is_valid: bool,
    /// Parse/validation error message, if any.
    pub error: Option<String>,
    /// 1-based line number in the oto.ini file.
    pub line_number: usize,
    /// Index into [`crate::Voicebank::oto_sets`]; filled in by the loader.
    pub set_index: usize,
}

impl Oto {
    /// Create an invalid placeholder.
    fn new(line_number: usize) -> Self {
        Oto {
            alias: String::new(),
            phonetic: String::new(),
            wav: String::new(),
            offset: 0.0,
            consonant: 0.0,
            cutoff: 0.0,
            preutter: 0.0,
            overlap: 0.0,
            is_valid: false,
            error: None,
            line_number,
            set_index: 0,
        }
    }
}

/// A parse error for one oto.ini line (invalid lines are skipped, not fatal).
#[derive(Debug, Clone, PartialEq)]
pub struct OtoError {
    pub line: usize,
    pub message: String,
}

/// Result of parsing one oto.ini file.
#[derive(Debug, Clone, Default)]
pub struct OtoIni {
    pub otos: Vec<Oto>,
    pub errors: Vec<OtoError>,
}

const FORMAT_MSG: &str =
    "Line does not match format <wav>=<alias>,<offset>,<consonant>,<cutoff>,<preutter>,<overlap>.";

/// Parse one oto.ini line (no leading/trailing whitespace expected).
fn parse_oto_line(line: &str, line_number: usize) -> Oto {
    let mut oto = Oto::new(line_number);
    let Some(eq) = line.find('=') else {
        oto.error = Some(FORMAT_MSG.to_string());
        return oto;
    };
    oto.wav = line[..eq].trim().to_string();
    let rest = &line[eq + 1..];

    let (alias, nums) = split_alias(rest);
    if alias.is_empty() {
        oto.alias = remove_extension(&oto.wav);
    } else {
        oto.alias = alias;
    }
    oto.phonetic = oto.alias.clone();

    let fields: Vec<&str> = nums.split(',').collect();
    if !parse_num(fields.first().copied(), &mut oto.offset) {
        oto.error = Some(format!("{FORMAT_MSG} Failed to parse offset."));
        return oto;
    }
    if !parse_num(fields.get(1).copied(), &mut oto.consonant) {
        oto.error = Some(format!("{FORMAT_MSG} Failed to parse consonant."));
        return oto;
    }
    if !parse_num(fields.get(2).copied(), &mut oto.cutoff) {
        oto.error = Some(format!("{FORMAT_MSG} Failed to parse cutoff."));
        return oto;
    }
    if !parse_num(fields.get(3).copied(), &mut oto.preutter) {
        oto.error = Some(format!("{FORMAT_MSG} Failed to parse preutter."));
        return oto;
    }
    if !parse_num(fields.get(4).copied(), &mut oto.overlap) {
        oto.error = Some(format!("{FORMAT_MSG} Failed to parse overlap."));
        return oto;
    }
    oto.is_valid = true;
    oto
}

/// Split the right side of `wav=...` into (alias, numbers). A leading
/// double-quote makes the alias extend to the closing quote, so aliases may
/// contain commas.
fn split_alias(rest: &str) -> (String, &str) {
    let rest = rest.trim_start();
    if let Some(stripped) = rest.strip_prefix('"') {
        // Quoted alias: `"a,b",1,2,3,4,5`
        if let Some(end) = stripped.find('"') {
            let nums = &stripped[end + 1..];
            let nums = nums.strip_prefix(',').unwrap_or(nums);
            return (stripped[..end].to_string(), nums);
        }
        // Unterminated quote: treat the whole rest as the alias.
        return (rest.to_string(), "");
    }
    match rest.find(',') {
        Some(i) => (rest[..i].trim().to_string(), &rest[i + 1..]),
        None => (rest.trim().to_string(), ""),
    }
}

/// Parse a numeric field. Empty/missing fields are 0 (OpenUtau behavior);
/// non-numeric fields fail the line.
fn parse_num(s: Option<&str>, out: &mut f64) -> bool {
    match s {
        None | Some("") => {
            *out = 0.0;
            true
        }
        Some(t) => match t.trim().parse::<f64>() {
            Ok(v) => {
                *out = v;
                true
            }
            Err(_) => false,
        },
    }
}

fn remove_extension(file: &str) -> String {
    match file.rfind('.') {
        Some(i) if i > 0 => file[..i].to_string(),
        _ => file.to_string(),
    }
}

/// Detect a `#Charset:<label>` header in the first 10 lines (UTAU convention).
pub fn detect_charset(bytes: &[u8]) -> Option<String> {
    for line in bytes.split(|&b| b == b'\n').take(10) {
        let line = trim_ascii(line);
        if let Some(rest) = line.strip_prefix(b"#Charset:") {
            let label = trim_ascii(rest);
            if !label.is_empty() {
                return Some(String::from_utf8_lossy(label).into_owned());
            }
        }
    }
    None
}

fn trim_ascii(mut b: &[u8]) -> &[u8] {
    while b.first().is_some_and(u8::is_ascii_whitespace) {
        b = &b[1..];
    }
    while b.last().is_some_and(u8::is_ascii_whitespace) {
        b = &b[..b.len() - 1];
    }
    b
}

/// Parse an oto.ini file. `declared` is the voicebank's `text_file_encoding`
/// (from character.yaml); a `#Charset:` header in the file overrides it.
pub fn parse_oto_ini(bytes: &[u8], declared: Option<&str>) -> OtoIni {
    let charset = detect_charset(bytes);
    let encoding = charset.as_deref().or(declared);
    let text = text::decode(bytes, encoding);
    let mut ini = OtoIni::default();
    for (idx, raw) in text.lines().enumerate() {
        let line = raw.trim_end_matches('\r').trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        let oto = parse_oto_line(line, idx + 1);
        match oto.error.clone() {
            Some(message) => ini.errors.push(OtoError {
                line: idx + 1,
                message,
            }),
            None if oto.is_valid => ini.otos.push(oto),
            None => {}
        }
    }
    ini
}

/// One directory's worth of otos (a `voice/` folder, typically).
#[derive(Debug, Clone)]
pub struct OtoSet {
    /// Relative path from the voicebank library dir (`""` for the root).
    pub name: String,
    /// Absolute path to this oto.ini.
    pub file: PathBuf,
    pub otos: Vec<Oto>,
}

impl OtoSet {
    /// Directory containing this oto.ini.
    pub fn dir(&self) -> &std::path::Path {
        self.file.parent().unwrap_or_else(|| std::path::Path::new(""))
    }

    /// Absolute path of a wav file referenced by this set.
    pub fn wav_path(&self, wav: &str) -> PathBuf {
        self.dir().join(wav)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(s: &str) -> Oto {
        parse_oto_line(s, 1)
    }

    #[test]
    fn parses_standard_line() {
        let o = line("_a.wav=a,120.0,92.905,-329.615,0.0,0.0");
        assert!(o.is_valid);
        assert_eq!(o.wav, "_a.wav");
        assert_eq!(o.alias, "a");
        assert_eq!(o.phonetic, "a");
        assert_eq!(o.offset, 120.0);
        assert_eq!(o.consonant, 92.905);
        assert_eq!(o.cutoff, -329.615);
        assert_eq!(o.preutter, 0.0);
        assert_eq!(o.overlap, 0.0);
        assert_eq!(o.line_number, 1);
    }

    #[test]
    fn alias_with_spaces_and_special_chars() {
        let o = line("_3_h3_3-.wav=3 h3,442.3,375.0,-583.333,250.0,83.333");
        assert!(o.is_valid);
        assert_eq!(o.alias, "3 h3");
        let o = line("_z+@_z+@_z-.wav=z+@,1,2,3,4,5");
        assert_eq!(o.alias, "z+@");
        let o = line("x.wav=- @,120.0,124.999,-333.332,0.0,0.0");
        assert_eq!(o.alias, "- @");
    }

    #[test]
    fn empty_alias_uses_filename() {
        let o = line("foo.wav=,1,2,3,4,5");
        assert!(o.is_valid);
        assert_eq!(o.alias, "foo");
        // A non-empty first field IS the alias (even a numeric one).
        let o = line("foo.wav=1,2,3,4,5");
        assert!(o.is_valid);
        assert_eq!(o.alias, "1");
        assert_eq!(o.offset, 2.0);
    }

    #[test]
    fn quoted_alias_with_comma() {
        let o = line(r#"file.wav="a,b",1,2,3,4,5"#);
        assert!(o.is_valid);
        assert_eq!(o.alias, "a,b");
        assert_eq!(o.offset, 1.0);
        let o = line(r#"file.wav="say, hi",0.5,1.5,2.5,3.5,4.5"#);
        assert!(o.is_valid);
        assert_eq!(o.alias, "say, hi");
        assert_eq!(o.cutoff, 2.5);
    }

    #[test]
    fn missing_numbers_default_to_zero() {
        let o = line("f.wav=a");
        assert!(o.is_valid);
        assert_eq!(o.offset, 0.0);
        assert_eq!(o.consonant, 0.0);
        assert_eq!(o.cutoff, 0.0);
        assert_eq!(o.preutter, 0.0);
        assert_eq!(o.overlap, 0.0);
    }

    #[test]
    fn garbage_number_invalidates() {
        let o = line("f.wav=a,abc,2,3,4,5");
        assert!(!o.is_valid);
        assert!(o.error.as_deref().unwrap().contains("offset"));
        let o = line("f.wav=a,1,2,3,4,zz");
        assert!(!o.is_valid);
        assert!(o.error.as_deref().unwrap().contains("overlap"));
    }

    #[test]
    fn no_equals_invalidates() {
        let o = line("not an oto line");
        assert!(!o.is_valid);
        assert!(o.error.is_some());
    }

    #[test]
    fn wav_with_equals_in_alias() {
        let o = line("f.wav=a=b,1,2,3,4,5");
        assert!(o.is_valid);
        assert_eq!(o.alias, "a=b");
        assert_eq!(o.offset, 1.0);
    }

    #[test]
    fn crlf_and_comments() {
        let ini = parse_oto_ini(b"f.wav=a,1,2,3,4,5\r\n; comment\r\n# another\r\n\r\ng.wav=b,6,7,8,9,10\r\n", None);
        assert_eq!(ini.otos.len(), 2);
        assert!(ini.errors.is_empty());
        assert_eq!(ini.otos[0].alias, "a");
        assert_eq!(ini.otos[1].alias, "b");
    }

    #[test]
    fn bad_lines_collected_as_errors() {
        let ini = parse_oto_ini(b"good.wav=a,1,2,3,4,5\nbad line\n", None);
        assert_eq!(ini.otos.len(), 1);
        assert_eq!(ini.errors.len(), 1);
        assert_eq!(ini.errors[0].line, 2);
    }

    #[test]
    fn charset_header_detected() {
        assert_eq!(
            detect_charset(b"#Charset:UTF-8\nf.wav=a,1,2,3,4,5\n"),
            Some("UTF-8".to_string())
        );
        assert_eq!(detect_charset(b"f.wav=a,1,2,3,4,5\n"), None);
        // Header beyond the first 10 lines is not detected.
        let mut bytes = Vec::new();
        for i in 0..12 {
            bytes.extend_from_slice(format!("line {i}\n").as_bytes());
        }
        bytes.extend_from_slice(b"#Charset:UTF-8\n");
        assert_eq!(detect_charset(&bytes), None);
    }

    #[test]
    fn shift_jis_alias_decoded() {
        // "あ" in Shift-JIS is 0x82 0xA0.
        let ini = parse_oto_ini(b"f.wav=\x82\xA0,1,2,3,4,5\r\n", None);
        assert_eq!(ini.otos.len(), 1);
        assert_eq!(ini.otos[0].alias, "あ");
    }
}
