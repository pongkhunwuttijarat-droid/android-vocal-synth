/**
 * Minimal ZIP reader for voicebank archives.
 *
 * A `.zip` is the other way a voicebank arrives — OpenUtau's own installer and
 * every DiffSinger release page ship one — so `Tools → Install Singer…` has to
 * take it next to the folder picker. The browser already ships the decompressor
 * (`DecompressionStream('deflate-raw')`), so a bank needs no zip library: this
 * reads the central directory and inflates stored/deflated entries.
 *
 * Deliberately not implemented, because a voicebank never uses them: encryption,
 * ZIP64, multi-disk archives, and any method other than store/deflate. Each is
 * reported as an error rather than silently skipped, so a bank that cannot be
 * read says so instead of installing as a husk with no aliases.
 *
 * Parsing is split from inflation so the structure reader stays synchronous and
 * unit-testable in Node (see `tests/zip.test.js`); only `readZipEntry` needs a
 * browser.
 */

const SIG_EOCD = 0x06054b50;
const SIG_CENTRAL = 0x02014b50;
const SIG_LOCAL = 0x04034b50;
const SIG_LOCAL_WRITE = SIG_LOCAL;
const SIG_CENTRAL_WRITE = SIG_CENTRAL;
const METHOD_STORE = 0;
const METHOD_DEFLATE = 8;
/** Guards a hostile archive from filling OPFS before the user sees a progress. */
export const MAX_ENTRY_BYTES = 512 * 1024 * 1024;
const MAX_TOTAL_BYTES = 4 * 1024 * 1024 * 1024;

let crcTable = null;

/** CRC-32 used by ZIP entries; exported for the small browser-side ZIP writer. */
export function zipCrc32(bytes) {
  if (!crcTable) {
    crcTable = new Uint32Array(256);
    for (let i = 0; i < 256; i += 1) {
      let value = i;
      for (let bit = 0; bit < 8; bit += 1) value = value & 1 ? 0xedb88320 ^ (value >>> 1) : value >>> 1;
      crcTable[i] = value >>> 0;
    }
  }
  let crc = 0xffffffff;
  for (const byte of bytes) crc = crcTable[(crc ^ byte) & 0xff] ^ (crc >>> 8);
  return (crc ^ 0xffffffff) >>> 0;
}

function u16(view, offset) { return view.getUint16(offset, true); }
function u32(view, offset) { return view.getUint32(offset, true); }

/**
 * Decode one entry name.
 *
 * A zip name is bytes, and which encoding they are in is genuinely ambiguous:
 * general-purpose bit 11 says "UTF-8", and when it is clear the spec requires
 * UTF-8. Japanese voicebanks are the case that breaks the usual shortcut — the
 * archives real banks ship in (`TETO-English-150401.zip`,
 * `TETO-OUset240323.zip`) set **no** flags at all and store Shift-JIS bytes, so
 * decoding as UTF-8 turns `重音テト...` into U+FFFD replacement characters. The
 * bank then installs under a mojibake directory and `oto.ini`'s aliases no
 * longer resolve to files that exist, which is why a real bank "imported" but
 * produced nothing.
 *
 * So: UTF-8 when the flag says so, and otherwise UTF-8 only if the bytes are
 * actually valid UTF-8. Anything else is Shift-JIS, which is what Japanese
 * Windows tools have written since 1995 and what OpenUtau's own installer reads
 * (`SHIFT_JIS` fallback in `ZipArchive.cs`). Decoding is best-effort because a
 * name that cannot be decoded still has to be reported rather than dropped.
 */
function decodeEntryName(bytes, utf8Flag) {
  const utf8 = () => new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  if (utf8Flag) {
    try { return utf8(); } catch { /* flagged but malformed: fall through */ }
  }
  try { return utf8(); } catch { /* not UTF-8 */ }
  for (const encoding of ['shift_jis', 'cp932']) {
    try { return new TextDecoder(encoding).decode(bytes); } catch { /* unsupported */ }
  }
  return new TextDecoder('utf-8').decode(bytes);
}

