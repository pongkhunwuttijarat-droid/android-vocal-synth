# Phoneme input and layered voicebank mapping

Status: design in progress · 2026-10-02
Update 2026-10-03: the composition core is implemented in
[`native/crates/phonemizer/src/composition.rs`](../../native/crates/phonemizer/src/composition.rs)
(DTOs, bounded deterministic candidates, committed-form persistence boundary),
with the versioned contract proposal and canonical JSON in
[phoneme-compose-contract-proposal.md](phoneme-compose-contract-proposal.md).
Session/adapter/UI wiring below is still pending.

## User workflow

The lyric editor behaves like an IME:

1. The user types a word or phonetic spelling into a note.
2. Rust returns a bounded list of pronunciation/composition candidates as the text changes.
3. Each candidate shows its language-derived phones, universal phone form, and the selected
   voicebank's playable aliases. An unresolved phone is visible in the candidate instead of
   being silently replaced or dropped.
4. The user chooses a candidate and an output view: universal phones or the voicebank's native
   aliases. Keyboard navigation and confirmation are presentation behavior; candidate meaning
   and the committed selection belong to Rust.

Composition text, highlighted row, and popup visibility are transient UI state. Once chosen,
the selected pronunciation/mapping mode is committed through the Rust session; render and save
consume that committed state.

## Mapping layers

```text
typed text / phonetic hint
       │ language-specific G2P or syllabary conversion
       ▼
language phone sequence
       │ normalize / convert through the universal inventory (IPA-based)
       ▼
universal phone sequence
       │ map against the selected voicebank's declared inventory and aliases
       ▼
voicebank-native alias sequence, tagged by its supported sequence style
```

The user may commit either the universal sequence or the bank-native sequence. A selected
voicebank's alias inventory is a mapping target, not evidence that an absent sound can be
rendered. Mapping must report unsupported phones explicitly. CV, VCV, CVVC, and VCCV are
voicebank-specific sequence/mapping strategies; they are not alternate universal phoneme
inventories and should not be inferred solely from a bank's display name.

## Proposed logical DTO (contract proposal, not yet frozen)

```json
{
  "composition_id": "opaque-stable-id",
  "input": "typed text",
  "language": "language-id",
  "voicebank_id": "voicebank-id",
  "mode": "universal | voicebank_native",
  "style": "auto | cv | vcv | cvvc | vccv",
  "candidates": [
    {
      "candidate_id": "opaque-stable-id",
      "display_text": "chosen spelling",
      "language_phones": ["..."],
      "universal_phones": ["..."],
      "voicebank_aliases": ["..."],
      "style": "cvvc",
      "unresolved_phones": []
    }
  ]
}
```

The cross-host logical operations are:

- `phoneme.compose` (read): `{project_id, part_id, note_id, expected_revision, input,
  language_id, voicebank_id, mode, style, limit}` → revision-tagged bounded candidate DTO.
- `note.set_pronunciation` (command): `{project_id, part_id, note_id, expected_revision,
  language_id, mode, style, universal_phones}` → one committed edit/undo step and the
  resulting pronunciation read model.

Both operations use stable project/part/note IDs. Candidate IDs are deterministic for the
request context but are transient; the commit carries the selected canonical phone sequence,
not a UI row index. Failed validation must not partially mutate the project. The exact wire
names and canonical JSON need review in the new contract revision before implementation.

### Persistence boundary

The committed semantic form is the universal phone sequence plus language ID, mapping mode,
and style ID. Bank aliases are derived engine output, so changing the selected voicebank can
recompute them without changing the universal choice. Ordinary imported USTX notes have no
such metadata and continue to use their lyric/phonetic hint as input.

The persistence mechanism needs to preserve the semantic choice across Rusted UTAU save/reopen while
keeping render inputs engine-owned. A versioned Rust-owned project sidecar keyed by stable note
ID is the leading option; the normal USTX lyric/phonetic hint remains the interoperable fallback
for external tools. Confirm compatibility with save-as, sidecar lifecycle, and USTX round-trip
before settling the storage schema. Do not place a second copy in frontend project state.

## Current implementation inventory

- `native/crates/phonemizer/src/ipa.rs` has a small ARPABET↔IPA subset, currently focused on
  the Teto English bank rather than a complete universal inventory.
- `native/crates/phonemizer/src/detect.rs` currently recognizes Japanese kana and otherwise
  treats text as English. Language conversion needs a distinct extensible registry, not a
  growing binary fallback.
- `native/crates/phonemizer/src/adapter.rs` maps IPA tokens to the voicebank manifest and
  nearest supported symbols. The manifest currently records single phonemes and adjacent
  pairs, not declared CV/VCV/CVVC/VCCV behavior or candidate provenance.
