// What a workspace's Next is: the plan's item in hand, and the session that
// carries it on. Home's first row and the workspace page's Next both take it
// from here, so the two screens never offer different things to start.

import type { LessonInfo, PlanItem } from "./types";
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

/** The day the plan puts an item on, as the window says it: "today", "tomorrow", else "Tue 17 Nov". */
export function planDay(day: string | null, now = new Date()): string {
  if (!day) return "";
  const pad = (n: number) => String(n).padStart(2, "0");
  const iso = (date: Date) => `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
  if (day === iso(now)) return "today";
  if (day === iso(new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1))) return "tomorrow";
  return shortDay(day);
}
