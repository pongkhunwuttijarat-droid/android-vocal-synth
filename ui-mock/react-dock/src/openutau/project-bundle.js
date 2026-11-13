import { parseZip, readZipEntry, writeStoredZip } from '../../../../web/zip.js';

export const PROJECT_BUNDLE_FORMAT = 'rusted-utau-project';
export const PROJECT_BUNDLE_VERSION = 1;
export const PROJECT_BUNDLE_EXTENSION = '.rustx';

export const MAX_PROJECT_BUNDLE_BYTES = 256 * 1024 * 1024;
const MAX_BUNDLE_ENTRIES = 8194;
const MAX_MANIFEST_BYTES = 1024 * 1024;
const MAX_PROJECT_BYTES = 128 * 1024 * 1024;
const utf8 = new TextEncoder();
const text = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true });

function asBytes(value, label = 'media') {
  if (value instanceof Uint8Array) return value.subarray(0);
  if (value instanceof ArrayBuffer) return new Uint8Array(value);
  if (ArrayBuffer.isView(value)) return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
  throw new TypeError(`${label} must be bytes`);
}

function safeMediaId(value) {
  const id = String(value ?? '');
  if (!id || id === '.' || id === '..' || !/^[\p{L}\p{N}_.-]+$/u.test(id)) return null;
  return id;
}

export function mediaIdFromRef(value) {
  const ref = String(value ?? '');
  return ref.startsWith('asset:') ? safeMediaId(ref.slice('asset:'.length)) : null;
}

