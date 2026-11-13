# Web Project Session adapter

`web/app/session-driver.js` is the browser host adapter for the shared Rust
`project-session` API. It does not hold a second project model: snapshots,
identity, revision, commands, history, and USTX serialization come from the
WASM `Engine`. The adapter sequences browser storage and publishes disposable
change notifications for a UI.

## Lifecycle

- `create({ name, bpm })` calls `session_new_json`.
- `openStored(name)` loads canonical USTX and the adjacent
  `<name>.ustx.rusted-utau-ids.json`, then calls `session_open_yaml` with both.
- `importUstx(text)` calls `session_open_yaml` without a sidecar; Rust assigns
  fresh entity IDs for the imported portable document.
- `save(name)` obtains canonical USTX and the current identity sidecar from the
  open Session and writes the pair before returning.
- `exportUstx()` returns canonical portable USTX without the private sidecar.
- `saveAs(name)` asks Rust to prepare a sidecar with a forked project ID while
  retaining entity IDs, persists both prepared artifacts, and only then asks
  Rust to adopt the fork. A host storage failure therefore leaves the live
  Session identity unchanged.
- `close()` drops the project Session while retaining the Engine's voicebank
  and sample state.

`ProjectStore.saveBundle` restores the previous pair on a detected write error.
This is rollback-based OPFS handling, not a crash-atomic filesystem transaction.
No autosave timer is installed here; the eventual UI should call `save` after
its chosen debounce and keep unsaved-state presentation ephemeral.

## Commands and views

`apply`, `undo`, and `redo` dispatch the stable-ID Session API and return Rust
results. `state()` reads the authoritative snapshot. Listeners receive a new
snapshot after lifecycle or mutation operations; listeners must not mutate or
serialize it as independent state. Grouped gesture methods and broader
`part.*`/`project.*` command support are not added by this adapter and remain
subject to the engine capability descriptor.

The adapter is ready for production UI integration. The React mock is still a
presentation-only prototype, and this document does not claim that it uses the
SessionDriver yet.
