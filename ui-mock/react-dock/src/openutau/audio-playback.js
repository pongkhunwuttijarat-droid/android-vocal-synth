// Real audio playback for the clone page.
//
// The point of this module is that the playhead is NOT a timer running next to
// the audio: while audio plays, the playhead is read back from the
// `AudioContext` clock, so the sample you hear and the tick under the cursor are
// the same position.
//
// `getTake()` supplies the finished Rust project mixdown, cached by project
// revision and voicebank epoch. That stereo take includes every renderable
// singer track and every registered wave part; it is the transport source even
// when the selected-track waveform preview fails. `getPool()` remains a
// fallback for the legacy chunk-preview path. A take starts at project time 0;
// a seek beyond its end parks at the end and the transport stops.

import { TICKS_PER_BEAT } from './mock-data.js';
import { SessionStreamSocket } from './session-stream.js';

const SAMPLE_RATE = 44100;
/** Bounded lookahead hides measured cold phrase-synthesis stalls. */
const STREAM_BUFFER_TARGET_SECONDS = 16;
/** A hair of lead-in so the buffer is scheduled before the clock asks for it. */
const START_LEAD_S = 0.02;

function playbackCancelledError() {
  return Object.assign(new Error('Audio stream stopped'), { code: 'PLAYBACK_CANCELLED' });
}

function streamFailureStage(error) {
  const message = String(error?.message || '').toLowerCase();
  if (message.includes('has not enabled progressive audio')) return 'capability_unavailable';
  if (message.includes('websocket')) return 'websocket_unavailable';
  if (error?.code === 'PLAYBACK_CANCELLED') return 'cancelled';
  return error?.code ? 'stream_rejected' : 'stream_start_failed';
}

/** Musical tick → milliseconds of project time (engine tempo, not a UI default). */
export const tickToMs = (tick, bpm) => tick / TICKS_PER_BEAT * 60000 / bpm;
/** Milliseconds of project time → musical tick. */
export const msToTick = (ms, bpm) => ms * bpm / 60000 * TICKS_PER_BEAT;

/**
 * Where chunk `index` lands in the mix timeline.
 *
 * `mix_from` skips exactly this many samples off the front, so it is both what
 * the range starts at and how far into that range the playhead is.
 */
export function chunkOriginMs(pool, index) {
  const meta = pool.plan[index];
  return (meta?.position_ms ?? 0) - (meta?.leading_ms ?? 0);
}

/** The last chunk that starts at or before `ms` — the range to stream from. */
export function chunkIndexForMs(pool, ms) {
  let index = 0;
  for (let i = 0; i < pool.plan.length; i++) {
    if (chunkOriginMs(pool, i) > ms) break;
    index = i;
  }
  return Math.min(index, Math.max(0, pool.plan.length - 1));
}

/**
 * @param {object} opts
 * @param {() => {samples: Float32Array|number[], channels?: Float32Array[], sampleRate?: number}|null} [opts.getTake]
 *   the last finished engine render, whole-track, starting at project time 0
 * @param {() => import('../../../../web/pool.js').RenderPool|null} opts.getPool
 * @param {() => number} opts.getBpm  the engine's tempo
 */
