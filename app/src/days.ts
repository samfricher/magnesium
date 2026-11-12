// Days and times as the log writes them. Every day is a local day, the way
// the learner lives it, written "2026-11-12" as the record writes it.

const WEEKDAYS = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

const pad = (n: number) => String(n).padStart(2, "0");

/** The local day of a moment, "2026-11-12". */
export function dayOf(ms: number | Date): string {
  const date = ms instanceof Date ? ms : new Date(ms);
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

/** Today, "2026-11-12". */
export const today = () => dayOf(Date.now());

/** Noon on a day written "2026-11-12", as a Date: far from any midnight, whatever the clocks do. */
export function dateOf(day: string): Date {
  const [y, m, d] = day.split("-").map(Number);
  return new Date(y, (m ?? 1) - 1, d ?? 1, 12);
}

/** "Thursday 12 November", and the year too when it is not this year's. */
export function longDay(day: string): string {
  const date = dateOf(day);
  const year = date.getFullYear() === new Date().getFullYear() ? "" : ` ${date.getFullYear()}`;
  return `${WEEKDAYS[date.getDay()]} ${date.getDate()} ${MONTHS[date.getMonth()]}${year}`;
}

/** "Tue 17 Nov". */
export function shortDay(day: string): string {
  const date = dateOf(day);
  return `${WEEKDAYS[date.getDay()].slice(0, 3)} ${date.getDate()} ${MONTHS[date.getMonth()].slice(0, 3)}`;
}

/**
 * A day said the way a person says when they last did something: "yesterday",
 * "on Tuesday" within the week, "on Thursday 5 November" before that. Never a
 * count of days.
 */
export function whenSaid(day: string, now = today()): string {
  const gap = Math.round((dateOf(now).getTime() - dateOf(day).getTime()) / 86_400_000);
  if (gap <= 0) return "today";
  if (gap === 1) return "yesterday";
  const date = dateOf(day);
  if (gap < 7) return `on ${WEEKDAYS[date.getDay()]}`;
  return `on ${longDay(day)}`;
}

/** "10:02", local time. */
export function clock(ms: number): string {
  const date = new Date(ms);
  return `${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

/** The moment a record's local time "2026-11-12 09:41" names, or null. */
export function recordMs(at: string): number | null {
  const found = /^(\d{4})-(\d{2})-(\d{2})[ T](\d{2}):(\d{2})/.exec(at);
  if (!found) return null;
  const [, y, m, d, h, min] = found.map(Number);
  return new Date(y, m - 1, d, h, min).getTime();
}

/** The day a number of days before `day`. */
export function daysBefore(day: string, n: number): string {
  const date = dateOf(day);
  date.setDate(date.getDate() - n);
  return dayOf(date);
}
