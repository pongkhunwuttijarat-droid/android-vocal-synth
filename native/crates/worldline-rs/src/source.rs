//! Voicebank sample resolution — the seam that lets the engine run with no
//! filesystem.
//!
//! The native path reads `<voicebank>/voice/*.wav` + `*_wav.frq` from disk.
//! A browser build has no disk: samples arrive from OPFS (or a fetched
//! archive), so the engine takes a [`SampleSource`] instead of calling
//! `std::fs` directly. [`FileSource`] is the native implementation and
//! [`MemorySource`] covers tests and in-browser decoding.

use std::collections::HashMap;
use std::path::Path;

use voicebank::WavData;

/// One resolved voicebank sample.
#[derive(Clone)]
pub struct ResolvedSample {
    /// Decoded wav.
    pub wav: WavData,
    /// Raw `FREQ0003` bytes when the bank ships a `.frq` next to the wav.
    pub frq: Option<Vec<u8>>,
}

/// Resolves a `PhonemeRequest::wav_path` to its sample data.
pub trait SampleSource {
    /// Return the sample for `wav_path`, or an error message.
    fn resolve(&self, wav_path: &str) -> Result<ResolvedSample, String>;
}

/// Native source: read from the filesystem (voicebank layout on disk).
pub struct FileSource;

impl SampleSource for FileSource {
    fn resolve(&self, wav_path: &str) -> Result<ResolvedSample, String> {
        let path = Path::new(wav_path);
        let wav = voicebank::read_wav(path).map_err(|e| format!("read {wav_path}: {e}"))?;
        Ok(ResolvedSample {
            wav,
            frq: read_frq_bytes(path),
        })
    }
}

/// In-memory source: caller pre-loads the samples (tests, wasm/OPFS).
///
/// Keys are the request's `wav_path` strings — the same string the feed puts
/// on `PhonemeRequest`, so the host can key its map by whatever stable id it
/// uses internally (absolute path, relative path, or an OPFS key).
#[derive(Default, Clone)]
pub struct MemorySource {
    samples: HashMap<String, ResolvedSample>,
}

impl MemorySource {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert one decoded sample under `wav_path`.
    pub fn insert(&mut self, wav_path: impl Into<String>, wav: WavData, frq: Option<Vec<u8>>) {
        self.samples
            .insert(wav_path.into(), ResolvedSample { wav, frq });
    }

    /// Insert a sample decoded from raw wav bytes (no `.frq`).
    pub fn insert_wav_bytes(
        &mut self,
        wav_path: impl Into<String>,
        wav_bytes: &[u8],
    ) -> Result<(), String> {
        let wav = voicebank::parse_wav(wav_bytes).map_err(|e| format!("parse wav: {e}"))?;
        self.insert(wav_path, wav, None);
        Ok(())
    }

    /// Whether `wav_path` is already loaded.
    pub fn contains(&self, wav_path: &str) -> bool {
        self.samples.contains_key(wav_path)
    }

    /// Number of loaded samples.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

impl SampleSource for MemorySource {
    fn resolve(&self, wav_path: &str) -> Result<ResolvedSample, String> {
        match self.samples.get(wav_path) {
            Some(s) => Ok(ResolvedSample {
                wav: s.wav.clone(),
                frq: s.frq.clone(),
            }),
            None => Err(format!("sample not loaded: {wav_path}")),
        }
    }
}

/// Read `<stem>_wav.frq` next to `wav` (OpenUtau's naming).
fn read_frq_bytes(path: &Path) -> Option<Vec<u8>> {
    let stem = path.file_stem()?.to_str()?;
    let frq = path.with_file_name(format!("{stem}_wav.frq"));
    let data = std::fs::read(frq).ok()?;
    if data.is_empty() {
        None
    } else {
        Some(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_source_reports_missing_samples() {
        let source = MemorySource::new();
        let err = match source.resolve("/vb/a.wav") {
            Ok(_) => panic!("expected a missing-sample error"),
            Err(e) => e,
        };
        assert!(err.contains("not loaded"), "{err}");
    }

    #[test]
    fn memory_source_round_trips_a_wav() {
        let pcm: Vec<f32> = (0..64).map(|i| (i as f32 / 64.0) - 0.5).collect();
        let bytes = wavwriter::write_wav_16(&pcm, 44100);
        let mut source = MemorySource::new();
        source.insert_wav_bytes("/vb/a.wav", &bytes).unwrap();
        let got = source.resolve("/vb/a.wav").unwrap();
        assert_eq!(got.wav.sample_rate, 44100);
        assert_eq!(got.wav.samples.len(), pcm.len());
        assert!(got.frq.is_none());
    }
}
