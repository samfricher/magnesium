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
import type { Mark } from "./types";

/**
 * The words for one marked answer, or null when the lesson has nothing to say
 * about it. `word` is what the margin says beside the answer ("Marked right");
 * `before`, `skill` and `after` are the whole sentence, with the skill's name
 * kept apart so the page can typeset it.
 */
export type MarkSaid = { word: string; right: boolean; before: string; skill: string | null; after: string };

/** A local day as YYYY-MM-DD, the form a record's date takes. */
const dayOf = (date: Date) =>
  `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;

/**
 * When a wrong answer returns, if the record says. A row that carries the day
 * it is next due (`due`, "2026-11-13") and that day is tomorrow says
 * "tomorrow"; a row with no such day, or a later one, says only that it comes
 * back on a later day. The window never works the day out for itself.
 */
function returns(row: Mark): string {
  const due = (row as Mark & { due?: unknown }).due;
  if (typeof due === "string") {
    const tomorrow = new Date();
    tomorrow.setDate(tomorrow.getDate() + 1);
    if (due.slice(0, 10) === dayOf(tomorrow)) return "It comes back tomorrow.";
  }
  return "It comes back on a later day.";
}

/**
 * A skill's name as plain words, for a tooltip and a screen reader: the
 * dollar signs and braces of its mathematics are dropped and a subscript is
 * said with its underscore ("intercept and $V_{max}$" is "intercept and V_max").
 */
export function plainSkill(label: string): string {
  return label
    .replace(/\\text\{([^}]*)\}/g, "$1")
    .replace(/_\{([^}]*)\}/g, "_$1")
    .replace(/[$\\{}]/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

/**
 * A skill's name ready to be drawn: a name that carries its mathematics in
 * dollar signs is drawn as it is; one that writes a subscript bare ("V_max")
 * has that word put in dollar signs, so it is typeset, never shown as plain
 * letters with an underscore.
 */
export function skillTex(label: string): string {
  if (label.includes("$")) return label;
  return label.replace(/(^|[\s(])([A-Za-z][A-Za-z0-9]*_(?:\{[A-Za-z0-9]+\}|[A-Za-z0-9]+))(?=$|[\s.,;:)])/g, "$1$$$2$$");
}

/** The whole sentence as plain text. */
export const plainSaid = (said: MarkSaid) => `${said.before}${said.skill ? plainSkill(said.skill) : ""}${said.after}`;

/**
 * What a row says, in the words of the runtime's rule: an answer counts
 * towards a proof only when it was right, with no help, from a blank page.
 * A partly right answer, a decline and a paper's answer have no line here.
 * The mark is about the answer it sits beside, and says only that the answer
 * counts towards a proof: a proof is earned by the runtime on its own day,
 * never by one answer, so no mark of an earned proof is drawn.
 */
export function markSaid(row: Mark): MarkSaid | null {
  if (row.paper !== null) return null;
  if (row.mark === "right") {
    if (row.help !== "none") return { word: "Right, with help", right: false, before: "Right, with help. Not counted towards a proof.", skill: null, after: "" };
    if (!row.cold) return { word: "Marked right", right: true, before: "Marked right. Not counted towards a proof.", skill: null, after: "" };
    const skill = row.label.trim() === "" ? null : row.label.trim();
    return {
      word: "Marked right",
      right: true,
      before: skill ? "Marked right. Counts towards a proof of " : "Marked right. Counts towards a proof.",
      skill,
      after: skill ? "." : "",
    };
  }
  if (row.mark === "wrong") return { word: "Not right yet", right: false, before: `Not right yet. ${returns(row)}`, skill: null, after: "" };
  return null;
}

/** Every row shown in this window, by the exchange it belongs to (the time it began). Kept so a lesson left and returned to still shows them. */
const shown = new Map<number, Mark[]>();
/** The exchanges whose answer was read before it was sent, so the window is watching the record for their mark. */
const reserved = new Set<number>();

/** Bumped whenever a mark is added, so the page can tell that the lesson has grown. */
let version = 0;
/** The version at which each row arrived, so the page can find the mark that has just come. */
const added = new Map<string, number>();

/**
 * What the record said of the answer that began at `since`: one mark for each
 * row, in the order they arrived. Empty until the record has a mark for it, and
 * nothing is kept ready for it: the page draws it beside the answer, in the
 * margin where there is one, out of the flow, or under the answer in a narrow
 * window once the reply has settled.
 */
export function marksFor(since: number): (MarkSaid & { added: number })[] {
  return (shown.get(since) ?? []).flatMap((row) => {
    const said = markSaid(row);
    return said ? [{ ...said, added: added.get(row.id) ?? 0 }] : [];
  });
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
export function useMarks(since: number | null, settled: boolean): { arm: () => void; version: number } {
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
    version += 1;
    rows.forEach((row) => added.set(row.id, version));
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
      // The runtime could not say: the exchange is not watched, and nothing is said.
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

  return { arm, version };
}
