//! Ordered, bounded-prefix whole-project mixdown.
//!
//! Voice chunks are rendered in a schedule that respects each track's
//! canonical chunk order. A final-mix prefix is released only before the
//! earliest still-unrendered chunk or wave part, so no later source can alter
//! already-published PCM. Track buses and their FX are processed in timeline
//! order, then summed in project track order.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use domain::{UPart, UProject, UVoicePart};
use media::MediaSource;
use runtime::RenderCache;
use voicebank::Voicebank;

use super::{
    apply_chunk_dynamics_at, build_phrase_inputs_with_choices, chunk_audio_sample_range,
    chunk_render_input, IncrementalTrackMixer, MixdownTrackReport, PhraseInput, PhraseRenderer,
    PlannedChunk, ProgressFn, CHUNK_CONTEXT_NOTES, CHUNK_TARGET_MS, SAMPLE_RATE,
};

/// Number of interleaved stereo frames emitted in one producer block.
pub const STREAM_BLOCK_FRAMES: usize = 2048;
/// Maximum compressed media bytes pinned for one stream. The value is a
/// conservative first host bound; device measurements may lower it.
pub const STREAM_MEDIA_SNAPSHOT_MAX_BYTES: usize = 128 * 1024 * 1024;

/// Immutable, deduplicated wave-asset bytes captured before the first PCM
/// block. This prevents an externally replaced file from changing generation
/// halfway through playback and rejects missing/oversized input before audio
/// is published.
#[derive(Debug)]
struct PinnedMediaSnapshot {
    blobs: HashMap<String, Vec<u8>>,
}

impl PinnedMediaSnapshot {
    fn capture(
        project: &UProject,
        source: &dyn MediaSource,
        max_bytes: usize,
    ) -> Result<Self, String> {
        let mut blobs = HashMap::new();
        let mut total_bytes = 0usize;
        for part in &project.parts {
            let UPart::Wave(wave) = part else { continue };
            let reference = wave
                .relative_path
                .as_deref()
                .map(str::trim)
                .filter(|reference| !reference.is_empty())
                .ok_or_else(|| format!("wave part '{}' has no relative_path", wave.name))?;
            if blobs.contains_key(reference) {
                continue;
            }
            let bytes = source
                .load(reference)
                .map_err(|error| format!("wave part '{}' ({reference}): {error}", wave.name))?;
            total_bytes = total_bytes
                .checked_add(bytes.len())
                .ok_or_else(|| "stream media byte count overflow".to_string())?;
            if total_bytes > max_bytes {
                return Err(format!(
                    "stream media snapshot exceeds limit ({} > {} bytes)",
                    total_bytes, max_bytes
                ));
            }
            blobs.insert(reference.to_string(), bytes);
        }
        Ok(Self { blobs })
    }
}

impl MediaSource for PinnedMediaSnapshot {
    fn load(&self, relative_path: &str) -> Result<Vec<u8>, String> {
        self.blobs
            .get(relative_path)
            .cloned()
            .ok_or_else(|| format!("media '{relative_path}' was not pinned for this stream"))
    }
}

/// Metadata returned after the final PCM block has been accepted by the sink.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamMixdownReport {
    pub channels: u16,
    pub sample_rate: u32,
    pub frames: u64,
    pub tracks: Vec<MixdownTrackReport>,
}

struct PendingWave<'a> {
    part: &'a domain::UWavePart,
    start_frame: i64,
    order: usize,
}

struct ActiveWave {
    samples: Vec<f32>,
    start_frame: i64,
    order: usize,
    complete: bool,
}

struct TrackStream<'a> {
    phrases: Vec<PhraseInput>,
    chunks: Vec<PlannedChunk>,
    suffix_min_start: Vec<i64>,
    next_chunk: usize,
    voice_mix: IncrementalTrackMixer,
    pending_waves: VecDeque<PendingWave<'a>>,
    active_waves: Vec<ActiveWave>,
    fx: Option<mixer_fx::OpenUtauTrackFx>,
    processed_frame: i64,
    report: MixdownTrackReport,
    has_pcm_source: bool,
}

impl TrackStream<'_> {
    fn next_voice_start(&self) -> Option<i64> {
        (self.next_chunk < self.chunks.len()).then(|| self.suffix_min_start[self.next_chunk])
    }

    fn final_input_end(&self) -> i64 {
        self.voice_mix
            .source_end_frame()
            .max(self.active_waves.iter().map(wave_end).max().unwrap_or(0))
    }

    fn has_future_source(&self) -> bool {
        self.next_chunk < self.chunks.len() || !self.pending_waves.is_empty()
    }
}

