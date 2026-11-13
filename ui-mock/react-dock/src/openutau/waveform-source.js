// HTTP render-preview adapter for the OpenUtau clone. Direct WASM preview is
// handled by `engine.renderTrack()` in clone-state so it renders the live Rust
// session. HTTP preview uses the synth-server §6.13 job and never substitutes a
// fixture-pool render that would omit project media or track effects.
//
// The peaks are presentation-only output from the engine and never project
// state. Stereo channels are retained for playback; the display takes the
// outer envelope across both channels.

import { RenderPool } from '../../../../web/pool.js';
import { HttpBank, OpfsBank } from '../../../../web/samples.js';
import { FIXTURE_OTO_DIR, FIXTURE_OTO_PATH, fixtureBankBaseUrl } from '../../../../web/fixture-bank.js';
import { choosePreviewBank } from './preview-bank.js';
import { buildPeaks } from './pcm-analysis.js';
export { buildPeaks } from './pcm-analysis.js';

const SAMPLE_RATE = 44100;
const POLL_MS = 250;
const SERVER_RENDER_TIMEOUT_MS = 120_000;
/** Both bank layouts keep their samples in `voice/`, like OpenUtau's banks. */
const OPFS_OTO_PATH = 'voice/oto.ini';
const OPFS_OTO_DIR = 'voice';

/** One pool per page: workers and the wasm instance are expensive to boot. */
let pool = null;
/** Which bank the current pool was built with; a different one needs a new pool. */
let poolBankKey = null;
/** The resolved pool once a browser render has succeeded, else null. */
let livePool = null;
/** The USTX the pool is currently planned for, so an unchanged project is free. */
let plannedUstx = null;

/** The `?voicebank=` value, read once; it is a dev/QA escape hatch. */
function voicebankOverride() {
  try {
    return new URLSearchParams(window.location.search).get('voicebank');
  } catch {
    return null;
  }
}

/**
 * Where an installed bank keeps its `oto.ini`.
 *
 * `voice/` is the convention, not the rule: OpenUtau resolves `oto.ini` from
 * wherever the bank puts it, and real Japanese banks ship their samples in a
 * named folder instead. So `voice/` is tried first (the common case, and no
 * directory listing needed) and the bank is walked only if that misses.
 */
async function findBankOto(bank) {
  const conventional = { otoPath: OPFS_OTO_PATH, otoDir: OPFS_OTO_DIR };
  // Probed, not assumed: returning an object cannot throw, so the bank has to
  // actually be read for this to mean anything.
  try {
    await bank.raw(conventional.otoPath);
    return conventional;
  } catch { /* this bank does not use voice/ — walk it */ }
  for (const candidate of await bank.listOtoCandidates()) {
    try {
      await bank.raw(candidate.otoPath);
      return candidate;
    } catch { /* keep looking */ }
  }
  return conventional;
}

/** Build the bank object and the human label for a chosen description. */
async function openPreviewBank(choice) {
  if (!choice) throw new Error('no voicebank is installed and the dev fixture is disabled');
  if (choice.kind === 'opfs') {
    const bank = new OpfsBank({ root: choice.root });
    const oto = await findBankOto(bank);
    return { bank, ...oto, key: `opfs:${choice.root}`, name: choice.root, kind: 'opfs' };
  }
  const bank = new HttpBank({ baseUrl: fixtureBankBaseUrl() });
  return { bank, otoPath: FIXTURE_OTO_PATH, otoDir: FIXTURE_OTO_DIR, key: 'fixture', name: 'dev fixture', kind: 'fixture' };
}

/**
 * The OPFS bank roots on this origin, or [] where OPFS is unavailable.
 *
 * Memoized for the page: the resolver runs on every render, and scanning the
 * OPFS root each time would be pure overhead for a set that only changes when
 * the user imports a bank (which needs a reload to be picked up anyway).
 */
let installedRootsPromise = null;
function installedBankRoots() {
  if (!installedRootsPromise) {
    installedRootsPromise = (async () => {
      try {
        if (!(await OpfsBank.available())) return [];
        return await OpfsBank.installedRoots();
      } catch {
        return [];
      }
    })();
  }
  return installedRootsPromise;
}

/**
 * Forget the cached voicebank list so the next render re-resolves its source.
 *
 * Called after a bank is installed or removed. The pool itself is not torn down
 * here: the next `browserPool` sees a different bank identity and rebuilds it,
 * whereas this list may still resolve to the same bank.
 */
export function resetPreviewBankScan() {
  installedRootsPromise = null;
}

/**
 * The pool itself, for a caller that wants to stream from it (`mixFrom`).
 * Null until a browser render has succeeded — the fallback render does not
 * populate a pool, so nothing can be streamed out of that one.
 */
export function getRenderPool() {
  return livePool;
}

