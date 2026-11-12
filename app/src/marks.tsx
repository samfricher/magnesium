// What the runtime's record says about the answers sent in a lesson.
//
// The window never takes the tutor's word for a mark. Before an answer is
// sent it reads the runtime's marked answers (`magnesium attempts`), and
// when the reply has settled it reads them again. A row that was not there
// before is the mark of that answer, and the lesson says what the row says
// and nothing more. No new row means nothing is said, whatever the tutor
// wrote. A runtime with no such command says nothing and changes nothing.

import { useCallback, useEffect, useLayoutEffect, useReducer, useRef } from "react";

import { core } from "./bridge";
import { Line } from "./ui/Rich";
import type { Mark } from "./types";

/** The words for one marked answer, or null when the lesson has nothing to say about it. */
export type MarkSaid = { proof: boolean; before: string; skill: string | null; after: string };

/**
 * What a row says, in the words of the runtime's rule: an answer counts
 * towards a proof only when it was right, with no help, from a blank page.
 * A partly right answer, a decline and a paper's answer have no line here.
 */
export function markSaid(row: Mark): MarkSaid | null {
  if (row.paper !== null) return null;
  if (row.mark === "right") {
    if (row.help !== "none") return { proof: false, before: "Marked right, with help: not counted.", skill: null, after: "" };
    if (!row.cold) return { proof: false, before: "Marked right. Not counted towards a proof.", skill: null, after: "" };
    const skill = row.label.trim() === "" ? null : row.label.trim();
    return { proof: true, before: skill ? "Marked right. Counts towards a proof of " : "Marked right. Counts towards a proof.", skill, after: skill ? "." : "" };
  }
  if (row.mark === "wrong") return { proof: false, before: "Marked: not yet. It comes back later.", skill: null, after: "" };
  return null;
}

/** Every row shown in this window, by the exchange it belongs to (the time it began). Kept so a lesson left and returned to still shows them. */
const shown = new Map<number, Mark[]>();
/** The exchanges that hold a line for a mark. */
const reserved = new Set<number>();

/** The quiet lines under one answer, or the room for them. Nothing above or below moves when they arrive. */
export function MarkLines({ since, replying }: { since: number; replying: boolean }) {
  const rows = shown.get(since) ?? [];
  const lines = rows.flatMap((row) => {
    const said = markSaid(row);
    return said ? [{ id: row.id, said }] : [];
  });
  // The room is kept once the reply starts, so the reply is never pushed down by a line arriving later.
  if (lines.length === 0 && !(reserved.has(since) && replying)) return null;
  return (
    <div className="mark-lines" data-role="marks" data-state={lines.length > 0 ? "said" : "held"}>
      {lines.map(({ id, said }) => (
        <p className="mark-line" key={id} data-proof={said.proof ? "yes" : "no"}>
          {said.proof && (
            <span className="dots" aria-hidden="true">
              <i className="on" />
            </span>
          )}
          <span>
            {said.before}
            {said.skill && <Line text={said.skill} />}
            {said.after}
          </span>
        </p>
      ))}
    </div>
  );
}

const ids = (rows: Mark[]) => new Set(rows.map((row) => row.id));
/** When, after the reply settles, the record is read again. The runtime may write the mark just after the reply is whole. */
const LOOKS = [0, 1000, 2500, 5000, 10_000, 20_000];

async function read(): Promise<Mark[] | null> {
  try {
    return await (await core()).marks();
  } catch {
    return null;
  }
}

/**
 * Keeps the marks of the lesson's answers. `since` names the exchange on
 * screen and `settled` is true once its reply is whole. `arm` is called just
 * before an answer is sent: it reads the record, and the next exchange that
 * begins is the answer's.
 */
export function useMarks(since: number | null, settled: boolean): { arm: () => void } {
  const [, redraw] = useReducer((n: number) => n + 1, 0);
  // Every id the window has accounted for: already in the record, or shown.
  const seen = useRef<Set<string> | null>(null);
  // The reading taken just before an answer was sent, until its exchange begins.
  const armed = useRef<{ at: number; reading: Promise<Mark[] | null> } | null>(null);
  // The reading taken before each answer, by the exchange it began.
  const readings = useRef(new Map<number, Promise<Mark[] | null>>());
  const latest = useRef<number | null>(null);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  /** Take in what the record now holds. Returns the rows that are new since the window last looked. */
  const take = useCallback((rows: Mark[]): Mark[] => {
    const known = seen.current;
    seen.current = new Set([...(known ?? []), ...ids(rows)]);
    return known ? rows.filter((row) => !known.has(row.id)) : [];
  }, []);

  const add = useCallback((at: number, rows: Mark[]) => {
    if (rows.length === 0) return;
    shown.set(at, [...(shown.get(at) ?? []), ...rows]);
    if (alive.current) redraw();
  }, []);

  const arm = useCallback(() => {
    const reading = read();
    armed.current = { at: Date.now(), reading };
    const previous = latest.current;
    void reading.then((rows) => {
      if (rows === null) return;
      // A mark that reached the record after the last look belongs to the exchange before this one.
      const late = take(rows);
      if (previous !== null && reserved.has(previous)) add(previous, late);
    });
  }, [add, take]);

  // The exchange that begins right after an answer was sent is that answer's.
  useLayoutEffect(() => {
    if (since === null) return;
    latest.current = since;
    const a = armed.current;
    if (!a) return;
    armed.current = null;
    if (Date.now() - a.at > 5000 || reserved.has(since)) return;
    reserved.add(since);
    readings.current.set(since, a.reading);
    void a.reading.then((rows) => {
      // The runtime could not say: no line, and no room for one.
      if (rows === null) {
        reserved.delete(since);
        if (alive.current) redraw();
      }
    });
    redraw();
  }, [since]);

  // When the reply settles, read the record again, and again for a while: the
  // mark can arrive just after the reply is whole.
  useEffect(() => {
    if (since === null || !settled || !reserved.has(since)) return;
    const timers: number[] = [];
    let stopped = false;
    let found = false;
    const current = () => !stopped && alive.current && latest.current === since && reserved.has(since);
    const look = async (n: number) => {
      if (!current()) return;
      const rows = await read();
      if (!current()) return;
      if (rows === null) {
        reserved.delete(since);
        redraw();
        return;
      }
      const fresh = take(rows);
      if (fresh.length > 0) {
        add(since, fresh);
        // One more look soon, for an answer the tutor marked in two parts.
        if (!found) {
          found = true;
          timers.push(window.setTimeout(() => void look(LOOKS.length), 1500));
        }
        return;
      }
      if (found || n + 1 >= LOOKS.length) return;
      timers.push(window.setTimeout(() => void look(n + 1), LOOKS[n + 1] - LOOKS[n]));
    };
    // The reading before the answer was sent comes first, so a row from before is never taken for this answer's.
    void (readings.current.get(since) ?? Promise.resolve(null)).then(() => void look(0));
    return () => {
      stopped = true;
      timers.forEach((timer) => window.clearTimeout(timer));
    };
  }, [since, settled, add, take]);

  return { arm };
}
