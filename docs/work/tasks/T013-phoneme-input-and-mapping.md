# T013 — Build layered phoneme input and voicebank mapping

**Status:** done
**Owner:** Codex
**Last updated (UTC):** 2026-10-03T09:38:54Z

## Goal

Let users type a lyric or sound hint and choose from an engine-produced list of phoneme
compositions, similar to an IME conversion list. Keep the phoneme model usable across
voicebanks: convert language input into a universal phone representation, then map that
representation through language-specific rules and the selected voicebank's own alias
inventory and singing style (for example CV, VCV, CVVC, or VCCV). The user can choose a
universal representation or a bank-native alias/sequence when editing.

All conversion rules, candidates, selected phoneme composition, validation, and persisted
project state remain Rust-owned. The frontend owns only raw composition text, highlighted
candidate, and popup visibility while the user is typing.

## Completion criteria

- [x] Initial set confirmed as Thai/English/Japanese; keep language and voicebank style as
      extensible IDs rather than hard-coded frontend choices.
- [x] Define Rust DTOs for composition input, candidate list, candidate selection, mapping
      mode (`universal` or bank-native), language, and bank-specific alias sequences.
      → `native/crates/phonemizer/src/composition.rs`; canonical JSON pinned by
      `tests/composition_conformance.rs` and the proposal
      [`docs/planning/phoneme-compose-contract-proposal.md`](../../planning/phoneme-compose-contract-proposal.md).
- [x] Rust returns bounded, deterministic candidates while the user types; each candidate
      exposes universal phones, language-level conversion, selected voicebank alias sequence,
      style classification, and unresolved-phone diagnostics. → engine core and shared
      `project-session` read path.
- [x] Support selectable bank styles including CV, VCV, CVVC, and VCCV where the bank has
      matching aliases; do not guess unsupported aliases or silently alter the selected
      universal phones. → style restriction + by-construction tagging + honest diagnostics at
      engine level; real-bank breadth (e.g. a true VCCV bank E2E) still rides on the demo
      G2p dictionary.
- [x] Store committed phoneme choice/mode in the authoritative Rust project/session model;
      undo/redo, lyric changes, save, Save As and render use that state. USTX stays interoperable
      and pronunciation data stays in a separately versioned sidecar.
- [x] Provide typed composition/commit semantics through the shared session implementation and
      native/WASM adapters, with a versioned proposal and shared conformance vectors. CONTRACT-v1
      §6 stays frozen; the composition extension remains a draft awaiting contract review.
- [x] Re-derive render aliases for the active voicebank from the committed strategy/style and
      preserve contextual Japanese VCV transitions. Fail explicitly when the selected semantic
      choice cannot be re-derived; never render stale UI aliases.
- [x] Preserve shared adapter history/change semantics and baseline phonemizer regressions.
- [x] Verify Thai conversion and VCCV alias resolution against an in-memory bank fixture. A
      real Thai voicebank parity run remains unverified because no distributable Thai bank fixture
      is checked into this repository.
- [x] Prove dynamic load and invocation of a managed OpenUtau API phonemizer in browser WASM;
      measure a production-published payload, startup, memory, and note the current API/dependency
      boundary. This is a runtime POC only; a user-targeted third-party plugin remains unverified.

### Scope decisions

- User direction on 2026-10-02 superseded the earlier requirement for a managed .NET
  compatibility host: port selected behavior and import compatible data into Rusted UTAU's own Rust
  phonemizer framework. The official OpenUtau plugin loading experiments above remain evidence
  and are not a prerequisite for T013 completion.
- The Web/WASM editor UI request is deferred to T014. T013 ends at the authoritative
  session/native/WASM/render API boundary; no Flutter, Web or other editor UI work is included.

## Repository checkpoint

