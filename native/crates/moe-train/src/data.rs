//! Training-set loading: parse train-prep's `train-set.jsonl` into
//! per-frame samples the baseline acoustic can learn from.
//!
//! Frame model: each mel frame is predicted from (phoneme_id, f0, and
//! the frame's position within its phoneme) — a frame-wise regression
//! that ignores long-range context (the POC baseline; the MoE experts
//! add context later).

use serde::Deserialize;
use std::collections::HashMap;

/// One frame-level training sample.
#[derive(Debug, Clone)]
pub struct FrameSample {
    /// Which source phrase (train-set entry) this frame came from —
    /// used for phrase-level holdout (no leakage between train/val).
    pub phrase_id: usize,
    /// Index of the containing phoneme within the phrase's sequence —
    /// the context window is built around this.
    pub phoneme_pos: usize,
    pub phoneme_id: usize,
    /// Natural-class group (see `phoneme_groups`) — the MoE router key.
    pub group_id: usize,
    /// f0 in Hz normalized to 0..1 (f0 / 500, clamped).
    pub f0_norm: f32,
    /// Frame position within its phoneme, 0..1.
    pub pos_frac: f32,
    /// Teacher mel frame (128 bins).
    pub mel: [f32; 128],
}

/// One source phrase: the phoneme-id sequence (for context windows) and
/// its frames.
#[derive(Debug, Clone)]
pub struct Phrase {
    pub phoneme_ids: Vec<usize>,
    /// Original oto aliases (for selection/display when rendering).
    pub aliases: Vec<String>,
    /// Score tone (MIDI) and note duration (ms) — for the lookup table.
    pub tone: i32,
    pub duration_ms: f64,
}

/// Loaded training set with a phoneme vocabulary.
#[derive(Debug, Clone)]
pub struct TrainSet {
    pub vocab: Vec<String>,
    pub samples: Vec<FrameSample>,
    pub phoneme_id: HashMap<String, usize>,
    pub phrases: Vec<Phrase>,
}

const FRAME_MS: f64 = 11.61;

#[derive(Deserialize)]
struct Entry {
    aliases: Vec<String>,
    #[serde(default)]
    tone: i32,
    #[serde(default)]
    duration_ms: f64,
    phonemes: Vec<Phoneme>,
    frames: usize,
    f0_hz: Vec<f32>,
    mel: Vec<Vec<f32>>,
}

#[derive(Deserialize)]
struct Phoneme {
    phoneme: String,
    position_ms: f64,
    duration_ms: f64,
}

/// Parse the compact ".tsbin" train-set format (written by train-prep
/// next to the JSONL): magic "TSBN", little-endian, entries in order.
fn parse_tsbin(bytes: &[u8]) -> Result<Vec<Entry>, String> {
    fn u16_at(b: &[u8], i: &mut usize) -> Result<u16, String> {
        let v = b
            .get(*i..*i + 2)
            .ok_or_else(|| "tsbin: truncated u16".to_string())?
            .try_into()
            .unwrap();
        *i += 2;
        Ok(u16::from_le_bytes(v))
    }
    fn u32_at(b: &[u8], i: &mut usize) -> Result<u32, String> {
        let v = b
            .get(*i..*i + 4)
            .ok_or_else(|| "tsbin: truncated u32".to_string())?
            .try_into()
            .unwrap();
        *i += 4;
        Ok(u32::from_le_bytes(v))
    }
    fn f32_at(b: &[u8], i: &mut usize) -> Result<f32, String> {
        let v = u32_at(b, i)?;
        Ok(f32::from_bits(v))
    }
    fn f64_at(b: &[u8], i: &mut usize) -> Result<f64, String> {
        let lo = u32_at(b, i)? as u64;
        let hi = u32_at(b, i)? as u64;
        Ok(f64::from_bits(lo | (hi << 32)))
    }
    fn str_at(b: &[u8], i: &mut usize) -> Result<String, String> {
        let len = u16_at(b, i)? as usize;
        let s = b
            .get(*i..*i + len)
            .ok_or_else(|| "tsbin: truncated string".to_string())?;
        *i += len;
        String::from_utf8(s.to_vec()).map_err(|e| format!("tsbin: utf8: {e}"))
    }

    if bytes.len() < 8 || &bytes[..4] != b"TSBN" {
        return Err("tsbin: bad magic".to_string());
    }
    let mut i = 4usize;
    let n = u32_at(bytes, &mut i)? as usize;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let n_aliases = u32_at(bytes, &mut i)? as usize;
        let mut aliases = Vec::with_capacity(n_aliases);
        for _ in 0..n_aliases {
            aliases.push(str_at(bytes, &mut i)?);
        }
        let tone = u32_at(bytes, &mut i)? as i32;
        let duration_ms = f64_at(bytes, &mut i)?;
        let n_phonemes = u32_at(bytes, &mut i)? as usize;
        let mut phonemes = Vec::with_capacity(n_phonemes);
        for _ in 0..n_phonemes {
            phonemes.push(Phoneme {
                phoneme: str_at(bytes, &mut i)?,
                position_ms: f64_at(bytes, &mut i)?,
                duration_ms: f64_at(bytes, &mut i)?,
            });
        }
        let frames = u32_at(bytes, &mut i)? as usize;
        let mut f0_hz = Vec::with_capacity(frames);
        for _ in 0..frames {
            f0_hz.push(f32_at(bytes, &mut i)?);
        }
        let mut mel: Vec<Vec<f32>> = Vec::with_capacity(frames);
        for _ in 0..frames {
            let mut row = Vec::with_capacity(128);
            for _ in 0..128 {
                row.push(f32_at(bytes, &mut i)?);
            }
            mel.push(row);
        }
        out.push(Entry {
            aliases,
            tone,
            duration_ms,
            phonemes,
            frames,
            f0_hz,
            mel,
        });
    }
    Ok(out)
}

