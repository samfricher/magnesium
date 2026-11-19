import assert from "node:assert/strict";
import { registerHooks } from "node:module";

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
const { checkKicker, checksAlone, checksDueWords, courseFact, dueWords, firstQuestion, latestSkill, namedSessions, nextOf, openedDay, startsWithWords, planDay } = await import("../src/next.ts");

const item = (id, title, status, day = null, note = null) => ({ id, title, status, day, note });
const session = (session, title, more = {}) => ({ session, title, opening: "", first_ms: 1, last_ms: 1, turns: 2, current: false, opened_ms: 1, ...more });

const plan = [item("1", "Sampling distributions", "done"), item("2", "Confidence intervals", "todo", "2026-11-17", "z and t"), item("3", "Hypothesis tests", "todo")];

assert.deepEqual(nextOf(plan, []), { item: plan[1], carried: null });
const doing = [item("1", "A", "todo"), item("2", "B", "doing")];
assert.equal(nextOf(doing, [])?.item.id, "2");
const named = namedSessions([session("s1", "Sampling distributions"), session("s2", " Confidence intervals ")], null);
assert.equal(nextOf(plan, named)?.carried?.info.session, "s2");
assert.equal(nextOf(plan, namedSessions([session("s3", "Bayes questions")], null))?.carried, null, "a session off the plan is not the plan's");
assert.equal(nextOf([], []), null);
assert.equal(nextOf([item("1", "A", "done")], named), null);
const kept = namedSessions([session("s4", null, { current: true }), session("s5", null)], "Confidence intervals");
assert.equal(kept[0].title, "Confidence intervals");
assert.equal(nextOf(plan, kept)?.carried?.info.session, "s4");

const now = new Date(2026, 10, 16, 9);
assert.equal(planDay("2026-11-16", now), "today");
assert.equal(planDay("2026-11-17", now), "tomorrow");
assert.equal(planDay("2026-11-19", now), "Thu 19 Nov");
assert.equal(planDay(null, now), "");

const opens = (checks, redos, sitting = null) => ({ opening: { checks, redos }, sitting });
assert.equal(startsWithWords(opens(1, 0)), "Starts with 1 quick check, about a minute.");
assert.equal(startsWithWords(opens(3, 0)), "Starts with 3 quick checks, about 3 minutes.");
assert.equal(startsWithWords(opens(0, 2)), "Starts with 2 paper redos.");
assert.equal(startsWithWords(opens(1, 1)), "Starts with 1 quick check and 1 paper redo.");
assert.equal(startsWithWords(opens(0, 0)), null);
assert.equal(startsWithWords({ opening: null, sitting: null }), null, "a sitting carried on opens with nothing");
assert.equal(startsWithWords(opens(1, 0, { name: "Midterm 2023" })), null, "no tutor while a paper runs");
assert.equal(startsWithWords(null), null);
assert.equal(checksDueWords(1), "1 quick check due, about a minute.");
assert.equal(checksDueWords(2), "2 quick checks due, about 2 minutes.");
assert.equal(checksDueWords(0), null);
assert.equal(checkKicker(1), "A quick check, about a minute.");
assert.equal(checkKicker(3), "First of 3 quick checks, about 3 minutes.");

const paper = { target: "midterm-2", name: "Midterm 2023", path: null, minutes: 75, started_ms: 1 };
assert.equal(checksAlone({ checks_alone: 3, sitting: null }), 3);
assert.equal(checksAlone({ checks_alone: 0, sitting: null }), 0);
assert.equal(checksAlone({ checks_alone: 3, sitting: paper }), 0, "a paper is running");
assert.equal(checksAlone({ sitting: null }), 0, "an older runtime says nothing of them");
assert.equal(checksAlone({ checks_alone: null, sitting: null }), 0);
assert.equal(checksAlone(null), 0);


const due = (more = {}) => ({ checks_alone: 1, sitting: null, opening: { checks: 1, redos: 0 }, first_check: "What is $b$ in $2x^2 - 9x + 4$?", ...more });
assert.deepEqual(firstQuestion(due()), { text: "What is $b$ in $2x^2 - 9x + 4$?", meta: "A quick check, about a minute.", id: null });
assert.equal(firstQuestion(due({ first_check_id: "item-a" }))?.id, "item-a", "the record's ID for the check goes with it");
assert.equal(firstQuestion(due({ checks_alone: 3 }))?.meta, "First of 3 quick checks, about 3 minutes.");
assert.equal(firstQuestion(due({ first_check: null })), null, "with no stored question there is nothing to set");
assert.equal(firstQuestion(due({ first_check: "  " })), null);
assert.equal(firstQuestion(due({ opening: null })), null, "a sitting carried on does not open with the check");
assert.equal(firstQuestion(due({ sitting: { paper: "P" } })), null, "not while a timed paper runs");
assert.equal(firstQuestion(due({ checks_alone: null })), null, "not from a runtime that cannot pose checks alone");
assert.equal(firstQuestion(null), null);

const exam = (days_left) => ({ target: "t", title: "Midterm 2", date: null, day: null, days_left });

