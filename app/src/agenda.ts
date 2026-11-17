// Your work as the days each skill can next count: the runtime's own next day for each skill, said once per day, with
// the plan's exam set among them on its date. Nothing here works out a day the record does not already hold.

import type { Exam, Row } from "./types";
import { fromToday, shortDay } from "./words";

/** The passes a skill holds now, as the runtime counts them: a 2 in its dots. */
export const earned = (row: Row) => row.dots.filter((dot) => dot === 2).length;

/**
 * Where a skill stands among the days: 0 a check or redo the record asks for today, 1 a skill that can count today,
 * 2 a later day (`day`), 3 no day yet (a paper question waiting for its repair), 4 proven.
 */
export type Again = { rank: 0 | 1 | 2 | 3 | 4; day: string };
export function againOf(row: Row, now = new Date()): Again {
  if (earned(row) >= 3) return { rank: 4, day: "" };
  if (row.due) return { rank: 0, day: "" };
  if (row.next_day && fromToday(row.next_day, now) === "past") return { rank: 1, day: "" };
  if (!row.next_day) return { rank: 3, day: "" };
  return { rank: 2, day: row.next_day };
}

/** One day of the agenda: its skills, or the exam on its own row. */
export type Day<T> = { key: string; label: string; today: boolean; proven: boolean; exam: string | null; items: T[] };

/**
 * The skills grouped by the day each can next count, in the order given (already ranked by `againOf`): "Today", then
 * "To repair", then "Tomorrow" and each later day ("Fri 20 Nov"), then "Proven". The exam, while it is ahead, is a
 * day of its own on its date, after that date's skills.
 */
export function agendaOf<T extends { row: Row; next: Again }>(work: T[], exam: Exam | null, now = new Date()): Day<T>[] {
  const days: Day<T>[] = [];
  const add = (key: string, label: string, item: T, extra: Partial<Day<T>> = {}) => {
    let day = days.find((each) => each.key === key);
    if (!day) days.push((day = { key, label, today: false, proven: false, exam: null, items: [], ...extra }));
    day.items.push(item);
  };
  for (const item of work) {
    const { rank, day } = item.next;
    if (rank <= 1) add("0", "Today", item, { today: true });
    else if (rank === 2) add(`2-${day}`, fromToday(day, now) === "tomorrow" ? "Tomorrow" : shortDay(day), item);
    // A skill with no day waits for its repair, which a lesson can give it any day: it could count well before the
    // exam, so it is kept above the dated days rather than after them, where it would read as "not before the exam".
    else if (rank === 3) add("1", "To repair", item);
    else add("4", "Proven", item, { proven: true });
  }
  days.sort((a, b) => (a.key < b.key ? -1 : a.key > b.key ? 1 : 0));
  if (exam?.date && exam.days_left !== null && exam.days_left >= 0) {
    // After the skills of its own day ("~" sorts after the date), before the next day's.
    const key = `2-${exam.date}~`;
    const at = days.findIndex((day) => day.key > key);
    const row: Day<T> = { key, label: shortDay(exam.date), today: false, proven: false, exam: exam.title, items: [] };
    if (at === -1) days.push(row);
    else days.splice(at, 0, row);
  }
  return days;
}
