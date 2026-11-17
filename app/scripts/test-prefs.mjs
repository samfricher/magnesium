// Checks how the learner's settings are read back (src/prefs.ts). Run by
// scripts/check.sh; node runs the TypeScript file as it is.
//
//   node scripts/test-prefs.mjs

import assert from "node:assert/strict";
const { cleanPrefs, DEFAULTS } = await import("../src/prefs.ts");

// Each of the line's looks chosen in Settings is kept as it was chosen.
for (const line of ["bare", "ruled", "soft"]) assert.equal(cleanPrefs({ line }).line, line);
// A look this window does not know, or none, is the new window's look.
assert.equal(cleanPrefs({ line: "dotted" }).line, DEFAULTS.line);
assert.equal(cleanPrefs({}).line, DEFAULTS.line);
// The ruled line is a third choice: a new window still starts on the soft field.
assert.equal(DEFAULTS.line, "soft");

console.log("prefs: ok");
