//! Minimal RIFF/WAVE writer (Sprint 2.2): mono `f32` sample streams →
//! standard WAV files.
//!
//! * [`write_wav_16`] — 16-bit PCM (`fmt` format 1), the classic UTAU
//!   render output format.
//! * [`write_wav_float32`] — 32-bit IEEE float (`fmt` format 3), lossless
//!   for pipeline-internal exchange.
//! * [`write_wav_file`] — write the bytes produced by either encoder to a
//!   path.
//!
//! The output is readable by the voicebank crate's [`voicebank::read_wav`]
//! (and any standard WAV consumer): canonical `RIFF`/`WAVE`/`fmt `/`data`
//! layout, little-endian, mono, no extra chunks.
//!
//! Quantization follows the standard f32→PCM16 convention: samples are
//! `round(clamp(s, -1, 1) * 32768)` with saturation, so `-1.0` maps to
//! `-32768` (decodes back exactly) and `+1.0` saturates to `32767` (the
//! format's inherent one-LSB asymmetry) — the same grid as the voicebank
//! reader's `x / 32768` normalization.

use std::io::Write;
use std::path::Path;

/// `WAVE_FORMAT_PCM` (`fmt` chunk audio format code).
pub const WAVE_FORMAT_PCM: u16 = 0x0001;
/// `WAVE_FORMAT_IEEE_FLOAT` (`fmt` chunk audio format code).
pub const WAVE_FORMAT_IEEE_FLOAT: u16 = 0x0003;

/// Encode `samples` (mono, `[-1, 1]`) as a 16-bit PCM WAV file.
///
/// Returns the complete RIFF/WAVE bytes. `sample_rate` must be non-zero.
pub fn write_wav_16(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let mut pcm = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        // The cast saturates: +1.0 → 32768 → 32767, -1.0 → -32768.
        let quantized = (s.clamp(-1.0, 1.0) * 32768.0).round() as i16;
        pcm.extend_from_slice(&quantized.to_le_bytes());
    }
    build_wav(&pcm, sample_rate, 16, WAVE_FORMAT_PCM)
}

/// Encode `samples` (mono) as a 32-bit IEEE float WAV file (lossless).
///
/// Values are written verbatim (no clamping — float WAVs carry any value);
/// the voicebank reader returns them bit-exact.
pub fn write_wav_float32(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity(samples.len() * 4);
    for &s in samples {
        data.extend_from_slice(&s.to_le_bytes());
    }
    build_wav(&data, sample_rate, 32, WAVE_FORMAT_IEEE_FLOAT)
}

/// Write WAV bytes (from [`write_wav_16`] or [`write_wav_float32`]) to
/// `path`, creating it (and its parent directories) as needed.
pub fn write_wav_file(path: impl AsRef<Path>, wav_bytes: &[u8]) -> std::io::Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let mut file = std::fs::File::create(path)?;
    file.write_all(wav_bytes)?;
    file.sync_all()
}

