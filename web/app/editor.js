// Editor shell: wires the DOM chrome to the driver and the roll.
//
// Web port of the Flutter editor shell; the piano roll remains canvas-backed.
// Layout: rail | app bar / tool row / [tracks | roll | inspector] / status / transport.

import { Driver } from "./driver.js";
import { PianoRoll, TOOL } from "./roll.js";
import { HttpBank, OpfsBank } from "../samples.js";
import { RenderPool } from "../pool.js";
import { ProjectStore, safeProjectName } from "./project-store.js";

const $ = (id) => document.getElementById(id);

export class Editor {
  constructor() {
    this.driver = null;
    this.roll = null;
    this.pool = null;
    this.bank = null;
    this.track = 0;
    this.renderEngine = "r1-rs";
    this.playing = false;
    /** Wav keys already installed in the engine. */
    this._loaded = new Set();
    this.audio = { ctx: null, source: null, startedAt: 0, offsetSec: 0, buffer: null, dirty: true };
    this.renderTimer = null;
    this.autoRender = true;
    /** Worker-pool init promise; the first render awaits it. */
    this._poolPromise = null;
    /** Monotonic render generation: a stale render never publishes audio. */
    this._epoch = 0;
    /** Cancel token handed to `pool.render()`; see `cancelRender()`. */
    this._renderToken = null;
    /** In-flight render, so Play can await it instead of racing it. */
    this._renderPromise = null;
    /** OPFS project store — null when the browser has no OPFS. */
    this.store = null;
    this.projectName = "scale-demo";
    /** YAML as last written to OPFS; autosave skips when nothing changed. */
    this._savedYaml = null;
    this._autosaveTimer = null;
  }