- Branch: `wasm-native-rust`
- Base commit recorded by board: `e77cc1a6`; verify current `HEAD` before any Git operation.
- Dirty working tree: extensive modified/untracked work already exists across Flutter, Web,
  `project-session`, `engine-wasm`, `synth-server`, and `phonemizer`; most of it predates this
  task and stays unowned. **Files this T013 handoff owns** (2026-10-03 DSH session):
  - new: `native/crates/phonemizer/src/composition.rs`, `tests/composition_conformance.rs`,
    `examples/t013_compose_json.rs` (composition core + conformance + canonical JSON generator)
  - new: `poc/openutau-browser-wasm/PluginImportProbe/` (native import-contract probe),
    `poc/openutau-browser-wasm/Host/FixtureCatalogData.cs` (baked catalog),
    `Host/CatalogSinger.cs`, generic `?autodll=` path in `Host/Pages/Home.razor`,
    random per-language lyrics in `Host/OfficialPhonemizerMatrixRunner.cs`,
    `Program.cs` CodePages registration, examples `t013_compose_json`
  - modified by this task: `phonemizer/Cargo.toml` (+serde), `phonemizer/src/lib.rs`
    (composition module), `phonemizer/src/japanese.rs` (`is_pure_vowel_mora` made pub),
    `phonemizer/tests/rusted_utau_dict.rs` (stale absolute path fixed),
    `phonemizer/src/default.rs` doc fence (untracked older file, fence fixed),
    `OpenUtau.Core/Util/Yaml.cs` in the checked-in Core tree(s) (field-shape compat shim,
    documented in code), `docs/planning/*` new planning docs
  - continued by this session (2026-10-03): `native/crates/phonemizer/src/thai.rs` (Thai
    orthography/VCCV token conversion), `composition.rs` (Thai candidate dispatch),
    `phonemizer/Cargo.toml` (regex dependency), `native/Cargo.lock` (phonemizer lock entry),
    and planning notes updated for the converter's actual scope
  - 2026-10-03 continuation attributable deltas (the files also contain pre-existing edits
    outside T013, which remain unowned): `domain/src/pronunciation.rs` + exports/model field;
    `project-session` pronunciation sidecar, note commands/views, compose read and render-choice
    mapping and `project-session/tests/pronunciation_session.rs`; `project-session/src/session.rs`
    history change results; shared document/part adapter vector expectations;
    `synth-cli/src/pipeline.rs` style/strategy rederivation from committed choices and focused
    committed-render/context tests;
    `engine-wasm` sidecar/compose/session-render bindings; `synth-server` sidecar storage,
    compose route and render-choice transport, plus the native/WASM compose-save-reopen parity
    test in `synth-server/tests/api_v2_sessions.rs`; `web/app/project-store.js` and
    `session-driver.js` storage/session adapters; Cargo dependency/lock entry;
    `docs/planning/phoneme-compose-contract-proposal.md`; T014 draft requirements.
- Relevant existing code to inspect and reuse: `phonemizer::{ipa,adapter,universal,detect}`;
  `voicebank::manifest::PhonemeManifest`; `project-session` snapshot/command surfaces; Web
  and Flutter lyric editing UI.
- Durable source: this task record, `docs/planning/default-phonemizer-parity.md`, and the
  existing phone-mapping implementation in `native/crates/phonemizer/`; design proposal:
  [`docs/planning/phoneme-input-composition.md`](../../planning/phoneme-input-composition.md).

## Progress

- 2026-10-03T09:38:54Z: Completed the non-UI T013 backend/session acceptance. Added session
  tests for pronunciation sidecar round-trip, undo/redo, lyric-reset, and clear; added a
  native-HTTP↔direct-WASM test that composes, commits, saves, and reopens the same pronunciation.
  Render tests pin style-aware active-bank alias rederivation and Japanese VCV continuity across
  note boundaries. Fixed the missing `history.changed` event for planner-backed commands and
  corrected shared-vector expectations; replaced stale Thai and committed-DTO tests with current
  behavior. `cargo test -p phonemizer` (all suites), project-session pronunciation tests, the full
  synth-server session suite (31), WASM adapter vectors (6), committed-render tests (2), and
  default phonemizer parity (4) pass; wasm32 `engine-wasm` check and formatting pass. The IME
  requirements remain draft-only in T014; no editor UI was changed. No real Thai voicebank is
  checked in, so Thai bank coverage uses a small in-memory alias fixture and real-bank parity is
  still unverified.

- 2026-10-03T09:22:20Z: Replaced the hint-only render bridge with typed, stable-ID-aligned
  pronunciation choices through the shared Rust pipeline. Native v2 render snapshots now carry
  note choices beside the immutable USTX snapshot; WASM renders from the open `Session` directly.
  `synth-cli` recomposes selected style/strategy aliases against the active voicebank and verifies
  that the result matches the committed language/universal phone sequences. Japanese VCV uses the
  contextual part phonemizer for cross-note vowel continuity. The temporary lyric-hint mechanism
  was removed. This was an intermediate checkpoint; the acceptance results and context coverage
  are recorded in the completed 09:38:54Z entry above.

- 2026-10-03T09:12:23Z: Invalid pronunciation sidecars now produce the session warning code
  `pronunciation_sidecar_ignored` while the USTX still opens; native open responses already
  carry session warnings, and WASM/browser can read them through
  `session_warnings_json` / `SessionDriver.warnings`. Formatting and the combined
  `cargo check` for project-session, engine-wasm, and synth-server pass. Warning detail granularity
  and tests are still part of the follow-on contract/conformance work.

- 2026-10-03T09:10:15Z: Initial render snapshot passed universal or language phones as disposable
  USTX hints. This was superseded by the typed strategy-aware render-choice path above.

- 2026-10-03T09:09:20Z: Added non-UI `SessionDriver` calls for revision-tagged Rust composition,
  lyric/pronunciation commands, and session-backed render snapshots. Re-ran Rust formatting
  successfully. Build status remains the passing `cargo check` below; browser JavaScript behavior
  and shared adapter parity have not yet been exercised.

