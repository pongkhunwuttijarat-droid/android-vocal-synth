//! `HashKey` implementations for feed's render input types — enables the
//! runtime chunker/cache to key chunks by the full input that determines
//! the rendered audio (phonemes, oto entries, pitch grid, curves, timing).
//! Two inputs with the same hash render identical audio.
//!
//! Canonical encoding rules:
//! * strings: length-prefixed UTF-8 (length as u64 LE)
//! * f64: raw little-endian bits (deterministic; no float formatting)
//! * enums: discriminant then payload

use std::io::Write;

use feed::render_input::{
    Curves, EnvelopePoint, NeuralInput, OtoEntry, PhraseInfo, RenderInput, RenderNote,
    RenderPhoneme, SampleBased,
};
use crate::chunker::HashKey;

impl HashKey for PhraseInfo {
    fn write_hash(&self, out: &mut dyn Write) -> std::io::Result<()> {
        out.write_all(&self.position_ms.to_le_bytes())?;
        out.write_all(&self.duration_ms.to_le_bytes())?;
        out.write_all(&self.leading_ms.to_le_bytes())?;
        out.write_all(&self.leading_ticks.to_le_bytes())?;
        match &self.time_axis_hint {
            Some(h) => {
                out.write_all(&1u8.to_le_bytes())?;
                h.write_hash(out)?;
            }
            None => out.write_all(&0u8.to_le_bytes())?,
        }
        Ok(())
    }
}

impl HashKey for RenderNote {
    fn write_hash(&self, out: &mut dyn Write) -> std::io::Result<()> {
        self.lyric.write_hash(out)?;
        out.write_all(&self.tone.to_le_bytes())?;
        out.write_all(&self.position_ms.to_le_bytes())?;
        out.write_all(&self.duration_ms.to_le_bytes())?;
        Ok(())
    }
}

impl HashKey for RenderPhoneme {
    fn write_hash(&self, out: &mut dyn Write) -> std::io::Result<()> {
        self.phoneme.write_hash(out)?;
        out.write_all(&self.position_ms.to_le_bytes())?;
        out.write_all(&self.duration_ms.to_le_bytes())?;
        out.write_all(&self.leading_ms.to_le_bytes())?;
        out.write_all(&self.overlap_ms.to_le_bytes())?;
        out.write_all(&self.tone.to_le_bytes())?;
        out.write_all(&self.tone_shift.to_le_bytes())?;
        out.write_all(&self.index.to_le_bytes())?;
        out.write_all(&self.parent_note.to_le_bytes())?;
        Ok(())
    }
}

impl HashKey for OtoEntry {
    fn write_hash(&self, out: &mut dyn Write) -> std::io::Result<()> {
        self.alias.write_hash(out)?;
        self.file.write_hash(out)?;
        self.wav_path.write_hash(out)?;
        out.write_all(&self.offset.to_le_bytes())?;
        out.write_all(&self.consonant.to_le_bytes())?;
        out.write_all(&self.cutoff.to_le_bytes())?;
        out.write_all(&self.preutter.to_le_bytes())?;
        out.write_all(&self.overlap.to_le_bytes())?;
        for e in &self.envelope {
            e.write_hash(out)?;
        }
        self.flags.write_hash(out)?;
        Ok(())
    }
}

impl HashKey for EnvelopePoint {
    fn write_hash(&self, out: &mut dyn Write) -> std::io::Result<()> {
        out.write_all(&self.x_ms.to_le_bytes())?;
        out.write_all(&self.y.to_le_bytes())?;
        Ok(())
    }
}

impl HashKey for SampleBased {
    fn write_hash(&self, out: &mut dyn Write) -> std::io::Result<()> {
        for o in &self.oto {
            o.write_hash(out)?;
        }
        match &self.wav_path {
            Some(p) => {
                out.write_all(&1u8.to_le_bytes())?;
                p.write_hash(out)?;
            }
            None => out.write_all(&0u8.to_le_bytes())?,
        }
        for e in &self.envelope {
            e.write_hash(out)?;
        }
        self.flags.write_hash(out)?;
        Ok(())
    }
}

impl HashKey for NeuralInput {
    fn write_hash(&self, out: &mut dyn Write) -> std::io::Result<()> {
        for t in &self.tokens {
            out.write_all(&t.to_le_bytes())?;
        }
        for d in &self.durations_frames {
            out.write_all(&d.to_le_bytes())?;
        }
        for f in &self.f0_hz {
            out.write_all(&f.to_le_bytes())?;
        }
        for f in &self.shifted_f0_hz {
            out.write_all(&f.to_le_bytes())?;
        }
        Ok(())
    }
}