fn wave_end(wave: &ActiveWave) -> i64 {
    wave.start_frame + (wave.samples.len() / 2) as i64
}

/// Render a full project into ordered, bounded-size final-mix PCM blocks.
///
/// `sink` is called synchronously for each contiguous block. A bounded channel
/// send from the sink provides render backpressure; returning an error stops
/// the producer. The project/session snapshot and pronunciation selections
/// must already be immutable for the duration of this call.
#[allow(clippy::too_many_arguments)]
pub fn render_mixdown_stream_with_pronunciations<F>(
    project: &UProject,
    voicebank: &Voicebank,
    renderer: &dyn PhraseRenderer,
    kind_override: Option<phonemizer::PhonemizerKind>,
    cache: &mut Option<RenderCache>,
    media: &dyn MediaSource,
    cancel: Option<&AtomicBool>,
    progress: Option<ProgressFn<'_>>,
    pronunciations: Option<&[Vec<Vec<Option<domain::UPronunciation>>>]>,
    block_frames: usize,
    sink: F,
) -> Result<StreamMixdownReport, String>
where
    F: FnMut(u64, &[f32]) -> Result<(), String>,
{
    render_mixdown_stream_with_pronunciations_from_frame(
        project,
        voicebank,
        renderer,
        kind_override,
        cache,
        media,
        cancel,
        progress,
        pronunciations,
        block_frames,
        0,
        sink,
    )
}

/// Stream a full project using a shared `voicebank` starting at `start_frame`
/// (44.1 kHz project frame index). Audio strictly before `start_frame` is skipped
/// cleanly while straddling chunks/waves remain bit-exact from `start_frame` onward.
#[allow(clippy::too_many_arguments)]
pub fn render_mixdown_stream_with_pronunciations_from_frame<F>(
    project: &UProject,
    voicebank: &Voicebank,
    renderer: &dyn PhraseRenderer,
    kind_override: Option<phonemizer::PhonemizerKind>,
    cache: &mut Option<RenderCache>,
    media: &dyn MediaSource,
    cancel: Option<&AtomicBool>,
    progress: Option<ProgressFn<'_>>,
    pronunciations: Option<&[Vec<Vec<Option<domain::UPronunciation>>>]>,
    block_frames: usize,
    start_frame: i64,
    sink: F,
) -> Result<StreamMixdownReport, String>
where
    F: FnMut(u64, &[f32]) -> Result<(), String>,
{
    let voicebanks = vec![Some(voicebank); project.tracks.len()];
    render_mixdown_stream_by_track_from_frame(
        project,
        &voicebanks,
        renderer,
        kind_override,
        cache,
        media,
        cancel,
        progress,
        pronunciations,
        block_frames,
        start_frame,
        sink,
    )
}

/// Full-project stream using the committed voicebank binding for each track.
/// `voicebanks` must have one entry per project track; non-voice tracks still
/// occupy their stable track slot so the plan cannot shift with ordering.
#[allow(clippy::too_many_arguments)]
pub fn render_mixdown_stream_by_track_with_pronunciations<F>(
    project: &UProject,
    voicebanks: &[Option<&Voicebank>],
    renderer: &dyn PhraseRenderer,
    kind_override: Option<phonemizer::PhonemizerKind>,
    cache: &mut Option<RenderCache>,
    media: &dyn MediaSource,
    cancel: Option<&AtomicBool>,
    progress: Option<ProgressFn<'_>>,
    pronunciations: Option<&[Vec<Vec<Option<domain::UPronunciation>>>]>,
    block_frames: usize,
    sink: F,
) -> Result<StreamMixdownReport, String>
where
    F: FnMut(u64, &[f32]) -> Result<(), String>,
{
    render_mixdown_stream_by_track_from_frame(
        project,
        voicebanks,
        renderer,
        kind_override,
        cache,
        media,
        cancel,
        progress,
        pronunciations,
        block_frames,
        0,
        sink,
    )
}

