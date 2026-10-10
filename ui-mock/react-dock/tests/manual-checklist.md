# React Dock prototype browser checklist

Verified by Chromium CDP against the isolated Vite preview (2026-09-30):

- React page mounts, Dockview renders six panels, and PixiJS creates canvases for the Piano Roll and curves.
- Workspace selector changes to Arrangement & Mix and Dockview recalculates its panel groups; switching back to Vocal Edit works.
- Layout selector changes to Curves focus and recomposes the panel layout.
- Dock Manager opens, closing Projects changes its row to Open, reopening Projects changes its row to Close, and the Manager Close action closes the modal.
- At 390px emulated viewport, document width matches viewport and Dockview stage remains visible.
- A separate peer GUI report saw the page load and canvas, but could not verify panel interactions; direct CDP assertions cover those controls.
- Mobile Dock Manager sizing is not yet verified. In the 390px CDP emulation attempt, the browser environment restored/retained a wider layout viewport while reporting 390; the manager width reported 540px. The compact CSS cap exists but needs a manual visual check on an actual narrow browser window before UX signoff.

Current boundary: project state and edits go through selectable Rust Session adapters. Render/audio playback and voicebank selection are still unavailable; layout and gesture drafts remain presentation state.


Session integration update (2026-10-01; build and automated adapter tests pass, click-level browser review pending):

- Direct WASM and Native HTTP v2 can be selected in the top bar. Project lifecycle controls call the selected adapter; HTTP projects use GET /projects and session save/open routes.
- Piano Roll reads engine note DTOs and dispatches note.add/remove; Inspector dispatches lyric/tone/move/resize; track mute/solo/part.create and mixer volume/pan use stable IDs.
- Curve strokes use actual Direct-WASM curve identity and descriptor context; Native HTTP curve mutation stays disabled pending a context-read contract. Render and playback controls are explicitly unavailable.
- Dock Manager now moves focus to Close, closes on Escape, and restores the prior focus; status feedback uses a polite live region. Browser keyboard/screen-reader review is still pending.
- Vite splits React, Dockview, and PixiJS vendor code for browser caching. The PixiJS vendor chunk remains over 500 KB; verify panel startup and loading behavior in the target browser before treating static delivery as final.

Fresh handoff review (2026-09-30):

- Opened the Vite page at `http://127.0.0.1:4187/` and visually reviewed its default desktop layout. The earlier panel insertion order let Projects occupy most of the top row and left the Piano Roll compressed. `layoutPanels()` now creates the Piano Roll root first, then docks Projects/Tracks to its left and the remaining panels around it.
- Reset after the change and confirmed the intended left / center / right dock composition, with Tracks and Mixer visible in their lower groups. Clicking New announced `New project · Session connection comes next` without creating frontend project state.
- The Piano Roll and curve canvases initially rendered black because `resizeTo="parent"` passed the literal string instead of a host element. Both applications now receive a React ref to their container. A fresh Vite page screenshot shows the piano grid, notes, playhead, and expression curve.
- Switching the curve lane to DYN visibly changes its color. Mixer fader keyboard input changes only its displayed mock percentage. The Dock Manager opens, Escape closes it, and accessibility state reports focus restored to the Dock Manager button.
- The CSS includes narrow-window Dock Manager sizing, but a true narrow-window visual review remains pending; the available browser control could not set a viewport override in this pass.

Follow-up viewport and Dock Manager review (2026-09-30):

- Zoom X/Y now transform the displayed Pixi viewport around its center. The mock note tick/pitch data stays fixed; selecting a note at 35%/60% zoom still resolves the correct mock note and inspector values. At 180%/150%, grid spacing and notes enlarge; the 100% control restores the view.
- Dock Manager rows now use accessible visibility switches instead of Open/Close actions. Hiding Projects removes that panel; toggling the switch again restores it and its checked state updates.
- A narrow-window visual check is still pending because the available browser surface does not expose a viewport override.

Dock resize regression check (2026-10-01):

- Pixi's CSS canvas had been stretched because its renderer buffer did not follow Dockview height changes. Both canvases now resize their renderer buffer from the host `ResizeObserver` and refresh the Pixi hit area.
- After dragging the Piano Roll/Curve splitter, observed the Piano Roll canvas change from 839×164 to 839×232 and the curve canvas from 839×171 to 839×103; in both cases the canvas drawing buffer matched the visible viewport exactly. Piano Roll notes kept their world dimensions, and the curve retained fixed world-space geometry while the viewport revealed/clipped more of it.
- Zoom remains uniform and anchored at document origin, so changing Dock size does not recenter the canvas contents.

Audio track / waveform strip (2026-10-04, Chromium CDP, HTTP session against a synth-server started with `--media`):

- Opened a project whose track 5 holds a wave part and clicked that part in the arrangement. The piano roll header and the footer both name the wave part, and the footer reads `render 4.0s from synth-server` — the strip follows the **active** track, not track 0.
- The strip draws peaks from the part's start to its end: pixel columns `gridLeft` → `gridLeft + beats × beatWidth`, one band of `±peak × height / 2` rows. Read one column's rows with `getImageData` rather than matching an RGBA triple — surface, zero line and peaks are three close greys and the canvas returns colour-managed values, so a triple match reports "no waveform" on a strip that is full of it.
- Switching from a long voice part to the wave part moves the roll to the part's start. Before this was fixed the viewport stayed at the previous part's offset (29,070 px on the Hatsune golden), which parks the roll past the end of a short audio part and leaves the strip blank. Re-select the voice part afterwards: it must scroll back to its own first note, not to the audio.
- An audio part with no notes must not throw away the user's vertical scroll — only `x` moves.
- Undo/redo after an audio render must leave the strip showing the audio still — an undo that changes the revision re-runs the preview, and the track it renders must be the active one. Not yet checked in a browser; the session used for the run above had no edits (rev 0), so undo is a no-op there.