/**
 * The in-browser pool, or a rejected promise (which resets it so a later
 * attempt — say, after the dev server comes back — can retry).
 */
async function browserPool({ ustx, track, trackMix, singerId = null }) {
  const choice = choosePreviewBank({ installed: await installedBankRoots(), singerId, override: voicebankOverride() });
  const opened = await openPreviewBank(choice);
  // A different bank is a different alias table, so the pool has to be rebuilt
  // rather than re-fed; the old workers and their samples go away with it.
  if (pool && poolBankKey !== opened.key) {
    const stale = pool;
    pool = null;
    livePool = null;
    plannedUstx = null;
    Promise.resolve(stale).then(created => created?.terminate?.()).catch(() => {});
  }
  if (!pool) {
    poolBankKey = opened.key;
    pool = RenderPool.create({
      workerCount: 2,
      bank: opened.bank,
      otoPath: opened.otoPath,
      otoDir: opened.otoDir,
      projectYaml: ustx,
      track,
      trackMix,
    }).then(created => {
      livePool = created;
      return created;
    }).catch(error => {
      pool = null;
      poolBankKey = null;
      livePool = null;
      plannedUstx = null;
      throw error;
    });
  }
  try {
    return { created: await pool, bank: { name: opened.name, kind: opened.kind, reason: choice.reason } };
  } catch (error) {
    throw new Error(`browser render pool unavailable (${error?.message || error})`);
  }
}

/**
 * Render `ustx` with the in-browser pool.
 *
 * `setProject` only marks changed chunks dirty, and `render()` reuses every
 * unchanged hash from the cache — so an edit re-renders one chunk and the
 * strip updates from the same pool the main editor plays through.
 */
async function renderWithPool({ ustx, track, trackMix, singerId }) {
  const { created: p, bank } = await browserPool({ ustx, track, trackMix, singerId });
  p.setTrackMix(trackMix);
  if (ustx !== plannedUstx) {
    await p.setProject(ustx);
    plannedUstx = ustx;
  }
  const { samples, cancelled, stats } = await p.render({});
  if (cancelled || !samples) throw new Error('render was cancelled');
  if (!samples.length) throw new Error('the render produced no audio (every phrase was skipped)');
  // `lastRendered`/`lastReused` are the cache split in numbers: a fader move
  // re-mixes the cached chunks (lastRendered 0) and never re-renders them.
  return { samples, sampleRate: SAMPLE_RATE, source: 'render-pool', bank, stats };
}

async function json(response) {
  const text = await response.text();
  let value = null;
  try { value = text ? JSON.parse(text) : null; } catch { value = text; }
  if (!response.ok || value?.error) {
    const error = new Error(value?.error?.message || value?.message || response.statusText || 'request failed');
    error.status = response.status;
    throw error;
  }
  return value;
}

/** Decode 16-bit PCM WAV. Keep a mono envelope projection and the real channels. */
function decodeWav16(bytes) {
  const view = new DataView(bytes);
  if (view.getUint32(0, false) !== 0x52494646) throw new Error('server audio is not a RIFF wav');
  let offset = 12;
  let sampleRate = SAMPLE_RATE;
  let channels = 1;
  let bitsPerSample = 16;
  let dataStart = -1;
  let dataLength = 0;
  while (offset + 8 <= view.byteLength) {
    const id = view.getUint32(offset, false);
    const size = view.getUint32(offset + 4, true);
    const body = offset + 8;
    if (id === 0x666d7420) {
      channels = view.getUint16(body + 2, true);
      sampleRate = view.getUint32(body + 4, true);
      bitsPerSample = view.getUint16(body + 14, true);
    }
    else if (id === 0x64617461) { dataStart = body; dataLength = Math.min(size, view.byteLength - body); } // 'data'
    offset = body + size + (size % 2);
  }
  if (dataStart < 0) throw new Error('server audio has no data chunk');
  if (channels < 1) throw new Error('server audio has an invalid channel count');
  if (bitsPerSample !== 16) throw new Error(`server audio is ${bitsPerSample}-bit; only 16-bit PCM is handled`);
  const frames = Math.floor(dataLength / (2 * channels));
  const samples = new Float32Array(frames);
  const channelData = Array.from({ length: channels }, () => new Float32Array(frames));
  for (let i = 0; i < frames; i++) {
    let sum = 0;
    for (let channel = 0; channel < channels; channel++) {
      const sample = view.getInt16(dataStart + (i * channels + channel) * 2, true) / 32768;
      channelData[channel][i] = sample;
      sum += sample;
    }
    samples[i] = sum / channels;
  }
  return { samples, channels: channelData, sampleRate };
}

