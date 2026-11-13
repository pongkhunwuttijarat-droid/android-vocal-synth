# OpenUtau UI detail pass — dialogs and auxiliary windows

**Status:** desktop static-source details and source-level Web crosswalk are complete for the 29 auxiliary surfaces; live visual/runtime behavior remains unverified.  
**Updated:** 2026-10-08T05:05:57Z  
**Parent inventory:** [`openutau-ui-screen-inventory.md`](openutau-ui-screen-inventory.md).  
**Reference:** `ref-openutau-clean/desktop-ref/OpenUtau`.

This pass covers 29 auxiliary `Views/*.axaml` surfaces, excluding the primary `MainWindow` and `PianoRollDetachedWindow`. It was derived from the Avalonia XAML and matching code-behind/view-model source. **Fact** denotes directly shown/wired behavior; **unknown** calls out runtime-dependent details or items that source markup cannot establish. This is not a screenshot/interaction test. The Web crosswalk below classifies source-level counterpart only: **partial** means a related surface exists but gaps remain; **missing** means no equivalent workflow was found; **clone-only** marks a lookalike/default rather than parity. No row claims live behavior.

## Singer and voicebank workflows

### Singers — browser and oto editor

- **Sources:** [`SingersDialog.axaml`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/SingersDialog.axaml#L1-L260), [`SingersViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/SingersViewModel.cs#L23-L100), [`SingersViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/SingersViewModel.cs#L154-L240), [`SingersViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/SingersViewModel.cs#L299-L469), [`SingersDialog.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/SingersDialog.axaml.cs#L20-L82), [`SingersDialog.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/SingersDialog.axaml.cs#L110-L186), [`SingersDialog.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/SingersDialog.axaml.cs#L453-L469).
- **Facts:** singer browser/selection and metadata; read-only subbank view; singer settings/actions; oto timing editor with waveform/playback and preview. Related actions open publish, merge, and subbank dialogs. Selection/loading, YAML settings, refresh/search, oto validation and save behavior are implemented in the view model. Closing unsubscribes and stops playback.
- **Entry:** Tools → Singers in [`MainWindow.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/MainWindow.axaml.cs#L521-L523); install routes in [`MainWindow.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/MainWindow.axaml.cs#L580-L619).
- **Unknown:** available rows, metadata, subbanks, oto entries and waveform depend on installed voicebanks; visible wording is localized.

### Singer Setup — import/install

- **Sources:** [`SingerSetupDialog.axaml`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/SingerSetupDialog.axaml#L17-L102), [`SingerSetupViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/SingerSetupViewModel.cs#L17-L69), [`SingerSetupViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/SingerSetupViewModel.cs#L79-L178), [`SingerSetupDialog.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/SingerSetupDialog.axaml.cs#L14-L26).
- **Facts:** three-step encoding/preview/missing-metadata workflow; defaults to UTAU and Shift-JIS; text/archive preview is capped at 256 nonblank lines; async install; failures are reported and the dialog closes.
- **Entry:** singer installation flows in [`MainWindow.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/MainWindow.axaml.cs#L580-L619); another setup path is at line 950.
- **Unknown:** actual archive contents, parsed metadata and final installed singer depend on the selected input.

### Singer Publish

- **Sources:** [`SingerPublishDialog.axaml`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/SingerPublishDialog.axaml#L12-L25), [`SingerPublishViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/SingerPublishViewModel.cs#L10-L44), [`SingerPublishDialog.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/SingerPublishDialog.axaml.cs#L16-L55).
- **Facts:** ignore rules, output path/extension and async publish/copy. Directory-backed singers choose ZIP; file-backed singers retain the original extension. Errors are shown before close.
- **Entry:** singer browser action in [`SingersDialog.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/SingersDialog.axaml.cs#L110-L123).

### Edit Subbanks

- **Sources:** [`EditSubbanksDialog.axaml`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/EditSubbanksDialog.axaml#L12-L51), [`EditSubbanksDialog.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/EditSubbanksDialog.axaml.cs#L20-L123), [`EditSubbanksViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/EditSubbanksViewModel.cs#L14-L61), [`EditSubbanksViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/EditSubbanksViewModel.cs#L63-L115), [`EditSubbanksViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/EditSubbanksViewModel.cs#L117-L220).
- **Facts:** editor covers 84 pitches C1–B7; add/rename uses Type In; TSV import/export; Save writes `character.yaml`, refreshes/remaps and sends notification; Cancel closes without this Save path.
- **Entry:** singer browser in [`SingersDialog.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/SingersDialog.axaml.cs#L144).

### Merge Voicebank

- **Sources:** [`MergeVoicebankDialog.axaml`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/MergeVoicebankDialog.axaml#L13-L110), [`MergeVoicebankDialog.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/MergeVoicebankDialog.axaml.cs#L14-L59), [`MergeVoicebankViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/MergeVoicebankViewModel.cs#L32-L117), [`MergeVoicebankViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/MergeVoicebankViewModel.cs#L119-L160), [`MergeVoicebankViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/MergeVoicebankViewModel.cs#L172-L350).
- **Facts:** four-step carousel: choose a second bank, rename folders, edit subbanks, map voice colors. Collision names receive suffixes (`_1`, `_2`, …); duplicates/existing names are validated with bound error text. Merge asynchronously copies supported audio/converted oto data, updates `character.yaml`, and emits singer-changed notification. Grid edit events invoke validation; Merge starts async work, logs/shows a fault, then closes.
- **Entry:** singer browser action in [`SingersDialog.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/SingersDialog.axaml.cs#L129-L141).
- **Unknown:** final merged bank depends on source files and validation result.

### Voice Color Mapping

- **Sources:** [`VoiceColorMappingDialog.axaml`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/VoiceColorMappingDialog.axaml#L10-L35), [`VoiceColorMappingDialog.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/VoiceColorMappingDialog.axaml.cs#L7-L38), [`VoiceColorMappingViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/VoiceColorMappingViewModel.cs#L7-L28), caller [`MainWindow.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/MainWindow.axaml.cs#L1766-L1828).
- **Facts:** map old colors to new-color choices; identical names are retained, unmatched mappings default; only OK invokes Apply. Project edits are command-backed.
- **Unknown:** selectable options depend on current project/singer mappings.

### Package Manager

- **Sources:** [`PackageManagerDialog.axaml`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/PackageManagerDialog.axaml#L13-L49), [`PackageManagerDialog.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/PackageManagerDialog.axaml.cs#L14-L77), [`PackageManagerViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/PackageManagerViewModel.cs#L12-L63), [`PackageManagerViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/PackageManagerViewModel.cs#L65-L155).
- **Facts:** package ID/developer/version/installed status list; install, update, uninstall, `.oudp` import, package location, refresh, status and error states.
- **Entry:** Tools menu route in [`MainWindow.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/MainWindow.axaml.cs#L613).

### External Tool Setup

- **Sources:** [`ExeSetupDialog.axaml`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/ExeSetupDialog.axaml#L9-L30), [`ExeSetupViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/ExeSetupViewModel.cs#L4-L15), [`ExeSetupDialog.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/ExeSetupDialog.axaml.cs#L7-L29).
- **Facts:** install instructions; macOS-specific command field; separate Install-as actions for resampler and wavtool; selected install then closes.
- **Entry:** preferences dependency/tool paths and MainWindow setup routes in [`MainWindow.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/MainWindow.axaml.cs#L621-L660).

## Preferences, theme, track and effect configuration

### Preferences

- **Sources:** [`PreferencesDialog.axaml`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/PreferencesDialog.axaml#L44-L359), [`PreferencesViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/PreferencesViewModel.cs#L28-L130), [`PreferencesViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/PreferencesViewModel.cs#L132-L194), [`PreferencesViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/PreferencesViewModel.cs#L199-L407), [`PreferencesViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/PreferencesViewModel.cs#L409-L461), [`PreferencesDialog.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/PreferencesDialog.axaml.cs#L20-L120), [`PreferencesDialog.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/PreferencesDialog.axaml.cs#L123-L166).
- **Facts:** nine sections: General, Playback, Paths, Editing, Rendering, Appearance, UTAU, DiffSinger, Advanced. Controls cover language/sorting/beta; output device/backend/test/lock-start/autoscroll; tool and folder paths; appearance/theme; engine-specific and advanced settings. Most view-model changes save immediately; audio/tool/theme handlers and Piano Roll notifications are wired. Code-behind selects paths/tools, reloads singer index and validates theme create/edit/delete (sanitized name, rejects empty, confirms delete).
- **Unknown:** localized resource strings obscure final wording; do not assume a dialog-level Cancel rolls back settings already persisted by individual controls.
- **Entry:** Tools → Preferences and related setup paths in [`MainWindow.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/MainWindow.axaml.cs#L621-L660).

### Theme Editor

- **Sources:** [`ThemeEditorWindow.axaml`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/ThemeEditorWindow.axaml#L1-L157), [`ThemeEditorViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/ThemeEditorViewModel.cs#L16-L112), [`ThemeEditorViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/ThemeEditorViewModel.cs#L113-L224), [`ThemeEditorViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/ThemeEditorViewModel.cs#L226-L272), [`ThemeEditorWindow.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/ThemeEditorWindow.axaml.cs#L10-L44).
- **Facts:** dark mode plus palette pickers for application background/foreground/borders/accents, timeline and piano keys. Palette changes update application resources live; Save writes custom-theme YAML; Cancel closes; closing reapplies the theme. Window is singleton.
- **Entry:** Preferences theme Create/Edit. **Boundary:** live visual preview and persisted theme are distinct; do not infer unimplemented rollback behavior from the XAML alone.

### Mix FX

- **Sources:** [`MixFxDialog.axaml`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/MixFxDialog.axaml#L1-L170), [`MixFxDialog.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/MixFxDialog.axaml.cs#L14-L48), [`MixFxViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/MixFxViewModel.cs#L20-L141), [`MixFxViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/MixFxViewModel.cs#L148-L260), [`MixFxViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/MixFxViewModel.cs#L276-L296), track-header caller [`TrackHeaderViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/TrackHeaderViewModel.cs#L612-L616).
- **Facts:** per-track enabled/recommended controls; user preset selector/save/delete; EQ (low/mid/high), compressor (threshold/ratio/makeup), reverb (size/damp/wet/pre-delay); apply-on-export-mixdown checkbox. Effect editor disables when FX disabled. Draft FX changes commit only on OK to track FX/preferences and issue notification; Cancel discards draft; user presets persist separately.
- **Entry:** track header FX action.

### Track Settings

- **Sources:** [`TrackSettingsDialog.axaml`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/TrackSettingsDialog.axaml#L10-L37), [`TrackSettingsViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/TrackSettingsViewModel.cs#L28-L103), [`TrackSettingsDialog.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/TrackSettingsDialog.axaml.cs), caller [`TrackHeader.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Controls/TrackHeader.axaml.cs#L176).
- **Facts:** non-Classic renderer warning vs Classic-specific resampler/wavtool selectors, locations/default actions and OK. View model filters wavtools by resampler; defaults persist immediately, Classic track settings apply under undo.
- **Unknown:** visible renderer branch depends on selected track state.

### Track Color

- **Sources:** [`TrackColorDialog.axaml`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/TrackColorDialog.axaml#L5-L37), [`TrackColorViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/TrackColorViewModel.cs#L8-L24), caller [`TrackHeaderViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/TrackHeaderViewModel.cs#L548-L558).
- **Facts:** two-row color swatches with name tooltips, Cancel/Apply; Apply uses undoable `ChangeTrackColorCommand`.

## Editing and note/part workflows

### Lyrics insertion

- **Sources:** [`LyricsDialog.axaml`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/LyricsDialog.axaml#L5-L72), [`LyricsDialog.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/LyricsDialog.axaml.cs#L8-L45), [`LyricsViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/LyricsViewModel.cs#L22-L98), caller [`PianoRoll.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Controls/PianoRoll.axaml.cs#L445-L448).
- **Facts:** multiline lyrics, count, separators, live-preview/apply-selection toggles, Reset/Cancel/Apply. Enter, Shift+Enter, Esc handlers. Preview rolls back then applies command; finish/cancel use an undo group; lyric options persist.
- **Unknown:** effect depends on selected notes and options.

### Lyrics Replace

- **Sources:** [`LyricsReplaceDialog.axaml`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/LyricsReplaceDialog.axaml#L5-L40), [`LyricsReplaceViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/LyricsReplaceViewModel.cs#L12-L79), caller [`PianoRoll.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Controls/PianoRoll.axaml.cs#L414-L421).
- **Facts:** regex old/new, presets, preview and Apply; invalid preview regex is caught; finish validates and groups `ChangeNoteLyricCommand`, closing only on successful apply.
- **Unknown:** markup has no explicit Cancel button; window-close behavior is not established by this XAML.

### Note Defaults

- **Sources:** [`NoteDefaultsDialog.axaml`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/NoteDefaultsDialog.axaml#L16-L149), [`NoteDefaultsViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/NoteDefaultsViewModel.cs#L11-L182), [`NoteDefaultsViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/NoteDefaultsViewModel.cs#L184-L238), [`NoteDefaultsDialog.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/NoteDefaultsDialog.axaml.cs#L7-L27), caller [`PianoRoll.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Controls/PianoRoll.axaml.cs#L451-L461).
- **Facts:** default lyric/split lyric, portamento/vibrato presets and numeric controls, auto-vibrato and reset. Values persist on change; vibrato is clamped; preset save/remove/reset actions exist. No explicit Save/Cancel is present in the markup.

### Paste Parameters

- **Sources:** [`PasteParamDialog.axaml`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/PasteParamDialog.axaml#L10-L23), [`PasteParamViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/PasteParamViewModel.cs#L6-L20), [`NotesViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/NotesViewModel.cs#L907-L955).
- **Facts:** multi-select for pitch points, vibrato and non-curve expressions; pitch is selected by default. Dialog is gated by clipboard/selection availability; selected clipboard values apply in an undo group.

### Time Signature

- **Sources:** [`TimeSignatureDialog.axaml`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/TimeSignatureDialog.axaml#L10-L21), [`TimeSignatureDialog.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/TimeSignatureDialog.axaml.cs#L9-L31), callers [`MainWindow.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/MainWindow.axaml.cs#L120-L145) and [`MainWindow.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/MainWindow.axaml.cs#L201-L212).
- **Facts:** numerator 1–32 and unit options 1, 2, 4, 8, 16, 32; dialog returns values through callback. The caller decides whether it updates playback display or applies a project command; the value collector alone does not establish project mutation semantics.

### Type In

- **Sources:** [`TypeInDialog.axaml`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/TypeInDialog.axaml#L8-L14), [`TypeInDialog.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/TypeInDialog.axaml.cs#L7-L46).
- **Facts:** generic prompt + text box + OK; focuses/selects input; OK/Enter returns the string, Esc closes; no intrinsic validation. Used by preferences, subbanks, note defaults and other callers.

### Slider

- **Sources:** [`SliderDialog.axaml`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/SliderDialog.axaml#L8-L19), [`SliderDialog.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/SliderDialog.axaml.cs#L14-L49), callers [`PianoRoll.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Controls/PianoRoll.axaml.cs#L503-L536).
- **Facts:** generic title/range/tick slider and numeric value; callback only on Finish/OK. Current callers randomize tuning (1–100) and lengthen crossfade (0–1, tick 0.1).

### Transcribe / Batch

- **Sources:** [`TranscribeDialog.axaml`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/TranscribeDialog.axaml#L30-L175), [`TranscribeDialog.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/TranscribeDialog.axaml.cs#L10-L70), [`TranscribeViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/TranscribeViewModel.cs#L14-L154), caller [`MainWindow.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/MainWindow.axaml.cs#L1488-L1505).
- **Facts:** SOME/GAME radio modes with unavailable tips; optional RMVPE checkbox; GAME language, sample steps `{1,2,4,8,16}`, boundary/score thresholds, radius, batch size and max-duration controls. OK enabled only when chosen algorithm is installed; Cancel/Esc; valid OK/Enter confirms. Defaults to GAME when available, else SOME; batch size 1 and max duration 60 seconds; builds language options and batching strategy. Entry is restricted to `UWavePart`; MainWindow blocks when both algorithms are absent and acts only after confirmation.
- **Unknown:** rendered/transcribed output is handled by the subsequent analysis path, not dialog markup.

### Expressions

- **Sources:** [`ExpressionsDialog.axaml`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/ExpressionsDialog.axaml#L20-L133), [`ExpressionsDialog.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/ExpressionsDialog.axaml.cs#L13-L31), [`ExpressionsViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/ExpressionsViewModel.cs#L15-L118), [`ExpressionsViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/ExpressionsViewModel.cs#L120-L241), entry [`MainWindow.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/MainWindow.axaml.cs#L512) and [`PianoRoll.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Controls/PianoRoll.axaml.cs#L557).
- **Facts:** project/track scope toggle; list/add/remove; name, abbreviation, type (numerical/options/curve) and type-specific values/flags/options. Project suggestions disabled on track override. Apply catches validation exceptions and closes only on success. Validation includes nonempty name/abbr, abbreviation length 1–4, min<max, defaults within bounds; duplicate abbreviations and nonempty flags rejected per scope. Apply issues `ConfigureExpressionsCommand` in an undo group. No explicit Cancel in markup.
- **State boundary:** descriptors flow through project/track commands; this legacy Avalonia reference is not permission to make a new frontend authoritative for musical/project state.

### Phonetic Assistant

- **Sources:** [`PhoneticAssistant.axaml`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/PhoneticAssistant.axaml#L8-L17), [`PhoneticAssistant.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/PhoneticAssistant.axaml.cs#L6-L15), [`PhoneticAssistantViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/PhoneticAssistantViewModel.cs#L11-L74), entry [`MainWindow.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/MainWindow.axaml.cs#L688-L705).
- **Facts:** G2P selector, grapheme input, read-only phoneme output and Copy. Supported classes include ARPAbet, French, German, Italian, Portuguese, Russian, Spanish, Korean and Filipino. Changing G2P immediately saves preference; input refreshes output. Opened as singleton.
- **Unknown:** no explicit validation/Save/Cancel controls in the surface.

### DiffSinger Script Export

- **Sources:** [`DsScriptExportDialog.axaml`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/DsScriptExportDialog.axaml#L20-L48), [`DsScriptExportDialog.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/DsScriptExportDialog.axaml.cs#L6-L33), [`DsScriptExportViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/DsScriptExportViewModel.cs#L6-L17), entry/export flow [`MainWindow.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/MainWindow.axaml.cs#L415-L452).
- **Facts:** pitch checkbox on by default; variance off/disabled without tensor cache and BuildOptions forces false; OK/Enter confirms, Cancel/Esc closes. Main flow checks render cache, then asks save path and exports voice parts.

## Startup and utility windows

### Splash

- **Sources:** [`SplashWindow.axaml`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/SplashWindow.axaml#L5-L27), [`SplashWindow.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/SplashWindow.axaml.cs#L15-L70). See the deeper startup walk in [`openutau-ui-screen-inventory.md`](openutau-ui-screen-inventory.md#L43-L73).
- **Facts:** startup branding/progress; theme-specific logo; initializes managers/audio then opens MainWindow/loading. Startup exceptions are logged and shown.

### Loading Window

- **Sources:** [`LoadingWindow.axaml`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/LoadingWindow.axaml#L6-L20), [`LoadingWindow.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/LoadingWindow.axaml.cs#L7-L133).
- **Facts:** centered icon/text and delayed-modal service with async wrappers, Begin/End state; no user action surface.

### Updater

- **Sources:** [`UpdaterDialog.axaml`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/UpdaterDialog.axaml#L6-L34), [`UpdaterDialog.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/UpdaterDialog.axaml.cs#L22-L50), [`UpdaterViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/UpdaterViewModel.cs#L35-L79).
- **Facts:** version/status/update controls; silent release check prompts only if an update is available and not skipped; failures logged.
- **Unknown:** network/release state is external.

### Debug Window

- **Sources:** [`DebugWindow.axaml`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/DebugWindow.axaml#L7-L38), [`DebugWindow.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/DebugWindow.axaml.cs#L9-L24), [`DebugViewModel.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/ViewModels/DebugViewModel.cs#L42-L143).
- **Facts:** log list, copy/reverse/clear; hidden log-level selector exists; attaches/detaches sink, formats/copies entries and toggles process debug/log level.

### MessageBox

- **Sources:** [`MessageBox.axaml`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/MessageBox.axaml#L6-L68), [`MessageBox.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/MessageBox.axaml.cs#L18-L85), [`MessageBox.axaml.cs`](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/MessageBox.axaml.cs#L238-L264).
- **Facts:** dynamic message, stack/URL links and caller-supplied result buttons; exception translation, link opening, button/result handling.
- **Unknown:** available buttons and semantics vary by caller; markup alone does not establish a specific prompt flow.

## Cross-cutting constraints and outstanding work

- Preference/theme/voicebank configuration writes are visible in legacy source (for example `Preferences.Save`, custom theme YAML and `character.yaml`). Musical/project edits usually route through `DocManager.ExecuteCmd` and undo groups. The React clone must still honor the current engine-owned state rule in [`engine-owned-state.md`](../architecture/engine-owned-state.md); this desktop source is behavioral evidence, not a mandate to port Avalonia ViewModels or to duplicate project state in the frontend.
- Localized `DynamicResource` strings do not reveal final wording; markup cannot determine installed data, runtime branch visibility or external updater/network outcomes. When no Cancel exists, do not infer rollback beyond the code path.
- **Still to do:** live screenshot/keyboard/focus checks and any runtime-dependent validation if the reference UI is available. The source-level 29-surface Web crosswalk follows; primary MainWindow/Piano Roll and dock workflows are tracked in the parent inventory. Keep source facts and runtime observations distinct.

## Web-clone crosswalk (source-only)

| Desktop auxiliary surface | Web status and nearest source | Source-level parity boundary / notable gap |
|---|---|---|
| [Singers browser + oto editor](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/SingersDialog.axaml#L1-L260) | **Partial:** [editor-menus.jsx](../../ui-mock/react-dock/src/openutau/editor-menus.jsx#L104-L170), [arrangement-panel.jsx](../../ui-mock/react-dock/src/openutau/arrangement-panel.jsx#L201-L265) | Browser lists/imports/removes OPFS bank roots and applies a bank separately; no metadata/subbank browser, oto timing grid, waveform/playback editor or search. Track assignment is real ([`engine.js`](../../ui-mock/react-dock/src/openutau/engine.js#L36-L40), [`v2.rs`](../../native/crates/project-session/src/v2.rs#L939-L956)); OPFS bank install is separate host storage ([`samples.js`](../../web/samples.js#L204-L212), [`samples.js`](../../web/samples.js#L309-L362)). |
| [Singer Setup](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/SingerSetupDialog.axaml#L17-L102) | **Partial:** [editor-menus.jsx](../../ui-mock/react-dock/src/openutau/editor-menus.jsx#L118-L124), [editor-menus.jsx](../../ui-mock/react-dock/src/openutau/editor-menus.jsx#L147-L209) | Folder/ZIP-to-OPFS install only; no encoding selection, 256-line preview or missing-metadata repair. |
| [Singer Publish](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/SingerPublishDialog.axaml#L12-L25) | **Missing:** [Tools menu](../../ui-mock/react-dock/src/openutau/editor-menus.jsx#L291-L309) | No voicebank publish/export; project WAV/USTX/bundle exports are not singer packaging ([`file-menu.jsx`](../../ui-mock/react-dock/src/openutau/file-menu.jsx#L582-L585)). |
| [Edit Subbanks](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/EditSubbanksDialog.axaml#L12-L51) | **Missing:** [Singers list](../../ui-mock/react-dock/src/openutau/editor-menus.jsx#L247-L264) | No metadata/subbank write editor; Rust voicebank code reads metadata only ([`voicebank.rs`](../../native/crates/voicebank/src/voicebank.rs#L1-L10), [`voicebank.rs`](../../native/crates/voicebank/src/voicebank.rs#L75-L80)). |
| [Merge Voicebank](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/MergeVoicebankDialog.axaml#L13-L110) | **Missing:** [Singers import](../../ui-mock/react-dock/src/openutau/editor-menus.jsx#L147-L209) | No merge wizard, collision/subbank/color mapping workflow or merge command. |
| [Voice Color Mapping](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/VoiceColorMappingDialog.axaml#L10-L35) | **Missing; clone-only lookalike:** [note-properties-panel.jsx](../../ui-mock/react-dock/src/openutau/note-properties-panel.jsx#L109-L124) | Bright/Dark is local presentation state and disabled when connected; disabled `voice color y` is not old→new bank-color mapping. |
| [Package Manager](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/PackageManagerDialog.axaml#L13-L49) | **Missing:** [Tools menu](../../ui-mock/react-dock/src/openutau/editor-menus.jsx#L291-L309) | No `.oudp` install/update/uninstall/location/status UI; project bundles are USTX+audio, not packages ([`project-bundle.js`](../../ui-mock/react-dock/src/openutau/project-bundle.js#L58-L63)). |
| [Preferences](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/PreferencesDialog.axaml#L44-L359) | **Partial:** [main.jsx](../../ui-mock/react-dock/src/openutau/main.jsx#L131-L162), [main.jsx](../../ui-mock/react-dock/src/openutau/main.jsx#L266-L315) | Theme/session selectors and browser audio-device/master-volume controls exist, but no Preferences dialog, nine-section settings UI or dependency paths; no Tools entry ([`editor-menus.jsx`](../../ui-mock/react-dock/src/openutau/editor-menus.jsx#L291-L309)). |
| [Theme Editor](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/ThemeEditorWindow.axaml#L1-L157) | **Partial:** [main.jsx](../../ui-mock/react-dock/src/openutau/main.jsx#L148-L153), [palette.js](../../ui-mock/react-dock/src/openutau/palette.js#L17-L24) | Bundled presets/tokens and selected-key localStorage only ([`palette.js`](../../ui-mock/react-dock/src/openutau/palette.js#L349-L366)); no custom palette editor, YAML save or Cancel/rollback. |
| [External Tool Setup](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/ExeSetupDialog.axaml#L9-L30) | **Missing:** [Tools menu](../../ui-mock/react-dock/src/openutau/editor-menus.jsx#L291-L309), disabled [track gear](../../ui-mock/react-dock/src/openutau/arrangement-panel.jsx#L299) | No dependency/tool path setup or Install-as resampler/wavtool workflow. |
| [Slider](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/SliderDialog.axaml#L8-L19) | **Missing as generic modal:** [Note Properties](../../ui-mock/react-dock/src/openutau/note-properties-panel.jsx#L65-L107), [Track FX](../../ui-mock/react-dock/src/openutau/track-fx-dialog.jsx#L60-L62) | Inline range/number fields are not the reusable title/range/tick callback dialog. |
| [Track Color](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/TrackColorDialog.axaml#L5-L37) | **Partial visualization only:** [arrangement-panel.jsx](../../ui-mock/react-dock/src/openutau/arrangement-panel.jsx#L235-L240), [palette.js](../../ui-mock/react-dock/src/openutau/palette.js#L317) | Accent is a deterministic palette by track index; no swatch chooser, Apply/Cancel or undoable color edit. |
| [Lyrics insertion](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/LyricsDialog.axaml#L5-L72) | **Partial:** [note-properties-panel.jsx](../../ui-mock/react-dock/src/openutau/note-properties-panel.jsx#L41-L55), [piano-roll-panel.jsx](../../ui-mock/react-dock/src/openutau/piano-roll-panel.jsx#L436-L441) | Per-note lyric editing is session-backed ([`engine.js`](../../ui-mock/react-dock/src/openutau/engine.js#L62); [`v2.rs`](../../native/crates/project-session/src/v2.rs#L1678-L1693)); no multiline/count/separator/live-preview/apply-selection dialog. Pen inserts fixed `'あ'`/480-tick notes. |
| [Lyrics Replace](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/LyricsReplaceDialog.axaml#L5-L40) | **Missing:** [Edit menu](../../ui-mock/react-dock/src/openutau/editor-menus.jsx#L55-L60), nearest [lyric field](../../ui-mock/react-dock/src/openutau/note-properties-panel.jsx#L41-L55) | No regex/presets/preview/replace workflow; backend maps only per-note `note.set_lyric` ([`v2.rs`](../../native/crates/project-session/src/v2.rs#L1678-L1693)). |
| [Note Defaults](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/NoteDefaultsDialog.axaml#L16-L149) | **Clone-only static defaults:** [piano-roll-panel.jsx](../../ui-mock/react-dock/src/openutau/piano-roll-panel.jsx#L436-L441), [clone-state.jsx](../../ui-mock/react-dock/src/openutau/clone-state.jsx#L124-L125) | Hardcoded Pen lyric/duration and local vibrato defaults are not an editable/persistent defaults or presets dialog ([`note-properties-panel.jsx`](../../ui-mock/react-dock/src/openutau/note-properties-panel.jsx#L35-L40), [`note-properties-panel.jsx`](../../ui-mock/react-dock/src/openutau/note-properties-panel.jsx#L88-L107)). |
| [Paste Parameters](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/PasteParamDialog.axaml#L10-L23) | **Missing:** [Edit menu](../../ui-mock/react-dock/src/openutau/editor-menus.jsx#L55-L60) | No clipboard-gated selective pitch/vibrato/expression paste UI or corresponding v2 workflow. |
| [Time Signature](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/TimeSignatureDialog.axaml#L10-L21) | **Partial, display only:** [main.jsx](../../ui-mock/react-dock/src/openutau/main.jsx#L122), [mock-data.js](../../ui-mock/react-dock/src/openutau/mock-data.js#L4-L8) | Visible `4/4` is static; no dialog or UI command. Rust has validated set/remove commands ([`v2.rs`](../../native/crates/project-session/src/v2.rs#L1198-L1251), [`views.rs`](../../native/crates/project-session/src/views.rs#L27-L37)); `engine.js` has no tempo/time-signature mapping ([`engine.js`](../../ui-mock/react-dock/src/openutau/engine.js#L26-L40)). |
| [Track Settings](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/TrackSettingsDialog.axaml#L10-L37) | **Partial disabled stub:** [arrangement-panel.jsx](../../ui-mock/react-dock/src/openutau/arrangement-panel.jsx#L299) | Gear is disabled with “Track settings are not connected to Project Session yet”; no dialog/dispatch. |
| [Mix FX](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/MixFxDialog.axaml#L1-L170) | **Partial, close functional match:** [arrangement-panel.jsx](../../ui-mock/react-dock/src/openutau/arrangement-panel.jsx#L280-L305), [track-fx-dialog.jsx](../../ui-mock/react-dock/src/openutau/track-fx-dialog.jsx#L36-L70) | Enable/EQ/comp/reverb presets, numeric controls, Apply/Cancel/Clear work through `track.set_mix_fx` ([`engine.js`](../../ui-mock/react-dock/src/openutau/engine.js#L35), [`v2.rs`](../../native/crates/project-session/src/v2.rs#L1836-L1868)); render pipeline applies FX ([`pipeline.rs`](../../native/tools/synth-cli/src/pipeline.rs#L1013-L1015), [`pipeline.rs`](../../native/tools/synth-cli/src/pipeline.rs#L1455-L1457)). Missing preset save/delete/recommended controls and apply-on-export checkbox. |
| [Type In](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/TypeInDialog.axaml#L8-L14) | **Partial lookalike:** [NameDialog](../../ui-mock/react-dock/src/openutau/file-menu.jsx#L101-L115) | Only project New/Save As name prompt, not a reusable generic input dialog ([`file-menu.jsx`](../../ui-mock/react-dock/src/openutau/file-menu.jsx#L569-L591)). |
| [Transcribe / Batch](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/TranscribeDialog.axaml#L30-L175) | **Missing:** [Tools](../../ui-mock/react-dock/src/openutau/editor-menus.jsx#L268-L309), [File menu](../../ui-mock/react-dock/src/openutau/file-menu.jsx#L566-L585) | No transcription/batch controls or project-session transcription route; audio import/render is not transcription. |
| [Expressions](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/ExpressionsDialog.axaml#L20-L133) | **Partial:** [Project Expressions table](../../ui-mock/react-dock/src/openutau/editor-menus.jsx#L63-L84), [expression lanes](../../ui-mock/react-dock/src/openutau/expression-lanes-panel.jsx#L267-L295) | Descriptor table is static/read-only with Add disabled; seven rows vs desktop ten slots. Curve edits are a distinct real path ([`engine.js`](../../ui-mock/react-dock/src/openutau/engine.js#L115-L130), Rust curve writes), not descriptor CRUD. |
| [Phonetic Assistant](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/PhoneticAssistant.axaml#L8-L17) | **Missing assistant UI:** [phoneme toggle](../../ui-mock/react-dock/src/openutau/piano-roll-panel.jsx#L819-L823), [mock phoneme table](../../ui-mock/react-dock/src/openutau/mock-data.js#L67-L85) | No G2P input/output/Copy. Rust has candidate compose/pronunciation routes, but React engine/backends expose no compose/pronunciation intent; proposal remains pre-freeze ([`phoneme-compose-contract-proposal.md`](phoneme-compose-contract-proposal.md#L1-L21)). |
| [DiffSinger Script Export](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/DsScriptExportDialog.axaml#L20-L48) | **Missing:** [File exports](../../ui-mock/react-dock/src/openutau/file-menu.jsx#L581-L585) | No `.ds` options, tensor-cache/variance controls or script export service. |
| [Splash](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/SplashWindow.axaml#L5-L27) | **Missing:** direct shell mount in [main.jsx](../../ui-mock/react-dock/src/openutau/main.jsx#L366-L370) | No startup branding/progress surface or startup exception dialog. |
| [Loading Window](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/LoadingWindow.axaml#L6-L20) | **Partial feedback only:** [inline status](../../ui-mock/react-dock/src/openutau/main.jsx#L280-L315), busy-disabled [File menu](../../ui-mock/react-dock/src/openutau/file-menu.jsx#L561-L563) | No standalone delayed-modal asynchronous loading service. |
| [Updater](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/UpdaterDialog.axaml#L6-L34) | **Missing:** no updater entry in [Tools menu](../../ui-mock/react-dock/src/openutau/editor-menus.jsx#L291-L309) | No version/status/check/update UI; external network outcomes are unverified. |
| [Debug Window](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/DebugWindow.axaml#L7-L38) | **Missing as a debug surface:** nearest [last-error status](../../ui-mock/react-dock/src/openutau/main.jsx#L315) | No log list/copy/reverse/clear/log-level control; last-error text is not the Debug Window. |
| [MessageBox](../../ref-openutau-clean/desktop-ref/OpenUtau/Views/MessageBox.axaml#L6-L68) | **Partial generic presentation:** [modal/toast](../../ui-mock/react-dock/src/openutau/file-menu.jsx#L82-L99), [toast](../../ui-mock/react-dock/src/openutau/file-menu.jsx#L587), [last error](../../ui-mock/react-dock/src/openutau/main.jsx#L315) | No caller-defined result buttons, exception stack or URL links; desktop result semantics vary by caller. |

**Crosswalk corrections / evidence boundary.** The table classifies Voice Color Mapping as **missing**; Bright/Dark and disabled `voice color y` are only a clone-side lookalike. Note Properties contains contradictory copy: non-Basic sections say disconnected ([`note-properties-panel.jsx`](../../ui-mock/react-dock/src/openutau/note-properties-panel.jsx#L51-L52)), but vibrato controls are actually session-mapped ([`engine.js`](../../ui-mock/react-dock/src/openutau/engine.js#L81-L109)) and panel text acknowledges Rust state ([`note-properties-panel.jsx`](../../ui-mock/react-dock/src/openutau/note-properties-panel.jsx#L107)). Treat that as inaccurate UI wording, not disconnected backend. Likewise singer assignment and Mix FX are real session routes, but installing a singer bank is separate OPFS storage and is not session CRUD. Statuses are source-level only; no live visual, build or test verification was performed.
