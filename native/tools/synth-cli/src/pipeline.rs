//! The render pipeline shared by the `render` / `synth-note` commands and
//! the integration tests.
//!
//! ```text
//! .ustx ──serde_yaml + after_load──▶ UProject
//!                                      │
//! voicebank dir ──load_voicebank─────▶ Voicebank
//!                                      │
//! track notes ──phonemizer──▶ UPhoneme[] ──TimingEngine──▶ groups
//!                                      │
//!                      PhraseGrouping ─┴─▶ PhraseGroup[]
//!                                      │
//!                       PhraseBuilder ─┴─▶ RenderInput[]
//!                                      │
//!                 WorldlineRenderer ───┴─▶ Vec<f32> per phrase
//!                                      │
//!                     runtime::mixer ──┴─▶ one mono 44.1 kHz stream
//! ```
//!
//! Phrase placement follows the reference (`Renderers.ApplyDynamics`):
//! a phrase's sample 0 lands at `position_ms - leading_ms`, which
//! [`runtime::mixer::mix`] implements by aligning every chunk at that
//! offset and summing.

use std::path::Path;

use rayon::prelude::*;

use crate::chunking;
use domain::{UPart, UProject, UTrack, UVoicePart};
use feed::render_input::RenderInput;
use phonemizer::{EnglishCvvcPhonemizer, JapaneseVcvPhonemizer, Phonemizer, TimingEngine};
use phrase::{PhraseBuilder, PhraseGroup, PhraseGrouping};
use runtime::mixer::{mix, AudioChunk, MixInput, TrackSpec};
use runtime::RenderCache;
use voicebank::load_voicebank as load_vb;
use voicebank::Voicebank;
use worldline_plugin::WorldlineRenderer;

/// Output sample rate of the whole pipeline (worldline + mixer).
pub use runtime::mixer::SAMPLE_RATE;

/// Chunk target duration (ms) — incremental re-render granularity.
/// Chunk target: phrases up to ~4 s render as ONE chunk (a single phrase
/// render — apples-to-apples with the golden renderer, no crossfade/trim
/// artifacts). Longer phrases split note-atomically to bound memory and
/// enable incremental re-render. Was 2000 ms — the 3 s scale demo split
/// into 2 chunks while the C# reference renders it as one phrase.
pub const CHUNK_TARGET_MS: f64 = 4000.0;
/// Whole neighbour notes carried into each chunk as crossfade context.
pub const CHUNK_CONTEXT_NOTES: usize = 1;

/// Which phonemizer derives the phonemes of a part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhonemizerKind {
    English,
    Japanese,
    /// IPA-first (L1+L2): detect → IPA → voicebank aliases via the
    /// manifest. Handles English AND Japanese through the same path.
    Universal,
}

impl PhonemizerKind {
    /// Pick from a track's configured phonemizer name (Universal default,
    /// matching the IPA plan: one phonemizer for every bank; English/
    /// Japanese remain available for the legacy paths).
    pub fn from_track(track: &UTrack) -> Self {
        let Some(name) = track.phonemizer.as_deref() else {
            return PhonemizerKind::Universal;
        };
        let lower = name.to_ascii_lowercase();
        if lower.contains("japanese") || lower.contains("jpn") || lower.contains("vcv") {
            PhonemizerKind::Japanese
        } else {
            // English (and anything else) → Universal: the legacy English
            // CVVC path falls back to the lyric VERBATIM on unknown words
            // ("shi" → no oto alias → the whole phrase skipped). Universal
            // letter-splits + NearestPhoneme-maps unknown words and
            // delegates kana to the VCV phonemizer — one path for every
            // lyric ("ถ้าไม่ตรงภาษาก็เข้า universal ให้หมดเลยนะ").
            PhonemizerKind::Universal
        }
    }
}

/// Load a `.ustx` project: parse YAML, merge legacy fields and build the
/// time axis (`after_load`).
pub fn load_project(path: &Path) -> Result<UProject, String> {
    let yaml =
        std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut project: UProject =
        serde_yaml::from_str(&yaml).map_err(|e| format!("parse {}: {e}", path.display()))?;
    project
        .after_load()
        .map_err(|e| format!("after_load: {e}"))?;
    Ok(project)
}