- 2026-10-03T09:07:10Z: Implemented the first session-backed persistence and adapter slice:
  committed pronunciation is stored in undoable `UProject` session state, saved in a separate
  `rusted-utau.pronunciations/1` sidecar (including Save As), exposed in note reads, cleared atomically
  on lyric changes, and set/cleared through the shared stable-ID command planner. Native HTTP
  and WASM expose the composer read; the browser storage adapter persists both sidecars. Added
  `Session::render_yaml` and routed native session renders, plus WASM session render entry points,
  through an immutable clone that materializes committed language phones as a temporary
  phonetic hint. `.ustx` export remains unchanged. `cargo fmt --manifest-path native/Cargo.toml
  -p project-session -p engine-wasm -p synth-server -p phonemizer -p domain` passed. The first
  `cargo check` found a missing `voicebank` dependency and an inferred JSON type in the new
  route; both were fixed. Final `cargo check --manifest-path native/Cargo.toml -p
  project-session -p engine-wasm -p synth-server` passed. No tests were run. Render currently
  consumes the selected language-level hint through existing phonemizers; exact strategy/style
  reproduction, neighbor-aware alias rederivation, adapter conformance tests, and UI remain
  incomplete. Next: implement the render-side strategy resolver, then establish shared vectors before
  considering T013 complete.

- 2026-10-03T08:44:00Z: User asked to continue T013 through the non-UI backend/session work,
  defer frontend UI, and draft any additional requirements for merge with T014. Created a
  thread goal for engine/session + native/WASM adapter completion. Added T013 dependency and
  deferred IME requirements to T014 and linked the session/phoneme contract proposal. §6 remains
  unchanged. The current unfinished focus is strategy/style-accurate render rederivation,
  neighboring-note context, and shared adapter acceptance; UI remains deferred.

- 2026-10-03T07:29:07Z: Continued the Thai language slice. Added an ordered Thai
  orthography-to-VCCV converter port with vowel/onset/coda maps and longest-match tokenization;
  connected it to `PhonemeComposer` as `thai_vccv` candidates with bank alias resolution and
  explicit unresolved aliases. Updated the contract/inventory notes. `cargo fmt -p phonemizer`
  completed; builds and tests were not run. Still missing real Thai bank fixtures and
  neighbor/part-boundary transitions; session persistence, adapters, and editor remain open.

- 2026-10-02T08:23:17Z: Re-scoped T013 from the prior parity-only/session-visible phonemizer
  task to the user's layered phoneme input workflow: IME-style candidates, universal phone
  representation, language conversion, then voicebank/style-specific aliases. The frozen
  §6 contract means host commands/routes need a versioned contract update first. Existing
  dirty phonemizer/server work is not attributed to this task.
- 2026-10-02T08:26:00Z: Added the first composition workflow/DTO proposal and audited the
  current phonemizer paths. Confirmed current language detection is Japanese-vs-English,
  IPA coverage is an English subset, adapter mappings are alias-manifest based, and existing
  universal rendering combines language and bank strategies. No code/test changes yet.
- 2026-10-02T08:27:33Z: Set a working initial language assumption of Thai/English/Japanese
  from the project/user context so implementation could proceed. Thai remains an explicit
  coverage gap.
- 2026-10-02T08:28:49Z: Added a versioned logical read/command outline. Proposed committed
  state is universal phones + language/mode/style; bank aliases stay derived. A Rust-owned
  sidecar keyed by stable note ID is the leading persistence option, pending save-as and
  sidecar lifecycle review.
- 2026-10-02T08:29:30Z: Fixture inventory confirms the repository has a Teto English UTAU bank
  (`test/golden/teto-english/library/voice/oto.ini`) but no Thai or Japanese UTAU `oto.ini`
  fixture. Existing Japanese phonemizer tests use code/test fixtures rather than a checked-in
  Japanese voicebank. Initial end-to-end style parity must therefore start with Teto English;
  add real-bank fixtures before claiming broader language × style coverage.
- 2026-10-02T08:44:33Z: User confirmed the first proposed language set, Thai/English/Japanese.
  The earlier assumption is now a confirmed requirement.
- 2026-10-02T09:00:06Z: Assessed importing OpenUtau-compatible phonemizers. OpenUtau `.dll`
  files are managed C# subclasses loaded through OpenUtau's plugin host and receive OpenUtau
  singer/project/note context; they cannot run in the current Rust engine via file import alone.
  Plugin-specific YAML dictionaries are a smaller supported-import target. Recorded a staged
  recommendation in the design proposal; no plugin loader or importer implemented.
- 2026-10-02T09:06:05Z: Clarified the two DLL options. Metadata inspection/decompilation can
  help a developer port a named plugin, but cannot reliably generate declarative rules from
  arbitrary executable CIL. Running DLLs needs a managed plugin host, dependency resolution,
  OpenUtau API/model adapters, and a browser plan. Recorded recommendation: import known data
  formats and port selected phonemizers to Rust; treat a managed host as a separate experiment.
- 2026-10-02T09:24:53Z: Corrected the prior interpretation after user clarification: the target is
  for Rusted UTAU to host OpenUtau phonemizer DLLs without bundling .NET, not to export a Rusted UTAU plugin
  for OpenUtau. Recorded that managed DLL execution still requires a compatible runtime and
  OpenUtau API/dependency/context bridge; desktop may use a user-installed runtime, while
  Android/Web cannot assume one under the stated constraint. No runtime host or plugin loader
  implemented.
