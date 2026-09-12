// Editor shell: wires the DOM chrome to the driver and the roll.
//
// Layout follows ui-mock/index.html (the project's designated UI reference):
// rail | topbar / toolbar / [left panel | roll | inspector] / status / transport.

import { Driver } from "./driver.js";
import { PianoRoll, TOOL } from "./roll.js";
import { HttpBank, OpfsBank } from "../samples.js";
import { RenderPool } from "../pool.js";

const $ = (id) => document.getElementById(id);

export class Editor {
  constructor() {
    this.driver = null;
    this.roll = null;
    this.pool = null;
    this.bank = null;
    this.track = 0;
    this.playing = false;
    /** Wav keys already installed in the engine. */
    this._loaded = new Set();
    this.audio = { ctx: null, source: null, startedAt: 0, offsetSec: 0, buffer: null };
    this.renderTimer = null;
    this.autoRender = true;
  }

  async boot() {
    this.driver = await Driver.boot();
    // The dev fixture is a self-contained subset bank: its oto.ini lists only
    // aliases whose wavs ship alongside it, so `keys` is exactly what exists.
    this.bank = new HttpBank({ baseUrl: "./fixture/bank" });
    const keys = await this.bank.openOto({ otoPath: "voice/oto.ini", otoDir: "voice" });
    await this.bank.prefetch(keys);
    const aliases = this.driver.setVoicebank("demo", "Teto (demo)", "voice", this.bank.otoBytes);
    this.log(`engine: wasm · fft ${this.driver.fftBackend()} · oto ${aliases} aliases · ${keys.length} wav`);

    // Load the demo project.
    const yaml = await (await fetch("./fixture/scale-demo.ustx")).text();
    this.driver.loadYaml(yaml);

    // Build the roll BEFORE the first refresh: `refresh()` is what pushes the
    // notes into it, so refreshing first left the roll holding an empty list
    // (the canvas drew grid only and hit-testing found nothing).
    this._buildRoll();
    this._bindChrome();
    this.refresh();
    this.log("พร้อม — กด Play เพื่อฟัง (ครั้งแรกจะ render)");
    $("status").textContent = "พร้อม";
  }

  log(msg) {
    const el = $("log");
    el.textContent += (el.textContent ? "\n" : "") + msg;
    el.scrollTop = el.scrollHeight;
  }

  // ---- views ------------------------------------------------------------

  refresh() {
    const meta = this.driver.meta();
    const tracks = this.driver.tracks();
    const notes = this.driver.notes(this.track);

    $("crumbName").textContent = meta.name || "(untitled)";
    $("bpmChip").textContent = `${meta.bpm} BPM`;
    $("meterChip").textContent = `${meta.beat_per_bar}/${meta.beat_unit}`;
    $("trackCount").textContent = `${tracks.length} track${tracks.length === 1 ? "" : "s"}`;
    $("noteCount").textContent = `${notes.length} notes`;
    this.renderTrackList(tracks);
    this.renderInspector(notes);

    if (this.roll) {
      this.roll.track = this.track;
      this.roll.setNotes(notes);
      this.roll.drawKeys($("keys"));
    }
    $("undoBtn").disabled = !this.driver.canUndo();
    $("redoBtn").disabled = !this.driver.canRedo();
    this.scheduleRender();
  }

  renderTrackList(tracks) {
    const el = $("trackList");
    el.innerHTML = "";
    for (const t of tracks) {
      const row = document.createElement("div");
      row.className = `track${t.index === this.track ? " selected" : ""}`;
      row.innerHTML = `<span class="swatch"></span>
        <span class="tname">${escapeHtml(t.name || `Track ${t.index + 1}`)}</span>
        <button class="mini" data-mute="${t.index}" title="Mute">${t.mute ? "M" : "·"}</button>
        <button class="mini" data-solo="${t.index}" title="Solo">${t.solo ? "S" : "·"}</button>`;
      row.addEventListener("click", (e) => {
        if (e.target.dataset.mute !== undefined) {
          this.driver.apply({ cmd: "set_track_mute", track: t.index, mute: !t.mute });
          this.refresh();
          return;
        }
        if (e.target.dataset.solo !== undefined) {
          this.driver.apply({ cmd: "set_track_solo", track: t.index, solo: !t.solo });
          this.refresh();
          return;
        }
        this.track = t.index;
        this._framed = false;
        if (this.roll) this.roll._framed = false;
        this.refresh();
      });
      el.appendChild(row);
    }
  }

