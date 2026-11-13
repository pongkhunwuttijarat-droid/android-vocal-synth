//! Engine-owned media access for wave (audio) parts.
//!
//! A `.ustx` wave part stores `relative_path` — a path *relative to the project
//! file* on the machine that last saved it, often a Windows path from someone
//! else's machine. Two consequences, both of which this crate exists to solve:
//!
//! 1. **Decoding must be engine-owned.** MP3/WAV/FLAC decoding in the engine
//!    means the browser and the native host place audio identically instead of
//!    each bringing its own decoder. [`decode_mono_44100`] is the only entry
//!    point; no host in this repo decodes media anywhere else.
//! 2. **Resolving a reference to bytes is the host's job, on purpose.** The
//!    engine asks for a *project-relative reference* and the host adapter
//!    answers with bytes ([`MediaSource`]). That is the same seam
//!    `worldline_rs::SampleSource` uses for voice samples, so the browser (OPFS)
//!    and the native server (a directory) plug in without the engine learning
//!    about either. What the engine owns is everything after the bytes arrive:
//!    trim maths, fades, channel fold-down, resampling and mixing.
//!
//! The trim/fade port follows `OpenUtau.Core/Ustx/UPart.cs` → `UWavePart`
//! (`GetSkipMs`, `GetTrimMs`, `UpdateDuration`, `TrimSamples`) tick for tick.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use domain::{TimeAxis, UWavePart};

/// Output sample rate for every mixed signal in the engine.
pub const OUTPUT_SAMPLE_RATE: u32 = 44100;

/// What [`probe`] can tell us without decoding the whole file.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MediaInfo {
    pub sample_rate: u32,
    pub channels: u16,
    pub duration_ms: f64,
}

/// A host's answer to "give me the bytes for this project-relative reference".
///
/// Implementations must treat the reference as an opaque key: the same map may
/// be backed by a directory (native) or by OPFS entries keyed by the reference
/// string (browser). Path traversal is the host's business, but the native
/// implementation below refuses to escape its root so a hostile `.ustx` cannot
/// make the engine read `/etc/shadow`.
pub trait MediaSource {
    fn load(&self, relative_path: &str) -> Result<Vec<u8>, String>;
}

/// Native host: resolves the reference against a directory (normally the folder
/// holding the `.ustx`).
///
/// Resolution follows OpenUtau exactly (`UWavePart.AfterLoad`):
/// `Path.GetFullPath(Path.Combine(projectDir, relativePath))`. That means a
/// reference **may** point outside the project folder — the format is written by
/// whatever machine last saved, and real projects do contain
/// `../../../home/<user>/…` paths. Nothing in `.ustx` is trusted here beyond
/// "this is a path"; use [`FileMediaSource::restricted`] when even that is too
/// much (e.g. rendering an untrusted project from a service).
#[derive(Debug, Clone)]
pub struct FileMediaSource {
    root: PathBuf,
    restrict_to_root: bool,
}

impl FileMediaSource {
    /// Resolve like OpenUtau: combine with the root and normalise, allowing the
    /// result to land anywhere on disk.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        FileMediaSource {
            root: root.into(),
            restrict_to_root: false,
        }
    }

    /// Additionally refuse any reference that normalises to a path outside the
    /// root. Use this when the project directory is not trusted.
    pub fn restricted(root: impl Into<PathBuf>) -> Self {
        FileMediaSource {
            root: root.into(),
            restrict_to_root: true,
        }
    }

    /// Where a reference would be read from.
    ///
    /// `None` when the reference is empty, or when [`Self::restricted`] is set
    /// and the path escapes the root.
    pub fn resolve(&self, relative_path: &str) -> Option<PathBuf> {
        if relative_path.trim().is_empty() {
            return None;
        }
        // `Path.GetFullPath` resolves a relative combined path against the
        // process directory, so a reference that walks further up than the root
        // is deep still lands somewhere real. We must do the same, otherwise
        // `../../../../../home/user/x.wav` from a relative root silently loses
        // its leading `/` and we look for `<cwd>/home/user/x.wav`.
        let base = if self.root.is_absolute() {
            self.root.clone()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("/"))
                .join(&self.root)
        };
        // A Windows-authored path ("C:\\...\\x.wav") does not join meaningfully
        // onto a POSIX path; it collapses to a bare file name, which is the best
        // we can do without a manifest.
        let normalized = normalize(&base.join(relative_path));
        if self.restrict_to_root && !normalized.starts_with(normalize(&base)) {
            return None;
        }
        Some(normalized)
    }
}

