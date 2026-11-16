// Checks for what Next offers (src/next.ts), the rule Home's first row and a
// workspace's page share. Run by scripts/check.sh; node runs the TypeScript
// file as it is.
//
//   node scripts/test-next.mjs

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
const { firstStep, namedSessions, nextOf, planDay } = await import("../src/next.ts");

const item = (id, title, status, day = null, note = null) => ({ id, title, status, day, note });
const session = (session, title, more = {}) => ({ session, title, opening: "", first_ms: 1, last_ms: 1, turns: 2, current: false, opened_ms: 1, ...more });

const plan = [item("1", "Sampling distributions", "done"), item("2", "Confidence intervals", "todo", "2026-11-17", "z and t"), item("3", "Hypothesis tests", "todo")];

// The first item not done, with nothing to carry on: Return starts a session.
assert.deepEqual(nextOf(plan, []), { item: plan[1], carried: null });
// The item the plan marks doing comes before an earlier one not done.
const doing = [item("1", "A", "todo"), item("2", "B", "doing")];
assert.equal(nextOf(doing, [])?.item.id, "2");
// A session named by the item is carried on; one on something else is not.
const named = namedSessions([session("s1", "Sampling distributions"), session("s2", " Confidence intervals ")], null);
assert.equal(nextOf(plan, named)?.carried?.info.session, "s2");
assert.equal(nextOf(plan, namedSessions([session("s3", "Bayes questions")], null))?.carried, null, "a session off the plan is not the plan's");
// No plan, a plan that could not be read (read as empty), and every item done: nothing to offer.
assert.equal(nextOf([], []), null);
assert.equal(nextOf([item("1", "A", "done")], named), null);
// A session an earlier window kept has no name of its own: the current one takes the folder's last goal.
const kept = namedSessions([session("s4", null, { current: true }), session("s5", null)], "Confidence intervals");
assert.equal(kept[0].title, "Confidence intervals");
assert.equal(nextOf(plan, kept)?.carried?.info.session, "s4");

// The day, as the window says it.
const now = new Date(2026, 10, 16, 9);
assert.equal(planDay("2026-11-16", now), "today");
assert.equal(planDay("2026-11-17", now), "tomorrow");
assert.equal(planDay("2026-11-19", now), "Thu 19 Nov");
assert.equal(planDay(null, now), "");

// What the session opens with, from the record's reading: nothing unless the next message begins a sitting.
const opens = (checks, redos, sitting = null) => ({ opening: { checks, redos }, sitting });
assert.equal(firstStep(opens(1, 0)), "Starts with 1 quick check · about a minute");
assert.equal(firstStep(opens(3, 0)), "Starts with 3 quick checks · about 3 minutes");
assert.equal(firstStep(opens(0, 2)), "Starts with 2 paper redos");
assert.equal(firstStep(opens(1, 1)), "Starts with 1 quick check and 1 paper redo");
assert.equal(firstStep(opens(0, 0)), null);
assert.equal(firstStep({ opening: null, sitting: null }), null, "a sitting carried on opens with nothing");
assert.equal(firstStep(opens(1, 0, { name: "Midterm 2023" })), null, "no tutor while a paper runs");
assert.equal(firstStep(null), null);

console.log("next checks passed");