- `native/crates/phonemizer/src/universal.rs` combines language detection, English subset G2P,
  IPA mapping, and Japanese VCV rendering. It is a useful compatibility path, but not yet the
  layered composition contract above.
- Flutter and Web lyric editors currently derive/display phonemes locally or through legacy
  server helpers. The UI should be switched only after the common Rust read/command contract
  exists.

## Contract/versioning note

`CONTRACT-v1.md` §6 is frozen. Composition candidate reads and commit commands must go through
the documented versioning/review process and be added as a new contract revision with shared
conformance vectors before native/WASM UI wiring. Do not present existing `/phonemize` or
legacy callbacks as the new stateful session API.

## OpenUtau plugin import feasibility

The reference OpenUtau installer accepts `.dll` files by copying them to its Plugins folder.
At startup it scans managed assemblies, loads exported types, and registers classes derived
from `OpenUtau.Api.Phonemizer`. That API is not a standalone `text -> phones` function: it
receives a singer and grouped note context (extenders plus neighboring notes), and plugins may
load singer-specific files from the bank. Therefore a downloaded OpenUtau `.dll` cannot be
executed by the current Rust engine just by importing the file.

OpenUtau also has auxiliary YAML dictionaries/configuration for some built-in phonemizers.
Those are more practical to import, but their schemas and behavior are plugin-specific; they do
not turn arbitrary DLL plugins into declarative rules.

Recommended implementation order for T013:

1. Import supported, documented dictionary/config files as data and apply them in Rust through
   the shared engine contract.
2. Add OpenUtau-compatibility profiles for selected language/bank phonemizers and compare their
   outputs with OpenUtau reference vectors.
3. For a selected OpenUtau phonemizer, use its `.dll` as a behavior/reference artifact and port
   its algorithm into Rusted UTAU's Rust phonemizer interface; import its dictionaries/configuration
   as data where their schemas are understood. This avoids a managed runtime in Rusted UTAU, while
   keeping the compatibility work explicit per phonemizer.

Feasibility by scope: importing a documented dictionary is moderate; porting one selected
phonemizer into Rust is medium-to-high but bounded; promising support for arbitrary DLLs is
open-ended because their algorithms, dependencies, languages, and bank assumptions vary.

### Convert DLL logic or run the DLL

These are different projects:

- **Inspect/convert:** a managed DLL contains both descriptive metadata (types, method names,
  references, attributes) and executable CIL. .NET metadata tools can enumerate the metadata,
  and the CIL can be decompiled for a human to port. That does not automatically turn arbitrary
  control flow, singer-specific resource loading, and context-dependent behavior into our
  declarative phonemizer format. This is practical only for named plugins and requires a
  per-plugin converter/port plus parity cases.
- **Execute:** host the managed runtime, implement the OpenUtau API objects the plugin expects,
  resolve the plugin's dependent assemblies, then bridge the call into the Rust session.
  .NET's plugin model uses an assembly load context and dependency resolver for this; a native
  AOT build cannot use dynamic `Assembly.LoadFile` plugin discovery. Cross-host WASM support
  would need its own supported runtime and packaging path. If Rusted UTAU does not bundle .NET, the
  host must find a compatible runtime installed by the user; the plugin DLL cannot supply that
  runtime itself.

Current recommendation for T013: keep Rust as the default. The browser runtime path is proven
for one small managed plugin built against the checked-in OpenUtau API; inspect and test a
specific user-targeted DLL before claiming it compatible. Keep the existing local HTTP server
as a fallback for plugins that need desktop APIs/native dependencies or for users who do not
want the browser runtime payload.

## Initial language scope

The user confirmed Thai, English, and Japanese as the initial language set. English has the
current Teto/CVVC fixtures, and Japanese has the current kana/VCV path. Language IDs and
converter registrations remain extensible, and voicebank style is selectable independently
of language. Do not claim complete Thai G2P until a suitable dictionary/converter and coverage
evidence are in place.

## OpenUtau phonemizers in browser WASM

The user expects the selected `.dll` files to be phonemizer plugins (note-to-phone logic), with
little need for desktop UI or system integration. That makes a direct browser-WASM experiment
more plausible than hosting all of OpenUtau. Treat it as a screening assumption until the
specific DLLs and dependency manifests are inspected.

Recommended shape:

```text
Web/WASM editor
   ├─ default: in-process Rust phonemizer
   └─ selected managed plugin:
          .NET browser-WASM runtime
             └─ pinned OpenUtau API compatibility adapter
                   └─ selected, browser-compatible phonemizer DLL

Fallback for incompatible plugins: Web/WASM UI → local HTTP → synth-server/.NET host
```