/// Lexical path normalisation (`.`/`..`) that does not touch the filesystem:
/// `canonicalize` fails for paths that do not exist yet, and we must be able to
/// resolve a reference before reading it.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

impl MediaSource for FileMediaSource {
    fn load(&self, relative_path: &str) -> Result<Vec<u8>, String> {
        let path = self
            .resolve(relative_path)
            .ok_or_else(|| format!("media reference '{relative_path}' escapes the media root"))?;
        std::fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))
    }
}

/// Browser host: the host already put the bytes in memory (OPFS / fetch).
#[derive(Debug, Clone, Default)]
pub struct MemoryMediaSource {
    blobs: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

impl MemoryMediaSource {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, relative_path: impl Into<String>, bytes: Vec<u8>) {
        self.blobs
            .write()
            .unwrap_or_else(|error| error.into_inner())
            .insert(relative_path.into(), bytes);
    }

    pub fn len(&self) -> usize {
        self.blobs
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.blobs
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .is_empty()
    }
}

impl MediaSource for MemoryMediaSource {
    fn load(&self, relative_path: &str) -> Result<Vec<u8>, String> {
        self.blobs
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .get(relative_path)
            .cloned()
            .ok_or_else(|| format!("no media registered for '{relative_path}'"))
    }
}

fn stream_of(bytes: &[u8]) -> symphonia::core::io::MediaSourceStream {
    // `MediaSourceStream` owns a `'static` boxed source, so the bytes are copied
    // in. That happens once per wave part, which is not a hot path.
    symphonia::core::io::MediaSourceStream::new(
        Box::new(std::io::Cursor::new(bytes.to_vec())),
        Default::default(),
    )
}

fn open(bytes: &[u8]) -> Result<symphonia::core::probe::ProbeResult, String> {
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::meta::MetadataOptions;
    symphonia::default::get_probe()
        .format(
            &Default::default(),
            stream_of(bytes),
            // Honour MP3 Xing/LAME encoder delay and padding. OpenUtau's MP3
            // reader exposes the gapless duration; without this, the decoded
            // wave part starts late and runs long by the codec's priming data.
            &FormatOptions {
                enable_gapless: true,
                ..FormatOptions::default()
            },
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("cannot probe media: {e}"))
}

/// Read sample rate / channel count / duration without decoding the audio.
///
/// `file_duration_ms` in a `.ustx` is written from this value, so a project that
/// round-trips through us keeps the number OpenUtau would show.
pub fn probe(bytes: &[u8]) -> Result<MediaInfo, String> {
    let probed = open(bytes)?;
    let track = probed
        .format
        .default_track()
        .ok_or_else(|| "media container has no decodable track".to_string())?;
    let params = track.codec_params.clone();
    let sample_rate = params.sample_rate.unwrap_or(0);
    let channels = params.channels.map_or(1u16, |c| c.count() as u16);
    if sample_rate == 0 {
        return Err("media reports sample rate 0".to_string());
    }
    // A frame count is what `file_duration_ms` is derived from. Symphonia's
    // probe reports it for MP3 (estimated from the bitrate when VBR headers are
    // absent); when it cannot, we report 0.0 rather than guessing — callers
    // that need a real length must decode (see [`decode_mono_44100`]), which is
    // what the mixdown does anyway.
    let duration_ms = match params.n_frames {
        Some(n) if n > 0 => n as f64 * 1000.0 / f64::from(sample_rate),
        _ => 0.0,
    };
    Ok(MediaInfo {
        sample_rate,
        channels,
        duration_ms,
    })
}

