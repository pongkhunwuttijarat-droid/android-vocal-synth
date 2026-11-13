// Engine binding for the OpenUtau clone page.
//
// The clone panels never mutate the document themselves: they raise the same
// intent objects the mock reducer used to consume, and this module turns them
// into Rust `project-session` commands. Grouped gestures (move, resize, split,
// pen draw) map onto one engine undo transaction, exactly like the main editor.
//
// Commands come from §6 or documented v2 drafts; anything not exposed yet is
// reported through `unsupported` instead of being faked locally.
import { createSessionBackend } from '../session-backends.js';
import { createProjectBundle, restoreProjectBundleMedia } from './project-bundle.js';
import { importWaveMediaFiles } from './ustx-import.js';

const TICKS_PER_BEAT = 480;
/** OpenUtau refuses gestures shorter than 15 ticks. */
const MIN_NOTE_TICKS = 15;

/**
 * Turn one UI intent into the engine commands it stands for.
 *
 * Gestures carry the position the pointer is asking for, not the delta: a drag
 * can outrun the session (every command is a round trip), and a dropped frame
 * must not leave the note behind. `findNote` resolves the engine's current
 * state so the wire format stays the §6 delta commands.
 */
const COMMAND_FOR = {
  'part/add': ({ trackId, tick, durationTicks }) => [{ type: 'part.create', track_id: trackId, kind: 'voice', name: 'New Part', position_ticks: tick, duration_ticks: durationTicks }],
  'track/mute': ({ trackId, mute }) => [{ type: 'track.set_mute', track_id: trackId, mute }],
  'track/solo': ({ trackId, solo }) => [{ type: 'track.set_solo', track_id: trackId, solo }],
  'track/volume': ({ trackId, volumeDb }) => [{ type: 'track.set_volume_db', track_id: trackId, volume: volumeDb }],
  'track/pan': ({ trackId, pan }) => [{ type: 'track.set_pan', track_id: trackId, pan }],
  'track/add': ({ name = 'New Track', toIndex }) => [{ type: 'track.create', name, ...(toIndex == null ? {} : { to_index: toIndex }) }],
  'track/remove': ({ trackId }) => [{ type: 'track.remove', track_id: trackId }],
  'track/reorder': ({ trackId, toIndex }) => [{ type: 'track.reorder', track_id: trackId, to_index: toIndex }],
  'track/mix_fx': ({ trackId, mixFx }) => [{ type: 'track.set_mix_fx', track_id: trackId, mix_fx: mixFx }],
  // `null` clears the singer, which is the state an audio-only track has. The
  // engine normalises a blank string the same way, so the picker and the
  // command agree on what "no singer" means.
  'track/singer': ({ trackId, singerId }) => [{ type: 'track.set_singer', track_id: trackId, singer_id: singerId }],
  'note/add': ({ partId, tick, tone, durationTicks, lyric }) => [{ type: 'note.add', part_id: partId, position_ticks: tick, tone, duration_ticks: durationTicks, lyric }],
  'note/remove': ({ ids }) => ids.map(noteId => ({ type: 'note.remove', note_id: noteId })),
  'note/move': ({ updates }, findNote) => updates.flatMap(update => {
    const current = findNote(update.id);
    if (!current) return [];
    const position = (update.position_ticks ?? current.position_ticks) - current.position_ticks;
    const tone = (update.tone ?? current.tone) - current.tone;
    // A coalesced drag re-sends the target it is already at; a no-op would only
    // add a history entry.
    if (position === 0 && tone === 0) return [];
    return [{ type: 'note.move', note_id: update.id, delta_position_ticks: position, delta_tone: tone }];
  }),
  'note/resize': ({ updates }, findNote) => updates.flatMap(update => {
    const current = findNote(update.id);
    if (!current) return [];
    const start = update.position_ticks ?? current.position_ticks;
    const duration = Math.max(MIN_NOTE_TICKS, update.duration_ticks ?? current.duration_ticks);
    return [
      ...(start !== current.position_ticks ? [{ type: 'note.move', note_id: update.id, delta_position_ticks: start - current.position_ticks, delta_tone: 0 }] : []),
      ...(duration !== current.duration_ticks ? [{ type: 'note.resize', note_id: update.id, delta_duration_ticks: duration - current.duration_ticks }] : []),
    ];
  }),
  'note/lyric': ({ ids, lyric }) => ids.map(noteId => ({ type: 'note.set_lyric', note_id: noteId, lyric })),
  'note/tone': ({ updates }, findNote) => updates.flatMap(update => {
    const current = findNote(update.id);
    if (!current || current.tone === update.tone) return [];
    return [{ type: 'note.set_tone', note_id: update.id, tone: update.tone }];
  }),
  // Per-note pitch curve. §6 has no frozen command for this — it is a T014
  // contract extension — so the envelope is the command the new Rust router
  // accepts: `pitch.data` carries OpenUtau's units (x = ms from the note start,
  // y = tenths of a semitone, shape = io/l/i/o/sp), which is what
  // `domain::PitchPoint` and the USTX codec already use, so nothing is
  // converted on the wire. Skipped when the curve is byte-identical to the one
  // already stored, so a click that changes nothing does not burn a revision.
  'note/pitch': ({ id, pitch, current }) => {
    if (!pitch?.data?.length) return [];
    if (JSON.stringify(current?.data ?? []) === JSON.stringify(pitch.data)
      && (current?.snap_first ?? true) === (pitch.snap_first ?? true)) return [];
    return [{ type: 'note.set_pitch', note_id: id, pitch: { data: pitch.data, snap_first: pitch.snap_first !== false } }];
  },
  'note/vibrato': ({ ids, patch }, findNote) => ids.flatMap(noteId => {
    const current = findNote(noteId);
    if (!current) return [];
    const value = patch === null ? { ...current.vibrato, on: false } : { ...current.vibrato, ...patch };
    const vibrato = {
      length: value.on ? value.length : 0,
      period: value.period,
      depth: value.depth,
      in: value.fadeIn,
      out: value.fadeOut,
      shift: value.shift,
      drift: value.drift,
      vol_link: value.volLink ?? 0,
    };
    const stored = {
      length: current.vibrato.on ? current.vibrato.length : 0,
      period: current.vibrato.period,
      depth: current.vibrato.depth,
      in: current.vibrato.fadeIn,
      out: current.vibrato.fadeOut,
      shift: current.vibrato.shift,
      drift: current.vibrato.drift,
      vol_link: current.vibrato.volLink ?? 0,
    };
    if (stored.length === vibrato.length && stored.period === vibrato.period
      && stored.depth === vibrato.depth && stored.in === vibrato.in
      && stored.out === vibrato.out && stored.shift === vibrato.shift
      && stored.drift === vibrato.drift && stored.vol_link === vibrato.vol_link) return [];
    return [{ type: 'note.set_vibrato', note_id: noteId, vibrato }];
  }),
  'note/split': ({ id, tick, note }) => [
    { type: 'note.resize', note_id: id, delta_duration_ticks: tick - note.position_ticks - note.duration_ticks },
    { type: 'note.add', part_id: note.part_id, position_ticks: tick, tone: note.tone, duration_ticks: note.duration_ticks - (tick - note.position_ticks), lyric: note.lyric },
  ],
  // CONTRACT-v1 §6.6 `curve.apply_stroke`. `curve_id: null` asks the engine to
  // mint one for a curve the part does not have yet; it answers with a
  // `curve.changed` patch carrying the id and the CANONICAL points (5-tick snap,
  // boundary anchors at the descriptor default, clamped to min/max), which is
  // what the editor must draw afterwards rather than what it sent.
  'exp/stroke': ({ partId, abbr, curveId = null, create = true, startTick, endTick, points }) => [{
    type: 'curve.apply_stroke',
    part_id: partId,
    abbr,
    curve_id: curveId,
    create,
    mode: 'replace',
    start_tick: startTick,
    end_tick: endTick,
    points: points.map(point => ({ tick: point.tick, value: point.value })),
  }],
};

