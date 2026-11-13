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

/**
 * Decode a voicebank text file.
 *
 * `oto.ini` and `character.txt` from a Japanese bank are Shift-JIS, the same as
 * the archive's entry names — see `web/zip.js` for why. Reading them as UTF-8
 * turned every alias into replacement characters, so no key matched a file that
 * was really on disk and the bank rendered silence while looking installed.
 */
function decodeBankText(bytes) {
  try { return new TextDecoder('utf-8', { fatal: true }).decode(bytes); } catch { /* not UTF-8 */ }
  for (const encoding of ['shift_jis', 'cp932']) {
    try { return new TextDecoder(encoding).decode(bytes); } catch { /* unsupported */ }
  }
  return new TextDecoder('utf-8').decode(bytes);
}

/** Derive the wav file names listed in an `oto.ini`. */
export function keysFromOto(otoBytes, otoDir = "") {
  const text = decodeBankText(otoBytes);
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
   * Every `oto.ini` this bank could use, shallowest first.
   *
   * `voice/oto.ini` is a convention, not a rule — OpenUtau reads `oto.ini` from
   * wherever a bank puts it — so a host that cannot assume the path asks the
   * bank. The base implementation has no directory listing, so it reports the
   * conventional path only and lets the caller decide.
   */
  async listOtoCandidates() {
    return [{ otoPath: "voice/oto.ini", otoDir: "voice" }];
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

  /**
   * Every `oto.ini` under this root, shallowest first.
   *
   * A real bank does not have to keep its samples in `voice/`: OpenUtau banks
   * routinely use a named folder, and `TETO-English-150401.zip` uses
   * `重音テト英語音源/`. Assuming `voice/` made such a bank import and then read
   * zero samples, because the loader asked for a file the bank never had.
   */
  async listOtoCandidates() {
    const found = [];
    const walk = async (dir, prefix) => {
      for await (const [name, handle] of dir.entries()) {
        if (handle.kind === "directory") { await walk(handle, `${prefix}${name}/`); continue; }
        if (name.toLowerCase() !== "oto.ini") continue;
        // A 0-byte `oto.ini` is not a usable one: `TETO-OUset240323.zip` ships
        // an empty one at the bank root beside the real per-colour ones, and
        // preferring it yields a bank with no aliases at all.
        let size = 0;
        try { size = (await handle.getFile()).size; } catch { /* unreadable: skip */ }
        if (size === 0) continue;
        found.push({ otoPath: `${prefix}${name}`, otoDir: prefix.replace(/\/$/, "") });
      }
    };
    let dir;
    try { dir = await navigator.storage.getDirectory(); } catch { return []; }
    for (const part of this.root.split("/").filter(Boolean)) {
      try { dir = await dir.getDirectoryHandle(part); } catch { return []; }
    }
    await walk(dir, "");
    found.sort((a, b) => a.otoDir.length - b.otoDir.length);
    return found;
  }

  /**
   * Open all non-empty oto sets in a bank as one in-memory table for hosts
   * whose engine API accepts one oto.ini. Preserve each set's directory in the
   * sample path, so aliases can still load their original WAV/FRQ files.
   */
  async openAllOtos() {
    const candidates = await this.listOtoCandidates();
    const lines = [];
    const aliases = new Set();
    const sampleKeys = new Set();
    for (const candidate of candidates) {
      const bytes = await this.raw(candidate.otoPath);
      const text = decodeBankText(new Uint8Array(bytes));
      for (const line of text.split(/\r?\n/)) {
        const eq = line.indexOf('=');
        if (eq <= 0) continue;
        const wav = line.slice(0, eq).trim().replaceAll('\\', '/');
        const fields = line.slice(eq + 1);
        const alias = fields.split(',', 1)[0].trim();
        if (!wav || !alias || wav.startsWith('/') || wav.split('/').some(part => part === '..' || part === '.')) continue;
        const key = [candidate.otoDir, wav].filter(Boolean).join('/');
        aliases.add(alias);
        sampleKeys.add(key);
        lines.push(`${key}=${fields}`);
      }
    }
    if (!aliases.size) throw new Error(`Voicebank “${this.root}” has no usable oto aliases`);
    this.otoBytes = new TextEncoder().encode(lines.join('\n'));
    this.otoDir = '';
    this.keys = [...sampleKeys].sort();
    return { otoSets: candidates.length, aliasCount: aliases.size, sampleCount: sampleKeys.size };
  }

  /**
   * Import into OPFS under this bank's root. Accepts either a
   * directory-picker `FileList` (Chrome / Firefox) or a list of
   * `{ path, bytes }` entries (tests, zip unpacking).
   */
  async importFiles(files, { stripTopDir = true } = {}) {
    let count = 0;
    for (const f of files) {
      let rel;
      let bytes;
      if (f && typeof f.path === "string" && f.bytes) {
        rel = f.path;
        bytes = f.bytes;
      } else {
        rel = f.webkitRelativePath || f.name;
        if (stripTopDir) {
          const slash = rel.indexOf("/");
          if (slash >= 0) rel = rel.slice(slash + 1);
        }
        bytes = new Uint8Array(await f.arrayBuffer());
      }
      await this.write(rel, bytes);
      count += 1;
    }
    this.cache = new ByteCache(this.cache.maxBytes);
    return count;
  }

  /** Top-level OPFS directories that contain at least one usable oto.ini. */
  static async installedRoots() {
    if (!(await this.available())) return [];
    const roots = [];
    const dir = await navigator.storage.getDirectory();
    const hasUsableOto = async folder => {
      for await (const [name, entry] of folder.entries()) {
        if (entry.kind === "directory") {
          if (await hasUsableOto(entry)) return true;
        } else if (name.toLowerCase() === "oto.ini") {
          try { if ((await entry.getFile()).size > 0) return true; } catch { /* unreadable */ }
        }
      }
      return false;
    };
    for await (const [name, handle] of dir.entries()) {
      if (handle.kind !== "directory") continue;
      try {
        if (await hasUsableOto(handle)) roots.push(name);
      } catch {
        /* not a readable bank directory */
      }
    }
    return roots.sort();
  }

  /** Remove an imported bank root (recursive). */
  static async removeRoot(root) {
    const dir = await navigator.storage.getDirectory();
    await dir.removeEntry(String(root), { recursive: true });
  }
}
