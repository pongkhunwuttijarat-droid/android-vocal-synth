// Naming an imported USTX.
//
// A USTX states its own `name:`; the file name is only the fallback, which is
// what a user who downloaded `my-song.ustx` from somewhere else expects. The
// name carries no identity with it — the engine mints every ID on import
// (§6.11 `imported_ustx`), so a clash with the file it came from is impossible.

/**
 * The project name an imported USTX should carry.
 *
 * @param {string} fileName the picked file's name
 * @param {string} text     the file's contents
 * @returns {string} never empty
 */
export function importName(fileName, text) {
  const declared = /^name:[ \t]*["']?([^"'\n#]+)/m.exec(text || '')?.[1]?.trim();
  if (declared) return declared;
  return String(fileName || '').replace(/\.[^.]+$/, '').trim() || 'Imported';
}

/** Forward selected companion files across the UI lifecycle boundary unchanged. */
export function importUstxWithMedia(engine, name, ustx, mediaFiles = []) {
  return engine.importUstx(name, ustx, mediaFiles);
}

function mediaFileNameKey(value) {
  const basename = String(value || '').replace(/\\/g, '/').split('/').filter(Boolean).pop() || '';
  return basename.normalize('NFC').toLowerCase();
}

/** Match imported wave parts to companion audio files selected with the USTX. */
export function matchWaveMediaFiles(tracks, files = []) {
  const filesByName = new Map();
  for (const file of files) {
    const key = mediaFileNameKey(file?.name || file?.webkitRelativePath);
    if (!key) continue;
    const candidates = filesByName.get(key) || [];
    candidates.push(file);
    filesByName.set(key, candidates);
  }

  const matches = [];
  const unresolved = [];
  for (const track of Array.isArray(tracks) ? tracks : []) {
    for (const part of Array.isArray(track?.parts) ? track.parts : []) {
      if (part?.kind !== 'wave') continue;
      const alreadyLinked = String(part.media_ref || '').startsWith('asset:');
      const keys = [...new Set([alreadyLinked ? '' : part.media_ref, part.name].map(mediaFileNameKey).filter(Boolean))];
      let file = null;
      let ambiguous = false;
      for (const key of keys) {
        const candidates = filesByName.get(key) || [];
        if (candidates.length === 1) { file = candidates[0]; break; }
        if (candidates.length > 1) ambiguous = true;
      }
      if (file) matches.push({ part, file, trackName: track.name });
      else if (!alreadyLinked) unresolved.push({ part, reason: ambiguous ? 'ambiguous' : 'missing' });
    }
  }
  return { matches, unresolved };
}

/** Register matched companion audio, assign Rust asset refs, and count missing links. */
export async function importWaveMediaFiles(tracks, mediaFiles = [], { registerMedia, apply } = {}) {
  const wavePartCount = (Array.isArray(tracks) ? tracks : [])
    .reduce((count, track) => count + (Array.isArray(track?.parts) ? track.parts.filter(part => part?.kind === 'wave').length : 0), 0);
  const { matches, unresolved } = matchWaveMediaFiles(tracks, mediaFiles);
  const remaining = [...unresolved];
  const linkErrors = [];
  const mediaByFile = new Map();
  let linkedMedia = 0;

  for (const { part, file } of matches) {
    if (typeof registerMedia !== 'function' || typeof apply !== 'function') {
      remaining.push({ part, reason: 'unsupported' });
      continue;
    }
    try {
      let media = mediaByFile.get(file);
      if (!media) {
        const bytes = file.bytes
          ? (file.bytes instanceof Uint8Array ? file.bytes : new Uint8Array(file.bytes))
          : new Uint8Array(await file.arrayBuffer());
        const mediaId = globalThis.crypto?.randomUUID?.() || `${Date.now()}-${Math.random().toString(16).slice(2)}`;
        media = await registerMedia(mediaId, bytes);
        mediaByFile.set(file, media);
      }
      await apply({ type: 'part.set_media_ref', part_id: part.id, media_ref: media.media_ref });
      linkedMedia++;
    } catch (error) {
      remaining.push({ part, reason: 'register-failed' });
      linkErrors.push(error?.message || String(error));
    }
  }

  return {
    wavePartCount,
    linkedMedia,
    unresolvedMedia: remaining.length,
    mediaError: linkErrors[0] || null,
  };
}
