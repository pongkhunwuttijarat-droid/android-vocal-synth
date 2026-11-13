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

mod incremental_mix;
pub use incremental_mix::IncrementalTrackMixer;
mod streaming;
pub use streaming::{
    render_mixdown_stream_by_track_from_frame, render_mixdown_stream_by_track_with_pronunciations,
    render_mixdown_stream_with_pronunciations, render_mixdown_stream_with_pronunciations_from_frame,
    StreamMixdownReport, STREAM_BLOCK_FRAMES, STREAM_MEDIA_SNAPSHOT_MAX_BYTES,
};

use std::path::Path;

#[cfg(feature = "parallel")]
use rayon::prelude::*;

use crate::chunking;
use domain::{UPart, UProject, UTrack, UVoicePart};
use feed::render_input::RenderInput;
use media::MediaSource;
use phonemizer::{
    composition::{CompositionRequest, Language, MappingMode, PhonemeComposer, Strategy, Style},
    make_phoneme, DefaultPhonemizer, EnglishCvvcPhonemizer, JapaneseVcvPhonemizer, Phonemizer,
    TimingEngine,
};
use phrase::{PhraseBuilder, PhraseGroup, PhraseGrouping};
use runtime::mixer::{mix, AudioChunk, MixInput, TrackSpec};

/// Post-mix FX applied to the final mixed stream.
///
/// Two implementations exist: the dlopen'd `libmixerfx.so` (native, `ffi`)
/// and the pure-Rust chain [`mixer_fx::MixerChain`] (all targets, incl.
/// wasm). The pipeline only needs "process this block".
pub trait PostMixFx {
    fn process(&mut self, samples: &mut [f32], pos_ms: f64) -> Result<(), String>;

    /// Legacy final-mix plugins have a mono sample-stream ABI. Refuse stereo
    /// audio until an implementation provides independent channel state.
    fn process_stereo(&mut self, _samples: &mut [f32], _pos_ms: f64) -> Result<(), String> {
        Err("legacy post-mix FX does not support stereo; use per-track mix FX".to_string())
    }
}

#[cfg(feature = "ffi")]
impl PostMixFx for mixer_fx::MixerFx {
    fn process(&mut self, samples: &mut [f32], pos_ms: f64) -> Result<(), String> {
        mixer_fx::MixerFx::process(self, samples, pos_ms)
    }
}

impl PostMixFx for mixer_fx::MixerChain {
    fn process(&mut self, samples: &mut [f32], pos_ms: f64) -> Result<(), String> {
        mixer_fx::MixerChain::process(self, samples, pos_ms);
        Ok(())
    }
}
use runtime::RenderCache;
use voicebank::load_voicebank as load_vb;
use voicebank::Voicebank;
#[cfg(feature = "ffi")]
use worldline_plugin::WorldlineRenderer;

/// Output sample rate of the whole pipeline (worldline + mixer).
pub use runtime::mixer::SAMPLE_RATE;

/// Chunk target duration (ms) — incremental re-render granularity.
/// Chunk target: keep normal musical phrases intact so the renderer does not
/// restart its synthesis context at arbitrary time boundaries. Long phrases
/// still split note-atomically to bound memory and enable incremental edits.
/// The previous 4 s threshold cut the same phrase into independently rendered
/// sections, which did not preserve OpenUtau's phrase-level envelope.
pub const CHUNK_TARGET_MS: f64 = 30_000.0;
/// Whole neighbour notes carried into each chunk as crossfade context.
pub const CHUNK_CONTEXT_NOTES: usize = 1;

/// Which phonemizer derives the phonemes of a part.
///
/// The decision itself now lives in the `phonemizer` crate
/// (`phonemizer::selection`), beside the phonemizers it chooses between. It
/// used to be defined here, in a CLI tool crate, so `engine-wasm` had to depend
/// on `synth-cli` purely to learn which phonemizer a track selects.
///
/// Re-exported rather than deleted so the existing `pipeline::PhonemizerKind`
/// call sites keep compiling. There is exactly one implementation.
pub use phonemizer::PhonemizerKind;

/// Load a `.ustx` project: parse YAML, merge legacy fields and build the
/// time axis (`after_load`).
pub fn load_project(path: &Path) -> Result<UProject, String> {
    let yaml =
        std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    parse_project(&yaml).map_err(|e| format!("{}: {e}", path.display()))
}

/// Parse a `.ustx` project from YAML text (no filesystem).
///
/// The browser path loads the project text from storage (OPFS) and parses it
/// here; the disk loader above is this plus the read.
///
/// Delegates to [`UProject::parse_yaml`], which strips the UTF-8 BOM every
/// OpenUtau-written `.ustx` starts with. Without that, no real project loads.
pub fn parse_project(yaml: &str) -> Result<UProject, String> {
    UProject::parse_yaml(yaml)
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
    let mut phonemes = derive_raw_phonemes(part, voicebank, kind);
    TimingEngine.process(
        &part.notes,
        part.position,
        &mut phonemes,
        &project.time_axis,
        Some(voicebank),
    );
    phonemes
}

fn time_phonemes(
    project: &UProject,
    part: &UVoicePart,
    voicebank: &Voicebank,
    mut phonemes: Vec<domain::UPhoneme>,
) -> Vec<domain::UPhoneme> {
    TimingEngine.process(
        &part.notes,
        part.position,
        &mut phonemes,
        &project.time_axis,
        Some(voicebank),
    );
    phonemes
}

fn derive_raw_phonemes(
    part: &UVoicePart,
    voicebank: &Voicebank,
    kind: PhonemizerKind,
) -> Vec<domain::UPhoneme> {
    match kind {
        PhonemizerKind::English => {
            // G2p dictionary: words → Teto-compatible phoneme sequences so
            // plain lyrics (no phonetic hint) still map to oto aliases.
            // Unknown words fall back to the lyric verbatim.
            EnglishCvvcPhonemizer::with_g2p(phonemizer::rusted_utau_dict::rusted_utau_demo_g2p())
                .process(&part.notes, Some(voicebank))
        }
        PhonemizerKind::Japanese => JapaneseVcvPhonemizer.process(&part.notes, Some(voicebank)),
        // T12: OpenUtau DefaultPhonemizer parity (one note -> one phoneme).
        PhonemizerKind::Default => DefaultPhonemizer.process(&part.notes, Some(voicebank)),
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
    }
}

/// Derive phonemes while honoring committed session choices. Candidate aliases
/// are recomputed against the current bank; the session stores semantic phones,
/// never stale bank aliases.
pub fn derive_phonemes_with_choices(
    project: &UProject,
    part: &UVoicePart,
    voicebank: &Voicebank,
    kind: PhonemizerKind,
    choices: &[Option<domain::UPronunciation>],
) -> Result<Vec<domain::UPhoneme>, String> {
    if choices.len() != part.notes.len() {
        return Err(format!(
            "committed pronunciation count {} does not match note count {}",
            choices.len(),
            part.notes.len()
        ));
    }
    let base = derive_raw_phonemes(part, voicebank, kind);
    derive_phonemes_with_choices_and_base(project, part, voicebank, choices, base)
}

