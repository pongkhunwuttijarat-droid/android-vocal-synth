// The OpenUtau clone panels speak in UI intents; `engine.js` is the only place
// that turns them into Rust `project-session` commands. These tests pin that
// translation to the envelopes in docs/CONTRACT-v1.md §6, so a panel change
// cannot quietly drift from the contract.
import test from 'node:test';
import assert from 'node:assert/strict';
import { commandsFor, createEngine, UI_ONLY } from '../src/openutau/engine.js';

test('panel intents become exactly the documented command envelopes', () => {
  assert.deepEqual(
    commandsFor({ type: 'part/add', trackId: 't-1', tick: 480, durationTicks: 1920 }),
    [{ type: 'part.create', track_id: 't-1', kind: 'voice', name: 'New Part', position_ticks: 480, duration_ticks: 1920 }],
  );
  // §6 exposes moves and resizes as deltas. Gestures carry the position the
  // pointer is asking for, so the engine turns targets into deltas against its
  // own state; that keeps a coalesced drag landing where the pointer is.
  const current = { 'n-1': { id: 'n-1', position_ticks: 480, tone: 69, duration_ticks: 480 }, 'n-2': { id: 'n-2', position_ticks: 960, tone: 69, duration_ticks: 480 } };
  const find = id => current[id] ?? null;
  assert.deepEqual(
    commandsFor({ type: 'note/move', updates: [{ id: 'n-1', position_ticks: 600, tone: 67 }, { id: 'n-2', position_ticks: 1080, tone: 67 }] }, find),
    [
      { type: 'note.move', note_id: 'n-1', delta_position_ticks: 120, delta_tone: -2 },
      { type: 'note.move', note_id: 'n-2', delta_position_ticks: 120, delta_tone: -2 },
    ],
  );
  // Dragging the left edge is a move plus an inverse resize, still two deltas.
  assert.deepEqual(
    commandsFor({ type: 'note/resize', updates: [{ id: 'n-1', position_ticks: 600, duration_ticks: 360 }] }, find),
    [
      { type: 'note.move', note_id: 'n-1', delta_position_ticks: 120, delta_tone: 0 },
      { type: 'note.resize', note_id: 'n-1', delta_duration_ticks: -120 },
    ],
  );
  // A target the engine already holds sends nothing, and a note it does not
  // know about cannot be turned into a delta at all.
  assert.deepEqual(commandsFor({ type: 'note/move', updates: [{ id: 'n-1', position_ticks: 480, tone: 69 }] }, find), []);
  assert.deepEqual(commandsFor({ type: 'note/tone', updates: [{ id: 'ghost', tone: 70 }] }, find), []);
  assert.deepEqual(commandsFor({ type: 'note/tone', updates: [{ id: 'n-1', tone: 71 }] }, find), [{ type: 'note.set_tone', note_id: 'n-1', tone: 71 }]);
  // A split shortens the head and adds the tail, so both halves keep engine ids.
  assert.deepEqual(
    commandsFor({ type: 'note/split', id: 'n-1', tick: 720, note: { id: 'n-1', part_id: 'p-1', position_ticks: 480, tone: 69, duration_ticks: 480, lyric: 'あ' } }),
    [
      { type: 'note.resize', note_id: 'n-1', delta_duration_ticks: -240 },
      { type: 'note.add', part_id: 'p-1', position_ticks: 720, tone: 69, duration_ticks: 240, lyric: 'あ' },
    ],
  );
  assert.deepEqual(commandsFor({ type: 'track/mute', trackId: 't-1', mute: true }), [{ type: 'track.set_mute', track_id: 't-1', mute: true }]);
  assert.deepEqual(commandsFor({ type: 'track/solo', trackId: 't-1', solo: true }), [{ type: 'track.set_solo', track_id: 't-1', solo: true }]);
  assert.deepEqual(commandsFor({ type: 'track/volume', trackId: 't-1', volumeDb: -6 }), [{ type: 'track.set_volume_db', track_id: 't-1', volume: -6 }]);
});

test('intents the contract cannot express stay UI state instead of faking a command', () => {
  // `track.create` is now part of the command set, while selection
  // remains UI state that never reaches the engine.
  assert.deepEqual(commandsFor({ type: 'track/add' }), [{ type: 'track.create', name: 'New Track' }]);
  assert.ok(UI_ONLY.has('part/select'));
  assert.ok(UI_ONLY.has('notes/select'));
  assert.equal(commandsFor({ type: 'notes/select', ids: ['n-1'] }), null);
  // Expression curves are NOT in that set: §6.6 has `curve.apply_stroke`, and an
  // earlier version of this page wrongly listed `exp/point` as UI-only.
  assert.equal(UI_ONLY.has('exp/stroke'), false);
  assert.equal(commandsFor({ type: 'exp/point' }), null);
});

