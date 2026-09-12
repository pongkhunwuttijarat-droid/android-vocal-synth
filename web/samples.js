// Voicebank sample sources.
//
// A voicebank is a set of `<name>.wav` files plus an optional `<name>_wav.frq`
// companion per sample (UTAU convention). The render engine needs the wav
// bytes, and needs the frq bytes too for pitch accuracy — the reference
// renderer always prefers the `.frq` when the bank ships one, and without it
// the rendered audio differs measurably from the native build.
//
// Two sources implement the same interface:
//   HttpBank — fetch from a served directory (the harness / demo pages)
//   OpfsBank — read from the Origin Private File System (the real app)
//
// Both cache decoded bytes in memory, because a voicebank is read many times
// (once per worker, then again whenever an edited chunk needs a new sample)
// and re-fetching is the dominant cost — see the pool measurements in
// docs/architecture/wasm-parallel.md.

/** `voice/a.wav` → `voice/a_wav.frq` (OpenUtau's naming). */
export function frqKeyFor(wavKey) {
  return wavKey.replace(/\.wav$/i, "_wav.frq");
}

/** Derive the wav file names listed in an `oto.ini`. */
export function keysFromOto(otoBytes, otoDir = "") {
  const text = new TextDecoder().decode(otoBytes);
  const names = new Set();
  for (const line of text.split(/\r?\n/)) {
    const eq = line.indexOf("=");
    if (eq <= 0) continue;
    const wav = line.slice(0, eq).trim();
    if (wav) names.add(wav);
  }
  const prefix = otoDir ? `${otoDir}/` : "";
  return [...names].sort().map((n) => prefix + n);
}

/** Shared cache + byte-budget bookkeeping. */
class ByteCache {
  constructor(maxBytes = Infinity) {
    this.maxBytes = maxBytes;
    this.bytes = 0;
    this.map = new Map(); // key -> { value, bytes }
  }
  get(key) {
    const hit = this.map.get(key);
    if (hit) {
      // Refresh recency (Map preserves insertion order).
      this.map.delete(key);
      this.map.set(key, hit);
      return hit.value;
    }
    return undefined;
  }
  has(key) {
    return this.map.has(key);
  }
  set(key, value, bytes) {
    if (this.map.has(key)) this.bytes -= this.map.get(key).bytes;
    this.map.set(key, { value, bytes });
    this.bytes += bytes;
    this.evict();
  }
  evict() {
    if (this.maxBytes === Infinity) return;
    for (const [key, hit] of this.map) {
      if (this.bytes <= this.maxBytes) break;
      this.map.delete(key);
      this.bytes -= hit.bytes;
    }
  }
  get size() {
    return this.map.size;
  }
}

/** Base class: implements `load(keys)` on top of a `raw(key)` primitive. */
class BaseBank {
  constructor({ maxBytes = Infinity } = {}) {
    /** @type {ByteCache} */
    this.cache = new ByteCache(maxBytes);
    this.otoBytes = null;
    this.otoDir = "";
    this.keys = [];
  }

  /** Subclasses: fetch the bytes for `key`, or throw. */
  async raw(_key) {
    throw new Error("raw() not implemented");
  }

  /** Load `oto.ini` and derive the sample key list. */
  async openOto({ otoPath, otoDir = "" } = {}) {
    const bytes = await this.raw(otoPath);
    this.otoBytes = bytes;
    this.otoDir = otoDir;
    this.keys = keysFromOto(bytes, otoDir);
    return this.keys;
  }

  /**
   * Load the given keys (or all keys of the bank when omitted).
   * Returns `[{ key, wav: ArrayBuffer, frq: ArrayBuffer|null }]`.
   *
   * Already-cached entries are returned without re-fetching; a missing `.frq`
   * is remembered so it is not refetched on every call.
   */
  async load(keys = this.keys) {
    const out = [];
    await Promise.all(
      keys.map(async (key) => {
        let entry = this.cache.get(key);
        if (!entry) {
          const wav = await this.raw(key);
          let frq = null;
          try {
            frq = await this.raw(frqKeyFor(key));
          } catch {
            frq = null; // bank has no frq for this sample
          }
          entry = { wav, frq };
          this.cache.set(key, entry, wav.byteLength + (frq ? frq.byteLength : 0));
        }
        out.push({ key, wav: entry.wav, frq: entry.frq });
      }),
    );
    // Keep the caller's order stable regardless of fetch completion order.
    const order = new Map(keys.map((k, i) => [k, i]));
    out.sort((a, b) => order.get(a.key) - order.get(b.key));
    return out;
  }

  /** Bytes currently held in memory. */
  get cachedBytes() {
    return this.cache.bytes;
  }
}

/** Fetch samples from a served directory (harness / demo pages). */
export class HttpBank extends BaseBank {
  constructor({ baseUrl, maxBytes, concurrency = 16 } = {}) {
    super({ maxBytes });
    this.baseUrl = baseUrl.replace(/\/$/, "");
    this.concurrency = concurrency;
  }

  async raw(key) {
    const res = await fetch(`${this.baseUrl}/${key}`);
    if (!res.ok) throw new Error(`fetch ${key}: HTTP ${res.status}`);
    return res.arrayBuffer();
  }

  /** Warm many keys with bounded concurrency (fetching 1000+ files at once
   *  upsets both the browser and a naive server). */
  async prefetch(keys = this.keys) {
    const q = [...keys];
    const worker = async () => {
      while (q.length) {
        const key = q.shift();
        try {
          await this.load([key]);
        } catch {
          /* skip missing */
        }
      }
    };
    await Promise.all(Array.from({ length: Math.min(this.concurrency, q.length || 1) }, worker));
  }
}

/**
 * Read samples from the Origin Private File System (the app's storage).
 *
 * `<root>/<key>` is the path convention, so `voice/a.wav` lives at
 * `<root>/voice/a.wav` — the same layout the HTTP bank serves.
 */
export class OpfsBank extends BaseBank {
  constructor({ root = "voicebank", maxBytes } = {}) {
    super({ maxBytes });
    this.root = root;
  }

  static async available() {
    return typeof navigator !== "undefined" && !!navigator.storage?.getDirectory;
  }

  async raw(key) {
    const parts = `${this.root}/${key}`.split("/").filter(Boolean);
    let dir = await navigator.storage.getDirectory();
    for (const part of parts.slice(0, -1)) {
      dir = await dir.getDirectoryHandle(part);
    }
    const handle = await dir.getFileHandle(parts[parts.length - 1]);
    const file = await handle.getFile();
    return file.arrayBuffer();
  }

  /** Store one file (used by the import path). */
  async write(key, bytes) {
    const parts = `${this.root}/${key}`.split("/").filter(Boolean);
    let dir = await navigator.storage.getDirectory();
    for (const part of parts.slice(0, -1)) {
      dir = await dir.getDirectoryHandle(part, { create: true });
    }
    const handle = await dir.getFileHandle(parts[parts.length - 1], { create: true });
    const writable = await handle.createWritable();
    await writable.write(bytes);
    await writable.close();
  }

  /** Import a directory-picker `FileList` into OPFS under this bank's root. */
  async importFiles(files, { stripTopDir = true } = {}) {
    let count = 0;
    for (const f of files) {
      let rel = f.webkitRelativePath || f.name;
      if (stripTopDir) {
        const slash = rel.indexOf("/");
        if (slash >= 0) rel = rel.slice(slash + 1);
      }
      await this.write(rel, new Uint8Array(await f.arrayBuffer()));
      count += 1;
    }
    this.cache = new ByteCache(this.cache.maxBytes);
    return count;
  }
}
