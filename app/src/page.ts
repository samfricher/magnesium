// A lesson as a page: the exchanges in order, which reply set the task that
// is still open, which task each answer answered, and where the days change.
// Pure, so the page and the self-check read it the same way.

import { HELP, opening, splitConfidence, type LessonTurn, type Live, type SessionState } from "./lesson";
import { splitTask, type Split } from "./task";
import type { Mark } from "./types";

/** What a message was, by where it was written. "opening" is the window's own words that start or continue a lesson. */
export type Said = "answer" | "talk" | "help" | "opening";

export type Meta = { said: Said; marks: Mark[] };

export type Exchange = {
  /** The time it began: the key of its meta while the window is open. */
  key: number;
  learner: string;
  tutor: string;
  /** False while the reply is being written. */
  final: boolean;
  interrupted: boolean;
  live: Live | null;
  said: Said;
  marks: Mark[];
  /** The task this reply set, split from what comes before it. */
  split: Split | null;
  /** For an answer or a request for help: the exchange whose task it was about. */
  about: number | null;
  /** The day it happened, when it differs from the exchange before: "Thursday 12 November". */
  day: string | null;
};

export type Page = { exchanges: Exchange[]; open: number | null };

/** The key an exchange's meta is kept under on this Mac: its words, which a reopened lesson shows again. */
export const exKey = (learner: string, tutor: string) => `${learner.trim()}\u241f${tutor.replace(/\s+/g, " ").trim().slice(0, 60)}`;

/** The key of what a message was, by its words alone: kept when it is sent, before any reply. */
export const saidKey = (learner: string) => `said\u241f${learner.trim()}`;

/** What the window kept of an exchange: by its words and its reply, else by its words alone. */
export function keptMeta(kept: Record<string, Meta>, learner: string, tutor: string): Meta | null {
  return kept[exKey(learner, tutor)] ?? kept[saidKey(learner)] ?? null;
}

const DAY = new Intl.DateTimeFormat("en-GB", { weekday: "long", day: "numeric", month: "long" });

export function dayName(ms: number): string {
  return DAY.format(new Date(ms));
}

const dayKey = (ms: number) => {
  const d = new Date(ms);
  return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
};

/** What a message the learner sent was, when the window did not keep it: help and the window's own words by their words. */
export function guessSaid(learner: string, taskOpen: boolean): Said {
  const plain = splitConfidence(learner).text.trim();
  if (opening(plain) || plain === "") return "opening";
  if ((Object.values(HELP) as string[]).includes(plain)) return "help";
  return taskOpen ? "answer" : "talk";
}

/** Read the page from the lesson's state. `metaOf` gives what the window kept of each exchange. */
export function readPage(session: SessionState, metaOf: (turn: LessonTurn, index: number) => Meta | null): Page {
  const turns: { turn: LessonTurn; live: Live | null }[] = session.earlier.map((turn) => ({ turn, live: null }));
  if (session.live) {
    const live = session.live;
    const tutor = [...live.done, live.streaming].filter((part) => part.trim() !== "").join("\n");
    turns.push({
      turn: { session: "", ms: live.since, learner: live.learner, tutor, interrupted: live.interrupted, ...(live.pick ? { pick: true } : {}) },
      live,
    });
  }
  const exchanges: Exchange[] = [];
  let open: number | null = null;
  let lastDay: string | null = null;
  turns.forEach(({ turn, live }, index) => {
    const final = live === null || live.status === "idle";
    const kept = metaOf(turn, index);
    const said = kept?.said ?? guessSaid(turn.learner, open !== null);
    const split = final && turn.tutor.trim() !== "" ? splitTask(turn.tutor) : null;
    const about = said === "answer" || said === "help" ? open : null;
    const day = dayKey(turn.ms);
    exchanges.push({
      key: turn.ms,
      learner: turn.learner,
      tutor: turn.tutor,
      final,
      interrupted: turn.interrupted,
      live,
      said,
      marks: kept?.marks ?? [],
      split,
      about,
      day: day !== lastDay ? dayName(turn.ms) : null,
    });
    lastDay = day;
    if (!final) return;
    // A new task opens. A task closes when the record marks what was sent
    // for it: until then the engine keeps it open, whatever the reply said.
    // Talk and help leave it open. A new sitting starts with no task open,
    // as the engine does.
    if (split) open = index;
    else if (said === "opening") open = null;
    else if ((kept?.marks.length ?? 0) > 0 && !turn.interrupted) open = null;
  });
  return { exchanges, open };
}
