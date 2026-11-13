# T013 — OpenUtau official phonemizer trial

**Run date:** 2026-10-02  
**Runtime:** .NET 8.0 Browser WASM, OpenUtau Core source plus official Builtin source from this checkout

## Scope and method

The source inventory contains 26 attributed phonemizers in `OpenUtau.Core` and 47 in `OpenUtau.Plugin.Builtin` (the latter includes one DEBUG-only Presamp sample class). The Builtin DLL is compiled from the checked-in official C# files against the checked-in Core project, loaded from bytes in the browser, and every attributed class is constructed and tried.

Each case receives singer-change calls, a minimal one-note project, a language-appropriate smoke lyric for Japanese/Voicevox/Cantonese and `a[a]` for the rest, then `SetUp()` and `Process()`. The singer supplies synthetic aliases. No voicebank, DiffSinger/Enunu model package, Voicevox singer or engine is installed for this run; statuses therefore distinguish returned output from missing runtime/context requirements. A PASS is only a smoke success under this fixture; it does not establish real-bank pronunciation parity.

## Browser WASM result

- 73/73 official source classes were discovered and attempted.
- 43 returned phone output: 41 clean PASS and 2 PARTIAL (output returned despite `System.Diagnostics.Process` being unsupported during singer setup).
- 30 did not return output: 21 need real singer/model/engine context, 7 hit a Browser WASM API or host dependency limit, and 2 Vogen phonemizers could not load the native `onnxruntime` library.
- Headless Chromium completed the full matrix without a WASM crash; results rendered as 73 rows in the POC page.

## Native .NET 8 comparison

The same runner on native .NET 8 returned output for 49/73 classes. The 24 remaining cases require a matching voicebank/config/model/server result, including DiffSinger, Enunu and Voicevox families. Both Vogen classes worked natively and failed only in Browser WASM because the native ONNX runtime library is unavailable there. This comparison isolates a real browser-host constraint from ordinary missing singer data.

## Real `.ustx` project run through a DLL-loaded adapter

The native .NET 8 project runner now loads `OpenUtau.Plugin.Builtin.dll` from bytes, discovers and registers its 47 phonemizer types, then loads each `.ustx` and instantiates the phonemizer selected by each track. It completed all eight `.ustx` paths under the user-provided, gitignored `test-ustx-example/` directory. Two autosave files are byte-for-byte duplicates, leaving six unique project contents and 4,949 unique notes. Every note produced one phonemizer result; this tests phoneme generation, not audio rendering.

| Project | Phonemizer actually used | Notes → phones | Output aliases not found in supplied oto metadata |
|---|---|---:|---:|
| `envybaby.ustx` and its autosave | Japanese Presamp | 1,303 → 1,303 each | 95 each, including empty aliases and `- ラ` |
| `export-test.ustx` | Core Default | 4 → 4 | Not checked: project has no singer; the runner uses a missing-singer stub |
| `golden-the-end-of-hutsune-miku.ustx` | Japanese Presamp | 1,709 → 1,709 | 0 |
| `incomplete-intensevoice.ustx` | Japanese Presamp | 809 → 809 | 3 (`a り2`, `u い2`, `っ`) |
| `king.ustx` | Japanese Presamp | 1,124 → 1,124 | 0 |
| `test1.ustx` and its autosave | Japanese Presamp | 0 → 0 each | 0; empty project |

Despite its filename, `incomplete-intensevoice.ustx` selects the Japanese Presamp plugin, not an IntenseVoice phonemizer. The three unresolved entries there and 95 in `envybaby` were returned by `Process()` but did not match the supplied Teto `oto.ini` aliases; this is an alias/voicebank coverage issue, separate from DLL load or phonemizer invocation. The empty `export-test` has no singer to resolve against, so its four default outputs (`あ`, `い`, `う`, `え`) were only checked for generation. The Teto archive was extracted as metadata only; no WAV files were read and no render was attempted.

The actual-project adapter exercised two classes present in the sample projects: Japanese Presamp and Core Default. It does not mean every official class was tested on a real singer/project: the 73-class matrix above remains synthetic. This real-project run is native .NET 8; the Browser WASM page still runs the synthetic matrix and does not yet import whole `.ustx` projects or voicebanks.