const { heldAnswerGoes } = await import("../src/render.ts");
const Q = "Two samples of 10 students have variances $s_1^2 = 4$ and $s_2^2 = 9$. What is their pooled variance?";
const held = (question = Q, id = null) => ({ question, id });
const after = (more) => ({ posed: undefined, task: null, taskUnseen: false, reply: "", ...more });
assert.equal(heldAnswerGoes(held(Q, "item-a"), after({ posed: "item-a", task: `Quick check 1: ${Q}` })), "answer", "the record says Home's check was posed, as Home asked it");
assert.equal(heldAnswerGoes(held(Q, "item-a"), after({ posed: "item-a", reply: `Quick check 1: ${Q}` })), "answer", "the page sees no task; the record and the reply say it is Home's");
assert.equal(
  heldAnswerGoes(held(Q, "item-a"), after({ posed: "item-a", task: "Quick check 1: Two samples of 8 have variances 1 and 4. What is their pooled variance?" })),
  "line",
  "Home's check, posed with a new problem the learner has not seen",
);
assert.equal(heldAnswerGoes(held(Q, "item-a"), after({ posed: "item-b", task: `Quick check 1: ${Q}` })), "line", "another check was posed, whatever its words");
assert.equal(heldAnswerGoes(held(Q, "item-a"), after({ posed: null, task: `Quick check 1: ${Q}` })), "line", "the record says none was posed");
assert.equal(heldAnswerGoes(held(), after({ task: `Quick check 1: ${Q}` })), "answer", "the open task is Home's question");
assert.equal(heldAnswerGoes(held(), after({ task: `Quick check 1:\n${Q.replace(/ /g, "  ")}` })), "answer", "spacing aside");
const otherNumbers = "Two samples of 10 students have variances $s_1^2 = 16$ and $s_2^2 = 25$. What is their pooled variance?";
assert.equal(heldAnswerGoes(held(), after({ task: `Quick check 1: ${otherNumbers}` })), "line", "the same template with other numbers is another check");
assert.equal(heldAnswerGoes(held(), after({ taskUnseen: true, reply: `Quick check 1: ${otherNumbers}` })), "line", "and in the reply, with the task unseen");
const derivative = "What is $\\frac{d}{dx}\\sin(x^2)$?";
const integral = "What is $\\int_0^1 x\\,dx$?";
assert.equal(heldAnswerGoes(held(derivative), after({ task: `Quick check 1: ${integral}` })), "line", "formula questions that share only \"what\" are different checks");
assert.equal(heldAnswerGoes(held(derivative), after({ taskUnseen: true, reply: `Quick check 1: ${integral}` })), "line");
assert.equal(heldAnswerGoes(held(), after({ task: "What is b in 2x² − 7x − 4 = 0?", reply: `Quick check 1: ${Q}` })), "line", "another task is open");
assert.equal(heldAnswerGoes(held(), after({ taskUnseen: true, reply: `Quick check 1: ${Q}` })), "unmarked", "the engine has not said; the reply posed Home's question");
assert.equal(heldAnswerGoes(held(), after({ taskUnseen: false, reply: `Quick check 1: ${Q}` })), "line", "the engine says no task is open");


const row = (name, last) => ({ dots: [0, 0, 0], name, what: "", note: "", due: false, next_day: null, last_answer: last });
const mark = (id, at) => ({ id, at, label: "", mark: "right", help: "none", cold: true, paper: null });
assert.equal(latestSkill([row("pooled variance", "a"), row("margin of error", "b")], [mark("a", "2026-11-16 09:00"), mark("b", "2026-11-17 08:00")]), "margin of error");
assert.equal(latestSkill([row("pooled variance", null)], []), null);

const day = new Date(2026, 10, 17, 12);
const at = (daysAgo) => new Date(2026, 10, 17 - daysAgo, 9).getTime();
assert.equal(openedDay(at(0), day), "today");
assert.equal(openedDay(at(1), day), "yesterday");
const course = (checks_due, days, can) => ({ checks_due, exam: days === null ? null : exam(days), can_count_today: can });
assert.equal(courseFact(course(2, 10, 1), at(1), false, day), "2 quick checks due", "another course leads with its checks");
assert.equal(courseFact(course(2, 10, 1), at(1), true, day), "Midterm 2 in 10 days", "the offered course leaves its checks to the cue");
assert.equal(courseFact({ ...course(2, 10, 1), sitting: { name: "Midterm 2023" } }, at(1), false, day), "Midterm 2 in 10 days", "no checks are said due while a paper runs there");
assert.equal(courseFact(course(0, 20, 3), at(1), false, day), "3 can count today", "an exam past two weeks gives way");
assert.equal(courseFact(course(0, null, 0), at(1), false, day), "studied yesterday");
assert.equal(courseFact(course(0, null, 0), null, false, day), "not studied yet");
assert.equal(courseFact(null, null, false, day), "not studied yet");

assert.equal(dueWords({ checks_due: 2, redos_due: 0 }), "2 quick checks due, about 2 minutes.");
assert.equal(dueWords({ checks_due: 1, redos_due: 1 }), "1 quick check and 1 paper redo due.");
assert.equal(dueWords({ checks_due: null, redos_due: 2 }), "2 paper redos due.");
assert.equal(dueWords({ checks_due: 0, redos_due: 0 }), null);
assert.equal(dueWords({ checks_due: 1, redos_due: 0, sitting: { name: "Midterm 2023" } }), null, "nothing is said due while a paper runs");

console.log("next checks passed");