impl HashKey for Curves {
    fn write_hash(&self, out: &mut dyn Write) -> std::io::Result<()> {
        for v in [&self.dynamics, &self.gender, &self.breathiness, &self.tension, &self.voicing] {
            out.write_all(&(v.len() as u64).to_le_bytes())?;
            for x in v {
                out.write_all(&x.to_le_bytes())?;
            }
        }
        out.write_all(&(self.extra.len() as u64).to_le_bytes())?;
        for c in &self.extra {
            c.abbr.write_hash(out)?;
            out.write_all(&(c.values.len() as u64).to_le_bytes())?;
            for x in &c.values {
                out.write_all(&x.to_le_bytes())?;
            }
        }
        Ok(())
    }
}

impl HashKey for RenderInput {
    fn write_hash(&self, out: &mut dyn Write) -> std::io::Result<()> {
        self.phrase.write_hash(out)?;
        out.write_all(&(self.notes.len() as u64).to_le_bytes())?;
        for n in &self.notes {
            n.write_hash(out)?;
        }
        out.write_all(&(self.phonemes.len() as u64).to_le_bytes())?;
        for p in &self.phonemes {
            p.write_hash(out)?;
        }
        out.write_all(&(self.pitches_cents.len() as u64).to_le_bytes())?;
        for p in &self.pitches_cents {
            out.write_all(&p.to_le_bytes())?;
        }
        self.curves.write_hash(out)?;
        match &self.sample_based {
            Some(sb) => {
                out.write_all(&1u8.to_le_bytes())?;
                sb.write_hash(out)?;
            }
            None => out.write_all(&0u8.to_le_bytes())?,
        }
        match &self.neural {
            Some(nn) => {
                out.write_all(&1u8.to_le_bytes())?;
                nn.write_hash(out)?;
            }
            None => out.write_all(&0u8.to_le_bytes())?,
        }
        Ok(())
    }
}

/// Test helper: canonical bytes for this input.
#[cfg(test)]
trait HashBytes {
    fn write_hash_bytes(&self) -> Vec<u8>;
}

#[cfg(test)]
impl HashBytes for RenderInput {
    fn write_hash_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.write_hash(&mut out).unwrap();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use feed::render_input::{OtoEntry, PhraseInfo, RenderInput, RenderPhoneme, SampleBased};

    fn base_input() -> RenderInput {
        RenderInput {
            phrase: PhraseInfo {
                position_ms: 0.0,
                duration_ms: 500.0,
                leading_ms: 20.0,
                leading_ticks: 96,
                time_axis_hint: None,
            },
            notes: vec![],
            phonemes: vec![RenderPhoneme {
                phoneme: "3 h3".into(),
                position_ms: 0.0,
                duration_ms: 500.0,
                leading_ms: 20.0,
                overlap_ms: 0.0,
                tone: 60,
                tone_shift: 0,
                index: 0,
                parent_note: 0,
            }],
            pitches_cents: vec![0; 25],
            curves: Curves::default(),
            sample_based: Some(SampleBased {
                oto: vec![OtoEntry {
                    alias: "3 h3".into(),
                    file: "3.wav".into(),
                    wav_path: "/v/3.wav".into(),
                    offset: 120.0,
                    consonant: 92.0,
                    cutoff: 0.0,
                    preutter: 20.0,
                    overlap: 40.0,
                    envelope: vec![],
                    flags: "g0B0".into(),
                }],
                wav_path: Some("/v/3.wav".into()),
                envelope: vec![],
                flags: "g0B0".into(),
            }),
            neural: None,
        }
    }

    #[test]
    fn deterministic_same_input() {
        assert_eq!(base_input().write_hash_bytes(), base_input().write_hash_bytes());
    }

    #[test]
    fn differs_on_phoneme() {
        let mut b = base_input();
        b.phonemes[0].tone = 62;
        assert_ne!(base_input().write_hash_bytes(), b.write_hash_bytes());
    }

    #[test]
    fn differs_on_pitch_grid() {
        let mut b = base_input();
        b.pitches_cents[10] = 42;
        assert_ne!(base_input().write_hash_bytes(), b.write_hash_bytes());
    }

    #[test]
    fn differs_on_oto() {
        let mut b = base_input();
        b.sample_based.as_mut().unwrap().oto[0].preutter = 30.0;
        assert_ne!(base_input().write_hash_bytes(), b.write_hash_bytes());
    }
}