/// Load a voicebank directory (must contain `voice/oto.ini`).
pub fn load_voicebank(path: &Path) -> Result<Voicebank, String> {
    load_vb(path).map_err(|e| format!("load voicebank {}: {e}", path.display()))
}

/// Derive a part's phonemes (phonemizer + timing engine).
pub fn derive_phonemes(
    project: &UProject,
    part: &UVoicePart,
    voicebank: &Voicebank,
    kind: PhonemizerKind,
) -> Vec<domain::UPhoneme> {
    let mut phonemes = match kind {
        PhonemizerKind::English => {
            // G2p dictionary: words → Teto-compatible phoneme sequences so
            // plain lyrics (no phonetic hint) still map to oto aliases.
            // Unknown words fall back to the lyric verbatim.
            EnglishCvvcPhonemizer::with_g2p(phonemizer::lilt_dict::lilt_demo_g2p())
                .process(&part.notes, Some(voicebank))
        }
        PhonemizerKind::Japanese => JapaneseVcvPhonemizer.process(&part.notes, Some(voicebank)),
        PhonemizerKind::Universal => {
            // IPA-first: manifest (L3) + adapter (L2) + universal
            // phonemizer (L1) — one path for every language.
            use phonemizer::adapter::IpaAdapter;
            use phonemizer::ipa::IpaDictionary;
            use phonemizer::universal::UniversalPhonemizer;
            let manifest = voicebank.phoneme_manifest();
            let dict = IpaDictionary::en_subset();
            let adapter = IpaAdapter::new(&manifest, &dict);
            UniversalPhonemizer::new(adapter).process(&part.notes, Some(voicebank))
        }
    };
    TimingEngine.process(
        &part.notes,
        part.position,
        &mut phonemes,
        &project.time_axis,
        Some(voicebank),
    );
    phonemes
}

/// One phrase of a part: its phoneme group plus the built render input.
pub struct PhraseInput {
    pub group: PhraseGroup,
    pub input: RenderInput,
}

/// Group a part's phonemes into phrases and build a `RenderInput` per
/// group (the `RenderPhrase.FromPart` walk + the `RenderPhrase`
/// constructor).
pub fn build_phrase_inputs(
    project: &UProject,
    track: &UTrack,
    part: &UVoicePart,
    voicebank: &Voicebank,
    kind: PhonemizerKind,
) -> Result<Vec<PhraseInput>, String> {
    let phonemes = derive_phonemes(project, part, voicebank, kind);
    let groups = PhraseGrouping::group(&phonemes);
    let builder = PhraseBuilder::new(project, track, part, Some(voicebank));
    groups
        .iter()
        .map(|group| {
            let input = builder
                .build(group)
                .map_err(|e| format!("build phrase: {e}"))?;
            Ok(PhraseInput {
                group: group.clone(),
                input,
            })
        })
        .collect()
}

/// Result of rendering a whole project.
pub struct RenderReport {
    /// Phrases that produced audio.
    pub phrases_rendered: usize,
    /// Human-readable reasons for every skipped phrase.
    pub skipped: Vec<String>,
    /// The mixed mono stream at [`SAMPLE_RATE`].
    pub samples: Vec<f32>,
}

/// A phrase renderer: `RenderInput` → mono PCM. Both the v1 WORLD
/// renderer (`WorldlineRenderer`, Synth()) and the v2 neural engine
/// (`R2Engine`, mel→HiFiGAN) implement this — the pipeline never depends
/// on a concrete engine, so hosts can select v1 / v2 per request.
/// A phrase renderer (r1/r2/classic/diffsinger). `Send + Sync` lets the
/// pipeline render the chunks of one phrase in parallel (each engine
/// instance is reentrant: fresh state per `render_phrase` call).
pub trait PhraseRenderer: Send + Sync {
    /// Render one phrase (the pipeline's chunking may call this
    /// concurrently on different chunks).
    fn render_phrase(&self, input: &feed::render_input::RenderInput) -> Result<Vec<f32>, String>;
    /// Engine discriminator for the render cache: different engines
    /// (r1 WORLD vs r2 neural) must never share cache entries even for
    /// identical phoneme input.
    fn cache_salt(&self) -> u64 {
        0
    }
    /// Whether concurrent `render_phrase` calls are deterministic. The
    /// default is SEQUENTIAL (golden-safe): the C++ WORLD and the
    /// multi-threaded ort reductions are float-schedule sensitive, so a
    /// parallel fan-out can shift the output. Engines that verified
    /// determinism (e.g. single-threaded ort sessions) opt in.
    fn parallel_safe(&self) -> bool {
        false
    }
}

