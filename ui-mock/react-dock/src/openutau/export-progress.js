const PHASE_LABELS = Object.freeze({
  engine: 'engine working',
  encoding: 'preparing WAV',
  download: 'starting browser download',
});

export function formatExportElapsed(milliseconds) {
  const elapsedSeconds = Number.isFinite(milliseconds)
    ? Math.max(0, Math.floor(milliseconds / 1000))
    : 0;
  const minutes = String(Math.floor(elapsedSeconds / 60)).padStart(2, '0');
  const seconds = String(elapsedSeconds % 60).padStart(2, '0');
  return `${minutes}:${seconds}`;
}

/** Display elapsed time and the current known phase; never imply a render percent. */
export function formatExportProgress({ label, phase, elapsedMs }) {
  const phaseLabel = PHASE_LABELS[phase] ?? 'working';
  return `${label} · ${phaseLabel} · ${formatExportElapsed(elapsedMs)} elapsed`;
}

export async function runExportTask({
  label,
  action,
  onStatus,
  successMessage,
  failureMessage,
  startedAt: initialStartedAt,
  now = () => performance.now(),
}) {
  const startedAt = Number.isFinite(initialStartedAt) ? initialStartedAt : now();
  const publish = (phase, details = {}) => onStatus?.({ label, phase, startedAt, ...details });
  publish('engine');
  try {
    const result = await action(phase => publish(phase));
    const finishedAt = now();
    publish('complete', {
      finishedAt,
      kind: 'info',
      message: successMessage(result, finishedAt - startedAt),
    });
    return result;
  } catch (error) {
    const finishedAt = now();
    publish('failed', {
      finishedAt,
      kind: 'error',
      message: failureMessage(error, finishedAt - startedAt),
    });
    throw error;
  }
}