/**
 * The end-of-central-directory record sits at the tail, after an optional
 * comment of up to 64 KiB, so scan backwards over exactly that window.
 */
function findEndOfCentralDirectory(view) {
  const min = Math.max(0, view.byteLength - 0xffff - 22);
  for (let at = view.byteLength - 22; at >= min; at -= 1) {
    if (u32(view, at) !== SIG_EOCD) continue;
    const commentLength = u16(view, at + 20);
    if (at + 22 + commentLength === view.byteLength) return at;
  }
  return -1;
}

/**
 * Reject anything that could escape the bank root once written: absolute paths,
 * drive letters, `..` segments, and NUL. A zip is untrusted input even when it
 * arrives from the user's own downloads folder.
 */
export function safeZipPath(path) {
  const cleaned = path.replace(/\\/g, '/').replace(/^\/+/, '');
  if (!cleaned || cleaned.includes('\0')) return null;
  const parts = cleaned.split('/').filter(part => part && part !== '.');
  if (!parts.length || parts.some(part => part === '..')) return null;
  return parts.join('/');
}

/**
 * Read a zip's table of contents.
 *
 * Returns `{ entries }` where each entry is
 * `{ path, method, compressedSize, size, localOffset }`, in archive order.
 * Throws with a message meant for the install dialog when the archive is not a
 * readable zip at all.
 */
export function parseZip(buffer) {
  const bytes = buffer instanceof Uint8Array ? buffer : new Uint8Array(buffer);
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (view.byteLength < 22) throw new Error('that file is too small to be a zip archive');
  const eocd = findEndOfCentralDirectory(view);
  if (eocd < 0) throw new Error('that file is not a zip archive (no end-of-central-directory record)');
  const total = u16(view, eocd + 10);
  const directorySize = u32(view, eocd + 12);
  const directoryOffset = u32(view, eocd + 16);
  if (directoryOffset + directorySize > view.byteLength) {
    throw new Error('that zip archive is truncated or uses ZIP64, which is not supported');
  }
  const entries = [];
  let at = directoryOffset;
  for (let index = 0; index < total; index += 1) {
    if (at + 46 > view.byteLength || u32(view, at) !== SIG_CENTRAL) {
      throw new Error(`that zip archive's table of contents is damaged at entry ${index + 1}`);
    }
    const method = u16(view, at + 10);
    const flags = u16(view, at + 8);
    const crc = u32(view, at + 16);
    const compressedSize = u32(view, at + 20);
    const size = u32(view, at + 24);
    const nameLength = u16(view, at + 28);
    const extraLength = u16(view, at + 30);
    const commentLength = u16(view, at + 32);
    const localOffset = u32(view, at + 42);
    const rawName = decodeEntryName(bytes.subarray(at + 46, at + 46 + nameLength), (flags & 0x800) !== 0);
    at += 46 + nameLength + extraLength + commentLength;
    if (method !== METHOD_STORE && method !== METHOD_DEFLATE) {
      throw new Error(`that zip archive uses compression method ${method}, which is not supported`);
    }
    if (size > MAX_ENTRY_BYTES) {
      throw new Error(`that zip archive holds an entry larger than ${MAX_ENTRY_BYTES} bytes`);
    }
    const path = safeZipPath(rawName);
    // A directory entry, or a path that would escape the root, is dropped rather
    // than installed: the directory list is rebuilt from the files themselves.
    if (path === null || rawName.endsWith('/')) continue;
    entries.push({ path, method, crc, compressedSize, size, localOffset });
  }
  const totalBytes = entries.reduce((sum, entry) => sum + entry.size, 0);
  if (totalBytes > MAX_TOTAL_BYTES) {
    throw new Error(`that zip archive unpacks to more than ${MAX_TOTAL_BYTES} bytes`);
  }
  return { entries };
}

