//! Minimal RIFF/WAVE reader for UTAU voicebank samples.
//!
//! Supports PCM 8/16/24/32-bit and IEEE float 32-bit, mono or stereo
//! (interleaved), any sample rate. Samples are normalized to `[-1, 1]` f32.
//! Stereo files can be downmixed with [`WavData::to_mono`]; files whose rate
//! differs from 44.1 kHz can be converted with [`WavData::resampled`].

use std::path::Path;

/// Decoded wave data. `samples` are interleaved (L,R,L,R,... for stereo) at
/// the file's native sample rate.
#[derive(Debug, Clone, PartialEq)]
pub struct WavData {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub samples: Vec<f32>,
}

impl WavData {
    /// Average channels into a single mono stream.
    pub fn to_mono(&self) -> Vec<f32> {
        let ch = self.channels.max(1) as usize;
        if ch == 1 {
            return self.samples.clone();
        }
        self.samples
            .chunks_exact(ch)
            .map(|frame| frame.iter().sum::<f32>() / ch as f32)
            .collect()
    }

    /// Linear-interpolation resample to `target_rate`. No-op when the rate
    /// already matches. (A placeholder-quality resampler; the synthesis
    /// pipeline can replace it with a windowed-sinc later.)
    pub fn resampled(&self, target_rate: u32) -> WavData {
        if target_rate == self.sample_rate || self.sample_rate == 0 {
            return self.clone();
        }
        let ch = self.channels.max(1) as usize;
        let frames = self.samples.len() / ch;
        let out_frames = (frames as f64 * target_rate as f64 / self.sample_rate as f64).floor();
        let out_frames = out_frames.max(0.0) as usize;
        let ratio = self.sample_rate as f64 / target_rate as f64;
        let mut out = Vec::with_capacity(out_frames * ch);
        for f in 0..out_frames {
            let src = f as f64 * ratio;
            let i0 = (src.floor() as usize).min(frames.saturating_sub(1));
            let i1 = (i0 + 1).min(frames.saturating_sub(1));
            let frac = (src - i0 as f64) as f32;
            for c in 0..ch {
                let a = self.samples[i0 * ch + c];
                let b = self.samples[i1 * ch + c];
                out.push(a + (b - a) * frac);
            }
        }
        WavData {
            sample_rate: target_rate,
            channels: self.channels,
            bits_per_sample: self.bits_per_sample,
            samples: out,
        }
    }

