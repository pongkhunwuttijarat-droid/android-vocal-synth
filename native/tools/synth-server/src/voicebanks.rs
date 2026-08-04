//! Voicebank discovery: scan the configured root for voicebanks and
//! produce the metadata `GET /voicebanks` reports.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use voicebank::load_voicebank as load_vb;
use voicebank::Voicebank;

/// One discovered voicebank, as listed by `GET /voicebanks`.
#[derive(Debug, Clone, Serialize)]
pub struct VoicebankInfo {
    /// Display name (character.txt `name=`, falling back to the dir name).
    pub name: String,
    /// Directory name (or path id) — also accepted as the `voicebank`
    /// request field.
    pub dir: String,
    /// Distinct oto aliases in voice/oto.ini.
    pub aliases_count: usize,
    /// Distinct wav files referenced by valid oto entries.
    pub wav_count: usize,
    /// Sample rate of the bank's first readable wav, when available.
    pub samples_rate: Option<u32>,
    /// Library dir (skipped in JSON).
    #[serde(skip)]
    pub path: PathBuf,
}

/// A discovered voicebank plus its loaded [`Voicebank`].
#[derive(Debug)]
pub struct VoicebankEntry {
    pub info: VoicebankInfo,
    pub bank: Arc<Voicebank>,
}

/// Result of a [`scan_voicebanks`] pass.
#[derive(Debug)]
pub struct ScanResult {
    /// Loaded voicebanks, sorted by name.
    pub entries: Vec<VoicebankEntry>,
    /// Non-fatal issues (subdirectories that failed to load, ...).
    pub warnings: Vec<String>,
}

impl VoicebankInfo {
    fn from_vb(vb: &Voicebank) -> Self {
        let mut aliases: HashSet<&str> = HashSet::new();
        let mut wavs: HashSet<&str> = HashSet::new();
        for oto in &vb.otos {
            if oto.is_valid {
                aliases.insert(oto.alias.as_str());
                wavs.insert(oto.wav.as_str());
            }
        }
        let samples_rate = vb
            .otos
            .iter()
            .find(|oto| oto.is_valid)
            .and_then(|first| vb.read_wav(first).ok())
            .map(|wav| wav.sample_rate);
        let dir = vb
            .path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| vb.id.clone());
        VoicebankInfo {
            name: if vb.name.is_empty() {
                dir.clone()
            } else {
                vb.name.clone()
            },
            dir,
            aliases_count: aliases.len(),
            wav_count: wavs.len(),
            samples_rate,
            path: vb.path.clone(),
        }
    }
}

/// Scan `root` for voicebanks.
///
/// If `root` itself is a voicebank (it contains `character.txt`), it is
/// returned as the only entry — so both a container directory
/// (`--voicebanks <dir with bank subdirs>`) and a single bank directory
/// (`--voicebanks .../library`) work. Otherwise every subdirectory that
/// loads as a voicebank is listed.
pub fn scan_voicebanks(root: &Path) -> Result<ScanResult, String> {
    if !root.is_dir() {
        return Err(format!(
            "voicebanks root {} is not a directory",
            root.display()
        ));
    }
    let candidates: Vec<PathBuf> = if root.join("character.txt").is_file() {
        vec![root.to_path_buf()]
    } else {
        let mut dirs = Vec::new();
        for entry in std::fs::read_dir(root)
            .map_err(|e| format!("read_dir {}: {e}", root.display()))?
        {
            let entry = entry.map_err(|e| format!("read_dir entry: {e}"))?;
            if entry.path().is_dir() {
                dirs.push(entry.path());
            }
        }
        dirs
    };

    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    for dir in candidates {
        match load_vb(&dir) {
            Ok(vb) => entries.push(VoicebankEntry {
                info: VoicebankInfo::from_vb(&vb),
                bank: Arc::new(vb),
            }),
            Err(e) => warnings.push(format!("skip {}: {e}", dir.display())),
        }
    }
    entries.sort_by(|a, b| a.info.name.cmp(&b.info.name));
    Ok(ScanResult { entries, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_data_dir() -> PathBuf {
        // tools/synth-server → native/test-data (two levels up)
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data")
    }

    #[test]
    fn scan_finds_mock_bank_in_container_dir() {
        let scan = scan_voicebanks(&test_data_dir()).expect("scan test-data");
        let mock = scan
            .entries
            .iter()
            .find(|e| e.info.dir == "mock-voicebank")
            .expect("mock-voicebank listed");
        assert_eq!(mock.info.name, "Teto Mock");
        assert_eq!(mock.info.aliases_count, 19);
        assert_eq!(mock.info.wav_count, 3);
        assert_eq!(mock.info.samples_rate, Some(44100));
    }

    #[test]
    fn scan_accepts_a_single_bank_dir_as_root() {
        let scan =
            scan_voicebanks(&test_data_dir().join("mock-voicebank")).expect("scan bank root");
        assert_eq!(scan.entries.len(), 1);
        assert_eq!(scan.entries[0].info.aliases_count, 19);
        assert_eq!(scan.entries[0].info.name, "Teto Mock");
    }

    #[test]
    fn scan_missing_root_is_error() {
        assert!(scan_voicebanks(Path::new("/nonexistent/synth-server-test")).is_err());
    }
}