/** Intents with no engine command; the caller keeps them as UI state. */
export const UI_ONLY = new Set([
  'part/select', 'notes/select', 'lane/toggle', 'ui/tool', 'ui/snap', 'ui/labels', 'viewport/horizontal-scroll',
  // Pitch-tool settings: the shape of the next point drawn and whether the
  // curve is stored with `snap_first`. They ride along with the next
  // `note/pitch` rather than being state of their own.
  'ui/pitch-shape', 'ui/pitch-snap-first',
  'transport/toggle', 'transport/seek', 'transport/stop', 'transport/restart',
  'transport/skip-measure', 'transport/skip-note',
  // Bank installs are host storage, not project state, so they only tell the
  // preview to re-resolve its source.
  'ui/banks-changed',
  // Waveform placement is presentation state. Expression curves are NOT: they
  // go through `exp/stroke` -> `curve.apply_stroke` and are read back from
  // `parts[].curves` in the session snapshot.
  'waveform/toggle', 'waveform/placement', 'exp/select', 'exp/hover',
]);

/**
 * Turn a panel intent into the §6 command envelopes it stands for.
 *
 * Every envelope is checked before it is returned. `JSON.stringify` DROPS a key
 * whose value is `undefined`, so an intent that forgot a required field used to
 * arrive at Rust as a command with the field simply absent; the engine answered
 * a validation error, the session died, and the page fell back to the mock
 * document — which reads as "the project would not load", not as "a panel sent
 * an incomplete command". Failing here, naming the field, keeps the real cause
 * at the panel that dropped it.
 */
