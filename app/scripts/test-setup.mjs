import assert from "node:assert/strict";
import { setupReady } from "../src/setup.ts";

const program = (version) => ({ shown_path: "~/.local/bin/magnesium", version });
const account = { name: "Claude Code", installed: true, signed_in: true, paid_with: "subscription", detail: null, problem: null, install_page: null };
const setup = (state, more = {}) => ({
  runtime: { state, installed: program("0.2.0"), bundled: program("0.3.0"), installed_now: false, message: null },
  account: { ...account, ...more },
  payment: "subscription",
  permissions: "auto",
});

assert.equal(setupReady(null), false, "before the look at this Mac is done");
assert.equal(setupReady(setup("ready")), true);
assert.equal(setupReady(setup("kept")), true);
assert.equal(setupReady(setup("different")), true, "a lesson runs on the installed copy while the choice is open");
for (const state of ["missing", "too_old", "failed"]) assert.equal(setupReady(setup(state)), false, state);
assert.equal(setupReady(setup("different", { signed_in: false })), false, "signed out");
assert.equal(setupReady(setup("ready", { installed: false })), false, "no Claude Code");

console.log("setup checks passed");
