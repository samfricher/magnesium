// Which of the record's marks belong to each turn of a reopened session.

import type { Mark } from "./types";

/** "2026-11-12 21:01" for a moment, as the record writes when an answer was given. */
function minuteOf(ms: number): string {
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/**
 * The record's marks for each kept turn, in the order of `turns`.
 *
 * A turn the window read the record for (`mark_ids` kept) has the rows with
 * those ids, and no others: the marks the lesson showed under it. The rows
 * carry only the minute, which several quick answers can share, so the ids
 * are what tells them apart.
 *
 * A turn kept before the window kept ids, or one whose reading was cut short,
 * has the rows from no paper, owned by no other turn, whose minute falls
 * between the turn before it and its own; `byMinute(at)` says whether the
 * turn at `at` may take a mark that way. A row is given to one turn at most.
 */
export function marksOfTurns(turns: { ms: number; mark_ids?: string[] | null }[], marks: Mark[], byMinute: (at: number) => boolean): Mark[][] {
  const byId = new Map(marks.map((mark) => [mark.id, mark]));
  const owned = new Set(turns.flatMap((turn) => turn.mark_ids ?? []));
  const free = marks.filter((mark) => !mark.paper && !owned.has(mark.id));
  return turns.map((turn, at) => {
    if (turn.mark_ids) return [...new Set(turn.mark_ids)].flatMap((id) => byId.get(id) ?? []);
    if (!byMinute(at)) return [];
    const from = at > 0 ? minuteOf(turns[at - 1].ms) : "";
    const to = minuteOf(turn.ms);
    const mine = free.filter((mark) => mark.at >= from && mark.at <= to);
    for (const mark of mine) free.splice(free.indexOf(mark), 1);
    return mine;
  });
}