impl PhraseRenderer for WorldlineRenderer {
    fn render_phrase(&self, input: &feed::render_input::RenderInput) -> Result<Vec<f32>, String> {
        WorldlineRenderer::render_phrase(self, input).map_err(|e| e.to_string())
    }
    fn cache_salt(&self) -> u64 {
        // "worldline-r" — fnv-ish from the engine name.
        0x726c31_0000_0001
    }
    fn parallel_safe(&self) -> bool {
        // The C++ WORLD's randn state is thread-local since the
        // thread-safety fix — concurrent chunk renders are verified
        // byte-identical (deterministic).
        true
    }
}

/// Render every voice part of `track_no` and mix the phrases in time.
///
/// A phrase whose phonemes do not map to oto aliases is skipped (with a
/// reason), mirroring the reference's "phonemes without an oto entry are
/// not rendered". If nothing renders, `samples` is empty and every skip
/// reason is reported.
pub fn render_project(
    project: &UProject,
    voicebank: &Voicebank,
    renderer: &dyn PhraseRenderer,
    track_no: i32,
    kind: PhonemizerKind,
    verbose: bool,
    cache: &mut Option<RenderCache>,
    mixer: Option<&mut mixer_fx::MixerFx>,
    cancel: Option<&std::sync::atomic::AtomicBool>,
) -> Result<RenderReport, String> {
    render_project_with(
        project,
        voicebank,
        renderer,
        track_no,
        kind,
        verbose,
        cache,
        mixer,
        cancel,
        None,
    )
}

