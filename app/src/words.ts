// The window's own few words: what the record's marks say, what changed in a
// plan, and how a day is written. Nothing here reads the tutor's words.

import type { Mark, Plan, PlanItem } from "./types";

/** "Demo-Course" as "Demo Course". */
export function displayName(name: string): string {
  return name.replace(/[-_]+/g, " ").trim() || name;
}

const DAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const LONG_DAYS = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const LONG_MONTHS = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

function dateOf(day: string): Date | null {
  const found = /^(\d{4})-(\d{2})-(\d{2})/.exec(day);
  return found ? new Date(Number(found[1]), Number(found[2]) - 1, Number(found[3]), 12) : null;
}

/** "2026-11-24" as "Tue 24 Nov". */
export function shortDay(day: string): string {
  const date = dateOf(day);
  return date ? `${DAYS[date.getDay()]} ${date.getDate()} ${MONTHS[date.getMonth()]}` : day;
}

/** "2026-11-24" as "Tuesday 24 November". */
export function longDay(day: string): string {
  const date = dateOf(day);
  return date ? `${LONG_DAYS[date.getDay()]} ${date.getDate()} ${LONG_MONTHS[date.getMonth()]}` : day;
}

const HELP_WORDS: Record<string, string> = {
  hint: "with a hint",
  step: "with a step shown",
  solution: "after the worked solution",
};

/**
 * What the record's row says of an answer, in plain words. The verdict is the
 * record's and nothing else: the tutor's words are never read for it.
 */
export function markWords(mark: Mark): string {
  const help = HELP_WORDS[mark.help] ?? null;
  const on = mark.paper ? ` on ${mark.paper}` : "";
  switch (mark.mark) {
    case "right":
      if (help) return `Marked right${on}, ${help}.`;
      return mark.cold ? `Marked right${on}, on your own.` : `Marked right${on}.`;
    case "partly":
      return help ? `Marked partly right${on}, ${help}.` : `Marked partly right${on}.`;
    case "wrong":
      return `Not right yet${on}.`;
    case "declined":
      return "Left for another day.";
    default:
      return `Marked ${mark.mark}${on}.`;
  }
}

/**
 * One thing that changed in the plan, as a line under the tutor's reply.
 * `route` is a change to the lesson's goal or steps, which the tutor makes as
 * it teaches: the plan's layer shows it, and the line under the reply does not.
 */
export type Change = { what: string; detail: string; route?: boolean };

/** How an item moved, for the plan layer. */
export type Moved = "up" | "down" | "new" | "day";

export type PlanDiff = {
  changes: Change[];
  /** By item id. */
  moved: Record<string, Moved>;
  /** The exam's date before, when it moved. */
  examFrom: string | null;
};

const order = (items: PlanItem[]) => new Map(items.map((item, at) => [item.id, at]));

/**
 * What changed between two readings of the plan, read from the plan itself:
 * the exam's date, the order of its items, items added or dropped, the goal,
 * the route. A change the window cannot name says only that the plan changed.
 */
export function planDiff(before: Plan | null, after: Plan): PlanDiff {
  const changes: Change[] = [];
  const moved: Record<string, Moved> = {};
  let examFrom: string | null = null;
  if (!before) return { changes, moved, examFrom };
  const title = after.title ?? before.title ?? "The exam";
  if (before.exam_date !== after.exam_date) {
    if (before.exam_date && after.exam_date) {
      examFrom = before.exam_date;
      changes.push({ what: title, detail: `${shortDay(before.exam_date)} → ${shortDay(after.exam_date)}` });
    } else if (after.exam_date) changes.push({ what: title, detail: `now on ${shortDay(after.exam_date)}` });
    else changes.push({ what: title, detail: "no date now" });
  }
  const was = order(before.items);
  const now = order(after.items);
  const kept = after.items.filter((item) => was.has(item.id));
  // Order among the items both readings have: an item that is now ahead of
  // one it used to follow moved up.
  const keptBefore = [...kept].sort((a, b) => (was.get(a.id) ?? 0) - (was.get(b.id) ?? 0));
  kept.forEach((item, at) => {
    const from = keptBefore.findIndex((other) => other.id === item.id);
    if (from > at) moved[item.id] = "up";
    else if (from < at) moved[item.id] = "down";
  });
  for (const item of kept) {
    if (moved[item.id] !== "up") continue;
    const next = kept[kept.indexOf(item) + 1];
    changes.push({ what: item.title, detail: next ? `now before ${next.title}` : "moved up" });
  }
  // Days the blocks are on: a change that only re-spaces them shows too.
  const redayed = kept.filter((item) => {
    const old = before.items.find((each) => each.id === item.id);
    return old && (old.day ?? "") !== (item.day ?? "") && item.day;
  });
  for (const item of redayed) if (!moved[item.id]) moved[item.id] = "day";
  if (redayed.length >= 3) changes.push({ what: "The blocks", detail: `re-spaced from ${shortDay(redayed[0].day!)}` });
  else for (const item of redayed) changes.push({ what: item.title, detail: `now on ${shortDay(item.day!)}` });
  for (const item of after.items) {
    if (!was.has(item.id)) {
      moved[item.id] = "new";
      changes.push({ what: item.title, detail: "added" });
    }
  }
  for (const item of before.items) if (!now.has(item.id)) changes.push({ what: item.title, detail: "taken off" });
  if ((before.goal ?? "") !== (after.goal ?? "") && after.goal) changes.push({ what: "The goal", detail: after.goal, route: true });
  const route = (plan: Plan) => plan.steps.map((step) => step.step).join("\n");
  if (route(before) !== route(after) && after.steps.length > 0) changes.push({ what: "The route", detail: `${after.steps.length} steps`, route: true });
  if (changes.length === 0 && (before.text ?? "") !== (after.text ?? "")) changes.push({ what: "The plan file", detail: "rewritten" });
  return { changes, moved, examFrom };
}

/**
 * A task's words, short enough for one line: whole sentences while they fit,
 * else the last sentence (the question), never cut inside a formula.
 */
export function firstWords(text: string, limit = 90): string {
  const plain = text
    .replace(/^\s*\*\*[^*]{0,40}\*\*[.:]?\s*/, "")
    .replace(/\s+/g, " ")
    .trim();
  if (plain.length <= limit) return plain;
  const sentences = plain.split(/(?<=[.?!])\s+/).filter((each) => each !== "");
  const question = [...sentences].reverse().find((each) => each.endsWith("?"));
  if (question && question.length <= limit) return question;
  let cut = plain.slice(0, limit - 1);
  // A formula the cut ends inside of is left out whole.
  if ((cut.match(/(?<!\\)\$/g)?.length ?? 0) % 2 === 1) cut = cut.slice(0, cut.lastIndexOf("$"));
  return `${cut.trimEnd()}…`;
}

/** Where the learner stopped, as a sentence: the saved line, with its own punctuation. */
export function stoppedWords(line: string): string {
  const text = line.trim();
  return /[.?!]$/.test(text) ? `You stopped at: ${text}` : `You stopped at ${text}.`;
}
