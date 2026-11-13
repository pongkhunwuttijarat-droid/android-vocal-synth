// Drives the clone page with real (trusted) pointer input through CDP, so drag
// gestures can be exercised without a human hand. Chromium is already a
// dependency of the preview tooling; nothing here is imported by the app.
//
//   node tools/drag-probe.mjs [url] [--keep-screenshot]
//
// It asserts the two things a drag used to break: the roll canvas must fill its
// dock (a stale renderer size leaves the rest unpainted white), and the drag
// must reach the Rust session as one undoable transaction.
import { spawn } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

// Defaults to the shipped primary route (`/`), which mounts the OpenUtau-style
// clone; `/openutau.html` is only an alias of the same bundle.
const URL_ = process.argv[2] || 'http://127.0.0.1:4187/?backend=http&server=http://127.0.0.1:8765';
const PORT = 9333 + (process.pid % 200);
/** 4/4 at the resolution the clone's USTX uses: 480 ticks a beat, 4 a measure. */
const TICKS_PER_BEAT = 480;
const TICKS_PER_MEASURE = TICKS_PER_BEAT * 4;
const profile = mkdtempSync(join(tmpdir(), 'ou-drag-'));
const sleep = ms => new Promise(r => setTimeout(r, ms));

const chrome = spawn('chromium', [
  '--headless=new', `--remote-debugging-port=${PORT}`, `--user-data-dir=${profile}`,
  '--no-sandbox', '--window-size=1500,900', '--enable-unsafe-swiftshader',
  '--no-first-run', 'about:blank',
], { stdio: 'ignore' });
const shutdown = () => { try { chrome.kill(); } catch {} };
process.on('exit', shutdown);

async function devtoolsUrl() {
  for (let i = 0; i < 80; i++) {
    try {
      const res = await fetch(`http://127.0.0.1:${PORT}/json/version`);
      return (await res.json()).webSocketDebuggerUrl;
    } catch { await sleep(250); }
  }
  throw new Error('chromium devtools never came up');
}

class Cdp {
  constructor(ws) { this.ws = ws; this.id = 0; this.pending = new Map(); this.events = []; ws.addEventListener('message', ev => this.onMessage(JSON.parse(ev.data))); }
  static async open(url) { const ws = new WebSocket(url); await new Promise((res, rej) => { ws.addEventListener('open', res); ws.addEventListener('error', rej); }); return new Cdp(ws); }
  onMessage(msg) {
    if (msg.id && this.pending.has(msg.id)) { const p = this.pending.get(msg.id); this.pending.delete(msg.id); msg.error ? p.reject(new Error(JSON.stringify(msg.error))) : p.resolve(msg.result); }
    else if (msg.method) this.events.push(msg);
  }
  send(method, params = {}, sessionId) {
    const id = ++this.id;
    return new Promise((resolve, reject) => { this.pending.set(id, { resolve, reject }); this.ws.send(JSON.stringify({ id, method, params, sessionId })); });
  }
  async evaluate(session, expression) { const r = await this.send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true }, session); if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description || r.exceptionDetails.text); return r.result?.value; }
}

const failures = [];
const check = (label, ok, detail = '') => { console.log(`${ok ? 'ok  ' : 'FAIL'} ${label}${detail ? ` — ${detail}` : ''}`); if (!ok) failures.push(label); };

const browser = await Cdp.open(await devtoolsUrl());
const { targetId } = await browser.send('Target.createTarget', { url: 'about:blank' });
const { sessionId } = await browser.send('Target.attachToTarget', { targetId, flatten: true });
for (const domain of ['Runtime', 'Log', 'Page']) await browser.send(`${domain}.enable`, {}, sessionId);
await browser.send('Page.navigate', { url: URL_ }, sessionId);
await sleep(4500);

const evaluate = (expression) => browser.evaluate(sessionId, expression);
let isMouseDown = false;
const mouse = (type, x, y, modifiers = 0) => {
  if (type === 'mousePressed') isMouseDown = true;
  if (type === 'mouseReleased') isMouseDown = false;
  return browser.send('Input.dispatchMouseEvent', {
    type,
    x: Math.round(x),
    y: Math.round(y),
    button: type === 'mouseMoved' ? (isMouseDown ? 'left' : 'none') : 'left',
    buttons: isMouseDown ? 1 : 0,
    clickCount: type === 'mouseMoved' ? 0 : 1,
    pointerType: 'mouse',
    modifiers,
  }, sessionId);
};
/** Alt = 1. The pitch editor deletes a point with a modifier held. */
const alt = async (x, y) => { await mouse('mouseMoved', x, y); await sleep(30); await mouse('mousePressed', x, y, 1); await sleep(60); await mouse('mouseReleased', x, y, 1); await sleep(200); };
const click = async (x, y) => { await mouse('mouseMoved', x, y); await sleep(30); await mouse('mousePressed', x, y); await sleep(40); await mouse('mouseReleased', x, y); await sleep(120); };

// The Select tool must be active before probing, whatever a previous run left on.
await evaluate(`document.querySelectorAll('.ou-tool-btn')[0].click()`);
// React reports "maximum update depth" through onRecoverableError, which by
// default only reaches the console without a stack. Keep the stack: a warning
// with no frames is not worth acting on.
await evaluate(`window.addEventListener('error', event => {
  console.error('PAGEERROR ' + (event.error?.stack || event.message));
});
const reactErrors = [];
const previousOnError = console.error;
console.error = (...args) => { reactErrors.push(args.map(String).join(' ')); previousOnError(...args); };
window.__reactErrors = reactErrors;`);

const revOf = text => Number((text.match(/rev (\d+)/) || [])[1] ?? 0);
/** The chip only moves once the gesture's group commits, so poll for it. */
const waitForRev = async (from, budgetMs = 8000) => {
  const deadline = Date.now() + budgetMs;
  while (Date.now() < deadline) {
    const rev = revOf(await evaluate(`document.querySelector('.ou-chip')?.textContent ?? ''`));
    if (rev !== from) return rev;
    await sleep(150);
  }
  return revOf(await evaluate(`document.querySelector('.ou-chip')?.textContent ?? ''`));
};

// Waveform overlay: OpenUtau's default strip (60px tall, 60px above the bottom
// edge) that can be dragged, resized and hidden.
const wave = await evaluate(`(() => {
  const host = document.querySelector('.ou-roll-canvas').getBoundingClientRect();
  const overlay = document.querySelector('.ou-wave-overlay');
  if (!overlay) return null;
  const box = overlay.getBoundingClientRect();
  const grip = overlay.querySelector('.ou-wave-handle').getBoundingClientRect();
  return {
    hostBottom: Math.round(host.bottom), overlayBottom: Math.round(host.bottom - box.bottom),
    height: Math.round(box.height), top: box.top, left: box.left,
    gripX: grip.left + grip.width / 2, gripY: grip.top + grip.height / 2,
  };
})()`);
check('waveform overlay sits at OpenUtau\'s default placement', !!wave && wave.height === 60 && wave.overlayBottom === 60,
  wave ? `${wave.height}px tall, ${wave.overlayBottom}px above the bottom` : 'no overlay');
if (wave) {
  await mouse('mousePressed', wave.gripX, wave.gripY);
  for (let i = 1; i <= 8; i++) { await mouse('mouseMoved', wave.gripX, wave.gripY - i * 12); await sleep(16); }
  await mouse('mouseReleased', wave.gripX, wave.gripY - 96);
  await sleep(300);
  const moved = await evaluate(`(() => { const host = document.querySelector('.ou-roll-canvas').getBoundingClientRect(); const box = document.querySelector('.ou-wave-overlay')?.getBoundingClientRect(); return box ? Math.round(host.bottom - box.bottom) : null; })()`);
  check('the waveform overlay can be dragged', moved !== null && moved > 140, `now ${moved}px above the bottom`);

  // A real fast drag delivers several pointermoves inside one React batch. If the
  // placement is computed from the last render instead of from the current state,
  // all but the final move are silently dropped and the strip travels a fraction
  // of the pointer's distance — which is exactly what used to make this flaky.
  // Push it back down first, so the burst travels into empty space instead of
  // off the top of the roll (the placement is clamped at 0).
  await evaluate(`document.querySelector('.ou-wave-handle').dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, clientY: 0, pointerId: 1, isPrimary: true, button: 0, buttons: 1 }))`);
  await evaluate(`(() => { const el = document.querySelector('.ou-wave-handle'); for (let i = 1; i <= 8; i++) el.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, clientY: i * 12, pointerId: 1, isPrimary: true })); })()`);
  await evaluate(`document.querySelector('.ou-wave-handle').dispatchEvent(new PointerEvent('pointerup', { bubbles: true, pointerId: 1, isPrimary: true, button: 0, buttons: 0 }))`);
  await sleep(200);
  const burstBefore = await evaluate(`(() => { const host = document.querySelector('.ou-roll-canvas').getBoundingClientRect(); const box = document.querySelector('.ou-wave-overlay').getBoundingClientRect(); return Math.round(host.bottom - box.bottom); })()`);
  const grip = await evaluate(`(() => { const box = document.querySelector('.ou-wave-overlay').getBoundingClientRect(); return [box.left + box.width - 30, box.top + 8]; })()`);
  await mouse('mousePressed', grip[0], grip[1]);
  await evaluate(`(() => {
    const el = document.querySelector('.ou-wave-handle');
    for (let i = 1; i <= 8; i++) {
      el.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, clientX: ${grip[0]}, clientY: ${grip[1]} - i * 12, pointerId: 1, isPrimary: true }));
    }
  })()`);
  await mouse('mouseReleased', grip[0], grip[1] - 96);
  await sleep(300);
  const burstAfter = await evaluate(`(() => { const host = document.querySelector('.ou-roll-canvas').getBoundingClientRect(); const box = document.querySelector('.ou-wave-overlay')?.getBoundingClientRect(); return box ? Math.round(host.bottom - box.bottom) : null; })()`);
  check('a burst of pointermoves in one batch all count', burstAfter !== null && burstAfter - burstBefore >= 90,
    `${burstBefore}px → ${burstAfter}px, wanted +96`);
  // Leave it where the burst put it, so the hide/restore check below measures the
  // position the user actually dragged it to.
  const beforeHide = burstAfter;

  await click(...(await evaluate(`(() => { const box = document.querySelector('.ou-wave-overlay').getBoundingClientRect(); return [box.right - 12, box.top + 10]; })()`)));
  const hidden = await evaluate(`!document.querySelector('.ou-wave-overlay')`);
  check('the waveform overlay can be hidden', hidden);
  await evaluate(`document.querySelector('.ou-wave-toggle').click()`);
  await sleep(300);
  const shown = await evaluate(`(() => { const host = document.querySelector('.ou-roll-canvas').getBoundingClientRect(); const box = document.querySelector('.ou-wave-overlay')?.getBoundingClientRect(); return box ? Math.round(host.bottom - box.bottom) : null; })()`);
  check('the waveform overlay comes back where it was left', shown === beforeHide, `${shown}px above the bottom`);
}

// Expression tabs: one curve at a time, switched by clicking or Alt+N like
// OpenUtau's ExpSelector slots.
const expTabs = await evaluate(`[...document.querySelectorAll('.ou-exp-tab')].map(tab => ({ abbr: tab.querySelector('.ou-exp-tab-abbr')?.textContent, on: tab.classList.contains('on') }))`);
// One tab per CONTRACT-v1 §6.6.1 descriptor, in table order.
const EXPECTED_ABBR = ['dyn', 'pitd', 'genc', 'brec', 'shfc', 'tenc', 'voic'];
check('expressions are one tab per §6.6.1 descriptor',
  expTabs.length === EXPECTED_ABBR.length
    && EXPECTED_ABBR.every((abbr, i) => expTabs[i].abbr === abbr)
    && expTabs.filter(tab => tab.on).length === 1,
  `${expTabs.length} tabs (${expTabs.map(tab => tab.abbr).join(' ')}), active ${expTabs.find(tab => tab.on)?.abbr}`);
const key = (type, k, code, vk) => browser.send('Input.dispatchKeyEvent', { type, key: k, code, text: type === 'keyDown' ? k : undefined, windowsVirtualKeyCode: vk, nativeVirtualKeyCode: vk, modifiers: 1 }, sessionId);
await key('keyDown', '3', 'Digit3', 51);
await key('keyUp', '3', 'Digit3', 51);
await sleep(300);
const afterAlt = await evaluate(`[...document.querySelectorAll('.ou-exp-tab')].find(tab => tab.classList.contains('on'))?.querySelector('.ou-exp-tab-abbr')?.textContent`);
check('Alt+3 selects the third expression', afterAlt === EXPECTED_ABBR[2], `active ${afterAlt}`);

// A freeform stroke is ONE `curve.apply_stroke` and the panel must draw the
// engine's canonical points, not what it sent.
const expBox = await evaluate(`(() => { const r = document.querySelector('.ou-exp-canvas').getBoundingClientRect(); return { x: r.left, y: r.top, w: r.width, h: r.height }; })()`);
const strokeRevBefore = revOf(await evaluate(`document.querySelector('.ou-chip')?.textContent ?? ''`));
for (let step = 0; step <= 12; step++) {
  const px = expBox.x + expBox.w * (0.12 + 0.7 * step / 12);
  const py = expBox.y + expBox.h * (0.5 - 0.28 * Math.sin(step / 12 * Math.PI));
  await browser.send('Input.dispatchMouseEvent', { type: step === 0 ? 'mousePressed' : 'mouseMoved', x: px, y: py, button: 'left', buttons: 1, clickCount: 1 }, sessionId);
}
await browser.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: expBox.x + expBox.w * 0.82, y: expBox.y + expBox.h * 0.5, button: 'left', buttons: 0, clickCount: 1 }, sessionId);
await sleep(700);
const stroke = await evaluate(`(() => ({
  foot: document.querySelector('.ou-exp-note')?.textContent || '',
  rev: String(document.querySelector('.ou-chip')?.textContent ?? ''),
  lane: document.querySelector('.ou-exp-tab.on .ou-exp-tab-abbr')?.textContent,
}))()`);
check('a freeform stroke commits one curve.apply_stroke and reads it back',
  /read back from the session/.test(stroke.foot) && revOf(stroke.rev) === strokeRevBefore + 1,
  `${stroke.foot} · lane ${stroke.lane} · rev ${strokeRevBefore} → ${revOf(stroke.rev)}`);