The Rust Japanese VCV implementation was also compared to the OpenUtau Presamp outputs as a separate alias check:

| Project | OpenUtau phones | Rust phones | Positional alias matches | Reading |
|---|---:|---:|---:|---|
| `king.ustx` | 1,124 | 1,124 | 1,124 / 1,124 | Exact |
| `golden-the-end-of-hutsune-miku.ustx` | 1,709 | 1,709 | 1,709 / 1,709 | Exact |
| `incomplete-intensevoice.ustx` | 809 | 816 | 798 / 816 | Differences include alternate aliases (`り2`, `い2`) and small `っ` handling |
| `envybaby.ustx` | 1,303 | 1,486 | 167 / 1,486 | Not a fair parity score: many input lyrics are already alias-shaped (`- ラ`, `u レ`) or empty |

The last two rows are useful gap-finding signals, not evidence that DLL loading failed. The Rust comparator is Japanese VCV and does not yet normalize pre-phonemized aliases or align outputs when phone counts differ.

Generated per-project JSON includes input notes, selected plugin type, output aliases, and oto lookup results: [envybaby](../../../poc/openutau-browser-wasm/artifacts/envybaby-openutau-presamp.json), [export-test](../../../poc/openutau-browser-wasm/artifacts/export-test-openutau-default.json), [golden](../../../poc/openutau-browser-wasm/artifacts/golden-hutsune-openutau-presamp.json), [incomplete-intensevoice](../../../poc/openutau-browser-wasm/artifacts/incomplete-intensevoice-openutau-presamp.json), [king](../../../poc/openutau-browser-wasm/artifacts/king-openutau-presamp.json), [test1](../../../poc/openutau-browser-wasm/artifacts/test1-openutau-presamp.json). Autosave JSON files are also present alongside these reports.

## Browser-specific findings

- Six phonemizer setup paths hit `System.Diagnostics.Process is not supported on this platform.`: German Diphone, DiffSinger Rhythmizer, English Arpasing, Enunu English, French CMUSphinx, and Chinese CVV Monophone. English VCCV and Korean CV still emitted phones but their singer setup had the same warning.
- Enunu also needs a registered `shift_jis` code page; this Browser WASM host reports that encoding name as unsupported.
- Vogen Mandarin and Yue fail construction with `DllNotFoundException: onnxruntime`; this path needs a WebAssembly-capable inference provider or a service boundary.
- DiffSinger cases report missing `dsdur/dsconfig.yaml` or related model context; Enunu and Voicevox cases require their specific singer/server state. The browser runner deliberately uses a synthetic Classic singer and makes no configured service request.
- The official Builtin DLL is 1,234,944 bytes. It is copied as a lazy static asset; the .NET runtime/Core payload remains the large part of this POC.

## Per-class results

