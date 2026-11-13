# OpenUtau phonemizer in browser WASM — T013 POC

This POC checks whether a managed OpenUtau phonemizer can be selected as a `.dll`, loaded
from bytes in a browser, discovered through the real `OpenUtau.Api.Phonemizer` type, and
invoked by .NET Browser WASM. Rust remains the product phonemizer path; this is an optional
runtime experiment.

## Run it

Requirements: .NET 8 SDK, its `wasm-tools` workload, and a current browser with WebAssembly
support.

```sh
dotnet workload install wasm-tools
dotnet run --project poc/openutau-browser-wasm/Host/OpenUtauBrowserWasmHost.csproj \
  --configuration Release --no-launch-profile --urls http://127.0.0.1:5199
```

Open `http://127.0.0.1:5199/`, click **Load bundled probe and run**, or use **Choose a DLL…**
then **Load selected DLL** to load a local plugin (up to 20 MiB). The sample is compiled against the checked-in
`ref(openutau+openutau mobile)/core/OpenUtau.Core` project. The page also accepts
`?autoprobe=1` to load and invoke the sample on startup for a smoke check.

To run a real OpenUtau implementation, click **Load OpenUtau Japanese VCV and run** or open
`http://127.0.0.1:5199/?autojapanesevcv=1`. This builds the checked-in
`desktop-ref/OpenUtau.Plugin.Builtin/JapaneseVCVPhonemizer.cs` source unchanged as a separate
`.NET 8` plugin DLL. It calls the implementation twice: with no previous neighbor and with a
`か` neighbor. An in-memory `USinger` provides the `- な` and `a な` aliases, so the expected
outputs are `- な` and `a な`. This exercises dynamic DLL discovery/construction, `SetSinger`,
neighbor handling, alias lookup, and `Process()` without requiring a Japanese bank.

To try the complete official source inventory, click **Try every official phonemizer (73)** or
open `http://127.0.0.1:5199/?autoofficial=1`. This compiles the checked-in Builtin sources into a
separate `.NET 8` DLL, then discovers every `[Phonemizer]` class in that DLL and the already-loaded
`OpenUtau.Core` assembly. The inventory is 26 Core classes and 47 Builtin source classes, including
one DEBUG-only Presamp sample. The matrix tries singer changes, one-note project setup, and
`Process()` with synthetic aliases; it does not load a real voicebank or model package.

The source plugin lives in `Plugin/T013ProbePhonemizer.cs`. Its `Process()` returns
`wasm:<lyric>` so execution is easy to distinguish from a Rust or HTTP fallback. The DLL is
copied to `Host/wwwroot/plugins/` by the build targets and is deliberately loaded on demand,
not listed as a startup assembly.

## Result in this checkout

Verified with .NET SDK 8.0.425 / browser runtime pack 8.0.31 and Chromium headless:

- A fresh browser profile reached the ready UI in **830 ms** on loopback.
- The production-published host fetched **38,254,882 encoded framework bytes** (36.5 MiB) across
  155 resources; decoded bodies totalled **56,833,886 bytes** (54.2 MiB). Browser transfer size
  was 38,301,382 bytes including response headers. These readings use browser resource timing,
  so they describe this build and compression setup, not every host.
- A separate plain Blazor/.NET Browser WASM baseline on the same SDK measured **3.1 MB
- compressed** across 32 framework assemblies. This OpenUtau host therefore adds about **35.2 MB
  compressed** beyond the plain .NET browser app, mostly from OpenUtau Core and its dependencies.
- `OpenUtau.Core.wasm` accounts for **28,056,708 encoded bytes**; `dotnet.native.wasm` is another
  1,116,741 bytes. This reference Core project
  brings its full phonemizer/API assembly and dependency graph into the browser bundle; the
  measurement is therefore much larger than the standalone .NET runtime alone.
- The sample DLL is **5,632 bytes**. Its local-file-picker import, OpenUtau base-type discovery,
  construction, and `Process()` call all succeeded in the published browser build. A cold bundled
  sample returned `wasm:ka`; plugin load was 3.3 ms and `Process()` was 0.3 ms in that run. Loading
  the same DLL through the local file picker returned `wasm:ka` as well.
- The checked-in Japanese VCV implementation also loaded and ran as a separate **9,216-byte
  .NET 8 plugin DLL** in Chromium Browser WASM. A local development-host run returned `- な` for
  the first note and `a な` after `か`; plugin load was 5.1 ms and the final `Process()` call was
  0.9 ms in that single run. These are smoke-check timings, not a benchmark.
- All **73** official source classes loaded and were tried in the Browser WASM matrix. **41** ran
  cleanly and **2** returned phones while singer setup logged an unsupported desktop API. **30**
  did not return phones: 21 needed real singer/model/engine context, 7 hit a Browser WASM API or
  host dependency limit (including `System.Diagnostics.Process` and unregistered `shift_jis`),
  and 2 Vogen classes could not load the native `onnxruntime` library. The same .NET 8 matrix on
  the native runtime returned output for **49/73**. Full cases
  and outputs are recorded in
  [`T013-openutau-official-phonemizer-matrix.md`](../../docs/work/tasks/T013-openutau-official-phonemizer-matrix.md).
- After plugin load, `GC.GetTotalMemory(false)` reported about **4.4 MB** of live managed heap.
  `Module.HEAPU8.buffer.byteLength` reported a **92.8 MiB WebAssembly memory buffer capacity**;
  that is the allocator's current buffer size, not a measure of resident or live memory.

The matrix shows the boundary between pure managed rule-based phonemizers and classes that use
desktop process APIs, ONNX native libraries, local engines, or voicebank-specific models. The
Japanese VCV demo uses an in-memory singer and alias table, not a real voicebank. A matrix PASS
means the class returned output with the supplied smoke-test context; it does not establish
pronunciation parity for a real bank. No user-targeted third-party DLL was present, so its
compatibility is still unverified. `dotnet publish` also emits two non-fatal `IL2105` warnings from NumSharp's
debugger-proxy type lookup. Blazor's standard `InputFile` change handler failed in the trimmed
build with `NullabilityInfoContext_NotSupported`; this POC uses browser `File.arrayBuffer()`
interop to import a DLL.

## Reading the result

The direct browser route is technically viable for a curated pure-managed phonemizer. It is not
small enough to enable eagerly in the current form: the measured bundle includes all of the
checked-in OpenUtau Core dependency graph. The next size experiment should isolate a pinned,
minimal OpenUtau API compatibility assembly and load the managed runtime/plugin only after the
user chooses that mode. Plugins that need native desktop APIs still need a local-server host.

To regenerate the release payload measurement:

```sh
dotnet publish poc/openutau-browser-wasm/Host/OpenUtauBrowserWasmHost.csproj \
  --configuration Release --output poc/openutau-browser-wasm/artifacts/publish
python3 poc/openutau-browser-wasm/serve_published.py --port 5201
```

Then open `http://127.0.0.1:5201/?autoprobe=1`. The published directory contains both raw and
precompressed assets; this server negotiates gzip and serves the same compressed resources the
browser measured. Keep encoded transfer bytes separate from the raw+gzip files on disk.