/// Playhead-seekable variant of [`render_mixdown_stream_by_track_with_pronunciations`].
#[allow(clippy::too_many_arguments)]
pub fn render_mixdown_stream_by_track_from_frame<F>(
    project: &UProject,
    voicebanks: &[Option<&Voicebank>],
    renderer: &dyn PhraseRenderer,
    kind_override: Option<phonemizer::PhonemizerKind>,
    cache: &mut Option<RenderCache>,
    media: &dyn MediaSource,
    cancel: Option<&AtomicBool>,
    progress: Option<ProgressFn<'_>>,
    pronunciations: Option<&[Vec<Vec<Option<domain::UPronunciation>>>]>,
    block_frames: usize,
    start_frame: i64,
    mut sink: F,
) -> Result<StreamMixdownReport, String>
where
    F: FnMut(u64, &[f32]) -> Result<(), String>,
{
    let emit_from = start_frame.max(0);
    let profile = std::env::var_os("RUSTED_UTAU_STREAM_PROFILE").is_some();
    let stream_started = Instant::now();
    let mut first_emit = false;
    let mut sink = |frame_start: u64, samples: &[f32]| {
        if profile && !first_emit {
            first_emit = true;
            eprintln!(
                "[STREAM_PROFILE] first_mix_block_ms={:.3} frame_start={} frames={}",
                stream_started.elapsed().as_secs_f64() * 1000.0,
                frame_start,
                samples.len() / 2,
            );
        }
        sink(frame_start, samples)
    };
    let planning_started = Instant::now();
    if block_frames == 0 {
        return Err("stream block size must be nonzero".to_string());
    }
    if voicebanks.len() != project.tracks.len() {
        return Err(format!(
            "voicebank bindings contain {} entries for {} tracks",
            voicebanks.len(),
            project.tracks.len()
        ));
    }

    // Resolve all wave inputs before synthesis can publish PCM. A stream must
    // represent one asset generation and must not silently omit a missing
    // project asset after playback has already begun.
    let media_started = Instant::now();
    let pinned_media =
        PinnedMediaSnapshot::capture(project, media, STREAM_MEDIA_SNAPSHOT_MAX_BYTES)?;
    if profile {
        eprintln!(
            "[STREAM_PROFILE] pin_media_ms={:.3} bytes={} elapsed_ms={:.3}",
            media_started.elapsed().as_secs_f64() * 1000.0,
            pinned_media.blobs.values().map(Vec::len).sum::<usize>(),
            stream_started.elapsed().as_secs_f64() * 1000.0,
        );
    }

    let mut tracks = Vec::with_capacity(project.tracks.len());
    let mut total_chunks = 0usize;
    let mut had_any_source = false;
    for (track_no, track) in project.tracks.iter().enumerate() {
        let voice_parts: Vec<&UVoicePart> = project
            .parts
            .iter()
            .filter_map(|part| match part {
                UPart::Voice(voice) if voice.track_no == track_no as i32 => Some(voice),
                _ => None,
            })
            .collect();
        let mut phrases = Vec::new();
        let mut chunks = Vec::new();
        let mut skipped = Vec::new();
        let voicebank = if voice_parts.is_empty() {
            None
        } else {
            Some(voicebanks[track_no].ok_or_else(|| {
                format!("track {track_no} has voice parts but no resolved voicebank")
            })?)
        };
        for (part_index, part) in voice_parts.iter().enumerate() {
            let choices = pronunciations
                .and_then(|all_tracks| all_tracks.get(track_no))
                .and_then(|track_parts| track_parts.get(part_index))
                .map(Vec::as_slice);
            let built = build_phrase_inputs_with_choices(
                project,
                track,
                part,
                voicebank.expect("voice parts require a voicebank"),
                kind_override.unwrap_or_else(|| phonemizer::PhonemizerKind::from_track(track)),
                choices,
            )?;
            for phrase in built {
                let phrase_index = phrases.len();
                let names = phrase
                    .input
                    .phonemes
                    .iter()
                    .map(|phone| phone.phoneme.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                if phrase.input.sample_based.is_none() {
                    skipped.push(format!(
                        "part '{}' phrase @{:.0} ms: no phoneme maps to an oto alias ({names})",
                        part.name, phrase.group.position_ms
                    ));
                    continue;
                }
                let oto_len = phrase
                    .input
                    .sample_based
                    .as_ref()
                    .map(|sb| sb.oto.len())
                    .unwrap_or(0);
                if oto_len != phrase.input.phonemes.len() {
                    skipped.push(format!(
                        "part '{}' phrase @{:.0} ms: render failed — partial oto mapping ({} of {}) ({names})",
                        part.name,
                        phrase.group.position_ms,
                        oto_len,
                        phrase.input.phonemes.len()
                    ));
                    continue;
                }
                let first_leading = |chunk: &crate::chunking::Chunk| {
                    phrase.input.phonemes[chunk.own_range.start].leading_ms
                };
                let plan = crate::chunking::plan_chunks(
                    &phrase.input.phonemes,
                    &phrase.input.notes,
                    CHUNK_TARGET_MS,
                    CHUNK_CONTEXT_NOTES,
                );
                chunks.extend(plan.into_iter().filter(|chunk| {
                    if emit_from <= 0 {
                        return true;
                    }
                    let is_last = chunk.own_range.end == phrase.input.phonemes.len();
                    let safe_end_ms = chunk.end_ms + if is_last { 250.0 } else { 5.0 };
                    runtime::mixer::ms_to_samples_signed(safe_end_ms, SAMPLE_RATE) > emit_from
                }).map(|chunk| PlannedChunk {
                    phrase_index,
                    position_ms: chunk.start_ms,
                    leading_ms: first_leading(&chunk),
                    chunk,
                }));
                had_any_source = true;
                phrases.push(phrase);
            }
        }

        let mut pending_waves = Vec::new();
        for (order, part) in project.parts.iter().enumerate() {
            let UPart::Wave(wave) = part else { continue };
            if wave.track_no != track_no as i32 {
                continue;
            }
            had_any_source = true;
            let start_frame = (project.time_axis.tick_to_ms(f64::from(wave.position))
                * f64::from(SAMPLE_RATE)
                / 1000.0) as i64;
            pending_waves.push(PendingWave {
                part: wave,
                start_frame,
                order,
            });
        }
        pending_waves.sort_by_key(|wave| (wave.start_frame, wave.order));
        let mut suffix_min_start = vec![i64::MAX; chunks.len() + 1];
        for index in (0..chunks.len()).rev() {
            let start = runtime::mixer::ms_to_samples_signed(
                chunks[index].position_ms - chunks[index].leading_ms,
                SAMPLE_RATE,
            );
            suffix_min_start[index] = start.min(suffix_min_start[index + 1]);
        }
        total_chunks += chunks.len();
        tracks.push(TrackStream {
            phrases,
            report: MixdownTrackReport {
                track_no: track_no as i32,
                phrases_rendered: 0,
                wave_parts_mixed: 0,
                skipped,
            },
            fx: track
                .mix_fx
                .as_ref()
                .filter(|fx| fx.enabled)
                .map(mixer_fx::OpenUtauTrackFx::new),
            chunks,
            suffix_min_start,
            next_chunk: 0,
            voice_mix: IncrementalTrackMixer::new_at(emit_from),
            pending_waves: pending_waves.into(),
            active_waves: Vec::new(),
            processed_frame: emit_from,
            has_pcm_source: false,
        });
    }
    if let Some(report) = progress {
        report(0, total_chunks);
    }
    if profile {
        eprintln!(
            "[STREAM_PROFILE] project_plan_ms={:.3} tracks={} voice_chunks={} elapsed_ms={:.3}",
            planning_started.elapsed().as_secs_f64() * 1000.0,
            tracks.len(),
            total_chunks,
            stream_started.elapsed().as_secs_f64() * 1000.0,
        );
    }

    let mut cursor = emit_from;
    let mut done_chunks = 0usize;
    loop {
        if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            return Err("render cancelled".to_string());
        }

        let due_wave_track = tracks.iter().position(|track_state| {
            track_state
                .pending_waves
                .front()
                .is_some_and(|wave| wave.start_frame <= cursor)
        });
        if let Some(track_no) = due_wave_track {
            let pending = tracks[track_no]
                .pending_waves
                .pop_front()
                .expect("due wave was checked");
            let wave_started = Instant::now();
            let part = pending.part;
            let track_position_ms = project.time_axis.tick_to_ms(f64::from(part.position));
            let start_frame = (track_position_ms * f64::from(SAMPLE_RATE) / 1000.0) as i64;
            let trim = media::wave_trim_frames(part, &project.time_axis, SAMPLE_RATE, usize::MAX);
            let hold_frames = trim.trim.saturating_add(trim.fadeout);
            let mut source_offset = 0usize;
            let mut output_offset = 0usize;
            let mut tail = Vec::<f32>::new();
            let active_order = pending.order;
            tracks[track_no].active_waves.push(ActiveWave {
                samples: Vec::new(),
                start_frame,
                order: pending.order,
                complete: false,
            });
            tracks[track_no].active_waves.sort_by_key(|wave| wave.order);
            let bytes = pinned_media.load(
                part.relative_path
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| format!("wave part '{}' has no relative_path", part.name))?,
            )?;
            let mut consume_wave_block = |decoded_block: &[f32]| -> Result<(), String> {
                if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
                    return Err("render cancelled".to_string());
                }
                let decoded_frames = decoded_block.len() / 2;
                let block_source_start = source_offset;
                source_offset = source_offset.saturating_add(decoded_frames);
                let skip = trim
                    .skip
                    .saturating_sub(block_source_start)
                    .min(decoded_frames);
                let mut eligible = if skip < decoded_frames {
                    decoded_block[skip * 2..].to_vec()
                } else {
                    Vec::new()
                };
                tail.extend_from_slice(&eligible);
                eligible.clear();
                let keep_samples = hold_frames.saturating_mul(2);
                let emit_samples = tail.len().saturating_sub(keep_samples);
                let emit_samples = emit_samples - (emit_samples % 2);
                if emit_samples > 0 {
                    let emitted: Vec<f32> = tail.drain(..emit_samples).collect();
                    let active_index = tracks[track_no]
                        .active_waves
                        .iter()
                        .position(|wave| wave.order == active_order)
                        .expect("wave remains active during decode");
                    let fadein = trim.fadein;
                    let mut emitted = emitted;
                    for frame in 0..(emitted.len() / 2) {
                        let index = output_offset + frame;
                        if index < fadein {
                            let gain = index as f32 / fadein.max(1) as f32;
                            emitted[frame * 2] *= gain;
                            emitted[frame * 2 + 1] *= gain;
                        }
                    }
                    let mut added_wave = false;
                    {
                        let active = &mut tracks[track_no].active_waves[active_index];
                        if active.samples.is_empty() && active.start_frame < cursor {
                            let drop_frames = (cursor - active.start_frame)
                                .min((emitted.len() / 2) as i64)
                                .max(0) as usize;
                            active.start_frame += drop_frames as i64;
                            if drop_frames * 2 < emitted.len() {
                                active.samples.extend_from_slice(&emitted[drop_frames * 2..]);
                                added_wave = true;
                            }
                        } else if !emitted.is_empty() {
                            active.samples.extend_from_slice(&emitted);
                            added_wave = true;
                        }
                    }
                    tracks[track_no].has_pcm_source |= added_wave;
                    output_offset += emitted.len() / 2;
                    let available_end = wave_end(&tracks[track_no].active_waves[active_index]);
                    let future_voice = tracks
                        .iter()
                        .filter_map(TrackStream::next_voice_start)
                        .min();
                    let future_wave = tracks
                        .iter()
                        .filter_map(|state| {
                            state.pending_waves.front().map(|wave| wave.start_frame)
                        })
                        .min();
                    let watermark = future_voice
                        .into_iter()
                        .chain(future_wave)
                        .fold(available_end, i64::min)
                        .max(0);
                    if watermark > cursor {
                        flush_until(
                            project,
                            &mut tracks,
                            &mut cursor,
                            watermark,
                            block_frames,
                            cancel,
                            &mut sink,
                        )?;
                    }
                }
                Ok(())
            };
            let reference = part.relative_path.as_deref().unwrap_or("<missing>");
            media::decode_stereo_44100_blocks(&bytes, 4096, &mut consume_wave_block)
                .map_err(|error| format!("wave part '{}' ({reference}): {error}", part.name))?;
            let trim_samples = trim.trim.saturating_mul(2).min(tail.len());
            tail.truncate(tail.len() - trim_samples);
            let fadeout = trim.fadeout.min(tail.len() / 2);
            for frame in 0..fadeout {
                let at = fadeout - 1 - frame;
                let gain = frame as f32 / fadeout.max(1) as f32;
                tail[at * 2] *= gain;
                tail[at * 2 + 1] *= gain;
            }
            let active_index = tracks[track_no]
                .active_waves
                .iter()
                .position(|wave| wave.order == active_order)
                .expect("wave remains active through EOF");
            let mut added_wave = false;
            {
                let active = &mut tracks[track_no].active_waves[active_index];
                if active.samples.is_empty() && active.start_frame < cursor {
                    let drop_frames = (cursor - active.start_frame)
                        .min((tail.len() / 2) as i64)
                        .max(0) as usize;
                    active.start_frame += drop_frames as i64;
                    if drop_frames * 2 < tail.len() {
                        active.samples.extend_from_slice(&tail[drop_frames * 2..]);
                        added_wave = true;
                    }
                } else if !tail.is_empty() {
                    active.samples.extend_from_slice(&tail);
                    added_wave = true;
                }
                active.complete = true;
            }
            tracks[track_no].has_pcm_source |= added_wave;
            tracks[track_no]
                .active_waves
                .retain(|wave| !wave.complete || wave_end(wave) > cursor);
            tracks[track_no].report.wave_parts_mixed += 1;
            if profile {
                eprintln!(
                    "[STREAM_PROFILE] wave_decode_ms={:.3} frames={} elapsed_ms={:.3}",
                    wave_started.elapsed().as_secs_f64() * 1000.0,
                    source_offset,
                    stream_started.elapsed().as_secs_f64() * 1000.0,
                );
            }
            continue;
        }

        let next_voice = tracks
            .iter()
            .filter_map(TrackStream::next_voice_start)
            .min();
        let next_wave = tracks
            .iter()
            .filter_map(|track| track.pending_waves.front().map(|wave| wave.start_frame))
            .min();
        let safe_frame = next_voice.into_iter().chain(next_wave).min();
        if let Some(safe_frame) = safe_frame {
            let safe_frame = safe_frame.max(0);
            if safe_frame > cursor {
                flush_until(
                    project,
                    &mut tracks,
                    &mut cursor,
                    safe_frame,
                    block_frames,
                    cancel,
                    &mut sink,
                )?;
                continue;
            }
        }

        let next_chunk_track = tracks
            .iter()
            .enumerate()
            .filter_map(|(track_no, track)| {
                track.chunks.get(track.next_chunk).map(|chunk| {
                    (
                        runtime::mixer::ms_to_samples_signed(
                            chunk.position_ms - chunk.leading_ms,
                            SAMPLE_RATE,
                        ),
                        track_no,
                    )
                })
            })
            .min_by_key(|(start, track_no)| (*start, *track_no));
        if let Some((start, track_no)) = next_chunk_track {
            if start < cursor && cursor > emit_from {
                return Err(format!(
                    "stream watermark {cursor} passed unrendered voice chunk start {start}"
                ));
            }
            let planned = tracks[track_no].chunks[tracks[track_no].next_chunk].clone();
            let phrase = &tracks[track_no].phrases[planned.phrase_index];
            let sub = chunk_render_input(phrase, &planned.chunk, renderer.curve_frame_ms());
            let key = planned.chunk.hash(&phrase.input, renderer.cache_salt());
            let phrase_position_ms = phrase.group.position_ms;
            let first_lead = phrase.input.phonemes[planned.chunk.own_range.start].leading_ms;
            let phrase_phoneme_count = phrase.input.phonemes.len();
            let chunk = planned.chunk.clone();
            let keep = chunk_audio_sample_range(
                &chunk,
                first_lead,
                phrase_phoneme_count,
                sub.render_origin_ms,
                usize::MAX,
            );
            let project_chunk_start =
                runtime::mixer::ms_to_samples_signed(chunk.start_ms - first_lead, SAMPLE_RATE);
            let mix_input = runtime::mixer::MixInput {
                chunk: runtime::mixer::AudioChunk {
                    samples: Vec::new(),
                    position_ms: chunk.start_ms,
                    leading_ms: first_lead,
                    hash: key,
                },
                track: runtime::mixer::TrackSpec::default(),
            };
            let cached = cache.as_mut().and_then(|cache| cache.get(key));
            let mut cache_samples = Vec::new();
            let mut raw_offset = 0usize;

            // Mark this chunk active before accepting its first callback. The
            // remaining planned chunks then define the safe project watermark
            // while this one is being rendered.
            tracks[track_no].next_chunk += 1;
            let mut consume_block = |raw: &[f32]| -> Result<(), String> {
                if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
                    return Err("render cancelled".to_string());
                }
                let block_start = raw_offset;
                raw_offset = raw_offset.saturating_add(raw.len());
                if cache.is_some() {
                    cache_samples.extend_from_slice(raw);
                }

                let kept_start = block_start.max(keep.start);
                let kept_end = raw_offset.min(keep.end);
                if kept_end > kept_start {
                    let block_lo = kept_start - block_start;
                    let block_hi = kept_end - block_start;
                    let mut samples = raw[block_lo..block_hi].to_vec();
                    apply_chunk_dynamics_at(&sub, kept_start, &mut samples);
                    let chunk_sample_offset = kept_start - keep.start;
                    tracks[track_no].voice_mix.push_fragment(
                        &mix_input,
                        chunk_sample_offset,
                        &samples,
                    )?;
                    tracks[track_no].has_pcm_source |= !samples.is_empty();

                    let available_end = project_chunk_start
                        .saturating_add(chunk_sample_offset as i64)
                        .saturating_add(samples.len() as i64);
                    let future_voice = tracks
                        .iter()
                        .filter_map(TrackStream::next_voice_start)
                        .min();
                    let future_wave = tracks
                        .iter()
                        .filter_map(|track| {
                            track.pending_waves.front().map(|wave| wave.start_frame)
                        })
                        .min();
                    let watermark = future_voice
                        .into_iter()
                        .chain(future_wave)
                        .fold(available_end, i64::min)
                        .max(0);
                    if watermark > cursor {
                        flush_until(
                            project,
                            &mut tracks,
                            &mut cursor,
                            watermark,
                            block_frames,
                            cancel,
                            &mut sink,
                        )?;
                    }
                }
                Ok(())
            };

            if let Some(samples) = cached {
                for block in samples.chunks(4096) {
                    consume_block(block)?;
                }
            } else {
                let synth_started = Instant::now();
                renderer
                    .render_phrase_stream(&sub, 4096, &mut consume_block)
                    .map_err(|error| {
                        format!(
                            "track {track_no} phrase @{phrase_position_ms:.0} ms chunk {}-{}: {error}",
                            chunk.own_range.start,
                            chunk.own_range.end
                        )
                    })?;
                if profile {
                    eprintln!(
                        "[STREAM_PROFILE] phrase_chunk_ms={:.3} track={} phrase_ms={:.1} notes={} elapsed_ms={:.3}",
                        synth_started.elapsed().as_secs_f64() * 1000.0,
                        track_no,
                        phrase_position_ms,
                        chunk.own_range.end.saturating_sub(chunk.own_range.start),
                        stream_started.elapsed().as_secs_f64() * 1000.0,
                    );
                }
                if let Some(cache) = cache.as_mut() {
                    let _ = cache.put(key, cache_samples);
                }
            }
            tracks[track_no].report.phrases_rendered += 1;
            done_chunks += 1;
            if let Some(report) = progress {
                report(done_chunks, total_chunks);
            }
            continue;
        }

        if let Some(next_wave_frame) = next_wave {
            if next_wave_frame > cursor {
                flush_until(
                    project,
                    &mut tracks,
                    &mut cursor,
                    next_wave_frame,
                    block_frames,
                    cancel,
                    &mut sink,
                )?;
                continue;
            }
            // Negative or already-current wave starts should have been
            // activated at the top of the loop.
            return Err("wave event could not advance the stream watermark".to_string());
        }

        let final_frame = tracks
            .iter()
            .map(|track| track.final_input_end())
            .max()
            .unwrap_or(cursor)
            .max(cursor);
        if final_frame > cursor {
            flush_until(
                project,
                &mut tracks,
                &mut cursor,
                final_frame,
                block_frames,
                cancel,
                &mut sink,
            )?;
            continue;
        }
        break;
    }

    if !tracks
        .iter()
        .any(|track| track.has_pcm_source || track.voice_mix.source_end_frame() > emit_from)
        && !(emit_from > 0 && had_any_source)
    {
        let reasons = tracks
            .iter()
            .flat_map(|track| track.report.skipped.iter().cloned())
            .collect::<Vec<_>>();
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

    Ok(StreamMixdownReport {
        channels: 2,
        sample_rate: SAMPLE_RATE,
        frames: (cursor - emit_from).max(0) as u64,
        tracks: tracks.into_iter().map(|track| track.report).collect(),
    })
}

