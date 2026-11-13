// Rusted UTAU web engine adapter — loads the wasm render engine and bridges OPFS.
//
// The Rust side (`crates/engine-wasm`) owns all synthesis; this file only
// moves bytes: voicebank files out of OPFS, project text out of a file, and
// the rendered WAV back to the audio element.
//
// Verified in Node/browser? NOT YET — see docs/planning/wasm-migration-status.md.
// The Rust API it calls is covered by `cargo test -p engine-wasm` (the
// in-memory path renders bit-identically to native).

import init, { Engine } from "./pkg/engine_wasm.js";

/** Voicebank files live under this OPFS directory. */
const VOICE_ROOT = "voicebank";

/**
 * Minimal OPFS helper. Returns null when OPFS is unavailable so the caller
 * can fall back to a zip upload.
 */
export async function opfsAvailable() {
  return typeof navigator !== "undefined" && !!navigator.storage?.getDirectory;
}

async function opfsDir(name, { create = false } = {}) {
  const root = await navigator.storage.getDirectory();
  return root.getDirectoryHandle(name, { create });
}

async function readOpfsFile(path) {
  // path looks like "voicebank/voice/a.wav"
  const parts = path.split("/").filter(Boolean);
  let dir = await navigator.storage.getDirectory();
  for (const part of parts.slice(0, -1)) {
    dir = await dir.getDirectoryHandle(part);
  }
  const handle = await dir.getFileHandle(parts[parts.length - 1]);
  const file = await handle.getFile();
  return new Uint8Array(await file.arrayBuffer());
}

async function writeOpfsFile(path, bytes) {
  const parts = path.split("/").filter(Boolean);
  let dir = await navigator.storage.getDirectory();
  for (const part of parts.slice(0, -1)) {
    dir = await dir.getDirectoryHandle(part, { create: true });
  }
  const handle = await dir.getFileHandle(parts[parts.length - 1], { create: true });
  const writable = await handle.createWritable();
  await writable.write(bytes);
  await writable.close();
}

/**
 * Store a voicebank into OPFS from a directory-picker `FileList` (Chrome /
 * Firefox) or any list of `{ path, bytes }`.
 *
 * Layout expected: `voice/oto.ini` + `voice/*.wav` (+ `*_wav.frq`).
 */
export async function importVoicebankFiles(files) {
  if (!(await opfsAvailable())) {
    throw new Error("OPFS unavailable — use the zip import path");
  }
  let count = 0;
  for (const f of files) {
    // webkitRelativePath carries the picked directory structure.
    const rel = f.webkitRelativePath || f.name;
    await writeOpfsFile(`${VOICE_ROOT}/${rel}`, new Uint8Array(await f.arrayBuffer()));
    count += 1;
  }
  return count;
}

/** Load the wasm module once. */
export async function loadEngine(wasmUrl = new URL("./pkg/engine_wasm_bg.wasm", import.meta.url)) {
  const wasm = await init({ module_or_path: wasmUrl });
  const engine = new Engine();
  // Attach the raw exports so callers can inspect memory if needed.
  engine.__wasm = wasm;
  return engine;
}

/**
 * Install the voicebank stored in OPFS into `engine`.
 *
 * Returns `{ aliases, samples, withFrq }`.
 */
export async function attachVoicebankFromOpfs(engine, { id = "web", name = "Voicebank", otoDir = "voice" } = {}) {
  const otoPath = `${VOICE_ROOT}/${otoDir}/oto.ini`;
  const oto = await readOpfsFile(otoPath);
  engine.set_voicebank(id, name, otoDir, oto);

  let samples = 0;
  let withFrq = 0;
  for (const key of engine.wav_keys().split("\n").filter(Boolean)) {
    const wav = await readOpfsFile(`${VOICE_ROOT}/${key}`);
    // `<stem>_wav.frq` sits next to the wav (UTAU convention). The reference
    // renderer always prefers it, so attach it when present — without it the
    // pitch track differs from the native render.
    const frqPath = `${VOICE_ROOT}/${key.replace(/\.wav$/i, "_wav.frq")}`;
    let frq = null;
    try {
      frq = await readOpfsFile(frqPath);
    } catch {
      frq = null;
    }
    if (frq) {
      engine.add_sample_with_frq(key, wav, frq);
      withFrq += 1;
    } else {
      engine.add_sample(key, wav);
    }
    samples += 1;
  }
  return { aliases: engine.alias_count(), samples, withFrq };
}

/** Render a project (YAML text) and return a playable object URL. */
export function renderToUrl(engine, ustxYaml, track = 0) {
  const wav = engine.render_project_wav(ustxYaml, track);
  return URL.createObjectURL(new Blob([wav], { type: "audio/wav" }));
}

/** Render a project and return raw mono f32 samples. */
export function renderSamples(engine, ustxYaml, track = 0) {
  return engine.render_project(ustxYaml, track);
}
