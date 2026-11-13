# T013 — dictionaries inventory and OpenUtau plugin-DLL import requirements

Status: findings · 2026-10-03 (DSH session)
Scope: next steps 1 (dictionary inventory / Thai gap quantification) and 4
(specific-DLL import requirements) of
[T013-phoneme-input-and-mapping.md](../work/tasks/T013-phoneme-input-and-mapping.md).

## 1. G2P dictionary inventory

### OpenUtau Core ships compressed G2P dictionaries (`ref-openutau-clean/core/OpenUtau.Core/G2p/Data/`)

| Package | Size | Language | Note |
|---|---:|---|---|
| `g2p-arpabet.zip` | 2,459,789 B | English | CMUdict-derived ARPABET — direct upgrade path for our demo dictionary |
| `g2p-arpabet-plus.zip` | 4,327,034 B | English+ | larger ARPABET variant (Arpasing+) |
| `g2p-ja-mono.zip` | 11,028 B | Japanese | monophone kana mapping — tiny |
| `g2p-de.zip` / `g2p-de-marzipan.zip` | 1.67–1.89 MB | German | |
| `g2p-es.zip` | 1.78 MB | Spanish | |
| `g2p-fil.zip` | 1.79 MB | Filipino | |
| `g2p-fr.zip` / `g2p-fr-millefeuille.zip` | 1.67–1.77 MB | French | |
| `g2p-it.zip` | 1.69 MB | Italian | |
| `g2p-ko.zip` | 2.00 MB | Korean | |
| `g2p-pt.zip` | 1.68 MB | Portuguese | |
| `g2p-ru.zip` | 2.59 MB | Russian | |

**No Thai dictionary exists in OpenUtau Core** — see §2. Despite their names,
the English ZIPs are not simple word-to-phone dictionaries: each contains
`dict.txt`, `phones.txt`, and `g2p.onnx`, and `ArpabetG2p` runs the ONNX model
for unknown words. The existing Rust composer accepts host-supplied
`G2pFallbacks` (`PhonemeComposer::with_g2p`), but full compatibility requires
an ONNX inference path or a separately sourced static lexicon plus a defined
unknown-word fallback. Do not claim a ZIP-only dictionary import is complete.

### Rusted UTAU-side

`native/crates/phonemizer/src/rusted_utau_dict.rs` is a demo-vocabulary fixture, not
a dictionary. `g2p-arpabet.zip` is a potential English inference-model
integration, not a direct `G2pDictionary` data import. Japanese needs no
dictionary for kana (mora split), and Thai needs no dictionary (§2).

## 2. Thai gap quantification — OpenUtau Thai is rule-based, not dictionary-based

`ThaiVCCVPhonemizer.cs` (checked-in reference,
`ref-openutau-clean/desktop-ref/OpenUtau.Plugin.Builtin/`, 340 lines) embeds
all of its data inline:

- `RemoveInvalidLetters`: strips tone marks (`่ ้ ๊ ๋`), karan (`์`), `็`.
- `VowelMapping`: 33 orthographic patterns with a small grammar — `c` =
  `[ก-ฮ][ลรว]?|อ[ย]?|ห[ก-ฮ]?` (consonant cluster incl. อย/หน), `x` = optional
  final consonant `[ก-ฮ]?` — mapped to the VCCV symbol set
  (`a e i o u @ Q 3 6 1 8 ia ua ...`), incl. the special cases
  `ั+ว → ua` and `เ…ย → 3`.
- `CMapping` / `XMapping`: 40 onset chars and 24 final chars → symbols.
- Length-based fallbacks for unparsed words (1 char → `{c}Q`,
  2 → `{c}o{x}`, 3 → `{c}ua{x}` / `{c2}o{x}`, 4 → `{c2}ua{x}`).