- 2026-10-02T09:27:10Z: User steered away from DLL/.NET execution and toward using Rusted UTAU's own
  framework. Updated T013 to port selected OpenUtau phonemizer behavior and import compatible
  dictionaries as data; the existing Rust `Phonemizer`/`G2p` framework is the implementation
  target. A port is per-phonemizer behavior compatibility, not automatic DLL conversion or a
  generic third-party plugin loader.
- 2026-10-02T10:28:24Z: User noted the existing local-server path and wants the Web/WASM editor to
  try OpenUtau plugin use too. Audited the current integration: `synth-server` already exposes
  `POST /phonemize` and Flutter uses loopback HTTP, but that endpoint currently calls Rust only;
  Web normally uses in-process Rust/WASM. Updated the proposal to put an optional managed
  compatibility host behind HTTP first, then evaluate direct browser .NET/WASM separately.
  The server boundary avoids a .NET runtime in clients but does not remove it from an Android
  server running on the same device.
- 2026-10-02T11:18:42Z: Built `poc/openutau-browser-wasm/` against the checked-in OpenUtau.Core
  source and proved that Chromium Browser WASM can load a managed phonemizer DLL from bytes,
  discover its real `OpenUtau.Api.Phonemizer` base type, and invoke `Process()`. Both a bundled
  plugin and a DLL selected from the local file picker returned `wasm:ka`; the sample does not
  need Rust or a server call. Cold production publish measured 38,254,882 encoded framework
  bytes (155 resources), versus a 3.1 MB plain .NET WASM baseline; OpenUtau.Core itself was
  28,056,708 bytes. Ready UI was 830 ms on loopback. After plugin load, managed GC heap was
  about 4.4 MB and the WASM linear-memory buffer capacity was 92.8 MiB. The full Core graph is
  the large component, so keep this route optional and lazy if pursued. Standard Blazor
  `InputFile` change handling threw `NullabilityInfoContext_NotSupported` in trimmed publish;
  the POC now uses browser `File.arrayBuffer()` interop. No user-targeted plugin DLL was found,
  so third-party compatibility, singer/context adaptation, and plugin-specific dependencies
  are still unproven.
- 2026-10-02T11:50:26Z: Kept the browser POC on .NET 8 and compiled the checked-in OpenUtau
  `JapaneseVCVPhonemizer.cs` source unchanged into a separate plugin DLL. Chromium Browser WASM
  loaded it dynamically and returned `- な` for a starting note and `a な` after `か`. This uses
  an in-memory `USinger`/Oto alias map, not a real Japanese voicebank or a distributed third-party
  plugin package. The 9,216-byte plugin loaded in 5.1 ms and processed the final case in 0.9 ms
  in one local run. Third-party dependencies and real voicebank behavior remain unverified.
- 2026-10-02T12:28:25Z: Compiled all 47 attributed classes in the checked-in official Builtin
  source (including one DEBUG-only class) into a .NET 8 DLL, combined them with 26 attributed
  Core phonemizers, loaded that DLL from bytes in Browser WASM, and tried all 73. Browser results:
  41 clean outputs, 2 partial outputs with unsupported desktop API warnings, 7 Browser WASM API
  or host-dependency limits (including `Process` and unregistered `shift_jis`), 2 Vogen
  construction failures because native `onnxruntime` is absent, and 21 cases that need their real
  singer/model/server context. The same harness returned outputs for 49/73 on
  native .NET 8. See the complete per-class result table in
  [`T013-openutau-official-phonemizer-matrix.md`](T013-openutau-official-phonemizer-matrix.md).
- 2026-10-02T13:21:25Z: Used the user's local, gitignored `test-ustx-example/king.ustx` as a
  real Classic test project. The five tracks select `WORLDLINE-R` and the official
  `JapanesePresampPhonemizer`; 1,124 notes produced 1,124 bank-resolvable aliases using
  metadata-only extraction from `TETO-OUset240323.zip` (16 files / 284,976 bytes; no WAVs).
  Compared the alias sequence with the Rust `JapaneseVcvPhonemizer`: 1,124/1,124 exact
  matches. The run first exposed missing small-yōon tail-vowel context in Rust (seven
  mismatches); added the mapping and a focused regression test, then reran to full match.
  This checks phoneme aliases and oto lookup only; it does not render or compare audio or
  exercise the selected WORLDLINE-R renderer. Added `.gitignore` coverage for the whole
  local sample/voicebank folder and verified both ZIP archives are ignored. Details and
  reproduction commands are in
  [`T013-openutau-official-phonemizer-matrix.md`](T013-openutau-official-phonemizer-matrix.md).