/**
 * The directory every entry shares, when they all share one.
 *
 * Voicebank zips come both ways round: some wrap the bank in a single folder,
 * others put `character.txt` and `voice/` at the top level. Peeling a common
 * wrapper is what lets the same install produce the same `voice/oto.ini` layout
 * either way. Returns `null` when there is no single shared directory.
 *
 * Sharing one top directory is not on its own enough: a bank that is *only*
 * `voice/…` shares `voice`, and peeling it would move `voice/oto.ini` to the
 * bank root and leave something that renders nothing. So the candidate must
 * also still look like a bank once it is removed.
 *
 * A real bank also breaks the "everything shares one top directory" rule
 * without meaning to: both `TETO-English-150401.zip` and `TETO-OUset240323.zip`
 * put an `install.txt` at the archive root **beside** the bank folder. Counting
 * distinct top-level names therefore gave 2, no wrapper was peeled, the bank
 * installed one level too deep, and `voice/oto.ini` was never where the loader
 * looked for it — the install "succeeded" and the singer still had no samples.
 * So stray top-level *files* are ignored when deciding; only directories count.
 */
export function commonTopDirectory(entries) {
  // Top-level names that are directories, i.e. some entry sits under them.
  const directories = new Set();
  for (const entry of entries) {
    const slash = entry.path.indexOf('/');
    if (slash > 0) directories.add(entry.path.slice(0, slash));
  }
  if (directories.size !== 1) return null;
  const [only] = [...directories];
  const inside = entries
    .filter(entry => entry.path.startsWith(`${only}/`))
    .map(entry => entry.path.slice(only.length + 1));
  if (!inside.length) return null;
  const stillABank = inside.some(path => path.startsWith('voice/') || path === 'character.txt');
  return stillABank ? only : null;
}

/**
 * Bank root for a zip: `character.txt`'s `name=` when the archive declares one,
 * else the shared wrapper directory, else the file's stem. OpenUtau lists a
 * singer by `character.txt`, so preferring it makes an imported bank match the
 * singer's name in a project that references it.
 */
export function zipBankRoot(fileName, entries, characterName = null) {
  const named = typeof characterName === 'string' ? characterName.trim() : '';
  if (named) return named;
  const wrapper = commonTopDirectory(entries);
  if (wrapper) return wrapper;
  return fileName.replace(/\.[^.]+$/, '') || 'rusted-utau-voicebank';
}

/**
 * Decode a text file from a voicebank.
 *
 * The same encoding question as the entry names, and for the same reason: the
 * real TETO archives store `character.txt` as Shift-JIS, so a UTF-8 decode
 * turned `name=重音テト（かさねてと）音声ライブラリー` into a bank root made of
 * replacement characters. Since that root is what the preview matches a project's
 * singer against, the bank installed and then could never be selected again.
 */
function decodeBankText(bytes) {
  try { return new TextDecoder('utf-8', { fatal: true }).decode(bytes); } catch { /* not UTF-8 */ }
  for (const encoding of ['shift_jis', 'cp932']) {
    try { return new TextDecoder(encoding).decode(bytes); } catch { /* unsupported */ }
  }
  return new TextDecoder('utf-8').decode(bytes);
}

/** `name=` out of a `character.txt`, or null when it is absent or unreadable. */
export function characterNameFrom(bytes) {
  let text;
  try { text = decodeBankText(bytes); } catch { return null; }
  const match = /^[ \t]*name[ \t]*=[ \t]*(.+)$/m.exec(text);
  return match ? match[1].trim() || null : null;
}

