// OPFS project store — `projects/<name>.ustx` plus its Session identity sidecar.
//
// This is the browser's answer to the native app's `.ustx` files: the document
// lives in the Origin Private File System, written from the Rust Session and
// reopened with its identity sidecar, so the engine stays the only owner of
// the format (no JS re-implementation of the .ustx codec).
//
// Design: docs/planning/web-ui-design.md §6 — autosave 1500 ms after an edit,
// `navigator.storage.persist()` requested on the first save, and an explicit
// `Export .ustx` because clearing site data wipes project + voicebank.
//
// Native counterpart: `app/lib/project/file_project_store.dart` (same idea, a
// real directory instead of OPFS).

/** A project name safe to use as a file name. */
export function safeProjectName(raw, fallback = "untitled") {
  const cleaned = String(raw ?? "")
    .trim()
    .replace(/\.ustx$/i, "")
    .replace(/[^\p{L}\p{N} _.-]+/gu, "_")
    .replace(/^[.\s]+|[.\s]+$/g, "")
    .slice(0, 80);
  return cleaned || fallback;
}

export class ProjectStore {
  static async available() {
    return typeof navigator !== "undefined" && !!navigator.storage?.getDirectory;
  }

  constructor({ root = "projects" } = {}) {
    this.root = root;
    this._dir = null;
    this._persistRequested = false;
  }

  async dir() {
    if (!this._dir) {
      this._dir = await navigator.storage.getDirectory();
    }
    return this._dir;
  }

  async _projectsDir({ create = false } = {}) {
    const root = await this.dir();
    return root.getDirectoryHandle(this.root, { create });
  }

  /** `[{ name, bytes, savedAt }]`, newest first. */
  async list() {
    const out = [];
    let dir;
    try {
      dir = await this._projectsDir();
    } catch {
      return out; // no OPFS directory yet → nothing saved
    }
    for await (const [name, handle] of dir.entries()) {
      if (handle.kind !== "file" || !name.toLowerCase().endsWith(".ustx")) continue;
      const file = await handle.getFile();
      out.push({
        name: name.replace(/\.ustx$/i, ""),
        bytes: file.size,
        savedAt: file.lastModified,
      });
    }
    out.sort((a, b) => b.savedAt - a.savedAt);
    return out;
  }

  async has(name) {
    const target = `${safeProjectName(name)}.ustx`;
    try {
      const dir = await this._projectsDir();
      await dir.getFileHandle(target);
      return true;
    } catch {
      return false;
    }
  }

  async load(name) {
    const dir = await this._projectsDir();
    const handle = await dir.getFileHandle(`${safeProjectName(name)}.ustx`);
    const file = await handle.getFile();
    return file.text();
  }

  /** Load canonical USTX and its identity sidecar as one host bundle. */
  async loadBundle(name) {
    const dir = await this._projectsDir();
    const safe = safeProjectName(name);
    const read = async (fileName) => {
      try {
        const handle = await dir.getFileHandle(fileName);
        return await (await handle.getFile()).text();
      } catch (error) {
        if (error?.name === "NotFoundError") return "";
        throw error;
      }
    };
    return {
      ustx: await read(`${safe}.ustx`),
      sidecar: await read(`${safe}.ustx.rusted-utau-ids.json`),
      pronunciationSidecar: await read(`${safe}.ustx.rusted-utau-pronunciations.json`),
    };
  }

  /**
   * Write a project. The first save asks for persistent storage so a browser
   * "clear site data" is the only thing that can take it away.
   */
  async save(name, yaml) {
    const safe = safeProjectName(name);
    const dir = await this._projectsDir({ create: true });
    const handle = await dir.getFileHandle(`${safe}.ustx`, { create: true });
    const writable = await handle.createWritable();
    await writable.write(yaml);
    await writable.close();
    if (!this._persistRequested) {
      this._persistRequested = true;
      try {
        await navigator.storage.persist?.();
      } catch {
        /* best effort */
      }
    }
    return safe;
  }

