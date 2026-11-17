// Checks for gathering a streamed reply's words into one change a frame
// (src/wordsBatch.ts). Run by scripts/check.sh.
//
//   node scripts/test-words-batch.mjs

import assert from "node:assert/strict";
import { wordsBatch } from "../src/wordsBatch.ts";

/** A clock run by hand: `tick()` is a frame, `pass(ms)` lets time go by. */
function handClock() {
  let frames = [];
  let timers = [];
  let now = 0;
  return {
    frame: (run) => {
      const entry = { run };
      frames.push(entry);
      return () => (frames = frames.filter((each) => each !== entry));
    },
    after: (ms, run) => {
      const entry = { at: now + ms, run };
      timers.push(entry);
      return () => (timers = timers.filter((each) => each !== entry));
    },
    tick() {
      const due = frames;
      frames = [];
      due.forEach((each) => each.run());
    },
    pass(ms) {
      now += ms;
      const due = timers.filter((each) => each.at <= now);
      timers = timers.filter((each) => each.at > now);
      due.forEach((each) => each.run());
    },
    waiting: () => frames.length + timers.length,
  };
}

// Many pieces between two frames are handed on once, whole and in order.
{
  const clock = handClock();
  const out = [];
  const batch = wordsBatch((text) => out.push(text), clock);
  for (const piece of ["The st", "andard", " error", " is s/√n."]) batch.add(piece);
  assert.deepEqual(out, [], "nothing before the frame");
  clock.tick();
  assert.deepEqual(out, ["The standard error is s/√n."]);
  assert.equal(clock.waiting(), 0, "the fallback timer is called off with the frame");
  clock.tick();
  assert.deepEqual(out, ["The standard error is s/√n."], "a frame with nothing new hands on nothing");
}

// The next frame's words are a change of their own.
{
  const clock = handClock();
  const out = [];
  const batch = wordsBatch((text) => out.push(text), clock);
  batch.add("one ");
  clock.tick();
  batch.add("two ");
  batch.add("three");
  clock.tick();
  assert.deepEqual(out, ["one ", "two three"]);
}

// Another event flushes first, so the words before it are never handed on after it.
{
  const clock = handClock();
  const out = [];
  const batch = wordsBatch((text) => out.push(text), clock);
  batch.add("last words");
  batch.flush();
  out.push("<turn ended>");
  clock.tick();
  clock.pass(500);
  assert.deepEqual(out, ["last words", "<turn ended>"]);
}

// A window that draws no frames (covered, or on another desktop) still gets its words, after the fallback.
{
  const clock = handClock();
  const out = [];
  const batch = wordsBatch((text) => out.push(text), clock, 100);
  batch.add("unseen ");
  clock.pass(99);
  assert.deepEqual(out, []);
  clock.pass(1);
  assert.deepEqual(out, ["unseen "]);
}

// Dropping lets go of what is held.
{
  const clock = handClock();
  const out = [];
  const batch = wordsBatch((text) => out.push(text), clock);
  batch.add("gone");
  batch.drop();
  clock.tick();
  clock.pass(500);
  assert.deepEqual(out, []);
}

console.log("words batch: ok");