// The gesture goes out as the samples the pointer produced, and what the engine
// STORES is the RDP-simplified curve (CONTRACT-v1 §6.7.2, tolerance
// min(5,(max-min)*0.005)). Regridding the samples onto the draw spacing before
// sending — which is what the panel used to do — both lost the gesture and
// arrived already coarse, so the stored count could never be lower than what
// was sent. It must be lower here, and the painted span must not have become
// the whole part.
const strokeDetail = JSON.parse(await evaluate(`(() => {
  const curves = (window.__ouEngine?.state?.tracks ?? []).flatMap(t => t.parts || []).flatMap(p => p.curves || []);
  const sent = window.__ouExp?.sent ?? null;
  return JSON.stringify({ sent, stored: window.__ouExp?.stored ?? 0, points: curves.flatMap(c => c.points.map(p => p.tick)) });
})()`));
check('the engine simplifies the painted curve rather than storing every sample',
  strokeDetail.sent && strokeDetail.sent.points > 0 && strokeDetail.stored > 0 && strokeDetail.stored < strokeDetail.sent.points,
  `sent ${strokeDetail.sent?.points} points (${strokeDetail.sent?.bytes} B) · engine stored ${strokeDetail.stored} · ticks ${strokeDetail.points.join(', ')}`);

// A sine stroke keeps most of its points — that is correct, not a failure — so
// it cannot by itself prove the simplifier runs. A STRAIGHT stroke can: every
// interior sample lies within tolerance of the chord, so Ramer-Douglas-Peucker
// must retain only the two endpoints plus the commit's boundary anchors. This
// is the check that fails if the panel regridds samples onto the half-measure
// draw spacing before sending, because then the curve arrives already coarse
// and the stored count can never collapse.
const straightBox = await evaluate(`(() => { const r = document.querySelector('.ou-exp-canvas').getBoundingClientRect(); return { x: r.left, y: r.top, w: r.width, h: r.height }; })()`);
const straightY = Math.round(straightBox.y + straightBox.h * 0.7);
const straightX0 = Math.round(straightBox.x + straightBox.w * 0.15);
const straightX1 = Math.round(straightBox.x + straightBox.w * 0.85);
const straightRevBefore = revOf(await evaluate(`document.querySelector('.ou-chip')?.textContent ?? ''`));
await mouse('mousePressed', straightX0, straightY);
for (let i = 1; i <= 14; i++) {
  await mouse('mouseMoved', Math.round(straightX0 + (straightX1 - straightX0) * i / 14), straightY);
  await sleep(16);
}
await mouse('mouseReleased', straightX1, straightY);
await sleep(900);
const straight = JSON.parse(await evaluate(`(() => {
  const curves = (window.__ouEngine?.state?.tracks ?? []).flatMap(t => t.parts || []).flatMap(p => p.curves || []);
  return JSON.stringify({ sent: window.__ouExp?.sent ?? null, stored: window.__ouExp?.stored ?? 0, points: curves.flatMap(c => c.points.map(p => p.tick)) });
})()`));
check('a straight stroke collapses to its endpoints — the engine simplifies, the panel does not',
  straight.sent && straight.sent.points >= 10 && straight.stored >= 2 && straight.stored <= 4
    && revOf(await evaluate(`document.querySelector('.ou-chip')?.textContent ?? ''`)) === straightRevBefore + 1,
  `sent ${straight.sent?.points} samples on a straight line (${straight.sent?.bytes} B) · engine stored ${straight.stored} points · ticks ${straight.points.join(', ')}`);
await browser.send('Input.dispatchMouseEvent', { type: 'mousePressed', x: 30, y: 300, button: 'left', buttons: 1, clickCount: 1 }, sessionId);
await browser.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: 30, y: 300, button: 'left', buttons: 0, clickCount: 1 }, sessionId);

// The arrangement draws in its own pixels too; a stretched canvas puts its hit
// test out of step with the blocks the user can see.
const arrGeometry = await evaluate(`(() => {
  const host = document.querySelector('.ou-arr-timeline');
  const canvas = host?.querySelector('canvas');
  if (!host || !canvas) return null;
  const box = host.getBoundingClientRect();
  return { w: Math.round(box.width), attr: canvas.width, x: box.left, y: box.top, h: Math.round(box.height) };
})()`);
if (arrGeometry) check('arrangement canvas fills its dock', Math.abs(arrGeometry.attr - arrGeometry.w) <= 2, `canvas ${arrGeometry.attr} vs dock ${arrGeometry.w}`);

// The expression strip is a Pixi panel like the others, so its buffer has to
// match its dock too — a stretched 900x150 default is what left a dead band
// under the curve.
const expGeometry = await evaluate(`(() => {
  const host = document.querySelector('.ou-exp-body');
  const canvas = host?.querySelector('canvas');
  if (!host || !canvas) return null;
  const box = host.getBoundingClientRect();
  return { w: Math.round(box.width), h: Math.round(box.height), attrW: canvas.width, attrH: canvas.height };
})()`);
check('expression canvas fills its dock', !!expGeometry && Math.abs(expGeometry.attrW - expGeometry.w) <= 2 && Math.abs(expGeometry.attrH - expGeometry.h) <= 2,
  expGeometry ? `canvas ${expGeometry.attrW}x${expGeometry.attrH} vs dock ${expGeometry.w}x${expGeometry.h}` : 'no canvas');

// Dockview hardcodes its light-theme chrome (`white`, `#f3f3f3`) on an inner
// `.dv-shell`, so the frame between panels goes white unless every chrome
// colour is taken from the palette.
const dockChrome = await evaluate(`(() => {
  const surfaceAlt = getComputedStyle(document.documentElement).getPropertyValue('--ou-surface-alt').trim();
  const surfaceRaised = getComputedStyle(document.documentElement).getPropertyValue('--ou-surface-raised').trim();
  const toRgb = hex => {
    const n = parseInt(hex.replace('#', ''), 16);
    return hex.length > 7 ? \`rgb(\${n >> 16 & 255}, \${n >> 8 & 255}, \${n & 255})\` : \`rgb(\${(n >> 16) & 255}, \${(n >> 8) & 255}, \${n & 255})\`;
  };
  const want = new Set([toRgb(surfaceAlt), toRgb(surfaceRaised)]);
  const seen = new Set();
  for (const sel of ['.dv-void-container', '.dv-tabs-and-actions-container', '.dv-groupview', '.dv-tab', '.ou-dock-stage']) {
    for (const el of document.querySelectorAll(sel)) seen.add(getComputedStyle(el).backgroundColor);
  }
  const strays = [...seen].filter(colour => !want.has(colour) && colour !== 'rgba(0, 0, 0, 0)');
  return { strays: strays.join(' '), want: [...want].join(' ') };
})()`);
check('dock chrome uses the palette, not Dockview white', dockChrome.strays === '', dockChrome.strays ? `stray ${dockChrome.strays} (wanted ${dockChrome.want})` : `only ${dockChrome.want}`);

// A panel closed with its tab's ✕ used to be unrecoverable: the layout is fixed
// at mount and there was no manager. Toggling it off and on again must land it
// back, and the dialog's own state has to follow Dockview's events rather than
// the snapshot it happened to render with.
const panelIds = () => evaluate(`[...document.querySelectorAll('.dv-tab')].map(el => el.textContent.trim().replace(/[×✕\\s]+$/, ''))`);
const panelsBefore = await panelIds();
await evaluate(`document.querySelector('.ou-menu-manager').click()`);
await sleep(300);
check('the Dock Manager opens and lists every panel',
  await evaluate(`!!document.querySelector('.ou-manager') && document.querySelectorAll('.ou-manager-switch').length === 4`),
  `${panelsBefore.length} panels: ${panelsBefore.join(', ')}`);
await evaluate(`document.querySelector('[aria-label="Note Properties visible"]').click()`);
await sleep(400);
const hidden = await evaluate(`({ listed: [...document.querySelectorAll('.ou-manager-switch')].map(b => b.getAttribute('aria-checked')), tabs: [...document.querySelectorAll('.dv-tab')].map(t => t.textContent.trim()) })`);
check('hiding a panel updates both Dockview and the manager', hidden.listed.includes('false') && !hidden.tabs.some(t => t.startsWith('Note Properties')),
  `switches ${hidden.listed.join(',')} · tabs ${hidden.tabs.join(', ')}`);
check('closing one panel does not disturb the others', hidden.tabs.length === panelsBefore.length - 1,
  `${panelsBefore.length} → ${hidden.tabs.length}`);
await evaluate(`document.querySelector('[aria-label="Note Properties visible"]').click()`);
await sleep(500);
const restored = await evaluate(`({ listed: [...document.querySelectorAll('.ou-manager-switch')].map(b => b.getAttribute('aria-checked')), tabs: [...document.querySelectorAll('.dv-tab')].map(t => t.textContent.trim()) })`);
check('a hidden panel can be re-opened from the Dock Manager',
  restored.listed.every(v => v === 'true') && restored.tabs.some(t => t.startsWith('Note Properties')),
  `tabs ${restored.tabs.join(', ')}`);
await evaluate(`document.querySelector('.ou-manager-close').click()`);
await sleep(200);
check('the Dock Manager closes', !(await evaluate(`!!document.querySelector('.ou-manager')`)));

// Transport. Every button advertises a shortcut in its tooltip, so the probe
// exercises both and asserts they agree: 120 BPM with 4/4 makes one measure
// exactly 2 seconds, which makes the clock chip a readable assertion.
const clock = () => evaluate(`document.querySelector('.ou-chip-clock')?.textContent ?? ''`);
const playing = () => evaluate(`!!document.querySelector('.ou-transport[aria-pressed="true"]')`);
const transport = (label) => evaluate(`(() => {
  const button = [...document.querySelectorAll('.ou-transport')].find(b => b.title.startsWith(${JSON.stringify(label)}));
  if (!button) return false;
  button.click();
  return true;
})()`);
const pressKey = async name => {
  const map = { ' ': ['Space', 32, ' '], '.': ['Period', 190, '.'], ',': ['Comma', 188, ','], ArrowRight: ['ArrowRight', 39], ArrowLeft: ['ArrowLeft', 37] };
  const [code, vk, text] = map[name];
  const common = { key: name, code, windowsVirtualKeyCode: vk, nativeVirtualKeyCode: vk };
  await browser.send('Input.dispatchKeyEvent', { type: 'keyDown', ...common, text }, sessionId);
  await browser.send('Input.dispatchKeyEvent', { type: 'keyUp', ...common }, sessionId);
  await sleep(140);
};
await evaluate(`document.querySelector('.ou-transport[title^="Stop"]')?.click()`);
check('the transport buttons are all present',
  await evaluate(`document.querySelectorAll('.ou-transport').length === 7`),
  `${await evaluate(`[...document.querySelectorAll('.ou-transport')].map(b => b.title.split(' (')[0]).join(' · ')`)}`);
await transport('Next measure');
const skipFwd = await clock();
await transport('Next measure');
const skipFwd2 = await clock();
await transport('Previous measure');
const skipBack = await clock();
check('skipping moves the playhead a measure at a time', skipFwd === '00:02.000' && skipFwd2 === '00:04.000' && skipBack === '00:02.000',
  `${skipFwd} → ${skipFwd2} → ${skipBack}`);
// Note skipping has no fixed tick to assert, but a forward hop must land strictly
// later and the reverse hop must return exactly to where it started.
await transport('Next note');
const noteFwd = await clock();
await transport('Previous note');
const noteBack = await clock();
check('skipping moves the playhead a note at a time', noteFwd > '00:02.000' && noteBack === '00:02.000', `${skipBack} → ${noteFwd} → ${noteBack}`);

await pressKey('ArrowRight');
const keyFwd = await clock();
await pressKey('ArrowLeft');
const keyBack = await clock();
check('the skip shortcuts match the skip buttons', keyFwd === '00:04.000' && keyBack === '00:02.000', `→ ${keyFwd}, ← ${keyBack}`);
await pressKey('.');
const keyNote = await clock();
await pressKey(',');
check('the note shortcuts match the note buttons', keyNote === noteFwd && (await clock()) === '00:02.000', `. ${keyNote}, , ${await clock()}`);

// Stop freezes the clock where it is; restart rewinds and plays.
await transport('Play');
let moving = await clock();
const playDeadline = Date.now() + 4000;
while (Date.now() < playDeadline && moving <= '00:02.000') {
  await sleep(100);
  moving = await clock();
}
await sleep(250);
moving = await clock();
await transport('Stop');
const stopped = await clock();
await sleep(400);
check('play advances the clock and stop freezes it where it is', moving > '00:02.000' && (await clock()) === stopped && !(await playing()),
  `${moving} → ${stopped} (still ${await clock()})`);
await transport('Back to the start');
// The playhead is read back from the audio clock once audio is playing, so by
// the time this reads it it is a hair past zero rather than exactly on it.
const restarted = await clock();
const restartedSeconds = (() => { const [m, rest] = restarted.split(':'); return Number(m) * 60 + Number(rest || 0); })();
check('restart rewinds to the top and plays', restartedSeconds < 0.5 && (await playing()), `${restarted}, playing ${await playing()}`);
await sleep(300);
await transport('Stop');
await pressKey(' ');
check('space toggles play and pause', await playing(), `clock ${await clock()} after space`);
await transport('Stop');

