import type { Mark } from "./types";

function minuteOf(ms: number): string {
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

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
