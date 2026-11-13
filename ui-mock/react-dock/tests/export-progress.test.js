import assert from 'node:assert/strict';
import test from 'node:test';
import { formatExportElapsed, formatExportProgress, runExportTask } from '../src/openutau/export-progress.js';

test('export progress reports the known phase and elapsed time without a fake percentage', () => {
  assert.equal(
    formatExportProgress({ label: 'Mixdown export', phase: 'engine', elapsedMs: 125_000 }),
    'Mixdown export · engine working · 02:05 elapsed',
  );
  assert.equal(
    formatExportProgress({ label: 'Mixdown export', phase: 'encoding', elapsedMs: 300_000 }),
    'Mixdown export · preparing WAV · 05:00 elapsed',
  );
  assert.equal(
    formatExportProgress({ label: 'Mixdown export', phase: 'download', elapsedMs: 301_000 }),
    'Mixdown export · starting browser download · 05:01 elapsed',
  );
});

test('export elapsed time stays non-negative and handles invalid input', () => {
  assert.equal(formatExportElapsed(-1), '00:00');
  assert.equal(formatExportElapsed(Number.NaN), '00:00');
});

test('export task reports engine, WAV preparation, download handoff, then completion', async () => {
  const statuses = [];
  const result = await runExportTask({
    label: 'Mixdown',
    startedAt: 1000,
    now: () => 4000,
    action: async reportPhase => {
      reportPhase('encoding');
      reportPhase('download');
      return { frames: 42 };
    },
    onStatus: status => statuses.push(status),
    successMessage: value => `WAV ready · browser download started · ${value.frames} frames`,
    failureMessage: error => error.message,
  });

  assert.deepEqual(statuses.map(status => status.phase), ['engine', 'encoding', 'download', 'complete']);
  assert.equal(statuses.at(-1).message, 'WAV ready · browser download started · 42 frames');
  assert.equal(statuses.at(-1).finishedAt - statuses.at(-1).startedAt, 3000);
  assert.deepEqual(result, { frames: 42 });
});

test('export task leaves a persistent failure status and rethrows the engine error', async () => {
  const statuses = [];
  await assert.rejects(runExportTask({
    label: 'Mixdown',
    startedAt: 1000,
    now: () => 3000,
    action: async () => { throw new Error('render failed'); },
    onStatus: status => statuses.push(status),
    successMessage: () => 'unexpected success',
    failureMessage: (error, elapsedMs) => `${error.message} · ${formatExportElapsed(elapsedMs)} elapsed`,
  }), /render failed/);

  assert.deepEqual(statuses.map(status => status.phase), ['engine', 'failed']);
  assert.equal(statuses.at(-1).message, 'render failed · 00:02 elapsed');
  assert.equal(statuses.at(-1).kind, 'error');
});
