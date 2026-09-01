//! Phrase generation: enumerate the bank's oto aliases (deduplicated)
//! plus a few connected multi-note phrases from the same alias set.

use std::collections::HashSet;

use voicebank::Voicebank;

use crate::PhraseSpec;

/// Build the POC phrase list: every oto alias as a single-note phrase,
/// then `n_chain` connected phrases of 2-3 aliases (the "สร้างทั้ง
/// phrase" part — connected rendering, not isolated syllables).
///
/// Diversity (2026-08-09 — teacher-rendered variety is the real data
/// bottleneck; the whole set was C4/500ms): when `tones`/`durs` have
/// multiple entries, each alias is rendered at every (tone, dur) combo.
pub fn phrases_for_bank(
    vb: &Voicebank,
    tones: &[i32],
    durs: &[f64],
    max_singles: Option<usize>,
    n_chain: usize,
    chain_max: usize,
) -> Vec<PhraseSpec> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut singles: Vec<PhraseSpec> = Vec::new();
    for oto in &vb.otos {
        if seen.insert(oto.alias.clone()) {
            for &tone in tones {
                for &dur in durs {
                    singles.push(PhraseSpec::single(&oto.alias, tone, dur));
                }
            }
        }
    }
    if let Some(max) = max_singles {
        singles.truncate(max * tones.len() * durs.len());
    }

    // Connected phrases: chain 2..=chain_max aliases (up to sentence-
    // length coarticulation). Keep it cheap: first `n_chain` aliases
    // chained in windows.
    let aliases: Vec<&String> = seen.iter().take(n_chain).collect();
    let mut chains: Vec<PhraseSpec> = Vec::new();
    for &tone in tones {
        for &dur in durs {
            for width in 2..=chain_max.max(2) {
                if aliases.len() >= width {
                    for w in aliases.windows(width) {
                        chains.push(PhraseSpec {
                            aliases: w.iter().map(|s| (*s).clone()).collect(),
                            tone,
                            duration_ms: dur,
                        });
                    }
                }
            }
        }
    }

    let mut out = singles;
    out.extend(chains);
    out
}