  renderInspector(notes) {
    const sel = this.roll ? [...this.roll.selection] : [];
    const el = $("inspector");
    if (!sel.length) {
      el.innerHTML = `<div class="muted">ไม่ได้เลือก note</div>
        <div class="muted small">${notes.length} notes ใน track นี้</div>`;
      return;
    }
    const n = notes[sel[0]];
    if (!n) return;
    el.innerHTML = `
      <div class="field"><label>Lyric</label>
        <input id="inspLyric" value="${escapeAttr(n.lyric)}"></div>
      <div class="field"><label>Tone</label>
        <input id="inspTone" type="number" value="${n.tone}"></div>
      <div class="field"><label>Position</label>
        <input id="inspPos" type="number" step="120" value="${n.position}"></div>
      <div class="field"><label>Duration</label>
        <input id="inspDur" type="number" step="120" value="${n.duration}"></div>
      <div class="muted small">${sel.length} selected${sel.length > 1 ? " (inspector shows the first)" : ""}</div>`;
    const commit = (cmd) => {
      this.driver.apply(cmd);
      this.refresh();
    };
    $("inspLyric").addEventListener("change", (e) =>
      commit({ cmd: "change_lyric", part: n.part, note: n.index, lyric: e.target.value }));
    $("inspTone").addEventListener("change", (e) =>
      commit({ cmd: "change_tone", part: n.part, note: n.index, tone: Number(e.target.value) }));
    $("inspPos").addEventListener("change", (e) =>
      commit({ cmd: "move_note", part: n.part, note: n.index,
               delta_pos: Number(e.target.value) - n.position, delta_tone: 0 }));
    $("inspDur").addEventListener("change", (e) =>
      commit({ cmd: "resize_note", part: n.part, note: n.index,
               delta_dur: Number(e.target.value) - n.duration, from_start: false }));
  }

  _buildRoll() {
    const canvas = $("roll");
    this.roll = new PianoRoll(canvas, {
      onCommand: (cmd) => this.onCommand(cmd),
      getPhonemes: (note) => splitPhonemes(note.lyric),
    });
    this.roll.drawKeys($("keys"));
    this._bindKeysPointer();
  }

  /** The key strip doubles as a click target for auditioning a pitch. */
  _bindKeysPointer() {
    $("keys").addEventListener("pointerdown", (e) => {
      const r = $("keys").getBoundingClientRect();
      const tone = Math.floor(this.roll.viewport.tone(e.clientY - r.top));
      this.log(`tone ${tone} (${toneNameSafe(tone)})`);
    });
  }

  // ---- commands ---------------------------------------------------------

  onCommand(cmd) {
    try {
      if (cmd.batch) {
        this.driver.applyGroup(cmd.name || "Edit", cmd.batch);
        this.log(`${cmd.name || "Edit"} (${cmd.batch.length} notes)`);
      } else {
        const name = this.driver.apply(cmd);
        this.log(name || cmd.cmd);
      }
      this.refresh();
    } catch (err) {
      this.log(`ปฏิเสธ: ${err}`);
      // Re-read from Rust: the preview may have drifted from the truth.
      this.refresh();
    }
  }

  // ---- chrome -----------------------------------------------------------

  _bindChrome() {
    for (const [id, tool] of [
      ["toolSelect", TOOL.SELECT],
      ["toolDraw", TOOL.DRAW],
      ["toolErase", TOOL.ERASE],
      ["toolSplit", TOOL.SPLIT],
    ]) {
      $(id).addEventListener("click", () => {
        for (const other of ["toolSelect", "toolDraw", "toolErase", "toolSplit"]) {
          $(other).classList.toggle("active", other === id);
        }
        this.roll.setTool(tool);
        this.log(`tool: ${tool}`);
      });
    }
    $("snapSel").addEventListener("change", (e) => {
      const v = e.target.value;
      if (v === "off") {
        this.roll.snap = false;
      } else {
        this.roll.snap = true;
        this.roll.setDivision(Number(v));
      }
    });
    $("zoomX").addEventListener("input", (e) => {
      this.roll.viewport.beatW = Number(e.target.value);
      this.roll.draw();
    });
    $("zoomY").addEventListener("input", (e) => {
      this.roll.viewport.rowH = Number(e.target.value);
      this.roll.draw();
      this.roll.drawKeys($("keys"));
    });
    $("undoBtn").addEventListener("click", () => {
      const n = this.driver.undo();
      this.log(`undo: ${n || "—"}`);
      this.refresh();
    });
    $("redoBtn").addEventListener("click", () => {
      const n = this.driver.redo();
      this.log(`redo: ${n || "—"}`);
      this.refresh();
    });
    $("playBtn").addEventListener("click", () => this.togglePlay());
    $("stopBtn").addEventListener("click", () => this.stop());
    $("renderBtn").addEventListener("click", () => this.renderNow(true));
    $("fxBtn").addEventListener("click", () => {
      this.roll.showChunks = !this.roll.showChunks;
      $("fxBtn").classList.toggle("active", this.roll.showChunks);
      this.roll.draw();
    });
  }

