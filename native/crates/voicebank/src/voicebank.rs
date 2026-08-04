//! Voicebank loading: character.txt / character.yaml / prefix.map / oto.ini
//! assembly, mirroring OpenUtau's `VoicebankLoader.LoadVoicebank` pipeline:
//!
//! 1. Load character.yaml (if present) and character.txt (Shift-JIS aware).
//! 2. Apply yaml overrides on top of character.txt.
//! 3. Build subbanks from yaml, else from prefix.map (+ prefix/*.map),
//!    else one default empty subbank.
//! 4. Find every oto.ini below the library dir, parse it, verify referenced
//!    wav files exist, and add filename aliases for unreferenced wavs
//!    (and optionally `use_filename_as_alias` entries).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use encoding_rs::Encoding;

use crate::config::{parse_character_txt, parse_character_yaml, CharacterTxt, Subbank, VoicebankConfigYaml};
use crate::frq::{read_frq_for_wav, FrqData, FrqError};
use crate::oto::{parse_oto_ini, Oto, OtoSet};
use crate::prefix_map::parse_prefix_map;
use crate::wav::{read_wav, WavData, WavError};

/// Tuning of the voicebank loader.
#[derive(Debug, Clone, Default)]
pub struct LoadOptions {
    /// Override `use_filename_as_alias` from character.yaml (`None` = follow
    /// the yaml; `Some(true)` additionally registers each wav's filename as
    /// an alias).
    pub use_filename_as_alias: Option<bool>,
}

#[derive(Debug, thiserror::Error)]
pub enum VoicebankError {
    #[error("voicebank library not found: {0}")]
    NotFound(PathBuf),
    #[error("no character.txt in {0}")]
    MissingCharacterTxt(PathBuf),
    #[error("character.yaml parse error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// A loaded UTAU/OpenUtau voicebank.
#[derive(Debug, Clone)]
pub struct Voicebank {
    /// Display name (from character.txt / character.yaml).
    pub name: String,
    /// Root path passed to the loader (parent of the library dir).
    pub base_path: PathBuf,
    /// Library dir containing character.txt and voice/.
    pub path: PathBuf,
    /// Relative id of the library within `base_path`.
    pub id: String,

    pub image: Option<String>,
    pub portrait: Option<String>,
    pub portrait_opacity: f32,
    pub portrait_height: i32,
    pub author: Option<String>,
    pub voice: Option<String>,
    pub web: Option<String>,
    pub version: Option<String>,
    pub sample: Option<String>,
    pub default_phonemizer: Option<String>,
    /// Lines of character.txt that matched no known key.
    pub other_info: String,
    pub localized_names: HashMap<String, String>,
    /// Resolved encoding used for all text files.
    pub text_file_encoding: &'static Encoding,
    pub singer_type: String,
    /// `use_filename_as_alias` from character.yaml (or load override).
    pub use_filename_as_alias: Option<bool>,

    /// Subbanks from character.yaml or prefix.map (never empty; a default
    /// empty subbank is added when neither exists).
    pub subbanks: Vec<Subbank>,
    /// All oto sets (one per oto.ini), in discovery order.
    pub oto_sets: Vec<OtoSet>,
    /// All valid otos flattened across sets.
    pub otos: Vec<Oto>,
    /// Alias -> oto (first occurrence wins on duplicate aliases).
    pub oto_map: HashMap<String, Oto>,
    /// Non-fatal issues encountered while loading (parse errors, missing
    /// wavs, ...).
    pub warnings: Vec<String>,
}

impl Voicebank {
    /// Look up a phoneme at a tone (note number, C4 = 60), applying
    /// prefix.map / subbank prefixes and suffixes, then falling back to the
    /// bare phoneme. Mirrors `ClassicSinger.TryGetMappedOto`.
    pub fn lookup(&self, phoneme: &str, tone: i32) -> Option<&Oto> {
        let subbank = self
            .subbanks
            .iter()
            .find(|s| s.color.is_empty() && s.contains_tone(tone));
        if let Some(sb) = subbank {
            let alias = format!("{}{}{}", sb.prefix, phoneme, sb.suffix);
            if let Some(oto) = self.oto_map.get(&alias) {
                return Some(oto);
            }
        }
        self.oto_map.get(phoneme)
    }

