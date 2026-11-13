//! Rusted UTAU-owned pronunciation metadata for a stable note identity.
//!
//! This is kept in the in-memory project model so document undo/redo includes
//! it, but it is not serialized into OpenUtau `.ustx`. Project Session hosts
//! persist it in the versioned pronunciation sidecar instead.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UPronunciation {
    pub language: String,
    pub mode: String,
    pub style: String,
    pub strategy: String,
    pub language_phones: Vec<String>,
    pub universal_phones: Vec<String>,
}
