// Checks for when a step's head is set into the lesson (src/stepHead.ts). Run by scripts/check.sh; node runs the
// TypeScript file as it is.
//
//   node scripts/test-step-head.mjs

import assert from "node:assert/strict";

const { drawHeadNow, stillHeld } = await import("../src/stepHead.ts");

const place = (over) => ({ folded: false, shown: true, replyTop: 300, viewTop: 60, ...over });

// The reply is in view: the head would push it down, so it waits.
assert.equal(drawHeadNow(place({})), false);
// A short lesson, its reply just under the heading: the same.
assert.equal(drawHeadNow(place({ replyTop: 200 })), false);
// The reply's top is scrolled above the view: the head goes in above it, out of sight.
assert.equal(drawHeadNow(place({ replyTop: 20 })), true);
assert.equal(drawHeadNow(place({ replyTop: 60 })), true);
// The lesson is hidden behind the task alone, or the tab is not in front: nothing is seen to move.
assert.equal(drawHeadNow(place({ folded: true })), true);
assert.equal(drawHeadNow(place({ shown: false })), true);
// No reply drawn: nothing to place it by, so it waits.
assert.equal(drawHeadNow(place({ replyTop: null })), false);

// Held for the turn on screen until the page folds or another message is sent.
assert.equal(stillHeld(1000, 1000, false), true);
assert.equal(stillHeld(1000, 1000, true), false);
assert.equal(stillHeld(1000, 2000, false), false);
assert.equal(stillHeld(null, 1000, false), false);

console.log("step heads: every case passed");
