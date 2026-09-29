// Piano roll: canvas rendering + pointer interaction.
//
// Rendering is layered into one canvas per concern so a cheap change (the
// playhead) does not repaint the notes. Interaction is a small state machine
// that ends in a command handed back to the caller — this module never talks
// to the engine directly, which keeps it testable.

import {
  Viewport,
  TICKS_PER_BEAT,
  toneName,
  isBlackKey,
  snapTick,
  clamp,
} from "./viewport.js";

export const TOOL = {
  SELECT: "select",
  DRAW: "draw",
  ERASE: "erase",
  SPLIT: "split",
};

const COLORS = {
  bg: "#141416",
  rowWhite: "#1c1c1f",
  rowBlack: "#171719",
  rowLine: "#26262b",
  octaveLine: "#3a3a42",
  beatLine: "#2c2c33",
  barLine: "#4a4a54",
  note: "#fe71a3",
  noteEdge: "#cc2a63",
  noteSel: "#ffffff",
  noteText: "#1a1a1a",
  phoneme: "#8fd0ff",
  keyWhite: "#d8d8de",
  keyBlack: "#2a2a30",
  keyText: "#6b6b78",
  playhead: "#55d7de",
  chunk: "#3b3652",
  chunkDone: "#2f4a3a",
  chunkRendering: "#6d4a24",
};

/** Hit target width for a note's resize edges, in px. */
const EDGE_PX = 6;
/** Drag distance before a press becomes a drag (px). */
const DRAG_SLOP = 4;

export class PianoRoll {
  /**
   * @param {HTMLCanvasElement} canvas
   * @param {object} opts
   * @param {Function} opts.onCommand   called with a command object
   * @param {Function} [opts.getPhonemes] `(note) => string[]` for labels
   */
  constructor(canvas, { onCommand, getPhonemes } = {}) {
    this.canvas = canvas;
    this.ctx = canvas.getContext("2d");
    this.viewport = new Viewport();
    this.onCommand = onCommand || (() => {});
    this.getPhonemes = getPhonemes || null;

    this.notes = [];
    this.track = 0;
    this.tool = TOOL.SELECT;
    this.division = 4; // snap: 480/4 = 120 ticks
    this.snap = true;

    /** Selected note indices (multi-select). */
    this.selection = new Set();
    /** Chunk marks for the FX overlay: [{start_ms,end_ms,state}] */
    this.chunks = [];
    this.showChunks = false;
    /** Playhead position in ticks, or null. */
    this.playheadTick = null;

    this.drag = null;
    this.hover = null;
    this.penSeen = false;

    this._bind();
    this._resizeObserver = new ResizeObserver(() => this.resize());
    this._resizeObserver.observe(canvas.parentElement || canvas);
    this.resize();
  }

  // ---- data -------------------------------------------------------------

  setNotes(notes) {
    this.notes = notes || [];
    // Drop selection entries that no longer exist.
    for (const i of [...this.selection]) {
      if (i >= this.notes.length) this.selection.delete(i);
    }
    if (!this._framed && this.notes.length) {
      this.viewport.frameNotes(this.notes);
      this._framed = true;
    }
    this.draw();
  }

  setChunks(chunks) {
    this.chunks = chunks || [];
    if (this.showChunks) this.draw();
  }

  setPlayheadTick(tick) {
    this.playheadTick = tick;
    this.drawOverlay();
  }

  setTool(tool) {
    this.tool = tool;
    this.canvas.style.cursor = tool === TOOL.DRAW ? "crosshair" : "default";
    this.draw();
  }

  setDivision(division) {
    this.division = division;
  }

  // ---- geometry ---------------------------------------------------------

  resize() {
    const host = this.canvas.parentElement || this.canvas;
    const dpr = window.devicePixelRatio || 1;
    const w = Math.max(1, host.clientWidth);
    const h = Math.max(1, host.clientHeight);
    this.canvas.width = Math.floor(w * dpr);
    this.canvas.height = Math.floor(h * dpr);
    this.canvas.style.width = `${w}px`;
    this.canvas.style.height = `${h}px`;
    this.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    this.viewport.resize(w, h);
    this.viewport.clampPan();
    this.draw();
  }