export function createAudioPlayer({ getTake = () => null, getCompletedTake = null, getTakeAudio = null, getSessionStream = () => null, getPool, getBpm, onEnded = () => {}, onError = () => {}, onStreamUnderrun = () => {} }) {
  let ctx = null;
  let source = null;
  let activeStream = null;
  /** Invalidates asynchronous startup work when the user stops or restarts playback. */
  let playbackIntent = 0;
  /** `{ at, absMs }` while audio is running, else null. */
  let started = null;
  /** Project time the player was last at while silent, in ms. */
  let parkedAt = 0;
  /**
   * Master out: gain → meter → destination.
   *
   * Everything audible goes through this, so there is exactly one place where
   * output can be scaled or measured. It also answers the question a transport
   * without a meter cannot: is the app producing samples (the meter moves and
   * the sink is silent ⇒ the device is wrong) or is the app itself silent (the
   * meter never moves)?
   */
  let out = null;
  let workletReady = null;
  let streamUnderruns = { events: 0, frames: 0, last: null };
  /** Optional dev trace for play-request → audible-output timing. */
  let playbackTrace = null;
  /** Selected output device id, '' = browser default. */
  let sinkId = '';
  /** Master gain, remembered across contexts so a volume set before the first click sticks. */
  let volume = 1;

  /**
   * Create/resume the AudioContext.
   *
   * Browsers only allow that inside a user gesture, so this is called from the
   * transport click handler rather than from the effect that starts playback.
   */
  function unlock() {
    if (!ctx) {
      const Ctor = window.AudioContext || window.webkitAudioContext;
      if (!Ctor) return false;
      try {
        ctx = new Ctor({ sampleRate: SAMPLE_RATE });
      } catch {
        ctx = null;
        return false;
      }
    }
    if (ctx.state === 'suspended') ctx.resume().catch(() => {});
    outChain();
    return true;
  }

  /**
   * Build the master out chain once per context.
   *
   * `meter` sits in the path rather than hanging off the source so that the
   * level it reports is the level actually handed to the device, gain included.
   */
  function outChain() {
    if (out) return out;
    const gain = ctx.createGain();
    gain.gain.value = volume;
    const meter = ctx.createAnalyser();
    // A short window: long enough to average, short enough that the bar tracks
    // the phrase instead of lagging a beat behind it.
    meter.fftSize = 1024;
    meter.smoothingTimeConstant = 0.6;
    gain.connect(meter);
    meter.connect(ctx.destination);
    out = { gain, meter, buf: new Float32Array(meter.fftSize) };
    return out;
  }

  function stopSource() {
    const stream = activeStream;
    activeStream = null;
    if (stream) {
      stream.stopRequested = true;
      stream.cancel().finally(() => stream.close()).catch(() => {});
    }
    if (!source) return;
    source.onended = null;
    try {
      if (typeof source.stop === 'function') source.stop();
      else source.port?.postMessage({ type: 'stop' });
    } catch { /* already stopped */ }
    try { source.disconnect(); } catch { /* already disconnected */ }
    source = null;
  }

  function assertPlaybackIntent(intent) {
    if (intent !== playbackIntent) throw playbackCancelledError();
  }

  /** Playhead as a tick, from the audio clock. `null` when nothing is playing. */
  function tickNow() {
    if (!started || !ctx) return null;
    if (started.rate && Number.isFinite(started.frame)) {
      // Worklet messages anchor the exact output frame. Interpolate between
      // those anchors with AudioContext.currentTime so one delayed progress
      // message cannot freeze the visible playhead while audio keeps playing.
      // A worklet underrun outputs silence without consuming project frames;
      // subtract those frames so the playhead stays with the PCM that resumes.
      const elapsed = Math.max(0, ctx.currentTime - (started.frameAt ?? started.at));
      const missedFrames = Math.max(0, streamUnderruns.frames - (started.underrunFramesAtAnchor ?? 0));
      const elapsedFrames = Math.max(0, elapsed * started.rate - missedFrames);
      const frame = Math.min(started.frames, started.frame + elapsedFrames);
      return msToTick(frame / started.rate * 1000, getBpm());
    }
    // Between the call to `start()` and `at` the clock is still behind it, so
    // a negative elapsed would walk the playhead backwards for a few ms.
    const elapsed = Math.max(0, ctx.currentTime - started.at) * 1000;
    return msToTick(started.absMs + elapsed, getBpm());
  }

  /**
   * Start (or restart) playback at `tick`.
   *
   * @returns {number} the tick playback actually began at, read back from the
   *   audio clock rather than from what was asked for.
   */
  async function play(tick, trace = null) {
    playbackTrace = trace;
    trace?.mark?.('player_start');
    const intent = ++playbackIntent;
    const streamConfig = await getSessionStream();
    trace?.mark?.(streamConfig ? 'stream_config_ready' : 'no_stream_config');
    assertPlaybackIntent(intent);
    const take = getTake();
    // A selected-track preview is not a safe substitute for a rejected
    // full-project stream. In that case only a completed mixdown may fallback.
    const fallbackTake = streamConfig ? getCompletedTake?.() : take;
    const pool = streamConfig ? null : getPool();
    if (!streamConfig && !take?.takeId && !take?.samples?.length && !pool) {
      throw new Error('nothing has been rendered yet — the waveform strip renders the song first');
    }
    if (!unlock()) throw new Error('this browser has no Web Audio support');
    await ctx.resume();
    assertPlaybackIntent(intent);

    const bpm = getBpm();
    const wantMs = tickToMs(tick, bpm);
    // Native streaming currently starts at frame zero and requires AudioWorklet.
    // A completed take still supports seeking and older browsers, so use it
    // whenever the streaming path cannot serve this request.
    if (streamConfig && wantMs <= 0.5 && ctx.audioWorklet && typeof AudioWorkletNode !== 'undefined') {
      try {
        trace?.mark?.('stream_starting');
        return await startSessionStream(streamConfig, wantMs, bpm, intent);
      } catch (error) {
        assertPlaybackIntent(intent);
        if (error?.code === 'PLAYBACK_CANCELLED') throw error;
        // Startup may fail because the server has not enabled render.stream,
        // the WebSocket is unavailable, or the current Session cannot begin.
        // Fall through to the already-rendered source when one exists. Errors
        // after playback starts are reported by the asynchronous pump instead.
        if (!(fallbackTake?.takeId || fallbackTake?.samples?.length) && !pool) throw error;
        trace?.mark?.('stream_start_failed_fallback', { reason: streamFailureStage(error) });
      }
    }
    if (!(fallbackTake?.takeId || fallbackTake?.samples?.length) && !pool) {
      if (streamConfig) throw new Error('Progressive playback is unavailable here and no completed mixdown is available');
      throw new Error('nothing has been rendered yet — the waveform strip renders the song first');
    }
    // The full project mixdown wins over the preview pool so every track shares
    // one song timeline and wave parts are included in the transport audio.
    assertPlaybackIntent(intent);
    if (fallbackTake?.takeId || fallbackTake?.samples?.length) {
      trace?.mark?.('mixdown_starting', { sampleRate: fallbackTake.sampleRate || SAMPLE_RATE });
      return startTake(fallbackTake, wantMs, bpm, intent);
    }
    trace?.mark?.('render_pool_starting');
    return startFromPool(pool, wantMs, bpm);
  }

  async function startSessionStream(config, wantMs, bpm, intent) {
    playbackTrace?.mark?.('stream_worklet_load_started');
    workletReady ||= ctx.audioWorklet.addModule(new URL('./session-stream.worklet.js', import.meta.url));
    await workletReady;
    playbackTrace?.mark?.('stream_worklet_load_ready');
    assertPlaybackIntent(intent);
    stopSource();
    const transport = new SessionStreamSocket(config);
    activeStream = transport;
    let begin;
    playbackTrace?.mark?.('stream_begin_request_started');
    try { begin = await transport.begin(); }
    catch (error) {
      if (activeStream === transport) activeStream = null;
      await transport.close();
      if (transport.stopRequested) throw playbackCancelledError();
      throw error;
    }
    playbackTrace?.mark?.('stream_begin_ready', { sampleRate: begin.sample_rate || SAMPLE_RATE });
    assertPlaybackIntent(intent);
    if (activeStream !== transport) {
      throw transport.stopRequested ? playbackCancelledError() : new Error('Audio stream stopped');
    }
    const rate = begin.sample_rate || SAMPLE_RATE;
    const startFrame = 0;
    const node = new AudioWorkletNode(ctx, 'rusted-utau-session-stream-player', {
      numberOfInputs: 0, numberOfOutputs: 1, outputChannelCount: [2],
    });
    streamUnderruns = { events: 0, frames: 0, last: null };
    let bufferedFrames = 0;
    let expectedFrame = 0;
    let ended = false;
    let pumpPromise = null;
    const at = ctx.currentTime + START_LEAD_S;
    source = node;
    node.connect(outChain().gain);
    node.port.onmessage = event => {
      if (source !== node) return;
      const message = event.data;
      if (message?.type === 'progress') {
        bufferedFrames = message.bufferedFrames;
        if (started) {
          started.frame = message.frame;
          started.frameAt = Number.isFinite(message.contextTime) ? message.contextTime : ctx.currentTime;
          started.underrunFramesAtAnchor = streamUnderruns.frames;
        }
      } else if (message?.type === 'need-data') {
        bufferedFrames = message.bufferedFrames;
        startPump();
      } else if (message?.type === 'ended') {
        ended = true;
        const endMs = message.frame / rate * 1000;
        started = null;
        parkedAt = endMs;
        onEnded(endMs);
        if (activeStream === transport) activeStream = null;
        transport.close().catch(() => {});
      } else if (message?.type === 'first-audio-output') {
        playbackTrace?.mark?.('audio_worklet_first_output', { frame: message.frame, frameCount: message.frameCount });
      } else if (message?.type === 'underrun'
        && Number.isSafeInteger(message.missingFrames) && message.missingFrames > 0
        && Number.isSafeInteger(message.frameStart) && message.frameStart >= 0) {
        streamUnderruns = {
          events: streamUnderruns.events + 1,
          frames: streamUnderruns.frames + message.missingFrames,
          last: { frameStart: message.frameStart, missingFrames: message.missingFrames },
        };
        try { onStreamUnderrun({ ...streamUnderruns, last: { ...streamUnderruns.last } }); }
        catch { /* diagnostics must never interrupt audio playback */ }
      }
    };

    const failStream = (error, notify = true) => {
      if (activeStream !== transport) return;
      activeStream = null;
      started = null;
      stopSource();
      transport.cancel().finally(() => transport.close()).catch(() => {});
      if (notify) onError(String(error?.message || error));
    };

    const enqueue = block => {
      const { header, left, right } = block.pcm;
      if (header.session_id !== config.sessionId || header.revision !== config.revision
        || header.frame_start !== expectedFrame || header.frame_count !== left.length || left.length !== right.length) {
        throw new Error('Native stream returned PCM for a different project revision');
      }
      const frameCount = left.length;
      if (!playbackTrace?.firstPcmReceived) {
        playbackTrace?.mark?.('first_pcm_received', { frameStart: header.frame_start, frameCount });
        if (playbackTrace) playbackTrace.firstPcmReceived = true;
      }
      expectedFrame += frameCount;
      const leftCopy = left.buffer;
      const rightCopy = right.buffer;
      node.port.postMessage({ type: 'push', frameStart: header.frame_start, left, right }, [leftCopy, rightCopy]);
      bufferedFrames += frameCount;
      if (!playbackTrace?.firstPcmEnqueued) {
        playbackTrace?.mark?.('first_pcm_enqueued', { frameCount, bufferedFrames });
        if (playbackTrace) playbackTrace.firstPcmEnqueued = true;
      }
    };

    const pump = async () => {
      while (activeStream === transport && !ended && bufferedFrames < rate * STREAM_BUFFER_TARGET_SECONDS) {
        const block = await transport.next(begin.limits.max_block_frames);
        if (activeStream !== transport) return;
        if (block.status === 'data') enqueue(block);
        else if (block.status === 'pending') continue;
        else if (block.status === 'end') {
          ended = true;
          node.port.postMessage({ type: 'finish' });
          try { await transport.request('session_render_stream_close', { stream_id: transport.streamId }); } catch { /* end is already terminal */ }
          return;
        } else if (block.status === 'cancelled') return;
        else throw new Error(block.error?.message || `Native stream ended with ${block.status || 'an invalid status'}`);
      }
    };

    const startPump = () => {
      if (activeStream !== transport || ended || pumpPromise) return;
      pumpPromise = pump().catch(error => failStream(error)).finally(() => {
        pumpPromise = null;
        if (activeStream === transport && !ended) {
          // need-data is edge-triggered. Let the worklet re-check its actual
          // queue after the main-thread estimate says this pump is complete.
          node.port.postMessage({ type: 'pump-complete' });
        }
      });
    };

    try {
      // Wait for the first PCM before starting the audio clock, then maintain
      // a bounded sixteen-second lookahead to absorb phrase-synthesis latency.
      while (activeStream === transport && bufferedFrames === 0) {
        const first = await transport.next(begin.limits.max_block_frames);
        if (first.status === 'data') enqueue(first);
        else if (first.status === 'pending') continue;
        else if (first.status === 'end') throw new Error('The native stream ended without audio');
        else throw new Error(first.error?.message || `Native stream ended with ${first.status || 'an invalid status'}`);
      }
      if (activeStream !== transport) throw new Error('Audio stream stopped');
      node.port.postMessage({ type: 'start', frame: startFrame, startAt: at });
      playbackTrace?.mark?.('audio_scheduled', { startAtContextTime: at, sampleRate: rate });
      started = { at, absMs: wantMs, frame: startFrame, frameAt: at, frames: Infinity, rate, underrunFramesAtAnchor: streamUnderruns.frames };
      startPump();
      return msToTick(wantMs, bpm);
    } catch (error) {
      if (transport.stopRequested) throw playbackCancelledError();
      // The stream failed before audio was scheduled. Let play() use the
      // completed-take/RenderPool fallback without surfacing a transient error.
      failStream(error, false);
      throw error;
    }
  }

  /**
   * Schedule a whole-track take at project time `wantMs`.
   *
   * The take starts at project time 0, so the offset is the playhead itself and
   * only the tail needs clamping — a project can be longer than its audio, and
   * seeking into that tail must play out the end rather than fail.
   */
  async function startTake(take, wantMs, bpm, intent) {
    if (take.takeId) return startWorkerTake(take, wantMs, bpm, intent);
    const pcm = take.samples instanceof Float32Array ? take.samples : Float32Array.from(take.samples);
    const channels = take.channels?.length ? take.channels : [pcm];
    const rate = take.sampleRate || SAMPLE_RATE;
    const rangeMs = (channels[0]?.length ?? pcm.length) / rate * 1000;
    const fromMs = Math.max(0, Math.min(wantMs, rangeMs));
    const offsetS = fromMs / 1000;
    const frameCount = channels[0]?.length ?? pcm.length;
    playbackTrace?.mark?.('audio_buffer_build_started', {
      channels: channels.length,
      frames: frameCount,
      pcmBytes: channels.reduce((total, channel) => total + (channel?.byteLength ?? 0), 0),
      sampleRate: rate,
    });
    const buffer = ctx.createBuffer(channels.length, channels[0]?.length ?? pcm.length, rate);
    playbackTrace?.mark?.('audio_buffer_allocated');
    playbackTrace?.mark?.('audio_buffer_copy_started');
    channels.forEach((channel, index) => buffer.copyToChannel(channel, index));
    playbackTrace?.mark?.('audio_buffer_copy_done');
    stopSource();
    source = ctx.createBufferSource();
    source.buffer = buffer;
    source.connect(outChain().gain);
    const at = ctx.currentTime + START_LEAD_S;
    source.start(at, offsetS);
    playbackTrace?.mark?.('audio_scheduled', { startAtContextTime: at, sampleRate: rate, startFrame: Math.floor(offsetS * rate) });
    started = { at, absMs: fromMs };
    source.onended = () => {
      if (started && started.at <= at) {
        started = null;
        parkedAt = rangeMs;
        onEnded(parkedAt);
      }
    };
    return msToTick(started.absMs, bpm);
  }

  /**
   * A WASM take can be several hundred seconds long. Its channel buffers stay
   * in the session worker until play; that worker transfers fresh channel
   * buffers here, then this transfers them to AudioWorklet without copying the
   * full PCM on the UI thread.
   */
  async function startWorkerTake(take, wantMs, bpm, intent) {
    if (!getTakeAudio) throw new Error('WASM playback worker is unavailable');
    if (!ctx.audioWorklet || typeof AudioWorkletNode === 'undefined') {
      throw new Error('This browser does not support streaming rendered audio');
    }
    workletReady ||= ctx.audioWorklet.addModule(new URL('./audio-take.worklet.js', import.meta.url));
    await workletReady;
    assertPlaybackIntent(intent);
    const audio = await getTakeAudio(take.takeId);
    assertPlaybackIntent(intent);
    const channels = audio.channels || [];
    if (!channels.length || !channels[0]?.length) throw new Error('The rendered take has no audio samples');
    const rate = audio.sampleRate || take.sampleRate || SAMPLE_RATE;
    const frames = channels[0].length;
    const rangeMs = frames / rate * 1000;
    const fromMs = Math.max(0, Math.min(wantMs, rangeMs));
    stopSource();
    const node = new AudioWorkletNode(ctx, 'rusted-utau-take-player', {
      numberOfInputs: 0,
      numberOfOutputs: 1,
      outputChannelCount: [channels.length],
    });
    const at = ctx.currentTime + START_LEAD_S;
    node.port.onmessage = event => {
      if (source !== node) return;
      if (event.data?.type === 'progress' && started) {
        started.frame = event.data.frame;
        started.frameAt = Number.isFinite(event.data.contextTime) ? event.data.contextTime : ctx.currentTime;
        return;
      }
      if (event.data?.type === 'first-audio-output') {
        playbackTrace?.mark?.('audio_worklet_first_output', { frame: event.data.frame, frameCount: event.data.frameCount });
        return;
      }
      if (event.data?.type === 'ended') {
        started = null;
        parkedAt = rangeMs;
        onEnded(parkedAt);
      }
    };
    node.connect(outChain().gain);
    source = node;
    const startFrame = Math.floor(fromMs / 1000 * rate);
    started = { at, absMs: fromMs, frame: startFrame, frameAt: at, frames, rate };
    node.port.postMessage({
      type: 'start', channels, frame: startFrame, startAt: at,
    }, channels.map(channel => channel.buffer));
    playbackTrace?.mark?.('audio_scheduled', { startAtContextTime: at, sampleRate: rate, startFrame });
    return msToTick(started.absMs, bpm);
  }

  /**
   * Schedule the pool's cached mix from the chunk the playhead is in.
   *
   * Pressing play before the whole plan is rendered starts as soon as the cache
   * reaches that chunk, instead of waiting for the song.
   */
  function startFromPool(pool, wantMs, bpm) {
    const index = chunkIndexForMs(pool, wantMs);
    const pcm = pool.mixFrom(index);
    if (!pcm || !pcm.length) {
      throw new Error(`the render cache does not reach this point in the song yet (chunk ${index + 1} of ${pool.plan.length})`);
    }
    const originMs = chunkOriginMs(pool, index);
    const rangeMs = pcm.length / SAMPLE_RATE * 1000;
    // A project can be longer than its audio: the render only covers where the
    // engine planned chunks. Skipping into the tail must play out the end of the
    // take rather than fail — the transport then stops by itself, which is what
    // "the song is over" looks like.
    wantMs = Math.max(originMs, Math.min(wantMs, originMs + rangeMs));
    const offsetS = (wantMs - originMs) / 1000;

    const buffer = ctx.createBuffer(1, pcm.length, SAMPLE_RATE);
    buffer.copyToChannel(pcm instanceof Float32Array ? pcm : Float32Array.from(pcm), 0);
    stopSource();
    source = ctx.createBufferSource();
    source.buffer = buffer;
    source.connect(outChain().gain);
    const at = ctx.currentTime + START_LEAD_S;
    source.start(at, offsetS);
    started = { at, absMs: originMs + offsetS * 1000 };
    source.onended = () => {
      // Only a natural end: a manual `stopSource()` clears the handler first,
      // so a skip or a pause cannot be mistaken for the song finishing.
      if (started && started.at <= at) {
        started = null;
        parkedAt = originMs + pcm.length / SAMPLE_RATE * 1000;
        onEnded(parkedAt);
      }
    };
    return msToTick(started.absMs, bpm);
  }

  /** Stop, keeping the playhead where the audio actually is. */
  function pause() {
    playbackIntent++;
    const at = tickNow();
    if (at !== null) parkedAt = tickToMs(at, getBpm());
    stopSource();
    started = null;
    return at;
  }

  /**
   * Output devices the browser is allowed to name.
   *
   * `enumerateDevices` only labels outputs after a permission has been granted
   * at least once, so `default` carries no name until then — which is why the
   * caller shows labels when it has them and ids when it does not.
   *
   * @returns {Promise<{id: string, label: string}[]>}
   */
  async function listOutputs() {
    try {
      const devices = await navigator.mediaDevices.enumerateDevices();
      return devices
        .filter(d => d.kind === 'audiooutput')
        .map((d, i) => ({ id: d.deviceId, label: d.label || `output ${i}` }));
    } catch {
      return [];
    }
  }

  /**
   * Send the output to a specific device.
   *
   * A browser can pick a sink the machine has no speaker on (an HDMI port with
   * nothing plugged in, for instance), which is indistinguishable from "the app
   * is broken" until you can change the device. Requires a context, so the
   * caller runs it from a click.
   *
   * @param {string} id deviceId, or '' for the browser default
   */
  async function setOutputDevice(id) {
    sinkId = id || '';
    if (!unlock()) throw new Error('this browser has no Web Audio support');
    // `setSinkId` on the context is the Web Audio half of `HTMLMediaElement`'s
    // device selection; where it is missing the choice cannot be honoured, and
    // saying so beats silently keeping the old device.
    if (typeof ctx.setSinkId !== 'function') {
      if (sinkId) throw new Error('this browser cannot choose an audio output device');
      return null;
    }
    await ctx.setSinkId(sinkId);
    return sinkId;
  }

  /** Master output level in dBFS, or null before a context exists. */
  function levelDb() {
    if (!ctx || !out) return null;
    const { meter, buf } = out;
    if (ctx.state !== 'running') return null;
    meter.getFloatTimeDomainData(buf);
    let sum = 0;
    for (let i = 0; i < buf.length; i++) sum += buf[i] * buf[i];
    const rms = Math.sqrt(sum / buf.length);
    // The RMS, not the peak: a level meter on RMS does not twitch on a single
    // click, so what you see is what you hear.
    if (rms > 1e-5) {
      const db = 20 * Math.log10(rms);
      if (started && playbackTrace && !playbackTrace.firstAudibleMeter) {
        playbackTrace.mark?.('first_non_silent_output_meter', { dbfs: Number(db.toFixed(1)), contextTime: ctx.currentTime });
        playbackTrace.firstAudibleMeter = true;
      }
      return db;
    }
    return -100;
  }

  return {
    unlock,
    play,
    pause,
    tickNow,
    listOutputs,
    setOutputDevice,
    levelDb,
    /** The device the output is going to, '' until one is chosen. */
    get outputDevice() { return sinkId; },
    /** Debug probe: graph state without reaching into closures. */
    get __dbg() { return { ctxState: ctx?.state, ctxTime: ctx?.currentTime, sampleRate: ctx?.sampleRate, baseLatency: ctx?.baseLatency, outputLatency: ctx?.outputLatency, hasOut: !!out, hasCtx: !!ctx, sinkId, volume, streamUnderruns: { ...streamUnderruns, last: streamUnderruns.last && { ...streamUnderruns.last } } }; },
    /** Master gain, 0..1.5. */
    setVolume(v) {
      volume = Math.max(0, Math.min(1.5, Number(v) || 0));
      if (ctx && out) out.gain.gain.value = volume;
      return volume;
    },
    /** True while audio is running; the caller falls back to a silent clock otherwise. */
    get playing() { return started !== null; },
    /** Last known position in project ms, for a transport that starts silent. */
    get parkedMs() { return parkedAt; },
    dispose() {
      playbackIntent++;
      stopSource();
      started = null;
      out = null;
      ctx?.close?.().catch(() => {});
      ctx = null;
    },
  };
}
