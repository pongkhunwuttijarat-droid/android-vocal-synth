// Where the repository's dev fixture voicebank lives, as a browser URL.
//
// The bank (`web/fixture/bank`, 6 wavs + oto.ini) ships with the repository
// rather than inside any one app, so the clone page cannot reach it by a plain
// relative path: Vite only serves files outside its root through the `/@fs/`
// route. `import.meta.url` of THIS module is already such a URL (it lives in
// `web/`), so resolving `./fixture/bank/` against it produces a base the
// browser can fetch — no build variable, and no second copy of the bank.
//
// Dev-only by nature: a production bundle has no `web/fixture/` next to it, so
// callers must treat a failed load as "no browser render source" and fall back.

/** Base URL of `web/fixture/bank` (pass to `HttpBank`). */
export function fixtureBankBaseUrl() {
  return new URL('./fixture/bank/', import.meta.url).href;
}

/** oto.ini path within the bank, and the directory its wavs live in. */
export const FIXTURE_OTO_PATH = 'voice/oto.ini';
export const FIXTURE_OTO_DIR = 'voice';
