// Browser-side WORLDLINE-R2 ONNX runtime. Feature generation stays with the
// project engine; this module owns the mel/vocoder sessions and can run in a
// dedicated worker as well as on the main thread.

const MODEL_ROOT = new URL("../../models/r2/", import.meta.url);
const VOCODER_FILE = "pc-nsf-hifigan/pc_nsf_hifigan_44.1k_hop512_128bin_2025.02.onnx";
const ASSET_ROOT = new URL("../vendor/ort-webgpu/", import.meta.url);
const WEBGPU_ENTRY = new URL("ort.webgpu.min.mjs", ASSET_ROOT);
const WEBGPU_WASM_MJS = new URL("ort-wasm-simd-threaded.asyncify.mjs", ASSET_ROOT).href;
const WEBGPU_WASM = new URL("ort-wasm-simd-threaded.asyncify.wasm", ASSET_ROOT).href;
const CPU_ROOT = new URL("../vendor/ort/", import.meta.url);
const CPU_ENTRY = new URL("ort.min.mjs", CPU_ROOT);
const CPU_WASM_MJS = new URL("ort-wasm-simd-threaded.mjs", CPU_ROOT).href;
const CPU_WASM = new URL("ort-wasm-simd-threaded.wasm", CPU_ROOT).href;
const MEL_SHA256 = "1493ffc868c6fb8b66104e2c6a2902cbf1a8c02bb09215db014f02e9d0e82f94";
const VOCODER_SHA256 = "b297c0c790dbcfe6b2123d87cc20bf8718969f91ffa3170a198c8fb53686155d";
const R2_HOP = 512;
const R2_LEFT_PAD = 4;

