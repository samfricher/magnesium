import type { Home, LessonInfo, Mark, PlanItem, Row } from "./types";
import { sessionName, shortDay, whenOpened } from "./words";

export type Named = { info: LessonInfo; title: string };

export type Next = { item: PlanItem; carried: Named | null };

export function namedSessions(sessions: LessonInfo[], goal: string | null): Named[] {
  return sessions.map((info) => ({ info, title: sessionName(info, info.current ? goal : null) }));
}

export function nextOf(items: PlanItem[], named: Named[]): Next | null {
  const item = items.find((each) => each.status === "doing") ?? items.find((each) => each.status !== "done") ?? null;
  if (!item) return null;
  return { item, carried: named.find((each) => each.title.trim() === item.title.trim()) ?? null };
}

const checkMinutes = (checks: number) => (checks === 1 ? "a minute" : `${checks} minutes`);

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

export function checksDueWords(checks: number): string | null {
  if (checks <= 0) return null;
  return `${checks} ${checks === 1 ? "quick check" : "quick checks"} due, about ${checkMinutes(checks)}.`;
}

export function dueWords(home: Pick<Home, "checks_due" | "redos_due" | "sitting"> | null): string | null {
  if (home?.sitting) return null;
  const checks = home?.checks_due ?? 0;
  const redos = home?.redos_due ?? 0;
  if (redos === 0) return checksDueWords(checks);
  const count = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
  return `${[checks > 0 ? count(checks, "quick check", "quick checks") : null, count(redos, "paper redo", "paper redos")].filter(Boolean).join(" and ")} due.`;
}

export function checkKicker(checks: number): string {
  return checks <= 1 ? "A quick check, about a minute." : `First of ${checks} quick checks, about ${checks} minutes.`;
}

export function checksAlone(home: Pick<Home, "checks_alone" | "sitting"> | null): number {
  if (!home || home.sitting) return 0;
  return home.checks_alone ?? 0;
}

export function planDay(day: string | null, now = new Date()): string {
  if (!day) return "";
  const pad = (n: number) => String(n).padStart(2, "0");
  const iso = (date: Date) => `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
  if (day === iso(now)) return "today";
  if (day === iso(new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1))) return "tomorrow";
  return shortDay(day);
}

export function firstQuestion(
  home: Pick<Home, "first_check" | "first_check_id" | "checks_alone" | "sitting" | "opening"> | null,
): { text: string; meta: string; id: string | null } | null {
  const checks = checksAlone(home);
  const text = home?.first_check?.trim() ?? "";
  if (checks === 0 || text === "" || !home?.opening?.checks) return null;
  return { text, meta: checkKicker(checks), id: home?.first_check_id ?? null };
}

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

export function openedDay(ms: number | null | undefined, now = new Date()): string {
  return whenOpened(ms, now).replace(/, \d\d:\d\d$/, "");
}

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