fn flush_until<F>(
    project: &UProject,
    tracks: &mut [TrackStream<'_>],
    cursor: &mut i64,
    target: i64,
    block_frames: usize,
    cancel: Option<&AtomicBool>,
    sink: &mut F,
) -> Result<(), String>
where
    F: FnMut(u64, &[f32]) -> Result<(), String>,
{
    while *cursor < target {
        if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            return Err("render cancelled".to_string());
        }
        let start = *cursor;
        let end = target.min(start.saturating_add(block_frames as i64));
        let frames = usize::try_from(end - start)
            .map_err(|_| "stream block frame range overflow".to_string())?;
        let mut master = vec![0.0f32; frames * 2];

        for (track_no, state) in tracks.iter_mut().enumerate() {
            if !state.has_pcm_source && state.chunks.is_empty() && state.pending_waves.is_empty() {
                continue;
            }
            let source_end = if state.has_future_source() {
                end
            } else {
                state.final_input_end()
            };
            let track_end = end.min(source_end).max(state.processed_frame);
            if track_end <= state.processed_frame {
                continue;
            }
            let track_start = state.processed_frame.max(start);
            if track_start > state.processed_frame {
                return Err(format!(
                    "track {track_no} stream has a frame gap: processed {}, next block starts {track_start}",
                    state.processed_frame
                ));
            }
            let segment_frames = usize::try_from(track_end - track_start)
                .map_err(|_| "track segment frame range overflow".to_string())?;
            let mut bus = vec![0.0f32; segment_frames * 2];
            if !state.chunks.is_empty() {
                let mono = state.voice_mix.drain_before(track_end)?;
                if mono.len() != segment_frames {
                    return Err(format!(
                        "track {track_no} voice prefix has {} frames; expected {segment_frames}",
                        mono.len()
                    ));
                }
                let track = &project.tracks[track_no];
                let gain = runtime::mixer::openutau_fader_scale(if track.mute {
                    -24.0
                } else {
                    track.volume
                });
                let angle = (track.pan.clamp(-1.0, 1.0) as f32 + 1.0) * std::f32::consts::FRAC_PI_4;
                let left = angle.cos() * gain;
                let right = angle.sin() * gain;
                for (frame, sample) in mono.iter().enumerate() {
                    bus[frame * 2] += sample * left;
                    bus[frame * 2 + 1] += sample * right;
                }
            }

            let track = &project.tracks[track_no];
            let gain =
                runtime::mixer::openutau_fader_scale(if track.mute { -24.0 } else { track.volume });
            let angle = (track.pan.clamp(-1.0, 1.0) as f32 + 1.0) * std::f32::consts::FRAC_PI_4;
            let left_gain = angle.cos() * gain;
            let right_gain = angle.sin() * gain;
            for wave in &state.active_waves {
                let wave_start = track_start.max(wave.start_frame);
                let wave_finish = track_end.min(wave_end(wave));
                if wave_finish <= wave_start {
                    continue;
                }
                for frame in wave_start..wave_finish {
                    let source = usize::try_from(frame - wave.start_frame)
                        .map_err(|_| "wave source frame overflow".to_string())?;
                    let dest = usize::try_from(frame - track_start)
                        .map_err(|_| "wave destination frame overflow".to_string())?;
                    bus[dest * 2] += wave.samples[source * 2] * left_gain;
                    bus[dest * 2 + 1] += wave.samples[source * 2 + 1] * right_gain;
                }
            }
            if let Some(fx) = &mut state.fx {
                fx.process(&mut bus);
            }
            let output_offset = usize::try_from(track_start - start)
                .map_err(|_| "track output offset overflow".to_string())?;
            for (index, sample) in bus.iter().enumerate() {
                master[output_offset * 2 + index] += sample;
            }
            state.processed_frame = track_end;
        }

        sink(start as u64, &master)?;
        *cursor = end;
        for state in tracks.iter_mut() {
            for wave in &mut state.active_waves {
                let consumed = (*cursor - wave.start_frame)
                    .max(0)
                    .min((wave.samples.len() / 2) as i64) as usize;
                if consumed > 0 {
                    wave.samples.drain(..consumed * 2);
                    wave.start_frame += consumed as i64;
                }
            }
            state
                .active_waves
                .retain(|wave| !wave.complete || wave_end(wave) > *cursor);
        }
    }
    Ok(())
}