    /// Color-aware lookup: try the subbank with `color` covering `tone`,
    /// then fall back to [`Voicebank::lookup`].
    pub fn lookup_with_color(&self, phoneme: &str, tone: i32, color: &str) -> Option<&Oto> {
        let subbank = self
            .subbanks
            .iter()
            .find(|s| s.color == color && s.contains_tone(tone));
        if let Some(sb) = subbank {
            let alias = format!("{}{}{}", sb.prefix, phoneme, sb.suffix);
            if let Some(oto) = self.oto_map.get(&alias) {
                return Some(oto);
            }
        }
        self.lookup(phoneme, tone)
    }

    /// Bare phoneme lookup with no tone mapping.
    pub fn lookup_plain(&self, phoneme: &str) -> Option<&Oto> {
        self.oto_map.get(phoneme)
    }

    /// Tone-name variant of [`Voicebank::lookup`] (`"C4"` etc.).
    pub fn lookup_tone_name(&self, phoneme: &str, tone_name: &str) -> Option<&Oto> {
        self.lookup(phoneme, crate::tone::name_to_tone(tone_name)?)
    }

    /// Absolute path of the wav file behind `oto`.
    pub fn wav_path(&self, oto: &Oto) -> PathBuf {
        match self.oto_sets.get(oto.set_index) {
            Some(set) => set.wav_path(&oto.wav),
            None => self.path.join(&oto.wav),
        }
    }

    /// Read and decode the sample behind `oto`.
    pub fn read_wav(&self, oto: &Oto) -> Result<WavData, WavError> {
        read_wav(&self.wav_path(oto))
    }

