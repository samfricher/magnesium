// The log: what was said on each day, in every course, and what the record
// marked. Earlier pages are built from the turns the window kept and the
// record's marks; today's page adds what happens while the window is open.
// Nothing here counts anything about the learner.

import { dayOf, recordMs } from "./days";
import { opening, splitConfidence } from "./lesson";
import { markSaid, plainSaid } from "./marks";
import type { JournalCourse, Mark, Turn } from "./types";

/** One thing the learner wrote, the tutor's reply to it, and the record's marks for it. */
export type Entry = {
  ms: number;
  folder: string;
  course: string;
  learner: string;
  tutor: string;
  interrupted: boolean;
  marks: Mark[];
};

/** A run of entries in one course, with no long gap between them. */
export type Sitting = { folder: string; course: string; start: number; end: number; entries: Entry[]; papers: Mark[] };

export type Page = { day: string; sittings: Sitting[] };

/** A sitting ends after three hours without a word, as the runtime's sittings do. */
const GAP = 3 * 3600_000;

/** A course's name as the learner named its folder, with dashes and underscores read as spaces. */
export function courseName(name: string): string {
  return name.replace(/[-_]+/g, " ").replace(/\s+/g, " ").trim() || name;
}

const flat = (text: string) => text.replace(/[\s$*_`\\{}]+/g, " ").trim().toLowerCase();

/**
 * Each mark goes with the turn that answered its question: the first turn
 * after the reply that asked it. Without a question it goes with the last turn
 * sent at or before its minute, in the same course.
 */
function attach(turns: Turn[], marks: Mark[]): { byTurn: Map<number, Mark[]>; loose: Mark[] } {
  const byTurn = new Map<number, Mark[]>();
  const loose: Mark[] = [];
  const sorted = [...turns].sort((a, b) => a.ms - b.ms);
  const taken = new Set<number>();
  for (const mark of marks) {
    const at = recordMs(mark.at);
    if (at === null) continue;
    const before = sorted.filter((turn) => turn.ms <= at + 59_999 && at - turn.ms < GAP && dayOf(turn.ms) === dayOf(at));
    let found: Turn | null = null;
    const asked = mark.question ? flat(mark.question).slice(0, 32) : "";
    if (asked.length >= 12) {
      for (let i = 1; i < sorted.length; i++) {
        const turn = sorted[i];
        if (!before.includes(turn) || taken.has(turn.ms)) continue;
        if (flat(sorted[i - 1].tutor).includes(asked)) {
          found = turn;
          break;
        }
      }
    }
    found ??= [...before].reverse().find((turn) => !taken.has(turn.ms)) ?? null;
    if (found && mark.paper === null) {
      taken.add(found.ms);
      byTurn.set(found.ms, [...(byTurn.get(found.ms) ?? []), mark]);
    } else loose.push(mark);
  }
  return { byTurn, loose };
}

/** Every day's page, newest first. `skip` leaves a day out (today, which the live page draws). */
export function pagesFrom(courses: JournalCourse[]): Page[] {
  const sittings: Sitting[] = [];
  for (const course of courses) {
    const name = courseName(course.name);
    const { byTurn, loose } = attach(course.turns, course.marks ?? []);
    let current: Sitting | null = null;
    for (const turn of [...course.turns].sort((a, b) => a.ms - b.ms)) {
      if (turn.learner.trim() === "" && turn.tutor.trim() === "") continue;
      if (!current || turn.ms - current.end > GAP || dayOf(turn.ms) !== dayOf(current.start)) {
        current = { folder: course.folder, course: name, start: turn.ms, end: turn.ms, entries: [], papers: [] };
        sittings.push(current);
      }
      current.end = turn.ms;
      current.entries.push({
        ms: turn.ms,
        folder: course.folder,
        course: name,
        learner: turn.learner,
        tutor: turn.tutor,
        interrupted: turn.interrupted,
        marks: byTurn.get(turn.ms) ?? [],
      });
    }
    // Marks with no turn the window kept: a paper, or a lesson in a terminal.
    for (const mark of loose) {
      const at = recordMs(mark.at);
      if (at === null) continue;
      sittings.push({
        folder: course.folder,
        course: name,
        start: at,
        end: at,
        entries: mark.paper === null ? [{ ms: at, folder: course.folder, course: name, learner: "", tutor: "", interrupted: false, marks: [mark] }] : [],
        papers: mark.paper === null ? [] : [mark],
      });
    }
  }
  sittings.sort((a, b) => b.start - a.start);
  const pages: Page[] = [];
  for (const sitting of sittings) {
    const day = dayOf(sitting.start);
    let page = pages.find((p) => p.day === day);
    if (!page) {
      page = { day, sittings: [] };
      pages.push(page);
    }
    // Paper rows of one paper on one day are one line.
    const same = sitting.papers.length > 0 ? page.sittings.find((s) => s.folder === sitting.folder && s.entries.length === 0 && s.papers[0]?.paper === sitting.papers[0]?.paper) : null;
    if (same) same.papers.push(...sitting.papers);
    else page.sittings.push(sitting);
  }
  for (const page of pages) page.sittings.sort((a, b) => a.start - b.start);
  pages.sort((a, b) => (a.day < b.day ? 1 : -1));
  return pages;
}

/** The record's mark as the margin says it: a word, a second word, and the whole sentence for a tooltip and a screen reader. */
export type Verdict = { word: string; how: string | null; title: string; right: boolean };

export function verdict(mark: Mark): Verdict {
  const said = markSaid(mark);
  const title = said ? plainSaid(said) : `Marked ${mark.mark}.`;
  const help = mark.help === "hint" ? "with a hint" : mark.help === "step" ? "with a step" : mark.help === "solution" ? "after the solution" : null;
  switch (mark.mark) {
    case "right":
      return { word: "right", how: help ?? (mark.cold ? "unaided" : null), title, right: true };
    case "partly":
      return { word: "partly", how: help, title: "Marked partly right.", right: false };
    case "wrong":
      return { word: "not right", how: help, title, right: false };
    case "declined":
      return { word: "skipped", how: null, title: "You said you did not know: nothing is lost by it.", right: false };
    default:
      return { word: mark.mark, how: help, title, right: false };
  }
}

/** What the learner wrote, as the log shows it. */
export type Words = {
  /** The words themselves, without the window's added lines. */
  text: string;
  /** "Sure", "Unsure" or "Guess", when they said. */
  confidence: string | null;
  /** How many photos of working went with it. */
  photos: number;
  /** The window wrote this line to start or pick up a lesson, and the stop line it quotes. */
  opening: { stoppedAt: string | null } | null;
};

const PHOTOS = /(?:^|\n\n)Photos? of my working: ([^\n]+)$/;

/** The line the log shows for words the window wrote to start or pick up a lesson. */
export function openingLine(text: string, stoppedAt: string | null): string {
  if (stoppedAt) return `Picked up at ${stoppedAt}`;
  const topic = /^Let's work on ([\s\S]+)\.$/.exec(text.trim());
  if (topic) return `Started on ${topic[1]}`;
  if (text.startsWith("No checks right now")) return "Picked up the lesson, without the checks";
  return "Picked up the lesson";
}

export function wordsOf(learner: string): Words {
  const photo = PHOTOS.exec(learner);
  const photos = photo ? photo[1].split(",").filter((p) => p.trim() !== "").length : 0;
  const rest = photo ? learner.slice(0, photo.index).trim() : learner;
  const { text, confidence } = splitConfidence(rest);
  return { text, confidence, photos, opening: opening(text) };
}