  /** Note rectangle in screen px. */
  noteRect(note) {
    const vp = this.viewport;
    const x = vp.x(note.position);
    const w = Math.max(3, (note.duration / TICKS_PER_BEAT) * vp.beatW - 1);
    const y = vp.y(note.tone);
    const h = vp.rowHeight() - 1;
    return { x, y, w, h };
  }

  /** Note index under a screen point, or -1. */
  noteAt(px, py) {
    // Reverse order so the topmost (last drawn) wins.
    for (let i = this.notes.length - 1; i >= 0; i--) {
      const r = this.noteRect(this.notes[i]);
      if (px >= r.x && px <= r.x + r.w && py >= r.y && py <= r.y + r.h) return i;
    }
    return -1;
  }

  /** `"left" | "right" | null` — which resize edge is under the point. */
  edgeAt(px, py) {
    const i = this.noteAt(px, py);
    if (i < 0) return { index: -1, edge: null };
    const r = this.noteRect(this.notes[i]);
    if (px - r.x <= EDGE_PX) return { index: i, edge: "left" };
    if (r.x + r.w - px <= EDGE_PX) return { index: i, edge: "right" };
    return { index: i, edge: null };
  }

  /** Tone under a screen y. */
  toneAt(py) {
    return Math.floor(this.viewport.tone(py));
  }

  // ---- interaction ------------------------------------------------------

  _bind() {
    const c = this.canvas;
    c.addEventListener("pointerdown", (e) => this._down(e));
    c.addEventListener("pointermove", (e) => this._move(e));
    c.addEventListener("pointerup", (e) => this._up(e));
    c.addEventListener("pointercancel", () => this._cancel());
    c.addEventListener("wheel", (e) => this._wheel(e), { passive: false });
    c.addEventListener("contextmenu", (e) => e.preventDefault());
    c.addEventListener("pointerleave", () => {
      this.hover = null;
      this.draw();
    });
  }

  /** Local canvas coordinates for a pointer event. */
  _pt(e) {
    const r = this.canvas.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  }

  _down(e) {
    // Pen beats finger: once a stylus is seen, a finger can only pan/zoom.
    // (The design doc's pen-priority rule.)
    if (e.pointerType === "pen") this.penSeen = true;
    const fingerPan = this.penSeen && e.pointerType === "touch";
    if (fingerPan) {
      this.drag = { kind: "pan", x: e.clientX, y: e.clientY };
      this.canvas.setPointerCapture(e.pointerId);
      return;
    }
    this.canvas.setPointerCapture(e.pointerId);
    const { x, y } = this._pt(e);

    if (e.button === 1 || e.shiftKey) {
      this.drag = { kind: "pan", x: e.clientX, y: e.clientY };
      return;
    }

    const hit = this.edgeAt(x, y);

    if (this.tool === TOOL.ERASE) {
      if (hit.index >= 0) {
        this.onCommand({
          cmd: "remove_note",
          part: this.notes[hit.index].part,
          note: this.notes[hit.index].index,
        });
      }
      return;
    }

    if (this.tool === TOOL.SPLIT) {
      if (hit.index >= 0) this._split(hit.index, x);
      return;
    }

    if (this.tool === TOOL.DRAW) {
      const tick = snapTick(this.viewport.tick(x), this.division, this.snap);
      const tone = this.toneAt(y);
      const dur = TICKS_PER_BEAT / this.division;
      this.onCommand({
        cmd: "add_note",
        part: this.notes[0]?.part ?? 0,
        tone,
        position: Math.max(0, tick),
        duration: dur,
        lyric: "la",
      });
      return;
    }

    // SELECT tool
    if (hit.index < 0) {
      if (!e.shiftKey) this.selection.clear();
      this.drag = { kind: "marquee", x0: x, y0: y, x1: x, y1: y };
      this.draw();
      return;
    }

    if (!this.selection.has(hit.index)) {
      if (!e.shiftKey) this.selection.clear();
      this.selection.add(hit.index);
    }
    const note = this.notes[hit.index];
    const tick0 = this.viewport.tick(x);
    const tone0 = this.toneAt(y);
    this.drag = {
      kind: hit.edge ? "resize" : "move",
      edge: hit.edge,
      index: hit.index,
      tick0,
      tone0,
      noteTick0: note.position,
      noteTone0: note.tone,
      noteDur0: note.duration,
      startX: x,
      startY: y,
      moved: false,
      // Every note in the selection moves together.
      members: [...this.selection].map((i) => ({
        index: i,
        part: this.notes[i].part,
        note: this.notes[i].index,
        tick: this.notes[i].position,
        tone: this.notes[i].tone,
        dur: this.notes[i].duration,
      })),
    };
    this.draw();
  }

