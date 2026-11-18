// What a workspace's Next is: the plan's item in hand, and the session that
// carries it on. Home's first row and the workspace page's Next both take it
// from here, so the two screens never offer different things to start.

import type { Home, LessonInfo, Mark, PlanItem, Row } from "./types";
import { sessionName, shortDay, whenOpened } from "./words";

/** A session with the name the window shows for it. */
export type Named = { info: LessonInfo; title: string };

/** What Next offers: the plan's item, and the session named by it when there is one, which Return carries on. */
export type Next = { item: PlanItem; carried: Named | null };

/**
 * A folder's sessions as the window names them. A session kept by an earlier window has no name of its own, so the
 * current one takes the folder's last goal.
 */
export function namedSessions(sessions: LessonInfo[], goal: string | null): Named[] {
  return sessions.map((info) => ({ info, title: sessionName(info, info.current ? goal : null) }));
}

/**
 * The plan's item in hand, as the plan panel and a started session take it: the one it marks doing, else the first
 * not done. Its session is carried on when one is named by it; otherwise Return starts one. Null when there is no
 * plan, it could not be read, or every item is done.
 */
export function nextOf(items: PlanItem[], named: Named[]): Next | null {
  const item = items.find((each) => each.status === "doing") ?? items.find((each) => each.status !== "done") ?? null;
  if (!item) return null;
  return { item, carried: named.find((each) => each.title.trim() === item.title.trim()) ?? null };
}

/** "a minute", "3 minutes": how long a run of quick checks takes, a minute each. */
const checkMinutes = (checks: number) => (checks === 1 ? "a minute" : `${checks} minutes`);

/**
 * What the session the step starts or carries on opens with, from the record's reading, as a sentence of the cue under
 * it: "Starts with 1 quick check, about a minute." A quick check is a one-minute question; a paper redo has no set
 * length, so a sitting with one gives no time. Null when the next message opens with nothing: it carries a sitting on,
 * the opener is off, nothing is due, or a timed paper runs.
 */
export function startsWithWords(home: Pick<Home, "opening" | "sitting"> | null): string | null {
  const opening = home?.opening;
  if (!opening || home.sitting) return null;
  const { checks, redos } = opening;
  const count = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
  const parts = [checks > 0 ? count(checks, "quick check", "quick checks") : null, redos > 0 ? count(redos, "paper redo", "paper redos") : null].filter(
    (part): part is string => part !== null,
  );
  if (parts.length === 0) return null;
  return `Starts with ${parts.join(" and ")}${redos === 0 ? `, about ${checkMinutes(checks)}` : ""}.`;
}

/** Quick checks that can be taken alone, as a sentence of the cue: "2 quick checks due, about 2 minutes." Null for none. */
export function checksDueWords(checks: number): string | null {
  if (checks <= 0) return null;
  return `${checks} ${checks === 1 ? "quick check" : "quick checks"} due, about ${checkMinutes(checks)}.`;
}

/**
 * What is due, as a sentence of the cue, when the next message carries a sitting on and so opens with nothing: "2 quick
 * checks due, about 2 minutes." or "1 quick check and 1 paper redo due." Null when nothing is, and while a timed paper
 * runs: there is no tutor to take them until it is handed in.
 */
export function dueWords(home: Pick<Home, "checks_due" | "redos_due" | "sitting"> | null): string | null {
  if (home?.sitting) return null;
  const checks = home?.checks_due ?? 0;
  const redos = home?.redos_due ?? 0;
  if (redos === 0) return checksDueWords(checks);
  const count = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
  return `${[checks > 0 ? count(checks, "quick check", "quick checks") : null, count(redos, "paper redo", "paper redos")].filter(Boolean).join(" and ")} due.`;
}

/** The words over a quick check's question on Home: "A quick check, about a minute." or "First of 3 quick checks, about 3 minutes." */
export function checkKicker(checks: number): string {
  return checks <= 1 ? "A quick check, about a minute." : `First of ${checks} quick checks, about ${checks} minutes.`;
}

/**
 * How many quick checks the control "Just the quick checks" would take: none while a timed paper runs (no tutor until
 * it is handed in), and none from a runtime that cannot run a sitting of checks alone, which says nothing of them.
 */
export function checksAlone(home: Pick<Home, "checks_alone" | "sitting"> | null): number {
  if (!home || home.sitting) return 0;
  return home.checks_alone ?? 0;
}

/** The day the plan puts an item on, as the window says it: "today", "tomorrow", else "Tue 17 Nov". */
export function planDay(day: string | null, now = new Date()): string {
  if (!day) return "";
  const pad = (n: number) => String(n).padStart(2, "0");
  const iso = (date: Date) => `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
  if (day === iso(now)) return "today";
  if (day === iso(new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1))) return "tomorrow";
  return shortDay(day);
}

/**
 * The question Home sets first: the quick check a sitting of checks alone would pose first, as the record stores it,
 * with the words over it. Null while a timed paper runs, when no check can be taken alone, or when the record holds
 * no question for the first.
 */
export function firstQuestion(
  home: Pick<Home, "first_check" | "first_check_id" | "checks_alone" | "sitting" | "opening"> | null,
): { text: string; meta: string; id: string | null } | null {
  const checks = checksAlone(home);
  const text = home?.first_check?.trim() ?? "";
  if (checks === 0 || text === "" || !home?.opening?.checks) return null;
  return { text, meta: checkKicker(checks), id: home?.first_check_id ?? null };
}

/**
 * The skill of the record's latest marked answer, for "Last time: …": the row whose newest answer is the newest of
 * all. Null when the record holds no marked answer. Never read from the tutor's words.
 */
export function latestSkill(rows: Row[], marks: Mark[]): string | null {
  const byId = new Map(marks.map((mark) => [mark.id, mark]));
  let best: string | null = null;
  let at = "";
  for (const row of rows) {
    const mark = row.last_answer ? byId.get(row.last_answer) : undefined;
    if (mark && mark.at > at) {
      at = mark.at;
      best = row.name;
    }
  }
  return best;
}

/** A day a session was opened, as a course's row says it: "today", "yesterday", "Monday", else "Wed 11 Nov". */
export function openedDay(ms: number | null | undefined, now = new Date()): string {
  return whenOpened(ms, now).replace(/, \d\d:\d\d$/, "");
}

/**
 * The one fact a course's row on Home says, from what the record says of its day. The course whose step Home is
 * offering leaves out its quick checks, which the cue above already says; any other course leads with them, unless a
 * timed paper runs there and nothing can be taken until it is handed in. Then an exam within two weeks, the skills
 * that can count today, the day it was last studied, or that it never was.
 */
export function courseFact(
  home: Pick<Home, "checks_due" | "exam" | "can_count_today"> & Partial<Pick<Home, "sitting">> | null,
  openedMs: number | null | undefined,
  offered: boolean,
  now = new Date(),
): string {
  const checks = home?.sitting ? 0 : (home?.checks_due ?? 0);
  if (!offered && checks > 0) return `${checks} ${checks === 1 ? "quick check" : "quick checks"} due`;
  const left = home?.exam?.days_left ?? null;
  if (home?.exam && left !== null && left >= 0 && left <= 14) return `${home.exam.title} ${left === 0 ? "today" : left === 1 ? "tomorrow" : `in ${left} days`}`;
  const count = home?.can_count_today ?? 0;
  if (count > 0) return `${count} can count today`;
  return openedMs ? `studied ${openedDay(openedMs, now)}` : "not studied yet";
}