| Status | Phonemizer | Tag / type | Package | Output or blocking detail |
|---|---|---|---|---|
| WASM API LIMIT | German Diphone Phonemizer | DE DIPHONE · OpenUtau.Plugin.Builtin.GermanDiphonePhonemizer | OpenUtau.Plugin.Builtin | — SetSinger(unloaded): PlatformNotSupportedException: System.Diagnostics.Process is not supported on this platform.; SetSinger(null): PlatformNotSupportedException: System.Diagnostics.Process is not supported on this platform.; SetSinger(dummy singer): PlatformNotSupportedException: System.Diagnostics.Process is not supported on this platform.; NullReferenceException: Object reference not set to an instance of an object. |
| PASS | German VCCV Phonemizer | DE VCCV · OpenUtau.Plugin.Builtin.GermanVCCVPhonemizer | OpenUtau.Plugin.Builtin | - a · a- |
| PASS | Default Phonemizer | DEFAULT · OpenUtau.Core.DefaultPhonemizer | OpenUtau.Core | a |
| NEEDS CONTEXT | DiffSinger Phonemizer | DIFFS · OpenUtau.Core.DiffSinger.DiffSingerPhonemizer | OpenUtau.Core | — SetSinger(unloaded): Exception: Singer location is null.; SetSinger(null): Exception: Singer is null.; SetSinger(dummy singer): Exception: Failed to load dsdur/dsconfig.yaml; Exception: Phonemizer failed to process. |
| NEEDS CONTEXT | DiffSinger German Phonemizer | DIFFS DE · OpenUtau.Core.DiffSinger.DiffSingerGermanPhonemizer | OpenUtau.Core | — SetSinger(unloaded): Exception: Singer location is null.; SetSinger(null): Exception: Singer is null.; SetSinger(dummy singer): Exception: Failed to load dsdur/dsconfig.yaml; Exception: Phonemizer failed to process. |
| NEEDS CONTEXT | DiffSinger German Marzipan Phonemizer | DIFFS DE MARZ · OpenUtau.Core.DiffSinger.DiDiffSingerGermanMarzipanPhonemizerr | OpenUtau.Core | — SetSinger(unloaded): Exception: Singer location is null.; SetSinger(null): Exception: Singer is null.; SetSinger(dummy singer): Exception: Failed to load dsdur/dsconfig.yaml; Exception: Phonemizer failed to process. |
| NEEDS CONTEXT | DiffSinger English Phonemizer | DIFFS EN · OpenUtau.Core.DiffSinger.DiffSingerEnglishPhonemizer | OpenUtau.Core | — SetSinger(unloaded): Exception: Singer location is null.; SetSinger(null): Exception: Singer is null.; SetSinger(dummy singer): Exception: Failed to load dsdur/dsconfig.yaml; Exception: Phonemizer failed to process. |
| NEEDS CONTEXT | DiffSinger English+ Phonemizer | DIFFS EN+ · OpenUtau.Core.DiffSinger.DiffSingerARPAPlusEnglishPhonemizer | OpenUtau.Core | — SetSinger(unloaded): Exception: Singer location is null.; SetSinger(null): Exception: Singer is null.; SetSinger(dummy singer): Exception: Failed to load dsdur/dsconfig.yaml; Exception: Result not found in the part |
| NEEDS CONTEXT | DiffSinger Spanish Phonemizer | DIFFS ES · OpenUtau.Core.DiffSinger.DiffSingerSpanishPhonemizer | OpenUtau.Core | — SetSinger(unloaded): Exception: Singer location is null.; SetSinger(null): Exception: Singer is null.; SetSinger(dummy singer): Exception: Failed to load dsdur/dsconfig.yaml; Exception: Phonemizer failed to process. |
| NEEDS CONTEXT | DiffSinger Filipino Phonemizer | DIFFS FIL · OpenUtau.Core.DiffSinger.DiffSingerFilipinoPhonemizer | OpenUtau.Core | — SetSinger(unloaded): Exception: Singer location is null.; SetSinger(null): Exception: Singer is null.; SetSinger(dummy singer): Exception: Failed to load dsdur/dsconfig.yaml; Exception: Phonemizer failed to process. |
| NEEDS CONTEXT | DiffSinger Rule-based Filipino Phonemizer | DIFFS FIL · OpenUtau.Core.DiffSinger.DiffSingerRuleBasedFilipinoPhonemizer | OpenUtau.Core | — SetSinger(unloaded): Exception: Singer location is null.; SetSinger(null): Exception: Singer is null.; SetSinger(dummy singer): Exception: Failed to load dsdur/dsconfig.yaml; Exception: Phonemizer failed to process. |
| NEEDS CONTEXT | DiffSinger French Millefeuille Phonemizer | DIFFS FR MILLE · OpenUtau.Core.DiffSinger.DiffSingerFrenchMillfeuillePhonemizer | OpenUtau.Core | — SetSinger(unloaded): Exception: Singer location is null.; SetSinger(null): Exception: Singer is null.; SetSinger(dummy singer): Exception: Failed to load dsdur/dsconfig.yaml; Exception: Phonemizer failed to process. |
| NEEDS CONTEXT | DiffSinger Italian Phonemizer | DIFFS IT · OpenUtau.Core.DiffSinger.DiffSingerItalianPhonemizer | OpenUtau.Core | — SetSinger(unloaded): Exception: Singer location is null.; SetSinger(null): Exception: Singer is null.; SetSinger(dummy singer): Exception: Failed to load dsdur/dsconfig.yaml; Exception: Phonemizer failed to process. |
| NEEDS CONTEXT | DiffSinger Japanese Phonemizer | DIFFS JA · OpenUtau.Core.DiffSinger.DiffSingerJapanesePhonemizer | OpenUtau.Core | — SetSinger(unloaded): Exception: Singer location is null.; SetSinger(null): Exception: Singer is null.; SetSinger(dummy singer): Exception: Failed to load dsdur/dsconfig.yaml; Exception: Phonemizer failed to process. |
| NEEDS CONTEXT | DiffSinger Korean G2P Phonemizer | DIFFS KO · OpenUtau.Core.DiffSinger.DiffSingerKoreanG2PPhonemizer | OpenUtau.Core | — SetSinger(unloaded): Exception: Singer location is null.; SetSinger(null): Exception: Singer is null.; SetSinger(dummy singer): Exception: Failed to load dsdur/dsconfig.yaml; Exception: Phonemizer failed to process. |
| NEEDS CONTEXT | DiffSinger Korean Phonemizer | DIFFS KO · OpenUtau.Core.DiffSinger.DiffSingerKoreanPhonemizer | OpenUtau.Core | — SetSinger(unloaded): Exception: Singer location is null.; SetSinger(null): Exception: Singer is null.; SetSinger(dummy singer): Exception: Failed to load dsdur/dsconfig.yaml; SetUp(one note): NullReferenceException: Object reference not set to an instance of an object.; Exception: Phonemizer failed to process. |
| NEEDS CONTEXT | DiffSinger Portuguese Phonemizer | DIFFS PT · OpenUtau.Core.DiffSinger.DiffSingerPortuguesePhonemizer | OpenUtau.Core | — SetSinger(unloaded): Exception: Singer location is null.; SetSinger(null): Exception: Singer is null.; SetSinger(dummy singer): Exception: Failed to load dsdur/dsconfig.yaml; Exception: Phonemizer failed to process. |
| WASM API LIMIT | DiffSinger Rhythmizer Phonemizer | DIFFS RHY · OpenUtau.Core.DiffSinger.DiffSingerRhythmizerPhonemizer | OpenUtau.Core | — SetSinger(unloaded): ArgumentNullException: Value cannot be null. (Parameter 'path1'); SetSinger(dummy singer): PlatformNotSupportedException: System.Diagnostics.Process is not supported on this platform.; Exception: Phonemizer failed to process. |
| NEEDS CONTEXT | DiffSinger Russian Phonemizer | DIFFS RU · OpenUtau.Core.DiffSinger.DiffSingerRussianPhonemizer | OpenUtau.Core | — SetSinger(unloaded): Exception: Singer location is null.; SetSinger(null): Exception: Singer is null.; SetSinger(dummy singer): Exception: Failed to load dsdur/dsconfig.yaml; Exception: Phonemizer failed to process. |
| NEEDS CONTEXT | DiffSinger Chinese Phonemizer | DIFFS ZH · OpenUtau.Core.DiffSinger.DiffSingerChinesePhonemizer | OpenUtau.Core | — SetSinger(unloaded): Exception: Singer location is null.; SetSinger(null): Exception: Singer is null.; SetSinger(dummy singer): Exception: Failed to load dsdur/dsconfig.yaml; Exception: Phonemizer failed to process. |
| NEEDS CONTEXT | DiffSinger Jyutping Phonemizer | DIFFS ZH-YUE · OpenUtau.Core.DiffSinger.DiffSingerJyutpingPhonemizer | OpenUtau.Core | — SetSinger(unloaded): Exception: Singer location is null.; SetSinger(null): Exception: Singer is null.; SetSinger(dummy singer): Exception: Failed to load dsdur/dsconfig.yaml; Exception: Phonemizer failed to process. |
| WASM API LIMIT | English Arpasing Phonemizer | EN ARPA · OpenUtau.Plugin.Builtin.ArpasingPhonemizer | OpenUtau.Plugin.Builtin | — SetSinger(unloaded): PlatformNotSupportedException: System.Diagnostics.Process is not supported on this platform.; SetSinger(null): PlatformNotSupportedException: System.Diagnostics.Process is not supported on this platform.; SetSinger(dummy singer): PlatformNotSupportedException: System.Diagnostics.Process is not supported on this platform.; NullReferenceException: Object reference not set to an instance of an object. |
| PASS | Arpasing+ Phonemizer | EN ARPA+ · OpenUtau.Plugin.Builtin.ArpasingPlusPhonemizer | OpenUtau.Plugin.Builtin | -a · a - |
| PASS | English C+V Phonemizer | EN C+V · OpenUtau.Plugin.Builtin.EnglishCpVPhonemizer | OpenUtau.Plugin.Builtin | -a · a - |
| PARTIAL | English VCCV Phonemizer | EN VCCV · OpenUtau.Plugin.Builtin.EnglishVCCVPhonemizer | OpenUtau.Plugin.Builtin | -a · a- SetSinger(unloaded): PlatformNotSupportedException: System.Diagnostics.Process is not supported on this platform.; SetSinger(dummy singer): PlatformNotSupportedException: System.Diagnostics.Process is not supported on this platform. |
| PASS | English X-SAMPA phonemizer | EN X-SAMPA · OpenUtau.Plugin.Builtin.EnXSampaPhonemizer | OpenUtau.Plugin.Builtin | - a · a - |
| PASS | English to Japanese Phonemizer | EN to JA · OpenUtau.Plugin.Builtin.ENtoJAPhonemizer | OpenUtau.Plugin.Builtin | - あ · a R |
| WASM API LIMIT | Enunu Phonemizer | ENUNU · OpenUtau.Core.Enunu.EnunuPhonemizer | OpenUtau.Core | — SetSinger(unloaded): ArgumentException: 'shift_jis' is not a supported encoding name. For information on defining a custom encoding, see the documentation for the Encoding.RegisterProvider method. (Parameter 'name'); SetSinger(null): ArgumentException: 'shift_jis' is not a supported encoding name. For information on defining a custom encoding, see the documentation for the Encoding.RegisterProvider method. (Parameter 'name'); SetSinger(dummy singer): ArgumentException: 'shift_jis' is not a supported encoding name. For information on defining a custom encoding, see the documentation for the Encoding.RegisterProvider method. (Parameter 'name'); Exception: Part result not found |
| WASM API LIMIT | Enunu English Phonemizer | ENUNU EN · OpenUtau.Core.Enunu.EnunuEnglishPhonemizer | OpenUtau.Core | — SetSinger(unloaded): PlatformNotSupportedException: System.Diagnostics.Process is not supported on this platform.; SetSinger(null): PlatformNotSupportedException: System.Diagnostics.Process is not supported on this platform.; SetSinger(dummy singer): PlatformNotSupportedException: System.Diagnostics.Process is not supported on this platform.; Exception: Part result not found |
| NEEDS CONTEXT | Enunu Korean Phonemizer | ENUNU KO · OpenUtau.Core.Enunu.EnunuKoreanPhonemizer | OpenUtau.Core | — SetSinger(null): NullReferenceException: Object reference not set to an instance of an object.; Exception: Part result not found |
| NEEDS CONTEXT | Enunu Onnx Phonemizer | ENUNU X · OpenUtau.Plugin.Builtin.EnunuOnnxPhonemizer | OpenUtau.Plugin.Builtin | — SetUp(one note): NullReferenceException: Object reference not set to an instance of an object.; Exception: Part result not found |
| NEEDS CONTEXT | Enunu Onnx English Phonemizer | ENUNU X EN · OpenUtau.Plugin.Builtin.EnunuOnnxEnglishPhonemizer | OpenUtau.Plugin.Builtin | — SetUp(one note): NullReferenceException: Object reference not set to an instance of an object.; Exception: Part result not found |
| PASS | Spanish Makkusan Phonemizer | ES MAKKU · OpenUtau.Plugin.Builtin.SpanishMakkusanPhonemizer | OpenUtau.Plugin.Builtin | -a · a R |
| PASS | Spanish Syllable-Based Phonemizer | ES SYL · OpenUtau.Plugin.Builtin.SpanishSyllableBasedPhonemizer | OpenUtau.Plugin.Builtin | - a · a - |
| PASS | Spanish VCCV Phonemizer | ES VCCV · OpenUtau.Plugin.Builtin.SpanishVCCVPhonemizer | OpenUtau.Plugin.Builtin | - a · a- |
| PASS | Spanish to Japanese Phonemizer | ES to JA · OpenUtau.Plugin.Builtin.EStoJAPhonemizer | OpenUtau.Plugin.Builtin | - あ · a R |
| PASS | Filipino Phonemizer | FIL VCV & CVVC · OpenUtau.Plugin.Builtin.FilipinoPhonemizer | OpenUtau.Plugin.Builtin | -a · a - |
| PASS | Filipino to Japanese Phonemizer | FIL to JA · OpenUtau.Plugin.Builtin.FILtoJAPhonemizer | OpenUtau.Plugin.Builtin | - あ · a R |
| PASS | French CVVC Phonemizer | FR CVVC · OpenUtau.Plugin.Builtin.FrenchCVVCPhonemizer | OpenUtau.Plugin.Builtin | -a · a- |
| WASM API LIMIT | French CMUSphinx Phonemizer | FR SPHINX · OpenUtau.Plugin.Builtin.FrenchCMUSphinxPhonemizer | OpenUtau.Plugin.Builtin | — SetSinger(unloaded): PlatformNotSupportedException: System.Diagnostics.Process is not supported on this platform.; SetSinger(null): PlatformNotSupportedException: System.Diagnostics.Process is not supported on this platform.; SetSinger(dummy singer): PlatformNotSupportedException: System.Diagnostics.Process is not supported on this platform.; NullReferenceException: Object reference not set to an instance of an object. |
| PASS | French VCCV m2RUg Phonemizer | FR VCCV · OpenUtau.Plugin.Builtin.FrenchVCCVPhonemizer | OpenUtau.Plugin.Builtin | - a · a - |
| PASS | Italian CVVC Phonemizer | IT CVVC · OpenUtau.Plugin.Builtin.ItalianCVVCPhonemizer | OpenUtau.Plugin.Builtin | -a |
| PASS | Italian Syllable-Based Phonemizer | IT SYL · OpenUtau.Plugin.Builtin.ItalianSyllableBasedPhonemizer | OpenUtau.Plugin.Builtin | -a · a R |
| PASS | Japanese CVVC Phonemizer (legacy) | JA CVVC · OpenUtau.Plugin.Builtin.JapaneseCVVCPhonemizer | OpenUtau.Plugin.Builtin | - な |
| PASS | Japanese VCV Phonemizer (legacy) | JA VCV · OpenUtau.Plugin.Builtin.JapaneseVCVPhonemizer | OpenUtau.Plugin.Builtin | - な |
| PASS | Japanese presamp Phonemizer | JA VCV & CVVC · OpenUtau.Plugin.Builtin.JapanesePresampPhonemizer | OpenUtau.Plugin.Builtin | - な |
| PASS | Korean CBNN Phonemizer | KO CBNN · OpenUtau.Plugin.Builtin.KoreanCBNNPhonemizer | OpenUtau.Plugin.Builtin | a |
| PARTIAL | Korean CV Phonemizer | KO CV · OpenUtau.Plugin.Builtin.KoreanCVPhonemizer | OpenUtau.Plugin.Builtin | a SetSinger(dummy singer): PlatformNotSupportedException: System.Diagnostics.Process is not supported on this platform. |
| PASS | KoreanCVCPhonemizer | KO CVC · OpenUtau.Plugin.Builtin.KoreanCVCPhonemizer | OpenUtau.Plugin.Builtin | a |
| PASS | Korean CVCCV Phonemizer | KO CVCCV · OpenUtau.Plugin.Builtin.KoreanCVCCVPhonemizer | OpenUtau.Plugin.Builtin | a |
| PASS | Korean CVVC Phonemizer | KO CVVC · OpenUtau.Plugin.Builtin.KoreanCVVCPhonemizer | OpenUtau.Plugin.Builtin | a |
| PASS | Korean VCV Phonemizer | KO VCV · OpenUtau.Plugin.Builtin.KoreanVCVPhonemizer | OpenUtau.Plugin.Builtin | - a |
| PASS | KO to JA Phonemizer | KO to JA · OpenUtau.Plugin.Builtin.KOtoJAPhonemizer | OpenUtau.Plugin.Builtin | - あ |
| PASS | Polish CVC Phonemizer | PL CVC · OpenUtau.Plugin.Builtin.PolishCVCPhonemizer | OpenUtau.Plugin.Builtin | - a · a - |
| PASS | Brazilian Portuguese CVC Phonemizer | PT-BR CVC · OpenUtau.Plugin.Builtin.BrazilianPortugueseCVCPhonemizer | OpenUtau.Plugin.Builtin | - a · a - |
| PASS | Russian CVC Phonemizer | RU CVC · OpenUtau.Plugin.Builtin.RussianCVCPhonemizer | OpenUtau.Plugin.Builtin | -a · a- |
| PASS | Russian VCCV Phonemizer | RU VCCV · OpenUtau.Plugin.Builtin.RussianVCCVPhonemizer | OpenUtau.Plugin.Builtin | - a · a - |
| PASS | Simple Voicevox ENtoJA Phonemizer | S-VOICEVOX EN to JA · OpenUtau.Core.Voicevox.SimpleVoicevoxENtoJAPhonemizer | OpenUtau.Plugin.Builtin | あ |
| PASS | Simple Voicevox Japanese Phonemizer | S-VOICEVOX JA · Voicevox.SimpleVoicevoxPhonemizer | OpenUtau.Core | あ |
| PASS | Thai VCCV Phonemizer | TH VCCV · OpenUtau.Plugin.Builtin.ThaiVCCVPhonemizer | OpenUtau.Plugin.Builtin | -a · a- |
| PASS | Turkish CVVC Phonemizer | TR CVVC · OpenUtau.Plugin.Builtin.TurkishCVVCPhonemizer | OpenUtau.Plugin.Builtin | - a · a - |
| PASS | Vietnamese CVVC Phonemizer | VIE CVVC · OpenUtau.Plugin.Builtin.VietnameseCVVCPhonemizer | OpenUtau.Plugin.Builtin | a |
| PASS | Vietnamese VCV Phonemizer | VIE VCV · OpenUtau.Plugin.Builtin.VietnameseVCVPhonemizer | OpenUtau.Plugin.Builtin | a |
| PASS | Vietnamese VINA Phonemizer | VIE VINA · OpenUtau.Plugin.Builtin.VietnameseVINAPhonemizer | OpenUtau.Plugin.Builtin | a |
| NATIVE ML RUNTIME | Vogen Chinese Mandarin Phonemizer | VOGEN ZH · OpenUtau.Core.Vogen.VogenMandarinPhonemizer | OpenUtau.Core | — DllNotFoundException: onnxruntime |
| NATIVE ML RUNTIME | Vogen Chinese Yue Phonemizer | VOGEN ZH-YUE · OpenUtau.Core.Vogen.VogenYuePhonemizer | OpenUtau.Core | — DllNotFoundException: onnxruntime |
| NEEDS CONTEXT | Voicevox Japanese Phonemizer | VOICEVOX JA · OpenUtau.Core.Voicevox.VoicevoxPhonemizer | OpenUtau.Core | — SetUp(one note): NullReferenceException: Object reference not set to an instance of an object.; Exception: Part result not found |
| WASM API LIMIT | Chinese CVV (十月式整音扩张) Phonemizer | ZH CVV · OpenUtau.Plugin.Builtin.ChineseCVVMonophonePhonemizer | OpenUtau.Plugin.Builtin | — SetSinger(unloaded): PlatformNotSupportedException: System.Diagnostics.Process is not supported on this platform.; SetSinger(null): PlatformNotSupportedException: System.Diagnostics.Process is not supported on this platform.; SetSinger(dummy singer): PlatformNotSupportedException: System.Diagnostics.Process is not supported on this platform.; NullReferenceException: Object reference not set to an instance of an object. |
| PASS | Chinese CVV Plus Phonemizer | ZH CVV+ · OpenUtau.Plugin.Builtin.ChineseCVVPlusPhonemizer | OpenUtau.Plugin.Builtin | a |
| PASS | Chinese CVVC Phonemizer | ZH CVVC · OpenUtau.Plugin.Builtin.ChineseCVVCPhonemizer | OpenUtau.Plugin.Builtin | - a · a R |
| PASS | Presamp Sample Phonemizer | ZH CVVC · OpenUtau.Plugin.Builtin.PresampSamplePhonemizer | OpenUtau.Plugin.Builtin · DEBUG only | - a |
| PASS | Cantonese CVVC Phonemizer | ZH-YUE CVVC · OpenUtau.Plugin.Builtin.CantoneseCVVCPhonemizer | OpenUtau.Plugin.Builtin | - a · a R |
| PASS | Cantonese Syo-Style Phonemizer | ZH-YUE SYO · OpenUtau.Plugin.Builtin.CantoneseSyoPhonemizer | OpenUtau.Plugin.Builtin | - nei5 · ei5 - |

