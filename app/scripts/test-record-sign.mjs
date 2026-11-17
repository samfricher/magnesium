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
const { passDay } = await import("../src/words.ts");
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

// Your work as the days each skill can next count, on Tue 17 Nov 2026.
const now = new Date(2026, 10, 17, 10);
const row = (name, dots, due, next_day) => ({ name, dots, due, next_day, what: "", note: "" });
const rows = [
  row("pooled variance", [0, 0, 0], true, null),
  row("reading a confidence interval", [2, 0, 0], false, "2026-11-16"),
  row("degrees of freedom", [0, 0, 0], false, "2026-11-18"),
  row("t or normal", [2, 0, 0], false, "2026-11-18"),
  row("margin of error", [2, 0, 0], false, "2026-11-20"),
  row("a gap in a paper", [0, 0, 0], false, null),
  row("standard error of a mean", [2, 2, 2], false, null),
  row("after the exam", [2, 2, 0], false, "2026-12-01"),
];
const work = rows.map((each) => ({ row: each, next: againOf(each, now) })).sort((a, b) => a.next.rank - b.next.rank || a.next.day.localeCompare(b.next.day));
const exam = { target: "midterm-2", title: "Midterm 2", date: "2026-11-27", day: "Fri 27 Nov", days_left: 10 };
const said = (days) => days.map((day) => `${day.label}: ${day.exam ?? day.items.map((item) => item.row.name).join(", ")}`);
// Each day once, in order: today, those waiting for a repair, tomorrow, each later day, the exam on its date among
// them, proven. Above the exam, every skill is today, dated before it, or waiting for a repair that may come any day.
assert.deepEqual(said(agendaOf(work, exam, now)), [
  "Today: pooled variance, reading a confidence interval",
  "To repair: a gap in a paper",
  "Tomorrow: degrees of freedom, t or normal",
  "Fri 20 Nov: margin of error",
  "Fri 27 Nov: Midterm 2",
  "Tue 1 Dec: after the exam",
  "Proven: standard error of a mean",
]);
// A skill waiting for its repair is never listed after the exam, even with nothing today.
const dateless = [row("a gap in a paper", [0, 0, 0], false, null), row("after the exam", [2, 2, 0], false, "2026-12-01")].map((each) => ({ row: each, next: againOf(each, now) })).sort((a, b) => a.next.rank - b.next.rank || a.next.day.localeCompare(b.next.day));
assert.deepEqual(said(agendaOf(dateless, exam, now)), ["To repair: a gap in a paper", "Fri 27 Nov: Midterm 2", "Tue 1 Dec: after the exam"]);
const days = agendaOf(work, exam, now);
assert.equal(days[0].today, true, "today is the only day drawn in the ink");
assert.equal(days.filter((day) => day.today).length, 1);
assert.equal(days[days.length - 1].proven, true);
// The exam only while it is ahead: today counts, a day gone does not, nor an exam with no date.
assert.ok(said(agendaOf(work, { ...exam, date: "2026-11-17", days_left: 0 }, now)).includes("Tue 17 Nov: Midterm 2"));
assert.equal(said(agendaOf(work, { ...exam, date: "2026-11-16", days_left: -1 }, now)).some((line) => line.includes("Midterm")), false);
assert.equal(said(agendaOf(work, { ...exam, date: null, days_left: null }, now)).some((line) => line.includes("Midterm")), false);
// An exam on a day that holds skills comes after them; with no skills after it, it ends the dated days.
assert.deepEqual(said(agendaOf(work, { ...exam, date: "2026-11-18", days_left: 1 }, now)).slice(2, 4), ["Tomorrow: degrees of freedom, t or normal", "Wed 18 Nov: Midterm 2"]);
assert.deepEqual(said(agendaOf(work.slice(0, 2), exam, now)), ["Today: pooled variance, reading a confidence interval", "Fri 27 Nov: Midterm 2"]);
// No skills, no exam: nothing.
assert.deepEqual(agendaOf([], null, now), []);

console.log("record sign: ok");