- 2026-10-02T13:45:25Z: Changed the native `.ustx` runner to load the official Builtin DLL with
  `Assembly.Load(byte[])`, discover/register all 47 plugin phonemizer types, then instantiate the
  class selected by each project's track. Ran all eight `.ustx` paths in `test-ustx-example/`;
  the two autosave pairs have identical SHA-256, leaving six unique projects and 4,949 unique
  notes. Each note produced one phone across Japanese Presamp and Core Default. King and Golden
  matched Rust Japanese VCV aliases exactly (1,124/1,124 and 1,709/1,709); Incomplete Intensevoice
  had 3 Oto misses and some Rust differences around `り2`, `い2`, and `っ`. Envybaby had 95
  aliases not present in the supplied Teto metadata; a raw Rust comparison is not meaningful
  because the input contains pre-phonemized and blank lyrics. `export-test` has no singer and was
  tested for Default output only. All sample files and both voicebank ZIPs remain gitignored.
  These are native .NET 8 project tests; the Browser WASM project importer and arbitrary user
  third-party DLL compatibility are still unverified. Full counts and reports are in
  [`T013-openutau-official-phonemizer-matrix.md`](T013-openutau-official-phonemizer-matrix.md).
- 2026-10-03T04:37:50Z (DSH session): Implemented the layered composition core
  `native/crates/phonemizer/src/composition.rs`: DTOs (`CompositionRequest/Result/Candidate`,
  `MappingMode`, `Style`, `Strategy`, `NotePronunciation`, `AliasResolution`), the
  `PhonemeComposer` candidate engine (direct alias → phonetic hint → language strategies
  ja-mora/ja-VCV/en-G2p/en-letters → universal IPA mapping; resolved-first stable sort;
  deterministic FNV ids; 256-char input bound; candidate limit clamped to ≤16), the
  committed-form persistence boundary (`NotePronunciation` with `resolve_aliases` rederivation),
  style restriction with honest diagnostics, Thai script detection reporting an explicit
  "converter not implemented" diagnostic, and forced-language-mismatch diagnostics. Canonical
  JSON pinned by new `tests/composition_conformance.rs` (4 vectors) and reproducible via
  `cargo run -p phonemizer --example t013_compose_json`. Wrote the versioned contract proposal
  `docs/planning/phoneme-compose-contract-proposal.md` (CONTRACT-v1 §6 untouched). Also fixed
  two pre-existing phonemizer-suite failures this session depends on: `tests/rusted_utau_dict.rs` used
  a stale absolute path `/home/seal/...` (now repo-relative), and the untracked `default.rs`
  doc block fenced as Rust (now `text`) broke `cargo test -p phonemizer --doc`. The pre-existing
  dirty state of `phonemizer/{Cargo.toml,japanese.rs}` and `default.rs` is not owned by this
  task; this session's deltas there are the serde dependency, one `pub` visibility change
  (`is_pure_vowel_mora`), and the doc fence respectively.
- 2026-10-03T04:57:47Z (same session): Completed next steps 1 and 4. (1) Dictionary inventory:
  OpenUtau Core ships 11 compressed G2P dictionaries in
  `ref-openutau-clean/core/OpenUtau.Core/G2p/Data/` (English `g2p-arpabet.zip` 2.4 MB +
  `g2p-arpabet-plus.zip` 4.1 MB, de/es/fil/fr/it/ko/pt/ru, and an 11 KB `g2p-ja-mono.zip`);
  there is **no Thai dictionary in OpenUtau Core**. (2) Thai gap quantified:
  `ThaiVCCVPhonemizer.cs` (340 lines) is entirely rule-based — inline vowel/consonant tables
  (33 vowel patterns with a `c`/`x` pattern grammar, 40 onset + 24 final mappings), tone-mark
  stripping, greedy (C, diphthong, V, ending) split, and alias chains with position rules; a
  Rust port is bounded (same shape as `JapaneseVcvPhonemizer`) and needs no dictionary — only a
  real Thai VCCV bank fixture is missing. (3) Plugin-DLL import requirements probed with a new
  runnable checklist `poc/openutau-browser-wasm/PluginImportProbe/` (native .NET 8): reads a
  plugin from bytes, loads it into a collectible `AssemblyLoadContext`, resolves
  OpenUtau.Core/Api from the host, scans `Phonemizer` subclasses, reads the
  `[Phonemizer(name, tag, author, language)]` metadata, runs a lifecycle probe
  (create → SetSinger → SetTiming → SetUp → Process), enumerates embedded resources, and unloads
  the context. Both saved reports are 8/8 ok: the 9,216-byte Japanese-VCV plugin DLL
  (`Process("か")` → `- か`, identical to our Rust composition candidate) and the official
  Builtin.dll (47 classes; attribute language ids EN×8, KO×7, ZH×4, ES×4, FR×3, JA×3, VI×3,
  ZH-YUE×2, FIL×2, DE×2, IT×2, RU×2, PT/PL/TH/TR×1). .NET 8 SDK was installed user-level at
  `~/.dotnet` (this machine had none; prior .NET evidence came from another machine). Findings
  written to [`docs/planning/t013-dictionaries-and-plugin-import.md`](../../planning/t013-dictionaries-and-plugin-import.md).