/// `render_project` with per-phrase engine selection. `engine_by_phrase`
/// maps a phrase index (in render order across the track's voice parts)
/// to the renderer to use for it; `None` = use `renderer` for every
/// phrase (the default). This is how the app renders "ระหว่างท่อน" —
/// different sections of a song with different synthesis engines
/// (WORLDLINE-R / WORLDLINE-R2 / Classic).
pub fn render_project_with(
    project: &UProject,
    voicebank: &Voicebank,
    renderer: &dyn PhraseRenderer,
    track_no: i32,
    kind: PhonemizerKind,
    verbose: bool,
    cache: &mut Option<RenderCache>,
    mixer: Option<&mut mixer_fx::MixerFx>,
    cancel: Option<&std::sync::atomic::AtomicBool>,
    engine_by_phrase: Option<&dyn Fn(usize) -> &'static dyn PhraseRenderer>,
) -> Result<RenderReport, String> {
    let track = project.tracks.get(track_no as usize).ok_or_else(|| {
        format!(
            "track {track_no} not found (project has {} tracks)",
            project.tracks.len()
        )
    })?;
    let parts: Vec<&UVoicePart> = project
        .parts
        .iter()
        .filter_map(|part| match part {
            UPart::Voice(voice) if voice.track_no == track_no => Some(voice),
            _ => None,
        })
        .collect();
    if parts.is_empty() {
        return Err(format!("no voice part found on track {track_no}"));
    }

    let mut chunks: Vec<MixInput> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut phrase_idx = 0usize;
    for part in parts {
        for phrase in build_phrase_inputs(project, track, part, voicebank, kind)? {
            // Per-phrase engine selection ("ระหว่างท่อน"): the host maps a
            // phrase index to a specific renderer (r1/r2/classic).
            let phrase_renderer: &dyn PhraseRenderer = match engine_by_phrase {
                Some(f) => f(phrase_idx),
                None => renderer,
            };
            phrase_idx += 1;
            if phrase.input.sample_based.is_none() {
                let names: Vec<&str> = phrase
                    .input
                    .phonemes
                    .iter()
                    .map(|p| p.phoneme.as_str())
                    .collect();
                skipped.push(format!(
                    "part '{}' phrase @{:.0} ms: no phoneme maps to an oto alias ({})",
                    part.name,
                    phrase.group.position_ms,
                    names.join(" ")
                ));
                continue;
            }
            // Partial mapping (some phonemes without an oto alias): the
            // oto list has fewer entries than `phonemes` and cannot be
            // sliced by chunk ctx_range — skip the whole phrase once,
            // like the reference's oto == null handling. Checked before
            // chunking so the reason is reported once, not per chunk.
            let sb_oto_len = phrase
                .input
                .sample_based
                .as_ref()
                .map(|sb| sb.oto.len())
                .unwrap_or(0);
            if sb_oto_len != phrase.input.phonemes.len() {
                let names: Vec<&str> = phrase
                    .input
                    .phonemes
                    .iter()
                    .map(|p| p.phoneme.as_str())
                    .collect();
                skipped.push(format!(
                    "part '{}' phrase @{:.0} ms: render failed — partial oto \
                     mapping ({} of {} phonemes) ({})",
                    part.name,
                    phrase.group.position_ms,
                    sb_oto_len,
                    phrase.input.phonemes.len(),
                    names.join(" "),
                ));
                continue;
            }
            // Chunk-level render (incremental): split the phrase into
            // time-based chunks — note-atomic (a word's phonemes never
            // split), rest-split, each chunk renders with context notes
            // each side (crossfade continuity) and is trimmed back to its
            // own span. Cache key = chunk hash (context included), so an
            // edit only invalidates the chunks that own it.
            //
            // Chunking applies to FRESH renders too: a whole-phrase chunk
            // would (a) spike memory on long songs and (b) store one
            // giant cache entry that later incremental edits cannot
            // reuse. Seams are made inaudible by the mixer's equal-power
            // crossfade.
            let chunk_plan = chunking::plan_chunks(
                &phrase.input.phonemes,
                &phrase.input.notes,
                CHUNK_TARGET_MS,
                CHUNK_CONTEXT_NOTES,
            );
            // --- render the chunks of this phrase ---------------------------
            // Parallel (rayon) when the engine verified deterministic
            // concurrent renders; otherwise the golden-safe sequential
            // path. Each chunk builds its own sub-phrase (the pure
            // origin/curve rebasing below) and renders through
            // `render_phrase(&self)`. The cache is behind a mutex
            // (per-key file IO — negligible contention); `collect`
            // preserves the chunk order, so the mixed output is
            // byte-identical to the sequential path.
            let cache_mutex = cache.as_mut().map(|c| std::sync::Mutex::new(&mut *c));
            let render_chunk = |chunk: &chunking::Chunk| -> Result<Option<MixInput>, String> {
// Cooperative cancellation: the host sets the flag (e.g.
// the user hit Stop) — bail out between chunks so the
// reply comes back fast instead of finishing the render.
if let Some(c) = cancel {
    if c.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("render cancelled".to_string());
    }
}
let key = chunk.hash(&phrase.input.phonemes, phrase_renderer.cache_salt());
let mut sub = phrase.input.clone();
sub.phonemes = phrase.input.phonemes[chunk.ctx_range.clone()].to_vec();
// Point the phrase frame at the chunk's first context
// phoneme so the f0/expression curves start there — the
// renderer's curves are sampled from
// `phrase.position_ms - phrase.leading_ms`, and a stale
// full-phrase origin would prepend silence to the chunk.
let ctx_first = &phrase.input.phonemes[chunk.ctx_range.start];
let ctx_last = &phrase.input.phonemes[chunk.ctx_range.end - 1];
sub.phrase.position_ms = ctx_first.position_ms;
sub.phrase.leading_ms = ctx_first.leading_ms;
sub.phrase.duration_ms =
    (ctx_last.position_ms + ctx_last.duration_ms) - ctx_first.position_ms;
// Rebasing `phrase.position_ms` above moved the curve
// sampling origin to the chunk's first context phoneme,
// but `pitches_cents`/`curves` are still FULL-phrase
// arrays indexed from the original phrase origin. A
// later chunk would sample the EARLIER notes' pitches
// (the scale demo's chunk 2 renders notes 0-7's pitches
// again — the descending scale "bounces back up").
// Shift the arrays to the chunk origin (floor: the grid
// index floors anyway, and the grid is 5-tick steps — a
// sub-interval remainder matches the whole-phrase path).
let origin_old = phrase.input.phrase.position_ms
    - phrase.input.phrase.leading_ms;
let origin_new = ctx_first.position_ms - ctx_first.leading_ms;
if origin_new > origin_old + 0.001 {
    let interval = worldline_plugin::ms_per_interval(&phrase.input);
    let skip = ((origin_new - origin_old) / interval).floor() as usize;
    if skip > 0 {
        sub.pitches_cents.drain(..skip.min(sub.pitches_cents.len()));
        for c in [
            &mut sub.curves.dynamics,
            &mut sub.curves.gender,
            &mut sub.curves.breathiness,
            &mut sub.curves.tension,
            &mut sub.curves.voicing,
        ] {
            c.drain(..skip.min(c.len()));
        }
    }
}
// sample_based.oto is per-phoneme (1:1 with `phonemes`
// after the partial-mapping gate above) — slice it to
// match the chunk's context range.
if let (Some(sb), Some(base)) = (
    sub.sample_based.as_mut(),
    phrase.input.sample_based.as_ref(),
) {
    sb.oto = base.oto[chunk.ctx_range.clone()].to_vec();
}
// `get_or_compute`'s closure cannot return a Result, so
// render first when missing and propagate errors; on a
// cache hit the render is skipped. The cache is behind a
// mutex for the parallel chunks (per-key file IO — the
// contention is negligible).
let rendered: Vec<f32> = match &cache_mutex {
    Some(m) => {
        // Lock ONLY for the key lookup/insert — the render
        // itself runs OUTSIDE the lock so the parallel
        // chunks actually run concurrently.
        let hit = {
            let mut c = m
                .lock()
                .map_err(|e| format!("render cache lock: {e}"))?;
            c.get(key)
        };
        match hit {
            Some(h) => h,
            None => {
                let r = phrase_renderer.render_phrase(&sub).map_err(|e| {
                    format!(
                        "part '{}' phrase @{:.0} ms chunk {}-{}: render failed: {e}",
                        part.name,
                        phrase.group.position_ms,
                        chunk.own_range.start,
                        chunk.own_range.end
                    )
                })?;
                let mut c = m
                    .lock()
                    .map_err(|e| format!("render cache lock: {e}"))?;
                let _ = c.put(key, r.clone());
                r
            }
        }
    }
    None => phrase_renderer.render_phrase(&sub).map_err(|e| {
        format!(
            "part '{}' phrase @{:.0} ms chunk {}-{}: render failed: {e}",
            part.name,
            phrase.group.position_ms,
            chunk.own_range.start,
            chunk.own_range.end
        )
    })?,
};
// Trim the rendered (own+context) audio back to the chunk's
// own span, keeping the first phoneme's leading.
let ctx_first = &phrase.input.phonemes[chunk.ctx_range.start];
let audio_start_ms = ctx_first.position_ms - ctx_first.leading_ms;
let first_lead = phrase.input.phonemes[chunk.own_range.start].leading_ms;
let sr = f64::from(SAMPLE_RATE) / 1000.0;
let start_i = (((chunk.start_ms - first_lead) - audio_start_ms) * sr).max(0.0) as usize;
let end_i = (((chunk.end_ms) - audio_start_ms) * sr) as usize;
let samples = if end_i > start_i && start_i < rendered.len() {
    rendered[start_i..end_i.min(rendered.len())].to_vec()
} else {
    Vec::new()
};
if verbose {
    println!(
        "  phrase @{:.0} ms chunk {}-{} (own {}-{}): {} samples ({:.0} ms){}",
        phrase.input.phrase.position_ms,
        chunk.ctx_range.start,
        chunk.ctx_range.end,
        chunk.own_range.start,
        chunk.own_range.end,
        samples.len(),
        samples.len() as f64 * 1000.0 / f64::from(SAMPLE_RATE),
        if cache_mutex.is_some() { " (cached)" } else { "" },
    );
}
Ok(Some(MixInput {
    chunk: AudioChunk {
        samples,
        // samples[0] starts at (chunk.start - first lead).
        position_ms: chunk.start_ms - first_lead,
        leading_ms: first_lead,
        hash: key,
    },
    track: TrackSpec::default(),
}))
            };
            let chunk_results: Result<Vec<Option<MixInput>>, String> = if phrase_renderer.parallel_safe() {
                chunk_plan.par_iter().map(render_chunk).collect()
            } else {
                chunk_plan.iter().map(render_chunk).collect()
            };
            chunks.extend(chunk_results?.into_iter().flatten());

        }
    }

    let audio = mix(&chunks);
    let mut samples = audio.samples;
    // Mixer FX plugin: process the final mixed track (gain -> EQ -> comp
    // -> soft clip in C++). Passthrough (default params) must not change
    // the audio — verified against golden.
    if let Some(mixer) = mixer {
        mixer.process(&mut samples, 0.0)?;
    }
    Ok(RenderReport {
        phrases_rendered: chunks.len(),
        skipped,
        samples,
    })
}