  // ---- rendering --------------------------------------------------------

  scheduleRender() {
    if (!this.autoRender) return;
    clearTimeout(this.renderTimer);
    this.renderTimer = setTimeout(() => this.renderNow(false), 250);
  }

  /** Make sure every sample the current plan references is loaded. */
  async ensureSamples() {
    const needed = new Set();
    const n = this.driver.plan(this.track).length;
    for (let i = 0; i < n; i++) {
      for (const k of this.driver.chunkWavKeys(i)) needed.add(k);
    }
    const missing = [...needed].filter((k) => !this._loaded.has(k));
    if (!missing.length) return 0;
    const files = await this.bank.load(missing);
    for (const f of files) {
      this.driver.addSample(f.key, f.wav, f.frq);
      this._loaded.add(f.key);
    }
    return missing.length;
  }

  /** Render dirty chunks through the pool (falls back to this thread). */
  async renderNow(force) {
    if (!this.driver) return;
    const t0 = performance.now();
    try {
      const meta = this.driver.plan(this.track);
      $("chunkInfo").textContent = `${meta.length} chunks`;
      const skipped = this.driver.planSkipped();
      if (skipped.length) this.log(`ข้าม: ${skipped.join("; ")}`);

      const loaded = await this.ensureSamples();
      if (loaded) this.log(`โหลด sample เพิ่ม ${loaded} (รวม ${this._loaded.size})`);

      const pcm = this.driver.renderAll();
      const ms = performance.now() - t0;
      const sec = pcm.length / 44100;
      this.audio.buffer = pcm;
      this.renderChunks(meta);
      $("status").textContent = `render ${ms.toFixed(0)} ms · ${sec.toFixed(2)}s audio`;
      if (force) {
        this.log(`render: ${pcm.length} samples (${sec.toFixed(2)}s) ใน ${ms.toFixed(0)} ms`);
      }
    } catch (err) {
      this.log(`render error: ${err}`);
      $("status").textContent = "render ล้มเหลว";
    }
  }

  /** Show the planned chunks as overlay marks (ms → ticks). */
  renderChunks(meta) {
    const bpm = this.driver.meta().bpm || 120;
    const msPerTick = 60000 / bpm / 480;
    this.roll.setChunks(
      meta.map((c) => ({
        start_tick: c.start_ms / msPerTick,
        end_tick: c.end_ms / msPerTick,
        state: "done",
      })),
    );
  }

  // ---- playback (Web Audio) --------------------------------------------

  async togglePlay() {
    if (this.playing) return this.stop();
    if (!this.audio.buffer) await this.renderNow(false);
    if (!this.audio.buffer) return;
    if (!this.audio.ctx) this.audio.ctx = new AudioContext();
    const ctx = this.audio.ctx;
    await ctx.resume();

    const pcm = this.audio.buffer;
    const buf = ctx.createBuffer(1, pcm.length, 44100);
    buf.copyToChannel(pcm, 0);
    const src = ctx.createBufferSource();
    src.buffer = buf;
    src.connect(ctx.destination);
    src.onended = () => {
      if (this.playing) this.stop();
    };
    src.start();
    this.audio.source = src;
    this.audio.startedAt = ctx.currentTime;
    this.playing = true;
    $("playBtn").textContent = "⏸";
    this._tickPlayhead();
  }

  stop() {
    this.playing = false;
    if (this.audio.source) {
      try {
        this.audio.source.onended = null;
        this.audio.source.stop();
      } catch {}
      this.audio.source = null;
    }
    $("playBtn").textContent = "▶";
    this.roll.setPlayheadTick(null);
  }

  /** Drive the playhead from the audio clock (immune to render latency). */
  _tickPlayhead() {
    if (!this.playing) return;
    const ctx = this.audio.ctx;
    const sec = ctx.currentTime - this.audio.startedAt;
    const bpm = this.driver.meta().bpm || 120;
    const tick = (sec * 1000) / (60000 / bpm / 480);
    this.roll.setPlayheadTick(tick);
    requestAnimationFrame(() => this._tickPlayhead());
  }
}

// ---- helpers ------------------------------------------------------------

function escapeHtml(s) {
  return String(s).replace(/[&<>"]/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
}
function escapeAttr(s) {
  return escapeHtml(s).replace(/'/g, "&#39;");
}
function toneNameSafe(tone) {
  const names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
  return `${names[((tone % 12) + 12) % 12]}${Math.floor(tone / 12) - 1}`;
}
/** Split a `la[l A]` lyric into its phoneme tokens. */
function splitPhonemes(lyric) {
  const m = /\[([^\]]*)\]/.exec(lyric || "");
  if (!m) return [];
  return m[1].split(/\s+/).filter(Boolean);
}