/// Assemble a canonical mono WAV: `RIFF` + size, `WAVE`, `fmt ` (16-byte
/// PCM-style payload), `data` + payload. `bits` is the container size
/// (16 or 32); `format` the `fmt` audio format code.
fn build_wav(data: &[u8], sample_rate: u32, bits: u16, format: u16) -> Vec<u8> {
    assert!(sample_rate > 0, "sample rate must be non-zero");
    let channels: u16 = 1;
    let block_align = channels * bits / 8;
    let byte_rate = sample_rate * block_align as u32;
    let data_len = data.len() as u32;
    // RIFF size: "WAVE" (4) + fmt chunk (8 + 16) + data chunk (8 + data).
    let riff_size = 4 + 8 + 16 + 8 + data_len;

    let mut out = Vec::with_capacity(12 + 8 + 16 + 8 + data.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_size.to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt payload size
    out.extend_from_slice(&format.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend_from_slice(data);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    /// Byte-level header inspection: RIFF/WAVE, fmt fields, data size.
    fn assert_header(bytes: &[u8], sample_rate: u32, bits: u16, format: u16, data_len: u32) {
        assert_eq!(&bytes[0..4], b"RIFF");
        let riff_size = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        assert_eq!(riff_size, 36 + data_len);
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[12..16], b"fmt ");
        assert_eq!(u32::from_le_bytes(bytes[16..20].try_into().unwrap()), 16);
        assert_eq!(
            u16::from_le_bytes(bytes[20..22].try_into().unwrap()),
            format
        );
        assert_eq!(u16::from_le_bytes(bytes[22..24].try_into().unwrap()), 1); // mono
        assert_eq!(
            u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
            sample_rate
        );
        let block_align = bits / 8;
        assert_eq!(
            u32::from_le_bytes(bytes[28..32].try_into().unwrap()),
            sample_rate * block_align as u32
        );
        assert_eq!(
            u16::from_le_bytes(bytes[32..34].try_into().unwrap()),
            block_align
        );
        assert_eq!(u16::from_le_bytes(bytes[34..36].try_into().unwrap()), bits);
        assert_eq!(&bytes[36..40], b"data");
        assert_eq!(
            u32::from_le_bytes(bytes[40..44].try_into().unwrap()),
            data_len
        );
        assert_eq!(bytes.len() as u32, 44 + data_len);
    }

    #[test]
    fn pcm16_header_is_canonical() {
        let samples = [0.0f32, 0.5, -0.5];
        let wav = write_wav_16(&samples, 44100);
        assert_header(&wav, 44100, 16, WAVE_FORMAT_PCM, 6);
        // Sample bytes: 0x0000, 0x4000 (16384), 0xC000 (-16384).
        assert_eq!(&wav[44..], &[0x00, 0x00, 0x00, 0x40, 0x00, 0xC0]);
    }

    #[test]
    fn pcm16_roundtrip_within_quantization_error() {
        let samples = vec![
            0.0, 0.5, -0.5, 1.0, -1.0, 0.25, -0.125, 0.0001, -0.9999, 0.3333333,
        ];
        let wav = write_wav_16(&samples, 22050);
        let data = voicebank::parse_wav(&wav).expect("parse back");
        assert_eq!(data.sample_rate, 22050);
        assert_eq!(data.channels, 1);
        assert_eq!(data.bits_per_sample, 16);
        assert_eq!(data.samples.len(), samples.len());
        let lsb = 1.0 / 32768.0;
        for (got, want) in data.samples.iter().zip(&samples) {
            assert!(
                (got - want).abs() <= lsb + 1e-7,
                "sample {want} decoded as {got}"
            );
        }
        // Exact spot checks of the quantization convention.
        assert!((data.samples[0]).abs() < 1e-6); // 0.0
        assert!((data.samples[1] - 0.5).abs() < 1e-6); // 0.5 → 16384 exactly
        assert!((data.samples[3] - 32767.0 / 32768.0).abs() < 1e-6); // +1.0 → 32767
        assert!((data.samples[4] + 1.0).abs() < 1e-6); // -1.0 → -32767
    }

    #[test]
    fn pcm16_clamps_out_of_range_samples() {
        let wav = write_wav_16(&[2.0, -2.0, f32::NAN], 44100);
        let data = voicebank::parse_wav(&wav).unwrap();
        assert!((data.samples[0] - 32767.0 / 32768.0).abs() < 1e-6);
        assert!((data.samples[1] + 1.0).abs() < 1e-6);
        // NaN clamps to 0.0 by the (NaN < -1) comparison chain.
        assert!((data.samples[2]).abs() < 1e-6);
    }

    #[test]
    fn float32_roundtrip_is_bit_exact() {
        let samples = vec![
            0.0,
            1.0,
            -1.0,
            0.5,
            -0.25,
            std::f32::consts::PI,
            -2.0e-5,
            1.0000001, // > 1.0 survives verbatim
        ];
        let wav = write_wav_float32(&samples, 48000);
        assert_header(&wav, 48000, 32, WAVE_FORMAT_IEEE_FLOAT, 32);
        let data = voicebank::parse_wav(&wav).expect("parse back");
        assert_eq!(data.sample_rate, 48000);
        assert_eq!(data.bits_per_sample, 32);
        assert_eq!(data.samples, samples); // exact equality
    }

    #[test]
    fn file_write_roundtrip() {
        let samples: Vec<f32> = (0..1000).map(|i| ((i as f32) / 500.0) - 1.0).collect();
        let wav = write_wav_16(&samples, 44100);
        let dir = std::env::temp_dir().join(format!("wavwriter-test-{}", std::process::id()));
        let path = dir.join("sub/out.wav");
        write_wav_file(&path, &wav).expect("write file");
        let back = voicebank::read_wav(&path).expect("read file");
        assert_eq!(back.sample_rate, 44100);
        assert_eq!(back.samples.len(), samples.len());
        let lsb = 1.0 / 32768.0;
        for (got, want) in back.samples.iter().zip(&samples) {
            assert!(
                (got - want).abs() <= lsb + 1e-7,
                "sample {want} decoded as {got}"
            );
        }
        // The raw bytes on disk match the encoder output exactly.
        let mut disk = Vec::new();
        std::fs::File::open(&path)
            .unwrap()
            .read_to_end(&mut disk)
            .unwrap();
        assert_eq!(disk, wav);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_input_produces_valid_zero_length_wav() {
        let wav = write_wav_16(&[], 44100);
        assert_header(&wav, 44100, 16, WAVE_FORMAT_PCM, 0);
        let wav = write_wav_float32(&[], 44100);
        assert_header(&wav, 44100, 32, WAVE_FORMAT_IEEE_FLOAT, 0);
    }
}