fn derive_phonemes_with_choices_and_base(
    project: &UProject,
    part: &UVoicePart,
    voicebank: &Voicebank,
    choices: &[Option<domain::UPronunciation>],
    base: Vec<domain::UPhoneme>,
) -> Result<Vec<domain::UPhoneme>, String> {
    if choices.len() != part.notes.len() {
        return Err(format!(
            "committed pronunciation count {} does not match note count {}",
            choices.len(),
            part.notes.len()
        ));
    }
    let has_vcv = choices
        .iter()
        .flatten()
        .any(|choice| choice.strategy == Strategy::JapaneseVcv.as_str());
    let vcv = has_vcv.then(|| {
        let mut notes = part.notes.clone();
        for (index, choice) in choices.iter().enumerate() {
            if let Some(choice) = choice
                .as_ref()
                .filter(|choice| choice.strategy == Strategy::JapaneseVcv.as_str())
            {
                if let Some(source) = choice.language_phones.first() {
                    notes[index].lyric = source.clone();
                }
            }
        }
        JapaneseVcvPhonemizer.process(&notes, Some(voicebank))
    });
    let composer = PhonemeComposer::new();
    let mut phonemes = Vec::new();
    for (note_index, note) in part.notes.iter().enumerate() {
        let Some(choice) = choices[note_index].as_ref() else {
            phonemes.extend(
                base.iter()
                    .filter(|phoneme| phoneme.parent == Some(note_index))
                    .cloned(),
            );
            continue;
        };
        if choice.strategy == Strategy::JapaneseVcv.as_str() {
            let contextual = vcv.as_ref().expect("VCV selections built above");
            let selected: Vec<_> = contextual
                .iter()
                .filter(|phoneme| phoneme.parent == Some(note_index))
                .cloned()
                .collect();
            if selected.is_empty() {
                return Err(format!(
                    "committed Japanese VCV pronunciation produced no phoneme for note {note_index}"
                ));
            }
            phonemes.extend(selected);
            continue;
        }
        let candidate = resolve_committed_candidate(&composer, note, choice, voicebank)?;
        if candidate.voicebank_aliases.is_empty() {
            return Err(format!(
                "committed pronunciation produced no aliases for note {note_index}"
            ));
        }
        let count = candidate.voicebank_aliases.len() as i32;
        let base_duration = note.duration / count;
        let remainder = note.duration % count;
        let mut position = 0;
        for (index, alias) in candidate.voicebank_aliases.into_iter().enumerate() {
            let duration = base_duration + i32::from((index as i32) < remainder);
            phonemes.push(make_phoneme(
                alias,
                position,
                duration,
                index as i32,
                note.tone,
                note_index,
            ));
            position += duration;
        }
    }
    Ok(time_phonemes(project, part, voicebank, phonemes))
}

fn resolve_committed_candidate(
    composer: &PhonemeComposer,
    note: &domain::UNote,
    choice: &domain::UPronunciation,
    voicebank: &Voicebank,
) -> Result<phonemizer::composition::CompositionCandidate, String> {
    let language = match choice.language.as_str() {
        "ja" => Language::Japanese,
        "en" => Language::English,
        "th" => Language::Thai,
        other => return Err(format!("unsupported committed language {other:?}")),
    };
    let mode = match choice.mode.as_str() {
        "universal" => MappingMode::Universal,
        "voicebank_native" => MappingMode::VoicebankNative,
        other => return Err(format!("unsupported committed mapping mode {other:?}")),
    };
    let style = match choice.style.as_str() {
        "cv" => Style::Cv,
        "vcv" => Style::Vcv,
        "cvvc" => Style::Cvvc,
        "vccv" => Style::Vccv,
        other => return Err(format!("unsupported committed style {other:?}")),
    };
    let request = CompositionRequest {
        input: note.lyric.clone(),
        language: Some(language),
        mode,
        style,
        limit: Some(phonemizer::composition::MAX_LIMIT),
        tone: note.tone,
        voicebank_id: Some(voicebank.id.clone()),
    };
    let result = composer.compose(&request, Some(voicebank));
    result
        .candidates
        .into_iter()
        .find(|candidate| {
            candidate.strategy.as_str() == choice.strategy
                && candidate.style.as_str() == choice.style
                && candidate.language_phones == choice.language_phones
                && candidate.universal_phones == choice.universal_phones
        })
        .ok_or_else(|| {
            format!(
                "committed pronunciation no longer resolves for {:?} on voicebank {}",
                note.lyric, voicebank.id
            )
        })
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
    build_phrase_inputs_with_choices(project, track, part, voicebank, kind, None)
}