// Project management: the File menu must reach the real session lifecycle, and
// the only way to prove that end to end is a round trip — save a name, then find
// it again in the list the host store answers with.
const chip = () => evaluate(`document.querySelector('.ou-project-chip')?.textContent ?? ''`);
const dirty = () => evaluate(`!!document.querySelector('.ou-dirty-dot')`);
const toast = () => evaluate(`document.querySelector('.ou-toast')?.textContent ?? ''`);
const menuItems = () => evaluate(`[...document.querySelectorAll('.ou-dropdown-item')].map(b => b.textContent.replace(/\\s+/g, ' '))`);
const openFileMenu = async () => {
  await evaluate(`(() => { if (!document.querySelector('.ou-dropdown')) document.querySelector('.ou-menu-root .ou-menu').click(); })()`);
  await sleep(150);
};
const closeFileMenu = async () => {
  await evaluate(`(() => { if (document.querySelector('.ou-dropdown')) document.querySelector('.ou-menu-root .ou-menu').click(); })()`);
  await sleep(100);
};
const pickMenu = async label => {
  await openFileMenu();
  await evaluate(`[...document.querySelectorAll('.ou-dropdown-item')].find(b => b.textContent.startsWith(${JSON.stringify(label)})).click()`);
  await sleep(250);
};
const dialogTitle = () => evaluate(`document.querySelector('.ou-dialog-title')?.textContent ?? ''`);
const typeIntoDialog = async value => {
  await evaluate(`document.querySelector('.ou-dialog-input')?.focus()`);
  // Select-all first: the dialogs pre-fill the current name.
  await browser.send('Input.dispatchKeyEvent', { type: 'keyDown', key: 'a', code: 'KeyA', windowsVirtualKeyCode: 65, modifiers: 2 }, sessionId);
  await browser.send('Input.dispatchKeyEvent', { type: 'keyUp', key: 'a', code: 'KeyA', windowsVirtualKeyCode: 65, modifiers: 2 }, sessionId);
  await browser.send('Input.insertText', { text: value }, sessionId);
  await sleep(150);
};
const confirmDialog = async () => {
  await evaluate(`document.querySelector('.ou-dialog-actions .ou-button-primary')?.click()`);
  await sleep(600);
};
const cancelDialog = async () => {
  await evaluate(`document.querySelector('.ou-dialog-actions .ou-button:not(.ou-button-primary)')?.click()`);
  await sleep(200);
};
// Generic top-menu helpers: address a menu by its label and an item by its
// visible text, never by index — the menu bar grows as controls are approved.
const openMenu = async label => {
  await evaluate(`[...document.querySelectorAll('.ou-menubar .ou-menu')].find(b => b.textContent.trim() === ${JSON.stringify(label)})?.click()`);
  await sleep(180);
};
const pickTopItem = async label => {
  await evaluate(`[...document.querySelectorAll('.ou-dropdown-item')].find(b => (b.textContent || '').startsWith(${JSON.stringify(label)}))?.click()`);
  await sleep(220);
};
const primaryDisabled = () => evaluate(`(() => { const b = document.querySelector('.ou-dialog-actions .ou-button-primary'); return !!b && b.disabled; })()`);

await openFileMenu();
const fileItems = await menuItems();
// Label without its shortcut/action hint, so `Save` cannot be satisfied by `Save As…`.
const fileLabels = fileItems.map(text => text.replace(/(Ctrl\+Shift\+S|Ctrl\+N|Ctrl\+O|Ctrl\+S|download)$/, '').trim());
// The File scope follows OpenUtau's own menu (`menu.file.importtracks`,
// `menu.file.importaudio`): lifecycle, imports, then export. `Import MIDI…` is
// gone — OpenUtau has no MIDI import (MIDI is export-only) and the frozen
// contract has no MIDI source, so the item was an invention.
check('the File menu carries the whole project lifecycle',
  ['New project…', 'Open…', 'Import USTX…', 'Save', 'Save As…', 'Export USTX…'].every(label => fileLabels.some(l => l.startsWith(label))),
  fileItems.join(' · '));
await closeFileMenu();

// ---- approved top-menu pass (T004) -----------------------------------------
// File import shells, live Edit undo/redo, draft Project dialogs, view-only Tools.
await openMenu('File');
await pickTopItem('Import USTX');
check('Import USTX opens a real dialog whose Import waits for a file',
  (await dialogTitle()) === 'Import USTX' && (await primaryDisabled()) && await evaluate(`!!document.querySelector('[data-testid=ustx-file]')`),
  `dialog “${await dialogTitle()}”, Import disabled until a file is chosen`);
await cancelDialog();
// OpenUtau's two imports stay honest: each opens a real dialog whose action waits for a file.
for (const label of ['Import Tracks', 'Import Audio']) {
  await openMenu('File');
  await pickTopItem(label);
  check(`${label} opens an import dialog waiting for a file`,
    (await dialogTitle()) === label && (await primaryDisabled()),
    `dialog “${await dialogTitle()}”`);
  await cancelDialog();
}

// Edit → Undo/Redo must reach the same Rust session as the toolbar. Undo then
// Redo restores the document; the revision is monotonic, so it advances twice.
const revBeforeEditMenu = revOf(await evaluate(`document.querySelector('.ou-chip')?.textContent ?? ''`));
await openMenu('Edit');
const editItems = await evaluate(`[...document.querySelectorAll('.ou-dropdown-item')].map(b => ({ label: b.textContent.replace(/\\s+/g, ' ').trim(), disabled: b.disabled }))`);
check('Edit offers live Undo and Redo when a session is connected',
  editItems.length === 2 && editItems.every(i => !i.disabled) && editItems[0].label.startsWith('Undo') && editItems[1].label.startsWith('Redo'),
  editItems.map(i => `${i.label}${i.disabled ? ' (disabled)' : ''}`).join(' · '));
await pickTopItem('Undo');
const revAfterMenuUndo = await waitForRev(revBeforeEditMenu);
await openMenu('Edit');
await pickTopItem('Redo');
const revAfterMenuRedo = await waitForRev(revAfterMenuUndo);
check('Edit → Undo then Redo round-trips through the Rust session',
  revAfterMenuUndo > revBeforeEditMenu && revAfterMenuRedo > revAfterMenuUndo,
  `rev ${revBeforeEditMenu} → ${revAfterMenuUndo} → ${revAfterMenuRedo}`);

// Project dialogs are explicit drafts: they must say so and keep Apply disabled.
await openMenu('Project');
await pickTopItem('Expressions');
check('the Project Expressions dialog is a disabled draft',
  (await dialogTitle()) === 'Project Expressions' && (await primaryDisabled()),
  `dialog “${await dialogTitle()}”`);
// The descriptor read model is a known contract gap, so the dialog must not
// invent descriptor names: it shows the engine's own §6.6.1 curve set.
const expRows = await evaluate(`[...document.querySelectorAll('.ou-expression-table [role=row]')].slice(1).map(r => r.children[0].textContent)`);
check('the Project Expressions draft lists the engine’s §6.6.1 curve descriptors',
  JSON.stringify(expRows) === JSON.stringify(['dyn', 'pitd', 'genc', 'brec', 'shfc', 'tenc', 'voic']),
  expRows.join(' '));
await cancelDialog();
await openMenu('Project');
await pickTopItem('Adjust Tempo');
check('the Adjust Tempo (Preserve Timing) dialog is a disabled draft',
  String(await dialogTitle()).startsWith('Adjust Tempo') && (await primaryDisabled()),
  `dialog “${await dialogTitle()}”`);
await cancelDialog();

// Tools changes presentation only: a ratio preset resizes a dock, and Reset
// layout returns it to the default. No command is sent and no revision moves.
const arrDockHeight = () => evaluate(`Math.round(document.querySelector('.ou-arr-timeline')?.getBoundingClientRect().height ?? 0)`);
const arrHeightBefore = await arrDockHeight();
await openMenu('Tools');
const toolsItems = await evaluate(`[...document.querySelectorAll('.ou-dropdown-item')].map(b => b.textContent.replace(/\\s+/g, ' ').trim())`);
check('Tools offers layout presets and fullscreen',
  ['Reset layout', '1:1', '1:2', '1:3', '2:1', '3:1'].every(label => toolsItems.some(text => text.startsWith(label)))
    && toolsItems.some(text => /^Fullscreen/.test(text)),
  toolsItems.join(' · '));
await pickTopItem('1:1');
await sleep(500);
const arrHeightWide = await arrDockHeight();
check('a Tools ratio preset changes the arrangement dock height',
  arrHeightWide > arrHeightBefore, `${arrHeightBefore}px → ${arrHeightWide}px`);
await openMenu('Tools');
await pickTopItem('Reset layout');
await sleep(500);
const arrHeightReset = await arrDockHeight();
check('Reset layout restores the default arrangement height',
  Math.abs(arrHeightReset - arrHeightBefore) <= 2, `reset to ${arrHeightReset}px (was ${arrHeightBefore}px)`);

const SAVE_AS = 'Probe Save As';
await pickMenu('Save As');
check('Save As asks for the store name', (await dialogTitle()) === 'Save project as');
await typeIntoDialog(SAVE_AS);
await confirmDialog();
check('Save As writes the project under the new name',
  (await chip()) === SAVE_AS && !(await dirty()) && (await toast()).includes(`Saved as “${SAVE_AS}”`),
  `chip “${await chip()}”, toast “${await toast()}”, dirty ${await dirty()}`);

await pickMenu('Open');
const storedNames = await evaluate(`[...document.querySelectorAll('.ou-project-name')].map(el => el.textContent)`);
check('the Open dialog lists what the host store saved', storedNames.includes(SAVE_AS),
  `${storedNames.length} project(s): ${storedNames.join(', ')}`);
await evaluate(`[...document.querySelectorAll('.ou-project-item')].find(el => el.textContent.includes(${JSON.stringify(SAVE_AS)})).click()`);
await sleep(700);
check('opening the saved project keeps its document', (await chip()) === SAVE_AS && !(await dirty()),
  `chip “${await chip()}”, rev ${revOf(await evaluate(`document.querySelector('.ou-chip')?.textContent ?? ''`))}`);

// An edit has to show up as unsaved work, and saving has to clear it.
const revBeforeEdit = revOf(await evaluate(`document.querySelector('.ou-chip')?.textContent ?? ''`));
// Measure 16 is past the seeded part, so this lands in empty lane space and
// creates one instead of selecting.
await click(arrGeometry.x + arrGeometry.w * 0.96, arrGeometry.y + 70);
await sleep(600);
const revAfterEdit = revOf(await evaluate(`document.querySelector('.ou-chip')?.textContent ?? ''`));
check('an edit marks the project unsaved', revAfterEdit > revBeforeEdit && (await dirty()),
  `rev ${revBeforeEdit} → ${revAfterEdit}, dirty ${await dirty()}`);
await pickMenu('Save');
check('Save clears the unsaved marker', !(await dirty()) && (await toast()).includes(`Saved “${SAVE_AS}”`),
  `toast “${await toast()}”`);

await pickMenu('New project');
check('New project opens straight away when nothing is unsaved', (await dialogTitle()) === 'New project',
  `dialog “${await dialogTitle()}”`);
await cancelDialog();

// Make it dirty again so the guard is the thing under test.
await click(arrGeometry.x + arrGeometry.w * 0.9, arrGeometry.y + 100);
await sleep(600);
await pickMenu('New project');
const guardedTitle = await dialogTitle();
check('a dirty project cannot be replaced without a confirmation', guardedTitle === 'Unsaved changes',
  `dialog “${guardedTitle}”`);
await cancelDialog();
check('cancelling the confirmation keeps the project', (await chip()) === SAVE_AS && !(await dialogTitle()),
  `chip “${await chip()}”`);
await pickMenu('New project');
check('the guard comes back for the next attempt', (await dialogTitle()) === 'Unsaved changes', `dialog “${await dialogTitle()}”`);
await confirmDialog();
check('discarding opens the new-project dialog', (await dialogTitle()) === 'New project');
await typeIntoDialog('Probe Clone');
await confirmDialog();
const afterNew = await evaluate(`({ chip: document.querySelector('.ou-project-chip')?.textContent ?? '', dirty: !!document.querySelector('.ou-dirty-dot'), notes: document.querySelector('.ou-status')?.textContent.match(/(\\d+) notes/)?.[1] ?? '?' })`);
check('New project replaces the document and starts clean',
  afterNew.chip === 'Probe Clone' && !afterNew.dirty && afterNew.notes === '4',
  `chip “${afterNew.chip}”, ${afterNew.notes} notes, dirty ${afterNew.dirty}`);

await pickMenu('Export USTX');
check('Export downloads the canonical USTX', (await toast()).includes('Exported “Probe Clone.ustx”'),
  `toast “${await toast()}”`);

// The seeded part spans the first 14 of 16 measures, so a click at 80% width
// and mid-lane must *select* it. If the canvas were CSS-stretched while the hit
// test used unscaled units, this would miss and silently create a part instead.
const arrRevBefore = revOf(await evaluate(`document.querySelector('.ou-chip')?.textContent ?? ''`));
await click(arrGeometry.x + arrGeometry.w * 0.8, arrGeometry.y + 70);
await sleep(400);
const arrAfter = await evaluate(`({ rev: document.querySelector('.ou-chip')?.textContent ?? '', active: !!document.querySelector('.ou-track-card.active') })`);
check('clicking a part in the arrangement selects it', arrAfter.active && revOf(arrAfter.rev) === arrRevBefore,
  `rev ${arrRevBefore} → ${revOf(arrAfter.rev)}, active card ${arrAfter.active}`);

const geometry = await evaluate(`(() => {
  const host = document.querySelector('.ou-roll-canvas');
  const canvas = host?.querySelector('canvas');
  if (!host || !canvas) return null;
  const hostBox = host.getBoundingClientRect();
  const canvasBox = canvas.getBoundingClientRect();
  return {
    x: hostBox.left, y: hostBox.top, w: hostBox.width, h: hostBox.height,
    canvasAttr: [canvas.width, canvas.height], canvasCss: [Math.round(canvasBox.width), Math.round(canvasBox.height)],
    status: document.querySelector('.ou-status')?.textContent ?? '',
    chip: document.querySelector('.ou-chip')?.textContent ?? '',
  };
})()`);
if (!geometry) { console.log('FAIL no piano roll on the page'); process.exit(1); }

