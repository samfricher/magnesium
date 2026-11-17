// What a workspace's Next is: the plan's item in hand, and the session that
// carries it on. Home's first row and the workspace page's Next both take it
// from here, so the two screens never offer different things to start.

import type { Home, LessonInfo, PlanItem } from "./types";
import { sessionName, shortDay } from "./words";

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

/**
 * What the session Next starts or carries on opens with, from the record's reading, as the row says it: "Starts with
 * 1 quick check · about a minute". A quick check is a one-minute question; a paper redo has no set length, so none is
 * said with one. Null when the next message opens with nothing: it carries a sitting on, the opener is off, nothing is
 * due, or a timed paper is running.
 */
export function firstStep(home: Pick<Home, "opening" | "sitting"> | null): string | null {
  const opening = home?.opening;
  if (!opening || home.sitting) return null;
  const { checks, redos } = opening;
  const count = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
  const parts = [checks > 0 ? count(checks, "quick check", "quick checks") : null, redos > 0 ? count(redos, "paper redo", "paper redos") : null].filter(
    (part): part is string => part !== null,
  );
  if (parts.length === 0) return null;
  const time = redos === 0 ? ` · about ${checks === 1 ? "a minute" : `${checks} minutes`}` : "";
  return `Starts with ${parts.join(" and ")}${time}`;
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
export function firstQuestion(home: Pick<Home, "first_check" | "checks_alone" | "sitting" | "opening"> | null): { text: string; meta: string } | null {
  const checks = checksAlone(home);
  const text = home?.first_check?.trim() ?? "";
  if (checks === 0 || text === "" || !home?.opening?.checks) return null;
  const meta = checks === 1 ? "A quick check · about a minute" : `First of ${checks} quick checks · about ${checks} minutes`;
  return { text, meta };
}

/** What a workspace's row on Home says of it: "2 can count today · Midterm 2 in 10 days", each part only when it applies. */
export function dayFacts(home: Pick<Home, "can_count_today" | "exam"> | null): string {
  if (!home) return "";
  const count = home.can_count_today ?? 0;
  const left = home.exam?.days_left ?? null;
  const exam =
    home.exam && left !== null && left >= 0 ? `${home.exam.title} ${left === 0 ? "today" : left === 1 ? "tomorrow" : `in ${left} days`}` : null;
  return [count > 0 ? `${count} can count today` : null, exam].filter(Boolean).join(" · ");
}
