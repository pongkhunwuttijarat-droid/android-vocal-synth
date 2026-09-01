//! Phoneme table loading — port of `DiffSingerUtils.LoadPhonemes`
//! (C# DiffSingerUtils.cs:311-330): a `.json` file maps phoneme → token
//! directly; a `.txt` file lists phonemes one per line (token = line
//! index, starting at 0).

use std::collections::HashMap;
use std::path::Path;

/// Load the phoneme → token map (token = the index fed to the acoustic
/// model's `tokens` input).
pub fn load_phonemes(path: &Path) -> Result<HashMap<String, i64>, String> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "json" => load_phonemes_json(path),
        _ => load_phonemes_txt(path),
    }
}

fn load_phonemes_json(path: &Path) -> Result<HashMap<String, i64>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str(&text)
        .map_err(|e| format!("parse {}: {e}", path.display()))
}

fn load_phonemes_txt(path: &Path) -> Result<HashMap<String, i64>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut map = HashMap::new();
    for (i, line) in text.lines().enumerate() {
        let phoneme = line.trim();
        if phoneme.is_empty() || phoneme.starts_with('#') {
            continue;
        }
        map.insert(phoneme.to_string(), i as i64);
    }
    if map.is_empty() {
        return Err(format!("no phonemes parsed from {}", path.display()));
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_txt_table() {
        let dir = std::env::temp_dir().join("diffsinger-tests-txt");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("phonemes.txt");
        std::fs::write(&p, "SP\nAP\nla\nl A\nA\n").unwrap();
        let map = load_phonemes(&p).unwrap();
        assert_eq!(map["SP"], 0);
        assert_eq!(map["AP"], 1);
        assert_eq!(map["la"], 2);
        assert_eq!(map["l A"], 3);
        assert_eq!(map["A"], 4);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn loads_json_table() {
        let dir = std::env::temp_dir().join("diffsinger-tests-json");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("phonemes.json");
        std::fs::write(&p, r#"{"SP":0,"A":1,"l":2}"#).unwrap();
        let map = load_phonemes(&p).unwrap();
        assert_eq!(map["l"], 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