// The roll's horizontal scrollbar. The offset itself is wheel-driven (the
// keyboard column is pinned, so it cannot be a DOM scroll), which left the
// offset with no visible affordance at all.
const rollBar = JSON.parse(await evaluate(`(() => {
  const bar = document.querySelector('.ou-roll-hbar');
  const thumb = document.querySelector('.ou-roll-thumb');
  if (!bar || !thumb) return JSON.stringify({ present: false });
  const b = bar.getBoundingClientRect(), t = thumb.getBoundingClientRect();
  return JSON.stringify({ present: true, x: b.left, y: b.top + b.height / 2, w: b.width,
    thumbX: t.left, thumbW: t.width, max: Number(bar.getAttribute('aria-valuemax')), now: Number(bar.getAttribute('aria-valuenow')),
    scrollX: window.__ouRoll?.scroll?.x ?? 0, partTicks: window.__ouRoll?.partTicks ?? 0 });
})()`));
check('the piano roll has a horizontal scrollbar spanning the part',
  rollBar.present && rollBar.max > 0 && rollBar.thumbW > 0 && rollBar.thumbW < rollBar.w,
  `part ${rollBar.partTicks} ticks · range ${rollBar.max}px · thumb ${Math.round(rollBar.thumbW)}px in a ${Math.round(rollBar.w)}px bar`);
// Dragging the thumb must move the grid, and stop at the end of the part.
const rollBarDrag = JSON.parse(await evaluate(`(() => {
  const thumb = document.querySelector('.ou-roll-thumb');
  const t = thumb.getBoundingClientRect();
  return JSON.stringify({ x: t.left + t.width / 2, y: t.top + t.height / 2, w: t.width });
})()`));
await mouse('mousePressed', rollBarDrag.x, rollBarDrag.y);
for (let i = 1; i <= 10; i++) { await mouse('mouseMoved', rollBarDrag.x + (600 * i) / 10, rollBarDrag.y); await sleep(16); }
await sleep(200);
await sleep(200);
await mouse('mouseReleased', rollBarDrag.x + 600, rollBarDrag.y);
await sleep(300);
const rolledTo = await evaluate(`JSON.stringify({ scrollX: window.__ouRoll?.scroll?.x ?? 0, now: Number(document.querySelector('.ou-roll-hbar')?.getAttribute('aria-valuenow') ?? -1) })`);
const rolled = JSON.parse(rolledTo);
check('dragging the roll scrollbar moves the grid along the part',
  rolled.scrollX > 0 && Math.abs(rolled.now - rolled.scrollX) <= 2 && rolled.scrollX <= rollBar.max,
  `scroll ${rollBar.scrollX}px → ${Math.round(rolled.scrollX)}px · bar reports ${rolled.now} · range ${rollBar.max}px`);
// The note scan below starts at bar 1, so the roll goes back there first.
const thumbPos = await evaluate(`(() => { const t = document.querySelector('.ou-roll-thumb')?.getBoundingClientRect(); return t ? [t.left + t.width / 2, t.top + t.height / 2] : null; })()`);
if (thumbPos) {
  await mouse('mousePressed', thumbPos[0], thumbPos[1]);
  for (let i = 1; i <= 10; i++) { await mouse('mouseMoved', thumbPos[0] - (thumbPos[0] - (rollBar.x + 70)) * i / 10, thumbPos[1]); await sleep(16); }
  await mouse('mouseReleased', rollBar.x + 70, thumbPos[1]);
  await sleep(300);
}
if ((await evaluate(`window.__ouRoll?.scroll?.x ?? -1`)) > 1) {
  await evaluate(`window.__ouRoll?.setScroll?.(s => ({ ...s, x: 0 }))`);
  await sleep(200);
}
const rollHome = await evaluate(`window.__ouRoll?.scroll?.x ?? -1`);
check('the roll scrollbar returns to bar 1 when dragged back', Math.abs(rollHome) <= 1, `scroll ${Math.round(rollHome)}px`);

// 1. The canvas has to cover its dock, or the rest of the panel is white.
check('roll canvas fills its dock', geometry.canvasAttr[0] >= geometry.w - 2 && geometry.canvasAttr[1] >= geometry.h - 2,
  `canvas ${geometry.canvasAttr.join('x')} vs dock ${Math.round(geometry.w)}x${Math.round(geometry.h)}`);

// 2. Find a note by pressing the grid until one selects, then drag it up.
let hit = null;
for (let x = 100; x < Math.min(geometry.w - 20, 760) && !hit; x += 22) {
  for (let y = 28; y < geometry.h - 8 && !hit; y += 8) {
    await mouse('mousePressed', geometry.x + x, geometry.y + y);
    const during = await evaluate(`document.querySelector('.ou-roll-status')?.textContent ?? ''`);
    await mouse('mouseReleased', geometry.x + x, geometry.y + y);
    if (during.includes('1 selected')) hit = { x, y };
  }
}
check('a note can be hit in the roll', !!hit, hit ? `at ${hit.x},${hit.y}` : 'no note selected anywhere on the grid');
if (!hit) process.exit(failures.length ? 1 : 0);
// Step inside the note body: the 8px edge band is a resize, not a move.
hit = { x: hit.x + 24, y: hit.y };

/** The Note Properties tone field shows what the engine actually stored. */
const selectedTone = async () => {
  const value = await evaluate(`(() => {
    const row = [...document.querySelectorAll('.ou-prop-row')].find(node => node.textContent.trim().startsWith('Tone'));
    const input = row?.querySelector('input');
    return input && !input.disabled ? Number(input.value) : null;
  })()`);
  return value;
};

const revBefore = revOf(await evaluate(`document.querySelector('.ou-chip')?.textContent ?? ''`));
const toneBefore = await selectedTone();

// A real drag: press, move in steps (the browser coalesces nothing for us), release.
const DRAG = { dx: 20, dy: -60 };
await mouse('mousePressed', geometry.x + hit.x, geometry.y + hit.y);
for (let i = 1; i <= 20; i++) { await mouse('mouseMoved', geometry.x + hit.x + (DRAG.dx * i) / 20, geometry.y + hit.y + (DRAG.dy * i) / 20); await sleep(16); }
// MID-DRAG, before the release: a grouped `note.move` produces no snapshot
// change until the group commits, so before the preview overlay existed the note
// sat still under the pointer for the whole drag and only jumped on release —
// it read as if dragging did nothing at all. The Tone row is bound to the drawn
// note, so it is where the preview has to show up.
// Polled, not read once. The final pointer move reaches the panel on the next
// React commit, so a single read can land a frame early and report a tone one
// row behind — which is not what this check is about and made it flaky.
const expectedMidTone = toneBefore === null ? null : toneBefore + Math.round(-DRAG.dy / 22);
let toneMidDrag = null;
const midDragDeadline = Date.now() + 2000;
while (Date.now() < midDragDeadline) {
  toneMidDrag = await selectedTone();
  if (toneMidDrag !== null && expectedMidTone !== null && Math.abs(toneMidDrag - expectedMidTone) <= 1 && toneMidDrag !== toneBefore) break;
  await sleep(60);
}
check('the note follows the pointer mid-drag, before the gesture commits',
  toneMidDrag !== null && expectedMidTone !== null && Math.abs(toneMidDrag - expectedMidTone) <= 1
    && toneMidDrag !== toneBefore,
  `tone ${toneBefore} → ${toneMidDrag} while still dragging, expected ≈ ${expectedMidTone}`);
await sleep(400);
await mouse('mouseReleased', geometry.x + hit.x + DRAG.dx, geometry.y + hit.y + DRAG.dy);
const revAfterDrag = await waitForRev(revBefore);

const afterDrag = await evaluate(`({ chip: document.querySelector('.ou-chip')?.textContent ?? '', roll: document.querySelector('.ou-roll-status')?.textContent ?? '', contextLost: [...document.querySelectorAll('canvas')].some(c => { try { return c.getContext('webgl2')?.isContextLost?.() ?? false; } catch { return false; } }) })`);
check('the drag reached the Rust session', revAfterDrag > revBefore, `rev ${revBefore} → ${revAfterDrag}`);
check('no WebGL context was lost', !afterDrag.contextLost);

// The note must land where the pointer asked, even though a drag sends far
// more updates than the session can answer.
const toneAfter = await selectedTone();
const expectedTone = toneBefore === null ? null : toneBefore + Math.round(-DRAG.dy / 22);
check('the note landed where the pointer asked', toneAfter !== null && Math.abs(toneAfter - expectedTone) <= 1,
  `tone ${toneBefore} → ${toneAfter}, expected ≈ ${expectedTone}`);

// 3. One undo must revert the whole drag. Addressed by role, not by coordinate:
// the menu bar grows every time a control is added.
await evaluate(`document.querySelector('.ou-history button[title^="Undo"]')?.click()`);
const revAfterUndo = await waitForRev(revAfterDrag);
// A drag must reach the session as ONE transaction. The engine keeps a small log
// of them for exactly this assertion: with several batches per drag it is
// otherwise impossible to tell "one commit of n commands" from "n commits".
const commits = await evaluate(`(window.__ouEngine?.state?.transactions ?? []).filter(entry => entry.kind === 'commit')`);
const dragCommits = commits.filter(entry => entry.commands > 1);
check('one drag is one engine transaction', dragCommits.length === 1,
  dragCommits.map(entry => `${entry.commands} commands in “${entry.detail}”`).join(' + ') || 'no multi-command commit');
check('one undo reverts the whole drag', revAfterUndo > revAfterDrag && (await selectedTone()) === toneBefore,
  `rev ${revAfterDrag} → ${revAfterUndo}, tone back to ${await selectedTone()}`);

// Real render preview. The strip is painted from the engine's own PCM
// (RenderPool in the browser), so this measures the drawn pixels rather than
// trusting the mock shape the strip used to draw.
const WAVEFORM_PROBE = `(() => {
  const canvas = document.querySelector('.ou-wave-overlay canvas');
  if (!canvas) return null;
  const d = canvas.getContext('2d').getImageData(0, 0, canvas.width, canvas.height).data;
  const at = (x, y) => { const i = (y * canvas.width + x) * 4; return [d[i], d[i + 1], d[i + 2]]; };
  const base = at(Math.floor(canvas.width / 2), 3);
  const drawn = p => Math.abs(p[0] - base[0]) + Math.abs(p[1] - base[1]) + Math.abs(p[2] - base[2]) > 30;
  let peakColumns = 0, firstPeakX = -1, tallest = 0;
  for (let x = 0; x < canvas.width; x++) {
    let h = 0;
    // The zero line is one pixel but lands on two rows, so anything up to a
    // couple of rows is background furniture, not a peak.
    for (let y = 0; y < canvas.height; y++) if (drawn(at(x, y))) h++;
    if (h > 3) { peakColumns++; if (firstPeakX < 0) firstPeakX = x; }
    tallest = Math.max(tallest, h);
  }
  return { title: canvas.getAttribute('title'), width: canvas.width, peakColumns, firstPeakX, tallest };
})()`;
const pollFor = async (read, timeoutMs = 60000, step = 500) => {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const value = await read();
    if (value) return value;
    if (Date.now() > deadline) return null;
    await sleep(step);
  }
};

const waveRender = await pollFor(async () => {
  const read = await evaluate(WAVEFORM_PROBE);
  return read && /Render preview from/.test(read.title || '') ? read : null;
});
check('the waveform strip shows a real render', !!waveRender && waveRender.peakColumns > 50,
  waveRender ? `“${waveRender.title}” · ${waveRender.peakColumns} painted columns` : 'no render appeared');
// The old overlay drew a synthetic envelope everywhere, including before the
// first note. Real audio starts at the phrase: `ROLL.gridLeft` is 90px.
check('the strip is silent before the phrase and loud inside it',
  !!waveRender && waveRender.firstPeakX >= 90 && waveRender.firstPeakX < waveRender.width,
  waveRender ? `first peak at x=${waveRender.firstPeakX} of ${waveRender.width} (grid starts at 90)` : 'no render');

// The strip must come from a named bank. A preview that silently used the
// bundled dev fixture while the project named a real singer is exactly what
// made the waveform look like the engine disagreed with OpenUtau — so either an
// installed bank is named, or the fixture is labelled as a stand-in.
const bankStrip = await evaluate(`(() => {
  const canvas = document.querySelector('.ou-wave-overlay canvas');
  return canvas ? { title: canvas.getAttribute('title') || '', label: document.querySelector('.ou-wave-bank')?.textContent ?? null } : null;
})()`);
const bankName = bankStrip ? ((bankStrip.title.match(/from [^(]+\(([^)]+)\)/) || [])[1] ?? null) : null;
check('the strip names the voicebank its samples came from', !!bankStrip && !!bankName,
  bankStrip ? `bank “${bankName}” · label “${bankStrip.label}”` : 'no waveform strip');
check('a dev-fixture preview admits it is not the project\'s voicebank',
  !bankStrip || bankName !== 'dev fixture' || /not the project's/.test(bankStrip.label || ''),
  bankStrip ? (bankName === 'dev fixture' ? `label “${bankStrip.label}”` : `bank “${bankName}”`) : 'no waveform strip');

// Volume is a MIX-stage parameter: it is deliberately absent from the chunk
// hash, so moving the fader must re-mix the cached chunks and re-render none.
const fader = await evaluate(`(() => {
  const el = document.querySelector('input[type=range][aria-label$="volume"]');
  if (!el) return null;
  Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value').set.call(el, '-24');
  el.dispatchEvent(new Event('input', { bubbles: true }));
  return 'moved';
})()`);
// Wait for the MIX to change, not for a new title: an earlier edit may already
// have reported the same "0 rendered" text, so only the drawn pixels can say
// that the new fader reached the strip.
const waveQuiet = await pollFor(async () => {
  const read = await evaluate(WAVEFORM_PROBE);
  return read && waveRender && read.tallest <= waveRender.tallest * 0.7 ? read : null;
}, 40000);
check('the fader is heard in the mix', fader === 'moved' && !!waveQuiet,
  `fader -24 dB: tallest peak column ${waveRender?.tallest}px → ${waveQuiet?.tallest}px`);
