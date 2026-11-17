// Checks for the record's sign of a skill's three days (src/words.ts): which day a mark earned, the only thing the
// rings beside a mark in a lesson are drawn from. Run by scripts/check.sh; node runs the TypeScript file as it is.
//
//   node scripts/test-record-sign.mjs

import assert from "node:assert/strict";
import { registerHooks } from "node:module";

// The page's modules import each other without the ".ts" that Vite adds and node does not.
registerHooks({
  resolve(name, context, next) {
    try {
      return next(name, context);
    } catch (error) {
      if (name.startsWith(".") && !name.endsWith(".ts")) return next(`${name}.ts`, context);
      throw error;
    }
  },
});
const { passDay } = await import("../src/words.ts");

const mark = (more = {}) => ({ id: "a-1", at: "2026-11-17 10:00", label: "standard error of a mean", mark: "right", help: "none", cold: true, paper: null, ...more });

// A pass: the day it earned, as the runtime counts it.
assert.equal(passDay(mark({ pass: 1, next_day: "2026-11-18" })), 1);
assert.equal(passDay(mark({ pass: 2, next_day: "2026-11-20" })), 2);
assert.equal(passDay(mark({ pass: 3 })), 3);
// No pass, no rings: right but not counted, helped, partly right, wrong, declined.
assert.equal(passDay(mark()), null, "a right answer that earned no day");
assert.equal(passDay(mark({ pass: null })), null);
assert.equal(passDay(mark({ help: "hint", pass: 1 })), null, "a helped answer keeps its words alone");
assert.equal(passDay(mark({ mark: "partly", pass: 1 })), null);
assert.equal(passDay(mark({ mark: "wrong", pass: 1 })), null);
assert.equal(passDay(mark({ mark: "declined", pass: 1 })), null);
// A count the window cannot draw as three days is not drawn.
assert.equal(passDay(mark({ pass: 4 })), null);
assert.equal(passDay(mark({ pass: 0 })), null);

console.log("record sign: ok");