- 2026-10-03T05:10:50Z (same session): Re-ran the .NET **Browser WASM** path end-to-end on this
  machine (prior evidence came from another host): published the Blazor WASM POC host fresh
  (`dotnet publish` exit 0, same two non-fatal IL2105 warnings), served the precompressed
  payload, and drove it in the local Chromium. Verified: probe plugin (5,632 B) loaded from
  bytes and `Process("ka")` → `wasm:ka` (boot 985 ms, payload 36.6 MB, Process 0.2 ms);
  Japanese VCV plugin → `- な` / `a な` after `か` (identical to native and to the Rust
  composition candidate); official matrix 73/73 rows — PASS 41 + PARTIAL 2 (the same 43
  phonemizers returning phones as Oct-02), NEEDS CONTEXT 24, NATIVE ML RUNTIME 2, PROCESS
  ERROR 4 (`Process_*` PlatformNotSupportedException: German Diphone, EN Arpasing, FR
  CMUSphinx, ZH CVV Monophone). All three flows ran in-browser with no server plugin call.
  Findings appended to
  [`docs/planning/t013-dictionaries-and-plugin-import.md` §3b](../../planning/t013-dictionaries-and-plugin-import.md).

## Verification

- 2026-10-03T09:38:54Z:
  - `cargo test --manifest-path native/Cargo.toml -p phonemizer` — PASS (45 library tests,
    5 adapter, 4 composition conformance, 6 IPA, 2 dictionary, 5 universal; doc tests pass).
  - `cargo test --manifest-path native/Cargo.toml -p project-session --test pronunciation_session`
    — PASS (2).
  - `cargo test --manifest-path native/Cargo.toml -p synth-server --test api_v2_sessions` — PASS
    (31, including native HTTP ↔ direct WASM compose/commit/save/reopen parity).
  - `cargo test --manifest-path native/Cargo.toml -p engine-wasm --test shared_adapter_vectors`
    — PASS (6).
  - `cargo test --manifest-path native/Cargo.toml -p synth-cli committed_pronunciation_tests`
    — PASS (2, including Japanese VCV neighbor context).
  - `cargo test --manifest-path native/Cargo.toml -p synth-cli --test default_phonemizer_parity`
    — PASS (4).
  - `cargo check --manifest-path native/Cargo.toml --target wasm32-unknown-unknown -p engine-wasm`
    — PASS.
  - `cargo fmt --manifest-path native/Cargo.toml --check -p project-session -p synth-cli
    -p engine-wasm -p synth-server -p phonemizer` and `git diff --check` — PASS.

- 2026-10-03: `cargo fmt --manifest-path native/Cargo.toml -p project-session -p synth-cli
  -p engine-wasm -p synth-server` — PASS.
- 2026-10-03: `cargo check --manifest-path native/Cargo.toml -p project-session -p synth-cli
  -p engine-wasm -p synth-server` — PASS; unrelated warnings remain in diffsinger, moe-train,
  synth-cli and synth-server. Tests were not run in this continuation.

- `dotnet build poc/openutau-browser-wasm/Host/OpenUtauBrowserWasmHost.csproj -c Release` — pass,
  0 warnings / 0 errors after the POC's source fixes. The upstream Core build has many existing
  nullable warnings on a clean build.
- `dotnet publish ... -c Release` — pass; emits two non-fatal IL2105 warnings from NumSharp's
  debugger-proxy lookup.
- Chromium headless against the published gzip-serving POC — pass: cold launch, in-browser
  assembly load, OpenUtau type discovery, `Process()` result, and local DLL picker path.
- Chromium headless against the live .NET 8 Browser WASM host at `?autoofficial=1` — pass:
  dynamically loaded official Builtin DLL, discovered 73 official classes, invoked the matrix,
  and rendered all result rows. The test records runtime/context limits from the synthetic singer
  and missing models.
- Native .NET 8 matrix runner — pass: tried all 73 attributed classes and produced results for 49.
- Native .NET 8 project adapter — pass on all eight sample project paths; dynamically loaded the
  official Builtin DLL from bytes, discovered 47 plugin classes, and emitted 4,949 phones for
  4,949 notes across six unique project contents.
- Rust/OpenUtau alias checks — King 1,124/1,124 and Golden 1,709/1,709 exact; additional projects
  showed data-specific unresolved aliases and parity gaps, recorded in the matrix report.
- `cargo test -p phonemizer` — pass, 7 suites green (45 lib incl. 17 new composition unit
  tests; adapter 5; ipa 6; rusted_utau_dict 2; universal 5; conformance 4; doc 0). Includes the two
  pre-existing failures fixed this session (rusted_utau_dict path, default.rs doc fence).
- `cargo check -p phonemizer --target wasm32-unknown-unknown` — pass (composition core is
  wasm-safe; serde only).
- `cargo run -p phonemizer --example t013_compose_json` — pass; emits the canonical
  request/result/style-restriction/commit JSON used verbatim in the contract proposal.
- `cargo clippy -p phonemizer --all-targets` — 2 pre-existing warnings in `default.rs` and
  `tests/universal_test.rs` (unowned); composition code is warning-free.