async function sha256(buffer) {
  const digest = await crypto.subtle.digest("SHA-256", buffer);
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

/**
 * Load the two pinned WORLDLINE-R2 ONNX models and keep their sessions alive.
 * `ep: "webgpu"` requires a secure-context WebGPU adapter; both ONNX models
 * try WebGPU first and fall back to WASM for unsupported operators.
 */
export class R2Runtime {
  static async load({
    ep = "webgpu",
    melUrl = new URL("mel.onnx", MODEL_ROOT),
    vocoderUrl = new URL(VOCODER_FILE, MODEL_ROOT),
    onStatus = () => {},
  } = {}) {
    if (ep !== "webgpu" && ep !== "wasm") throw new Error(`unsupported R2 EP: ${ep}`);
    if (ep === "webgpu" && !navigator.gpu) {
      throw new Error("R2 WebGPU needs a secure context and a browser WebGPU adapter");
    }

    const ortEntry = ep === "webgpu" ? WEBGPU_ENTRY : CPU_ENTRY;
    const importStart = performance.now();
    onStatus(`import ${ortEntry.pathname}`);
    const imported = await import(/* @vite-ignore */ ortEntry.href);
    const ort = imported.default ?? imported;
    const importMs = performance.now() - importStart;
    const wasmMjs = ep === "webgpu" ? WEBGPU_WASM_MJS : CPU_WASM_MJS;
    const wasmFile = ep === "webgpu" ? WEBGPU_WASM : CPU_WASM;
    ort.env.wasm.wasmPaths = { mjs: wasmMjs, wasm: wasmFile };
    ort.env.wasm.numThreads = 1;
    ort.env.wasm.proxy = false;
    ort.env.logLevel = "warning";
    if (ep === "webgpu") ort.env.webgpu.powerPreference = "high-performance";

    const melPath = new URL(melUrl, location.href);
    const vocoderPath = new URL(vocoderUrl, location.href);
    const loadOne = async (url, providers, label, expectedSha) => {
      onStatus(`fetch ${label}`);
      const start = performance.now();
      const response = await fetch(url);
      if (!response.ok) throw new Error(`${label}: HTTP ${response.status} (${url})`);
      const buffer = await response.arrayBuffer();
      const fetchMs = performance.now() - start;
      const hash = await sha256(buffer);
      if (hash !== expectedSha) {
        throw new Error(`${label} SHA-256 mismatch: got ${hash}, expected ${expectedSha}`);
      }
      const sessionStart = performance.now();
      const session = await ort.InferenceSession.create(new Uint8Array(buffer), {
        executionProviders: providers,
        graphOptimizationLevel: "all",
        intraOpNumThreads: 1,
        interOpNumThreads: 1,
      });
      return {
        session,
        report: {
          url: url.pathname,
          bytes: buffer.byteLength,
          sha256: hash,
          fetchMs,
          sessionMs: performance.now() - sessionStart,
          providers,
          inputs: session.inputNames.slice(),
          outputs: session.outputNames.slice(),
        },
      };
    };

    const neuralProviders = ep === "webgpu" ? ["webgpu", "wasm"] : ["wasm"];
    let mel, vocoder;
    try {
      onStatus("create mel session");
      mel = await loadOne(melPath, neuralProviders, "mel.onnx", MEL_SHA256);
      onStatus("create vocoder session");
      vocoder = await loadOne(vocoderPath, neuralProviders, "pc-nsf-hifigan", VOCODER_SHA256);
    } catch (error) {
      await mel?.session.release().catch(() => {});
      throw error;
    }

    for (const input of ["f0", "sp_env", "ap"]) {
      if (!mel.report.inputs.includes(input)) {
        await mel.session.release();
        await vocoder.session.release();
        throw new Error(`mel.onnx missing input ${input}`);
      }
    }
    for (const input of ["mel", "f0"]) {
      if (!vocoder.report.inputs.includes(input)) {
        await mel.session.release();
        await vocoder.session.release();
        throw new Error(`pc-nsf-hifigan missing input ${input}`);
      }
    }

    const device = ep === "webgpu" ? ort.env.webgpu.device : null;
    const adapterInfo = device?.adapterInfo;
    const runtime = new R2Runtime(ort, ep, mel, vocoder, {
      version: ort.env.versions?.web ?? ort.env.versions?.common ?? null,
      importMs,
      device: adapterInfo ? Object.fromEntries(
        ["vendor", "architecture", "device", "description"].map((k) => [k, adapterInfo[k] || ""]),
      ) : null,
    });
    onStatus(`ready · ${runtime.info.version} · ${ep}`);
    return runtime;
  }

  constructor(ort, ep, mel, vocoder, info) {
    this.ort = ort;
    this.ep = ep;
    this.mel = mel.session;
    this.vocoder = vocoder.session;
    this.models = { mel: mel.report, vocoder: vocoder.report };
    this.info = info;
    this.disposed = false;
  }

  async runMel({ f0, spEnv, ap, frames, bins = 1025 }) {
    if (this.disposed) throw new Error("R2 runtime has been disposed");
    const outputs = await this.mel.run({
      f0: new this.ort.Tensor("float32", f0, [1, frames]),
      sp_env: new this.ort.Tensor("float32", spEnv, [1, frames, bins]),
      ap: new this.ort.Tensor("float32", ap, [1, frames, bins]),
    });
    const tensor = outputs[this.models.mel.outputs.includes("mel") ? "mel" : this.models.mel.outputs[0]];
    return { data: Float32Array.from(tensor.data), dims: Array.from(tensor.dims) };
  }

  async runVocoder({ mel, f0, frames, bins = 128 }) {
    if (this.disposed) throw new Error("R2 runtime has been disposed");
    const outputs = await this.vocoder.run({
      mel: new this.ort.Tensor("float32", mel, [1, frames, bins]),
      f0: new this.ort.Tensor("float32", f0, [1, frames]),
    });
    const name = this.models.vocoder.outputs.includes("waveform")
      ? "waveform" : this.models.vocoder.outputs[0];
    const tensor = outputs[name];
    return { data: Float32Array.from(tensor.data), dims: Array.from(tensor.dims) };
  }

  /**
   * Render an `R2WF` packet returned by engine-wasm for one planned chunk.
   * Packet layout: magic, frame/bin counts, four f64 trim positions, last
   * chunk flag, then f32 f0/sp_env/ap arrays.
   */
  async renderFeaturePacket(packet) {
    const bytes = packet instanceof Uint8Array ? packet : new Uint8Array(packet);
    if (bytes.byteLength < 48 || String.fromCharCode(...bytes.subarray(0, 4)) !== "R2WF") {
      throw new Error("invalid R2 feature packet");
    }
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const frames = view.getUint32(4, true);
    const bins = view.getUint32(8, true);
    const audioStartMs = view.getFloat64(12, true);
    const startMs = view.getFloat64(20, true);
    const endMs = view.getFloat64(28, true);
    const leadingMs = view.getFloat64(36, true);
    const lastChunk = view.getUint32(44, true) !== 0;
    if (frames < 1 || bins !== 1025 || bytes.byteLength !== 48 + frames * (1 + bins * 2) * 4) {
      throw new Error(`invalid R2 feature packet shape ${frames}x${bins} (${bytes.byteLength} bytes)`);
    }

    const source = new Float32Array(bytes.buffer, bytes.byteOffset + 48, frames * (1 + bins * 2));
    const padded = Math.ceil((frames + R2_LEFT_PAD) / 16) * 16;
    const f0 = new Float32Array(padded);
    const spEnv = new Float32Array(padded * bins);
    const ap = new Float32Array(padded * bins);
    ap.fill(1);
    const f0Src = source.subarray(0, frames);
    const spSrc = source.subarray(frames, frames + frames * bins);
    const apSrc = source.subarray(frames + frames * bins);
    f0.set(f0Src, R2_LEFT_PAD);
    for (let i = 0; i < frames; i++) {
      const dst = (i + R2_LEFT_PAD) * bins;
      const src = i * bins;
      spEnv.set(spSrc.subarray(src, src + bins), dst);
      ap.set(apSrc.subarray(src, src + bins), dst);
    }

    const tMel = performance.now();
    const mel = await this.runMel({ f0, spEnv, ap, frames: padded, bins });
    const melMs = performance.now() - tMel;
    const tVoc = performance.now();
    const waveform = await this.runVocoder({ mel: mel.data, f0, frames: padded });
    const vocoderMs = performance.now() - tVoc;
    const trimmed = new Float32Array(frames * R2_HOP);
    trimmed.set(waveform.data.subarray(R2_LEFT_PAD * R2_HOP, (R2_LEFT_PAD + frames) * R2_HOP));
    const ease = Math.min(512, trimmed.length);
    for (let i = 0; i < ease; i++) {
      trimmed[i] *= i / ease;
      trimmed[trimmed.length - ease + i] *= (ease - i) / ease;
    }

    const start = Math.max(0, Math.floor((startMs - leadingMs - audioStartMs) * 44.1));
    const planEnd = Math.max(0, Math.floor((endMs - audioStartMs) * 44.1));
    const end = Math.min(trimmed.length, lastChunk ? trimmed.length : planEnd);
    return {
      pcm: end > start ? trimmed.slice(start, end) : new Float32Array(0),
      frames, padded, melMs, vocoderMs,
    };
  }

  async dispose() {
    if (this.disposed) return;
    this.disposed = true;
    await Promise.all([this.mel.release(), this.vocoder.release()]);
  }
}