The direct path does not need to reproduce OpenUtau's UI or audio renderer. It does need to
provide the plugin API types and lifecycle, and translate notes, singer info, timing, hints,
extenders, neighbors, and singer resources. Current OpenUtau `OpenUtau.Core` targets `net8.0`
and its phonemizer API is explicitly experimental. The API allows plugins to load singer-
specific resource files, so browser-safe access to dictionaries/configuration still needs a
resource adapter; not every plugin is entirely computation-only.

Screen each candidate DLL for target framework, `OpenUtau.Core` API version, managed/native
references, P/Invoke, reflection-based loading, and singer-specific files. A pure managed
phonemizer with data files and no native calls is a good first browser-WASM candidate. Plugins
that require native libraries or unavailable desktop APIs can use the local server fallback.

### Runtime POC result — 2026-10-02

`poc/openutau-browser-wasm/` builds a small phonemizer against the repository's OpenUtau.Core
source, then imports its DLL both as a bundled lazy asset and through the browser file picker.
The .NET Browser WASM runtime loaded the real `OpenUtau.Api.Phonemizer` type and invoked
`Process()` in Chromium; both paths returned `wasm:ka` without Rust or an HTTP plugin server.
This proves that dynamic managed assembly loading is possible in this browser-WASM host.

Production publish numbers on this checkout: **38,254,882 encoded bytes** (36.5 MiB) across 155
framework resources, and **830 ms** to the ready UI on loopback. A plain .NET WASM baseline was
**3.1 MB** compressed, so this OpenUtau Core host adds about **35.2 MB**. `OpenUtau.Core.wasm`
alone is **28,056,708 encoded bytes**. After plugin load, the managed GC heap reported about
**4.4 MB**; the current WebAssembly memory buffer capacity was **92.8 MiB**. The sample's
`Process()` took about 0.3 ms in the measured run. Treat timings and memory as this machine's
single local-browser measurement, not a product guarantee.

The current size includes the checked-in Core assembly and its broad dependency graph, not just
the minimal .NET runtime and plugin. Direct execution works technically but should be optional
and lazy if productized. The sample passes a null singer and a single note; it does not prove the
adapter for voicebanks, note grouping, timing, neighbors, or per-bank files. No user-targeted DLL
was found, so the next compatibility gate is to inspect and run one exact real plugin. The
standard Blazor `InputFile` handler also failed in trimmed publish with
`NullabilityInfoContext_NotSupported`; direct browser `File.arrayBuffer()` interop fixed the
POC's local DLL import.

The repository already has a server transport seam: Flutter talks to `synth-server` over
loopback HTTP, and the server exposes `POST /phonemize`. That route currently calls the Rust
phonemizer and reads a project file; it is not a generic plugin or live composition endpoint.
The Web editor normally calls Rust in-process. If the browser plugin host is incompatible or
too costly, add a versioned local/server HTTP request path for selected plugin results. A
server on the same Android device still needs its managed runtime; a desktop/LAN server can use
an installed compatible .NET runtime while browser and Android clients keep theirs out.

Current OpenUtau `OpenUtau.Core` targets `net8.0`, and its phonemizer API is explicitly
experimental. A compatible host must provide the matching API assembly/dependencies and map
Rusted UTAU's singer, note groups, previous/next note context, timing, phonetic hints, and singer
resources into the OpenUtau calls. The minimal host does not need to reproduce OpenUtau's UI,
but it does need the API objects and lifecycle expected by each selected plugin. Pin the API
version and start with a curated plugin list; a stock .NET runtime by itself is not the
OpenUtau environment.

Effort for one pure-managed plugin is moderate-to-high: adapt the OpenUtau contract, package
its managed dependencies, and test browser resource access. Supporting arbitrary plugins stays
high effort. The measured browser cost is a second runtime and managed assemblies in addition to
Rust/WASM; the sample's full OpenUtau Core package adds about 35.2 MB over a plain .NET WASM app.
Lazy loading could keep this off the default path, and a smaller compatible API surface is the
next size experiment. See [Blazor download-size guidance](https://learn.microsoft.com/en-us/aspnet/core/blazor/performance/app-download-size?view=aspnetcore-10.0)
and [assembly lazy loading](https://learn.microsoft.com/en-us/aspnet/core/blazor/webassembly-lazy-load-assemblies?view=aspnetcore-10.0).

Next proof step: identify the user's candidate DLL, inspect the exact API/dependency/resource
requirements, and run that plugin through the browser POC with representative singer/note
context. Measure output parity and startup. If it requires unsupported desktop/native APIs, use
local HTTP hosting for that plugin and keep the Rust implementation as the cross-platform
default/fallback.