test('expression strokes become one curve.apply_stroke envelope', () => {
  // §6.6 `curve.apply_stroke`. A curve the part does not have yet sends
  // `curve_id: null` + `create: true` and lets the engine mint the id; the
  // `replace` mode is what OpenUtau's overwrite commit does
  // (`MergedSetCurveCommand`).
  assert.deepEqual(
    commandsFor({
      type: 'exp/stroke',
      partId: 'p-1',
      abbr: 'dyn',
      startTick: 0,
      endTick: 1920,
      points: [{ tick: 0, value: 0 }, { tick: 960, value: -120 }, { tick: 1920, value: 0 }],
    }),
    [{
      type: 'curve.apply_stroke',
      part_id: 'p-1',
      abbr: 'dyn',
      curve_id: null,
      create: true,
      mode: 'replace',
      start_tick: 0,
      end_tick: 1920,
      points: [{ tick: 0, value: 0 }, { tick: 960, value: -120 }, { tick: 1920, value: 0 }],
    }],
  );
  // Editing an existing curve names its id and does not ask to create one.
  assert.deepEqual(
    commandsFor({ type: 'exp/stroke', partId: 'p-1', abbr: 'pitd', curveId: 'c-9', create: false, startTick: 0, endTick: 480, points: [{ tick: 0, value: 0 }] }),
    [{
      type: 'curve.apply_stroke',
      part_id: 'p-1',
      abbr: 'pitd',
      curve_id: 'c-9',
      create: false,
      mode: 'replace',
      start_tick: 0,
      end_tick: 480,
      points: [{ tick: 0, value: 0 }],
    }],
  );
});

test('clone panels edit the live Rust session and undo a drag as one step', { skip: process.env.RUSTED_UTAU_SERVER_URL ? false : 'set RUSTED_UTAU_SERVER_URL to run' }, async t => {
  const seen = [];
  const engine = createEngine({
    kind: 'http',
    baseUrl: process.env.RUSTED_UTAU_SERVER_URL,
    onChange: state => seen.push(state),
  });
  t.after(() => engine.close());
  await engine.boot({ name: 'Clone Engine E2E' });

  // Booting seeds a phrase through real commands: one part plus four notes.
  const ready = seen.at(-1);
  assert.equal(ready.status, 'ready');
  const part = ready.tracks.flatMap(track => track.parts).find(entry => entry.kind === 'voice');
  assert.ok(part, 'a session must come up with a voice part to edit');
  const notes = ready.notes[part.id];
  assert.equal(notes.length, 4);
  // The seed uses the dev bank's own tokens (see `seedNotes`) so a render
  // produces audio rather than skipping every phrase.
  assert.deepEqual(notes.map(note => note.lyric), ['see[s i]', 'led[l e d]', 'ah[3]', 'la[l A]']);

  // A drag is beginGroup → delta commands → commitGroup, so one undo reverts it.
  const ids = notes.slice(0, 2).map(note => note.id);
  await engine.beginGroup('Move notes');
  await engine.send([{ type: 'note/move', updates: notes.slice(0, 2).map(note => ({ id: note.id, position_ticks: note.position_ticks + 240, tone: note.tone + 3 })) }]);
  await engine.commitGroup();
  const moved = seen.at(-1).notes[part.id].filter(note => ids.includes(note.id));
  assert.deepEqual(moved.map(note => note.tone), [69, 69], 'three semitones up from the seeded 66');
  assert.deepEqual(moved.map(note => note.position_ticks), [720, 1200], 'one beat later');

  await engine.undo();
  const reverted = seen.at(-1).notes[part.id].filter(note => ids.includes(note.id));
  assert.deepEqual(reverted.map(note => ({ tick: note.position_ticks, tone: note.tone })), [
    { tick: 480, tone: 66 },
    { tick: 960, tone: 66 },
  ]);

  await engine.redo();
  const redone = seen.at(-1).notes[part.id].filter(note => ids.includes(note.id));
  assert.deepEqual(redone.map(note => ({ tick: note.position_ticks, tone: note.tone })), [
    { tick: 720, tone: 69 },
    { tick: 1200, tone: 69 },
  ]);

  // A fast tap releases before the group-open response arrives; the commit must
  // still close the transaction, otherwise the note stays pending and the next
  // gesture inherits an open group. A grouped command answers with no changes,
  // so the commit patch is also the only place the minted id shows up.
  await engine.beginGroup('Tap');
  await engine.send([{ type: 'note/add', partId: part.id, tick: 1920, tone: 64, durationTicks: 480, lyric: 'ん' }]);
  const committed = await engine.commitGroup();
  assert.deepEqual(committed.filter(change => change.kind === 'note.added').map(change => change.lyric ?? change.data?.lyric), ['ん']);
  const tapped = seen.at(-1).notes[part.id];
  assert.ok(tapped.some(note => note.lyric === 'ん'), 'the tapped note must be visible after the commit');
  await engine.undo();
  assert.ok(!seen.at(-1).notes[part.id].some(note => note.lyric === 'ん'), 'one undo reverts the tap');

  // A long drag sends many updates, and a grouped command stays invisible to a
// session read until the group commits. Every delta must therefore be measured
// from what the gesture has already asked for, not from the pre-drag position,
// or the moves compound (a 3-row drag would land 6 rows away).
  const before = engine.state.notes[part.id].find(note => note.id === ids[0]);
  await engine.beginGroup('Long drag');
  for (let i = 1; i <= 6; i++) {
    await engine.send([{ type: 'note/move', updates: [{ id: ids[0], position_ticks: before.position_ticks + i * 120, tone: before.tone + i }] }]);
  }
  await engine.commitGroup();
  const dragged = engine.state.notes[part.id].find(note => note.id === ids[0]);
  assert.deepEqual({ tick: dragged.position_ticks, tone: dragged.tone }, { tick: before.position_ticks + 720, tone: before.tone + 6 });
  await engine.undo();
  assert.deepEqual(
    (({ position_ticks, tone }) => ({ position_ticks, tone }))(engine.state.notes[part.id].find(note => note.id === ids[0])),
    { position_ticks: before.position_ticks, tone: before.tone },
  );

  // Unsupported intents are reported instead of silently dropped.
  const result = await engine.send([{ type: 'note/vibrato', ids, patch: { on: true } }]);
  assert.deepEqual(result.unsupported, ['note/vibrato']);

  // The pen tool needs the engine-minted id to select what it just drew.
  const drawn = await engine.send([{ type: 'note/add', partId: part.id, tick: 1440, tone: 71, durationTicks: 480, lyric: 'お' }]);
  assert.deepEqual(drawn.changes.filter(change => change.kind === 'note.added').map(change => change.part_id), [part.id]);
  const mintedId = drawn.changes.find(change => change.kind === 'note.added').note_id;
  assert.ok(mintedId, 'note.add must report the id it minted');

  // `group: true` without an explicit beginGesture still closes its own
  // transaction, so the next edit is not swallowed by an open group.
  await engine.send([{ type: 'note/lyric', ids: [mintedId], lyric: 'き' }], { group: true });
  await engine.undo();
  assert.equal(seen.at(-1).notes[part.id].find(note => note.id === mintedId).lyric, 'お');
});