#[cfg(test)]
mod media_snapshot_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingMedia {
        calls: AtomicUsize,
        bytes: Vec<u8>,
    }

    impl MediaSource for CountingMedia {
        fn load(&self, _relative_path: &str) -> Result<Vec<u8>, String> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(self.bytes.clone())
        }
    }

    fn project_with_repeated_wave_reference() -> UProject {
        let mut project = UProject::create();
        let mut wave = domain::UWavePart {
            relative_path: Some("asset:shared".into()),
            ..Default::default()
        };
        wave.name = "first".into();
        project.parts.push(UPart::Wave(wave.clone()));
        wave.name = "second".into();
        project.parts.push(UPart::Wave(wave));
        project
    }

    #[test]
    fn snapshot_reads_repeated_asset_once_and_enforces_compressed_byte_limit() {
        let project = project_with_repeated_wave_reference();
        let source = CountingMedia {
            calls: AtomicUsize::new(0),
            bytes: vec![1, 2, 3, 4],
        };
        let snapshot = PinnedMediaSnapshot::capture(&project, &source, 4).unwrap();
        assert_eq!(source.calls.load(Ordering::Relaxed), 1);
        assert_eq!(snapshot.load("asset:shared").unwrap(), vec![1, 2, 3, 4]);

        let error = PinnedMediaSnapshot::capture(&project, &source, 3).unwrap_err();
        assert!(error.contains("exceeds limit"), "{error}");
    }
}