/** The §6.13 render job, for a session that lives on the synth-server. */
async function renderWithServer({ baseUrl, sessionId, revision, track = 0, singerId = null, signal }) {
  const base = (baseUrl || '').replace(/\/$/, '');
  if (!sessionId) throw new Error('the synth-server fallback needs a live HTTP session');
  const listed = await json(await fetch(`${base}/voicebanks`));
  // A bank with no wavs has nothing to render; picking one would only produce
  // an empty take, so say what is missing instead.
  const entries = (listed?.voicebanks || []).filter(entry => (entry.wav_count || 0) > 0);
  if (!entries.length) throw new Error('the synth-server has no voicebank with samples (start it with --voicebanks at a real bank)');
  const bank = (singerId && entries.find(e => e.name.toLowerCase() === singerId.toLowerCase() || e.dir.toLowerCase() === singerId.toLowerCase()))
    || entries.find(e => /fixture|dev/i.test(e.name || e.dir))
    || entries[0];

  const started = await json(await fetch(`${base}/api/v2/sessions/${encodeURIComponent(sessionId)}/render`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ expected_revision: revision, voicebank: bank.name, track }),
  }));
  const renderId = started?.render_id;
  if (!renderId) throw new Error('the render job did not return a render_id');

  const deadline = Date.now() + SERVER_RENDER_TIMEOUT_MS;
  for (;;) {
    if (signal?.cancelled) {
      await fetch(`${base}/api/v2/render/${encodeURIComponent(renderId)}/cancel`, { method: 'POST' }).catch(() => {});
      throw new Error('render was cancelled');
    }
    if (Date.now() > deadline) throw new Error(`render ${renderId} timed out`);
    const status = await json(await fetch(`${base}/api/v2/render/${encodeURIComponent(renderId)}`));
    if (status?.status === 'done') break;
    if (status?.status === 'failed' || status?.status === 'cancelled') {
      throw new Error(status?.error?.message || `render ended ${status?.status}`);
    }
    await new Promise(resolve => setTimeout(resolve, POLL_MS));
  }
  const audio = await fetch(`${base}/api/v2/render/${encodeURIComponent(renderId)}/audio`);
  if (!audio.ok) throw new Error(`render audio unavailable (HTTP ${audio.status})`);
  const { samples, sampleRate } = decodeWav16(await audio.arrayBuffer());
  if (!samples.length) throw new Error('the render produced no audio');
  return { samples, sampleRate, source: 'synth-server', bank: { name: bank.name, kind: 'installed' } };
}

/**
 * Render the project to PCM for the waveform strip.
 *
 * @param {object} opts
 * @param {'http'} opts.kind             HTTP session backend; direct WASM callers use engine.renderTrack
 * @param {string} opts.baseUrl         synth-server origin (http backend)
 * @param {string|null} opts.sessionId  server session id, for the fallback
 * @param {number} opts.revision        the revision `ustx` came from
 * @param {string} opts.ustx            canonical USTX from the session
 * @param {number} [opts.track]
 * @param {{volumeDb:number, pan:number}} [opts.trackMix]
 * @param {string|null} [opts.singerId] the track's singer id, to pick its bank
 * @param {{cancelled:boolean}} [opts.signal]
 * @returns {Promise<{samples: Float32Array, sampleRate: number, source: string, bank?: object}>}
 */
export async function renderProjectAudio({ kind, baseUrl, sessionId, revision, ustx, track = 0, trackMix = { volumeDb: 0, pan: 0 }, singerId = null, signal }) {
  // On the HTTP backend the SERVER renders first, not the pool. The in-browser
  // pool only has the preview bank, which maps a fraction of a real project's
  // phonemes, so a "successful" pool render can be a few seconds of sparse
  // audio that silently REPLACES the server take — the transport then plays
  // that stub and stops, which reads as a hang a few seconds into the song.
  // The server is also the only source that owns the project's real voicebank,
  // so preferring it is the engine-owned-state rule, not just a quality call.
  if (kind === 'http') {
    // Do not replace the authoritative Rust render with the fixture pool when
    // the server errors: the pool lacks project media and the track Mix FX bus.
    return renderWithServer({ baseUrl, sessionId, revision, track, singerId, signal });
  }
  throw new Error('Direct WASM preview must use the Rust session renderTrack command');
}

/** The `[min, max]` of the samples covered by `[from, to)` seconds. */
export function peaksInRange(peaks, fromSeconds, toSeconds) {
  if (!peaks) return null;
  const first = Math.max(0, Math.floor(fromSeconds * peaks.sampleRate / peaks.bucket));
  const last = Math.min(peaks.max.length - 1, Math.ceil(toSeconds * peaks.sampleRate / peaks.bucket));
  if (last < 0 || first >= peaks.max.length) return null;
  let lo = peaks.min[first];
  let hi = peaks.max[first];
  for (let b = first + 1; b <= last; b++) {
    if (peaks.min[b] < lo) lo = peaks.min[b];
    if (peaks.max[b] > hi) hi = peaks.max[b];
  }
  return [lo, hi];
}
