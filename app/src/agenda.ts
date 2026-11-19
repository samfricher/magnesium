import type { Exam, Row } from "./types";
import { fromToday, shortDay } from "./words";

export const earned = (row: Row) => row.dots.filter((dot) => dot === 2).length;

export type Again = { rank: 0 | 1 | 2 | 3 | 4 | 5; day: string };
export function againOf(row: Row, now = new Date()): Again {
  if (earned(row) >= 3) return { rank: 5, day: "" };
  if (row.due) return { rank: 0, day: "" };
  if (row.next_day && fromToday(row.next_day, now) === "past") return { rank: 1, day: "" };
  if (row.next_day) return { rank: 2, day: row.next_day };
  return { rank: row.needs_repair ? 3 : 4, day: "" };
}

export type Day<T> = { key: string; label: string; today: boolean; proven: boolean; exam: string | null; items: T[] };

export function agendaOf<T extends { row: Row; next: Again }>(work: T[], exam: Exam | null, now = new Date()): Day<T>[] {
  const ahead = exam?.date && exam.days_left !== null && exam.days_left >= 0 ? exam.date : null;
  const examKey = ahead ? `2-${ahead}~` : null;
  const repairKey = ahead && fromToday(ahead, now) !== "later" ? `${examKey}~` : "1";
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
    else if (rank === 3) add(repairKey, "To repair", item);
    else if (rank === 4) add("3", "No day yet", item);
    else add("4", "Proven", item, { proven: true });
  }
  if (ahead && examKey) days.push({ key: examKey, label: shortDay(ahead), today: false, proven: false, exam: exam!.title, items: [] });
  days.sort((a, b) => (a.key < b.key ? -1 : a.key > b.key ? 1 : 0));
  return days;
}
