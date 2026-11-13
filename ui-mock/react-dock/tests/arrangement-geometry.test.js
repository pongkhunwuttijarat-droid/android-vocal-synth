// The arrangement timeline's scroll scale, its legal range, and the agreement
// between what is drawn and what can be clicked. A pixel drawn at X must be the
// pixel a press at X selects, at every scroll offset and zoom level — that is
// the whole argument for keeping these functions pure and in a plain module.
import test from 'node:test';
import assert from 'node:assert/strict';
import {
  ARR, arrContentMeasures, arrFirstMeasure, arrHitPart, arrLaneAt, arrMeasureWidth,
  arrNoteRect, arrPartRect, arrTickAt, arrVisibleMeasures, arrWaveformTimeRange, arrZoomScrollX, clampArrScrollX,
} from '../src/openutau/arrangement-geometry.js';

const TPM = 480 * 4;
const part = (position_ticks, duration_ticks, id = 'part-1') => ({ id, position_ticks, duration_ticks });
// Lane 1's block sits on top of lane 0's in document space, so a hit test that
// searched every lane would return the wrong one for a press in lane 1.
const tracks = [
  { id: 'track-1', parts: [part(TPM * 10, TPM * 2, 'part-a')] },
  { id: 'track-2', parts: [part(TPM * 10, TPM * 2, 'part-b'), part(TPM * 4, TPM * 2, 'part-c')] },
];

test('the opening view is 16 measures across whatever the dock is', () => {
  assert.equal(arrMeasureWidth(1152), 72);
  assert.equal(arrMeasureWidth(1600), 100);
  // A very narrow dock must not produce a zero or negative column.
  assert.equal(arrMeasureWidth(40), ARR.minMeasureWidth);
});

test('the timeline is as long as the song, not as long as the viewport', () => {
  assert.equal(arrContentMeasures(TPM * 8), 16);
  assert.equal(arrContentMeasures(TPM * 100), 104);
  // An empty document still shows the opening view.
  assert.equal(arrContentMeasures(0), ARR.visibleMeasures);
  // A short song gets the trailing bars so the last bar is not flush right.
  assert.equal(arrContentMeasures(TPM * 13), 17);
});

test('the scroll range is the overflow, and never negative', () => {
  assert.equal(clampArrScrollX(500, 1000, 2000), 500);
  assert.equal(clampArrScrollX(5000, 1000, 2000), 1000);
  assert.equal(clampArrScrollX(-40, 1000, 2000), 0);
  // Nothing to scroll: the offset stays at the start whatever was asked for.
  assert.equal(clampArrScrollX(400, 1000, 900), 0);
});

test('the visible measure window follows the offset', () => {
  assert.equal(arrFirstMeasure(0, 72), 0);
  assert.equal(arrFirstMeasure(80, 72), 1);
  // A partial column at the left edge still has to be drawn.
  assert.equal(arrVisibleMeasures(1152, 72), 17);
  assert.ok(arrFirstMeasure(72 * 500, 72) + arrVisibleMeasures(1152, 72) > 500);
});

test('zooming keeps the centre measure where it was', () => {
  // Centre of a 1000px viewport at offset 400 is measure ~9.7 at 72px.
  const zoomed = arrZoomScrollX(400, 1000, 72, 144, 10000);
  assert.equal(Math.round((zoomed + 500) / 144), Math.round((400 + 500) / 72));
  // And it still lands inside the scrollable range.
  assert.ok(zoomed >= 0 && zoomed <= 9000);
});

test('a part block moves with the scroll, and keeps its size', () => {
  const at = scrollX => arrPartRect(part(TPM * 10, TPM * 2), 0, 72, scrollX);
  assert.equal(at(0).x, 720);
  assert.equal(at(300).x, 420);
  assert.equal(at(300).w, at(0).w);
  // The row, not the scroll, decides y.
  assert.equal(at(9999).y, ARR.rulerHeight + 8);
});

test('miniature synth notes follow the part offset, zoom, scroll and pitch range', () => {
  const measureWidth = 96;
  const scrollX = 100;
  const synthPart = part(TPM * 2, TPM * 4, 'synth-part');
  const middle = arrNoteRect(synthPart, { position_ticks: 480, duration_ticks: 960, tone: 60 }, 1, measureWidth, scrollX, { low: 48, high: 72 });
  assert.equal(middle.x, 116); // part starts at bar 3; note starts one beat later
  assert.equal(middle.w, 48);
  const high = arrNoteRect(synthPart, { position_ticks: 480, duration_ticks: 960, tone: 72 }, 1, measureWidth, scrollX, { low: 48, high: 72 });
  const low = arrNoteRect(synthPart, { position_ticks: 480, duration_ticks: 960, tone: 48 }, 1, measureWidth, scrollX, { low: 48, high: 72 });
  assert.ok(high.y < middle.y && middle.y < low.y);
  assert.equal(arrNoteRect(synthPart, { position_ticks: TPM * 5, duration_ticks: 480, tone: 60 }, 1, measureWidth, scrollX), null);
});

test('waveform columns map through zoom and scroll to project audio time', () => {
  const measureWidth = 96;
  const scrollX = 96 * 3;
  const bpm = 120;
  // Pixel 0 after scrolling is bar 4 (6 seconds at 120 BPM); half a
  // measure pixel span represents two beats, or one second.
  const time = arrWaveformTimeRange(0, 48, measureWidth, scrollX, bpm);
  assert.deepEqual(time, { fromSeconds: 6, toSeconds: 7 });
  assert.equal(arrWaveformTimeRange(0, 2, 0, 0, 0), null);
});

test('the ruler is not a lane, and lanes are the track rows', () => {
  assert.equal(arrLaneAt(0), -1);
  assert.equal(arrLaneAt(ARR.rulerHeight - 1), -1);
  assert.equal(arrLaneAt(ARR.rulerHeight), 0);
  assert.equal(arrLaneAt(ARR.rulerHeight + ARR.rowHeight * 2 + 4), 2);
});

test('clicking a drawn part selects that part at any offset', () => {
  const measure = 72;
  const rect = arrPartRect(part(TPM * 10, TPM * 2), 0, measure, 640);
  assert.equal(rect.x, 80);
  assert.equal(arrHitPart(tracks, rect.x + 5, rect.y + 5, measure, 640)?.part.id, 'part-a');
  // The same pixel after scrolling back is empty space: a hit test that ignored
  // the offset would select whatever slid under the pointer.
  assert.equal(arrHitPart(tracks, rect.x + 5, rect.y + 5, measure, 0), null);
  // Row first, as in OpenUtau: a press in lane 1 gets lane 1's block even
  // though lane 0's overlaps it in the document.
  const lower = ARR.rulerHeight + ARR.rowHeight + 20;
  assert.equal(arrHitPart(tracks, rect.x + 5, lower, measure, 640)?.part.id, 'part-b');
});

test('a click in empty lane space maps to the tick under the pointer', () => {
  const measure = 72;
  // Bar 10 plus one beat, with the timeline scrolled three bars along.
  const x = 10 * measure + measure / 4 - 3 * measure;
  assert.equal(arrTickAt(x, measure, 3 * measure), TPM * 10 + 480);
  // Scrolled past the start of the song, the tick never goes negative.
  assert.equal(arrTickAt(0, measure, 0), 0);
  assert.equal(arrTickAt(0, measure, 5 * measure), TPM * 5);
});