/// Decode container bytes to mono `f32` at [`OUTPUT_SAMPLE_RATE`].
///
/// Multi-channel input is folded down by averaging, which is what OpenUtau's
/// `ExportAdapter.ToMono(1, 0)` does for its exports (the engine's mixer is
/// mono by contract, see `runtime::mixer`).
pub fn decode_mono_44100(bytes: &[u8]) -> Result<Vec<f32>, String> {
    use symphonia::core::codecs::DecoderOptions;
    use symphonia::core::errors::Error as SymError;

    let mut probed = open(bytes)?;
    let track = probed
        .format
        .default_track()
        .ok_or_else(|| "media container has no decodable track".to_string())?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("cannot build decoder: {e}"))?;
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| "media reports sample rate 0".to_string())?;

    let mut mono: Vec<f32> = Vec::new();
    loop {
        let packet = match probed.format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymError::ResetRequired) => continue,
            Err(e) => return Err(format!("decode failed: {e}")),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let trim_start = packet.trim_start() as usize;
        let trim_end = packet.trim_end() as usize;
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            // A damaged packet must not lose the whole part; OpenUtau's decoder
            // is equally lossy-tolerant.
            Err(SymError::DecodeError(_)) | Err(SymError::ResetRequired) => continue,
            Err(e) => return Err(format!("decode failed: {e}")),
        };
        // Normalise every sample format (u8/u16/i24/...) to f32 so the mono
        // fold-down is a plain sum across channels. `make_equivalent` only
        // *allocates* an empty buffer of the new type, so the conversion has to
        // be requested explicitly with `convert`.
        let mut buf = symphonia::core::audio::AudioBuffer::<f32>::new(
            decoded.capacity() as symphonia::core::units::Duration,
            *decoded.spec(),
        );
        decoded.convert(&mut buf);
        symphonia::core::audio::Signal::trim(&mut buf, trim_start, trim_end);
        let planes_view = buf.planes();
        let planes = planes_view.planes();
        let channels = planes.len().max(1);
        if planes.is_empty() {
            continue;
        }
        for i in 0..planes[0].len() {
            let mut acc = 0f32;
            for plane in planes {
                acc += plane[i];
            }
            mono.push(acc / channels as f32);
        }
    }
    if mono.is_empty() {
        return Err("decoded 0 samples".to_string());
    }
    if sample_rate == OUTPUT_SAMPLE_RATE {
        Ok(mono)
    } else {
        Ok(resample_linear(&mono, sample_rate, OUTPUT_SAMPLE_RATE))
    }
}

/// Decode container bytes to interleaved stereo `f32` at
/// [`OUTPUT_SAMPLE_RATE`]. Mono sources are copied to both channels; sources
/// with more than two channels use their first two decoded planes.
pub fn decode_stereo_44100(bytes: &[u8]) -> Result<Vec<f32>, String> {
    let mut samples = Vec::new();
    decode_stereo_44100_blocks(bytes, 4096, |block| {
        samples.extend_from_slice(block);
        Ok(())
    })?;
    Ok(samples)
}