/**
 * Where a bank's `oto.ini` lives inside an installed bank, and which directory
 * its alias paths are relative to.
 *
 * The obvious answer — always `voice/` — is wrong for most real banks.
 * OpenUtau reads `oto.ini` from wherever the bank puts it and resolves each
 * alias against that same directory, so a bank that ships its samples in
 * `重音テト英語音源/` (as `TETO-English-150401.zip` does) or in a colour folder
 * like `voice/`+`colorA/` is normal, not malformed. Assuming `voice/` made a
 * real bank install and then read zero samples, because the loader asked for a
 * file the bank never had.
 *
 * Candidates come back shallowest first, but an EMPTY `oto.ini` is not a usable
 * one: `TETO-OUset240323.zip` carries a 0-byte `oto.ini` at the bank root next
 * to the real ones in per-colour folders, and picking the shallowest silently
 * produced a bank with zero aliases. So `sizes` (when given) lets a zero-byte
 * candidate be skipped in favour of one that actually lists samples.
 *
 * Returns `{ otoPath, otoDir }`, or null when the bank has no usable
 * `oto.ini` at all.
 */
export function findOtoDir(paths, sizes = null) {
  const sizeOf = path => (sizes instanceof Map ? sizes.get(path) : sizes?.[path]);
  const candidates = [];
  for (const path of paths) {
    if (!/(^|\/)oto\.ini$/i.test(path)) continue;
    if (sizes && sizeOf(path) === 0) continue;
    const slash = path.lastIndexOf('/');
    candidates.push({ otoPath: path, otoDir: slash < 0 ? '' : path.slice(0, slash) });
  }
  if (!candidates.length) return null;
  candidates.sort((a, b) => a.otoDir.length - b.otoDir.length);
  return candidates[0];
}

/** Decompress one entry. Store is a copy; deflate goes through the platform. */
export async function readZipEntry(entry, buffer) {
  const bytes = buffer instanceof Uint8Array ? buffer : new Uint8Array(buffer);
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const at = entry.localOffset;
  if (at + 30 > view.byteLength || u32(view, at) !== SIG_LOCAL) {
    throw new Error(`entry ${entry.path} has no local header; the archive is damaged`);
  }
  const nameLength = u16(view, at + 26);
  const extraLength = u16(view, at + 28);
  const start = at + 30 + nameLength + extraLength;
  const end = start + entry.compressedSize;
  if (end > view.byteLength) throw new Error(`entry ${entry.path} is truncated`);
  const raw = bytes.subarray(start, end);
  let output;
  if (entry.method === METHOD_STORE) output = raw.slice();
  else {
    if (typeof DecompressionStream !== 'function') {
      throw new Error('this browser cannot unzip a deflated voicebank');
    }
    const stream = new Blob([raw]).stream().pipeThrough(new DecompressionStream('deflate-raw'));
    output = new Uint8Array(await new Response(stream).arrayBuffer());
  }
  if (output.byteLength !== entry.size) {
    throw new Error(`entry ${entry.path} has an unexpected uncompressed size`);
  }
  if (entry.crc !== undefined && zipCrc32(output) !== entry.crc) {
    throw new Error(`entry ${entry.path} failed its CRC check`);
  }
  return output;
}

/**
 * Every entry of a zip as `{ path, bytes }`, ready for
 * `OpfsBank#importFiles`. `prefix` drops a wrapper directory once the caller
 * has decided which root the bank should land under.
 */
export async function readZipEntries(buffer, { prefix = '' } = {}) {
  const { entries } = parseZip(buffer);
  const out = [];
  for (const entry of entries) {
    let path = entry.path;
    if (prefix) {
      if (path === prefix) continue;
      if (!path.startsWith(`${prefix}/`)) continue;
      path = path.slice(prefix.length + 1);
    }
    out.push({ path, bytes: await readZipEntry(entry, buffer) });
  }
  return out;
}

/**
 * Write a standards-compliant ZIP using the uncompressed `store` method.
 * Project bundles are already media-heavy and need no CPU-heavy codec in the UI;
 * standard readers can still open the resulting archive.
 */
