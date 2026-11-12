// The demo's half of the shared core capabilities: the plan, what a message
// with an intent may do, and the `plan_changed` and `marked` events. It is
// kept apart from demo.ts, which each concept rewrites for its own screens:
// demo.ts calls these, so every concept's demo behaves alike here.
//
// Everything here is made up. It mirrors what the window's core does:
// - talk is never marked, and the open task stays open;
// - "change the plan" moves the exam a week later and swaps two items, and
//   `plan_changed` follows the reply;
// - a mark is announced with `marked` once the record has its row.

import type { AgentEvent, Intent, Mark, Plan, PlanItem, PlanPart } from "./types";

type Emit = (event: AgentEvent) => void;

const isoDay = (ahead: number) => {
  const date = new Date();
  date.setDate(date.getDate() + ahead);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
};

const EXAM = "2026-11-17";
const MOVED = "2026-11-24";

function firstItems(): PlanItem[] {
  return [
    { id: "1", title: "Diagnostic paper", status: "done", day: isoDay(-6), note: "50 min, marked the same evening" },
    { id: "2", title: "Repair: reading $V_{max}$ and $K_m$", status: "doing", day: isoDay(0), note: "the y-intercept and the slope" },
    { id: "3", title: "Paper B", status: "todo", day: isoDay(1), note: "50 min, timed, cold" },
    { id: "4", title: "Cold redos of the gap log", status: "todo", day: isoDay(2), note: "from a blank page" },
    { id: "5", title: "Paper C", status: "todo", day: isoDay(4), note: "at exam level" },
  ];
}

function planText(date: string, items: PlanItem[]): string {
  const rows = items.map((item) => `| ${item.id} | ${item.title} | ${item.note ?? ""} | ${item.day ?? ""} |`).join("\n");
  return [
    "# Midterm 1: study plan",
    "",
    "## 1. Facts",
    "",
    "| Item | Value |",
    "|---|---|",
    "| Target | Midterm 1 |",
    `| When | ${date} 10:00 to 11:30 |`,
    "| Coverage | Weeks 1 to 5 |",
    "",
    "## 6. Blocks",
    "",
    "| # | Block | Purpose | Day |",
    "|---|---|---|---|",
    rows,
    "",
  ].join("\n");
}

let examDate = EXAM;
let items = firstItems();
let changedMs = Date.now() - 3 * 24 * 3600 * 1000;

/** The demo's plan, as `plan()` returns it. */
export function demoPlan(): Plan {
  return {
    target: "midterm-1",
    title: "Midterm 1",
    exam_date: examDate,
    goal: "PS3 Q3 · reading $V_{max}$ and $K_m$ from a Lineweaver–Burk plot",
    steps: [
      { step: "Write the rate equation", status: "done" },
      { step: "Take reciprocals of both sides", status: "done" },
      { step: "What the y-intercept gives", status: "doing" },
      { step: "What the slope gives", status: "todo" },
      { step: "Check the units", status: "todo" },
    ],
    items: items.map((item) => ({ ...item })),
    text: planText(examDate, items),
    path: "Study/midterm-1/plan.md",
    changed_ms: changedMs,
  };
}

/** Put the demo's plan back as it started. */
export function demoResetPlan(): void {
  examDate = EXAM;
  items = firstItems();
  changedMs = Date.now() - 3 * 24 * 3600 * 1000;
}

/**
 * The change the stand-in tutor makes when asked to change the plan: the exam
 * a week later, and the next two items swapped. Returns what changed. A
 * screen that opens straight on "after the change" (a URL hash) calls this.
 */
export function demoChangePlan(): PlanPart[] {
  examDate = examDate === EXAM ? MOVED : EXAM;
  const next = items.findIndex((item) => item.status !== "done");
  if (next >= 0 && next + 2 < items.length) {
    const swapped = [...items];
    [swapped[next + 1], swapped[next + 2]] = [swapped[next + 2], swapped[next + 1]];
    items = swapped;
  }
  changedMs = Date.now();
  return ["exam_date", "items", "text"];
}

const asksForPlanChange = (lower: string) => lower.includes("change the plan") || lower.includes("change my plan");
const isOffPlan = (lower: string) => lower.includes("by the way");
const isReturn = (lower: string) => lower.includes("back to");

/**
 * Whether the record would get a mark for this message. Talk never does, and
 * neither does a request to change the plan, an off-plan question or a
 * return to the task, whatever the intent.
 */
export function demoMayMark(text: string, intent?: Intent): boolean {
  if (intent === "talk") return false;
  const lower = text.toLowerCase();
  return !(asksForPlanChange(lower) || isOffPlan(lower) || isReturn(lower));
}

/**
 * The tutor's words for the shared scenarios, or null to let demo.ts answer.
 * The open task is the demo lesson's: what the y-intercept gives.
 */
export function demoBaseReply(text: string): string | null {
  const lower = text.toLowerCase();
  if (asksForPlanChange(lower)) {
    return (
      "Done. Midterm 1 is now on Tue 24 Nov, and Paper B moves after the cold redos so the redos come first.\n\n" +
      "Nothing is lost by the days off: the plan starts from where you are. Your open task is still here when you want it."
    );
  }
  if (isOffPlan(lower)) {
    return (
      "Good question, and it is not on this plan, so nothing here is marked. " +
      "A Hanes–Woolf plot puts $[S]/v$ against $[S]$; its slope is $1/V_{max}$ and its x-intercept is $-K_m$. " +
      "It spreads the points more evenly than Lineweaver–Burk, which is why some courses prefer it.\n\n" +
      "Say when you want to go back to the task."
    );
  }
  if (isReturn(lower)) {
    return "Back to it. You were on step 3.\n\n**Task.** The line meets the y-axis at $0.25$ s/µM. What is $V_{max}$, with its units?";
  }
  return null;
}

/**
 * Call when the reply to `text` has ended. A request to change the plan
 * changes it, and `plan_changed` is sent, as the core does after the turn.
 */
export function demoAfterTurn(text: string, emit: Emit): void {
  if (!asksForPlanChange(text.toLowerCase())) return;
  emit({ type: "plan_changed", what: demoChangePlan() });
}

/** Call when the demo's record gets a row for the last message: `marked` is sent. */
export function demoMarked(mark: Mark, emit: Emit): void {
  emit({ type: "marked", mark: { ...mark } });
}