- `ParseInput`: greedy split into `(consonant, diphthong r/l/w, vowel, ending)`.
- `Process`: alias candidate chains — CV `{c}{v}`; C+glide+V
  `{c}{d}{v}` else `{c}{d}`+`{d}{v}`; vowel-only notes bridge the previous
  ending/vowel (`{prevEnding}{v}` → `{prevV}{v}` → `{v}`); ending via
  `{v}{ending}` or VC into the next note `{v} {nextC}`; head `−`/tail `−`
  rewrite at part boundaries; verbatim-lyric fallback. Position rules: second
  phoneme at max(75 % of duration, VC position from the next note's oto),
  diphthong middle phoneme at min(10 %, 60 ticks).

**Conclusion:** the Thai coverage gap is not a missing dictionary — it is a
bounded port of tables + splitting rules into Rust, plus a real Thai VCCV bank
fixture for alias-resolution evidence (none is checked in today). Single-lyric
orthographic conversion and basic VCCV candidate mapping now live in
`phonemizer::thai` and `PhonemeComposer`; note-neighbour CV/VC transitions,
part-boundary aliases, timing, and a real-bank fixture remain open.

## 3. What an OpenUtau plugin-DLL import must do — probed, not claimed

New probe tool: `poc/openutau-browser-wasm/PluginImportProbe/`
(native .NET 8 console; loads a plugin DLL **from bytes** and walks the
import contract step by step, emitting JSON). Saved reports:

- `poc/openutau-browser-wasm/artifacts/t013-plugin-import-probe-japanesevcv.json`
  (9,216-byte plugin-shaped DLL)
- `poc/openutau-browser-wasm/artifacts/t013-plugin-import-probe-builtin.json`
  (official `OpenUtau.Plugin.Builtin.dll`, 1,234,944 bytes, 47 classes)

Both reports: **8/8 steps ok**. The steps are the requirement list:

1. **Read plugin bytes.** Import = copying a file; the host loads the
   assembly from bytes (`Assembly.Load(byte[])` equivalent), not from a
   plugin-directory scan. 9,216 B plugin and 1.23 MB official package both
   load fine.
2. **Load into an isolated, collectible `AssemblyLoadContext`.** Proves a
   plugin can be added/removed/reloaded without a host restart.
3. **Resolve dependencies from the host.** The plugin references
   `OpenUtau.Core`/`OpenUtau.Api` (and their trees — DryWetMidi, Serilog).
   The host must preload the pinned API assemblies; the plugin's refs then
   resolve through the default context. *This is why Rusted UTAU must pin the
   OpenUtau API version it hosts.*
4. **Scan exported types** for non-abstract `Phonemizer` subclasses.
   47 classes in the official package.
5. **Read `[Phonemizer]` metadata** — display name, tag, author, and the
   `language` id. The language ids found in the official package:
   EN×8, KO×7, ZH×4, ES×4, FR×3, JA×3, VI×3, ZH-YUE×2, FIL×2, DE×2, IT×2,
   RU×2, PT/PL/TH/TR×1, one null. This id is the host's key for mapping a
   plugin to the composition engine's language registry (our `Language`
   ids: `ja`/`en`/`th`); `TH` exists in the wild.
6. **Lifecycle probe:** `Activator.CreateInstance` → `Testing = true` →
   `SetSinger` → `SetTiming(project.timeAxis)` → `SetUp(groups, project,
   track)` → `Process(notes, prev, next, prevNeighbour, nextNeighbour,
   prevNeighbours)`. With the synthetic accept-all singer the JA VCV plugin
   returned `- か` — identical to our Rust composition engine's Japanese VCV
   candidate for the same input. Classes needing real singer resources
   (e.g. EN ARPA) still report their known errors — the probe surfaces them
   instead of hiding them.
7. **Enumerate embedded resources** (`GetManifestResourceNames`) — the
   import surface must also plan for plugin data files that load from the
   singer directory (classic plugins), which is a singer-resource adapter
   concern, not an assembly concern.
8. **Unload the collectible ALC** — plugin replacement/reload without a
   host restart.

### What the host must provide beyond the probe (from the Oct-02 matrix evidence)

- The OpenUtau note context: grouped notes (extenders attached to their
  leading note), prev/next neighbour arrays, tone, phonetic hint, per-note
  phoneme attributes.
- A `USinger` adapter over Rusted UTAU's voicebank (oto lookup with subbank
  prefix/suffix and tone mapping — `TryGetMappedOto`).
- A `UProject`/`TimeAxis` adapter (only used for timing queries).
- Browser WASM additionally needs the .NET runtime payload (~35 MB over a
  plain .NET WASM app, Oct-02 measurement) — keep the managed host lazy and
  optional; plugins needing `System.Diagnostics.Process`, `shift_jis`, or
  native `onnxruntime` stay on the native server fallback path.

## 3b. Browser-WASM re-run on the DSH machine — 2026-10-03

The previous Browser-WASM evidence came from another machine; this section
records a fresh end-to-end run on the current checkout host (this machine).

Reproduce:

