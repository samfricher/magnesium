// Checks for the record's sign of a skill's three days: which day a mark earned (src/words.ts), the only thing the
// rings beside a mark in a lesson are drawn from, and Your work as the days each skill can next count
// (src/agenda.ts). Run by scripts/check.sh; node runs the TypeScript files as they are.
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
const { passDay, passDaySpoken, passWords } = await import("../src/words.ts");
const { agendaOf, againOf } = await import("../src/agenda.ts");

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

// The line under a pass says when the next day can count; the day itself is in the rings, and said to a screen
// reader, which does not see them. A day with no next says which it was, on screen and to a screen reader alike.
const today = new Date(2026, 10, 17, 12);
assert.equal(passWords(mark({ pass: 1, next_day: "2026-11-18" }), today), "The second day can count from tomorrow.");
assert.equal(passDaySpoken(mark({ pass: 1, next_day: "2026-11-18" })), "First of three days.");
assert.equal(passWords(mark({ pass: 2, next_day: "2026-11-20" }), today), "The third day can count from Fri 20 Nov.");
assert.equal(passDaySpoken(mark({ pass: 2, next_day: "2026-11-20" })), "Second of three days.");
assert.equal(passWords(mark({ pass: 3 }), today), "Third of three days.");
assert.equal(passDaySpoken(mark({ pass: 3 })), null, "the third day is said once");
assert.equal(passWords(mark({ pass: 1 }), today), "First of three days.", "a pass with no next day says which it was");
assert.equal(passDaySpoken(mark({ pass: 1 })), null);
assert.equal(passDaySpoken(mark({ help: "hint", pass: 1, next_day: "2026-11-18" })), null, "no day earned, nothing said");

// Your work as the days each skill can next count, on Tue 17 Nov 2026.
const now = new Date(2026, 10, 17, 10);
const row = (name, dots, due, next_day, needs_repair = false) => ({ name, dots, due, next_day, needs_repair, what: "", note: "" });
const rows = [
  row("pooled variance", [0, 0, 0], true, null),
  row("reading a confidence interval", [2, 0, 0], false, "2026-11-16"),
  row("degrees of freedom", [0, 0, 0], false, "2026-11-18"),
  row("t or normal", [2, 0, 0], false, "2026-11-18"),
  row("margin of error", [2, 0, 0], false, "2026-11-20"),
  row("a gap in a paper", [0, 0, 0], false, null, true),
  row("a repaired gap with no redo date", [0, 0, 0], false, null),
  row("standard error of a mean", [2, 2, 2], false, null),
  row("after the exam", [2, 2, 0], false, "2026-12-01"),
];
const ranked = (list) => list.map((each) => ({ row: each, next: againOf(each, now) })).sort((a, b) => a.next.rank - b.next.rank || a.next.day.localeCompare(b.next.day));
const work = ranked(rows);
const exam = { target: "midterm-2", title: "Midterm 2", date: "2026-11-27", day: "Fri 27 Nov", days_left: 10 };
const said = (days) => days.map((day) => `${day.label}: ${day.exam ?? day.items.map((item) => item.row.name).join(", ")}`);
// Each day once, in order: today, those the runtime says wait for their repair, tomorrow, each later day, the exam on
// its date among them, those with no day for another reason, proven. Above the exam, every skill is today, dated
// before it, or waiting for a repair that may come any day.
assert.deepEqual(said(agendaOf(work, exam, now)), [
  "Today: pooled variance, reading a confidence interval",
  "To repair: a gap in a paper",
  "Tomorrow: degrees of freedom, t or normal",
  "Fri 20 Nov: margin of error",
  "Fri 27 Nov: Midterm 2",
  "Tue 1 Dec: after the exam",
  "No day yet: a repaired gap with no redo date",
  "Proven: standard error of a mean",
]);
// "To repair" only when the runtime says so: a skill with no day and nothing to repair is not one, nor is an older
// core's row that does not say.
assert.equal(againOf(row("x", [0, 0, 0], false, null), now).rank, 4);
assert.equal(againOf({ name: "x", dots: [0, 0, 0], due: false, next_day: null, what: "", note: "" }, now).rank, 4);
assert.equal(againOf(row("x", [0, 0, 0], false, null, true), now).rank, 3);
// A skill waiting for its repair is listed above the exam, even with nothing today.
const dateless = ranked([row("a gap in a paper", [0, 0, 0], false, null, true), row("after the exam", [2, 2, 0], false, "2026-12-01")]);
assert.deepEqual(said(agendaOf(dateless, exam, now)), ["To repair: a gap in a paper", "Fri 27 Nov: Midterm 2", "Tue 1 Dec: after the exam"]);
// With the exam today or tomorrow, a skill waiting for its repair cannot count before it (a pass needs a later day
// than its repair), so it is listed after the exam's row.
assert.deepEqual(said(agendaOf(dateless, { ...exam, date: "2026-11-18", days_left: 1 }, now)), ["Wed 18 Nov: Midterm 2", "To repair: a gap in a paper", "Tue 1 Dec: after the exam"]);
assert.deepEqual(said(agendaOf(dateless, { ...exam, date: "2026-11-17", days_left: 0 }, now)), ["Tue 17 Nov: Midterm 2", "To repair: a gap in a paper", "Tue 1 Dec: after the exam"]);
assert.deepEqual(said(agendaOf(work, { ...exam, date: "2026-11-18", days_left: 1 }, now)).slice(0, 4), [
  "Today: pooled variance, reading a confidence interval",
  "Tomorrow: degrees of freedom, t or normal",
  "Wed 18 Nov: Midterm 2",
  "To repair: a gap in a paper",
]);
// Two days ahead, a repair today can still count tomorrow, before the exam: above it again.
assert.deepEqual(said(agendaOf(dateless, { ...exam, date: "2026-11-19", days_left: 2 }, now)).slice(0, 2), ["To repair: a gap in a paper", "Thu 19 Nov: Midterm 2"]);
const days = agendaOf(work, exam, now);
assert.equal(days[0].today, true, "today is the only day drawn in the ink");
assert.equal(days.filter((day) => day.today).length, 1);
assert.equal(days[days.length - 1].proven, true);
// The exam only while it is ahead: today counts, a day gone does not, nor an exam with no date.
assert.ok(said(agendaOf(work, { ...exam, date: "2026-11-17", days_left: 0 }, now)).includes("Tue 17 Nov: Midterm 2"));
assert.equal(said(agendaOf(work, { ...exam, date: "2026-11-16", days_left: -1 }, now)).some((line) => line.includes("Midterm")), false);
assert.equal(said(agendaOf(work, { ...exam, date: null, days_left: null }, now)).some((line) => line.includes("Midterm")), false);
// With no exam ahead, the order holds without it.
assert.deepEqual(said(agendaOf(dateless, null, now)), ["To repair: a gap in a paper", "Tue 1 Dec: after the exam"]);
// An exam on a day that holds skills comes after them; with no skills after it, it ends the dated days.
assert.deepEqual(said(agendaOf(work.slice(0, 2), exam, now)), ["Today: pooled variance, reading a confidence interval", "Fri 27 Nov: Midterm 2"]);
// No skills, no exam: nothing.
assert.deepEqual(agendaOf([], null, now), []);

console.log("record sign: ok");
