// Which voicebank the render preview reads its samples from.
//
// This is deliberately a *pure* decision, separate from the IO that opens the
// bank (`waveform-source.js`): it is the part worth testing, and it has to be
// obvious that nothing here invents project state.
//
// Which bank a project uses is Rust-owned (`tracks[].singer_id` in the §6
// snapshot, plus the `singer` field of the canonical USTX). The preview only
// decides *where to read the samples from* on this host — an OPFS-imported bank
// or the repository's dev fixture. A preview that silently used the fixture
// while the project named a real singer is what made the waveform look like the
// engine disagreed with OpenUtau.

/**
 * Pick a preview bank from what the host has.
 *
 * Order, as the deleted legacy editor used it (`web/app/editor.js`, removed
 * 2026-10-03 when this page became the only Web UI):
 *   1. an explicit choice: `?voicebank=<installed root>` or `?voicebank=fixture`;
 *   2. the bank the project's singer names, when such a bank is installed;
 *   3. the only/first installed bank;
 *   4. the bundled dev fixture — unless `fixture` is false, in which case there
 *      is no source and the caller must say so rather than draw a wrong shape.
 *
 * @param {object} opts
 * @param {string[]} [opts.installed] OPFS bank roots on this origin
 * @param {string|null} [opts.singerId] the project track's singer id
 * @param {string|null} [opts.override] the `?voicebank=` value
 * @param {boolean} [opts.fixture] whether the bundled dev fixture may be used
 * @returns {{kind:'opfs', root:string, reason:string}|{kind:'fixture', reason:string}|null}
 */
export function choosePreviewBank({ installed = [], singerId = null, override = null, fixture = true } = {}) {
  const roots = [...installed];
  // A stale link must still preview something, so a name that is not installed
  // falls through to the normal order instead of failing.
  if (override && override !== 'fixture') {
    const wanted = findRoot(roots, override);
    if (wanted) return { kind: 'opfs', root: wanted, reason: `?voicebank=${override}` };
  }
  if (override !== 'fixture' && singerId) {
    // OpenUtau singer ids are `<bank dir>` or `<bank dir>/<color>`, so the first
    // path segment names the bank. Match case-insensitively: these ids come from
    // a directory name on a filesystem that may not preserve case.
    const head = String(singerId).split('/')[0];
    const match = findRoot(roots, head);
    if (match) return { kind: 'opfs', root: match, reason: `the project's singer “${singerId}”` };
  }
  if (override !== 'fixture' && roots.length) {
    return {
      kind: 'opfs',
      root: roots[0],
      reason: override ? `?voicebank=${override} is not installed` : 'the only installed voicebank',
    };
  }
  if (!fixture) return null;
  return {
    kind: 'fixture',
    reason: roots.length ? '?voicebank=fixture' : 'no voicebank is installed, so the dev fixture stands in',
  };
}

/** Exact match first, then case-insensitive. */
function findRoot(roots, name) {
  return roots.find(root => root === name)
    || roots.find(root => root.toLowerCase() === String(name).toLowerCase());
}
