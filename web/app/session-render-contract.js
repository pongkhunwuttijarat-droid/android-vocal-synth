/**
 * Browser-side helpers for the committed Session -> render adapter boundary.
 *
 * The Engine is the only authority for the snapshot. Keep capture synchronous
 * so no UI command can interleave between revision checks and reading the
 * canonical project plus both Session sidecars.
 */

export class SessionRenderError extends Error {
  constructor(code, message, details = {}) {
    super(message);
    this.name = "SessionRenderError";
    this.code = code;
    this.details = details;
  }
}

function deepFreeze(value) {
  if (value && typeof value === "object" && !Object.isFrozen(value)) {
    for (const child of Object.values(value)) deepFreeze(child);
    Object.freeze(value);
  }
  return value;
}

function canonicalJson(value) {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map(key => `${JSON.stringify(key)}:${canonicalJson(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

/**
 * Capture immutable canonical project data from the committed Rust session.
 *
 * `expectedRevision` is the revision the user requested to render. The
 * returned object can be retained by a render job even if the live session is
 * edited afterwards.
 */
export function captureCommittedRenderSnapshot(engine, expectedRevision) {
  if (!Number.isSafeInteger(expectedRevision) || expectedRevision < 0) {
    throw new SessionRenderError("validation_error", "expectedRevision must be a non-negative safe integer");
  }
  if (!engine || typeof engine.session_id !== "function" ||
      typeof engine.session_revision !== "function" ||
      typeof engine.project_snapshot !== "function" ||
      typeof engine.session_snapshot_json !== "function" ||
      typeof engine.session_save_sidecar !== "function" ||
      typeof engine.session_save_pronunciation_sidecar !== "function") {
    throw new SessionRenderError("unsupported_operation", "Engine does not expose the committed Session snapshot API");
  }

  const sessionId = engine.session_id();
  const revisionValue = engine.session_revision();
  const revision = typeof revisionValue === "bigint" ? Number(revisionValue) : revisionValue;
  if (!Number.isSafeInteger(revision) || revision < 0) {
    throw new SessionRenderError("unsupported_operation", "The current session revision cannot be represented safely in the browser adapter");
  }
  if (!sessionId) {
    throw new SessionRenderError("not_found", "No project session is open");
  }
  if (revision !== expectedRevision) {
    throw new SessionRenderError("revision_conflict", "The requested render revision is no longer current", {
      expected_revision: expectedRevision,
      current_revision: revision,
    });
  }

  // project_snapshot is the Rust-owned committed canonical USTX cache. These
  // synchronous WASM calls cannot interleave with another UI command.
  const ustx = engine.project_snapshot();
  let projectSnapshot;
  try { projectSnapshot = JSON.parse(engine.session_snapshot_json()); }
  catch (error) {
    throw new SessionRenderError("unsupported_operation", "Engine returned an invalid committed Session snapshot", { cause: String(error) });
  }
  const identitySidecar = engine.session_save_sidecar();
  const pronunciationSidecar = engine.session_save_pronunciation_sidecar();
  const revisionAfterValue = engine.session_revision();
  const revisionAfter = typeof revisionAfterValue === "bigint" ? Number(revisionAfterValue) : revisionAfterValue;
  const sessionIdAfter = engine.session_id();
  if (revisionAfter !== revision || sessionIdAfter !== sessionId) {
    throw new SessionRenderError("revision_conflict", "The project session changed while capturing its render input", {
      expected_revision: revision,
      current_revision: revisionAfter,
    });
  }
  if (typeof ustx !== "string" || ustx.length === 0 ||
      typeof identitySidecar !== "string" || typeof pronunciationSidecar !== "string") {
    throw new SessionRenderError("validation_error", "The committed project snapshot or sidecars are empty");
  }

  if (!projectSnapshot || projectSnapshot.project == null) {
    throw new SessionRenderError("validation_error", "The committed Session snapshot has no project view");
  }
  return Object.freeze({ sessionId, revision, ustx, identitySidecar, pronunciationSidecar, projectSnapshot: deepFreeze(projectSnapshot) });
}

/**
 * Copy each render input into job-owned bytes and pin the SHA-256 of the exact
 * copy that the render worker will consume. The caller must pass these returned
 * buffers to the worker; reopening an OPFS path after this point breaks the
 * snapshot guarantee. Asset order is canonicalized for stable manifests.
 *
 * `assets` entries are `{ kind, key, bytes }`, where kind is `oto`, `sample-wav`,
 * `sample-frq`, or `project-media`. Keys identify the logical input, not its
 * mutable storage path. Empty optional FRQ inputs should be omitted.
 */
export async function pinRenderAssetBytes(assets, cryptoProvider = globalThis.crypto) {
  if (!Array.isArray(assets)) throw new SessionRenderError("validation_error", "assets must be an array");
  if (!cryptoProvider?.subtle?.digest) {
    throw new SessionRenderError("unsupported_operation", "SHA-256 WebCrypto is unavailable");
  }
  const normalized = assets.map(asset => {
    if (!asset || !["oto", "sample-wav", "sample-frq", "project-media"].includes(asset.kind) ||
        typeof asset.key !== "string" || !asset.key || asset.bytes == null) {
      throw new SessionRenderError("validation_error", "Each render asset needs a supported kind, key, and bytes");
    }
    const source = asset.bytes instanceof ArrayBuffer
      ? new Uint8Array(asset.bytes)
      : ArrayBuffer.isView(asset.bytes)
        ? new Uint8Array(asset.bytes.buffer, asset.bytes.byteOffset, asset.bytes.byteLength)
        : null;
    if (!source) throw new SessionRenderError("validation_error", `Asset ${asset.key} bytes must be an ArrayBuffer or view`);
    return { kind: asset.kind, key: asset.key, bytes: source.slice() };
  });
  normalized.sort((a, b) => (a.kind < b.kind ? -1 : a.kind > b.kind ? 1 : 0)
    || (a.key < b.key ? -1 : a.key > b.key ? 1 : 0));
  for (let index = 1; index < normalized.length; index++) {
    if (normalized[index - 1].kind === normalized[index].kind && normalized[index - 1].key === normalized[index].key) {
      throw new SessionRenderError("validation_error", `Duplicate render asset ${normalized[index].kind}:${normalized[index].key}`);
    }
  }
  const pinned = [];
  for (const asset of normalized) {
    const digest = await cryptoProvider.subtle.digest("SHA-256", asset.bytes);
    const sha256 = [...new Uint8Array(digest)].map(byte => byte.toString(16).padStart(2, "0")).join("");
    pinned.push(Object.freeze({ kind: asset.kind, key: asset.key, byteLength: asset.bytes.byteLength, sha256, bytes: asset.bytes }));
  }
  const manifest = Object.freeze(pinned.map(({ kind, key, byteLength, sha256 }) =>
    Object.freeze({ kind, key, byteLength, sha256 }),
  ));
  return Object.freeze({ manifest, assets: Object.freeze(pinned), transfer: pinned.map(asset => asset.bytes.buffer) });
}

/** Stable digest tying the Rust document, sidecars, render settings and assets to one job. */
export async function fingerprintRenderJob(snapshot, assetManifest, renderConfig = {}, cryptoProvider = globalThis.crypto) {
  if (!snapshot || !Array.isArray(assetManifest) || !cryptoProvider?.subtle?.digest) {
    throw new SessionRenderError("validation_error", "A snapshot, asset manifest, and SHA-256 provider are required");
  }
  const encoder = new TextEncoder();
  const digestText = async value => {
    const hash = await cryptoProvider.subtle.digest("SHA-256", encoder.encode(value));
    return [...new Uint8Array(hash)].map(byte => byte.toString(16).padStart(2, "0")).join("");
  };
  const [projectSha256, identitySha256, pronunciationSha256] = await Promise.all([
    digestText(snapshot.ustx), digestText(snapshot.identitySidecar), digestText(snapshot.pronunciationSidecar),
  ]);
  const assets = assetManifest.map(({ kind, key, byteLength, sha256 }) => ({ kind, key, byteLength, sha256 }))
    .sort((a, b) => (a.kind < b.kind ? -1 : a.kind > b.kind ? 1 : 0) || (a.key < b.key ? -1 : a.key > b.key ? 1 : 0));
  const canonical = canonicalJson({
    sessionId: snapshot.sessionId,
    revision: snapshot.revision,
    projectSha256,
    identitySha256,
    pronunciationSha256,
    renderConfig,
    assets,
  });
  return digestText(canonical);
}

/** True only when a completed job belongs to the currently displayed session revision. */
export function isCurrentRenderResult(result, currentSession) {
  return Boolean(
    result && currentSession &&
    result.sessionId === currentSession.sessionId &&
    result.revision === currentSession.revision,
  );
}