    /// Read the companion frq pitch file behind `oto`.
    pub fn read_frq(&self, oto: &Oto) -> Result<FrqData, FrqError> {
        read_frq_for_wav(&self.wav_path(oto))
    }
}

/// Load a voicebank from its library dir (the folder containing
/// character.txt), with default options.
pub fn load_voicebank(library: &Path) -> Result<Voicebank, VoicebankError> {
    load_voicebank_with_options(library, &LoadOptions::default())
}

/// Load a voicebank from its library dir, with options.
pub fn load_voicebank_with_options(
    library: &Path,
    opts: &LoadOptions,
) -> Result<Voicebank, VoicebankError> {
    if !library.is_dir() {
        return Err(VoicebankError::NotFound(library.to_path_buf()));
    }
    let char_txt_path = library.join("character.txt");
    if !char_txt_path.is_file() {
        return Err(VoicebankError::MissingCharacterTxt(library.to_path_buf()));
    }

    let base_path = library
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(library)
        .to_path_buf();
    let id = relative_path(&base_path, library);

    let mut vb = Voicebank {
        name: String::new(),
        base_path,
        path: library.to_path_buf(),
        id,
        image: None,
        portrait: None,
        portrait_opacity: 0.0,
        portrait_height: 0,
        author: None,
        voice: None,
        web: None,
        version: None,
        sample: None,
        default_phonemizer: None,
        other_info: String::new(),
        localized_names: HashMap::new(),
        text_file_encoding: encoding_rs::SHIFT_JIS,
        singer_type: String::new(),
        use_filename_as_alias: None,
        subbanks: Vec::new(),
        oto_sets: Vec::new(),
        otos: Vec::new(),
        oto_map: HashMap::new(),
        warnings: Vec::new(),
    };

    // 1. character.yaml (modern metadata, UTF-8).
    let yaml_path = library.join("character.yaml");
    let yaml_cfg: Option<VoicebankConfigYaml> = if yaml_path.is_file() {
        let text = std::fs::read_to_string(&yaml_path)?;
        match parse_character_yaml(text.as_bytes()) {
            Ok(cfg) => Some(cfg),
            Err(e) => {
                vb.warnings.push(format!("character.yaml parse failed: {e}"));
                None
            }
        }
    } else {
        None
    };
    let declared = yaml_cfg
        .as_ref()
        .and_then(|c| c.text_file_encoding.clone());

    // 2. character.txt (legacy, often Shift-JIS).
    let bytes = std::fs::read(&char_txt_path)?;
    let txt = parse_character_txt(&bytes, declared.as_deref());
    apply_character_txt(&mut vb, &txt);
    if let Some(enc) = declared.as_deref().and_then(|l| Encoding::for_label(l.as_bytes())) {
        vb.text_file_encoding = enc;
    }

    // 3. character.yaml overrides.
    if let Some(cfg) = &yaml_cfg {
        apply_yaml(&mut vb, cfg);
    }

    // 4. Subbanks: yaml wins; else prefix.map (+ prefix/*.map); else default.
    if vb.subbanks.is_empty() {
        let pm_path = library.join("prefix.map");
        if pm_path.is_file() {
            let bytes = std::fs::read(&pm_path)?;
            vb.subbanks
                .extend(parse_prefix_map(&bytes, "", declared.as_deref()));
        }
        let prefix_dir = library.join("prefix");
        if prefix_dir.is_dir() {
            let mut names: Vec<PathBuf> = std::fs::read_dir(&prefix_dir)?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|e| e == "map"))
                .collect();
            names.sort();
            for p in names {
                let color = p
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let bytes = std::fs::read(&p)?;
                vb.subbanks
                    .extend(parse_prefix_map(&bytes, &color, declared.as_deref()));
            }
        }
        if vb.subbanks.is_empty() {
            vb.subbanks.push(Subbank::default());
        }
    }

    // 5. Oto sets (recursive oto.ini discovery).
    load_oto_sets(&mut vb, library, opts)?;

    // 6. Flatten valid otos into the map.
    for (set_idx, set) in vb.oto_sets.iter().enumerate() {
        for oto in &set.otos {
            if !oto.is_valid {
                continue;
            }
            let mut oto = oto.clone();
            oto.set_index = set_idx;
            vb.oto_map
                .entry(oto.alias.clone())
                .or_insert_with(|| oto.clone());
            vb.otos.push(oto);
        }
    }

    // 7. Name fallback.
    if vb.name.trim().is_empty() {
        vb.name = format!("No Name ({})", vb.id);
    }

    Ok(vb)
}

fn apply_character_txt(vb: &mut Voicebank, txt: &CharacterTxt) {
    vb.name = txt.name.clone().unwrap_or_default();
    vb.image = txt.image.clone();
    vb.author = txt.author.clone();
    vb.voice = txt.voice.clone();
    vb.sample = txt.sample.clone();
    vb.web = txt.web.clone();
    vb.version = txt.version.clone();
    vb.other_info = txt.other_info.clone();
}