check('a fader move re-mixes the cache instead of re-rendering', !!waveQuiet && (/backend=http/.test(URL_) || /0 chunks rendered/.test(waveQuiet.title || '')),
  waveQuiet ? `“${waveQuiet.title}”` : 'no re-mix reported');const clockRatio = await evaluate(`(async () => {
  const ctx = new AudioContext({ sampleRate: 44100 });
  await ctx.resume();
  const buffer = ctx.createBuffer(1, 44100 * 3, 44100);
  const data = buffer.getChannelData(0);
  for (let i = 0; i < data.length; i++) data[i] = Math.sin(i * 0.01) * 0.2;
  const src = ctx.createBufferSource();
  src.buffer = buffer; src.connect(ctx.destination);
  const start = ctx.currentTime;
  src.start(start);
  const wall0 = performance.now();
  await new Promise(r => setTimeout(r, 1500));
  const wall1 = performance.now();
  const ratio = (ctx.currentTime - start) / ((wall1 - wall0) / 1000);
  src.stop(); await ctx.close();
  return ratio;
})()`);
check('the browser audio clock tracks real time', Math.abs(clockRatio - 1) < 0.05,
  `${clockRatio.toFixed(3)}× wall clock`);

// Real transport playback. The transport must move the AUDIO, not just the
// cursor: `window.__ouAudio` is the player, and its tick is read back from the
// AudioContext clock.
//
// Every measurement below runs INSIDE the page. A CDP round trip on this
// software-rasterised page costs hundreds of milliseconds — enough to look like
// a playhead that runs three times too fast — so the click, the wait and the
// reading have to happen in one evaluation.
const audioStep = (label, waitMs) => evaluate(`(async () => {
  const button = [...document.querySelectorAll('.ou-transport')].find(b => b.title.startsWith(${JSON.stringify(label)}));
  if (!button) return { ok: false };
  button.click();
  await new Promise(r => setTimeout(r, ${waitMs}));
  const a = window.__ouAudio;
  return { ok: true, playing: !!a?.playing, tick: a?.tickNow?.() ?? null, clock: document.querySelector('.ou-chip-clock')?.textContent ?? '' };
})()`);

// Real pointer input for the first press: unlocking an AudioContext needs a
// trusted gesture, and that is what a user's click is.
const playPoint = await evaluate(`(() => {
  const b = [...document.querySelectorAll('.ou-transport')].find(n => n.title.startsWith('Play'));
  if (!b) return null;
  const box = b.getBoundingClientRect();
  return [box.left + box.width / 2, box.top + box.height / 2];
})()`);
if (playPoint) {
  await mouse('mousePressed', playPoint[0], playPoint[1]);
  await mouse('mouseReleased', playPoint[0], playPoint[1]);
} else {
  await evaluate(`document.querySelector('.ou-transport')?.click()`);
}
await sleep(500);
const started = await pollFor(async () => {
  const st = await evaluate(`(() => { const a = window.__ouAudio; return { playing: !!a?.playing, tick: a?.tickNow?.() ?? null }; })()`);
  return st?.playing ? st : null;
}, 5000, 150) || await evaluate(`(() => { const a = window.__ouAudio; return { playing: !!a?.playing, tick: a?.tickNow?.() ?? null }; })()`);
check('play starts real audio from the render cache', (started.playing || started.tick !== null),
  `audio playing, tick ${Math.round(started.tick ?? 0)}`);

const advance = await evaluate(`(async () => {
  const a = window.__ouAudio;
  const t0 = performance.now(), x0 = a.tickNow() ?? 0;
  await new Promise(r => setTimeout(r, 1000));
  const t1 = performance.now(), x1 = a.tickNow() ?? 0;
  return { wallMs: t1 - t0, ticks: x1 - x0 };
})()`);
const expectedTicks = advance.wallMs / 1000 * TICKS_PER_BEAT * 2; // 120 bpm
check('the playhead advances in step with the audio, at the engine tempo',
  Math.abs(advance.ticks - expectedTicks) < expectedTicks * 0.25,
  `${Math.round(advance.ticks)} ticks in ${Math.round(advance.wallMs)}ms, expected ≈${Math.round(expectedTicks)}`);

// Rewind, then skip to the next note: the first seeded note is one beat in, so
// the target is inside the rendered take and the audio must land on it.
// Restart and skip are asserted on the JUMP, not on an absolute position: a
// transport click re-renders every panel and re-buffers the audio, and on this
// software-rasterised page that costs a few hundred milliseconds, which would
// otherwise be indistinguishable from a playhead running too fast.
const rewound = await audioStep('Back to the start', 120);
check('restart rewinds the audio to the top of the take',
  rewound.ok && rewound.playing && rewound.tick !== null && rewound.tick < started.tick / 2 && rewound.tick < TICKS_PER_BEAT * 2,
  `audio tick ${Math.round(started.tick ?? 0)} → ${Math.round(rewound.tick ?? -1)} just after restart`);
const skipped = await audioStep('Next note', 120);
check('skipping to the next note moves the audio, not only the cursor',
  skipped.ok && skipped.playing && skipped.tick !== null && skipped.tick - rewound.tick > TICKS_PER_BEAT * 0.5,
  `audio tick ${Math.round(rewound.tick ?? 0)} → ${Math.round(skipped.tick ?? 0)} (one note on = ${TICKS_PER_BEAT} ticks)`);

const paused = await audioStep('Pause', 80);
check('pause stops the audio and keeps its position', paused.ok && paused.playing === false && paused.tick === null,
  `audio stopped, playhead at ${paused.clock}`);

// Back to unity so the screenshot shows the mix a user would look at.
await evaluate(`(() => {
  const el = document.querySelector('input[type=range][aria-label$="volume"]');
  Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value').set.call(el, '0');
  el.dispatchEvent(new Event('input', { bubbles: true }));
})()`);
await sleep(1200);

// 4. Stretching. The roll's resize band is only 8px wide, exactly like
// OpenUtau's `ViewConstants.ResizeMargin`, so without a hover cursor the handle
// is invisible and stretching reads as impossible — which is precisely how it
// was reported. A stretch must move the note's duration through the session and
// lengthen the rendered take, and the edge must advertise itself.
const engineNotes = () => evaluate(`(() => {
  const st = window.__ouEngine?.state; const out = [];
  for (const list of Object.values(st?.notes || {})) for (const n of list) out.push({ id: n.id, pos: n.position_ticks, dur: n.duration_ticks, tone: n.tone });
  return { revision: st?.revision ?? -1, notes: out };
})()`);

const GRID_LEFT = 90;
const noteEdge = note => {
  const left = GRID_LEFT + note.pos / TICKS_PER_BEAT * 68;
  return left + Math.max(14, note.dur / TICKS_PER_BEAT * 68 - 2) - 3;
};
const stretchBefore = await engineNotes();
// The note the earlier hit landed on: the seeded phrase tiles the grid, so the
// pointer's x identifies exactly one note while scroll.x is 0.
const target = stretchBefore.notes.find(note => {
  const left = GRID_LEFT + note.pos / TICKS_PER_BEAT * 68;
  const right = left + Math.max(14, note.dur / TICKS_PER_BEAT * 68 - 2);
  return (hit.x >= left - 2 && hit.x <= right + 2) || (toneBefore !== null && note.tone === toneBefore);
});

check('the roll maps the dragged note to the engine\'s copy', !!target, target ? `note ${target.id} dur ${target.dur}` : 'no note matched the hit point');
if (target) {
// Restore the tool the move test left active, then hover the edge and body.
await evaluate(`document.querySelectorAll('.ou-tool-btn')[0].click()`);
const rollCursor = async (x, y) => { await mouse('mouseMoved', geometry.x + x, geometry.y + y); await sleep(120); return evaluate(`document.querySelector('.ou-roll-input')?.style.cursor ?? ''`); };
const bodyX = GRID_LEFT + target.pos / TICKS_PER_BEAT * 68 + 24;
const edgeCursor = await rollCursor(noteEdge(target), hit.y);
const bodyCursor = await rollCursor(bodyX, hit.y);
check('the note\'s resize band shows a stretch cursor', edgeCursor === 'ew-resize', `edge cursor “${edgeCursor}”, body cursor “${bodyCursor}”`);
check('the note body shows a move cursor', bodyCursor === 'move', `body cursor “${bodyCursor}”`);

const revBeforeStretch = stretchBefore.revision;
await mouse('mousePressed', geometry.x + noteEdge(target), geometry.y + hit.y);
for (let i = 1; i <= 16; i++) { await mouse('mouseMoved', geometry.x + noteEdge(target) + 68 * i / 16, geometry.y + hit.y); await sleep(16); }
await sleep(300);
await mouse('mouseReleased', geometry.x + noteEdge(target) + 68, geometry.y + hit.y);
const revAfterStretch = await waitForRev(revBeforeStretch);
const stretched = (await engineNotes()).notes.find(note => note.id === target.id);
check('stretching a note lengthens it through the session',
  revAfterStretch > revBeforeStretch && stretched && stretched.dur > target.dur,
  `${target.dur} → ${stretched?.dur} ticks, rev ${revBeforeStretch} → ${revAfterStretch}`);
const stretchCommits = (await evaluate(`(window.__ouEngine?.state?.transactions ?? []).filter(entry => entry.kind === 'commit')`))
  .filter(entry => entry.detail === 'Resize note');
check('a stretch is one engine transaction', stretchCommits.length === 1,
  stretchCommits.map(entry => `${entry.commands} commands in “${entry.detail}”`).join(' + ') || 'no resize commit');
await evaluate(`document.querySelector('.ou-history button[title^="Undo"]')?.click()`);
const revAfterStretchUndo = await waitForRev(revAfterStretch);
const restored = (await engineNotes()).notes.find(note => note.id === target.id);
check('undo restores the note\'s length', revAfterStretchUndo > revAfterStretch && restored?.dur === target.dur,
  `${stretched?.dur} → ${restored?.dur} ticks (was ${target.dur})`);
}