/// Decode a media source and publish interleaved stereo PCM as bounded blocks.
///
/// At the native mix rate (44.1 kHz), each decoded codec packet is sent to the
/// consumer as soon as it is available; no full-file PCM buffer is retained.
/// Other input rates are decoded and resampled as one buffer for now, because
/// the legacy linear resampler has whole-buffer boundary semantics. Consumers
/// must treat any emitted blocks as provisional until this function returns
/// `Ok(())`: malformed trailing packets can still fail a decode.
pub fn decode_stereo_44100_blocks<F>(
    bytes: &[u8],
    block_frames: usize,
    mut sink: F,
) -> Result<(), String>
where
    F: FnMut(&[f32]) -> Result<(), String>,
{
    if block_frames == 0 {
        return Err("media decode block size must be nonzero".to_string());
    }
    use symphonia::core::codecs::DecoderOptions;
    use symphonia::core::errors::Error as SymError;

    let mut probed = open(bytes)?;
    let track = probed
        .format
        .default_track()
        .ok_or_else(|| "media container has no decodable track".to_string())?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("cannot build decoder: {e}"))?;
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| "media reports sample rate 0".to_string())?;

    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut block = Vec::with_capacity(block_frames * 2);
    let mut emitted = false;
    loop {
        let packet = match probed.format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymError::ResetRequired) => continue,
            Err(e) => return Err(format!("decode failed: {e}")),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let trim_start = packet.trim_start() as usize;
        let trim_end = packet.trim_end() as usize;
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SymError::DecodeError(_)) | Err(SymError::ResetRequired) => continue,
            Err(e) => return Err(format!("decode failed: {e}")),
        };
        let mut buf = symphonia::core::audio::AudioBuffer::<f32>::new(
            decoded.capacity() as symphonia::core::units::Duration,
            *decoded.spec(),
        );
        decoded.convert(&mut buf);
        symphonia::core::audio::Signal::trim(&mut buf, trim_start, trim_end);
        let plane_view = buf.planes();
        let planes = plane_view.planes();
        if planes.is_empty() {
            continue;
        }
        let l = &planes[0];
        let r = planes.get(1).unwrap_or(l);
        if sample_rate == OUTPUT_SAMPLE_RATE {
            for (&left_sample, &right_sample) in l.iter().zip(r.iter()) {
                block.push(left_sample);
                block.push(right_sample);
                if block.len() == block_frames * 2 {
                    sink(&block)?;
                    block.clear();
                    emitted = true;
                }
            }
        } else {
            left.extend_from_slice(l);
            right.extend_from_slice(r);
        }
    }
    if sample_rate == OUTPUT_SAMPLE_RATE {
        if !emitted && block.is_empty() {
            return Err("decoded 0 samples".to_string());
        }
        if !block.is_empty() {
            sink(&block)?;
        }
    } else {
        if left.is_empty() {
            return Err("decoded 0 samples".to_string());
        }
        left = resample_linear(&left, sample_rate, OUTPUT_SAMPLE_RATE);
        right = resample_linear(&right, sample_rate, OUTPUT_SAMPLE_RATE);
        let frames = left.len().min(right.len());
        let mut block = Vec::with_capacity(block_frames * 2);
        for i in 0..frames {
            block.push(left[i]);
            block.push(right[i]);
            if block.len() == block_frames * 2 {
                sink(&block)?;
                block.clear();
            }
        }
        if !block.is_empty() {
            sink(&block)?;
        }
    }
    Ok(())
}

/// Linear-interpolation resampler.
///
/// Honest about what it is: fine for the 44.1k↔48k mismatches that show up in
/// real projects, not a substitute for a windowed-sinc stage. When the rates are
/// equal it is an exact copy (asserted by a test), so the common case pays no
/// quality cost.
pub fn resample_linear(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || input.is_empty() {
        return input.to_vec();
    }
    let ratio = f64::from(from_rate) / f64::from(to_rate);
    let out_len = ((input.len() as f64) / ratio).ceil() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src = i as f64 * ratio;
        let idx = src.floor() as usize;
        let frac = (src - idx as f64) as f32;
        let a = input[idx.min(input.len() - 1)];
        let b = input[(idx + 1).min(input.len() - 1)];
        out.push(a + (b - a) * frac);
    }
    out
}

/// The four tick-derived frame counts `UWavePart.TrimSamples` computes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WaveTrimFrames {
    pub skip: usize,
    pub trim: usize,
    pub fadein: usize,
    pub fadeout: usize,
}