function mediaName(value, fallback) {
  const name = String(value ?? '').replace(/\\/g, '/').split('/').filter(Boolean).pop() || fallback;
  const safe = name.normalize('NFC').replace(/[\u0000-\u001f<>:"/\\|?*]/g, '_').trim();
  return safe && safe !== '.' && safe !== '..' ? safe.slice(0, 240) : fallback;
}

function mediaPath(id, name) {
  return `media/${id}/${mediaName(name, id)}`;
}

function contentTypeForName(name) {
  const extension = String(name || '').split('.').pop()?.toLowerCase();
  return ({
    wav: 'audio/wav', mp3: 'audio/mpeg', ogg: 'audio/ogg', flac: 'audio/flac',
    m4a: 'audio/mp4', aac: 'audio/aac', opus: 'audio/opus',
  })[extension] || 'application/octet-stream';
}

function waveParts(tracks) {
  return (Array.isArray(tracks) ? tracks : []).flatMap(track =>
    (Array.isArray(track?.parts) ? track.parts : [])
      .filter(part => part?.kind === 'wave')
      .map(part => ({ ...part, trackName: track?.name || '' })),
  );
}

/**
 * Package portable USTX text and every media asset available in host storage.
 * USTX remains the canonical document; this archive only makes its audio refs
 * portable. Parts whose bytes are unavailable remain visibly unresolved.
 */
export async function createProjectBundle({ name, ustx, tracks, loadMedia }) {
  if (typeof ustx !== 'string' || !ustx.trim()) throw new TypeError('project USTX is empty');
  const projectBytes = utf8.encode(ustx);
  if (projectBytes.byteLength > MAX_PROJECT_BYTES) throw new RangeError('project USTX is too large for a portable bundle');

  const mediaById = new Map();
  const unresolvedNames = [];
  for (const part of waveParts(tracks)) {
    const id = mediaIdFromRef(part.media_ref);
    if (!id) {
      unresolvedNames.push(part.name || part.trackName || 'Audio part');
      continue;
    }
    const existing = mediaById.get(id);
    if (existing) continue;
    mediaById.set(id, {
      id,
      name: mediaName(part.name, id),
      partName: part.name || part.trackName || 'Audio part',
    });
  }

  const included = [];
  for (const asset of mediaById.values()) {
    if (typeof loadMedia !== 'function') throw new Error('The host does not expose stored audio for portable export');
    try {
      const bytes = asBytes(await loadMedia(asset.id), `audio asset ${asset.id}`);
      if (bytes.byteLength > MAX_PROJECT_BUNDLE_BYTES) throw new RangeError(`audio asset ${asset.id} is too large for a portable bundle`);
      included.push({ ...asset, bytes });
    } catch (error) {
      if (error?.name !== 'NotFoundError') throw error;
      unresolvedNames.push(asset.partName);
    }
  }

  const manifest = {
    format: PROJECT_BUNDLE_FORMAT,
    version: PROJECT_BUNDLE_VERSION,
    project: { name: String(name || 'Untitled').trim().slice(0, 160) || 'Untitled', file: 'project.ustx' },
    media: included.map(asset => ({
      id: asset.id,
      path: mediaPath(asset.id, asset.name),
      name: asset.name,
      contentType: contentTypeForName(asset.name),
      size: asset.bytes.byteLength,
    })),
    unresolvedMedia: unresolvedNames.length,
  };
  const entries = [
    { path: 'manifest.json', bytes: utf8.encode(JSON.stringify(manifest, null, 2)) },
    { path: 'project.ustx', bytes: projectBytes },
    ...included.map(asset => ({ path: mediaPath(asset.id, asset.name), bytes: asset.bytes })),
  ];
  const payloadBytes = entries.reduce((total, entry) => total + entry.bytes.byteLength, 0);
  if (payloadBytes + entries.length * 160 > MAX_PROJECT_BUNDLE_BYTES) {
    throw new RangeError('project bundle is larger than 256 MiB');
  }
  const bytes = writeStoredZip(entries);
  if (bytes.byteLength > MAX_PROJECT_BUNDLE_BYTES) throw new RangeError('project bundle is larger than 256 MiB');
  return { bytes, mediaCount: included.length, unresolvedMedia: unresolvedNames.length };
}

/** Parse and validate a `.rustx` ZIP without extracting paths to disk. */
export async function readProjectBundle(input) {
  const archive = asBytes(input, 'project bundle');
  if (archive.byteLength > MAX_PROJECT_BUNDLE_BYTES) throw new RangeError('project bundle is larger than 256 MiB');
  const { entries } = parseZip(archive);
  if (entries.length > MAX_BUNDLE_ENTRIES) throw new RangeError('project bundle has too many files');
  const expandedBytes = entries.reduce((total, entry) => total + entry.size, 0);
  if (expandedBytes > MAX_PROJECT_BUNDLE_BYTES) throw new RangeError('project bundle expands beyond 256 MiB');
  const byPath = new Map();
  for (const entry of entries) {
    if (byPath.has(entry.path)) throw new Error(`project bundle contains duplicate path: ${entry.path}`);
    byPath.set(entry.path, entry);
  }
  const manifestEntry = byPath.get('manifest.json');
  const projectEntry = byPath.get('project.ustx');
  if (!manifestEntry || !projectEntry) throw new Error('project bundle needs manifest.json and project.ustx');
  if (manifestEntry.size > MAX_MANIFEST_BYTES) throw new RangeError('project bundle manifest is too large');
  if (projectEntry.size > MAX_PROJECT_BYTES) throw new RangeError('project USTX is too large');

  let manifest;
  try {
    const manifestText = text.decode(await readZipEntry(manifestEntry, archive)).replace(/^\uFEFF/, '');
    manifest = JSON.parse(manifestText);
  }
  catch (error) { throw new Error(`project bundle manifest is invalid: ${error.message}`); }
  if (manifest?.format !== PROJECT_BUNDLE_FORMAT || manifest?.version !== PROJECT_BUNDLE_VERSION) {
    throw new Error('unsupported project bundle format or version');
  }
  if (manifest.project?.file !== 'project.ustx' || typeof manifest.project?.name !== 'string') {
    throw new Error('project bundle manifest has an invalid project entry');
  }
  if (!Array.isArray(manifest.media) || manifest.media.length > MAX_BUNDLE_ENTRIES - 2) {
    throw new Error('project bundle manifest has an invalid media list');
  }
  const projectName = manifest.project.name.trim().slice(0, 160) || 'Imported';
  const allowedPaths = new Set(['manifest.json', 'project.ustx']);
  const ids = new Set();
  const media = [];
  for (const item of manifest.media) {
    const id = safeMediaId(item?.id);
    if (!id || ids.has(id)) throw new Error('project bundle contains an invalid or duplicate media id');
    ids.add(id);
    const name = mediaName(item.name, id);
    const path = mediaPath(id, name);
    if (item.path !== path || allowedPaths.has(path)) throw new Error(`project bundle media path is invalid for ${id}`);
    if (!Number.isSafeInteger(item.size) || item.size < 0 || item.size > MAX_PROJECT_BUNDLE_BYTES) {
      throw new Error(`project bundle media size is invalid for ${id}`);
    }
    if (typeof item.contentType !== 'string' || item.contentType.length > 128) {
      throw new Error(`project bundle media type is invalid for ${id}`);
    }
    const entry = byPath.get(path);
    if (!entry || entry.size !== item.size) throw new Error(`project bundle media is missing or has the wrong size: ${name}`);
    allowedPaths.add(path);
    media.push({ id, name, contentType: item.contentType, bytes: await readZipEntry(entry, archive) });
  }
  for (const path of byPath.keys()) {
    if (!allowedPaths.has(path)) throw new Error(`project bundle contains an unexpected file: ${path}`);
  }
  const ustx = text.decode(await readZipEntry(projectEntry, archive));
  if (!ustx.trim()) throw new Error('project bundle contains an empty USTX');
  const unresolvedMedia = Number.isSafeInteger(manifest.unresolvedMedia) && manifest.unresolvedMedia >= 0
    ? manifest.unresolvedMedia : 0;
  return { name: projectName, ustx, media, unresolvedMedia };
}

function mediaNameKey(value) {
  return String(value ?? '').replace(/\\/g, '/').split('/').filter(Boolean).pop()?.normalize('NFC').toLowerCase() || '';
}

function missingRef(part) {
  const raw = String(part?.id || part?.name || 'wave-part');
  let key = raw.normalize('NFC').replace(/[^\p{L}\p{N}_.-]/gu, '_').replace(/^\.+|\.+$/g, '');
  if (!key || key === '.' || key === '..') key = 'wave-part';
  return `missing/${key.slice(0, 160)}`;
}

/** Restore bundle media by stable asset id, then by unambiguous file name. */
export async function restoreProjectBundleMedia(tracks, mediaAssets = [], { registerMedia, apply } = {}) {
  if (typeof registerMedia !== 'function' || typeof apply !== 'function') {
    throw new TypeError('bundle media restore requires registerMedia and apply');
  }
  const parts = waveParts(tracks);
  const assetsById = new Map();
  const assetsByName = new Map();
  for (const asset of Array.isArray(mediaAssets) ? mediaAssets : []) {
    const id = safeMediaId(asset?.id);
    if (!id || assetsById.has(id)) throw new Error('project bundle contains an invalid or duplicate media id');
    const value = { ...asset, id, bytes: asBytes(asset.bytes, `audio asset ${id}`) };
    assetsById.set(id, value);
    const key = mediaNameKey(value.name);
    if (key) assetsByName.set(key, [...(assetsByName.get(key) || []), value]);
  }

  const registrations = new Map();
  const linkErrors = [];
  let linkedMedia = 0;
  let unresolvedMedia = 0;
  const register = asset => {
    if (!registrations.has(asset.id)) {
      registrations.set(asset.id, Promise.resolve().then(() => registerMedia(asset.id, asset.bytes)));
    }
    return registrations.get(asset.id);
  };
  const applyRef = async (part, mediaRef) => {
    await apply({ type: 'part.set_media_ref', part_id: part.id, media_ref: mediaRef });
  };

  for (const part of parts) {
    const currentRef = String(part.media_ref || '');
    const currentId = mediaIdFromRef(currentRef);
    let asset = currentId ? assetsById.get(currentId) : null;
    if (!asset) {
      const keys = [...new Set([mediaNameKey(currentRef), mediaNameKey(part.name)].filter(Boolean))];
      for (const key of keys) {
        const candidates = assetsByName.get(key) || [];
        if (candidates.length === 1) { [asset] = candidates; break; }
      }
    }

    if (!asset) {
      unresolvedMedia++;
      if (currentRef.startsWith('asset:') || !currentRef.startsWith('missing/')) {
        await applyRef(part, missingRef(part));
      }
      continue;
    }

    try {
      const registered = await register(asset);
      const mediaRef = registered?.media_ref || `asset:${asset.id}`;
      if (currentRef !== mediaRef) await applyRef(part, mediaRef);
      linkedMedia++;
    } catch (error) {
      unresolvedMedia++;
      linkErrors.push(error?.message || String(error));
      await applyRef(part, missingRef(part));
    }
  }

  return {
    wavePartCount: parts.length,
    linkedMedia,
    unresolvedMedia,
    mediaError: linkErrors[0] || null,
  };
}
