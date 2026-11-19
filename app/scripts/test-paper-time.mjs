import assert from "node:assert/strict";

const { discFill, shareLeft } = await import("../src/paperTime.ts");

assert.equal(shareLeft(53, 75), 53 / 75);
assert.equal(shareLeft(75, 75), 1);
assert.equal(shareLeft(0, 75), 0);
assert.equal(shareLeft(80, 75), 1);
assert.equal(shareLeft(-3, 75), 0);
assert.equal(shareLeft(10, 0), 0);

assert.equal(discFill(1), "whole");
assert.equal(discFill(0), "none");
assert.equal(discFill(0.5).path, "M24 24 L24 45 A21 21 0 0 1 24 3 Z");
assert.equal(discFill(0.75).path, "M24 24 L45 24 A21 21 0 1 1 24 3 Z");
assert.equal(discFill(0.25).path, "M24 24 L3 24 A21 21 0 0 1 24 3 Z");

console.log("paper time: ok");