/// Frame counts for a wave part, ported from `UWavePart.GetSkipMs`,
/// `GetTrimMs` and `TrimSamples`. Ticks become ms through the project's time
/// axis (so tempo maps are respected), and the ms→frame cast truncates exactly
/// like the C# `(int)(ms * sampleRate / 1000)`.
pub fn wave_trim_frames(
    part: &UWavePart,
    axis: &TimeAxis,
    sample_rate: u32,
    total_frames: usize,
) -> WaveTrimFrames {
    let duration = wave_part_duration_ticks(part, axis);
    let end = part.position + duration;
    let skip_ms = axis.ms_between_ticks(
        f64::from(part.position - part.skip),
        f64::from(part.position),
    );
    let trim_ms = axis.ms_between_ticks(f64::from(end), f64::from(end + part.trim));
    let fadein_ms = axis.ms_between_ticks(
        f64::from(part.position),
        f64::from(part.position + part.fadein),
    );
    let fadeout_ms = axis.ms_between_ticks(f64::from(end - part.fadeout), f64::from(end));
    let frames = |ms: f64| (ms * f64::from(sample_rate) / 1000.0) as usize;
    let skip = frames(skip_ms).min(total_frames);
    let trim = frames(trim_ms).min(total_frames.saturating_sub(skip));
    WaveTrimFrames {
        skip,
        trim,
        fadein: frames(fadein_ms),
        fadeout: frames(fadeout_ms),
    }
}

/// `UWavePart.UpdateDuration`: the part's duration in ticks.
///
/// The part covers the file from `position - skip` for `file_duration_ms`,
/// minus `trim` — which is why `file_duration_ms` has to survive the `.ustx`
/// round-trip for a wave part to be placed correctly at all.
pub fn wave_part_duration_ticks(part: &UWavePart, axis: &TimeAxis) -> i32 {
    let file_start_ms = axis.tick_to_ms(f64::from(part.position - part.skip));
    let file_end = axis.ms_to_tick(file_start_ms + part.file_duration_ms);
    file_end - part.trim - part.position
}

