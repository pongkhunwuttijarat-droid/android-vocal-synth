# ENGINE HOSTING ON ANDROID — Handoff for the UI session (2026-08-02)

> **Status:** Engine side done. UI session owns `app/` — engine session will NOT touch `app/`.
> Coordination point: **CONTRACT-v1.md** (transport = EngineClient interface).

## What the engine session delivered (ready to consume)

### 1. In-app engine hosting pattern (Kotlin — `app/android/.../EngineProcess.kt`)

The bundled synth-server runs INSIDE the app as a child process (dev transport, CONTRACT-v1 §0):

```
APK layout:
  jniLibs/arm64-v8a/synth-server      ← Rust server binary (MUST be here)
  jniLibs/arm64-v8a/libworldline.so   ← renderer .so (dlopened by server)
  assets/engine/voicebanks/...        ← voicebank tree (extracted to filesDir)
  assets/engine/demo-song.ustx        ← demo project (extracted to filesDir)
```

**CRITICAL pitfall (already debugged):** `filesDir` / `getDir("bin")` are SELinux
`app_data_file` on Android 16 → `exec()` fails with EACCES (error=13).
The ONLY app-owned dir that allows exec is **`nativeLibraryDir`** (`apk_data_file`).
⇒ binaries go in `jniLibs`, data (voicebanks/project) goes in `filesDir/engine/`.

Server args: `--so <nativeLibDir>/libworldline.so --voicebanks <filesDir>/engine/voicebanks
--port 18080 --bind 127.0.0.1` — UI talks to `http://127.0.0.1:18080` (EngineClient default).

### 2. MethodChannel (MainActivity.kt — already written)

```
channel: com.lilt.lilt/engine
  engineDir      → {dir, project, voicebanks} (device-side absolute paths)
  engineRunning  → bool
```
`lib/api/engine_paths.dart` wraps it; `EditorScreen._renderTarget()` uses it on Android,
falls back to host paths on desktop.

### 3. Voicebank note

Bundled bank dir name is `teto-english` (not `library`) — `/render` must send
`voicebank: "teto-english"` when talking to the in-app server.

### 4. Known UI state at handoff (as of 20:57)

- `piano_roll.dart` was mid-edit by the UI session (fields `_dragCurveNote` /
  `_curveDragStartOffset` referenced but not declared in the version the engine
  session last saw) — **UI session owns fixing this**; engine session will not touch it.
- Engine session's last verified state: `flutter analyze` clean + 18 tests pass
  (before the UI session's newer edits landed).

## Engine side is otherwise occupied with (do not touch):

- `native/` workspace (389 tests green) — synth-cli, synth-server, golden tooling
- `test/golden/` — OpenUtau reference pipeline
- Docs: `docs/CONTRACT-v1.md`, `docs/planning/milestones/ms2/MS.md`

## Next integration step (whoever picks it up)

1. Fix `piano_roll.dart` fields (UI session)
2. `flutter run -d <device>` → app starts → EngineProcess spawns server (check logcat `LiltEngine`)
3. `adb forward tcp:18080 tcp:18080` (optional — in-app server binds 127.0.0.1)
4. Tap Export → renders demo song on-device → SnackBar shows size/duration