  /** Persist both Session artifacts; roll back the prior pair if a write fails. */
  async saveBundle(name, { ustx, sidecar, pronunciationSidecar }) {
    if (typeof ustx !== "string" || typeof sidecar !== "string" || !sidecar
      || typeof pronunciationSidecar !== "string" || !pronunciationSidecar) {
      throw new TypeError("A Session save requires canonical USTX and both Session sidecars");
    }
    const safe = safeProjectName(name);
    const dir = await this._projectsDir({ create: true });
    const files = [
      `${safe}.ustx`,
      `${safe}.ustx.rusted-utau-ids.json`,
      `${safe}.ustx.rusted-utau-pronunciations.json`,
    ];
    const old = await Promise.all(files.map(async (fileName) => {
      try {
        const handle = await dir.getFileHandle(fileName);
        return await (await handle.getFile()).text();
      } catch (error) {
        if (error?.name === "NotFoundError") return null;
        throw error;
      }
    }));
    const write = async (fileName, contents) => {
      const handle = await dir.getFileHandle(fileName, { create: true });
      const writable = await handle.createWritable();
      try { await writable.write(contents); await writable.close(); }
      catch (error) { try { await writable.abort(); } catch {} throw error; }
    };
    try {
      await write(files[0], ustx);
      await write(files[1], sidecar);
      await write(files[2], pronunciationSidecar);
    } catch (error) {
      for (let i = 0; i < files.length; i++) {
        try {
          if (old[i] === null) await dir.removeEntry(files[i]);
          else await write(files[i], old[i]);
        } catch { /* best-effort restoration; preserve original storage error */ }
      }
      throw error;
    }
    if (!this._persistRequested) {
      this._persistRequested = true;
      try { await navigator.storage.persist?.(); } catch { /* best effort */ }
    }
    return safe;
  }

  /** Persist imported audio bytes separately from the portable USTX document. */
  async saveMedia(mediaId, bytes) {
    const safe = safeMediaId(mediaId);
    const root = await this.dir();
    const dir = await root.getDirectoryHandle("media", { create: true });
    const handle = await dir.getFileHandle(safe, { create: true });
    const writable = await handle.createWritable();
    try { await writable.write(bytes); await writable.close(); }
    catch (error) { try { await writable.abort(); } catch {} throw error; }
    return safe;
  }

  /** Read previously imported audio bytes for re-registering with Rust. */
  async loadMedia(mediaId) {
    const safe = safeMediaId(mediaId);
    const root = await this.dir();
    const dir = await root.getDirectoryHandle("media");
    const handle = await dir.getFileHandle(safe);
    return new Uint8Array(await (await handle.getFile()).arrayBuffer());
  }

  async remove(name) {
    const dir = await this._projectsDir();
    const safe = safeProjectName(name);
    await dir.removeEntry(`${safe}.ustx`);
    try { await dir.removeEntry(`${safe}.ustx.rusted-utau-ids.json`); } catch (error) {
      if (error?.name !== "NotFoundError") throw error;
    }
    try { await dir.removeEntry(`${safe}.ustx.rusted-utau-pronunciations.json`); } catch (error) {
      if (error?.name !== "NotFoundError") throw error;
    }
  }

  /** Name of the project to reopen on boot (a tiny sidecar file). */
  async lastOpened() {
    try {
      const dir = await this._projectsDir();
      const handle = await dir.getFileHandle(".session.json");
      const file = await handle.getFile();
      const data = JSON.parse(await file.text());
      return typeof data.lastOpened === "string" ? data.lastOpened : null;
    } catch {
      return null;
    }
  }

  async remember(name) {
    try {
      const dir = await this._projectsDir({ create: true });
      const handle = await dir.getFileHandle(".session.json", { create: true });
      const writable = await handle.createWritable();
      await writable.write(JSON.stringify({ lastOpened: safeProjectName(name) }));
      await writable.close();
    } catch {
      /* the sidecar is a convenience, not a requirement */
    }
  }
}

function safeMediaId(raw) {
  const id = String(raw ?? "").replace(/^asset:/, "");
  if (!id || id === "." || id === ".." || !/^[\p{L}\p{N}_.-]+$/u.test(id)) {
    throw new TypeError("Media id must be a simple asset identifier");
  }
  return id;
}