## Real project sample: Japanese Classic voicebank

The local, ignored [`test-ustx-example/king.ustx`](../../../test-ustx-example/king.ustx)
is a useful real-project input: five parts (`arrange`, `main`, `cho`, `arrange-cho`,
`shita`) contain 1,124 notes total. Each track selects
`OpenUtau.Plugin.Builtin.JapanesePresampPhonemizer`; the serialized track renderer is
`WORLDLINE-R`. The project sample and both voicebank ZIPs stay local under the
`test-ustx-example/` ignore rule.

The test ran the checked-in OpenUtau Core/Builtin source on .NET 8 with the actual Teto
voicebank metadata. It emitted 1,124 phonemes and every emitted alias resolved through the
bank's `oto.ini` metadata (0 unresolved). The Rust `JapaneseVcvPhonemizer` emitted the same
1,124 alias sequence: **1,124/1,124 exact alias matches** across all five parts. This is an
alias sequence comparison between Presamp and the current Japanese VCV implementation; it
does not compare phoneme timing or rendered audio. The implementations are in the
[.NET sample runner](../../../poc/openutau-browser-wasm/UserProjectCompare/Program.cs) and
[Rust alias comparator](../../../native/crates/phonemizer/examples/t013_king_openutau_compare.rs).

The comparison exposed a missing tail-vowel case for combined small-y kana such as `じゃ`.
Rust reset the VCV context after that mora, while OpenUtau continued with `a`. The Rust
`tail_vowel` handling now maps `きゃ/きゅ/きょ`-style morae to the vowel of their small-y
suffix; the focused regression test and the full sample comparison both pass.