  _move(e) {
    if (this.drag && this.drag.kind === "pan") {
      this.viewport.scrollBy(
        this.drag.x - e.clientX,
        this.drag.y - e.clientY,
      );
      this.drag.x = e.clientX;
      this.drag.y = e.clientY;
      this.draw();
      return;
    }

    const { x, y } = this._pt(e);

    if (!this.drag) {
      this.hover = { index: this.noteAt(x, y) };
      this.drawOverlay();
      return;
    }

    if (this.drag.kind === "marquee") {
      this.drag.x1 = x;
      this.drag.y1 = y;
      this._applyMarquee();
      this.draw();
      return;
    }

    const dx = Math.abs(x - this.drag.startX);
    const dy = Math.abs(y - this.drag.startY);
    if (!this.drag.moved && dx < DRAG_SLOP && dy < DRAG_SLOP) return;
    this.drag.moved = true;

    const dTick = this.viewport.tick(x) - this.drag.tick0;
    const dTone = this.toneAt(y) - this.drag.tone0;

    // Preview: move every member by the (snapped) delta.
    for (const m of this.drag.members) {
      const n = this.notes[m.index];
      if (this.drag.kind === "resize") {
        if (this.drag.edge === "right") {
          n.duration = Math.max(30, snapTick(m.dur + dTick, this.division, this.snap));
        } else {
          const start = snapTick(m.tick + dTick, this.division, this.snap);
          const delta = start - m.tick;
          n.position = Math.max(0, m.tick + delta);
          n.duration = Math.max(30, m.dur - delta);
        }
      } else {
        n.position = Math.max(0, snapTick(m.tick + dTick, this.division, this.snap));
        n.tone = m.tone + dTone;
      }
    }
    this.draw();
  }

  _up(e) {
    const drag = this.drag;
    this.drag = null;
    if (!drag) return;

    if (drag.kind === "marquee") {
      this.draw();
      return;
    }
    if (drag.kind === "pan" || !drag.moved) {
      this.draw();
      return;
    }

    // Commit: one command per changed note, grouped into a single undo step.
    const cmds = [];
    for (const m of drag.members) {
      const n = this.notes[m.index];
      if (drag.kind === "resize") {
        const dDur = n.duration - m.dur;
        if (this.drag?.edge === "left" || drag.edge === "left") {
          const dPos = n.position - m.tick;
          if (dPos !== 0 || dDur !== 0) {
            cmds.push({
              cmd: "resize_note",
              part: m.part,
              note: m.note,
              delta_dur: -dPos + dDur - (n.position - m.tick) + (n.position - m.tick),
              from_start: true,
            });
            // `resize_note` with from_start moves the start by delta_dur, so
            // the requested start change is expressed as a duration delta.
            cmds[cmds.length - 1].delta_dur = dDur + (m.tick - n.position);
          }
        } else if (dDur !== 0) {
          cmds.push({
            cmd: "resize_note",
            part: m.part,
            note: m.note,
            delta_dur: dDur,
            from_start: false,
          });
        }
      } else {
        const dPos = n.position - m.tick;
        const dTone = n.tone - m.tone;
        if (dPos !== 0 || dTone !== 0) {
          cmds.push({
            cmd: "move_note",
            part: m.part,
            note: m.note,
            delta_pos: dPos,
            delta_tone: dTone,
          });
        }
      }
    }

    if (cmds.length) {
      this.onCommand({ batch: cmds, name: drag.kind === "resize" ? "Resize note" : "Move note" });
    } else {
      this.draw();
    }
  }

  _cancel() {
    this.drag = null;
    this.draw();
  }