/// Load a train-set (`.jsonl` from train-prep, or its compact `.tsbin`
/// sibling — auto-detected by the "TSBN" magic) into frame samples.
pub fn load_train_set(path: &str) -> Result<TrainSet, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {path}: {e}"))?;
    let entries: Vec<Entry> = if bytes.starts_with(b"TSBN") {
        parse_tsbin(&bytes)?
    } else {
        let text = String::from_utf8_lossy(&bytes);
        let mut v = Vec::new();
        for (i, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            v.push(serde_json::from_str(line).map_err(|e| format!("line {}: {e}", i + 1))?);
        }
        v
    };
    let mut vocab: Vec<String> = Vec::new();
    let mut phoneme_id: HashMap<String, usize> = HashMap::new();
    let mut samples: Vec<FrameSample> = Vec::new();
    let mut phrases: Vec<Phrase> = Vec::new();
    let mut phrase_id = 0usize;

    for (i, entry) in entries.iter().enumerate() {
        if entry.frames != entry.f0_hz.len() || entry.frames != entry.mel.len() {
            return Err(format!(
                "entry {i}: frames={} but f0={} mel={}",
                entry.frames,
                entry.f0_hz.len(),
                entry.mel.len()
            ));
        }
        // Phrase phoneme sequence (for context windows).
        let mut phoneme_ids: Vec<usize> = Vec::with_capacity(entry.phonemes.len());
        let mut aliases: Vec<String> = Vec::new();
        for p in &entry.phonemes {
            let id = *phoneme_id.entry(p.phoneme.clone()).or_insert_with(|| {
                vocab.push(p.phoneme.clone());
                vocab.len() - 1
            });
            phoneme_ids.push(id);
        }
        aliases.extend(entry.aliases.iter().cloned());
        for f in 0..entry.frames {
            let time_ms = f as f64 * FRAME_MS;
            let (pidx, pfrac) = locate_phoneme(&entry.phonemes, time_ms);
            let phoneme_id = phoneme_ids[pidx];
            let mel: [f32; 128] = entry.mel[f][..128]
                .try_into()
                .map_err(|_| format!("entry {i}: mel row not 128"))?;
            samples.push(FrameSample {
                phrase_id,
                phoneme_pos: pidx,
                phoneme_id,
                group_id: crate::phoneme_groups::group_of(&entry.phonemes[pidx].phoneme),
                f0_norm: (entry.f0_hz[f] / 500.0).clamp(0.0, 1.0),
                pos_frac: pfrac as f32,
                mel,
            });
        }
        phrases.push(Phrase {
            phoneme_ids,
            aliases,
            tone: entry.tone,
            duration_ms: entry.duration_ms,
        });
        phrase_id += 1;
    }
    if vocab.len() >= crate::baseline::EMBED_SLOTS {
        return Err(format!(
            "{path}: vocab has {} symbols but embedding capacity is {}",
            vocab.len(),
            crate::baseline::EMBED_SLOTS - 1
        ));
    }
    if samples.is_empty() {
        return Err(format!("{path}: no samples loaded"));
    }
    Ok(TrainSet {
        vocab,
        samples,
        phoneme_id,
        phrases,
    })
}

/// Which phoneme contains `time_ms`; returns (index, in-phoneme
/// fraction 0..1). Falls back to the nearest phoneme when outside all
/// ranges (e.g. the leading silence before the first phoneme).
fn locate_phoneme(phonemes: &[Phoneme], time_ms: f64) -> (usize, f64) {
    for (i, p) in phonemes.iter().enumerate() {
        let start = p.position_ms;
        let end = p.position_ms + p.duration_ms;
        if time_ms >= start && time_ms < end {
            let frac = ((time_ms - start) / p.duration_ms).clamp(0.0, 1.0);
            return (i, frac);
        }
    }
    let mut best = 0usize;
    let mut best_d = f64::INFINITY;
    for (i, p) in phonemes.iter().enumerate() {
        let start = p.position_ms;
        let end = p.position_ms + p.duration_ms;
        let d = if time_ms < start {
            start - time_ms
        } else {
            time_ms - end
        };
        if d < best_d {
            best_d = d;
            best = i;
        }
    }
    let p = &phonemes[best];
    let frac = ((time_ms - p.position_ms) / p.duration_ms.max(1.0)).clamp(0.0, 1.0);
    (best, frac)
}