test('a stroke round-trips through the session as an authoritative curve', { skip: process.env.RUSTED_UTAU_SERVER_URL ? false : 'set RUSTED_UTAU_SERVER_URL to run' }, async t => {
  const engine = createEngine({
    kind: 'http',
    baseUrl: process.env.RUSTED_UTAU_SERVER_URL,
    onChange: () => {},
  });
  t.after(() => engine.close());
  await engine.boot({ name: 'Clone Curve E2E' });
  const part = engine.state.tracks.flatMap(track => track.parts).find(entry => entry.kind === 'voice');
  assert.ok(part, 'a session must come up with a voice part');
  const partTicks = part.duration_ticks;
  assert.deepEqual(part.curves ?? [], [], 'a fresh part has no curves yet');

  // One freeform gesture is one command, and the engine answers with the
  // CANONICAL points — 5-tick snap, boundary anchors at the descriptor default,
  // clamped to the descriptor range. The editor must draw what came back.
  const stroke = await engine.send([{
    type: 'exp/stroke',
    partId: part.id,
    abbr: 'dyn',
    startTick: 0,
    endTick: partTicks,
    points: [{ tick: 3, value: -240 }, { tick: 911, value: -500 }, { tick: partTicks - 1, value: 120 }],
  }]);
  assert.deepEqual(stroke.unsupported, [], 'exp/stroke must not be reported unsupported');
  const changed = stroke.changes.find(change => change.kind === 'curve.changed');
  assert.ok(changed, 'the engine must answer with a curve.changed patch');
  assert.equal(changed.abbr, 'dyn');
  assert.ok(changed.curve_id, 'the engine mints the id when curve_id is null');
  assert.ok(stroke.changes.some(change => change.kind === 'render.invalidated'), 'the render must be invalidated');

  // The canonical points: `dyn` is -240..120, so -500 clamps, and ticks snap to 5.
  for (const point of changed.data.points) {
    assert.ok(point.tick % 5 === 0, `tick ${point.tick} must snap to 5`);
    assert.ok(point.value >= -240 && point.value <= 120, `value ${point.value} must be inside the descriptor range`);
  }

  // And the snapshot the panels render is the engine's, not what we sent.
  const written = engine.state.tracks
    .flatMap(track => track.parts)
    .find(entry => entry.id === part.id)
    .curves.find(curve => curve.abbr === 'dyn');
  assert.ok(written, 'parts[].curves must carry the new curve');
  assert.equal(written.id, changed.curve_id);
  assert.deepEqual(written.points, changed.data.points);

  // Editing it again names the existing id instead of minting another one.
  const again = await engine.send([{
    type: 'exp/stroke',
    partId: part.id,
    abbr: 'dyn',
    curveId: written.id,
    create: false,
    startTick: 0,
    endTick: partTicks,
    points: [{ tick: 0, value: 0 }, { tick: 960, value: 60 }, { tick: partTicks, value: 0 }],
  }]);
  assert.equal(again.changes.find(change => change.kind === 'curve.changed').curve_id, written.id);

  // One undo restores the FIRST stroke's canonical points, not an empty curve:
  // the second stroke was an edit of an existing curve, not its creation.
  await engine.undo();
  const afterUndo = engine.state.tracks
    .flatMap(track => track.parts)
    .find(entry => entry.id === part.id)
    .curves.find(curve => curve.abbr === 'dyn');
  assert.ok(afterUndo, 'undoing an edit must not delete the curve');
  assert.deepEqual(afterUndo.points, changed.data.points, 'one undo restores the previous curve');

  // A second undo removes the curve itself (undo data carries the old value).
  await engine.undo();
  const gone = engine.state.tracks
    .flatMap(track => track.parts)
    .find(entry => entry.id === part.id)
    .curves ?? [];
  assert.deepEqual(gone, [], 'undoing the creation removes the curve');
});
test('renderTrack is reachable on the engine and refuses before a session exists', async () => {
  // The File menu calls `engine.renderTrack(...)`. When this method was missing
  // the menu item failed with `renderTrack is not a function` — a UI crash
  // instead of an engine refusal — so pin both the presence of the method and
  // the promise it returns with no session open.
  const engine = createEngine({ kind: 'http', baseUrl: '', onChange: () => {} });
  assert.equal(typeof engine.renderTrack, 'function');
  await assert.rejects(() => engine.renderTrack(0), /No project session is open/);
});

