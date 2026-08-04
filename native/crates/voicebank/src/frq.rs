//! `.frq` (FREQ0003) pitch file reader.
//!
//! UTAU resamplers store per-frame f0 and amplitude next to each sample:
//! `sample.wav` -> `sample_wav.frq` (extension dots become underscores).
//! Layout (all little-endian):
//!
//! ```text
//! 0..8   "FREQ0003"
//! 8..12  hop size (i32)
//! 12..20 average f0 (f64)
//! 20..36 16 bytes reserved
//! 36..40 frame count (i32)
//! 40..   frame count × (f0 f64, amp f64)
//! ```

use std::path::{Path, PathBuf};

/// Loaded frq data.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FrqData {
    /// Samples between pitch frames (usually 256).
    pub hop_size: i32,
    /// Average f0 of voiced frames, Hz.
    pub average_f0: f64,
    /// Per-frame f0, Hz (0 = unvoiced).
    pub f0: Vec<f64>,
    /// Per-frame amplitude.
    pub amp: Vec<f64>,
}

#[derive(Debug, thiserror::Error)]
pub enum FrqError {
    #[error("frq file not found: {0}")]
    NotFound(PathBuf),
    #[error("bad FREQ0003 header")]
    BadHeader,
    #[error("file too short")]
    TooShort,
    #[error("invalid frame count {0}")]
    InvalidLength(i32),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Derive the frq path for a wav path: `dir/x.wav` -> `dir/x_wav.frq`.
pub fn frq_path_for_wav(wav_path: &Path) -> PathBuf {
    let file = wav_path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = wav_path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let stem = file.strip_suffix(&ext).unwrap_or(&file);
    let frq_name = format!("{stem}{}.frq", ext.replace('.', "_"));
    wav_path.with_file_name(frq_name)
}

/// Parse FREQ0003 bytes.
pub fn parse_frq(bytes: &[u8]) -> Result<FrqData, FrqError> {
    if bytes.len() < 40 {
        return Err(FrqError::TooShort);
    }
    if &bytes[0..8] != b"FREQ0003" {
        return Err(FrqError::BadHeader);
    }
    let hop_size = i32::from_le_bytes(bytes[8..12].try_into().unwrap());
    let average_f0 = f64::from_le_bytes(bytes[12..20].try_into().unwrap());
    let len = i32::from_le_bytes(bytes[36..40].try_into().unwrap());
    if len < 0 {
        return Err(FrqError::InvalidLength(len));
    }
    let len = len as usize;
    let need = 40usize.checked_add(len.checked_mul(16).ok_or(FrqError::InvalidLength(len as i32))?);
    let Some(need) = need else { return Err(FrqError::InvalidLength(len as i32)) };
    if bytes.len() < need {
        return Err(FrqError::TooShort);
    }
    let mut f0 = Vec::with_capacity(len);
    let mut amp = Vec::with_capacity(len);
    for i in 0..len {
        let base = 40 + i * 16;
        f0.push(f64::from_le_bytes(bytes[base..base + 8].try_into().unwrap()));
        amp.push(f64::from_le_bytes(bytes[base + 8..base + 16].try_into().unwrap()));
    }
    Ok(FrqData {
        hop_size,
        average_f0,
        f0,
        amp,
    })
}

/// Read a frq file. Use [`read_frq_for_wav`] to resolve the companion file
/// of a wav automatically.
pub fn read_frq(path: &Path) -> Result<FrqData, FrqError> {
    parse_frq(&std::fs::read(path)?)
}

/// Read the companion frq file of a wav (`x.wav` -> `x_wav.frq`).
/// Returns [`FrqError::NotFound`] when no frq file exists.
pub fn read_frq_for_wav(wav_path: &Path) -> Result<FrqData, FrqError> {
    let frq_path = frq_path_for_wav(wav_path);
    if !frq_path.is_file() {
        return Err(FrqError::NotFound(frq_path));
    }
    read_frq(&frq_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frq_bytes(hop: i32, avg: f64, frames: &[(f64, f64)]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"FREQ0003");
        b.extend_from_slice(&hop.to_le_bytes());
        b.extend_from_slice(&avg.to_le_bytes());
        b.extend_from_slice(&[0u8; 16]);
        b.extend_from_slice(&(frames.len() as i32).to_le_bytes());
        for (f, a) in frames {
            b.extend_from_slice(&f.to_le_bytes());
            b.extend_from_slice(&a.to_le_bytes());
        }
        b
    }

    #[test]
    fn parses_frq() {
        let frames = vec![(0.0, 100.0), (261.6, 200.0), (329.6, 150.0)];
        let f = parse_frq(&frq_bytes(256, 261.6, &frames)).unwrap();
        assert_eq!(f.hop_size, 256);
        assert_eq!(f.average_f0, 261.6);
        assert_eq!(f.f0, vec![0.0, 261.6, 329.6]);
        assert_eq!(f.amp, vec![100.0, 200.0, 150.0]);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(matches!(parse_frq(b"BOGUS"), Err(FrqError::TooShort)));
        let mut b = frq_bytes(256, 0.0, &[(1.0, 1.0)]);
        b[0..8].copy_from_slice(b"FREQ0004");
        assert!(matches!(parse_frq(&b), Err(FrqError::BadHeader)));
        let mut b = frq_bytes(256, 0.0, &[(1.0, 1.0)]);
        b[36..40].copy_from_slice(&(-3i32).to_le_bytes());
        assert!(matches!(parse_frq(&b), Err(FrqError::InvalidLength(_))));
        // Truncated frame data.
        let full = frq_bytes(256, 0.0, &[(1.0, 1.0), (2.0, 2.0)]);
        assert!(matches!(parse_frq(&full[..40 + 8]), Err(FrqError::TooShort)));
        // Trailing garbage is ignored.
        let mut b = frq_bytes(256, 0.0, &[(1.0, 1.0)]);
        b.extend_from_slice(b"junk");
        assert!(parse_frq(&b).is_ok());
    }

    #[test]
    fn frq_path_derivation() {
        assert_eq!(
            frq_path_for_wav(Path::new("/vb/voice/_3_h3_3-.wav")),
            PathBuf::from("/vb/voice/_3_h3_3-_wav.frq")
        );
        assert_eq!(
            frq_path_for_wav(Path::new("/vb/voice/note.wav")),
            PathBuf::from("/vb/voice/note_wav.frq")
        );
        assert_eq!(
            frq_path_for_wav(Path::new("/vb/voice/noext")),
            PathBuf::from("/vb/voice/noext.frq")
        );
    }
}
