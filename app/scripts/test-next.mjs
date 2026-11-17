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
const { checksAlone, dayFacts, firstQuestion, firstStep, namedSessions, nextOf, planDay } = await import("../src/next.ts");

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

// Just the quick checks: offered from what the runtime says it would pose, never while a paper runs, and never by a
// runtime that cannot run them alone.
const paper = { target: "midterm-2", name: "Midterm 2023", path: null, minutes: 75, started_ms: 1 };
assert.equal(checksAlone({ checks_alone: 3, sitting: null }), 3);
assert.equal(checksAlone({ checks_alone: 0, sitting: null }), 0);
assert.equal(checksAlone({ checks_alone: 3, sitting: paper }), 0, "a paper is running");
assert.equal(checksAlone({ sitting: null }), 0, "an older runtime says nothing of them");
assert.equal(checksAlone({ checks_alone: null, sitting: null }), 0);
assert.equal(checksAlone(null), 0);


// Home's first question: the record's stored question for the check a sitting of checks alone poses first.
const due = (more = {}) => ({ checks_alone: 1, sitting: null, opening: { checks: 1, redos: 0 }, first_check: "What is $b$ in $2x^2 - 9x + 4$?", ...more });
assert.deepEqual(firstQuestion(due()), { text: "What is $b$ in $2x^2 - 9x + 4$?", meta: "A quick check · about a minute" });
assert.equal(firstQuestion(due({ checks_alone: 3 }))?.meta, "First of 3 quick checks · about 3 minutes");
assert.equal(firstQuestion(due({ first_check: null })), null, "with no stored question there is nothing to set");
assert.equal(firstQuestion(due({ first_check: "  " })), null);
assert.equal(firstQuestion(due({ opening: null })), null, "a sitting carried on does not open with the check");
assert.equal(firstQuestion(due({ sitting: { paper: "P" } })), null, "not while a timed paper runs");
assert.equal(firstQuestion(due({ checks_alone: null })), null, "not from a runtime that cannot pose checks alone");
assert.equal(firstQuestion(null), null);

// What a workspace's row says of the day.
const exam = (days_left) => ({ target: "t", title: "Midterm 2", date: null, day: null, days_left });
assert.equal(dayFacts({ can_count_today: 2, exam: exam(10) }), "2 can count today · Midterm 2 in 10 days");
assert.equal(dayFacts({ can_count_today: 0, exam: exam(1) }), "Midterm 2 tomorrow");
assert.equal(dayFacts({ can_count_today: 1, exam: exam(0) }), "1 can count today · Midterm 2 today");
assert.equal(dayFacts({ can_count_today: 3, exam: exam(-2) }), "3 can count today", "an exam past is not named");
assert.equal(dayFacts({ can_count_today: null, exam: null }), "");
assert.equal(dayFacts(null), "");

// Where Home's held answer goes once the posing reply has ended: only to the check Home showed.
const { heldAnswerGoes } = await import("../src/render.ts");
const Q = "Two samples of 10 students have variances $s_1^2 = 4$ and $s_2^2 = 9$. What is their pooled variance?";
assert.equal(heldAnswerGoes(Q, { task: `Quick check 1: ${Q}`, taskUnseen: false, reply: "" }), "answer", "the open task is Home's question");
assert.equal(heldAnswerGoes(Q, { task: "What is b in 2x² − 7x − 4 = 0?", taskUnseen: false, reply: `Quick check 1: ${Q}` }), "line", "another check is open: Home's words wait on the line");
assert.equal(heldAnswerGoes(Q, { task: null, taskUnseen: true, reply: `Quick check 1: ${Q}` }), "unmarked", "the engine has not said; the reply posed Home's question");
assert.equal(heldAnswerGoes(Q, { task: null, taskUnseen: true, reply: "Quick check 1: What is b in 2x² − 7x − 4 = 0?" }), "line", "the reply posed another question");
assert.equal(heldAnswerGoes(Q, { task: null, taskUnseen: false, reply: `Quick check 1: ${Q}` }), "line", "the engine says no task is open");

console.log("next checks passed");