fn apply_yaml(vb: &mut Voicebank, cfg: &VoicebankConfigYaml) {
    if let Some(v) = &cfg.name {
        if !v.trim().is_empty() {
            vb.name = v.clone();
        }
    }
    for (k, v) in &cfg.localized_names {
        vb.localized_names.insert(k.clone(), v.clone());
    }
    if let Some(v) = &cfg.image {
        if !v.trim().is_empty() {
            vb.image = Some(v.clone());
        }
    }
    if let Some(v) = &cfg.portrait {
        if !v.trim().is_empty() {
            vb.portrait = Some(v.clone());
        }
    }
    if let Some(v) = cfg.portrait_opacity {
        vb.portrait_opacity = v;
    }
    if let Some(v) = cfg.portrait_height {
        vb.portrait_height = v;
    }
    if let Some(v) = &cfg.author {
        if !v.trim().is_empty() {
            vb.author = Some(v.clone());
        }
    }
    if let Some(v) = &cfg.voice {
        if !v.trim().is_empty() {
            vb.voice = Some(v.clone());
        }
    }
    if let Some(v) = &cfg.web {
        if !v.trim().is_empty() {
            vb.web = Some(v.clone());
        }
    }
    if let Some(v) = &cfg.version {
        if !v.trim().is_empty() {
            vb.version = Some(v.clone());
        }
    }
    if let Some(v) = &cfg.sample {
        if !v.trim().is_empty() {
            vb.sample = Some(v.clone());
        }
    }
    if let Some(v) = &cfg.default_phonemizer {
        if !v.trim().is_empty() {
            vb.default_phonemizer = Some(v.clone());
        }
    }
    if let Some(v) = &cfg.singer_type {
        if !v.trim().is_empty() {
            vb.singer_type = v.clone();
        }
    }
    if let Some(v) = cfg.use_filename_as_alias {
        vb.use_filename_as_alias = Some(v);
    }
    for sb in &cfg.subbanks {
        vb.subbanks.push(Subbank::from_ranges(
            sb.color.clone().unwrap_or_default(),
            sb.prefix.clone().unwrap_or_default(),
            sb.suffix.clone().unwrap_or_default(),
            sb.tone_ranges.clone(),
        ));
    }
}

fn load_oto_sets(vb: &mut Voicebank, dir: &Path, opts: &LoadOptions) -> std::io::Result<()> {
    let oto_ini = dir.join("oto.ini");
    if oto_ini.is_file() {
        let bytes = std::fs::read(&oto_ini)?;
        let ini = parse_oto_ini(&bytes, Some(vb.text_file_encoding.name()));
        for err in &ini.errors {
            vb.warnings.push(format!(
                "{}:{}: {}",
                oto_ini.display(),
                err.line,
                err.message
            ));
        }
        let mut otos = ini.otos;
        check_wav_exist(dir, &mut otos, &mut vb.warnings);
        add_alias_for_missing_files(dir, &mut otos, &mut vb.warnings);
        let use_filename_as_alias = opts
            .use_filename_as_alias
            .or(vb.use_filename_as_alias)
            .unwrap_or(false);
        if use_filename_as_alias {
            add_filename_alias(&mut otos);
        }
        let name = relative_path(&vb.path, dir);
        let name = if name == "." { String::new() } else { name };
        vb.oto_sets.push(OtoSet {
            name,
            file: oto_ini,
            otos,
        });
    }
    let mut subdirs: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    subdirs.sort();
    for sub in subdirs {
        load_oto_sets(vb, &sub, opts)?;
    }
    Ok(())
}

/// Mark otos whose wav file does not exist next to the oto.ini as invalid.
fn check_wav_exist(dir: &Path, otos: &mut [Oto], warnings: &mut Vec<String>) {
    let mut by_wav: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, oto) in otos.iter().enumerate() {
        if oto.is_valid {
            by_wav.entry(oto.wav.clone()).or_default().push(i);
        }
    }
    for (wav, indices) in by_wav {
        let path = dir.join(&wav);
        if !path.is_file() {
            warnings.push(format!("Sound file missing: {}", path.display()));
            for i in indices {
                otos[i].is_valid = false;
                otos[i].error = Some(format!("Sound file missing: {}", path.display()));
            }
        }
    }
}

/// Add `filename-without-extension` aliases for wav files that exist on disk
/// but are not referenced by any valid oto line (OpenUtau's
/// `AddAliasForMissingFiles`).
fn add_alias_for_missing_files(dir: &Path, otos: &mut Vec<Oto>, warnings: &mut Vec<String>) {
    let known: HashSet<String> = otos
        .iter()
        .filter(|o| o.is_valid)
        .map(|o| o.wav.clone())
        .collect();
    let mut added = 0;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
        })
        .collect();
    files.sort();
    for p in files {
        let fname = p
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if known.contains(fname.as_str()) {
            continue;
        }
        let stem = p
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        otos.push(Oto {
            alias: stem.clone(),
            phonetic: stem,
            wav: fname,
            offset: 0.0,
            consonant: 0.0,
            cutoff: 0.0,
            preutter: 0.0,
            overlap: 0.0,
            is_valid: true,
            error: None,
            line_number: 0,
            set_index: 0,
        });
        added += 1;
    }
    if added > 0 {
        warnings.push(format!(
            "{added} wav file(s) not referenced in oto.ini; added filename aliases"
        ));
    }
}