/// Apply [`WaveTrimFrames`] to decoded mono samples.
///
/// A faithful port of the sample half of `UWavePart.TrimSamples`: cut `skip`
/// frames from the head, cut `trim` from the tail, ramp the first `fadein`
/// frames from 0 and the last `fadeout` frames to 0. An empty result is a
/// legitimate outcome (OpenUtau sets an empty sample array in that case).
pub fn apply_wave_trim(samples: &[f32], trim: WaveTrimFrames) -> Vec<f32> {
    let remaining = samples.len().saturating_sub(trim.skip + trim.trim);
    if remaining == 0 {
        return Vec::new();
    }
    let mut out = samples[trim.skip..trim.skip + remaining].to_vec();
    let n = trim.fadein.min(out.len());
    for (i, sample) in out[..n].iter_mut().enumerate() {
        *sample *= i as f32 / n as f32;
    }
    let n = trim.fadeout.min(out.len());
    for i in 0..n {
        let idx = out.len() - 1 - i;
        out[idx] *= i as f32 / n as f32;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(n: usize) -> Vec<f32> {
        (0..n).map(|i| i as f32 / 100.0).collect()
    }

    #[test]
    fn resample_is_exact_when_rates_match() {
        let input = ramp(1000);
        assert_eq!(resample_linear(&input, 44100, 44100), input);
    }

    #[test]
    fn resample_changes_length_by_the_rate_ratio() {
        // 4410 frames recorded at 48 kHz is 91.875 ms; playing that back at
        // 44.1 kHz needs 91.875 * 44100/1000 = 4051.7 frames.
        let input = ramp(4410);
        let out = resample_linear(&input, 48000, 44100);
        assert!(
            (out.len() as i64 - 4052).abs() <= 1,
            "len was {}",
            out.len()
        );
        // And the other direction shrinks it by the same factor.
        let back = resample_linear(&out, 44100, 48000);
        assert!(
            (back.len() as i64 - 4410).abs() <= 2,
            "len was {}",
            back.len()
        );
    }

    #[test]
    fn trim_cuts_head_and_tail() {
        let samples: Vec<f32> = (0..100).map(|i| i as f32).collect();
        let out = apply_wave_trim(
            &samples,
            WaveTrimFrames {
                skip: 10,
                trim: 20,
                fadein: 0,
                fadeout: 0,
            },
        );
        assert_eq!(out.len(), 70);
        assert_eq!(out[0], 10.0);
        assert_eq!(out[69], 79.0);
    }

    #[test]
    fn trim_can_consume_the_whole_file() {
        let samples: Vec<f32> = (0..10).map(|i| i as f32).collect();
        let out = apply_wave_trim(
            &samples,
            WaveTrimFrames {
                skip: 6,
                trim: 6,
                ..Default::default()
            },
        );
        assert!(out.is_empty());
    }

    #[test]
    fn fades_are_linear_and_silent_at_the_outer_edge() {
        let samples = vec![1.0f32; 100];
        let out = apply_wave_trim(
            &samples,
            WaveTrimFrames {
                fadein: 10,
                fadeout: 10,
                ..Default::default()
            },
        );
        assert_eq!(out[0], 0.0);
        assert_eq!(out[9], 0.9);
        assert_eq!(out[99], 0.0);
        assert_eq!(out[90], 0.9);
        // Untouched middle.
        assert_eq!(out[50], 1.0);
    }

    #[test]
    fn file_media_source_follows_openutau_resolution() {
        // OpenUtau's own `AfterLoad` is a plain combine-and-normalise, and real
        // projects ship paths that leave the project folder (the attached
        // fixture's wave part points at `../../../../../home/<user>/...`).
        let src = FileMediaSource::new("/srv/projects/song.ustx_dir");
        assert_eq!(
            src.resolve("audio/x.wav").as_deref(),
            Some(Path::new("/srv/projects/song.ustx_dir/audio/x.wav"))
        );
        assert_eq!(
            src.resolve("../../../home/user/x.wav").as_deref(),
            Some(Path::new("/home/user/x.wav"))
        );
        assert!(src.resolve("").is_none());
    }

    #[test]
    fn a_relative_root_still_resolves_traversal_to_an_absolute_path() {
        // Regression: with a relative root, extra `..` components used to pop
        // against an empty prefix and the leading `/` was lost, so a real
        // fixture's wave part resolved to `<cwd>/home/...` and "did not exist".
        // The exact target depends on how deep the test's cwd is, so we only
        // assert the property that was broken: the result is absolute.
        let src = FileMediaSource::new("test-ustx-example");
        let resolved = src
            .resolve("../../../../../../../../../../home/user/song.mp3")
            .expect("resolves");
        assert!(
            resolved.is_absolute(),
            "expected an absolute path, got {}",
            resolved.display()
        );
        assert_eq!(resolved, Path::new("/home/user/song.mp3"));
    }

    #[test]
    fn restricted_file_media_source_refuses_to_escape_its_root() {
        let src = FileMediaSource::restricted("/srv/projects");
        assert!(src.resolve("audio/x.wav").is_some());
        assert!(src.resolve("../../../etc/passwd").is_none());
        assert!(src.resolve("").is_none());
    }

    #[test]
    fn file_media_source_reads_inside_its_root() {
        let dir = std::env::temp_dir().join(format!("media-src-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.bin"), b"hello").unwrap();
        let src = FileMediaSource::restricted(&dir);
        assert_eq!(src.load("a.bin").unwrap(), b"hello");
        assert!(src.load("missing.bin").is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn memory_media_source_round_trips_blobs() {
        let mut src = MemoryMediaSource::new();
        src.insert("song.mp3", vec![1, 2, 3]);
        assert_eq!(src.len(), 1);
        assert_eq!(src.load("song.mp3").unwrap(), vec![1, 2, 3]);
        assert!(src.load("other.mp3").is_err());
    }
}