- No broad workspace regression suite was run this turn; the composition core is additive and
  the phonemizer suite is green.
- `dotnet build poc/openutau-browser-wasm/PluginImportProbe -c Release` after the Yaml field
  shim — pass; probe of `EN TETO (net8).dll` + real Teto English bank dir: import 8/8, SetSinger
  writes/reads `en_teto.yaml`, Process completes with empty output (synthetic note).
- Browser matrix with random per-language lyrics + fixture singers (`?autofull=1`): 73/73 rows,
  PASS 40 + PARTIAL 2, NEEDS CONTEXT 25, NATIVE ML RUNTIME 2, PROCESS ERROR 4; per-language
  evidence captured (FR "lune" → `- lune · lune -`; KO→JA kana via ja_vcv singer).
- Browser generic `?autodll=1&dll=plugins/EN-TETO-net8.dll&singer=en_vccv`: loads and binds
  (no MissingFieldException) then hits `Process_PlatformNotSupported` — plugin belongs on the
  server-fallback path per architecture.
- Known follow-up bugs left open (non-blocking for import): Tetomizer needs real
  note-group/syllable input (empty output on synthetic notes); `GetRandomSampleInput` has a
  guarded fallback path (verify per-language pools once more against real usage).
- `dotnet build poc/openutau-browser-wasm/PluginImportProbe -c Release` — pass (0 errors; .NET 8
  SDK 8.0.425 user-level install).
- `dotnet run .../PluginImportProbe -- ../Host/wwwroot/plugins/T013.OpenUtauJapaneseVcv.Plugin.dll か`
  — pass, 8/8 steps ok, probe output `- か`; report saved at
  `poc/openutau-browser-wasm/artifacts/t013-plugin-import-probe-japanesevcv.json`.
- `dotnet run .../PluginImportProbe -- ../Host/wwwroot/plugins/OpenUtau.Plugin.Builtin.dll な`
  — pass, 8/8 steps ok, 47 classes discovered with per-class metadata + lifecycle probes; report
  saved at `poc/openutau-browser-wasm/artifacts/t013-plugin-import-probe-builtin.json`.
- Dictionary inventory is a read-only source audit (sizes quoted from `ls -la`); no dictionary
  was imported into Rust yet.
- `dotnet publish poc/openutau-browser-wasm/Host/OpenUtauBrowserWasmHost.csproj -c Release`
  — pass (exit 0; two non-fatal IL2105 NumSharp warnings, as on Oct-02).
- Chromium (in-app, real-time) against `?autoprobe=1` — pass: `wasm:ka`, boot 985 ms, payload
  36.6 MB encoded, memory buffer 98.8 MB.
- Chromium against `?autoofficial=1` — pass: 73/73 rows rendered in-browser (41 PASS, 2
  PARTIAL, 24 NEEDS CONTEXT, 2 NATIVE ML RUNTIME, 4 PROCESS ERROR = the known `Process` browser
  limits). Note: headless `--dump-dom --virtual-time-budget` stalls at ~99% boot on this
  machine — verify with a real-time browser session, not virtual time.
- `cargo test --manifest-path native/Cargo.toml -p phonemizer --lib yoon_morae_preserve_their_tail_vowel`
  — pass.
- `git check-ignore -v` — confirmed the project and both voicebank ZIP archives match the
  `test-ustx-example/` rule.
- No broad Rust/application regression suite was run; this turn ran the requested official
  phonemizer matrix, actual-project alias comparison, and Browser WASM dynamic-load check.

## Blockers and dependencies

- Initial Thai/English/Japanese set is confirmed. The model remains extensible.
- No blocker remains for the T013 backend/session scope. The versioned composition proposal is
  still unapproved; keep §6 frozen and carry the draft through T014 contract review before any UI
  starts.
- No redistributable Thai voicebank fixture is present; real-bank alias parity is unverified and
  remains optional follow-up. Current Thai mapping has an in-memory alias fixture.

## Next concrete steps

T013 implementation is complete. The next contract review should merge the additional UI IME
requirements in T014 and review `phoneme-compose-contract-proposal.md`; only after that review
should any editor UI be started. A real Thai voicebank parity run is optional follow-up when a
redistributable fixture becomes available.

Historical managed-plugin loading experiments are recorded above and in the T013 plugin matrix;
they are evidence only. User direction moved Rusted UTAU toward its own Rust phonemizer framework and
away from a managed compatibility host.

## Phonemizer selection moved into the `phonemizer` crate (2026-10-04T05:10:00Z)

`PhonemizerKind` and its `from_track` resolver — the rule that decides which phonemizer
derives a track — lived in `native/tools/synth-cli/src/pipeline.rs`, inside a **CLI tool
crate**. That is the wrong owner twice over: it is musical state, and it was unreachable to
every consumer that is not the CLI. `engine-wasm` depended on `synth-cli` (a binary crate) for
16 call sites, several of which were only reaching for this one decision.

### What moved

