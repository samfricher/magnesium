import assert from "node:assert/strict";

const { drawHeadNow, stillHeld } = await import("../src/stepHead.ts");

const place = (over) => ({ folded: false, shown: true, replyTop: 300, viewTop: 60, ...over });

assert.equal(drawHeadNow(place({})), false);
assert.equal(drawHeadNow(place({ replyTop: 200 })), false);
assert.equal(drawHeadNow(place({ replyTop: 20 })), true);
assert.equal(drawHeadNow(place({ replyTop: 60 })), true);
assert.equal(drawHeadNow(place({ folded: true })), true);
assert.equal(drawHeadNow(place({ shown: false })), true);
assert.equal(drawHeadNow(place({ replyTop: null })), false);

assert.equal(stillHeld(1000, 1000, false), true);
assert.equal(stillHeld(1000, 1000, true), false);
assert.equal(stillHeld(1000, 2000, false), false);
assert.equal(stillHeld(null, 1000, false), false);

console.log("step heads: every case passed");