export function commandsFor(intent, findNote = () => null) {
  const build = COMMAND_FOR[intent.type];
  if (!build) return null;
  const commands = build(intent, findNote) ?? [];
  for (const command of commands) {
    for (const [field, value] of Object.entries(command)) {
      if (value === undefined) {
        throw new Error(`intent '${intent.type}' produced ${command.type} without '${field}'`);
      }
    }
  }
  return commands;
}

/**
 * Boots a session and keeps an authoritative copy of the document.
 * `onChange` fires with `{ tracks, notes, revision, status }` after every
 * command, undo and redo, so panels only ever render engine state.
 */
export function createEngine({ kind, baseUrl, onChange, onError, onWorkerLifecycle }) {
  let backend = null;
  let disposed = false;
  let queue = Promise.resolve();

  const state = {
    status: 'connecting', kind, revision: 0, tracks: [], notes: {}, error: null, workerLifecycle: null,
    // Project identity, owned by the engine's side of the contract: `sessionId`
    // changes whenever the document behind the panels is swapped, and
    // `savedRevision` is what the dirty indicator compares against.
    sessionId: null, storeName: null, documentName: null, savedRevision: 0, dirty: false,
    // The engine's tempo, used to align the render preview (audio seconds) with
    // the roll's tick grid. It is NOT assumed to be the UI's default.
    bpm: 120,
    busy: false, lastReason: null,
    // One entry per undo transaction, newest last. A gesture must produce exactly
    // one; a drag that splits in two is invisible in the UI but obvious here, and
    // the probe reads it to say which. In memory only, no I/O.
    transactions: [],
  };

  const emit = () => {
    if (disposed) return;
    const tracks = (backend?.state?.snapshot?.project?.tracks || []).map(track => ({
      id: track.id,
      name: track.name,
      // `singer_id` IS part of the frozen §6 snapshot (`tracks[].singer_id`,
      // null until a track names a bank) — keep it verbatim: the waveform
      // preview matches on it to pick the bank it renders with, and a UI
      // placeholder here would look like a real singer name. Panels render
      // their own placeholder for the empty case.
      singer_id: track.singer_id ?? null,
      singer_detail: track.singer_detail || '',
      mute: !!track.mute,
      solo: !!track.solo,
      volume_db: track.volume_db ?? 0,
      pan: track.pan ?? 0,
      parts: (track.parts || []).map(part => ({ ...part })),
    }));
    // Store the mapped tracks so `engine.state` and `onChange` never disagree;
    // `parts[].curves` rides along here, which is how the expression editor
    // reads the engine's canonical curve points back.
    state.tracks = tracks;
    state.dirty = state.revision !== state.savedRevision;
    onChange({ ...state, tracks });
  };

  const observeWorkerLifecycle = lifecycle => {
    state.workerLifecycle = lifecycle;
    if (lifecycle.status === 'failed') {
      state.status = 'worker-failed';
      state.error = lifecycle.error;
    }
    onWorkerLifecycle?.(lifecycle);
    emit();
  };

  const readNotes = async () => {
    const routes = backend.partRoutes;
    const notes = {};
    for (const route of routes) {
      const result = await backend.readNotes(route.partId);
      notes[route.partId] = (result.notes || []).map(note => ({
        ...note,
        vibrato: {
          on: (note.vibrato?.length ?? 0) > 0,
          length: note.vibrato?.length ?? 0,
          period: note.vibrato?.period ?? 175,
          depth: note.vibrato?.depth ?? 25,
          fadeIn: note.vibrato?.in ?? 10,
          fadeOut: note.vibrato?.out ?? 10,
          shift: note.vibrato?.shift ?? 0,
          drift: note.vibrato?.drift ?? 0,
          volLink: note.vibrato?.vol_link ?? 0,
        },
      }));
    }
    return notes;
  };

  /** The engine's own view of a note, used to turn a gesture target into a delta. */
  const findNote = noteId => {
    for (const list of Object.values(state.notes)) {
      const note = list.find(candidate => candidate.id === noteId);
      if (note) return pendingNotes.has(noteId) ? { ...note, ...pendingNotes.get(noteId) } : note;
    }
    return null;
  };

  /**
   * What the open gesture has already asked for.
   *
   * Grouped commands stay invisible to a session read until the group commits,
   * so without this every delta in a drag would be measured from the state
   * before the gesture and the moves would compound — dragging up three rows
   * would apply +1, +2 and +3 and land six rows away. The overlay is dropped
   * when the group closes, and the read that follows is authoritative again.
   *
   * It is also the drag PREVIEW. A `note.move` inside an open group produces no
   * snapshot change, so before this was published the note sat still under the
   * pointer and only jumped when the gesture committed on pointer-up — the drag
   * looked like it was not working at all. Panels draw the overlay and drop it
   * the moment the engine answers, so this stays a projection of what was
   * asked for and never a second document model: nothing is persisted, and the
   * overlay is cleared on commit, cancel, undo and redo alike.
   */
  const pendingNotes = new Map();
  /** Bumped on every overlay change so React sees a new object, not a mutated map. */
  let pendingVersion = 0;
  const publishPending = () => {
    pendingVersion += 1;
    state.pendingNotes = pendingVersion;
    state.pending = [...pendingNotes.entries()].map(([id, note]) => ({ id, ...note }));
  };
  const trackPending = command => {
    const id = command.note_id;
    if (!id) return;
    if (command.type === 'note.remove') { pendingNotes.delete(id); publishPending(); return; }
    const current = findNote(id);
    if (!current) return;
    const next = { ...current };
    if (command.type === 'note.move') { next.position_ticks += command.delta_position_ticks; next.tone += command.delta_tone; }
    else if (command.type === 'note.resize') next.duration_ticks += command.delta_duration_ticks;
    else if (command.type === 'note.set_tone') next.tone = command.tone;
    else if (command.type === 'note.set_pitch') next.pitch = command.pitch;
    else if (command.type === 'note.set_lyric') next.lyric = command.lyric;
    pendingNotes.set(id, next);
    publishPending();
  };

  /** A project is clean when it was just created, opened or saved. */
const CLEAN_REASONS = new Set(['boot', 'new', 'open', 'import', 'save', 'save-as']);

const refresh = async reason => {
    state.revision = backend.state.revision;
    state.sessionId = backend.state.sessionId ?? null;
    state.documentName = backend.state?.snapshot?.project?.name ?? null;
    state.bpm = backend.state?.snapshot?.project?.bpm ?? state.bpm;
    // The dirty flag compares against the last create/open/save, so undoing back
    // to it clears the dot and a saved file never looks edited. `new`/`open`
    // decide the store name themselves; everything else keeps the last answer.
    if (CLEAN_REASONS.has(reason)) state.savedRevision = state.revision;
    state.notes = await readNotes();
    state.status = 'ready';
    state.lastReason = reason;
    emit();
    return reason;
  };

  const registeredMediaDuringLifecycle = new Set();
  const registerMediaDuringLifecycle = async (mediaId, bytes) => {
    const media = await backend.registerMedia(mediaId, bytes);
    registeredMediaDuringLifecycle.add(mediaId);
    return media;
  };

  /**
   * Run a project lifecycle call with the UI locked to it.
   *
   * A lifecycle change swaps the document behind the panels, so nothing else may
   * be in flight; `busy` lets the menu disable itself and the status line say
   * why. Failures land in `error` instead of rejecting, because a dialog has to
   * stay open with a readable reason rather than vanish on a console trace.
   */
  const lifecycle = async (reason, step) => {
    if (!backend) throw new Error('No project session is open');
    state.busy = true;
    state.error = null;
    emit();
    // Serialised behind the edit queue, but NOT through `enqueue`: a rejected
    // project action (a name clash, a missing file) must not trip `onError` and
    // drop the whole page back to the mock document.
    const run = async () => {
      registeredMediaDuringLifecycle.clear();
      const result = await step();
      await hydrateMedia();
      await refresh(reason);
      return result;
    };
    const next = queue.then(run, run);
    queue = next.catch(() => {});
    try {
      return await next;
    } catch (error) {
      state.error = error;
      emit();
      throw error;
    } finally {
      registeredMediaDuringLifecycle.clear();
      state.busy = false;
      emit();
    }
  };

  /** Reattach host-persisted audio bytes to the authoritative Rust media source. */
  const hydrateMedia = async () => {
    if (!backend?.registerMedia || !backend?.store?.loadMedia) return;
    const refs = new Set((backend.state?.snapshot?.project?.tracks || [])
      .flatMap(track => track.parts || [])
      .filter(part => part.kind === 'wave' && typeof part.media_ref === 'string' && part.media_ref.startsWith('asset:'))
      .map(part => part.media_ref.slice('asset:'.length)));
    for (const mediaId of refs) {
      if (registeredMediaDuringLifecycle.has(mediaId)) continue;
      let bytes;
      try { bytes = await backend.store.loadMedia(mediaId); }
      catch (error) {
        if (error?.name === 'NotFoundError') throw new Error(`Audio asset '${mediaId}' is missing from browser storage`);
        throw error;
      }
      await registerMediaDuringLifecycle(mediaId, bytes);
    }
  };

  /** One entry per undo transaction, for the probe to assert on. In memory only. */
  const record = (kind, commands, detail = null) => {
    state.transactions = [...state.transactions.slice(-23), { kind, commands, detail, revision: state.revision }];
  };

  /**
   * Open transactions, keyed by the gesture that owns them.
   *
   * The group must belong to a gesture and not to "whatever is open": a press
   * can arrive while the previous gesture is still committing, and with a single
   * shared slot the new gesture would adopt the old group and then watch its own
   * commit close it — one drag would become several undo entries. Each entry here
   * carries its own group id and the count of commands in it.
   */
  const gestures = new Map();
  let gestureSeq = 0;

  /** Serialised so a fast gesture cannot interleave revisions. */
  const enqueue = step => {
    queue = queue.then(step).catch(error => {
      state.status = 'error';
      state.error = error;
      emit();
      onError?.(error);
      throw error;
    });
    return queue;
  };

  const api = {
    get backend() { return backend; },
    get state() { return state; },
    /** Boot the session; falls back to `onError` so the caller can stay on mock. */
    async boot({ name = 'OpenUtau Clone', seed = true } = {}) {
      try {
        backend = await createSessionBackend(kind, { baseUrl, onLifecycle: observeWorkerLifecycle });
        backend.onChange?.(event => {
          if (event.reason !== 'recovery-warning') return;
          const warning = event.warning || {};
          state.error = Object.assign(
            new Error(warning.message || 'Automatic recovery checkpoint failed; this project may not survive a server restart.'),
            { code: warning.code || 'recovery_checkpoint_failed' },
          );
          emit();
        });
        const recovered = await backend.restoreRecovery?.();
        if (recovered) {
          state.storeName = recovered.store_name ?? null;
          state.documentName = recovered.snapshot?.project?.name ?? null;
        } else {
          await backend.createProject({ name });
          // Until the first save the project only exists in the session, so the
          // menu shows a name that is not yet a file. `Save` upserts it.
          state.storeName = name;
          if (seed) await seedNotes(backend);
        }
        await refresh('boot');
        return api;
      } catch (error) {
        state.status = 'error';
        state.error = error;
        emit();
        throw error;
      }
    },
    /**
     * Open an engine undo transaction owned by one gesture. `id` is supplied by
     * the caller (the panel knows which gesture it is) so every later batch and
     * the release can name the same transaction.
     */
    beginGroup(name, id = ++gestureSeq) {
      const gesture = { id, name, groupId: null, promise: null, commands: 0 };
      gestures.set(id, gesture);
      gesture.promise = enqueue(async () => {
        try {
          const result = await backend.beginGroup(name);
          gesture.groupId = result?.group_id ?? result?.groupId ?? null;
        } catch (error) {
          // Sessions allow one open group at a time, so a begin can still be
          // refused (a lost release, two pointers). Losing the grouping is bad;
          // dropping the page back to the mock document is worse, so the gesture
          // continues ungrouped and says so in the transaction log.
          gesture.error = error;
          record('begin-refused', 0, error.message);
        }
        return gesture.groupId;
      }).finally(() => { gesture.promise = null; });
      return gesture.promise;
    },
    /**
     * Apply intents. With `gesture`, every command joins that gesture's
     * transaction, awaiting its open request first — the first pointermove can
     * beat the press's `beginGroup` round trip, and without the wait it would
     * open (and immediately commit) a transaction of its own.
     *
     * `group: true` without a gesture still works for one-shot callers: a group
     * opened here is committed before returning, so nothing is left open to
     * swallow the next edit.
     */
    send(intents, { group = false, gesture: gestureId = null } = {}) {
      // Without an explicit gesture the newest open transaction is adopted, which
      // is what a caller that only did `beginGroup` → `send` → `commitGroup`
      // expects. The clone always names its gesture, so this cannot bind one
      // gesture's batch to another's transaction.
      const owner = gestureId != null ? gestures.get(gestureId) : [...gestures.values()].at(-1) ?? null;
      // Captured outside the queue: awaiting a promise from inside a queue task
      // would wait on ourselves. FIFO order means it has already run by then.
      const pendingOpen = owner?.promise ?? null;
      const lateBatch = group && gestureId != null && !owner;
      return enqueue(async () => {
        if (pendingOpen) { try { await pendingOpen; } catch { /* the batch falls back below */ } }
        const unsupported = [];
        let groupId = owner?.groupId ?? null;
        let openedHere = false;
        let changes = [];
        if (group && !groupId) {
          // Not `api.beginGroup`: that would enqueue behind this very task and
          // deadlock the serial queue.
          const begun = await backend.beginGroup('Edit gesture');
          groupId = begun?.group_id ?? begun?.groupId ?? null;
          openedHere = groupId !== null;
        }
        try {
          for (const intent of intents) {
            const commands = commandsFor(intent, findNote);
            if (!commands) { unsupported.push(intent.type); continue; }
            for (const command of commands) {
              const result = groupId ? await backend.applyGrouped(groupId, command) : await backend.apply(command);
              state.lastChange = result;
              // Only a grouped command needs tracking: an ungrouped one is
              // already visible to the next read.
              if (groupId) {
                trackPending(command);
                if (owner) owner.commands += 1;
                // Publish the preview as soon as the engine has accepted the
                // command, not when the whole batch finishes: waiting for the
                // batch is exactly the lag this removes.
                emit();
              }
              // `note.add`/`part.create` mint engine ids here; panels need them
              // to select what they just drew.
              changes = result?.changes ?? changes;
            }
          }
        } finally {
          if (openedHere) {
            await backend.commitGroup(groupId);
            record('one-shot', changes?.length ?? intents.length);
          } else if (lateBatch) {
            // The gesture already committed, so this stray batch is its own
            // transaction. Recorded because it is exactly the split the probe
            // looks for.
            record('late', intents.length);
          }
        }
        await refresh('edit');
        return { unsupported, changes };
      });
    },
    /** Close a gesture's transaction: one undo entry, whatever the batch count. */
    async commitGroup(gestureId = null) {
      const gesture = gestureId != null ? gestures.get(gestureId) : [...gestures.values()][0];
      if (!gesture) return [];
      if (gesture.promise) { try { await gesture.promise; } catch { /* the group stays null */ } }
      gestures.delete(gesture.id);
      const id = gesture.groupId;
      if (!id) return [];
      return enqueue(async () => {
        // A grouped command answers with no changes; the commit response is
        // where minted ids (note.added / part.added) finally show up.
        const result = await backend.commitGroup(id);
        record('commit', gesture.commands, gesture.name);
        pendingNotes.clear();
        publishPending();
        await refresh('group-commit');
        return result?.changes ?? [];
      });
    },
    async cancelGroup(gestureId = null) {
      const gesture = gestureId != null ? gestures.get(gestureId) : [...gestures.values()][0];
      if (!gesture) return [];
      if (gesture.promise) { try { await gesture.promise; } catch { /* the group stays null */ } }
      gestures.delete(gesture.id);
      const id = gesture.groupId;
      if (!id) return [];
      return enqueue(async () => { await backend.cancelGroup(id); pendingNotes.clear(); publishPending(); await refresh('group-cancel'); return []; });
    },
    undo() { return enqueue(async () => { await backend.undo(); pendingNotes.clear(); publishPending(); await refresh('undo'); }); },
    redo() { return enqueue(async () => { await backend.redo(); pendingNotes.clear(); publishPending(); await refresh('redo'); }); },

    /* ------------------------------------------------------- *
     * Project lifecycle
     *
     * `storeName` is the name the host store files the project under — it is
     * what `openProject` takes, and it is deliberately not the same thing as
     * the document's own `project.name` field: Save As forks a new file
     * without rewriting the song title, exactly like OpenUtau.
     * ------------------------------------------------------- */

    /** Empty project. `seed` fills it with the demo phrase the clone edits. */
    newProject({ name = 'Untitled', seed = true } = {}) {
      return lifecycle('new', async () => {
        pendingNotes.clear();
        await backend.createProject({ name });
        if (seed) await seedNotes(backend);
        state.storeName = name;
        return { name };
      });
    },
    /** Open a stored project by store name; the session behind it changes. */
    openProject(name) {
      return lifecycle('open', async () => {
        pendingNotes.clear();
        await backend.openStored(name);
        state.storeName = name;
        return { name };
      });
    },
    /** Start a new Worker after failure; only an explicitly named stored project is reopened. */
    async recover({ reopenStoreName = null } = {}) {
      if (kind !== 'wasm') {
        throw new Error('Only the Direct WASM session can be restarted here');
      }
      if (typeof backend?.recover === 'function') await backend.recover();
      else backend = await createSessionBackend(kind, { baseUrl, onLifecycle: observeWorkerLifecycle });
      state.sessionId = null;
      state.revision = 0;
      state.tracks = [];
      state.notes = {};
      state.storeName = null;
      state.documentName = null;
      state.savedRevision = 0;
      state.dirty = false;
      state.error = null;
      state.status = 'reopen-required';
      emit();
      if (!reopenStoreName) return { workerReady: true, reopened: false, unsavedStateRestored: false };
      try {
        await api.openProject(reopenStoreName);
        return { workerReady: true, reopened: true, storeName: reopenStoreName, unsavedStateRestored: false };
      } catch (error) {
        return { workerReady: true, reopened: false, storeName: reopenStoreName, unsavedStateRestored: false, error };
      }
    },
    /**
     * Import portable USTX as a NEW document (§6.11 `imported_ustx`).
     *
     * The engine does the import: Rust parses the file and mints every ID, so an
     * imported project can never inherit identity from the file it came from
     * (§6.2.6). `storeName` is cleared on purpose — nothing is in the host store
     * yet, so a later Save must go through Save As rather than silently
     * overwriting a stored project that happens to share the name.
     */
    importUstx(name, ustx, mediaFiles = []) {
      return lifecycle('import', async () => {
        pendingNotes.clear();
        await backend.importUstx(ustx, name);
        state.storeName = null;
        state.documentName = name;

        const tracks = backend.state?.snapshot?.project?.tracks || [];
        const mediaResult = await importWaveMediaFiles(tracks, mediaFiles, {
          registerMedia: (mediaId, bytes) => registerMediaDuringLifecycle(mediaId, bytes),
          apply: async command => {
            state.lastChange = await backend.apply(command);
            return state.lastChange;
          },
        });
        return { name, ...mediaResult };
      });
    },
    importProjectBundle(name, bundle) {
      if (!backend) return Promise.reject(new Error('No project session is open'));
      return lifecycle('bundle-import', async () => {
        if (!bundle || typeof bundle.ustx !== 'string' || !Array.isArray(bundle.media)) {
          throw new TypeError('The project bundle is invalid');
        }
        pendingNotes.clear();
        await backend.importUstx(bundle.ustx, name);
        state.storeName = null;
        state.documentName = name;

        const tracks = backend.state?.snapshot?.project?.tracks || [];
        const mediaResult = await restoreProjectBundleMedia(tracks, bundle.media, {
          registerMedia: (mediaId, bytes) => registerMediaDuringLifecycle(mediaId, bytes),
          apply: async command => {
            state.lastChange = await backend.apply(command);
            return state.lastChange;
          },
        });
        return { name, ...mediaResult };
      });
    },
    async save(name = state.storeName) {
      if (!name) throw new Error('This project has never been saved — use Save As first');
      return lifecycle('save', async () => {
        const result = await backend.save(name);
        state.storeName = result?.name ?? name;
        return result;
      });
    },
    saveAs(name) {
      return lifecycle('save-as', async () => {
        const result = await backend.saveAs(name);
        state.storeName = result?.name ?? name;
        return result;
      });
    },
    /**
     * Render one track through the engine (§6.13).
     *
     * `track` is the track NUMBER the engine addresses, not a UI list index.
     * Both backends answer `{ sampleRate, samples }`, so callers need no
     * backend branch; `options` (voicebank, engine, onProgress) is passed
     * through because only some hosts accept them.
     *
     * This is deliberately NOT queued through `enqueue`: a render is a read of
     * a revision snapshot, not a document mutation, so it must not sit behind
     * pending edits or report itself as part of one.
     */
    renderTrack(track, options = {}) {
      if (!backend) return Promise.reject(new Error('No project session is open'));
      return backend.renderTrack(track, options);
    },
    /** Whole-project stereo mixdown, rendered from the same Rust session. */
    renderMixdown(options = {}) {
      if (!backend) return Promise.reject(new Error('No project session is open'));
      return backend.renderMixdown(options);
    },
    /** Fetch worker-owned PCM for streaming playback; WASM buffers stay off React. */
    getPlaybackTake(takeId) {
      if (!backend?.getPlaybackTake) return Promise.reject(new Error('No worker-owned take is available'));
      return backend.getPlaybackTake(takeId);
    },
    /** Encode worker-owned PCM to PCM16 WAV without cloning float channels to React. */
    encodeTakeWav(takeId) {
      if (!backend?.encodeTakeWav) return Promise.reject(new Error('No worker-owned take is available'));
      return backend.encodeTakeWav(takeId);
    },
    /** Register audio with the host, then let Rust create its wave part. */
    importAudio({ mediaId, bytes, trackId, name, positionTicks }) {
      if (!backend) return Promise.reject(new Error('No project session is open'));
      return enqueue(async () => {
        const media = await backend.registerMedia(mediaId, bytes);
        const result = await backend.apply({
          type: 'part.import_audio',
          track_id: trackId,
          name,
          position_ticks: Math.max(0, Math.round(positionTicks)),
          media_ref: media.media_ref,
          duration_ms: media.duration_ms,
        });
        state.lastChange = result;
        await refresh('edit', result);
        return result;
      });
    },
    relinkAudio({ partId, mediaId, bytes }) {
      if (!backend) return Promise.reject(new Error('No project session is open'));
      return enqueue(async () => {
        const media = await backend.registerMedia(mediaId, bytes);
        const result = await backend.apply({
          type: 'part.set_media_ref',
          part_id: partId,
          media_ref: media.media_ref,
        });
        state.lastChange = result;
        await refresh('edit', result);
        return result;
      });
    },
    importTracks(ustxYaml) {
      if (!backend) return Promise.reject(new Error('No project session is open'));
      return enqueue(async () => {
        const result = await backend.apply({ type: 'project.import_tracks', ustx_yaml: ustxYaml });
        state.lastChange = result;
        await refresh('edit', result);
        return result;
      });
    },
    /** Portable USTX text; the host decides what to do with it (download, …). */
    exportUstx() { return backend ? backend.exportUstx() : Promise.reject(new Error('No project session is open')); },
    /** A versioned ZIP carrying USTX plus audio bytes available in host storage. */
    async exportProjectBundle() {
      if (!backend) throw new Error('No project session is open');
      const host = backend;
      const tracks = host.state?.snapshot?.project?.tracks || [];
      const ustx = await host.exportUstx();
      return createProjectBundle({
        name: state.documentName || state.storeName || 'Untitled',
        ustx,
        tracks,
        loadMedia: mediaId => host.store.loadMedia(mediaId),
      });
    },
    listProjects() { return backend ? backend.listProjects() : Promise.resolve([]); },

    async close() { disposed = true; await backend?.close?.(); },
  };
  // Dev-only handle so a probe or the browser console can read the transaction
  // log (and anything else on the engine) without instrumenting the panel code.
  if (import.meta.env?.DEV) window.__ouEngine = api;
  return api;
}