/// Build the synthetic one-note phrase for `synth-note` (no renderer
/// involved): a one-note project with default expressions, the phoneme
/// given as the note's phonetic hint (so the phonemizer emits it verbatim
/// when it does not pair further), timed and grouped like any part.
pub fn synth_note_input(
    voicebank: &Voicebank,
    alias: &str,
    tone: i32,
    duration_ms: f64,
) -> Result<PhraseInput, String> {
    let project = UProject::create();
    let ticks = project.time_axis.ms_to_tick(duration_ms).max(1);
    let mut note = project.create_note_at(tone, 0, ticks);
    note.lyric = format!("{alias}[{alias}]");
    let part = UVoicePart {
        position: 0,
        notes: vec![note],
        ..Default::default()
    };
    let track = &project.tracks[0];
    let phrases = build_phrase_inputs(&project, track, &part, voicebank, PhonemizerKind::English)?;
    phrases
        .into_iter()
        .next()
        .ok_or_else(|| "no phrase derived from the note".to_string())
}

/// The `synth-note` oto gate: the phrase must map every phoneme to an oto
/// entry, or the renderer has nothing to synthesize. Returns the error
/// (with the available aliases) that the CLI surfaces.
pub fn synth_note_validate(
    voicebank: &Voicebank,
    alias: &str,
    tone: i32,
    duration_ms: f64,
) -> Result<(), String> {
    let phrase = synth_note_input(voicebank, alias, tone, duration_ms)?;
    if phrase.input.sample_based.is_none() {
        let names: Vec<&str> = phrase
            .input
            .phonemes
            .iter()
            .map(|p| p.phoneme.as_str())
            .collect();
        return Err(format!(
            "phoneme '{alias}' has no oto entry in this voicebank \
             (phonemized to: {}). Available aliases include: {}",
            names.join(" "),
            available_aliases(voicebank, 16),
        ));
    }
    Ok(())
}

