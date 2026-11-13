# T035 — P2C edit-during-playback and advanced scheduling

**Status:** blocked — v1 stop-and-replay policy is settled; host acceptance follows T033/T024  
**Owner:** unassigned — proposed Streaming/transport lane  
**Last updated (UTC):** 2026-10-15T04:23:42Z

## Scope and deliverables

### V1: invalidate playback after an edit

When a note or FX edit changes the Rust Session revision during playback, stop the old audio and return transport to stopped. Do not restart automatically; the next Play must render/read the new revision. Never keep playing stale audio under a changed project.

### Later: live edit scheduling

Seamless future-buffer replacement, lookahead scheduling, backpressure and user-visible stale/late-update behavior are later work. They are not required for V1 and need a separate implementation scope/approval after the streaming baseline is accepted.

## Existing task relationship

- Depends on [T033](T033-session-streaming-product-integration.md), P0C [T024](T024-render-worker-lifecycle.md), and P0B [T023](T023-wasm-session-transport.md).
- Reuse done [T020](T020-arrangement-worker-isolation.md) playhead/AudioWorklet evidence; coordinate UI transport through T004/T019 and FX with T018. T011's current pool integration is not presumed to support Session edits during playback.

## Dependencies and blockers

- User decision (2026-10-15): for v1, editing notes or FX during playback stops the old audio; the user presses Play again to hear the updated project. Do not implement seamless future-buffer replacement in this increment.
- Read-only source check: `clone-state.jsx` keys playback by session ID, revision and bank epoch; when that identity changes during playback it calls `player.pause()`, resets transport to stopped and asks the user to press Play. `audio-playback.js::pause()` advances the playback intent and stops the active source. This is source evidence only; there is no focused React/browser regression proving the full edit-to-stop-to-replay flow yet.
- Host acceptance still follows T033 and T024. T033's WebSocket host gate/credential decision and T024's Web memory budget remain unresolved. Keep live edit scheduling, lookahead and future-buffer replacement as later work; no public Session API change is required for the chosen v1 stop-and-replay policy.

## Likely files / ownership

Approved Rust Session streaming API, worker transport, scheduler, AudioWorklet client, and deterministic edit/playback tests. Coordinate with T023/T024/T033 and existing T011/T018 owners before touching shared runtime files.

## Acceptance and evidence

- [x] Direct-WASM: a note edit during playback is accepted through Rust, stops the old source, and does not restart automatically.
- [x] Direct-WASM: after a fresh Play, the player uses revision 2; audio resumes after the updated full-project mixdown renders.
- [ ] Repeat the focused browser regression for the Native HTTP/WebSocket stream host after T033 provides a non-Android credential and advertises the tested capability. Android remains deferred.
- [x] The edit is reflected by Rust Session revision `1 → 2`; playback remains an adapter around that authoritative state. Playback time is shown from the transport/AudioContext path.

## Direct-WASM browser regression (2026-10-15T04:23:42Z)

- Used the actual React editor at `http://127.0.0.1:4187/openutau.html` in the Codex in-app Chromium browser. Imported checked-in `web/fixture/scale-demo.ustx` into a transient Direct-WASM Rust Session and temporarily installed `web/fixture/bank` as `Dev Fixture` in OPFS so the 29-note fixture rendered.
- Started playback; the output meter reached `−18.1 dBFS`. While it was playing, changed one selected note from MIDI 76/E5 to 77/F5 through the Note Properties control. The Rust Session revision advanced `1 → 2`; the playback control returned to Play, the playhead stayed at `00:01.889`, output stopped, and the UI did not restart audio while the new preview rendered.
- After preview completion, pressed Play again. The updated full-project mixdown rendered for about 3 seconds; then playback resumed from revision 2, the output meter reached `−19.4 dBFS`, and the playhead advanced to `00:03.215`. This verifies the selected Direct-WASM v1 stop-and-replay behavior; the temporary render delay is expected on the full-mixdown fallback.
- Cleanup: stopped playback, removed only the temporary `Dev Fixture` bank (the pre-existing Japanese bank remained listed), closed the test tab, stopped Vite, and removed the generated `/tmp/rusted-utau-fixture-bank.zip`. The imported project was never saved; no project bundle or checked-in fixture was modified.
- This is one Direct-WASM browser result, not Native HTTP/WebSocket evidence, device-matrix coverage, or a memory/latency benchmark.

## Concurrency and approvals

Implementation follows T033 and may proceed with T034 only on disjoint modules with explicit file ownership. No implementation or contract approval is granted by this Taskboard update.

## Repository checkpoint and next step

- Current checkout: `wasm-native-rust`, HEAD `e77cc1a63a29ede04dfbe2c2c6ae768d94470a7f`; current dirty files are not assumed owned.
- Plan references: [Worker-first UI plan](../../planning/openutau-ui-parity-plan.md), [Taskboard](../BOARD.md).
- Next: when T033's Web credential/capability gate is resolved, run the same regression through Native HTTP/WebSocket and record its host configuration. The Direct-WASM v1 regression is complete; keep T024's memory-budget decision as a separate host acceptance gate. Track seamless live scheduling separately if the user later requests it.