/// Add each distinct wav's filename (without extension) as an alias, copying
/// the parameters of the entry with the smallest offset for that wav
/// (OpenUtau's `AddFilenameAlias`, used when `use_filename_as_alias`).
fn add_filename_alias(otos: &mut Vec<Oto>) {
    let existing: HashSet<&str> = otos.iter().map(|o| o.alias.as_str()).collect();
    let mut best: HashMap<&str, usize> = HashMap::new(); // wav -> oto index with min offset
    for (i, oto) in otos.iter().enumerate() {
        if !oto.is_valid {
            continue;
        }
        match best.get_mut(oto.wav.as_str()) {
            Some(j) if otos[*j].offset <= oto.offset => {}
            _ => {
                best.insert(oto.wav.as_str(), i);
            }
        }
    }
    let mut new_otos: Vec<Oto> = Vec::new();
    for (wav, idx) in best {
        let stem = Path::new(wav)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| wav.to_string());
        if existing.contains(stem.as_str()) {
            continue;
        }
        let r = &otos[idx];
        new_otos.push(Oto {
            alias: stem.to_string(),
            phonetic: stem.to_string(),
            wav: wav.to_string(),
            offset: r.offset,
            consonant: r.consonant,
            cutoff: r.cutoff,
            preutter: r.preutter,
            overlap: r.overlap,
            is_valid: true,
            error: None,
            line_number: 0,
            set_index: 0,
        });
    }
    otos.extend(new_otos);
}

