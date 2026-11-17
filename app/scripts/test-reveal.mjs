// Checks for waiting on the first screen before the window is seen
// (src/reveal.ts). Run by scripts/check.sh.
//
//   node scripts/test-reveal.mjs

import assert from "node:assert/strict";

// The little of a page that whenDrawn uses: frames, a timer, a mutation observer, focus.
let frames = 0;
let observing = 0;
const listeners = new Set();
globalThis.requestAnimationFrame = (run) => {
  frames += 1;
  return setTimeout(() => run(performance.now()), 4);
};
globalThis.MutationObserver = class {
  observe() {
    observing += 1;
  }
  disconnect() {
    observing -= 1;
  }
};
const events = {
  addEventListener: (name, run) => listeners.add(`${name}`),
  removeEventListener: (name) => listeners.delete(`${name}`),
};
globalThis.document = { body: {}, hasFocus: () => true, ...events };
globalThis.window = { setTimeout, clearTimeout, ...events };

const { whenDrawn } = await import("../src/reveal.ts");
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

// A page that is drawn and still: resolved within a few frames, long before the deadline.
{
  const started = performance.now();
  await whenDrawn(() => true, started + 5000);
  assert.ok(performance.now() - started < 1000, "a drawn page does not wait for the deadline");
  assert.equal(observing, 0, "the observer is let go");
  assert.equal(listeners.size, 0, "the focus listeners are let go");
}

// A page whose content never comes: resolved at the deadline, and nothing keeps running after it.
{
  const started = performance.now();
  await whenDrawn(() => false, started + 150);
  const waited = performance.now() - started;
  assert.ok(waited >= 140 && waited < 1000, `gave up at the deadline, after ${Math.round(waited)} ms`);
  assert.equal(observing, 0, "the observer is let go");
  assert.equal(listeners.size, 0, "the focus listeners are let go");
  const after = frames;
  await sleep(100);
  assert.equal(frames, after, "no frame is asked for after giving up");
}

// A window that draws no frames at all still gives up, by the timer.
{
  const asked = globalThis.requestAnimationFrame;
  globalThis.requestAnimationFrame = () => 0;
  const started = performance.now();
  await whenDrawn(() => false, started + 100);
  assert.ok(performance.now() - started < 1000, "the timer ends the wait without frames");
  assert.equal(observing, 0);
  globalThis.requestAnimationFrame = asked;
}

console.log("reveal: ok");
