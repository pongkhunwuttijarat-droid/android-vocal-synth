# rusted Utau Web Editor

Session-backed React/Dockview/PixiJS editor frontend. Layout, tools, viewport and gesture drafts are ephemeral presentation state; project state and mutations live in the Rust Session backend:

- React 19 + Vite
- PixiJS v8 via `@pixi/react` for Piano Roll and curve surfaces
- Dockview React for docked tabs, resizing, drag/drop and floating panes

## Run

```sh
cd ui-mock/react-dock
npm install
npm run dev
```

Vite listens at <http://127.0.0.1:4187/>. `npm run build` produces a static `dist/` build with a web app manifest, SVG icon, and generated service worker that precaches the built HTML, JS, CSS, WASM, fonts, and SVG assets. Serve the complete `dist/` directory over HTTPS (or localhost) from the same path prefix used at build time. The app shell can reopen offline after its first successful load; Native HTTP Session calls remain online-only and are never cached. Projects continue to use the existing OPFS store.

The service worker is registered only in production builds. `npm run dev` remains an ordinary Vite preview without offline caching. Bump the `CACHE` version in `vite.config.js` when changing the cache lifecycle policy; old `rusted-utau-shell-*` caches are removed on activation.

## Session boundary

The OpenUtau-style editor is the only app, at `/` and `/openutau.html`. The generic Dock workspace and the legacy `web/` editor page were removed on 2026-10-03; the clone is the product. The editor offers selectable Direct WASM and Native HTTP Session backends. Project lifecycle, track mute/solo/mixer, stable-ID note edits, expression curves, RenderPool preview and audio playback use the Rust Session/render adapters. Controls without a Session command stay disabled while a live session is connected. Dock layouts persist in `localStorage`; Direct WASM projects use OPFS plus ID sidecars, while Native HTTP uses server project storage with process-local edit sessions.

## Layout model

- Workspace: Vocal Edit / Arrangement & Mix
- Layout presets: Compose / Curves focus / Mix focus
- Dockview owns tab placement, panel drag/drop, splitters, hide/close/open and float behavior
- Dock Manager offers explicit open/close for panels
- Workspace/preset layouts persist separately in browser localStorage
- Piano Roll and curves are PixiJS canvases; surrounding controls are React/HTML for keyboard/accessibility