Only the 16 voicebank files needed for phonemizer setup were extracted from the 497,536,210
byte ZIP: `character.txt`, `character.yaml`, and nested `oto.ini` files (284,976 bytes total).
No WAVs were extracted or read. The archive has no `presamp.ini`, so OpenUtau's Presamp class
uses its built-in defaults. This POC also does not instantiate WORLDLINE-R or render audio;
it verifies note-to-alias behavior and bank lookup only.

Reproduce from the repository root (with .NET 8 installed):

```sh
python3 - <<'PY'
from pathlib import Path
from zipfile import ZipFile

archive = Path("test-ustx-example/voicebank/TETO-OUset240323.zip")
destination = Path("/tmp/t013-teto-bank")
keep = {"character.txt", "character.yaml", "oto.ini", "presamp.ini", "prefix.map"}
with ZipFile(archive) as source:
    for item in source.infolist():
        relative = Path(item.filename)
        if item.is_dir() or relative.name.lower() not in keep:
            continue
        output = destination / relative
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_bytes(source.read(item))
PY

dotnet run --project poc/openutau-browser-wasm/UserProjectCompare/T013.OpenUtauUserProjectCompare.csproj \
  -c Release -- test-ustx-example/king.ustx \
  "/tmp/t013-teto-bank/重音テト OU用日本語統合ライブラリー" \
  poc/openutau-browser-wasm/artifacts/king-openutau-presamp.json
cargo run --manifest-path native/Cargo.toml -p phonemizer \
  --example t013_king_openutau_compare
```

The project runner builds the official Builtin project as a build dependency, copies the DLL
beside the executable, then calls `Assembly.Load(byte[])` at runtime. Change the project argument
and JSON output path to repeat it for another sample. The Browser WASM host has not yet been
extended to import these complete projects and voicebanks.