/// `base` relative to `path` (both absolute); returns "." when equal.
fn relative_path(base: &Path, path: &Path) -> String {
    match path.strip_prefix(base) {
        Ok(rel) => rel.to_string_lossy().into_owned(),
        Err(_) => path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Build a tiny throwaway voicebank in a temp dir.
    fn temp_voicebank(files: &[(&str, &[u8])]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "voicebank_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("voice")).unwrap();
        for (name, content) in files {
            let path = dir.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }
        dir
    }

    #[test]
    fn loads_minimal_voicebank() {
        let dir = temp_voicebank(&[
            ("character.txt", b"name=Test Bank\r\n"),
            ("voice/oto.ini", b"a.wav=a,1,2,3,4,5\r\n"),
            ("voice/a.wav", &[0u8; 44]),
        ]);
        let vb = load_voicebank(&dir).unwrap();
        assert_eq!(vb.name, "Test Bank");
        assert_eq!(vb.otos.len(), 1);
        assert_eq!(vb.oto_map.len(), 1);
        assert!(vb.lookup("a", 60).is_some());
        assert!(vb.lookup("missing", 60).is_none());
        assert_eq!(vb.subbanks.len(), 1);
        assert!(vb.subbanks[0].tones.is_empty());
        assert_eq!(vb.warnings.len(), 0);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_wav_invalidates_oto() {
        let dir = temp_voicebank(&[
            ("character.txt", b"name=T\r\n"),
            ("voice/oto.ini", b"a.wav=a,1,2,3,4,5\r\n"),
        ]);
        let vb = load_voicebank(&dir).unwrap();
        assert!(vb.otos.is_empty());
        assert!(vb.oto_map.is_empty());
        assert!(vb.warnings.iter().any(|w| w.contains("missing")));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_wavs_get_filename_aliases() {
        let dir = temp_voicebank(&[
            ("character.txt", b"name=T\r\n"),
            ("voice/oto.ini", b"a.wav=a,1,2,3,4,5\r\n"),
            ("voice/a.wav", &[0u8; 44]),
            ("voice/unlisted.wav", &[0u8; 44]),
        ]);
        let vb = load_voicebank(&dir).unwrap();
        // "unlisted" alias added for the unreferenced wav.
        assert!(vb.lookup_plain("unlisted").is_some());
        assert_eq!(vb.otos.len(), 2);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn filename_alias_option() {
        let dir = temp_voicebank(&[
            ("character.txt", b"name=T\r\n"),
            ("voice/oto.ini", b"a.wav=a,10,2,3,4,5\r\nb.wav=b,1,2,3,4,5\r\n"),
            ("voice/a.wav", &[0u8; 44]),
            ("voice/b.wav", &[0u8; 44]),
        ]);
        let opts = LoadOptions {
            use_filename_as_alias: Some(true),
        };
        let vb = load_voicebank_with_options(&dir, &opts).unwrap();
        let oto = vb.lookup_plain("a").unwrap();
        // Parameter copy from the entry with min offset for a.wav.
        assert_eq!(oto.offset, 10.0);
        assert_eq!(oto.wav, "a.wav");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn prefix_map_drives_tone_lookup() {
        let dir = temp_voicebank(&[
            ("character.txt", b"name=T\r\n"),
            ("prefix.map", b"C3\tP_\t_S\nC#3\tP_\t_S\n"),
            ("voice/oto.ini", b"P_a_S.wav=P_a_S,1,2,3,4,5\r\na.wav=a,1,2,3,4,5\r\n"),
            ("voice/P_a_S.wav", &[0u8; 44]),
            ("voice/a.wav", &[0u8; 44]),
        ]);
        let vb = load_voicebank(&dir).unwrap();
        assert_eq!(vb.subbanks.len(), 1);
        assert_eq!(vb.subbanks[0].tone_ranges, vec!["C3-C#3"]);
        // Tone inside the mapped range resolves through prefix/suffix.
        let oto = vb.lookup("a", 48).unwrap();
        assert_eq!(oto.alias, "P_a_S");
        // Tone outside falls back to the plain alias.
        let oto = vb.lookup("a", 60).unwrap();
        assert_eq!(oto.alias, "a");
        // Color-aware lookup falls back too (no colored subbank exists).
        let oto = vb.lookup_with_color("a", 48, "power").unwrap();
        assert_eq!(oto.alias, "P_a_S");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn yaml_subbanks_override_prefix_map() {
        let dir = temp_voicebank(&[
            ("character.txt", b"name=T\r\n"),
            (
                "character.yaml",
                b"name: Yaml Bank\nuse_filename_as_alias: true\nsubbanks:\n  - color: power\n    prefix: X_\n    suffix: _X\n    tone_ranges:\n      - C4\n",
            ),
            ("prefix.map", b"C3\tP_\t_S\n"),
            ("voice/oto.ini", b"X_a_X.wav=X_a_X,1,2,3,4,5\r\n"),
            ("voice/X_a_X.wav", &[0u8; 44]),
        ]);
        let vb = load_voicebank(&dir).unwrap();
        assert_eq!(vb.name, "Yaml Bank");
        assert_eq!(vb.subbanks.len(), 1);
        assert_eq!(vb.subbanks[0].color, "power");
        // Yaml subbanks present => prefix.map not loaded.
        // Colored subbanks only match via color-aware lookup...
        let oto = vb.lookup_with_color("a", 60, "power").unwrap();
        assert_eq!(oto.alias, "X_a_X");
        // ...and the plain fallback finds nothing (no bare "a" alias).
        assert!(vb.lookup("a", 60).is_none());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_character_txt_errors() {
        let dir = temp_voicebank(&[("voice/oto.ini", b"a.wav=a,1,2,3,4,5\r\n")]);
        assert!(matches!(
            load_voicebank(&dir),
            Err(VoicebankError::MissingCharacterTxt(_))
        ));
        fs::remove_dir_all(&dir).ok();
    }
}
