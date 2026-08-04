//! Voicebank loading for the Android voice synthesis engine (Sprint 1.3).
//!
//! Parses UTAU/OpenUtau voicebanks: `oto.ini` alias tables, `character.txt`
//! / `character.yaml` metadata, `prefix.map` tone mapping, 44.1 kHz wav
//! samples, and `FREQ0003` frq pitch files, with Shift-JIS support for
//! legacy Japanese voicebanks.

pub mod config;
pub mod frq;
pub mod oto;
pub mod prefix_map;
pub mod text;
pub mod tone;
pub mod voicebank;
pub mod wav;

pub use config::{parse_character_txt, parse_character_yaml, CharacterTxt, Subbank, VoicebankConfigYaml};
pub use frq::{frq_path_for_wav, parse_frq, read_frq, read_frq_for_wav, FrqData, FrqError};
pub use oto::{detect_charset, parse_oto_ini, Oto, OtoError, OtoIni, OtoSet};
pub use prefix_map::parse_prefix_map;
pub use text::{decode, decode_lines, detect_encoding, read_text};
pub use tone::{name_to_tone, tone_to_name};
pub use voicebank::{
    load_voicebank, load_voicebank_with_options, LoadOptions, Voicebank, VoicebankError,
};
pub use wav::{parse_wav, read_wav, WavData, WavError};