pub fn build_phrase_inputs_with_choices(
    project: &UProject,
    track: &UTrack,
    part: &UVoicePart,
    voicebank: &Voicebank,
    kind: PhonemizerKind,
    choices: Option<&[Option<domain::UPronunciation>]>,
) -> Result<Vec<PhraseInput>, String> {
    // The named Japanese Presamp plugin is executed by the managed OpenUtau
    // host on native targets. A committed per-note choice still overrides its
    // note, while unedited notes use the plugin result as the base sequence.
    let force_default =
        std::env::var("RUSTED_UTAU_PHONEMIZER")
            .is_ok_and(|value| value.eq_ignore_ascii_case("default"));
    let external_base = if !force_default
        && track.phonemizer.as_deref().is_some_and(|name| {
            name.eq_ignore_ascii_case(crate::openutau_phonemizer::JAPANESE_PRESAMP)
        }) {
        Some(crate::openutau_phonemizer::derives_for_part(
            project, part, voicebank,
        )?)
    } else if !force_default
        && track.phonemizer.as_deref().is_some_and(|name| {
            let trimmed = name.trim();
            trimmed.starts_with("OpenUtau.")
                && !trimmed.eq_ignore_ascii_case(crate::openutau_phonemizer::JAPANESE_PRESAMP)
        }) {
        let name = track.phonemizer.as_deref().unwrap().trim();
        return Err(format!(
            "track selects unsupported phonemizer plugin '{name}'; only {} is currently supported",
            crate::openutau_phonemizer::JAPANESE_PRESAMP
        ));
    } else {
        None
    };
    let phonemes = match choices {
        Some(choices) => derive_phonemes_with_choices_and_base(
            project,
            part,
            voicebank,
            choices,
            external_base.unwrap_or_else(|| derive_raw_phonemes(part, voicebank, kind)),
        )?,
        None => match external_base {
            Some(phonemes) => time_phonemes(project, part, voicebank, phonemes),
            None => derive_phonemes(project, part, voicebank, kind),
        },
    };
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
///
/// `Debug` prints only the counts and the skip reasons: the sample vector runs
/// to millions of entries and a `{:?}` of it would flood any test failure.
#[derive(Debug)]
pub struct RenderReport {
    /// Phrases that produced audio.
    pub phrases_rendered: usize,
    /// Wave (audio) parts of this track that were resolved and mixed in.
    ///
    /// Zero on every call that was not given a [`MediaSource`].
    pub wave_parts_mixed: usize,
    /// Human-readable reasons for every skipped phrase.
    pub skipped: Vec<String>,
    /// Interleaved render samples at [`SAMPLE_RATE`]. `channels` is one for
    /// the raw synthesis/per-track-export path and two for track previews.
    pub samples: Vec<f32>,
    pub channels: u16,
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
    /// Render one phrase and deliver PCM as contiguous blocks. Renderers
    /// without native mid-render support use a compatibility implementation
    /// that emits blocks after whole-phrase rendering.
    fn render_phrase_stream(
        &self,
        input: &feed::render_input::RenderInput,
        block_samples: usize,
        callback: &mut dyn FnMut(&[f32]) -> Result<(), String>,
    ) -> Result<usize, String> {
        if block_samples == 0 {
            return Err("stream block size must be nonzero".to_string());
        }
        let samples = self.render_phrase(input)?;
        for block in samples.chunks(block_samples) {
            callback(block)?;
        }
        Ok(samples.len())
    }
    /// Synthesis/expression-curve frame spacing used by this renderer.
    fn curve_frame_ms(&self) -> f64 {
        worldline_plugin::DEFAULT_FRAME_MS
    }
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

#[cfg(feature = "ffi")]
impl PhraseRenderer for WorldlineRenderer {
    fn render_phrase(&self, input: &feed::render_input::RenderInput) -> Result<Vec<f32>, String> {
        WorldlineRenderer::render_phrase(self, input).map_err(|e| e.to_string())
    }
    fn render_phrase_stream(
        &self,
        input: &feed::render_input::RenderInput,
        block_samples: usize,
        callback: &mut dyn FnMut(&[f32]) -> Result<(), String>,
    ) -> Result<usize, String> {
        WorldlineRenderer::render_phrase_stream(self, input, block_samples, callback)
            .map_err(|e| e.to_string())
    }
    fn cache_salt(&self) -> u64 {
        // "worldline-r" — fnv-ish from the engine name. Curve frame spacing
        // changes PCM and must partition persistent cache entries.
        0x726c31_0000_0001 ^ self.frame_ms().to_bits()
    }
    fn curve_frame_ms(&self) -> f64 {
        self.frame_ms()
    }
    fn parallel_safe(&self) -> bool {
        // The C++ WORLD's randn state is thread-local since the
        // thread-safety fix — concurrent chunk renders are verified
        // byte-identical (deterministic).
        true
    }
}

/// Build the per-chunk render input.
///
/// Slices the phrase's phonemes to the chunk's context range, aligns
/// synthesis frame zero to the source phrase's frame lattice, preserves the
/// absolute curve-grid origin, and slices the oto list to match.
///
/// Extracted from [`render_project_with`] so a chunk renders identically
/// whether it runs inside this pipeline (sequential or rayon-parallel) or in
/// a wasm worker (`engine-wasm`), which fans chunks across Web Workers.
pub fn chunk_render_input(
    phrase: &PhraseInput,
    chunk: &chunking::Chunk,
    frame_ms: f64,
) -> RenderInput {
    let mut sub = phrase.input.clone();
    sub.phonemes = phrase.input.phonemes[chunk.ctx_range.clone()].to_vec();
    // Keep phoneme-local phrase metadata, but align synthesis frame zero to
    // the original phrase's frame lattice. A chunk-local 10 ms reset changes
    // which f0/gender/tension samples land on WORLD frames.
    let ctx_first = &phrase.input.phonemes[chunk.ctx_range.start];
    let ctx_last = &phrase.input.phonemes[chunk.ctx_range.end - 1];
    sub.phrase.position_ms = ctx_first.position_ms;
    sub.phrase.leading_ms = ctx_first.leading_ms;
    sub.phrase.duration_ms = (ctx_last.position_ms + ctx_last.duration_ms) - ctx_first.position_ms;
    sub.phrase.leading_ticks = time_axis_tick_at_ms(&phrase.input, ctx_first.position_ms)
        - time_axis_tick_at_ms(&phrase.input, ctx_first.position_ms - ctx_first.leading_ms);
    let desired_origin_ms = ctx_first.position_ms - ctx_first.leading_ms;
    let frame_ms = frame_ms.max(1e-9);
    let frame_offset = ((desired_origin_ms - phrase.input.render_origin_ms) / frame_ms).floor();
    sub.render_origin_ms = phrase.input.render_origin_ms + frame_offset * frame_ms;
    // These arrays live on an absolute 5-tick grid. Slice from the last grid
    // sample at/before this chunk's pitch origin, and carry that exact sample
    // tick forward so consumers retain the fractional phase to the local
    // phrase/frame origin.
    let grid = worldline_plugin::Grid::new(&phrase.input);
    let skip = grid.index_at(sub.render_origin_ms).floor().max(0.0) as usize;
    sub.curve_grid_start_tick = phrase.input.curve_grid_start_tick + (skip as i32) * 5;
    if skip > 0 {
        sub.pitches_cents.drain(..skip.min(sub.pitches_cents.len()));
        sub.pitches_cents_f32
            .drain(..skip.min(sub.pitches_cents_f32.len()));
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
    // sample_based.oto is per-phoneme (1:1 with `phonemes` after the
    // partial-mapping gate) — slice it to match the chunk's context range.
    if let (Some(sb), Some(base)) = (
        sub.sample_based.as_mut(),
        phrase.input.sample_based.as_ref(),
    ) {
        sb.oto = base.oto[chunk.ctx_range.clone()].to_vec();
    }
    sub
}

/// OpenUtau `TimeAxis.MsPosToTickPos`, using the phrase's project tempo map.
fn time_axis_tick_at_ms(input: &RenderInput, ms: f64) -> i32 {
    let Some(segment) = input
        .phrase
        .tempo_map
        .iter()
        .rev()
        .find(|point| point.ms_pos <= ms)
        .or_else(|| input.phrase.tempo_map.first())
    else {
        let bpm = worldline_plugin::parse_tempo_hint(input.phrase.time_axis_hint.as_deref())
            .unwrap_or(worldline_plugin::DEFAULT_TEMPO_BPM);
        return (ms * bpm / 125.0).round_ties_even() as i32;
    };
    (segment.tick_pos as f64 + (ms - segment.ms_pos) * segment.bpm / 125.0).round_ties_even() as i32
}

/// Apply the chunk's sampled `dyn` curve to a rendered chunk buffer.
///
/// `dyn` is the one expression descriptor the engine does NOT apply itself:
/// [`PhraseSynthRs::set_curves`](worldline_rs) takes f0 / gender / tension /
/// breathiness / voicing, while `render_input::Curves::dynamics` (already
/// converted from dB to linear by `phrase::builder`) was only ever carried
/// through the chunk hash. Without this the volume expression saves to the
/// document and renders as silence-level no-op.
///
/// The curve array keeps its absolute 5-tick grid origin. Each output sample
/// maps through the project tempo map before interpolation, so chunk starts and
/// tempo changes do not reset or stretch the curve phase.
pub fn apply_chunk_dynamics(input: &RenderInput, rendered: &mut [f32]) {
    apply_chunk_dynamics_at(input, 0, rendered);
}

/// Apply dynamics to a contiguous renderer callback whose first sample has
/// `sample_offset` samples preceding it in the full render buffer.
pub fn apply_chunk_dynamics_at(input: &RenderInput, sample_offset: usize, rendered: &mut [f32]) {
    let dynamics = &input.curves.dynamics;
    if rendered.is_empty() || dynamics.is_empty() || dynamics.iter().all(|gain| *gain == 1.0) {
        return;
    }
    let start_ms = input.render_origin_ms;
    let grid = worldline_plugin::Grid::new(input);
    for (frame, sample) in rendered.iter_mut().enumerate() {
        let frame = sample_offset.saturating_add(frame);
        let ms = start_ms + frame as f64 * 1000.0 / f64::from(SAMPLE_RATE);
        let index = grid.index_at(ms);
        if index >= 0.0 && index < dynamics.len() as f64 {
            *sample *= grid.sample_lerped(dynamics, ms);
        }
    }
}

/// Trim a rendered (own+context) buffer back to the chunk's own span.
///
/// Returns `(samples, position_ms, leading_ms)`: the samples start at
/// `position_ms - leading_ms` on the project timeline, which is what the
/// mixer aligns by.
pub fn trim_chunk_audio(
    phrase: &PhraseInput,
    chunk: &chunking::Chunk,
    render_origin_ms: f64,
    rendered: &[f32],
) -> (Vec<f32>, f64, f64) {
    let first_lead = phrase.input.phonemes[chunk.own_range.start].leading_ms;
    let range = chunk_audio_sample_range(
        chunk,
        first_lead,
        phrase.input.phonemes.len(),
        render_origin_ms,
        rendered.len(),
    );
    let samples = if range.start < range.end {
        rendered[range].to_vec()
    } else {
        Vec::new()
    };
    // position_ms is the NOTE/chunk start, not the buffer origin: the mixer
    // aligns chunks at (position_ms - leading_ms) (runtime::mixer::mix and its
    // alignment_uses_position_minus_leading test), subtracting the leading
    // exactly once.
    (samples, chunk.start_ms, first_lead)
}

/// Raw renderer sample range retained by [`trim_chunk_audio`]. Exposed to the
/// incremental callback path so it can discard context/tail samples as they
/// arrive without buffering the whole phrase.
pub fn chunk_audio_sample_range(
    chunk: &chunking::Chunk,
    first_lead: f64,
    phrase_phoneme_count: usize,
    render_origin_ms: f64,
    rendered_len: usize,
) -> std::ops::Range<usize> {
    let audio_start_ms = render_origin_ms;
    let sr = f64::from(SAMPLE_RATE) / 1000.0;
    let start_i = (((chunk.start_ms - first_lead) - audio_start_ms) * sr).max(0.0) as usize;
    let plan_end_i = ((chunk.end_ms - audio_start_ms) * sr) as usize;
    // The LAST chunk of a phrase must NOT be capped at the plan-derived
    // `chunk.end_ms`: that bound is computed from the phoneme spans (engine
    // independent), while the renderer's own buffer is authoritative and can
    // be LONGER — r1 returns +61 samples past the plan, r2 returns +511
    // (= hop-1). Capping there silently truncated the tail fade-out, which is
    // why r2 came out the same length as r1 (T20/T21).
    // Non-final chunks keep the cap: their render carries the NEXT chunk's
    // context, which must not be spliced in twice.
    let is_last_chunk = chunk.own_range.end == phrase_phoneme_count;
    let end_i = if is_last_chunk {
        rendered_len
    } else {
        plan_end_i.min(rendered_len)
    };
    start_i.min(end_i)..end_i
}

/// One entry of a track's chunk plan — the unit of parallel work.
#[derive(Debug, Clone)]
pub struct PlannedChunk {
    /// Index into the track's phrase list.
    pub phrase_index: usize,
    /// The chunk (phoneme ranges + keep-span).
    pub chunk: chunking::Chunk,
    /// Start of the chunk's own NOTE span on the project timeline — NOT the
    /// buffer origin. The mixer aligns every chunk at
    /// `position_ms - leading_ms` (runtime::mixer::mix), subtracting the
    /// leading exactly once. Reporting `start_ms - leading` here (the buffer
    /// origin) made the pool/worker path subtract it a SECOND time, shifting
    /// every chunk `leading_ms` early (T24/T26).
    pub position_ms: f64,
    pub leading_ms: f64,
}

struct PhraseRenderWork<'a> {
    part_name: String,
    phrase: PhraseInput,
    renderer: &'a dyn PhraseRenderer,
}

struct ChunkRenderJob {
    phrase_index: usize,
    chunk: chunking::Chunk,
}

/// Plan every renderable chunk of `track_no`, in render order.
///
/// Phrases that cannot render (no/partial oto mapping) are reported in
/// `skipped` and produce no chunks. A host can fan the plan out across
/// workers and render one entry with [`render_planned_chunk`]; mixing the
/// results in plan order with `runtime::mixer::mix` reproduces
/// `render_project` exactly.
pub fn plan_track_chunks(
    project: &UProject,
    voicebank: &Voicebank,
    track_no: i32,
    kind: PhonemizerKind,
) -> Result<(Vec<PhraseInput>, Vec<PlannedChunk>, Vec<String>), String> {
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

    let mut phrases_all: Vec<PhraseInput> = Vec::new();
    let mut planned: Vec<PlannedChunk> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    for part in parts {
        for phrase in build_phrase_inputs(project, track, part, voicebank, kind)? {
            let phrase_index = phrases_all.len();
            let names = |input: &RenderInput| -> String {
                input
                    .phonemes
                    .iter()
                    .map(|p| p.phoneme.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            if phrase.input.sample_based.is_none() {
                skipped.push(format!(
                    "part '{}' phrase @{:.0} ms: no phoneme maps to an oto alias ({})",
                    part.name,
                    phrase.group.position_ms,
                    names(&phrase.input)
                ));
                continue;
            }
            let sb_oto_len = phrase
                .input
                .sample_based
                .as_ref()
                .map(|sb| sb.oto.len())
                .unwrap_or(0);
            if sb_oto_len != phrase.input.phonemes.len() {
                skipped.push(format!(
                    "part '{}' phrase @{:.0} ms: render failed — partial oto mapping ({} of {}) ({})",
                    part.name,
                    phrase.group.position_ms,
                    sb_oto_len,
                    phrase.input.phonemes.len(),
                    names(&phrase.input)
                ));
                continue;
            }
            let chunks = chunking::plan_chunks(
                &phrase.input.phonemes,
                &phrase.input.notes,
                CHUNK_TARGET_MS,
                CHUNK_CONTEXT_NOTES,
            );
            let first_lead_of = |c: &chunking::Chunk| -> f64 {
                phrase.input.phonemes[c.own_range.start].leading_ms
            };
            let entries: Vec<PlannedChunk> = chunks
                .into_iter()
                .map(|chunk| PlannedChunk {
                    phrase_index,
                    position_ms: chunk.start_ms,
                    leading_ms: first_lead_of(&chunk),
                    chunk,
                })
                .collect();
            phrases_all.push(phrase);
            planned.extend(entries);
        }
    }
    Ok((phrases_all, planned, skipped))
}

/// Render one planned chunk (one unit of parallel work) → its trimmed PCM.
pub fn render_planned_chunk(
    phrases: &[PhraseInput],
    planned: &PlannedChunk,
    renderer: &dyn PhraseRenderer,
) -> Result<Vec<f32>, String> {
    let phrase = &phrases[planned.phrase_index];
    let sub = chunk_render_input(phrase, &planned.chunk, renderer.curve_frame_ms());
    let mut rendered = renderer.render_phrase(&sub)?;
    apply_chunk_dynamics(&sub, &mut rendered);
    let (samples, _, _) = trim_chunk_audio(phrase, &planned.chunk, sub.render_origin_ms, &rendered);
    Ok(samples)
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
    mixer: Option<&mut dyn PostMixFx>,
    cancel: Option<&std::sync::atomic::AtomicBool>,
) -> Result<RenderReport, String> {
    render_project_with(
        project, voicebank, renderer, track_no, kind, verbose, cache, mixer, cancel, None,
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
    mixer: Option<&mut dyn PostMixFx>,
    cancel: Option<&std::sync::atomic::AtomicBool>,
    engine_by_phrase: Option<&dyn Fn(usize) -> &'static dyn PhraseRenderer>,
) -> Result<RenderReport, String> {
    render_project_with_progress(
        project,
        voicebank,
        renderer,
        track_no,
        kind,
        verbose,
        cache,
        mixer,
        cancel,
        engine_by_phrase,
        None,
    )
}

/// One chunk of a render finished (or the plan is known).
///
/// `done`/`total` are chunk counts. The callback is called once with
/// `done = 0` as soon as the chunk plan exists, then after every chunk that
/// completes — the coarsest granularity the pipeline can report without
/// changing what is rendered. Callers must not read it as audio progress:
/// the last chunks of a track can differ wildly in cost.
pub type ProgressFn<'a> = &'a (dyn Fn(usize, usize) + Sync);

/// Per-track outcome of a [`render_mixdown`].
#[derive(Debug, Clone, PartialEq)]
pub struct MixdownTrackReport {
    pub track_no: i32,
    /// Voice phrases that produced audio on this track.
    pub phrases_rendered: usize,
    /// Wave parts that were resolved and mixed in.
    pub wave_parts_mixed: usize,
    /// Every reason something on this track did not make it into the mix.
    pub skipped: Vec<String>,
}

/// Result of rendering a whole project (all tracks + wave parts) into one stream.
#[derive(Debug, Clone, PartialEq)]
pub struct MixdownReport {
    /// The mixed interleaved stereo stream at [`SAMPLE_RATE`].
    pub samples: Vec<f32>,
    pub channels: u16,
    pub sample_rate: u32,
    pub tracks: Vec<MixdownTrackReport>,
}

/// Build one `MixInput` for a wave part: resolve, decode, trim, fade, place.
///
/// Every step after the bytes is engine-owned; only `media.load` is the host's.
/// A part whose media cannot be resolved is **not** silently dropped — the
/// reason is returned so the caller can put it in the skip list.
pub fn wave_part_mix_input(
    part: &domain::UWavePart,
    project: &UProject,
    track: TrackSpec,
    media: &dyn MediaSource,
) -> Result<MixInput, String> {
    let reference = part
        .relative_path
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .ok_or_else(|| format!("wave part '{}' has no relative_path", part.name))?;
    let bytes = media
        .load(reference)
        .map_err(|e| format!("wave part '{}': {e}", part.name))?;
    let decoded = media::decode_mono_44100(&bytes)
        .map_err(|e| format!("wave part '{}' ({reference}): {e}", part.name))?;
    let frames = media::wave_trim_frames(part, &project.time_axis, SAMPLE_RATE, decoded.len());
    let samples = media::apply_wave_trim(&decoded, frames);
    Ok(MixInput {
        chunk: AudioChunk {
            samples,
            position_ms: project.time_axis.tick_to_ms(f64::from(part.position)),
            // A wave part has no preutter: its first sample is the start of the
            // (already-skipped) file, exactly like `WaveSource(offsetMs, …, 0, …)`.
            leading_ms: 0.0,
            hash: 0,
        },
        track,
    })
}

fn wave_part_stereo_samples(
    part: &domain::UWavePart,
    project: &UProject,
    media: &dyn MediaSource,
) -> Result<(Vec<f32>, usize), String> {
    let reference = part
        .relative_path
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .ok_or_else(|| format!("wave part '{}' has no relative_path", part.name))?;
    let bytes = media
        .load(reference)
        .map_err(|e| format!("wave part '{}': {e}", part.name))?;
    let decoded = media::decode_stereo_44100(&bytes)
        .map_err(|e| format!("wave part '{}' ({reference}): {e}", part.name))?;
    let trim = media::wave_trim_frames(part, &project.time_axis, SAMPLE_RATE, decoded.len() / 2);
    let frame_count = decoded.len() / 2;
    let kept = frame_count.saturating_sub(trim.skip + trim.trim);
    let begin = trim.skip * 2;
    let end = begin + kept * 2;
    let mut samples = decoded[begin..end].to_vec();
    let fade_in = trim.fadein.min(kept);
    for frame in 0..fade_in {
        let gain = frame as f32 / fade_in as f32;
        samples[frame * 2] *= gain;
        samples[frame * 2 + 1] *= gain;
    }
    let fade_out = trim.fadeout.min(kept);
    for frame in 0..fade_out {
        let at = kept - 1 - frame;
        let gain = frame as f32 / fade_out as f32;
        samples[at * 2] *= gain;
        samples[at * 2 + 1] *= gain;
    }
    let start = (project.time_axis.tick_to_ms(f64::from(part.position)) * f64::from(SAMPLE_RATE)
        / 1000.0) as usize;
    Ok((samples, start))
}

fn ensure_stereo_frames(samples: &mut Vec<f32>, frames: usize) {
    let needed = frames.saturating_mul(2);
    if samples.len() < needed {
        samples.resize(needed, 0.0);
    }
}

fn add_vocal_bus(samples: &mut Vec<f32>, mono: &[f32], track: &UTrack) {
    if mono.is_empty() {
        return;
    }
    let gain = runtime::mixer::openutau_fader_scale(if track.mute { -24.0 } else { track.volume });
    let angle = (track.pan.clamp(-1.0, 1.0) as f32 + 1.0) * std::f32::consts::FRAC_PI_4;
    let left = angle.cos() * gain;
    let right = angle.sin() * gain;
    ensure_stereo_frames(samples, mono.len());
    for (i, sample) in mono.iter().enumerate() {
        samples[i * 2] += sample * left;
        samples[i * 2 + 1] += sample * right;
    }
}

fn add_wave_bus(samples: &mut Vec<f32>, stereo: &[f32], start: usize, track: &UTrack) {
    if stereo.len() < 2 {
        return;
    }
    let frames = stereo.len() / 2;
    ensure_stereo_frames(samples, start.saturating_add(frames));
    let gain = runtime::mixer::openutau_fader_scale(if track.mute { -24.0 } else { track.volume });
    // OpenUtau's Fader applies the same equal-power pan law to every
    // interleaved channel, including stereo wave parts.
    let angle = (track.pan.clamp(-1.0, 1.0) as f32 + 1.0) * std::f32::consts::FRAC_PI_4;
    let left_gain = angle.cos() * gain;
    let right_gain = angle.sin() * gain;
    for frame in 0..frames {
        let dest = (start + frame) * 2;
        samples[dest] += stereo[frame * 2] * left_gain;
        samples[dest + 1] += stereo[frame * 2 + 1] * right_gain;
    }
}

/// The per-track mix parameters, ported from `RenderEngine`'s fader setup:
/// `Scale = DecibelToVolume(track.Muted ? -24 : track.Volume)`, `Pan = track.Pan`.
///
/// The fader scale is a **linear gain**, not a dB value (`TrackSpec::volume_db`
/// would turn unity into +1 dB), so it goes into `TrackSpec::gain`.
/// Render every track of a project plus its wave parts into one stereo stream.
///
/// This is OpenUtau's *mixdown* export (`PlaybackManager.RenderMixdown`), not
/// its per-track export: each track gets a fader (volume through
/// [`runtime::mixer::openutau_fader_scale`], pan through
/// [`runtime::mixer::pan_gain`], mute as `-24 dB` = silent) and the tracks are
/// summed. Solo is deliberately **not** applied — the reference mixdown ignores
/// it too (`RenderEngine` only reads `Muted`, `Volume` and `Pan`).
///
/// `render_project_with_progress_and_pronunciations` is reused per track so the
/// single-track path keeps its exact behaviour, cache keys and skip reporting;
/// this function only adds the track fader and the wave parts around it.
///
/// `phonemizer_override` forces one phonemizer for every track; `None` (the
/// default) resolves each track's own `phonemizer` field, which is what a
/// multitrack project needs — tracks in one project can legitimately differ.
#[allow(clippy::too_many_arguments)]
pub fn render_mixdown(
    project: &UProject,
    voicebank: &Voicebank,
    renderer: &dyn PhraseRenderer,
    phonemizer_override: Option<PhonemizerKind>,
    verbose: bool,
    cache: &mut Option<RenderCache>,
    media: &dyn MediaSource,
    mixer: Option<&mut dyn PostMixFx>,
    cancel: Option<&std::sync::atomic::AtomicBool>,
    progress: Option<ProgressFn<'_>>,
) -> Result<MixdownReport, String> {
    render_mixdown_with_pronunciations(
        project,
        voicebank,
        renderer,
        phonemizer_override,
        verbose,
        cache,
        media,
        mixer,
        cancel,
        progress,
        None,
    )
}

/// `render_mixdown` with stable-session pronunciation overrides, aligned by
/// engine track order and then by voice part/note as returned by the session.
#[allow(clippy::too_many_arguments)]
pub fn render_mixdown_with_pronunciations(
    project: &UProject,
    voicebank: &Voicebank,
    renderer: &dyn PhraseRenderer,
    phonemizer_override: Option<PhonemizerKind>,
    verbose: bool,
    cache: &mut Option<RenderCache>,
    media: &dyn MediaSource,
    mixer: Option<&mut dyn PostMixFx>,
    cancel: Option<&std::sync::atomic::AtomicBool>,
    progress: Option<ProgressFn<'_>>,
    pronunciations: Option<&[Vec<Vec<Option<domain::UPronunciation>>>]>,
) -> Result<MixdownReport, String> {
    let mut samples = Vec::<f32>::new();
    let mut tracks = Vec::new();
    for track_no in 0..project.tracks.len() {
        let track = &project.tracks[track_no];
        let kind = phonemizer_override.unwrap_or_else(|| PhonemizerKind::from_track(track));
        let mut track_bus = Vec::<f32>::new();
        let mut report = MixdownTrackReport {
            track_no: track_no as i32,
            phrases_rendered: 0,
            wave_parts_mixed: 0,
            skipped: Vec::new(),
        };
        let has_voice = project
            .parts
            .iter()
            .any(|part| matches!(part, UPart::Voice(v) if v.track_no == track_no as i32));
        if has_voice {
            let track_pronunciations = pronunciations
                .and_then(|all| all.get(track_no))
                .map(Vec::as_slice);
            let rendered = render_project_with_progress_and_pronunciations(
                project,
                voicebank,
                renderer,
                track_no as i32,
                kind,
                verbose,
                cache,
                None,
                cancel,
                None,
                progress,
                track_pronunciations,
            )?;
            report.phrases_rendered = rendered.phrases_rendered;
            report.skipped.extend(rendered.skipped);
            if !rendered.samples.is_empty() {
                add_vocal_bus(&mut track_bus, &rendered.samples, track);
            }
        }
        for part in &project.parts {
            let wave = match part {
                UPart::Wave(w) if w.track_no == track_no as i32 => w,
                _ => continue,
            };
            match wave_part_stereo_samples(wave, project, media) {
                Ok((wave_samples, start)) => {
                    report.wave_parts_mixed += 1;
                    add_wave_bus(&mut track_bus, &wave_samples, start, track);
                }
                Err(reason) => report.skipped.push(reason),
            }
        }
        if let Some(fx) = track.mix_fx.as_ref().filter(|fx| fx.enabled) {
            mixer_fx::OpenUtauTrackFx::new(fx).process(&mut track_bus);
        }
        ensure_stereo_frames(&mut samples, track_bus.len() / 2);
        for (dst, src) in samples.iter_mut().zip(track_bus.iter()) {
            *dst += src;
        }
        tracks.push(report);
    }
    if samples.is_empty() {
        // Name the reasons: a project whose only content failed to resolve is
        // exactly the case where "no audio" is useless on its own.
        let reasons: Vec<String> = tracks
            .iter()
            .flat_map(|t| t.skipped.iter().cloned())
            .collect();
        return Err(if reasons.is_empty() {
            "nothing to mix: no track produced voice or wave audio".to_string()
        } else {
            format!(
                "nothing to mix ({} problem(s)): {}",
                reasons.len(),
                reasons.join("; ")
            )
        });
    }
    if let Some(mixer) = mixer {
        mixer.process_stereo(&mut samples, 0.0)?;
    }
    Ok(MixdownReport {
        samples,
        channels: 2,
        sample_rate: SAMPLE_RATE,
        tracks,
    })
}

/// [`render_project_with`] plus a chunk-progress callback.
///
/// Kept as a separate entry point so existing callers (the CLI, tests) stay
/// unchanged; the render worker passes a callback so a v2 render job can
/// report `{done_chunks, total_chunks}` (CONTRACT-v1 §6.13 item 3).
#[allow(clippy::too_many_arguments)]
pub fn render_project_with_progress(
    project: &UProject,
    voicebank: &Voicebank,
    renderer: &dyn PhraseRenderer,
    track_no: i32,
    kind: PhonemizerKind,
    verbose: bool,
    cache: &mut Option<RenderCache>,
    mixer: Option<&mut dyn PostMixFx>,
    cancel: Option<&std::sync::atomic::AtomicBool>,
    engine_by_phrase: Option<&dyn Fn(usize) -> &'static dyn PhraseRenderer>,
    progress: Option<ProgressFn<'_>>,
) -> Result<RenderReport, String> {
    render_project_with_progress_and_pronunciations(
        project,
        voicebank,
        renderer,
        track_no,
        kind,
        verbose,
        cache,
        mixer,
        cancel,
        engine_by_phrase,
        progress,
        None,
    )
}

/// Session-aware variant. `pronunciations` is aligned with voice parts in
/// project order and each part's note order.
#[allow(clippy::too_many_arguments)]
pub fn render_project_with_progress_and_pronunciations(
    project: &UProject,
    voicebank: &Voicebank,
    renderer: &dyn PhraseRenderer,
    track_no: i32,
    kind: PhonemizerKind,
    verbose: bool,
    cache: &mut Option<RenderCache>,
    mixer: Option<&mut dyn PostMixFx>,
    cancel: Option<&std::sync::atomic::AtomicBool>,
    engine_by_phrase: Option<&dyn Fn(usize) -> &'static dyn PhraseRenderer>,
    progress: Option<ProgressFn<'_>>,
    pronunciations: Option<&[Vec<Option<domain::UPronunciation>>]>,
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
    let mut phrase_work = Vec::new();
    let mut jobs = Vec::new();
    for (part_index, part) in parts.into_iter().enumerate() {
        let choices = pronunciations.and_then(|all| all.get(part_index).map(Vec::as_slice));
        for phrase in
            build_phrase_inputs_with_choices(project, track, part, voicebank, kind, choices)?
        {
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
            let phrase_work_index = phrase_work.len();
            jobs.extend(chunk_plan.into_iter().map(|chunk| ChunkRenderJob {
                phrase_index: phrase_work_index,
                chunk,
            }));
            phrase_work.push(PhraseRenderWork {
                part_name: part.name.clone(),
                phrase,
                renderer: phrase_renderer,
            });
        }
    }

    // Schedule the entire track, not one phrase at a time. Most phrases fit
    // in one chunk, so per-phrase Rayon calls left those jobs serialized even
    // when a project had many independent phrases to render.
    //
    // The plan is now final, so report the total before the first chunk lands
    // (§6.13 item 3: progress granularity is the chunk boundary, no finer).
    let total_chunks = jobs.len();
    let done_chunks = std::sync::atomic::AtomicUsize::new(0);
    if let Some(p) = progress {
        p(0, total_chunks);
    }
    let cache_mutex = cache.as_mut().map(|c| std::sync::Mutex::new(&mut *c));
    let render_chunk = |job: &ChunkRenderJob| -> Result<(MixInput, String), String> {
        if let Some(c) = cancel {
            if c.load(std::sync::atomic::Ordering::Relaxed) {
                return Err("render cancelled".to_string());
            }
        }

        let work = &phrase_work[job.phrase_index];
        let phrase = &work.phrase;
        let chunk = &job.chunk;
        let key = chunk.hash(&phrase.input, work.renderer.cache_salt());
        let sub = chunk_render_input(phrase, chunk, work.renderer.curve_frame_ms());
        let render = || {
            work.renderer.render_phrase(&sub).map_err(|e| {
                format!(
                    "part '{}' phrase @{:.0} ms chunk {}-{}: render failed: {e}",
                    work.part_name,
                    phrase.group.position_ms,
                    chunk.own_range.start,
                    chunk.own_range.end
                )
            })
        };
        let (rendered, cache_hit) = match &cache_mutex {
            Some(m) => {
                let hit = {
                    let mut c = m.lock().map_err(|e| format!("render cache lock: {e}"))?;
                    c.get(key)
                };
                match hit {
                    Some(h) => (h, true),
                    None => {
                        // Never hold the cache lock while doing expensive DSP.
                        let samples = render()?;
                        let mut c = m.lock().map_err(|e| format!("render cache lock: {e}"))?;
                        let _ = c.put(key, samples.clone());
                        (samples, false)
                    }
                }
            }
            None => (render()?, false),
        };
        // Applied AFTER the cache lookup on purpose: the cache stores the raw
        // engine output, and `chunk.hash` already covers `curves.dynamics`, so a
        // volume edit invalidates the entry anyway.
        let mut rendered = rendered;
        apply_chunk_dynamics(&sub, &mut rendered);
        let (samples, position_ms, leading_ms) =
            trim_chunk_audio(phrase, chunk, sub.render_origin_ms, &rendered);
        let log = format!(
            "  phrase @{:.0} ms chunk {}-{} (own {}-{}): {} samples ({:.0} ms){}",
            phrase.input.phrase.position_ms,
            chunk.ctx_range.start,
            chunk.ctx_range.end,
            chunk.own_range.start,
            chunk.own_range.end,
            samples.len(),
            samples.len() as f64 * 1000.0 / f64::from(SAMPLE_RATE),
            if cache_hit { " (cached)" } else { "" },
        );
        // Count a cached chunk too: it is one chunk of the plan that no longer
        // needs work, which is exactly what a progress bar must show.
        let done = done_chunks.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        if let Some(p) = progress {
            p(done, total_chunks);
        }
        Ok((
            MixInput {
                chunk: AudioChunk {
                    samples,
                    position_ms,
                    leading_ms,
                    hash: key,
                },
                track: TrackSpec::default(),
            },
            log,
        ))
    };

    #[cfg(feature = "parallel")]
    let indexed_results: Result<Vec<(usize, (MixInput, String))>, String> = {
        let (parallel_jobs, serial_jobs): (Vec<_>, Vec<_>) = jobs
            .iter()
            .enumerate()
            .partition(|(_, job)| phrase_work[job.phrase_index].renderer.parallel_safe());
        let parallel_results: Result<Vec<_>, String> = parallel_jobs
            .into_par_iter()
            .map(|(index, job)| render_chunk(job).map(|result| (index, result)))
            .collect();
        let mut results = parallel_results?;
        let serial_results: Result<Vec<_>, String> = serial_jobs
            .into_iter()
            .map(|(index, job)| render_chunk(job).map(|result| (index, result)))
            .collect();
        results.extend(serial_results?);
        // Rayon preserves order for an indexed iterator; merge in the
        // sequential jobs and restore the original plan order before mixing.
        results.sort_by_key(|(index, _)| *index);
        Ok(results)
    };
    #[cfg(not(feature = "parallel"))]
    let indexed_results: Result<Vec<(usize, (MixInput, String))>, String> = jobs
        .iter()
        .enumerate()
        .map(|(index, job)| render_chunk(job).map(|result| (index, result)))
        .collect();
    for (_, (chunk, log)) in indexed_results? {
        if verbose {
            println!("{log}");
        }
        chunks.push(chunk);
    }

    let audio = mix(&chunks);
    let mut samples = audio.samples;
    // Mixer FX plugin: process the final mixed track (gain -> EQ -> comp
    // -> soft clip in C++). Passthrough (default params) must not change
    // the audio — verified against golden.
    if let Some(mixer) = mixer {
        mixer.process_stereo(&mut samples, 0.0)?;
    }
    Ok(RenderReport {
        phrases_rendered: chunks.len(),
        wave_parts_mixed: 0,
        skipped,
        samples,
        channels: 1,
    })
}

/// One track of a project, voice parts AND wave parts, as one stream.
///
/// This is the per-track stream of OpenUtau's `RenderPreview`
/// (`core/OpenUtau.Core/Render/RenderEngine.cs:92-99`) in the one respect that
/// was missing: it adds every `UWavePart` whose `TrackNo` matches to the
/// track's rendered phrases. `RenderTracks` — the per-file *export* — adds no
/// wave parts at all, which is why [`render_project_with`] (unchanged, still
/// voice-only) stays the export path.
///
/// `media` is the host's asset resolver. Wave parts it cannot resolve are
/// reported in [`RenderReport::skipped`] with the reason rather than dropped,
/// so an audio track whose file moved says so instead of playing nothing.
///
/// A track with only wave parts and no voice part used to fail outright
/// ("no voice part found on track N"); it now renders, which is what the
/// reference does with it.
#[allow(clippy::too_many_arguments)]
pub fn render_track_with_media(
    project: &UProject,
    voicebank: &Voicebank,
    renderer: &dyn PhraseRenderer,
    track_no: i32,
    kind: PhonemizerKind,
    verbose: bool,
    cache: &mut Option<RenderCache>,
    media: &dyn MediaSource,
    mixer: Option<&mut dyn PostMixFx>,
    cancel: Option<&std::sync::atomic::AtomicBool>,
    engine_by_phrase: Option<&dyn Fn(usize) -> &'static dyn PhraseRenderer>,
    progress: Option<ProgressFn<'_>>,
    pronunciations: Option<&[Vec<Option<domain::UPronunciation>>]>,
) -> Result<RenderReport, String> {
    project.tracks.get(track_no as usize).ok_or_else(|| {
        format!(
            "track {track_no} not found (project has {} tracks)",
            project.tracks.len()
        )
    })?;
    let track = &project.tracks[track_no as usize];
    let has_voice = project
        .parts
        .iter()
        .any(|part| matches!(part, UPart::Voice(v) if v.track_no == track_no));

    let mut samples = Vec::<f32>::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut phrases_rendered = 0usize;
    let mut wave_parts_mixed = 0usize;

    if has_voice {
        let rendered = render_project_with_progress_and_pronunciations(
            project,
            voicebank,
            renderer,
            track_no,
            kind,
            verbose,
            cache,
            None,
            cancel,
            engine_by_phrase,
            progress,
            pronunciations,
        )?;
        phrases_rendered = rendered.phrases_rendered;
        skipped.extend(rendered.skipped);
        if !rendered.samples.is_empty() {
            add_vocal_bus(&mut samples, &rendered.samples, track);
        }
    }

    for part in &project.parts {
        let wave = match part {
            UPart::Wave(w) if w.track_no == track_no => w,
            _ => continue,
        };
        match wave_part_stereo_samples(wave, project, media) {
            Ok((wave_samples, start)) => {
                wave_parts_mixed += 1;
                add_wave_bus(&mut samples, &wave_samples, start, track);
            }
            Err(reason) => skipped.push(reason),
        }
    }

    if samples.is_empty() && !has_voice {
        // Only a track with nothing on it at all is an error. A voice track
        // whose phrases were all skipped stays a successful EMPTY render,
        // exactly like `render_project_with_progress_and_pronunciations`
        // (which reports the skip reasons without failing) — the caller
        // decides what an empty stream means.
        return Err(if skipped.is_empty() {
            format!("nothing to render on track {track_no}: it has no voice part and no wave part")
        } else {
            format!(
                "nothing to render on track {track_no} ({} problem(s)): {}",
                skipped.len(),
                skipped.join("; ")
            )
        });
    }
    if let Some(fx) = track.mix_fx.as_ref().filter(|fx| fx.enabled) {
        mixer_fx::OpenUtauTrackFx::new(fx).process(&mut samples);
    }
    if let Some(mixer) = mixer {
        mixer.process(&mut samples, 0.0)?;
    }
    Ok(RenderReport {
        phrases_rendered,
        wave_parts_mixed,
        skipped,
        samples,
        channels: 2,
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

#[cfg(test)]
mod chunk_pitch_grid_tests {
    use super::*;
    use feed::render_input::{Curves, PhraseInfo, RenderNote, RenderPhoneme};

    #[test]
    fn chunk_preserves_absolute_pitch_grid_phase_for_integer_and_float_grids() {
        let input = RenderInput {
            phrase: PhraseInfo {
                position_ms: 0.0,
                duration_ms: 1000.0,
                leading_ms: 0.0,
                leading_ticks: 8,
                tempo_map: Vec::new(),
                time_axis_hint: Some("120bpm 4/4".into()),
            },
            notes: vec![
                RenderNote {
                    lyric: "a".into(),
                    tone: 60,
                    position_ms: 0.0,
                    duration_ms: 500.0,
                },
                RenderNote {
                    lyric: "i".into(),
                    tone: 61,
                    position_ms: 500.0,
                    duration_ms: 500.0,
                },
            ],
            phonemes: vec![
                RenderPhoneme {
                    phoneme: "- a".into(),
                    position_ms: 0.0,
                    duration_ms: 500.0,
                    leading_ms: 0.0,
                    overlap_ms: 0.0,
                    tone: 60,
                    adjusted_tempo: 120.0,
                    consonant_velocity: 100.0,
                    volume: 100.0,
                    modulation: 0.0,
                    tone_shift: 0,
                    index: 0,
                    parent_note: 0,
                },
                RenderPhoneme {
                    phoneme: "a i".into(),
                    position_ms: 500.0,
                    duration_ms: 500.0,
                    leading_ms: 25.0,
                    overlap_ms: 0.0,
                    tone: 61,
                    adjusted_tempo: 120.0,
                    consonant_velocity: 100.0,
                    volume: 100.0,
                    modulation: 0.0,
                    tone_shift: 0,
                    index: 0,
                    parent_note: 1,
                },
            ],
            pitches_cents: (0..100).collect(),
            pitches_cents_f32: (0..100).map(|value| value as f32 + 0.25).collect(),
            curve_grid_start_tick: -8,
            render_origin_ms: 0.0,
            curves: Curves {
                dynamics: vec![1.0; 100],
                gender: vec![0.0; 100],
                breathiness: vec![0.0; 100],
                tension: vec![0.0; 100],
                voicing: vec![100.0; 100],
                extra: Vec::new(),
            },
            sample_based: None,
            neural: None,
        };
        let phrase = PhraseInput {
            group: PhraseGroup {
                phonemes: Vec::new(),
                position_ms: 0.0,
                duration_ms: 1000.0,
                leading_ms: 0.0,
            },
            input,
        };
        let chunk = chunking::Chunk {
            ctx_range: 1..2,
            own_range: 1..2,
            start_ms: 500.0,
            end_ms: 1000.0,
        };

        let chunk_input = chunk_render_input(&phrase, &chunk, 10.0);
        assert_eq!(chunk_input.render_origin_ms, 470.0);
        let expected_skip = worldline_plugin::Grid::new(&phrase.input)
            .index_at(chunk_input.render_origin_ms)
            .floor() as usize;
        assert_eq!(expected_skip, 91);
        assert_eq!(chunk_input.pitches_cents[0], 91);
        assert_eq!(chunk_input.pitches_cents_f32[0], 91.25);
        assert_eq!(chunk_input.curve_grid_start_tick, 447);
        assert_eq!(chunk_input.phrase.leading_ticks, 24);
    }
}

#[cfg(test)]
mod committed_pronunciation_tests {
    use super::*;
    use domain::{UNote, UPart, UProject, UVoicePart};
    use phonemizer::composition::{CompositionRequest, Language, MappingMode, Strategy, Style};

    #[test]
    fn render_rederives_the_committed_style_strategy_alias_sequence() {
        let bank = Voicebank::from_oto_bytes(
            "fixture",
            "fixture",
            ".",
            ".",
            b"s.wav=s,0,100,0,0,0\ni.wav=i,0,100,0,0,0\nt.wav=t,0,100,0,0,0\n",
        );
        let composer = PhonemeComposer::new();
        let request = CompositionRequest {
            input: "city".to_string(),
            language: Some(Language::English),
            mode: MappingMode::VoicebankNative,
            style: Style::Cvvc,
            limit: Some(phonemizer::composition::MAX_LIMIT),
            tone: 60,
            voicebank_id: Some(bank.id.clone()),
        };
        let candidate = composer
            .compose(&request, Some(&bank))
            .candidates
            .into_iter()
            .find(|candidate| candidate.strategy == Strategy::G2pCvvc)
            .expect("built-in demo dictionary contains city");

        let pronunciation = domain::UPronunciation {
            language: candidate.language.as_str().to_string(),
            mode: request.mode.as_str().to_string(),
            style: candidate.style.as_str().to_string(),
            strategy: candidate.strategy.as_str().to_string(),
            language_phones: candidate.language_phones.clone(),
            universal_phones: candidate.universal_phones.clone(),
        };
        let mut note = UNote::default();
        note.lyric = "city".to_string();
        note.duration = 480;
        note.tone = 60;
        let mut part = UVoicePart::new("fixture");
        part.notes.push(note);
        part.duration = 480;
        let mut project = UProject::create();
        project.parts.push(UPart::Voice(part.clone()));
        project.validate().expect("fixture project is valid");

        let actual = derive_phonemes_with_choices(
            &project,
            &part,
            &bank,
            PhonemizerKind::Default,
            &[Some(pronunciation)],
        )
        .expect("committed strategy rederives");
        let actual_aliases: Vec<_> = actual
            .iter()
            .map(|phoneme| phoneme.phoneme.as_str())
            .collect();
        let expected_aliases: Vec<_> = candidate
            .voicebank_aliases
            .iter()
            .map(String::as_str)
            .collect();
        assert_eq!(actual_aliases, expected_aliases);
        assert!(actual.iter().all(|phoneme| phoneme.parent == Some(0)));
    }

    #[test]
    fn japanese_vcv_commits_keep_preceding_note_vowel_context() {
        let bank = Voicebank::from_oto_bytes(
            "fixture",
            "fixture",
            ".",
            ".",
            "ka.wav=- か,0,100,0,0,0\nki.wav=a き,0,100,0,0,0\n".as_bytes(),
        );
        let pronunciation = |mora: &str| domain::UPronunciation {
            language: "ja".to_string(),
            mode: "voicebank_native".to_string(),
            style: "vcv".to_string(),
            strategy: Strategy::JapaneseVcv.as_str().to_string(),
            language_phones: vec![mora.to_string()],
            universal_phones: vec![mora.to_string()],
        };
        let mut first = UNote::default();
        first.lyric = "か".to_string();
        first.duration = 480;
        first.tone = 60;
        let mut second = first.clone();
        second.position = 480;
        second.lyric = "き".to_string();
        let mut part = UVoicePart::new("fixture");
        part.notes = vec![first, second];
        part.duration = 960;
        let mut project = UProject::create();
        project.parts.push(UPart::Voice(part.clone()));
        project.validate().expect("fixture project is valid");

        let actual = derive_phonemes_with_choices(
            &project,
            &part,
            &bank,
            PhonemizerKind::Default,
            &[Some(pronunciation("か")), Some(pronunciation("き"))],
        )
        .expect("contextual VCV strategy rederives");
        let aliases: Vec<_> = actual
            .iter()
            .map(|phoneme| phoneme.phoneme.as_str())
            .collect();
        assert_eq!(aliases, ["- か", "a き"]);
        assert_eq!(actual[0].parent, Some(0));
        assert_eq!(actual[1].parent, Some(1));
    }
}