/// Render one note with a single phoneme (the `synth-note` quick test).
///
/// Builds a synthetic one-note project (default expressions, 120 bpm),
/// phonemizes `alias` (as a phonetic hint, so the phonemizer emits it
/// verbatim when it does not pair further), and renders the resulting
/// single phrase. Fails with the available aliases when the phoneme has
/// no oto entry.
pub fn synth_note(
    voicebank: &Voicebank,
    renderer: &dyn PhraseRenderer,
    alias: &str,
    tone: i32,
    duration_ms: f64,
) -> Result<Vec<f32>, String> {
    synth_note_validate(voicebank, alias, tone, duration_ms)?;
    let phrase = synth_note_input(voicebank, alias, tone, duration_ms)?;
    renderer
        .render_phrase(&phrase.input)
        .map_err(|e| format!("render failed: {e}"))
}

/// Sorted, deduplicated sample of the voicebank's oto aliases for error
/// messages.
fn available_aliases(voicebank: &Voicebank, max: usize) -> String {
    let mut aliases: Vec<&str> = voicebank
        .otos
        .iter()
        .map(|oto| oto.alias.as_str())
        .collect();
    aliases.sort_unstable();
    aliases.dedup();
    if aliases.len() <= max {
        aliases.join(", ")
    } else {
        format!("{} ({} total)", aliases[..max].join(", "), aliases.len())
    }
}