    /// Mono stream resampled to 44.1 kHz — the canonical format for the
    /// classic UTAU rendering pipeline.
    pub fn mono_44100(&self) -> Vec<f32> {
        self.resampled(44100).to_mono()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WavError {
    #[error("not a RIFF file (missing RIFF header)")]
    NotRiff,
    #[error("not a WAVE file (missing WAVE tag)")]
    NotWave,
    #[error("missing fmt chunk")]
    MissingFmt,
    #[error("no data chunk")]
    NoData,
    #[error("unsupported format: audio_format={audio_format}, bits={bits}")]
    UnsupportedFormat { audio_format: u16, bits: u16 },
    #[error("malformed file: {0}")]
    Malformed(&'static str),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

const WAVE_FORMAT_PCM: u16 = 0x0001;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 0x0003;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;

/// Read and parse a .wav file.
pub fn read_wav(path: &Path) -> Result<WavData, WavError> {
    parse_wav(&std::fs::read(path)?)
}

/// Parse RIFF/WAVE bytes.
pub fn parse_wav(bytes: &[u8]) -> Result<WavData, WavError> {
    if bytes.len() < 12 {
        return Err(WavError::Malformed("file too short"));
    }
    if &bytes[0..4] != b"RIFF" {
        return Err(WavError::NotRiff);
    }
    if &bytes[8..12] != b"WAVE" {
        return Err(WavError::NotWave);
    }
    let mut pos = 12usize;
    let mut fmt: Option<(u16, u16, u32, u16)> = None; // (format, channels, rate, bits)
    let mut data: Vec<u8> = Vec::new();
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let start = pos + 8;
        let end = (start + size).min(bytes.len());
        match id {
            b"fmt " => {
                if end < start + 16 {
                    return Err(WavError::Malformed("fmt chunk too short"));
                }
                let audio_format = u16::from_le_bytes(bytes[start..start + 2].try_into().unwrap());
                let channels = u16::from_le_bytes(bytes[start + 2..start + 4].try_into().unwrap());
                let sample_rate = u32::from_le_bytes(bytes[start + 4..start + 8].try_into().unwrap());
                let bits = u16::from_le_bytes(bytes[start + 14..start + 16].try_into().unwrap());
                let mut format = audio_format;
                if audio_format == WAVE_FORMAT_EXTENSIBLE {
                    // SubFormat GUID starts at offset 24 of the fmt payload
                    // (after cbSize + validBits + channelMask); its first two
                    // bytes are the actual format code.
                    if end >= start + 26 {
                        format = u16::from_le_bytes(bytes[start + 24..start + 26].try_into().unwrap());
                    }
                }
                fmt = Some((format, channels, sample_rate, bits));
            }
            b"data" => {
                if size == u32::MAX as usize {
                    // Streaming size marker: data runs to end of file.
                    data.extend_from_slice(&bytes[start..]);
                    break;
                }
                data.extend_from_slice(&bytes[start..end]);
                // A data chunk of declared size 0 followed by more chunks is
                // possible; keep scanning. Otherwise stop at the first real
                // data chunk (trailing chunks after data are rare).
                if size > 0 {
                    break;
                }
            }
            _ => {}
        }
        pos = start + size + (size & 1); // chunks are word-aligned
    }
    let (format, channels, sample_rate, bits) = fmt.ok_or(WavError::MissingFmt)?;
    if channels == 0 {
        return Err(WavError::Malformed("zero channels"));
    }
    let samples = decode_samples(&data, format, bits)?;
    if samples.is_empty() {
        return Err(WavError::NoData);
    }
    Ok(WavData {
        sample_rate,
        channels,
        bits_per_sample: bits,
        samples,
    })
}

fn decode_samples(data: &[u8], format: u16, bits: u16) -> Result<Vec<f32>, WavError> {
    let bytes_per_sample = bits as usize / 8;
    if bytes_per_sample == 0 || bytes_per_sample > 4 {
        return Err(WavError::UnsupportedFormat { audio_format: format, bits });
    }
    let mut out = Vec::with_capacity(data.len() / bytes_per_sample + 1);
    let mut i = 0;
    while i + bytes_per_sample <= data.len() {
        let v = match (format, bits) {
            (WAVE_FORMAT_PCM, 8) => (data[i] as f32 - 128.0) / 128.0,
            (WAVE_FORMAT_PCM, 16) => {
                i16::from_le_bytes([data[i], data[i + 1]]) as f32 / 32768.0
            }
            (WAVE_FORMAT_PCM, 24) => {
                let raw = (data[i] as i32)
                    | ((data[i + 1] as i32) << 8)
                    | ((data[i + 2] as i32) << 16);
                ((raw << 8) >> 8) as f32 / 8388608.0 // arithmetic shift sign-extends
            }
            (WAVE_FORMAT_PCM, 32) => {
                i32::from_le_bytes(data[i..i + 4].try_into().unwrap()) as f32 / 2147483648.0
            }
            (WAVE_FORMAT_IEEE_FLOAT, 32) => {
                f32::from_le_bytes(data[i..i + 4].try_into().unwrap())
            }
            _ => {
                return Err(WavError::UnsupportedFormat {
                    audio_format: format,
                    bits,
                })
            }
        };
        out.push(v);
        i += bytes_per_sample;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        b.extend_from_slice(&(rate * block_align as u32).to_le_bytes()); // byte rate
        b.extend_from_slice(&block_align.to_le_bytes());
        b.extend_from_slice(&bits.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data_len.to_le_bytes());
        b.extend_from_slice(pcm);
        b
    }

    #[test]
    fn pcm16_mono() {
        let pcm: Vec<u8> = [0i16, 16384, -16384, 32767, -32768]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let w = parse_wav(&wav_bytes(&pcm, 1, 44100, 16)).unwrap();
        assert_eq!(w.sample_rate, 44100);
        assert_eq!(w.channels, 1);
        assert_eq!(w.samples.len(), 5);
        assert!((w.samples[0]).abs() < 1e-6);
        assert!((w.samples[1] - 0.5).abs() < 1e-5);
        assert!((w.samples[2] + 0.5).abs() < 1e-5);
        // 32767/32768 = 0.9999695 (16-bit is asymmetric by one LSB).
        assert!((w.samples[3] - 32767.0 / 32768.0).abs() < 1e-6);
        assert!((w.samples[4] + 1.0).abs() < 1e-6);
    }

    #[test]
    fn pcm8_unsigned_and_stereo_downmix() {
        // Two frames of stereo 8-bit: [ (0,255), (128,128) ].
        let pcm = [0u8, 255, 128, 128];
        let w = parse_wav(&wav_bytes(&pcm, 2, 22050, 8)).unwrap();
        assert_eq!(w.samples, vec![-1.0, 127.0 / 128.0, 0.0, 0.0]);
        let mono = w.to_mono();
        assert_eq!(mono, vec![-1.0 / 256.0, 0.0]); // (-1 + 127/128) / 2
        let r = w.resampled(44100);
        assert_eq!(r.sample_rate, 44100);
        assert_eq!(r.samples.len(), 8); // 4 frames at 2x rate, 2 channels each
        assert_eq!(w.resampled(22050), w);
    }

    #[test]
    fn pcm24_sign_extension() {
        // 0xFFFFFF = -1 (normalized -1/2^23), 0x800000 = -8388608 (-1.0).
        let pcm = [0xFFu8, 0xFF, 0xFF, 0x00, 0x00, 0x80];
        let w = parse_wav(&wav_bytes(&pcm, 1, 44100, 24)).unwrap();
        assert!((w.samples[0] + 1.0 / 8388608.0).abs() < 1e-9);
        assert!((w.samples[1] + 1.0).abs() < 1e-6);
    }

    #[test]
    fn float32() {
        let pcm: Vec<u8> = [0.5f32, -0.25]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let mut b = wav_bytes(&pcm, 1, 44100, 32);
        b[20..22].copy_from_slice(&3u16.to_le_bytes()); // format = IEEE float
        let w = parse_wav(&b).unwrap();
        assert_eq!(w.samples, vec![0.5, -0.25]);
    }

    #[test]
    fn extensible_format() {
        // fmt payload with cbSize=22, validBits=16, channelMask, then GUID
        // whose first two bytes are 1 (PCM).
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&0xFFFEu16.to_le_bytes());
        fmt.extend_from_slice(&1u16.to_le_bytes()); // channels
        fmt.extend_from_slice(&44100u32.to_le_bytes());
        fmt.extend_from_slice(&88200u32.to_le_bytes());
        fmt.extend_from_slice(&2u16.to_le_bytes());
        fmt.extend_from_slice(&16u16.to_le_bytes());
        fmt.extend_from_slice(&22u16.to_le_bytes()); // cbSize
        fmt.extend_from_slice(&16u16.to_le_bytes()); // valid bits
        fmt.extend_from_slice(&4u32.to_le_bytes()); // channel mask
        fmt.extend_from_slice(&1u16.to_le_bytes()); // subformat: PCM
        fmt.extend_from_slice(&[0u8; 14]); // rest of GUID
        let pcm = [0x0000i16, 0x4000, 0x0000, -16384] // 0, 0.5, 0, -0.5
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>();
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(4 + 8 + fmt.len() as u32 + 8 + pcm.len() as u32).to_le_bytes());
        b.extend_from_slice(b"WAVE");
        b.extend_from_slice(b"fmt ");
        b.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
        b.extend_from_slice(&fmt);
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
        b.extend_from_slice(&pcm);
        let w = parse_wav(&b).unwrap();
        assert_eq!(w.samples, vec![0.0, 0.5, 0.0, -0.5]);
    }

    #[test]
    fn rejects_non_wav() {
        assert!(matches!(
            parse_wav(b"NOTRIFF.........."),
            Err(WavError::NotRiff)
        ));
        let mut b = wav_bytes(&[0u8; 2], 1, 44100, 16);
        b[8..12].copy_from_slice(b"AVI ");
        assert!(matches!(parse_wav(&b), Err(WavError::NotWave)));
        assert!(matches!(parse_wav(b"RIFF....WAVE"), Err(WavError::MissingFmt)));
    }

    #[test]
    fn unsupported_format() {
        let mut b = wav_bytes(&[0u8; 4], 1, 44100, 16);
        b[20..22].copy_from_slice(&0x0011u16.to_le_bytes()); // ADPCM
        assert!(matches!(
            parse_wav(&b),
            Err(WavError::UnsupportedFormat { .. })
        ));
    }
}