  _split(index, px) {
    const note = this.notes[index];
    const r = this.noteRect(note);
    const at = snapTick(this.viewport.tick(px), this.division, this.snap);
    const left = at - note.position;
    if (left < 30 || note.duration - left < 30) return;
    // Split = resize the original + add the tail note, as one undo step.
    this.onCommand({
      batch: [
        { cmd: "resize_note", part: note.part, note: note.index, delta_dur: left - note.duration, from_start: false },
        {
          cmd: "add_note",
          part: note.part,
          tone: note.tone,
          position: at,
          duration: note.duration - left,
          lyric: note.lyric,
        },
      ],
      name: "Split note",
    });
  }

  _applyMarquee() {
    const d = this.drag;
    const x0 = Math.min(d.x0, d.x1);
    const x1 = Math.max(d.x0, d.x1);
    const y0 = Math.min(d.y0, d.y1);
    const y1 = Math.max(d.y0, d.y1);
    this.selection.clear();
    for (let i = 0; i < this.notes.length; i++) {
      const r = this.noteRect(this.notes[i]);
      if (r.x + r.w >= x0 && r.x <= x1 && r.y + r.h >= y0 && r.y <= y1) {
        this.selection.add(i);
      }
    }
  }

  _wheel(e) {
    e.preventDefault();
    if (e.ctrlKey || e.metaKey) {
      const { x, y } = this._pt(e);
      const factor = e.deltaY < 0 ? 1.12 : 1 / 1.12;
      this.viewport.zoomAt(x, y, factor, "both");
    } else if (e.shiftKey) {
      this.viewport.scrollBy(e.deltaY, 0);
    } else {
      this.viewport.scrollBy(e.deltaX, e.deltaY);
    }
    this.draw();
  }

  // ---- rendering --------------------------------------------------------

  draw() {
    this.drawBackground();
    this.drawNotes();
    this.drawOverlay();
  }

  drawBackground() {
    const ctx = this.ctx;
    const vp = this.viewport;
    const { width: w, height: h } = vp;
    ctx.fillStyle = COLORS.bg;
    ctx.fillRect(0, 0, w, h);

    // Pitch rows.
    const tones = vp.visibleTones();
    for (let tone = tones.from; tone <= tones.to; tone++) {
      const y = vp.y(tone);
      const rh = vp.rowHeight();
      ctx.fillStyle = isBlackKey(tone) ? COLORS.rowBlack : COLORS.rowWhite;
      ctx.fillRect(0, y, w, rh);
      if (tone % 12 === 0) {
        ctx.fillStyle = COLORS.octaveLine;
        ctx.fillRect(0, y + rh - 1, w, 1);
      }
    }

    // Beat / bar lines.
    const ticks = vp.visibleTicks();
    const firstBeat = Math.floor(ticks.from / TICKS_PER_BEAT);
    const lastBeat = Math.ceil(ticks.to / TICKS_PER_BEAT);
    for (let beat = firstBeat; beat <= lastBeat; beat++) {
      const x = vp.x(beat * TICKS_PER_BEAT);
      const isBar = beat % 4 === 0;
      ctx.fillStyle = isBar ? COLORS.barLine : COLORS.beatLine;
      ctx.fillRect(Math.round(x), 0, 1, h);
    }
  }

