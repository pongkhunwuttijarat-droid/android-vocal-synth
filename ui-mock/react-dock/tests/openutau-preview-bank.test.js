// The render preview reads its samples from an installed voicebank when there
// is one; only when the host has none does it fall back to the repository's dev
// fixture. That fallback is exactly what makes the strip look like the engine
// disagrees with OpenUtau, so the order is pinned here.
//
// `choosePreviewBank` is pure on purpose: which bank a project *uses* is
// Rust-owned state (`tracks[].singer_id`), and this only decides where to read
// the samples that reproduce it.
import test from 'node:test';
import assert from 'node:assert/strict';
import { choosePreviewBank } from '../src/openutau/preview-bank.js';

test('an installed bank wins over the dev fixture', () => {
  // No project singer: the only installed bank is the best the host can do, and
  // it must not be the fixture.
  assert.deepEqual(
    choosePreviewBank({ installed: ['Teto'], singerId: null, override: null }),
    { kind: 'opfs', root: 'Teto', reason: 'the only installed voicebank' },
  );
});

test('the project singer picks its own bank out of several', () => {
  assert.equal(choosePreviewBank({ installed: ['Teto', 'Haru'], singerId: 'Haru' }).root, 'Haru');
  // OpenUtau singer ids can carry a subbank/color, and the bank is the head.
  assert.equal(choosePreviewBank({ installed: ['Teto', 'Haru'], singerId: 'Teto/soft' }).root, 'Teto');
  // Directory names do not always preserve case.
  assert.equal(choosePreviewBank({ installed: ['Teto'], singerId: 'teto' }).root, 'Teto');
  // A singer the host does not have falls back rather than failing.
  assert.equal(choosePreviewBank({ installed: ['Haru'], singerId: 'Teto' }).root, 'Haru');
});

test('?voicebank= selects a bank by name, and "fixture" forces the dev bank', () => {
  assert.deepEqual(
    choosePreviewBank({ installed: ['Teto', 'Haru'], singerId: 'Teto', override: 'haru' }),
    { kind: 'opfs', root: 'Haru', reason: '?voicebank=haru' },
  );
  assert.equal(choosePreviewBank({ installed: ['Teto'], override: 'fixture' }).kind, 'fixture');
  // A stale link (a bank that is gone) still previews *something*.
  assert.deepEqual(
    choosePreviewBank({ installed: ['Haru'], override: 'Teto' }),
    { kind: 'opfs', root: 'Haru', reason: '?voicebank=Teto is not installed' },
  );
});

test('with nothing installed the dev fixture is named as a stand-in', () => {
  const choice = choosePreviewBank({ installed: [], singerId: 'Teto', override: null });
  assert.equal(choice.kind, 'fixture');
  assert.match(choice.reason, /no voicebank is installed/);
});

test('a host with no voicebank and no fixture has no source, not a wrong one', () => {
  assert.equal(choosePreviewBank({ installed: [], override: null, fixture: false }), null);
});
