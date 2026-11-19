import assert from "node:assert/strict";
const { cleanPrefs, DEFAULTS } = await import("../src/prefs.ts");

for (const line of ["bare", "ruled", "soft"]) assert.equal(cleanPrefs({ line }).line, line);
assert.equal(cleanPrefs({ line: "dotted" }).line, DEFAULTS.line);
assert.equal(cleanPrefs({}).line, DEFAULTS.line);

console.log("prefs: ok");
