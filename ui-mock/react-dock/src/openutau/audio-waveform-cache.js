/** Stable identity for Arrangement wave previews, excluding unrelated project edits. */
export function audioWaveformCacheKey(sessionId, tracks) {
  const audioTracks = (tracks || [])
    .map((track, trackIndex) => ({ track, trackIndex }))
    .filter(({ track }) => (track.parts || []).some(part => part.kind === 'wave'));
  return JSON.stringify([
    sessionId || '',
    audioTracks.map(({ track, trackIndex }) => [trackIndex, track]),
  ]);
}
