//! G2p base: grapheme-to-phoneme conversion.
//!
//! Mirrors `OpenUtau.Core/Api/IG2p.cs` and `G2pDictionary.cs`:
//!
//! * [`G2pDictionary`] — a trie-backed word → phoneme-sequence dictionary
//!   with a symbol table classifying each phoneme as vowel / glide / other.
//! * [`G2pFallbacks`] — tries a chain of G2ps, first match wins.
//! * [`parse_hint`] — splits a phonetic hint (`"r iy d"`) into tokens;
//!   [`G2p::unpack_hint`] additionally filters out symbols the dictionary
//!   does not know.
//! * [`SP`] / [`AP`] — the standard silence / breath symbols. They are
//!   always considered valid symbols (non-vowel, non-glide).

use std::collections::HashMap;

/// Standard silence symbol (OpenUtau uses `SP` for rests).
pub const SP: &str = "SP";
/// Standard breath / aspiration symbol.
pub const AP: &str = "AP";

/// A grapheme-to-phoneme converter (`IG2p`).
pub trait G2p {
    /// Whether `symbol` is a phoneme this G2p knows.
    fn is_valid_symbol(&self, symbol: &str) -> bool;
    /// Whether `symbol` is a vowel.
    fn is_vowel(&self, symbol: &str) -> bool;
    /// Whether `symbol` is a semivowel or liquid (y, w, l, r ...).
    fn is_glide(&self, symbol: &str) -> bool;
    /// Phoneme sequence for `grapheme`, or `None` when unknown.
    fn query(&self, grapheme: &str) -> Option<Vec<String>>;
    /// Split a phonetic hint on `separator`, dropping invalid symbols
    /// (OpenUtau `G2pDictionary.UnpackHint`).
    fn unpack_hint(&self, hint: &str, separator: char) -> Vec<String>;
}

/// Split a phonetic hint on whitespace, keeping tokens verbatim.
///
/// Unlike [`G2p::unpack_hint`] this performs no symbol filtering — used
/// when no dictionary is available, so an unmatched hint still falls back
/// to itself ("use hint as-is").
pub fn parse_hint(hint: &str) -> Vec<String> {
    hint.split_whitespace().map(str::to_string).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SymbolInfo {
    is_vowel: bool,
    is_glide: bool,
}

#[derive(Debug, Default)]
struct TrieNode {
    children: HashMap<char, TrieNode>,
    symbols: Option<Vec<String>>,
}

/// A trie-backed G2p dictionary (`G2pDictionary`).
///
/// Build it with [`G2pDictionary::new`], register symbols *before* entries
/// (entries whose phonemes are not registered symbols are dropped, exactly
/// like the reference builder).
#[derive(Debug, Default)]
pub struct G2pDictionary {
    root: TrieNode,
    symbols: HashMap<String, SymbolInfo>,
}

impl G2pDictionary {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `symbol` with its classification.
    pub fn add_symbol(&mut self, symbol: impl Into<String>, is_vowel: bool, is_glide: bool) -> &mut Self {
        self.symbols
            .insert(symbol.into(), SymbolInfo { is_vowel, is_glide });
        self
    }

    /// Register `symbol` by type name, mirroring the C# builder's
    /// `AddSymbol(symbol, type)`: `"vowel"` marks a vowel, `"semivowel"`
    /// and `"liquid"` mark glides, anything else is a plain consonant.
    pub fn add_symbol_with_type(&mut self, symbol: impl Into<String>, ty: &str) -> &mut Self {
        let symbol = symbol.into();
        let is_vowel = ty == "vowel";
        let is_glide = matches!(ty, "semivowel" | "liquid");
        self.symbols.insert(symbol, SymbolInfo { is_vowel, is_glide });
        self
    }

    /// Register the standard `SP` / `AP` symbols (valid, neither vowel nor
    /// glide).
    pub fn add_standard_symbols(&mut self) -> &mut Self {
        self.add_symbol(SP, false, false);
        self.add_symbol(AP, false, false);
        self
    }

    /// Register `grapheme` → `phonemes`. Phonemes not in the symbol table
    /// are silently dropped (reference builder behavior).
    pub fn add_entry(&mut self, grapheme: impl AsRef<str>, phonemes: &[&str]) -> &mut Self {
        let valid: Vec<String> = phonemes
            .iter()
            .filter(|p| self.symbols.contains_key(**p))
            .map(|p| p.to_string())
            .collect();
        let mut node = &mut self.root;
        for ch in grapheme.as_ref().chars() {
            node = node.children.entry(ch).or_default();
        }
        if !valid.is_empty() {
            node.symbols = Some(valid);
        }
        self
    }

    /// Number of registered symbols.
    pub fn symbol_count(&self) -> usize {
        self.symbols.len()
    }

    /// Whether any entries were registered.
    pub fn is_empty(&self) -> bool {
        self.root.children.is_empty() && self.root.symbols.is_none()
    }
}

impl G2p for G2pDictionary {
    fn is_valid_symbol(&self, symbol: &str) -> bool {
        self.symbols.contains_key(symbol)
    }

    fn is_vowel(&self, symbol: &str) -> bool {
        self.symbols.get(symbol).is_some_and(|s| s.is_vowel)
    }

    fn is_glide(&self, symbol: &str) -> bool {
        self.symbols.get(symbol).is_some_and(|s| s.is_glide)
    }

    fn query(&self, grapheme: &str) -> Option<Vec<String>> {
        let mut node = &self.root;
        for ch in grapheme.chars() {
            node = node.children.get(&ch)?;
        }
        node.symbols.clone()
    }

    fn unpack_hint(&self, hint: &str, separator: char) -> Vec<String> {
        hint.split(separator)
            .filter(|s| self.symbols.contains_key(*s))
            .map(str::to_string)
            .collect()
    }
}

/// Try a chain of G2ps, first match wins (`G2pFallbacks`).
pub struct G2pFallbacks {
    g2ps: Vec<Box<dyn G2p>>,
}

impl G2pFallbacks {
    pub fn new(g2ps: Vec<Box<dyn G2p>>) -> Self {
        G2pFallbacks { g2ps }
    }
}

impl G2p for G2pFallbacks {
    fn is_valid_symbol(&self, symbol: &str) -> bool {
        self.g2ps.iter().any(|g| g.is_valid_symbol(symbol))
    }

    fn is_vowel(&self, symbol: &str) -> bool {
        self.g2ps
            .iter()
            .any(|g| g.is_valid_symbol(symbol) && g.is_vowel(symbol))
    }

    fn is_glide(&self, symbol: &str) -> bool {
        self.g2ps
            .iter()
            .any(|g| g.is_valid_symbol(symbol) && g.is_glide(symbol))
    }

    fn query(&self, grapheme: &str) -> Option<Vec<String>> {
        self.g2ps.iter().find_map(|g| g.query(grapheme))
    }

    fn unpack_hint(&self, hint: &str, separator: char) -> Vec<String> {
        hint.split(separator)
            .filter(|s| self.is_valid_symbol(s))
            .map(str::to_string)
            .collect()
    }
}