  async boot() {
    this.driver = await Driver.boot();
    // Use the fixture by default so the demo boots anywhere. The local full
    // Teto bank can be selected for real-phoneme testing with `?voicebank=full`.
    const useFullVoicebank = new URLSearchParams(location.search).get("voicebank") === "full";
    this.bank = new HttpBank({
      baseUrl: useFullVoicebank
        ? "../test/golden/teto-english/library"
        : "./fixture/bank",
    });
    const keys = await this.bank.openOto({ otoPath: "voice/oto.ini", otoDir: "voice" });
    // The full bank is 93 MB: let RenderPool fetch only the samples referenced
    // by chunks. Keep the small fixture warm for a snappy default demo.
    if (!useFullVoicebank) await this.bank.prefetch(keys);
    const aliases = this.driver.setVoicebank("demo", "Teto (demo)", "voice", this.bank.otoBytes);
    this.log(
      `engine: wasm · fft ${this.driver.fftBackend()} · oto ${aliases} aliases · ` +
      `${keys.length} wav${useFullVoicebank ? " · full Teto bank" : ""}`,
    );

    // ---- project source: OPFS (?project=<name> → last opened) → fixture ----
    // The bundled fixture stays the fallback so the editor always boots.
    this.store = (await ProjectStore.available()) ? new ProjectStore() : null;
    const wanted = new URLSearchParams(location.search).get("project");
    let name = wanted ? safeProjectName(wanted) : null;
    let yaml = null;
    if (this.store) {
      if (!name) name = await this.store.lastOpened();
      if (name) {
        try {
          yaml = await this.store.load(name);
        } catch {
          this.log(`ไม่พบโปรเจกต์ "${name}" ใน OPFS — เปิด demo แทน`);
          yaml = null;
        }
      }
    }
    if (!yaml) {
      yaml = await (await fetch("./fixture/scale-demo.ustx")).text();
      name = name || "scale-demo";
    }
    this.projectName = safeProjectName(name);
    this.driver.loadYaml(yaml);
    this._savedYaml = this.driver.yaml();

    // Build the roll BEFORE the first refresh: `refresh()` is what pushes the
    // notes into it, so refreshing first left the roll holding an empty list
    // (the canvas drew grid only and hit-testing found nothing).
    this._buildRoll();
    this._bindChrome();
    // Start the worker pool in the background — boot must not wait for it, and
    // a machine without Workers falls back to main-thread rendering.
    this._poolPromise = this._initPool();
    this.refresh();
    this.renderProjectList();
    $("projectState").textContent = this.store
      ? `OPFS · ${this.projectName}`
      : "ไม่มี OPFS";
    this.log(`พร้อม — โปรเจกต์ "${this.projectName}" · กด Play เพื่อฟัง (ครั้งแรกจะ render)`);
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
    // The document changed: the audio in hand is stale until a render lands.
    this.audio.dirty = true;
    this.scheduleRender();
    this.scheduleAutosave();
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
    $("renderEngine").addEventListener("change", (e) => this.setRenderEngine(e.target.value));
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
    $("saveBtn").addEventListener("click", () => this.saveCurrentProject());
    $("redoBtn").addEventListener("click", () => {
      const n = this.driver.redo();
      this.log(`redo: ${n || "—"}`);
      this.refresh();
    });
    $("projNew").addEventListener("click", () => this.newProject());
    $("projSaveAs").addEventListener("click", () => this.saveProjectAs());
    $("projExport").addEventListener("click", () => this.exportUstx());
    $("projImport").addEventListener("click", () => $("projFile").click());
    $("projFile").addEventListener("change", (e) => {
      const f = e.target.files?.[0];
      if (f) this.importUstx(f);
      e.target.value = "";
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

  // ---- projects (OPFS) --------------------------------------------------

  /** Autosave 1500 ms after the last edit (web-ui-design §6). */
  scheduleAutosave() {
    if (!this.store) return;
    clearTimeout(this._autosaveTimer);
    this._autosaveTimer = setTimeout(() => this.autosaveNow(), 1500);
  }

  /** Write the document to OPFS when it differs from the last write. */
  async autosaveNow() {
    if (!this.store) return null;
    const yaml = this.driver.yaml();
    if (yaml === this._savedYaml) return null;
    const saved = await this.store.save(this.projectName, yaml);
    await this.store.remember(saved);
    this._savedYaml = yaml;
    $("projectState").textContent = `บันทึกแล้ว · ${saved}`;
    this.renderProjectList();
    return saved;
  }

  async openProject(name) {
    if (!this.store) return false;
    const yaml = await this.store.load(name);
    this.driver.loadYaml(yaml);
    this.projectName = safeProjectName(name);
    this._savedYaml = this.driver.yaml();
    this.roll._framed = false;
    await this.store.remember(this.projectName);
    this.refresh();
    this.renderProjectList();
    $("projectState").textContent = `OPFS · ${this.projectName}`;
    this.log(`เปิดโปรเจกต์: ${this.projectName}`);
    return true;
  }

  newProject() {
    const yaml = this.driver.newProject();
    this.projectName = "untitled";
    this._savedYaml = null;
    this.roll._framed = false;
    this.refresh();
    this.renderProjectList();
    this.log("โปรเจกต์ใหม่ (ยังไม่บันทึก — แก้อะไรแล้ว autosave จะเขียน)");
    return yaml;
  }

  /** Explicit save under a new name; falls back to the current one. */
  async saveProjectAs(name) {
    if (!this.store) return null;
    const wanted = name ?? globalThis.prompt?.("ชื่อโปรเจกต์", this.projectName);
    if (!wanted) return null;
    const saved = await this.store.save(wanted, this.driver.yaml());
    this.projectName = saved;
    this._savedYaml = this.driver.yaml();
    await this.store.remember(saved);
    this.renderProjectList();
    $("projectState").textContent = `OPFS · ${saved}`;
    this.log(`บันทึกเป็น: ${saved}.ustx`);
    return saved;
  }

  /** Flutter-style app-bar save: persist to OPFS, or download the .ustx. */
  async saveCurrentProject() {
    if (!this.store) return this.exportUstx();
    const saved = await this.store.save(this.projectName, this.driver.yaml());
    this.projectName = saved;
    this._savedYaml = this.driver.yaml();
    await this.store.remember(saved);
    this.renderProjectList();
    $("projectState").textContent = `บันทึกแล้ว · ${saved}`;
    this.log(`บันทึกโปรเจกต์: ${saved}.ustx`);
    return saved;
  }

  async removeProject(name) {
    if (!this.store) return;
    await this.store.remove(name);
    if (safeProjectName(name) === this.projectName) this._savedYaml = null;
    this.renderProjectList();
    this.log(`ลบโปรเจกต์: ${name}`);
  }

  /** Download the document as `.ustx` (the only copy that survives a wipe). */
  exportUstx() {
    const fileName = `${safeProjectName(this.projectName)}.ustx`;
    const blob = new Blob([this.driver.yaml()], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = fileName;
    document.body.appendChild(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 0);
    this.log(`export ${fileName}`);
    return fileName;
  }

  /** Open a `.ustx` chosen from disk (works where OPFS does not). */
  async importUstx(file) {
    const text = await file.text();
    this.driver.loadYaml(text);
    this.projectName = safeProjectName(String(file.name).replace(/\.ustx$/i, ""), "imported");
    this._savedYaml = null;
    this.roll._framed = false;
    this.refresh();
    this.log(`import: ${file.name} → "${this.projectName}"`);
    return this.projectName;
  }

  renderProjectList() {
    const el = $("projectList");
    if (!el) return;
    if (!this.store) {
      el.innerHTML = '<div class="muted small">เบราว์เซอร์นี้ไม่มี OPFS — ใช้ Export/Import</div>';
      return;
    }
    this.store.list().then((items) => {
      if (!items.length) {
        el.innerHTML = '<div class="muted small">ยังไม่มีโปรเจกต์ที่บันทึก</div>';
        return;
      }
      el.innerHTML = "";
      for (const p of items) {
        const row = document.createElement("div");
        row.className = `proj${p.name === this.projectName ? " selected" : ""}`;
        row.innerHTML = `<span class="pname" title="${escapeAttr(p.name)}">${escapeHtml(p.name)}</span>
          <span class="pmeta">${(p.bytes / 1024).toFixed(1)} KB</span>
          <button class="pdel" data-del="${escapeAttr(p.name)}" title="ลบ">✕</button>`;
        row.addEventListener("click", (ev) => {
          const del = ev.target.dataset?.del;
          if (del !== undefined) {
            ev.stopPropagation();
            this.removeProject(del);
            return;
          }
          this.openProject(p.name);
        });
        el.appendChild(row);
      }
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

  /** Create the worker pool. Never blocks boot; failure → main-thread path. */
  async _initPool() {
    const renderEngine = this.renderEngine;
    try {
      const pool = await RenderPool.create({
        workerCount: renderEngine === "r2-webgpu" ? 1 : POOL_WORKERS,
        bank: this.bank,
        otoPath: "voice/oto.ini",
        otoDir: "voice",
        projectYaml: this.driver.yaml(),
        track: this.track,
        renderEngine,
      });
      if (this.renderEngine !== renderEngine) {
        pool.terminate();
        return null;
      }
      this.pool = pool;
      if (renderEngine === "r2-webgpu") {
        const info = pool.links[0]?.timings;
        this.log(`R2 WebGPU พร้อม · ${JSON.stringify(info?.r2Device || {})} · โหลด runtime ${Math.round(info?.r2LoadMs || 0)} ms`);
        this.log("R2: Rust WORLD ใน WASM → mel/vocoder WebGPU-first (WASM fallback)");
      } else {
        this.log(`pool: ${pool.workerCount} workers พร้อม · ${pool.plan.length} chunks`);
      }
      return pool;
    } catch (err) {
      this.pool = null;
      this.log(`pool ใช้ไม่ได้ (${err}) — render บน main thread แทน`);
      return null;
    }
  }

  async setRenderEngine(engine) {
    if (engine !== "r1-rs" && engine !== "r2-webgpu") return;
    if (engine === this.renderEngine) return;
    this.cancelRender();
    clearTimeout(this.renderTimer);
    this.pool?.terminate();
    this.pool = null;
    this.renderEngine = engine;
    $("status").textContent = engine === "r2-webgpu" ? "กำลังโหลด R2 WebGPU…" : "กำลังสลับ R1…";
    this.audio.buffer = null;
    this.audio.dirty = true;
    this._poolPromise = this._initPool();
    await this._poolPromise;
    await this.renderNow(true);
  }

  /**
   * Render the project. The pool is the production path (workers + per-chunk
   * sample loading + chunk cache); the main thread is the fallback.
   *
   * @returns {Promise<object|null>} pool stats, or null when the render was
   *   superseded/cancelled or when the fallback path ran.
   */
  async renderNow(force) {
    if (!this.driver) return null;
    const pool = this.pool || (this._poolPromise ? await this._poolPromise : null);
    const p = pool ? this._renderWithPool(pool, force) : this._renderOnMainThread(force);
    this._renderPromise = p;
    try {
      return await p;
    } finally {
      if (this._renderPromise === p) this._renderPromise = null;
    }
  }

  /**
   * Drop the in-flight render. Workers still finish the chunk they are on, but
   * the reply is neither cached nor published — the audio already in hand stays
   * valid instead of being replaced by a half render.
   */
  cancelRender() {
    this._epoch += 1;
    if (this._renderToken) this._renderToken.cancelled = true;
    this._renderToken = null;
  }

  async _renderWithPool(pool, force) {
    // The epoch is claimed synchronously, before the first await, so a render
    // started later always wins.
    const epoch = ++this._epoch;
    const token = { cancelled: false };
    this._renderToken = token;
    const t0 = performance.now();
    try {
      pool.track = this.track;
      const prep = await pool.setProject(this.driver.yaml(), { force });
      if (epoch !== this._epoch) return null;
      if (prep.skipped.length) this.log(`ข้าม: ${prep.skipped.join("; ")}`);
      if (prep.skipped.length || prep.chunks === 0) {
        this.audio.dirty = true;
        this._paintChunks(pool, "idle");
        $("chunkInfo").textContent = `${prep.chunks} chunks · cache ${pool.cache.size}/${prep.chunks}`;
        $("status").textContent = prep.skipped.length
          ? "เรนเดอร์ไม่ได้ · voicebank ไม่มี alias/sample บางเสียง"
          : "ไม่มีโน้ตให้เรนเดอร์";
        if (!prep.skipped.length) this.log("render blocked: ไม่มีโน้ตที่ render ได้");
        return null;
      }
      this._paintChunks(pool, "rendering");
      $("status").textContent = prep.dirty
        ? `กำลังเรนเดอร์ ${prep.dirty}/${prep.chunks} chunk…`
        : "ตรวจ chunk cache…";

      const res = await pool.render({
        signal: token,
        onProgress: ({ rendered, total }) => {
          if (epoch !== this._epoch) return;
          $("status").textContent = `กำลังเรนเดอร์ ${rendered}/${total} chunk…`;
          this._paintChunks(pool, "rendering");
        },
      });
      if (epoch !== this._epoch || res.cancelled || !res.samples) return null;
      if (res.samples.length === 0) {
        this.audio.dirty = true;
        this._paintChunks(pool, "idle");
        $("status").textContent = "เรนเดอร์ไม่ได้ · ไม่มีเสียงที่ render สำเร็จ";
        this.log("render blocked: renderer returned 0 samples");
        return null;
      }

      const pcm = res.samples;
      const s = res.stats;
      const ms = performance.now() - t0;
      const sec = pcm.length / 44100;
      this.audio.buffer = pcm;
      this.audio.dirty = false;
      this._paintChunks(pool, "idle");
      $("status").textContent = `render ${ms.toFixed(0)} ms · ${sec.toFixed(2)}s audio`;
      $("chunkInfo").textContent =
        `${pool.plan.length} chunks · cache ${pool.cache.size}/${pool.plan.length}`;
      if (force || s.lastRendered) {
        this.log(
          `render: ${pcm.length} samples (${sec.toFixed(2)}s) · chunk ใหม่ ` +
          `${s.lastRendered}/${pool.plan.length} · โหลด ${s.lastLoadMs.toFixed(0)}ms · mix ${s.lastMixMs.toFixed(1)}ms`,
        );
      }
      return s;
    } catch (err) {
      if (epoch !== this._epoch) return null;
      this.log(`render error: ${err}`);
      $("status").textContent = "render ล้มเหลว";
      return null;
    }
  }

  /** Fallback: render every chunk on this thread (no Workers available). */
  async _renderOnMainThread(force) {
    if (this.renderEngine === "r2-webgpu") {
      this.log("R2 WebGPU worker unavailable; refusing to switch audio to R1 automatically");
      $("status").textContent = "R2 WebGPU render ใช้ไม่ได้";
      return null;
    }
    const epoch = ++this._epoch;
    const t0 = performance.now();
    try {
      const meta = this.driver.plan(this.track);
      $("chunkInfo").textContent = `${meta.length} chunks (main thread)`;
      const skipped = this.driver.planSkipped();
      if (skipped.length) this.log(`ข้าม: ${skipped.join("; ")}`);
      if (skipped.length || meta.length === 0) {
        this.audio.dirty = true;
        $("status").textContent = skipped.length
          ? "เรนเดอร์ไม่ได้ · voicebank ไม่มี alias/sample บางเสียง"
          : "ไม่มีโน้ตให้เรนเดอร์";
        if (!skipped.length) this.log("render blocked: ไม่มีโน้ตที่ render ได้");
        return null;
      }

      const loaded = await this.ensureSamples();
      if (epoch !== this._epoch) return null;
      if (loaded) this.log(`โหลด sample เพิ่ม ${loaded} (รวม ${this._loaded.size})`);

      const pcm = this.driver.renderAll();
      if (epoch !== this._epoch) return null;
      if (pcm.length === 0) {
        this.audio.dirty = true;
        $("status").textContent = "เรนเดอร์ไม่ได้ · ไม่มีเสียงที่ render สำเร็จ";
        this.log("render blocked: renderer returned 0 samples");
        return null;
      }
      const ms = performance.now() - t0;
      const sec = pcm.length / 44100;
      this.audio.buffer = pcm;
      this.audio.dirty = false;
      this.renderChunks(meta);
      $("status").textContent = `render ${ms.toFixed(0)} ms · ${sec.toFixed(2)}s audio`;
      if (force) {
        this.log(`render (main thread): ${pcm.length} samples (${sec.toFixed(2)}s) ใน ${ms.toFixed(0)} ms`);
      }
    } catch (err) {
      if (epoch !== this._epoch) return null;
      this.log(`render error: ${err}`);
      $("status").textContent = "render ล้มเหลว";
    }
    return null;
  }

  /** Chunk overlay straight from the pool's cache — not decoration. */
  _paintChunks(pool, phase) {
    const bpm = this.driver.meta().bpm || 120;
    const msPerTick = 60000 / bpm / 480;
    this.roll.setChunks(
      pool.plan.map((c) => ({
        start_tick: c.start_ms / msPerTick,
        end_tick: c.end_ms / msPerTick,
        state: pool.cache.has(c.hash) ? "done" : phase === "rendering" ? "rendering" : "stale",
      })),
    );
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
    // Never play stale audio: an edit marks the buffer dirty, and a render may
    // already be in flight — await that one instead of starting a second.
    if (!this.audio.buffer || this.audio.dirty) {
      if (this._renderPromise) await this._renderPromise;
      else await this.renderNow(false);
      if (!this.audio.buffer || this.audio.dirty || this.audio.buffer.length === 0) return;
    }
    if (!this.audio.buffer || this.audio.dirty || this.audio.buffer.length === 0) return;
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
    // The transport's stop button also drops an in-flight render.
    this.cancelRender();
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

/** Pool size: leave a core for the UI thread, and cap at 4 (tablet-first). */
const POOL_WORKERS = Math.max(1, Math.min(4, (navigator.hardwareConcurrency || 4) - 1));

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
