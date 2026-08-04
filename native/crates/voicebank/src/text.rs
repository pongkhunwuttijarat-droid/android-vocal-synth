//! Text decoding with legacy voicebank encodings (Shift-JIS etc.).
//!
//! UTAU voicebanks are often encoded in Shift-JIS (especially `character.txt`
//! and older `oto.ini` files). Decoding strategy, most specific first:
//!
//! 1. An explicitly declared encoding (from `character.yaml`'s
//!    `text_file_encoding`, or an oto.ini `#Charset:` header).
//! 2. A byte order mark (UTF-8 / UTF-16LE / UTF-16BE).
//! 3. Strict UTF-8 validation (modern voicebanks).
//! 4. Shift-JIS as the UTAU legacy default.
//!
//! Decoding is lossy (`encoding_rs` replaces unmappable bytes with U+FFFD);
//! a malformed legacy file never aborts loading.

use encoding_rs::{Encoding, SHIFT_JIS, UTF_16BE, UTF_16LE, UTF_8};

/// Resolve the encoding to use for `bytes`, given an optional declared label
/// (e.g. `"shift_jis"`, `"utf-8"`). Unknown labels fall through to sniffing.
pub fn detect_encoding(bytes: &[u8], declared: Option<&str>) -> &'static Encoding {
    if let Some(label) = declared {
        if let Some(enc) = Encoding::for_label(label.as_bytes()) {
            return enc;
        }
    }
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return UTF_8;
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return UTF_16LE;
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return UTF_16BE;
    }
    if std::str::from_utf8(bytes).is_ok() {
        return UTF_8;
    }
    SHIFT_JIS
}

/// Decode `bytes` to a `String` using the resolved encoding (lossy).
pub fn decode(bytes: &[u8], declared: Option<&str>) -> String {
    let enc = detect_encoding(bytes, declared);
    let bytes = if std::ptr::eq(enc, UTF_8) && bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &bytes[3..]
    } else if (std::ptr::eq(enc, UTF_16LE) && bytes.starts_with(&[0xFF, 0xFE]))
        || (std::ptr::eq(enc, UTF_16BE) && bytes.starts_with(&[0xFE, 0xFF]))
    {
        &bytes[2..]
    } else {
        bytes
    };
    enc.decode(bytes).0.into_owned()
}

/// Decode `bytes` and split into lines, handling both LF and CRLF.
pub fn decode_lines(bytes: &[u8], declared: Option<&str>) -> Vec<String> {
    decode(bytes, declared)
        .lines()
        .map(|l| l.trim_end_matches('\r').to_string())
        .collect()
}

/// Read a text file and decode it.
pub fn read_text(path: &std::path::Path, declared: Option<&str>) -> std::io::Result<String> {
    Ok(decode(&std::fs::read(path)?, declared))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_sniffed_when_valid() {
        let s = decode("こんにちは".as_bytes(), None);
        assert_eq!(s, "こんにちは");
    }

    #[test]
    fn shift_jis_fallback() {
        // "重音" in Shift-JIS: 0x8F 0x64 0x89 0xB9 (as in the real Teto
        // character.txt) — invalid UTF-8, so sniffing falls back to SJIS.
        let bytes = b"name=\x8F\x64\x89\xB9";
        let s = decode(bytes, None);
        assert_eq!(s, "name=重音");
    }

    #[test]
    fn declared_wins() {
        // "重音" in UTF-8 is valid UTF-8; forcing shift_jis garbles it.
        let bytes = "重音".as_bytes();
        let s = decode(bytes, Some("shift_jis"));
        assert_ne!(s, "重音");
        let s = decode(bytes, Some("utf-8"));
        assert_eq!(s, "重音");
    }

    #[test]
    fn unknown_label_falls_back_to_sniffing() {
        let s = decode(b"hello", Some("no-such-encoding"));
        assert_eq!(s, "hello");
    }

    #[test]
    fn bom_handling() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("abc".as_bytes());
        assert_eq!(decode(&bytes, None), "abc");
    }

    #[test]
    fn crlf_lines() {
        let lines = decode_lines(b"a\r\nb\nc\r\n", None);
        assert_eq!(lines, vec!["a", "b", "c"]);
    }
}
