import { TICKS_PER_BEAT, TICKS_PER_MEASURE } from './mock-data.js';

/** End tick of the current project, with four measures as the empty-project floor. */
export function projectTicks(tracks) {
  const parts = (tracks || []).flatMap(track => track.parts || []);
  return parts.reduce((max, part) => Math.max(max, part.position_ticks + part.duration_ticks), TICKS_PER_MEASURE * 4);
}

/** Every note onset in the document, ascending. Used by skip-to-note. */
export function noteOnsets(notes) {
  const ticks = [];
  for (const list of Object.values(notes || {})) {
    for (const note of list || []) ticks.push(note.position_ticks);
  }
  return [...new Set(ticks)].sort((a, b) => a - b);
}

/** Strictly next/previous boundary, clamped to the project timeline. */
export function skipTarget(from, total, step, direction) {
  if (!(step > 0)) return Math.max(0, Math.min(total, from));
  if (direction > 0) {
    const next = Math.ceil(from / step + 1e-9) * step;
    return Math.min(total, next);
  }
  const previous = Math.floor(from / step + 1e-9) * step;
  return Math.max(0, previous === from && from > 0 ? previous - step : previous);
}

/** Skip to the previous/next note onset, not snapping to one. */
export function skipNoteTarget(from, total, onsets, direction) {
  if (!onsets.length) return Math.max(0, Math.min(total, from));
  if (direction > 0) {
    const next = onsets.find(tick => tick > from + 1e-9);
    return next === undefined ? Math.min(total, total) : Math.min(total, next);
  }
  const earlier = [...onsets].reverse().find(tick => tick < from - 1e-9);
  return earlier === undefined ? 0 : earlier;
}

/**
 * Resolve a transport intent against the Rust document when connected. The UI
 * reducer's `state.tracks` is mock/presentation state, not a second project
 * document; pass the live snapshot here and carry its resolved bounds/target in
 * the intent so the reducer cannot clamp a long song to the mock timeline.
 */
export function transportTarget(state, intent, project = state) {
  const tracks = project?.tracks ?? state.tracks ?? [];
  const notes = project?.notes ?? state.notes ?? {};
  const total = Number.isFinite(intent.totalTicks)
    ? Math.max(0, intent.totalTicks)
    : projectTicks(tracks);
  const clamp = tick => Math.max(0, Math.min(total, tick));

  if (Number.isFinite(intent.targetTicks)) return clamp(intent.targetTicks);
  switch (intent.type) {
    case 'transport/restart': return 0;
    case 'transport/seek': return clamp(intent.ticks);
    case 'transport/skip-measure': return skipTarget(state.playheadTicks, total, TICKS_PER_MEASURE, intent.direction ?? 1);
    case 'transport/skip-note': return skipNoteTarget(state.playheadTicks, total, noteOnsets(notes), intent.direction ?? 1);
    default: return state.playheadTicks;
  }
}