```sh
export PATH="$HOME/.dotnet:$PATH"   # user-level .NET 8 SDK (8.0.425)
dotnet publish poc/openutau-browser-wasm/Host/OpenUtauBrowserWasmHost.csproj \
  -c Release -o poc/openutau-browser-wasm/artifacts/publish-fresh
python3 poc/openutau-browser-wasm/serve_published.py --port 5201 \
  --directory poc/openutau-browser-wasm/artifacts/publish-fresh/wwwroot
# then open http://127.0.0.1:5201/?autoprobe=1 (or ?autojapanesevcv=1 / ?autoofficial=1)
```

Verified in the local Chromium (in-app browser, real time):

- `?autoprobe=1` — 5,632-byte probe plugin loaded from bytes and invoked:
  `Process("ka")` → `wasm:ka`; load 2.7 ms, Process 0.2 ms; boot 985 ms;
  encoded .NET payload 36.6 MB; WASM memory buffer 98.8 MB.
- Japanese VCV plugin (9,216 bytes, same size as the Oct-02 build):
  `first note: - な | after か: a な` — identical behaviour to the native
  runner and the Rust composition candidate.
- `?autoofficial=1` — full official matrix in-browser: **73/73 rows**;
  PASS 41 + PARTIAL 2 (phones returned, same 43 as Oct-02), NEEDS CONTEXT 24,
  NATIVE ML RUNTIME 2, PROCESS ERROR 4 (German Diphone, EN Arpasing,
  FR CMUSphinx, ZH CVV Monophone — all
  `PlatformNotSupportedException: Process_*`, the known browser limit).
  Memory buffer grew to 142.2 MB after the full matrix.

Notes: this machine had no .NET SDK; 8.0.425 was installed user-level at
`~/.dotnet`. The static Chromium `--dump-dom --virtual-time-budget` probe
hangs at 99% (virtual-time vs WASM-compile interaction) — use a real-time
browser session instead of virtual time for verification.

## 3c. Real third-party DLL: EN TETO (Tetomizer) — what import actually took

The user supplied a genuine community plugin: `EN TETO (net8).dll` —
"OpenUtau.Plugin.Builtin.Tetomizer", "TETO EN+JP Phonemizer", tag `EN TETO v101`,
author Cadlaxa, language `EN`, 696,832 bytes, one embedded resource
(`EN_TETO.Data.Resources.resources`).

Import contract (probe): 8/8 steps ok. The instructive failure came at
`SetSinger`: `MissingFieldException: OpenUtau.Core.Yaml::DefaultDeserializer`.

Root cause (from ilspycmd IL): the plugin contains
`ldsfld class [YamlDotNet]YamlDotNet.Serialization.IDeserializer
[OpenUtau.Core]OpenUtau.Core.Yaml::DefaultDeserializer` — it was compiled
against a Core where `Yaml.DefaultDeserializer` is a **static readonly FIELD of
the raw `IDeserializer` interface type**. Every Core here (checkout source and
0.1.568/0.1.570/0.1.571 releases) exposes it as a **property returning the Yaml
facade**. Field-vs-property is a binary-breaking change: the C# source reads
identically, but the MemberRef the plugin JITs cannot bind (and it fails the
same way whether the host side is a property or a field of the wrong type).

Fix (both checked-in Core trees, `Util/Yaml.cs`): expose
`public static readonly ISerializer DefaultSerializer; public static readonly
IDeserializer DefaultDeserializer;` built with the original configuration.
In-tree consumers compile unchanged against the interface shapes.

Result with the shim: SetSinger proceeds, the plugin **writes and reloads its
`en_teto.yaml` configuration into the singer folder** (native; the embedded
resource carries the template), and `Process` completes without exceptions —
returning zero phonemes for a synthetic single note, i.e. it now needs real
note-group/syllable context, not import work. In browser WASM the same plugin
loads and binds, then hits `Process_* PlatformNotSupportedException` — it
spawns an external process during setup, so this plugin belongs on the
local-server fallback path exactly as the architecture prescribes.

Also required host-side for a generic `?dll=` import path: the WASM payload
strips `NullabilityInfoContext` (breaking `System.Text.Json` fixture-catalog
loading) — solved by baking the fixture catalog as C# source
(`FixtureCatalogData.cs`) instead of deserializing JSON at runtime.

## 4. Next actions enabled by these findings

1. Add a real Thai VCCV bank fixture and parity vectors for orthographic
   conversion, aliases, note-neighbour transitions, and part boundaries.
2. Decide whether English production coverage should add ONNX inference for
   `g2p-arpabet(.plus)` or import a separately licensed static lexicon; define
   model-size and unknown-word behavior before wiring it behind `with_g2p`.
3. Expose the probe's 8-step contract as the import checklist for the
   versioned plugin-import feature (per-plugin pinning, ALC isolation,
   metadata → language/style mapping).