export function writeStoredZip(files) {
  if (!Array.isArray(files) || !files.length) throw new TypeError('a zip needs at least one file');
  if (files.length > 0xffff) throw new RangeError('a zip cannot contain more than 65535 files');
  const encoder = new TextEncoder();
  const paths = new Set();
  const locals = [];
  const centrals = [];
  let localOffset = 0;
  let totalBytes = 0;
  const dosDate = 0x0021; // 1980-01-01, the first representable DOS date.

  for (const file of files) {
    const path = String(file?.path || '');
    if (!path || safeZipPath(path) !== path || path.endsWith('/')) {
      throw new TypeError(`unsafe zip path: ${path}`);
    }
    if (paths.has(path)) throw new Error(`duplicate zip path: ${path}`);
    paths.add(path);
    const name = encoder.encode(path);
    if (!name.length || name.length > 0xffff) throw new RangeError(`zip path is too long: ${path}`);
    const source = file.bytes;
    const data = source instanceof Uint8Array
      ? source.subarray(0)
      : source instanceof ArrayBuffer
        ? new Uint8Array(source)
        : ArrayBuffer.isView(source)
          ? new Uint8Array(source.buffer, source.byteOffset, source.byteLength)
          : null;
    if (!data) throw new TypeError(`zip entry ${path} must provide bytes`);
    if (data.byteLength > MAX_ENTRY_BYTES) throw new RangeError(`zip entry ${path} is too large`);
    totalBytes += data.byteLength;
    if (totalBytes > MAX_TOTAL_BYTES) throw new RangeError('zip payload is too large');
    const crc = zipCrc32(data);
    const local = new Uint8Array(30 + name.length + data.length);
    const lv = new DataView(local.buffer);
    lv.setUint32(0, SIG_LOCAL_WRITE, true);
    lv.setUint16(4, 20, true);
    lv.setUint16(6, 0x0800, true); // UTF-8 path names.
    lv.setUint16(8, METHOD_STORE, true);
    lv.setUint16(10, 0, true);
    lv.setUint16(12, dosDate, true);
    lv.setUint32(14, crc, true);
    lv.setUint32(18, data.length, true);
    lv.setUint32(22, data.length, true);
    lv.setUint16(26, name.length, true);
    lv.setUint16(28, 0, true);
    local.set(name, 30);
    local.set(data, 30 + name.length);

    const central = new Uint8Array(46 + name.length);
    const cv = new DataView(central.buffer);
    cv.setUint32(0, SIG_CENTRAL_WRITE, true);
    cv.setUint16(4, 20, true);
    cv.setUint16(6, 20, true);
    cv.setUint16(8, 0x0800, true);
    cv.setUint16(10, METHOD_STORE, true);
    cv.setUint16(12, 0, true);
    cv.setUint16(14, dosDate, true);
    cv.setUint32(16, crc, true);
    cv.setUint32(20, data.length, true);
    cv.setUint32(24, data.length, true);
    cv.setUint16(28, name.length, true);
    cv.setUint16(30, 0, true);
    cv.setUint16(32, 0, true);
    cv.setUint16(34, 0, true);
    cv.setUint16(36, 0, true);
    cv.setUint32(38, 0, true);
    cv.setUint32(42, localOffset, true);
    central.set(name, 46);
    locals.push(local);
    centrals.push(central);
    localOffset += local.length;
  }

  const centralOffset = localOffset;
  const centralSize = centrals.reduce((sum, part) => sum + part.length, 0);
  if (centralOffset + centralSize > 0xffffffff) throw new RangeError('ZIP64 bundles are not supported');
  const end = new Uint8Array(22);
  const ev = new DataView(end.buffer);
  ev.setUint32(0, SIG_EOCD, true);
  ev.setUint16(4, 0, true);
  ev.setUint16(6, 0, true);
  ev.setUint16(8, files.length, true);
  ev.setUint16(10, files.length, true);
  ev.setUint32(12, centralSize, true);
  ev.setUint32(16, centralOffset, true);
  ev.setUint16(20, 0, true);
  const output = new Uint8Array(centralOffset + centralSize + end.length);
  let offset = 0;
  for (const part of locals) { output.set(part, offset); offset += part.length; }
  for (const part of centrals) { output.set(part, offset); offset += part.length; }
  output.set(end, offset);
  return output;
}