  drawNotes() {
    const ctx = this.ctx;
    const vp = this.viewport;
    const rh = vp.rowHeight();

    // Chunk overlay sits under the notes.
    if (this.showChunks && this.chunks.length) {
      for (const c of this.chunks) {
        const x = vp.x(c.start_tick);
        const w = Math.max(2, ((c.end_tick - c.start_tick) / TICKS_PER_BEAT) * vp.beatW);
        ctx.fillStyle =
          c.state === "done" ? COLORS.chunkDone
          : c.state === "rendering" ? COLORS.chunkRendering
          : COLORS.chunk;
        ctx.globalAlpha = 0.55;
        ctx.fillRect(x, 0, w, ctx.canvas.height / (window.devicePixelRatio || 1));
        ctx.globalAlpha = 1;
        ctx.fillStyle = COLORS.barLine;
        ctx.fillRect(Math.round(x), 0, 1, ctx.canvas.height);
      }
    }

    for (let i = 0; i < this.notes.length; i++) {
      const n = this.notes[i];
      const r = this.noteRect(n);
      if (r.x + r.w < 0 || r.x > vp.width) continue;
      if (r.y + r.h < 0 || r.y > vp.height) continue;

      const selected = this.selection.has(i);
      ctx.fillStyle = COLORS.note;
      ctx.fillRect(r.x, r.y, r.w, r.h);
      ctx.strokeStyle = selected ? COLORS.noteSel : COLORS.noteEdge;
      ctx.lineWidth = selected ? 2 : 1;
      ctx.strokeRect(r.x + 0.5, r.y + 0.5, r.w - 1, r.h - 1);

      // Lyric, then phoneme labels when the row is tall enough.
      if (r.w > 18 && rh >= 12) {
        ctx.save();
        ctx.beginPath();
        ctx.rect(r.x, r.y, r.w, r.h);
        ctx.clip();
        ctx.fillStyle = COLORS.noteText;
        ctx.font = `${Math.min(12, rh - 5)}px ui-sans-serif, system-ui, sans-serif`;
        ctx.textBaseline = "middle";
        ctx.fillText(n.lyric || "", r.x + 4, r.y + r.h / 2);
        if (this.getPhonemes && rh >= 22 && r.w > 40) {
          const ph = this.getPhonemes(n);
          if (ph && ph.length) {
            ctx.fillStyle = COLORS.phoneme;
            ctx.font = `${Math.min(10, rh - 12)}px ui-monospace, monospace`;
            ctx.fillText(ph.join(" "), r.x + 4, r.y + r.h - 6);
          }
        }
        ctx.restore();
      }
    }
  }

  drawOverlay() {
    const ctx = this.ctx;
    const vp = this.viewport;
    const h = ctx.canvas.height / (window.devicePixelRatio || 1);

    // Playhead.
    if (this.playheadTick != null) {
      const x = vp.x(this.playheadTick);
      if (x >= 0 && x <= vp.width) {
        ctx.fillStyle = COLORS.playhead;
        ctx.fillRect(Math.round(x), 0, 2, h);
      }
    }

    // Marquee.
    if (this.drag && this.drag.kind === "marquee") {
      const d = this.drag;
      const x = Math.min(d.x0, d.x1);
      const y = Math.min(d.y0, d.y1);
      const w = Math.abs(d.x1 - d.x0);
      const hh = Math.abs(d.y1 - d.y0);
      ctx.fillStyle = "rgba(85,215,222,0.15)";
      ctx.fillRect(x, y, w, hh);
      ctx.strokeStyle = COLORS.playhead;
      ctx.lineWidth = 1;
      ctx.strokeRect(x + 0.5, y + 0.5, w, hh);
    }
  }

  /** Draw the key strip into a separate canvas. */
  drawKeys(keyCanvas) {
    const ctx = keyCanvas.getContext("2d");
    const dpr = window.devicePixelRatio || 1;
    const vp = this.viewport;
    const w = keyCanvas.clientWidth || 60;
    const h = vp.height;
    keyCanvas.width = Math.floor(w * dpr);
    keyCanvas.height = Math.floor(h * dpr);
    keyCanvas.style.width = `${w}px`;
    keyCanvas.style.height = `${h}px`;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.fillStyle = COLORS.bg;
    ctx.fillRect(0, 0, w, h);

    const tones = vp.visibleTones();
    for (let tone = tones.from; tone <= tones.to; tone++) {
      const y = vp.y(tone);
      const rh = vp.rowHeight();
      if (y + rh < 0 || y > h) continue;
      const black = isBlackKey(tone);
      ctx.fillStyle = black ? COLORS.keyBlack : COLORS.keyWhite;
      ctx.fillRect(0, y, w, rh - 1);
      if (tone % 12 === 0 && rh >= 10) {
        ctx.fillStyle = COLORS.keyText;
        ctx.font = "10px ui-monospace, monospace";
        ctx.textBaseline = "middle";
        ctx.fillText(toneName(tone), 4, y + rh / 2);
      }
    }
  }
}