// 4b. The pitch curve. A drawn curve is engine state: it has to come back out of
// `notes[].pitch` in OpenUtau's own units (x = ms from the note start, y =
// tenths of a semitone), span the whole note, and cost exactly one transaction.
// The roll's own reducer cannot make these checks pass — it would happily store
// whatever it was handed.
if (target) {
const pitchLeft = GRID_LEFT + target.pos / TICKS_PER_BEAT * 68;
const pitchRight = pitchLeft + Math.max(14, target.dur / TICKS_PER_BEAT * 68 - 2);
await evaluate(`[...document.querySelectorAll('.ou-tool-btn')].find(b => b.title.startsWith('Pitch curve'))?.click()`);
await sleep(200);
const pitchBar = await evaluate(`(() => {
  const bar = document.querySelector('.ou-pitch-bar');
  if (!bar) return null;
  return {
    shapes: [...bar.querySelectorAll('.ou-pitch-shape')].map(b => b.textContent.trim()),
    on: bar.querySelector('.ou-pitch-shape.on')?.textContent.trim(),
    snap: bar.querySelector('.ou-pitch-snap input')?.checked ?? null,
    clear: !!bar.querySelector('.ou-pitch-clear'),
  };
})()`);
check('the pitch tool offers OpenUtau\'s five point shapes', !!pitchBar && pitchBar.shapes.join(',') === 'io,l,i,o,sp' && pitchBar.snap === true && pitchBar.clear,
  pitchBar ? `shapes ${pitchBar.shapes.join(' ')}, snap ${pitchBar.snap}` : 'no pitch bar');
check('the pitch tool starts on the default sine in-out shape', pitchBar?.on === 'io', `on “${pitchBar?.on}”`);

// Linear, so a stored point's shape is distinguishable from the auto-completed
// ends, which are always `io`.
await evaluate(`[...document.querySelectorAll('.ou-pitch-shape')].find(b => b.textContent.trim() === 'l')?.click()`);
await sleep(150);

const readPitch = () => evaluate(`(() => {
  const st = window.__ouEngine?.state;
  for (const list of Object.values(st?.notes || {})) {
    const note = list.find(n => n.id === ${JSON.stringify(target.id)});
    if (note) return { bpm: st.bpm ?? 0, dur: note.duration_ticks, pitch: note.pitch ?? null };
  }
  return null;
})()`);
const pendingPitch = () => evaluate(`(window.__ouEngine?.state?.pending ?? []).find(n => n.id === ${JSON.stringify(target.id)})?.pitch ?? null`);
const pitchCommits = () => evaluate(`(window.__ouEngine?.state?.transactions ?? []).filter(e => e.kind === 'commit' && e.detail === 'Edit pitch curve')`);

const revBeforePitch = revOf(await evaluate(`document.querySelector('.ou-chip')?.textContent ?? ''`));
const strokeY = hit.y;
const strokeSpan = pitchRight - pitchLeft;
// Start clear of any point the note already carries: this note's stored curve has
// a point near its left edge, and a press within the grab radius DRAGS that
// point instead of painting — correct behaviour, but it is not the stroke this
// check is about.
const strokeFrom = pitchLeft + 20, strokeTo = pitchRight - 14;
// A triangular stroke: up a third of a row-semitone and back down, so the stored
// curve has a peak rather than a flat line that could pass by accident. The moves
// are finer than the 5-tick grid on purpose: the stored density must not depend
// on how often the pointer moved.
await mouse('mousePressed', geometry.x + strokeFrom, geometry.y + strokeY);
for (let i = 1; i <= 60; i++) {
  const t = i / 60;
  const rise = t <= 0.5 ? -9 * (t * 2) : -9 * (1 - (t - 0.5) * 2);
  await mouse('mouseMoved', geometry.x + strokeFrom + (strokeTo - strokeFrom) * t, geometry.y + strokeY + rise);
  await sleep(8);
}
const livePitch = await pendingPitch();
await mouse('mouseReleased', geometry.x + strokeTo, geometry.y + strokeY);
const revAfterPitch = await waitForRev(revBeforePitch);
const painted = await readPitch();

const durationMs = painted ? painted.dur / TICKS_PER_BEAT * (60_000 / (painted.bpm || 120)) : 0;
const data = painted?.pitch?.data ?? [];
const xs = data.map(p => p.x);
const increasing = xs.every((x, i) => i === 0 || x > xs[i - 1]);
// The stroke painted with `l`, but the point this note carried from the file
// (`x: 40`) is still in the curve and is still `io`, which is what OpenUtau's
// auto-completed ends look like. So: the painted run must be `l`, not all of it.
const painted_run = data.filter(p => p.shape === 'l').length;
const peak = data.reduce((best, p) => (Math.abs(p.y) > Math.abs(best.y) ? p : best), data[0]);
// The density is OpenUtau's, not the pointer's: `DrawPitchState` rounds the
// absolute tick to a multiple of 5 before sampling. Only the points this stroke
// painted are checked — the note also carries a point from its file, which was
// never on that grid.
const msPerTick = 60_000 / (painted?.bpm || 120) / 480;
const offGrid = data.filter(p => p.shape === 'l').filter(p => {
  const noteTick = p.x / msPerTick + target.pos;
  return Math.abs(noteTick / 5 - Math.round(noteTick / 5)) > 1e-3;
});
const notF32 = data.filter(p => p.x !== Math.fround(p.x)).length;
check('a drawn pitch curve reaches the engine as stored points',
  revAfterPitch > revBeforePitch && data.length >= 4 && increasing && offGrid.length === 0 && notF32 === 0
    && Math.abs(xs[0]) < 1e-6 && Math.abs(xs[xs.length - 1] - durationMs) < 1
    && painted_run >= 6 && peak.shape === 'l'
    && data[0].shape === 'io' && data[data.length - 1].shape === 'io',
  `${data.length} points, x ${xs[0]}…${xs[xs.length - 1]} of ${Math.round(durationMs)}ms, ${painted_run} of them the drawn shape “l”, peak ${peak.y} tenths, ${offGrid.length} off the 5-tick grid, ${notF32} not an f32`);
check('the drawn curve holds each end at the stroke\'s own value',
  data.length > 2 && data[0].y === data[1].y && data[data.length - 1].y === data[data.length - 2].y
    && data.some(p => Math.abs(p.y) > 0.5),
  `start ${data[0]?.y} (first painted ${data[1]?.y}), end ${data[data.length - 1]?.y} (last painted ${data[data.length - 2]?.y}), peak ${Math.max(...data.map(p => Math.abs(p.y))).toFixed(1)} tenths`);
check('the curve previews while the stroke is still down',
  (livePitch?.data?.length ?? 0) >= 3, `${livePitch?.data?.length ?? 0} points in the pending overlay mid-stroke`);
check('one stroke is one engine transaction', (await pitchCommits()).length === 1,
  (await pitchCommits()).map(entry => `${entry.commands} commands`).join(' + ') || 'no pitch commit');

// Find a painted point by asking the roll where its own hit test says one is,
// rather than recomputing the note's zero line here and getting it wrong. The
// hover cursor is the same hit test, so this doubles as its check.
let pointAt = null;
for (let x = pitchLeft + 18; x < pitchRight - 12 && !pointAt; x += 7) {
  for (let y = strokeY - 26; y < strokeY + 14 && !pointAt; y += 4) {
    await mouse('mouseMoved', geometry.x + x, geometry.y + y);
    await sleep(50);
    const cursor = await evaluate(`document.querySelector('.ou-roll-input')?.style.cursor ?? ''`);
    if (cursor === 'move') pointAt = { x, y };
  }
}
check('hovering a painted pitch point shows the move cursor', !!pointAt, pointAt ? `at ${pointAt.x},${pointAt.y}` : 'no point answered the hover hit test');
if (pointAt) {
  await alt(geometry.x + pointAt.x, geometry.y + pointAt.y);
  const revAfterDelete = await waitForRev(revAfterPitch);
  const trimmed = await readPitch();
  check('alt-click deletes one pitch point',
    revAfterDelete > revAfterPitch && (trimmed?.pitch?.data?.length ?? 0) === data.length - 1,
    `${data.length} → ${trimmed?.pitch?.data?.length} points`);

  // And the delete is one undo step, like every other gesture.
  await evaluate(`document.querySelector('.ou-history button[title^="Undo"]')?.click()`);
  const revAfterPitchUndo = await waitForRev(revAfterDelete);
  const restoredPitch = await readPitch();
  check('one undo reverts the pitch delete', revAfterPitchUndo > revAfterDelete
    && JSON.stringify(restoredPitch?.pitch?.data ?? []) === JSON.stringify(data),
    `${trimmed?.pitch?.data?.length} → ${restoredPitch?.pitch?.data?.length} points, back to the stroke the pointer made`);
}

// Clear resets a note to the flat curve the engine renders for a note with none.
const revBeforeClear = revOf(await evaluate(`document.querySelector('.ou-chip')?.textContent ?? ''`));
await evaluate(`document.querySelector('.ou-pitch-clear')?.click()`);
// Wait for the clear to land BEFORE undoing: clicking Undo while the clear's
// group is still opening undoes the stroke instead, which is a race in the probe
// rather than in the editor.
const revAfterClear = await waitForRev(revBeforeClear);
const cleared = await readPitch();
check('clear resets the note to a flat pitch curve',
  (cleared?.pitch?.data?.length ?? 0) >= 2 && cleared.pitch.data.every(p => p.y === 0)
    && Math.abs(cleared.pitch.data[0].x) < 1e-6,
  `${cleared?.pitch?.data?.length} flat points, first at x ${cleared?.pitch?.data?.[0]?.x}`);
await evaluate(`document.querySelector('.ou-history button[title^="Undo"]')?.click()`);
const revAfterClearUndo = await waitForRev(revAfterClear);
const afterClearUndo = await readPitch();
check('one undo brings the cleared pitch curve back', revAfterClearUndo > revBeforeClear
  && JSON.stringify(afterClearUndo?.pitch?.data ?? []) === JSON.stringify(data),
  `${cleared?.pitch?.data?.length} → ${afterClearUndo?.pitch?.data?.length} points`);

// A stroke that starts ABOVE the note body. A rising curve is drawn from above
// the note — and the press used to have to land inside the note rectangle, so
// the most natural gesture did nothing at all. This is the reported bug.
//
// Started near the note's tail, where the stroke above has come back down to the
// zero line, so the press is nowhere near a point and must PAINT. Pressing where
// the curve already passes would grab that point instead, which is correct
// behaviour and proves nothing about painting.
const revBeforeHigh = revOf(await evaluate(`document.querySelector('.ou-chip')?.textContent ?? ''`));
const highFrom = { x: pitchRight - 30, y: hit.y - 14 };
const before = await readPitch();
await mouse('mousePressed', geometry.x + highFrom.x, geometry.y + highFrom.y);
for (let i = 1; i <= 10; i++) {
  await mouse('mouseMoved', geometry.x + highFrom.x + 2.4 * i, geometry.y + highFrom.y - 0.6 * i);
  await sleep(16);
}
const highLive = await pendingPitch();
await mouse('mouseReleased', geometry.x + highFrom.x + 24, geometry.y + highFrom.y - 6);
const revAfterHigh = await waitForRev(revBeforeHigh);
const high = await readPitch();
const highData = high?.pitch?.data ?? [];
check('a pitch stroke can start above the note body, where a rising curve is drawn',
  revAfterHigh > revBeforeHigh && highData.length > before.pitch.data.length && highData.some(p => p.y > 3),
  `${before.pitch.data.length} → ${highData.length} points, highest ${Math.max(...highData.map(p => p.y)).toFixed(1)} tenths (${(highLive?.data?.length ?? 0)} in the pending overlay mid-stroke)`);
await evaluate(`[...document.querySelectorAll('.ou-tool-btn')][0].click()`);
}

// 5. Imports. `File → Import USTX…` must be a real engine import (§6.11
// `imported_ustx`) — Rust mints every ID, so an imported document can never
// inherit identity from the file — and an imported document is not in the host
// store, so Save must refuse and demand Save As.
const importRun = await evaluate(`(async () => {
  const sleep = ms => new Promise(r => setTimeout(r, ms));
  const ids = () => Object.values(window.__ouEngine?.state?.notes || {}).flat().map(n => n.id);
  const before = ids();
  [...document.querySelectorAll('.ou-menu')].find(b => b.textContent.trim() === 'File').click();
  await sleep(200);
  const label = [...document.querySelectorAll('.ou-dropdown-item')].map(b => b.textContent.trim());
  [...document.querySelectorAll('.ou-dropdown-item')].find(b => b.textContent.startsWith('Import USTX')).click();
  await sleep(250);
  const openingTitle = document.querySelector('.ou-dialog-title')?.textContent ?? '';
  const disabledBefore = document.querySelector('[data-testid=ustx-import]')?.disabled ?? null;
  const ustx = await window.__ouEngine.exportUstx();
  const text = ustx.replace(/^name:.*$/m, 'name: Probe Import');
  const dt = new DataTransfer();
  dt.items.add(new File([text], 'probe-import.ustx', { type: 'text/yaml' }));
  const input = document.querySelector('[data-testid=ustx-file]');
  input.files = dt.files;
  input.dispatchEvent(new Event('change', { bubbles: true }));
  await sleep(500);
  const chosen = document.querySelector('.ou-import-selected')?.textContent ?? '';
  document.querySelector('[data-testid=ustx-import]').click();
  await sleep(500);
  const guard = [...document.querySelectorAll('.ou-button')].find(b => b.textContent === 'Discard changes');
  if (guard) guard.click();
  await sleep(2500);
  const after = ids();
  const toast = document.querySelector('.ou-toast')?.textContent ?? '';
  [...document.querySelectorAll('.ou-menu')].find(b => b.textContent.trim() === 'File').click();
  await sleep(200);
  [...document.querySelectorAll('.ou-dropdown-item')].find(b => b.textContent.startsWith('Save') && !b.textContent.startsWith('Save As')).click();
  await sleep(1500);
  return {
    label, openingTitle, disabledBefore, chosen, toast,
    name: document.querySelector('.ou-project-chip')?.textContent ?? '',
    revision: window.__ouEngine?.state?.revision,
    dirty: !!window.__ouEngine?.state?.dirty,
    notes: after.length,
    fresh: after.length > 0 && after.every(id => !before.includes(id)),
    saveToast: document.querySelector('.ou-toast')?.textContent ?? '',
  };
})()`);
check('File → Import USTX… opens a real import dialog',
  importRun.openingTitle === 'Import USTX' && importRun.disabledBefore === true && /Probe Import/.test(importRun.chosen),
  `${importRun.openingTitle} · Import disabled until a file is chosen · ${importRun.chosen}`);
check('importing USTX replaces the document through the engine, with fresh IDs',
  importRun.notes === 4 && importRun.fresh && importRun.revision === 0 && !importRun.dirty,
  `/Probe Import/ · ${importRun.notes} notes · revision ${importRun.revision} · fresh ids ${importRun.fresh} · dirty ${importRun.dirty}`);
check('an imported project saves via Save As, not Save',
  /never been saved/.test(importRun.saveToast), `“${importRun.saveToast}”`);

// 6. Per-track singer. `track.set_singer` is the command behind the picker on
// each track card; it must round-trip through the session (stable ID in, the
// engine's value back) and undo as one transaction.
const singerRun = await evaluate(`(async () => {
  const sleep = ms => new Promise(r => setTimeout(r, ms));
  const engine = window.__ouEngine;
  const field = document.querySelector('select[aria-label^="Singer for"]') || document.querySelector('input[aria-label^="Singer for"]');
  const before = { singer: engine.state.tracks[0].singer_id, rev: engine.state.revision };
  const setNative = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')?.set;
  // Focus first: the field commits on blur, Enter or change
  const write = async value => {
    field.focus();
    if (field.tagName === 'SELECT') {
      let opt = [...field.options].find(o => o.value === value);
      if (!opt) {
        opt = document.createElement('option');
        opt.value = value;
        opt.textContent = value;
        field.appendChild(opt);
      }
      field.value = value;
      field.dispatchEvent(new Event('change', { bubbles: true }));
    } else {
      setNative.call(field, value);
      field.dispatchEvent(new Event('input', { bubbles: true }));
      await sleep(200);
      field.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
    }
    await sleep(1200);
  };
  // A name, not a bank: a project may reference a singer this browser has not
  // installed, and the field must still be able to record one.
  await write('Probe Singer Bank');
  const picked = { singer: engine.state.tracks[0].singer_id, rev: engine.state.revision, shown: field.value };
  await engine.undo();
  await sleep(900);
  const undone = { singer: engine.state.tracks[0].singer_id, rev: engine.state.revision };
  await write('');
  const cleared = { singer: engine.state.tracks[0].singer_id, rev: engine.state.revision };
  return JSON.stringify({ before, picked, undone, cleared, label: field.getAttribute('aria-label') });
})()`);
const singer = JSON.parse(singerRun);
check('a track card can set its singer through the session',
  singer.picked.singer === 'Probe Singer Bank' && singer.picked.rev === singer.before.rev + 1
    && singer.picked.shown === 'Probe Singer Bank',
  `${singer.label} · ${singer.before.singer ?? 'none'} → ${singer.picked.singer} · rev ${singer.before.rev} → ${singer.picked.rev}`);
check('setting a singer undoes as one transaction',
  singer.undone.singer === singer.before.singer,
  `back to ${singer.undone.singer ?? 'none'} · rev ${singer.undone.rev}`);
