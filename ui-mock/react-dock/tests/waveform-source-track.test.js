// The waveform strip renders the ACTIVE track, not always track 0.
//
// This is what makes an audio (wave) track drawable: a wave part has no
// phrases, so only its own track's render carries its waveform. When the strip
// asked the server for track 0 whatever the user had selected, selecting an
// audio track silently drew a voice track's peaks instead — a wrong picture
// with no error anywhere.
//
// The render pool is stubbed out so this exercises the server fallback, which
// is the path the HTTP backend actually takes in a production bundle (no
// fixture bank is bundled there).
import test from 'node:test';
import assert from 'node:assert/strict';

const RATE = 44100;
/** 1000 frames of silence in a 16-bit PCM wav: a decodable, empty take. */
function silentWav(frames = 1000) {
  const bytes = new Uint8Array(44 + frames * 2);
  const view = new DataView(bytes.buffer);
  const tag = (offset, text) => [...text].forEach((c, i) => view.setUint8(offset + i, c.charCodeAt(0)));
  tag(0, 'RIFF');
  view.setUint32(4, 36 + frames * 2, true);
  tag(8, 'WAVE');
  tag(12, 'fmt ');
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, RATE, true);
  view.setUint32(28, RATE * 2, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  tag(36, 'data');
  view.setUint32(40, frames * 2, true);
  return bytes;
}

/** Stub `fetch` for the render-job flow and record every request body. */
function stubFetch(calls) {
  globalThis.fetch = async (url, init = {}) => {
    calls.push({ url: String(url), method: init.method || 'GET', body: init.body ? JSON.parse(init.body) : undefined });
    if (String(url).endsWith('/voicebanks')) {
      return json({ voicebanks: [{ name: 'bank', wav_count: 12 }] });
    }
    if (String(url).includes('/render') && (init.method || 'GET') === 'POST') {
      return json({ render_id: 'rj-1' });
    }
    if (/\/api\/v2\/render\/rj-1$/.test(String(url))) {
      return json({ status: 'done', frames: 1000, sample_rate: RATE });
    }
    if (String(url).endsWith('/audio')) {
      return { ok: true, arrayBuffer: async () => silentWav().buffer };
    }
    throw new Error(`unexpected fetch ${url}`);
  };
}

function json(value) {
  return { ok: true, status: 200, text: async () => JSON.stringify(value) };
}

test('the server fallback renders the track it was given, not always track 0', async () => {
  const originalFetch = globalThis.fetch;
  const originalWorker = globalThis.Worker;
  const originalWorkerUrl = globalThis.Worker?.url;
  try {
    // The pool cannot be built without workers/fixture bank, so the module
    // falls through to the server path — which is what we want to observe.
    delete globalThis.Worker;
    const calls = [];
    stubFetch(calls);

    const { renderProjectAudio } = await import('../src/openutau/waveform-source.js');
    const audio = await renderProjectAudio({
      kind: 'http',
      baseUrl: '',
      sessionId: 's-1',
      revision: 7,
      ustx: 'name: x\n',
      track: 4,
    });

    assert.equal(audio.sampleRate, RATE);
    assert.equal(audio.samples.length, 1000);
    const post = calls.find(call => call.method === 'POST' && call.url.includes('/render'));
    assert.ok(post, 'a render job must be started');
    assert.equal(post.body.track, 4, `expected track 4, sent ${JSON.stringify(post.body)}`);
    assert.equal(post.body.expected_revision, 7);
    // The waveform must come from the engine, not be invented here.
    assert.equal(audio.source, 'synth-server');
  } finally {
    globalThis.fetch = originalFetch;
    if (originalWorker) {
      globalThis.Worker = originalWorker;
      if (originalWorkerUrl) Object.defineProperty(globalThis.Worker, 'url', { value: originalWorkerUrl, configurable: true });
    }
  }
});

test('track 0 stays the default so an unselected strip behaves as before', async () => {
  const originalFetch = globalThis.fetch;
  const originalWorker = globalThis.Worker;
  const originalWorkerUrl = globalThis.Worker?.url;
  try {
    delete globalThis.Worker;
    const calls = [];
    stubFetch(calls);
    const { renderProjectAudio } = await import('../src/openutau/waveform-source.js');
    await renderProjectAudio({ kind: 'http', baseUrl: '', sessionId: 's-1', revision: 1, ustx: 'name: x\n' });
    const post = calls.find(call => call.method === 'POST' && call.url.includes('/render'));
    assert.equal(post.body.track, 0);
  } finally {
    globalThis.fetch = originalFetch;
    if (originalWorker) {
      globalThis.Worker = originalWorker;
      if (originalWorkerUrl) Object.defineProperty(globalThis.Worker, 'url', { value: originalWorkerUrl, configurable: true });
    }
  }
});