test('mixer toggles carry their boolean, because the engine requires it', () => {
  // Clicking M/S used to dispatch `{ type: 'track/mute', trackId }` with no
  // value. `undefined` does not survive JSON, so the envelope reached the Rust
  // session as `track.set_mute` with no `mute` field, the engine answered
  // "'mute' is required and must be a boolean", and the session DIED — the page
  // fell back to the mock document and looked as if loading the project had
  // failed. The value is required, so pin it at the translation boundary.
  assert.deepEqual(
    commandsFor({ type: 'track/mute', trackId: 't-1', mute: true }),
    [{ type: 'track.set_mute', track_id: 't-1', mute: true }],
  );
  assert.deepEqual(
    commandsFor({ type: 'track/solo', trackId: 't-2', solo: false }),
    [{ type: 'track.set_solo', track_id: 't-2', solo: false }],
  );
  // Both are toggles, so `false` is just as load-bearing as `true`: a panel
  // that only ever sends the truthy value can never turn a flag back off.
  for (const envelope of [
    commandsFor({ type: 'track/mute', trackId: 't-1', mute: false })[0],
    commandsFor({ type: 'track/solo', trackId: 't-1', solo: false })[0],
  ]) {
    assert.equal(JSON.parse(JSON.stringify(envelope)).mute ?? JSON.parse(JSON.stringify(envelope)).solo, false,
      'a false flag must survive JSON as false, not disappear');
  }
});

test('an intent missing a required field fails here, not inside the engine', () => {
  // The guard is the load-bearing part: a panel that forgets the value must be
  // caught at the translation boundary, naming the field, instead of sending a
  // command with the key silently dropped by JSON and killing the session.
  assert.throws(
    () => commandsFor({ type: 'track/mute', trackId: 't-1' }),
    /track\/mute' produced track\.set_mute without 'mute'/,
  );
  assert.throws(
    () => commandsFor({ type: 'track/solo', trackId: 't-1' }),
    /track\/solo' produced track\.set_solo without 'solo'/,
  );
  // An intent with no command builder is still `null`, not a throw: the caller
  // distinguishes "nothing to send" from "something to send, wrongly".
  assert.equal(commandsFor({ type: 'ui/tool', tool: 'pencil' }), null);
});