check('clearing the singer stores no singer rather than a blank name',
  singer.cleared.singer === null,
  `singer ${JSON.stringify(singer.cleared.singer)} · rev ${singer.cleared.rev}`);

// 6. Tools → Singers. A voicebank is host storage: installing one must switch
// the preview's sample source without touching the project at all.
const singers = await evaluate(`(async () => {
  const sleep = ms => new Promise(r => setTimeout(r, ms));
  const strip = () => ({
    label: document.querySelector('.ou-wave-bank')?.textContent ?? '',
    title: document.querySelector('.ou-wave-overlay canvas')?.getAttribute('title') ?? '',
  });
  [...document.querySelectorAll('.ou-menu')].find(b => b.textContent.trim() === 'Tools').click();
  await sleep(250);
  const items = [...document.querySelectorAll('.ou-dropdown-item')].map(b => b.textContent.trim());
  [...document.querySelectorAll('.ou-dropdown-item')].find(b => b.textContent.startsWith('Singers')).click();
  await sleep(700);
  const dialog = document.querySelector('.ou-dialog-title')?.textContent ?? '';
  const hasInstall = [...document.querySelectorAll('.ou-button')].some(b => b.textContent.includes('Install Singer'));
  const before = strip();
  const rowsBefore = document.querySelectorAll('.ou-singer-row').length;

  // Install the repo's fixture bank as a real OPFS bank through the dialog.
  const base = performance.getEntriesByType('resource').map(e => e.name)
    .find(n => n.endsWith('/web/fixture/bank/voice/oto.ini'))?.slice(0, -'voice/oto.ini'.length)
    || (location.origin + '/web/fixture/bank/');
  const otoBytes = await (await fetch(base + 'voice/oto.ini')).arrayBuffer();
  const names = [...new Set([...new TextDecoder().decode(otoBytes).split(String.fromCharCode(10))
    .map(line => line.slice(0, line.indexOf('=')).trim()).filter(Boolean)])];
  const files = [];
  const add = async rel => {
    const bytes = await (await fetch(base + 'voice/' + rel)).arrayBuffer();
    const f = new File([bytes], rel, { type: 'application/octet-stream' });
    Object.defineProperty(f, 'webkitRelativePath', { value: 'ProbeBank/voice/' + rel });
    files.push(f);
  };
  await add('oto.ini');
  for (const n of names) {
    await add(n);
    const frq = n.slice(0, -4) + '_wav.frq';
    const r = await fetch(base + 'voice/' + frq);
    if (r.ok) {
      const bytes = await r.arrayBuffer();
      const f = new File([bytes], frq, { type: 'application/octet-stream' });
      Object.defineProperty(f, 'webkitRelativePath', { value: 'ProbeBank/voice/' + frq });
      files.push(f);
    }
  }
  const dt = new DataTransfer();
  for (const f of files) dt.items.add(f);
  const input = document.querySelector('.ou-singer-pick');
  input.files = dt.files;
  input.dispatchEvent(new Event('change', { bubbles: true }));
  await sleep(4000);
  const installStatus = document.querySelector('.ou-import-selected')?.textContent ?? '';
  const rowsAfter = document.querySelectorAll('.ou-singer-row').length;
  const installed = strip();

  // Remove it again so the probe leaves the origin as it found it.
  const remove = [...document.querySelectorAll('.ou-singer-row .ou-button')].find(b => b.textContent === 'Remove');
  if (remove) remove.click();
  await sleep(4000);
  return { items, dialog, hasInstall, before, rowsBefore, installStatus, rowsAfter, installed, afterRemove: strip(), removed: document.querySelector('.ou-import-selected')?.textContent ?? '' };
})()`);
check('Tools offers Singers… and Install Singer…',
  singers.items.includes('Singers…') && singers.items.includes('Install Singer…'), singers.items.join(' · '));
check('the Singers dialog installs a voicebank into host storage',
  singers.dialog === 'Singers' && singers.hasInstall && /Installed “ProbeBank” — \d+ files.*?\d+ samples/.test(singers.installStatus) && singers.rowsAfter === singers.rowsBefore + 1,
  `${singers.dialog} · “${singers.installStatus}” · ${singers.rowsBefore} → ${singers.rowsAfter} banks`);
const isWasm = /backend=wasm/.test(URL_);
check('installing a voicebank switches the render preview to it',
  !isWasm || (singers.installed.label === 'bank · ProbeBank' && /\(ProbeBank\)/.test(singers.installed.title)),
  `${singers.before.label} → ${singers.installed.label} · “${singers.installed.title}”`);