`native/crates/phonemizer/src/selection.rs` (new). `pipeline.rs` now re-exports
`pub use phonemizer::PhonemizerKind;`, so every existing call site compiles unchanged and there
is exactly one implementation. Resolution order:

1. `RUSTED_UTAU_PHONEMIZER=default` — the T12 A/B override, unchanged.
2. Exact match against OpenUtau's **registered** phonemizer keys, case-insensitive.
3. Substring fallback, in a fixed order.
4. Everything else → `Universal`.

The exact table is not invented: `OpenUtau.Core.DefaultPhonemizer` and the Japanese
Presamp/CVVC keys were read out of the checked-in reference sources
(`ref-openutau-clean/core/OpenUtau.Core`, `ref-openutau-clean/desktop-ref`), and the Presamp key
is the string the real `.ustx` fixtures actually store.

### Two defects the move exposed

**A vacuous test.** `pipeline::load_project` cannot parse the `test-ustx-example/` fixtures —
they are multi-document YAML and serde rejects them ("more than one document is not
supported"). That is a **pre-existing loader limitation, not fixed here**. The new fixture test
therefore reads `phonemizer:` keys straight out of the file text rather than through the
loader, so it still pins the rule against real project text. Worth fixing separately: OpenUtau
projects are multi-document, so the loader cannot open a real-world `.ustx`.

**An empty-string hole.** The old resolver called `to_ascii_lowercase()` on the raw name
without trimming, so `phonemizer: ` (present but empty after a YAML round-trip) produced an
empty string that fell through the substring chain. Blank names now resolve to `Universal`.

### One thing I got wrong, and the test that caught it

I first made an explicitly-English track name reach `PhonemizerKind::English`, reading the
variant being unreachable from `from_track` as a bug. `synth-cli`'s
`phonemizer_kind_from_track_name` **FAILED** and was right to: routing English there is a
deliberate decision, documented in that test. The legacy English CVVC path falls back to the
lyric verbatim, so an unknown word ("shi") finds no oto alias and the **whole phrase is
skipped** — a song renders only its first phrase. Universal always produces phonemes.

Reverted, and the reasoning is now pinned in both places so it is not "fixed" again later.
`English` stays reachable only through the explicit `--phonemizer english` CLI flag.

### Matching on the `ja` language token

My first Japanese test failed on `JA CVVC` — OpenUtau's own registered alias for the Japanese
CVVC phonemizer — because `cvvc` matched the English branch before any Japanese marker. The
real signal is that OpenUtau registers `[Phonemizer(name, alias, …, language: "JA")]`, so the
short alias a user types is language-prefixed. `is_japanese_name` now matches the `ja` token as
a **whole word**, so `FIL VCV & CVVC` (Filipino) and `Fijian` do not get captured. Bare `vcv`
was dropped as a Japanese marker entirely: it is ambiguous, and Universal delegates kana to VCV
anyway.

### Verified

- `cargo test -p phonemizer` → **76 pass / 0 fail** across all targets (was 67; +9 in
  `selection.rs`).
- `cargo test -p synth-cli` → **29 pass / 0 fail / 15 ignored** (+3 in the new fixture test).
  The pre-existing `phonemizer_kind_from_track_name`, `from_track_maps_default_explicitly…`,
  and the 4 DefaultPhonemizer parity tests all still pass.
- `cargo test -p synth-cli --test phonemizer_selection_fixtures -- --nocapture` →
  **checked 31 real fixture phonemizer keys** and **6 golden tracks**. The 31 is
  independently confirmed: `king.ustx` 5, `golden-the-end-of-hutsune-miku.ustx` 4,
  `envybaby.ustx` 11, `incomplete-intensevoice.ustx` 11 — all
  `OpenUtau.Plugin.Builtin.JapanesePresampPhonemizer`.
  An earlier revision of this file passed **vacuously** because `repo_root()` was one level
  short, so all three tests silently skipped; the fixed version asserts on the checked-in
  fixtures instead of skipping, and asserts `checked > 0` so it cannot pass by proving nothing.
- `cargo build --workspace` → clean; `cargo clippy -p phonemizer --lib` → no findings in
  `selection.rs`. (Clippy on the workspace still fails in `worldline-sys` — **pre-existing**,
  that crate is untouched by this work.)
- Host regression, because the render path runs this code:
  - `./web/run-editor.sh` → **OK, exit 0**
  - probe `?backend=http` → **all checks passed, exit 0**
  - probe `?backend=wasm` → **all checks passed, exit 0**
  - `node --test tests/` → 74 tests, **71 pass / 0 fail**, 3 skipped
- `git diff --check` → clean.

### Still owed

- `engine-wasm` still depends on `synth-cli` for 16 call sites. Moving the selection rule out
  was the cheap half; the pipeline/chunking/mixing half is a larger extraction and was not
  attempted.
- The multi-document `.ustx` loader limitation above.
- Thai has `word_to_phonemes` in `thai.rs` but no `impl Phonemizer`, so it is unreachable from
  `from_track` and Thai lyrics go to Universal.
- No real Thai voicebank fixture; alias parity remains an in-memory fixture.
