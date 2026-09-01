//! DiffSinger lyric dictionary — the bank's dsdict (romaji → phonemes).
//! The C# `DiffSingerChinesePhonemizer` tokenizes through this dict (with
//! a letter-split fallback); mirroring it makes our token stream IDENTICAL
//! to the C# reference (the case-inconsistent o/O entries included), so
//! the deterministic acoustic model sees the same tokens.
//!
//! Formats: legacy `dsdict.txt` (tab-separated "romaji\tph ph ph") and the
//! modern G2pDictionary yaml (`dsdict-zh.yaml` — symbols + entries).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// romaji → phoneme list (the C# g2p entries).
#[derive(Debug, Default, Clone)]
pub struct LyricDict {
    entries: HashMap<String, Vec<String>>,
    /// Vowel symbols (the yaml `symbols` entries with type "vowel",
    /// prefixed as stored — "cn/a"); the C# grouping splits on vowels.
    vowels: Vec<String>,
}

impl LyricDict {
    /// Load ALL the bank's dsdict files merged — the base dsdict.yaml
    /// first, then each language dict (dsdict-<lang>.yaml) so later
    /// (more specific) entries override — mirroring the C# preference
    /// for the phonemizer's language dict. The modern banks' entries
    /// carry the language prefix already ("sa" → [cn/s, cn/a]).
    pub fn load(dir: &Path) -> Result<Self, String> {
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                let lower = name.to_lowercase();
                if lower.starts_with("dsdict") && (lower.ends_with(".yaml") || lower.ends_with(".txt")) {
                    candidates.push(e.path());
                }
            }
        }
        // Base dsdict.yaml first, then language dicts alphabetically
        // (dsdict-en/ja/zh...) — the base entries stay unless a language
        // dict overrides them.
        candidates.sort_by(|a, b| {
            let rank = |p: &PathBuf| -> u8 {
                let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
                if name == "dsdict.yaml" { 0 } else if name == "dsdict.txt" { 1 } else { 2 }
            };
            rank(a).cmp(&rank(b)).then_with(|| a.cmp(b))
        });
        let mut entries: HashMap<String, Vec<String>> = HashMap::new();
        let mut vowels: Vec<String> = Vec::new();
        let mut found = false;
        for path in &candidates {
            if let Ok(mut dict) = load_file(path) {
                found = true;
                entries.extend(dict.entries.drain());
                vowels.extend(dict.vowels);
            }
        }
        if !found {
            return Ok(Self::default());
        }
        vowels.sort();
        vowels.dedup();
        Ok(LyricDict { entries, vowels })
    }

    pub fn lookup(&self, word: &str) -> Option<&Vec<String>> {
        self.entries.get(word)
    }

    /// The vowel symbols (prefixed, e.g. "cn/a") — the C# word-division
    /// splits the phoneme stream on vowels.
    pub fn vowels(&self) -> &[String] {
        &self.vowels
    }
}

fn load_file(path: &Path) -> Result<LyricDict, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("read {}: {e}", path.display()))?;
    let is_yaml = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase() == "yaml")
        .unwrap_or(false);
    let mut entries = HashMap::new();
    let mut vowels: Vec<String> = Vec::new();
    if is_yaml {
        // G2pDictionary: {symbols: [{symbol, type}], entries: [{grapheme, phonemes}]}
        #[derive(serde::Deserialize)]
        struct G2pSymbol {
            symbol: String,
            #[serde(default)]
            r#type: String,
        }
        #[derive(serde::Deserialize)]
        struct G2pEntry {
            grapheme: String,
            phonemes: Vec<String>,
        }
        #[derive(serde::Deserialize)]
        struct G2pData {
            #[serde(default)]
            symbols: Vec<G2pSymbol>,
            #[serde(default)]
            entries: Vec<G2pEntry>,
        }
        let data: G2pData = serde_yaml::from_str(&text)
            .map_err(|e| format!("parse {}: {e}", path.display()))?;
        for s in data.symbols {
            let t = s.r#type.to_lowercase();
            if t == "vowel" || t == "v" {
                vowels.push(s.symbol);
            }
        }
        for e in data.entries {
            if !e.phonemes.is_empty() {
                entries.insert(e.grapheme, e.phonemes);
            }
        }
    } else {
        // Legacy dsdict.txt: "romaji\tph ph ph" per line.
        for line in text.lines() {
            let line = line.trim_end_matches('\r');
            if let Some((word, phs)) = line.split_once('\t') {
                let phs: Vec<String> = phs.split_whitespace().map(|s| s.to_string()).collect();
                if !phs.is_empty() {
                    entries.insert(word.to_string(), phs);
                }
            }
        }
    }
    if entries.is_empty() && vowels.is_empty() {
        return Err(format!("no entries in {}", path.display()));
    }
    Ok(LyricDict { entries, vowels })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_txt_format() {
        let dir = std::env::temp_dir().join("ds-dict-tests-legacy");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("dsdict.txt"), "sa\ts a\nno\tn O\n").unwrap();
        let d = LyricDict::load(&dir).unwrap();
        assert_eq!(d.lookup("sa").unwrap(), &vec!["s".to_string(), "a".to_string()]);
        assert_eq!(d.lookup("no").unwrap(), &vec!["n".to_string(), "O".to_string()]);
        assert!(d.lookup("ra").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn modern_yaml_format() {
        let dir = std::env::temp_dir().join("ds-dict-tests-yaml");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("dsdict-zh.yaml"),
            "symbols:\n- {symbol: s, type: consonant}\nentries:\n- {grapheme: sa, phonemes: [s, a]}\n",
        )
        .unwrap();
        let d = LyricDict::load(&dir).unwrap();
        assert_eq!(d.lookup("sa").unwrap(), &vec!["s".to_string(), "a".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