/**
 * A new session starts empty, so the clone seeds a short phrase to edit. This
 * is still engine state: every note goes through `note.add`.
 */
async function seedNotes(backend) {
  const track = backend.state?.snapshot?.project?.tracks?.[0];
  if (!track) return;
  const created = await backend.apply({ type: 'part.create', track_id: track.id, kind: 'voice', name: 'New Part', position_ticks: 0, duration_ticks: TICKS_PER_BEAT * 4 * 14 });
  const partId = created?.changes?.find(change => change.kind === 'part.added')?.part_id
    || backend.state?.snapshot?.project?.tracks?.[0]?.parts?.find(part => part.kind === 'voice')?.id;
  if (!partId) return;
  // The dev bank (`web/fixture/bank`) is an English CVVC fixture with no kana
  // aliases, so a kana seed renders to silence and the waveform strip has
  // nothing to show. These are the words the fixture is built for
  // (`web/fixture/demo-song.ustx`), written in the engine's `word[phonemes]`
  // lyric form — each hint is a single-token alias of that bank.
  const phrase = [['see[s i]', 1, 66], ['led[l e d]', 2, 66], ['ah[3]', 3, 68], ['la[l A]', 4, 70]];
  for (const [lyric, beat, tone] of phrase) {
    await backend.apply({ type: 'note.add', part_id: partId, position_ticks: beat * TICKS_PER_BEAT, tone, duration_ticks: TICKS_PER_BEAT, lyric });
  }
}