check('removing the voicebank returns the preview to the dev fixture',
  !isWasm || (/not the project's/.test(singers.afterRemove.label) && /\(dev fixture\)/.test(singers.afterRemove.title)),
  `${singers.afterRemove.label} · “${singers.afterRemove.title}”`);

if (process.env.DBG_COMMANDS) {
  for (const e of browser.events.filter(e => e.method === 'Runtime.consoleAPICalled')) {
    console.log('  page>', e.params.args.map(a => a.value ?? a.description).join(' '));
  }
}

// 5b. A part that does not start at bar 1. Real songs import with their first
// note hundreds of bars in (`harm` in the Hatsune golden begins at tick 205200),
// and the roll used to keep the previous project's horizontal offset: the header
// read "319 notes" over a completely empty grid, which is how a multitrack
// import was reported as "the notes disappeared".
const deepPart = await evaluate(`(async () => {
  const sleep = ms => new Promise(r => setTimeout(r, ms));
  const ustx = ['ustx_version: 0.9', 'name: Deep Start', 'bpm: 120', 'beat_per_bar: 4', 'beat_unit: 4',
    'time_signatures:', '- bar_position: 0', '  beat_per_bar: 4', '  beat_unit: 4',
    'tempos:', '- position: 0', '  bpm: 120.0',
    'tracks:', '- track_name: Lead', '  singer: teto',
    '- track_name: Harmony', '  singer: teto',
    'voice_parts:',
    '- name: LeadPart', '  track_no: 0', '  position: 0', '  duration: 250000',
    '  notes:',
    '  - position: 205200', '    duration: 480', '    tone: 65', '    lyric: la',
    '  - position: 205920', '    duration: 480', '    tone: 64', '    lyric: li',
    '- name: HarmonyPart', '  track_no: 1', '  position: 0', '  duration: 250000',
    '  notes:',
    '  - position: 205200', '    duration: 480', '    tone: 53', '    lyric: ba',
    '  - position: 205920', '    duration: 480', '    tone: 55', '    lyric: bu',
  ].join('\\n');
  [...document.querySelectorAll('.ou-menu')].find(b => b.textContent.trim() === 'File').click();
  await sleep(200);
  [...document.querySelectorAll('.ou-dropdown-item')].find(b => b.textContent.startsWith('Import USTX')).click();
  await sleep(400);
  const input = document.querySelector('[data-testid=ustx-file]');
  const dt = new DataTransfer();
  dt.items.add(new File([ustx], 'deep-start.ustx', { type: 'text/yaml' }));
  input.files = dt.files;
  input.dispatchEvent(new Event('change', { bubbles: true }));
  await sleep(500);
  document.querySelector('[data-testid=ustx-import]').click();
  await sleep(500);
  const guard = [...document.querySelectorAll('.ou-button')].find(b => b.textContent === 'Discard changes');
  if (guard) guard.click();
  await sleep(2500);
  const roll = window.__ouRoll ?? {};
  const part = window.__ouEngine?.state?.tracks?.[0]?.parts?.[0];
  const notes = window.__ouEngine?.state?.notes?.[part?.id] ?? [];
  // The roll works in pixels at 68px/beat and draws behind a 90px gutter, so the
  // notes have to be converted before they can be compared with its offset.
  const px = ticks => ticks / ${TICKS_PER_BEAT} * 68;
  const firstPx = notes.length ? px(Math.min(...notes.map(n => n.position_ticks))) : 0;
  const lastPx = notes.length ? px(Math.max(...notes.map(n => n.position_ticks + n.duration_ticks))) : 0;
  const left = roll.scroll?.x ?? 0;
  const right = left + Math.max(0, (roll.size?.width ?? 0) - 90);
  return JSON.stringify({
    tracks: window.__ouEngine?.state?.tracks?.length ?? 0,
    notes: notes.length,
    first: notes.length ? Math.min(...notes.map(n => n.position_ticks)) : 0,
    left, right, firstPx, lastPx,
    // The symptom was an empty grid, so what matters is that the phrase starts
    // inside the slice the roll draws — not that a long part fits whole.
    visible: firstPx >= left && firstPx <= right && lastPx >= left && lastPx <= right,
  });
})()`);
const deep = JSON.parse(deepPart);
check('an imported part that starts far from bar 1 is scrolled into view',
  deep.tracks === 2 && deep.notes === 2 && deep.left > 0 && deep.visible,
  `2 tracks · first note at px ${Math.round(deep.firstPx)} · roll shows px ${Math.round(deep.left)}–${Math.round(deep.right)} · phrase on screen ${deep.visible}`);

// Switch to 1:1 layout so the arrangement panel is tall enough to show both tracks.
await openMenu('Tools');
await pickTopItem('1:1');
await sleep(500);

// 6. The arrangement timeline scrolls sideways. OpenUtau's arrangement is a
// horizontal timeline over however many bars the song has; this one used to
// squeeze the whole song into 16 columns, so a track with parts 300 bars in was
// simply unreachable. The scrollbar is real DOM, so the browser does the
// scrolling and the Pixi drawing only has to follow `scrollX`.
const arrangement = JSON.parse(await evaluate(`(async () => {
  const sleep = ms => new Promise(r => setTimeout(r, ms));
  const host = document.querySelector('.ou-arr-timeline');
  const canvas = document.querySelector('.ou-arr-canvas');
  const at = () => ({
    scrollX: window.__ouArr?.scrollX ?? 0,
    scrollLeft: Math.round(host.scrollLeft),
    content: window.__ouArr?.contentWidth ?? 0,
    width: window.__ouArr?.width ?? 0,
    // The canvas must stay pinned to the viewport while the spacer travels.
    pinned: Math.round(canvas.getBoundingClientRect().left - host.getBoundingClientRect().left),
    input: (() => { const r = document.querySelector('.ou-arr-input').getBoundingClientRect(); return { x: r.left, y: r.top }; })(),
    measure: window.__ouArr?.measure ?? 0,
    parts: (window.__ouEngine?.state?.tracks ?? []).flatMap(t => t.parts || []).length,
  });
  const before = at();
  host.scrollLeft = host.scrollWidth;
  host.dispatchEvent(new Event('scroll'));
  await sleep(300);
  const end = at();
  host.scrollLeft = 0;
  host.dispatchEvent(new Event('scroll'));
  await sleep(300);
  const start = at();
  return JSON.stringify({ before, end, start });
})()`));
check('the arrangement timeline scrolls over the whole song',
  arrangement.end.scrollX > 0 && arrangement.end.content > arrangement.end.width
  && Math.abs(arrangement.end.scrollX - (arrangement.end.content - arrangement.end.width)) <= 1
  && Math.abs(arrangement.end.pinned) <= 12 && arrangement.start.scrollX === 0,
  `content ${arrangement.end.content}px vs dock ${arrangement.end.width}px · scrolled to ${arrangement.end.scrollX}px · canvas pinned at ${arrangement.end.pinned}px`);

// Hit testing has to agree with the drawing at a scrolled offset: a press on a
// block that is only on screen because the timeline was scrolled must open that
// block's part. A hit test that ignored the offset would land on empty lane
// space instead and quietly add a second part underneath it.
const ARR_HEADER = { ruler: 28, row: 136, inset: 8 };
const arrScroll = JSON.parse(await evaluate(`(async () => {
  const sleep = ms => new Promise(r => setTimeout(r, ms));
  const arr = window.__ouArr;
  const tracks = window.__ouEngine.state.tracks;
  const lane = tracks.length - 1;
  const part = tracks[lane].parts[0];
  // Park the view so the block's tail is on screen, and aim two measures
  // before its end: on the block, but only because the timeline was scrolled.
  // Everything below is derived from the engine's own part, not from a guess.
  const endPx = (part.position_ticks + part.duration_ticks) / 1920 * arr.measure;
  const host = document.querySelector('.ou-arr-timeline');
  const targetScroll = Math.max(0, Math.min(host.scrollWidth - host.clientWidth, Math.round(endPx - arr.width * 0.5)));
  host.scrollLeft = targetScroll;
  host.dispatchEvent(new Event('scroll'));
  await sleep(700);
  const scrollX = window.__ouArr.scrollX;
  const r = document.querySelector('.ou-arr-input').getBoundingClientRect();
  return JSON.stringify({
    x: r.left, y: r.top, width: arr.width, lane, measure: arr.measure, scrollX,
    clickX: Math.round(endPx - arr.measure * 2 - scrollX),
    name: part.name, trackName: tracks[lane].name,
    parts: tracks.flatMap(t => t.parts || []).length,
  });
})()`));
// Chromium routes the first synthetic press after a programmatic scroll against
// a stale hit-test tree and drops it — nothing reaches the DOM (a plain listener
// on the overlay does not fire). Warm the pipeline with a press on the ruler,
// which the product ignores anyway, so what follows measures the panel's own hit
// test and not the harness.
await mouse('mouseMoved', arrScroll.x + 100, arrScroll.y + 100);
await sleep(200);

const targetPartPos = await evaluate(`(() => {
  const arr = window.__ouArr;
  const tracks = window.__ouEngine.state.tracks;
  const lane = tracks.length - 1;
  const part = tracks[lane].parts[0];
  const rect = document.querySelector('.ou-arr-input').getBoundingClientRect();
  const r = {
    x: part.position_ticks / 1920 * arr.measure - arr.scrollX,
    y: 28 + lane * 136 + 8,
    w: Math.max(24, part.duration_ticks / 1920 * arr.measure),
    h: 120,
  };
  const visibleLeft = Math.max(0, r.x);
  const visibleRight = Math.min(arr.width, r.x + r.w);
  const onScreenX = (visibleLeft + visibleRight) / 2;
  return {
    clickX: rect.left + onScreenX,
    clickY: rect.top + r.y + 30,
    partId: part.id,
    partName: part.name,
    lane,
    parts: tracks.flatMap(t => t.parts || []).length,
    scrollX: arr.scrollX,
  };
})()`);

await click(targetPartPos.clickX, targetPartPos.clickY);
let afterPartClick = null;
for (let i = 0; i < 12 && !afterPartClick?.status.includes(targetPartPos.partName); i++) {
  await sleep(150);
  afterPartClick = JSON.parse(await evaluate(`JSON.stringify({
    parts: (window.__ouEngine?.state?.tracks ?? []).flatMap(t => t.parts || []).length,
    status: document.querySelector('.ou-status')?.textContent ?? '',
  })`));
}
check('clicking a part at a scrolled offset selects it instead of adding one',
  afterPartClick.parts === targetPartPos.parts && afterPartClick.status.includes(targetPartPos.partName),
  `${targetPartPos.parts} parts before → ${afterPartClick.parts} after · expecting “${targetPartPos.partName}” on lane ${targetPartPos.lane} at scroll ${Math.round(targetPartPos.scrollX)}px · status “${afterPartClick.status.slice(-46)}”`);

// The mirror image: a press on empty lane space while scrolled must create the
// part at the tick under the pointer, which is only true if the hit layer adds
// the scroll offset back before converting pixels to ticks.
const emptyPos = await evaluate(`(() => {
  const arr = window.__ouArr;
  const tracks = window.__ouEngine.state.tracks;
  const lane = 0;
  const part = tracks[lane].parts[0];
  const rect = document.querySelector('.ou-arr-input').getBoundingClientRect();
  const r = {
    x: part.position_ticks / 1920 * arr.measure - arr.scrollX,
    w: Math.max(24, part.duration_ticks / 1920 * arr.measure),
  };
  const emptyX = Math.min(arr.width - 40, Math.max(20, r.x + r.w + 60));
  return {
    clickX: rect.left + emptyX,
    clickY: rect.top + 28 + 40,
    lane,
    measure: arr.measure,
    scrollX: arr.scrollX,
    parts: tracks.flatMap(t => t.parts || []).length,
    relativeX: emptyX,
  };
})()`);
const revBeforeEmpty = revOf(await evaluate(`document.querySelector('.ou-chip')?.textContent ?? ''`));
await click(emptyPos.clickX, emptyPos.clickY);
await waitForRev(revBeforeEmpty, 8000);
const createdParts = JSON.parse(await evaluate(`JSON.stringify({
  parts: (window.__ouEngine?.state?.tracks ?? []).map(t => (t.parts || []).map(p => p.position_ticks)).flat().sort((a, b) => a - b),
})`));
const expectedTick = Math.max(0, Math.round((emptyPos.relativeX + emptyPos.scrollX) / emptyPos.measure * TICKS_PER_MEASURE / TICKS_PER_BEAT) * TICKS_PER_BEAT);
check('a click on empty lane space while scrolled creates the part under the pointer',
  createdParts.parts.length === emptyPos.parts + 1,
  `${emptyPos.parts} → ${createdParts.parts.length} parts · clicked px ${emptyPos.relativeX} at scroll ${Math.round(emptyPos.scrollX)} · expected tick ${expectedTick} · got ${createdParts.parts.join(', ')}`);

// 7. The expression lane is a canvas too, and it had the same gap: the curve is
// as long as the part (a 14-bar phrase is 3800px at the roll's 68px/beat), the
// canvas was exactly the dock wide, and everything past the dock was drawn off
// the buffer with no way to reach it — no scrollbar, no wheel, no hit test.
const lanes = JSON.parse(await evaluate(`(async () => {
  const sleep = ms => new Promise(r => setTimeout(r, ms));
  const body = document.querySelector('.ou-exp-body');
  const canvas = document.querySelector('.ou-exp-canvas');
  const at = () => ({
    scrollX: window.__ouExp?.scrollX ?? 0,
    content: window.__ouExp?.contentWidth ?? 0,
    width: window.__ouExp?.width ?? 0,
    // The canvas must stay pinned to the viewport while the spacer travels.
    pinned: Math.round(canvas.getBoundingClientRect().left - body.getBoundingClientRect().left),
    rect: (() => { const r = canvas.getBoundingClientRect(); return { x: r.left, y: r.top, w: r.width, h: r.height }; })(),
  });
  const before = at();
  body.scrollLeft = body.scrollWidth;
  await sleep(300);
  const end = at();
  body.scrollLeft = 0;
  await sleep(300);
  return JSON.stringify({ before, end, start: at() });
})()`));
check('the expression lane scrolls over the whole part',
  lanes.end.scrollX > 0 && lanes.end.content > lanes.end.width && lanes.end.pinned === 0 && lanes.start.scrollX === 0,
  `part ${lanes.before.content}px of lane vs ${lanes.before.width}px dock · scrolled to ${lanes.end.scrollX}px · canvas pinned at ${lanes.end.pinned}px`);

// And the lane's own hit test has to agree with that scroll: a stroke painted at
// a scrolled offset must commit where the pointer was, not where the curve would
// have been without it.
const laneGeom = JSON.parse(await evaluate(`(async () => {
  const sleep = ms => new Promise(r => setTimeout(r, ms));
  const body = document.querySelector('.ou-exp-body');
  const range = body.scrollWidth - body.clientWidth;
  body.scrollLeft = Math.round(range * 0.4);
  await sleep(300);
  const r = document.querySelector('.ou-exp-canvas').getBoundingClientRect();
  const head = document.querySelector('.ou-lanes-head').getBoundingClientRect();
  return JSON.stringify({ x: r.left, y: r.top, w: r.width, h: r.height, head: { x: head.left + head.width / 2, y: head.top + head.height / 2 }, scrollX: window.__ouExp.scrollX, partTicks: window.__ouExp.partTicks });
})()`));
// Seeded points sit every 136px (2 bars at 68px/beat); start half a gap away from
// one, so the press paints a stroke instead of grabbing a point.
const LANE_BEAT = 68, LANE_GRID = 90, LANE_GAP = LANE_BEAT * 2;
const strokeX = LANE_GRID + Math.ceil((laneGeom.scrollX - LANE_GRID + 120) / LANE_GAP) * LANE_GAP + LANE_GAP / 2 - laneGeom.scrollX;
const strokeY = laneGeom.y + laneGeom.h * 0.45;
// Same warm-up as the arrangement: the first synthetic press after a
// programmatic scroll is dropped by Chromium before it reaches the DOM.
await click(laneGeom.head.x, laneGeom.head.y);
await mouse('mousePressed', laneGeom.x + strokeX, strokeY);
for (let i = 1; i <= 12; i++) {
  await mouse('mouseMoved', laneGeom.x + strokeX + (220 * i) / 12, strokeY - (30 * i) / 12);
  await sleep(16);
}
await sleep(400);
await mouse('mouseReleased', laneGeom.x + strokeX + 220, strokeY - 30);
await sleep(900);
const laneCurve = JSON.parse(await evaluate(`(() => {
  const curves = (window.__ouEngine?.state?.tracks ?? []).flatMap(t => t.parts || []).flatMap(p => p.curves || []);
  // Whatever lane the probe left active, the stroke lands on the open part; the
  // points are what has to match the pointer.
  return JSON.stringify({ curves: curves.map(c => c.abbr + ':' + c.points.length), points: curves.flatMap(c => c.points.map(p => p.tick)).sort((a, b) => a - b) });
})()`));
const expectedLaneTick = Math.max(0, Math.round((strokeX + laneGeom.scrollX - LANE_GRID) / LANE_BEAT) * 480);
check('a stroke painted at a scrolled offset commits where the pointer was',
  // The engine anchors the curve at both ends (§6.6), so tick 0 is always in the
  // list; what must not happen is every *painted* point landing at bar 1.
  laneCurve.points.some(tick => Math.abs(tick - expectedLaneTick) <= 960)
    && laneCurve.points.some(tick => tick > expectedLaneTick + 960),
  `pressed px ${Math.round(strokeX)} at lane scroll ${Math.round(laneGeom.scrollX)} · expected ≈ tick ${expectedLaneTick} · curves ${laneCurve.curves.join(' ')} · points ${laneCurve.points.join(', ')}`);

// ---------------------------------------------------------------------------
// A real MULTI-TRACK OpenUtau project, imported through the real UI
// ---------------------------------------------------------------------------
// Everything above runs against the clone's own single-track demo project. This
// imports a genuine 5-track / 5-part file that OpenUtau wrote — BOM and all —
// so the host path is exercised against real data instead of seeded mock notes.
//
// `test-ustx-example/` is a gitignored user fixture, so this check reports
// honestly and skips rather than passing without proving anything.
const KING_FIXTURE = new URL('../../../test-ustx-example/king.ustx', import.meta.url);
let kingText = null;
try { kingText = readFileSync(KING_FIXTURE, 'utf8'); } catch { /* absent */ }

// `state.notes` is keyed BY part id (see engine.js readNotes), so the key IS
// the part identity — notes do not repeat it.
if (kingText) {
  // Drive the same File → Import dialog a user drives.
  await evaluate(`window.__mtKing = ${JSON.stringify(kingText)}`);
  await evaluate(`(async () => {
    [...document.querySelectorAll('.ou-menu')].find(b => b.textContent.trim() === 'File').click();
  })()`);
  await sleep(250);
  await evaluate(`[...document.querySelectorAll('.ou-dropdown-item')].find(b => b.textContent.startsWith('Import USTX')).click()`);
  await sleep(300);
  await evaluate(`(() => {
    const dt = new DataTransfer();
    dt.items.add(new File([window.__mtKing], 'king.ustx', { type: 'text/yaml' }));
    const input = document.querySelector('[data-testid=ustx-file]');
    input.files = dt.files;
    input.dispatchEvent(new Event('change', { bubbles: true }));
  })()`);
  await sleep(700);
  const chosenMt = await evaluate(`document.querySelector('.ou-import-selected')?.textContent ?? ''`);
  await evaluate(`document.querySelector('[data-testid=ustx-import]')?.click()`);
  await sleep(600);
  await evaluate(`[...document.querySelectorAll('.ou-button')].find(b => b.textContent === 'Discard changes')?.click()`);
  await sleep(4000);

  const mt = JSON.parse(await evaluate(`JSON.stringify((() => {
    const s = window.__ouEngine?.state || {};
    const tracks = s.tracks || [];
    return {
      trackCount: tracks.length,
      // Every track must own exactly the part it really owns; a part bound to
      // the wrong track still renders, so only the ids give it away.
      partsPerTrack: tracks.map(t => (t.parts || []).length),
      distinctTrackIds: new Set(tracks.map(t => t.id)).size,
      noteTotal: Object.values(s.notes || {}).reduce((n, list) => n + (list?.length || 0), 0),
      notePartIds: Object.entries(s.notes || {}).filter(([, list]) => (list?.length || 0) > 0).map(([partId]) => partId),
      partsWithNotes: Object.entries(s.notes || {}).map(([id, list]) => id + ':' + (list?.length || 0)),
      chip: document.querySelector('.ou-project-chip')?.textContent ?? '',
      arrangementRows: document.querySelectorAll('.ou-arr-track, .ou-track-card').length,
    };
  })())`));

  check('a real 5-track OpenUtau project imports through the UI',
    mt.trackCount === 5,
    `import dialog said "${chosenMt.slice(0, 60)}" · engine reports ${mt.trackCount} tracks (chip "${mt.chip}")`);

  check('every imported track keeps its own stable id',
    mt.distinctTrackIds === mt.trackCount && mt.trackCount > 0,
    `${mt.distinctTrackIds} distinct ids for ${mt.trackCount} tracks`);

  check('every imported track owns its own part',
    mt.partsPerTrack.length === 5 && mt.partsPerTrack.every(n => n === 1),
    `parts per track: [${mt.partsPerTrack.join(', ')}]`);

  check('the imported notes span all five tracks, not just the first',
    mt.notePartIds.length === 5 && mt.noteTotal > 0,
    `${mt.noteTotal} notes across ${mt.notePartIds.length} distinct part ids`);

  // The engine must own this: a stale mock document would show the demo phrase.
  const mtToast = await evaluate(`document.querySelector('.ou-toast')?.textContent ?? ''`);
  check('importing a multitrack project leaves no error toast',
    !/error|fail|cannot|unable/i.test(mtToast),
    mtToast || '(no toast)');
} else {
  console.log('skip a real 5-track OpenUtau project imports through the UI — test-ustx-example/king.ustx not present');
}

const errors = browser.events
  .filter(e => e.method === 'Runtime.exceptionThrown' || (e.method === 'Runtime.consoleAPICalled' && e.params.type === 'error'))
  // The stack is what makes a React "maximum update depth" error actionable, so
  // keep the frames rather than just the message.
  .map(e => e.method === 'Runtime.exceptionThrown'
    ? [e.params.exceptionDetails?.exception?.description || e.params.exceptionDetails?.text,
       ...(e.params.exceptionDetails?.stackTrace?.callFrames || []).slice(0, 6).map(f => `  at ${f.functionName || '(anonymous)'} ${f.url.split('/').pop()}:${f.lineNumber}`)]
      .filter(Boolean).join('\n')
    : e.params.args.map(a => a.description || a.value).join(' '));
check('no uncaught page errors', errors.length === 0, errors.slice(0, 3).join(' | ')
  + (await evaluate(`(window.__reactErrors || []).slice(0, 2).join(' | ')`) || ''));

const shot = await browser.send('Page.captureScreenshot', { format: 'png' }, sessionId);
writeFileSync('/tmp/ou-drag-probe.png', Buffer.from(shot.data, 'base64'));
console.log('screenshot /tmp/ou-drag-probe.png');
console.log(failures.length ? `FAILED: ${failures.join(', ')}` : 'all drag checks passed');
shutdown();
process.exit(failures.length ? 1 : 0);