// Checks for a running paper's time drawn as a disc (src/paperTime.ts): the share left, from the whole minutes its
// words say, and the sector that fills it. Run by scripts/check.sh; node runs the TypeScript file as it is.
//
//   node scripts/test-paper-time.mjs

import assert from "node:assert/strict";

const { discFill, shareLeft } = await import("../src/paperTime.ts");

// The share left follows the words: 53 minutes left of 75.
assert.equal(shareLeft(53, 75), 53 / 75);
assert.equal(shareLeft(75, 75), 1);
assert.equal(shareLeft(0, 75), 0);
// Never more than the whole, never less than none, and nothing for a paper with no length.
assert.equal(shareLeft(80, 75), 1);
assert.equal(shareLeft(-3, 75), 0);
assert.equal(shareLeft(10, 0), 0);

// All of the time left: the whole disc. None left, or past the end: an empty circle.
assert.equal(discFill(1), "whole");
assert.equal(discFill(0), "none");
// Half left: the hand at the bottom, the sector running clockwise from it to the top, the short way round.
assert.equal(discFill(0.5).path, "M24 24 L24 45 A21 21 0 0 1 24 3 Z");
// Three quarters left: the hand at three o'clock, the long way round to the top.
assert.equal(discFill(0.75).path, "M24 24 L45 24 A21 21 0 1 1 24 3 Z");
// A quarter left: the hand at nine o'clock, a quarter to the top.
assert.equal(discFill(0.25).path, "M24 24 L3 24 A21 21 0 0 1 24 3 Z");

console.log("paper time: ok");
