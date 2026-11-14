// What the session screen holds, and how each event from the tutor changes it.

import type { Answer, AgentEvent, Intent, Mark, NoticeKind, Question, Turn } from "./types";
import type { PlanDiff } from "./words";

export type Card =
  | { kind: "permission"; id: string; title: string; detail: string; canAlways: boolean }
  | { kind: "question"; id: string; questions: Question[] };

export type Status = "idle" | "waiting" | "writing" | "working";

/**
 * How the learner's words were sent: as the answer to the open task, as talk,
 * as one of the window's own help phrases, or as the window's opening words.
 * A turn read back from an earlier sitting does not know.
 */
export type Said = Intent | "help" | "opening";

/**
 * A turn of the lesson. `pick` is set when what the learner said was a choice
 * made on a card of the tutor's. `marks` are the record's rows for it, from
 * the `marked` event; `change` is what the turn changed in the plan, read from
 * the plan after `plan_changed`.
 */
export type LessonTurn = Omit<Turn, "posed"> & {
  pick?: boolean;
  said?: Said;
  marks?: Mark[];
  change?: PlanDiff;
  /** The task this turn's reply posed, as the engine read it from the tutor's record. */
  posed?: string;
  /** For an answer: the task it answered, as it stood when it was sent. */
  question?: string;
  /** For an answer the record kept no mark for: why, in the record's terms. */
  unmarked?: string;
};

/** The exchange on screen: what the learner said and the tutor's reply to it. */
export type Live = {
  learner: string;
  done: string[];
  streaming: string;
  status: Status;
  /** The work in hand, in plain words, while the tutor works: "writing paper 2". */
  doing: string | null;
  since: number;
  error: string | null;
  interrupted: boolean;
  /** What the learner said was an option picked on the tutor's card (or words typed on it), not a message they wrote. */
  pick?: boolean;
  said?: Said;
  marks?: Mark[];
  change?: PlanDiff;
  posed?: string;
  question?: string;
  unmarked?: string;
};

export type Notice = { kind: NoticeKind; text: string };

export type SessionState = {
  /** Whether an agent process is running for this folder. */
  open: boolean;
  earlier: LessonTurn[];
  live: Live | null;
  cards: Card[];
  notices: Notice[];
  /** When this sitting's clock started, or null before the first message. */
  startedMs: number | null;
  /**
   * The open task as the engine last said it, or undefined when the engine
   * has said nothing (Claude Code's does not): the page then finds a task by
   * its bold opening word.
   */
  task?: { text: string | null };
};

export const emptySession: SessionState = {
  open: false,
  earlier: [],
  live: null,
  cards: [],
  notices: [],
  startedMs: null,
};

export type Action =
  | { type: "opened"; turns: Turn[] }
  /** A session's saved turns, rebuilt with what each was, shown without its tutor running. */
  | { type: "shown"; turns: LessonTurn[]; task?: { text: string | null } }
  | { type: "reopened" }
  /** The tutor's program is another session's now. */
  | { type: "released" }
  | { type: "sent"; shown: string; now: number; said?: Said; question?: string }
  | { type: "planned"; change: PlanDiff }
  | { type: "unmarked"; since: number; reason: string }
  | { type: "picked"; shown: string; now: number }
  | { type: "event"; event: AgentEvent }
  | { type: "note"; notice: Notice }
  | { type: "dismiss"; text: string }
  | { type: "failed"; error: string }
  | { type: "stopped" }
  | { type: "reset" };

/** A kept turn as the lesson draws it, before anything is known of what it was. */
export function plainTurn(turn: Turn): LessonTurn {
  return { session: turn.session, ms: turn.ms, learner: turn.learner, tutor: turn.tutor, interrupted: turn.interrupted };
}

export function tutorText(live: Live): string {
  return [...live.done, live.streaming].filter((part) => part.trim() !== "").join("\n");
}

function blankLive(learner: string, now: number, status: Status): Live {
  return { learner, done: [], streaming: "", status, doing: null, since: now, error: null, interrupted: false };
}

function asTurn(live: Live): LessonTurn | null {
  const tutor = tutorText(live);
  if (tutor === "" && live.learner.trim() === "") return null;
  return {
    session: "",
    ms: live.since,
    learner: live.learner,
    tutor,
    interrupted: live.interrupted,
    ...(live.pick ? { pick: true } : {}),
    ...(live.said ? { said: live.said } : {}),
    ...(live.marks ? { marks: live.marks } : {}),
    ...(live.change ? { change: live.change } : {}),
    ...(live.posed ? { posed: live.posed } : {}),
    ...(live.question ? { question: live.question } : {}),
    ...(live.unmarked ? { unmarked: live.unmarked } : {}),
  };
}

/** Claude Code's line while it waits out a limit and will try again by itself. */
const waitingOut = (text: string) => /waiting to try again/i.test(text);

/**
 * One usage limit, said once. Claude Code reports a limit up to three ways:
 * waiting to try again, when it resets, and at the turn's end what to do.
 * The one that says when it resets is kept, with what to do added if it
 * lacks it; the waiting line gives way to either.
 */
function limitNotice(old: Notice, next: Notice): Notice {
  if (waitingOut(old.text)) return next;
  if (waitingOut(next.text)) return old;
  const resets = (text: string) => /\bresets?\b/i.test(text);
  // The one that says when it resets; else the later, which is the turn's end saying what to do.
  const base = resets(old.text) ? old : next;
  if (/send your message again/i.test(base.text)) return base;
  return { ...base, text: `${base.text.trim()} ${resets(base.text) ? "Send your message again once it resets." : "Send your message again later."}` };
}

/** What the learner writes next lets go of what was said about the turn before; the rest still stands. */
const aboutTheTurn = (notice: Notice) => notice.kind === "turn" || notice.kind === "retry" || notice.kind === "usage_limit";

function addNotice(notices: Notice[], notice: Notice): Notice[] {
  if (notice.kind === "usage_limit") {
    const at = notices.findIndex((n) => n.kind === "usage_limit");
    if (at !== -1) return notices.map((n, i) => (i === at ? limitNotice(n, notice) : n));
  }
  // A retry line replaces the retry line before it, so they do not pile up.
  const kept = notices.filter((n) => n.text !== notice.text && !(n.kind === "retry" && notice.kind === "retry"));
  return [...kept, notice].slice(-3);
}

function onEvent(state: SessionState, event: AgentEvent): SessionState {
  const live = state.live ?? blankLive("", Date.now(), "idle");
  switch (event.type) {
    case "ready":
      return {
        ...state,
        notices: event.problems.reduce((all, text) => addNotice(all, { kind: "other", text }), state.notices),
      };
    case "turn_started":
      return { ...state, live: { ...live, status: "writing" }, notices: state.notices.filter((n) => n.kind !== "retry") };
    case "text_delta":
      return { ...state, live: { ...live, streaming: live.streaming + event.text, status: "writing", doing: null } };
    case "text_done":
      // A block already on screen is not shown twice.
      if (live.done.includes(event.text)) return { ...state, live: { ...live, streaming: "", status: "writing", doing: null } };
      return { ...state, live: { ...live, done: [...live.done, event.text], streaming: "", status: "writing", doing: null } };
    case "working":
      // The line names the latest work the window can name, and keeps it
      // while the next step starts.
      return { ...state, live: { ...live, status: "working", doing: event.doing ?? live.doing } };
    case "permission":
      return {
        ...state,
        cards: [
          ...state.cards.filter((card) => card.id !== event.id),
          { kind: "permission", id: event.id, title: event.title, detail: event.detail, canAlways: event.can_always },
        ],
      };
    case "question":
      return {
        ...state,
        cards: [...state.cards.filter((card) => card.id !== event.id), { kind: "question", id: event.id, questions: event.questions }],
      };
    case "withdrawn":
      return { ...state, cards: state.cards.filter((card) => card.id !== event.id) };
    case "turn_ended": {
      // Words written before an interrupt stay on screen as they were.
      const done = live.streaming.trim() !== "" ? [...live.done, live.streaming] : live.done;
      return {
        ...state,
        cards: [],
        live: { ...live, done, streaming: "", status: "idle", error: event.error, interrupted: event.interrupted },
        // Nothing waits to try again once the turn has ended.
        notices: state.notices.filter((n) => !waitingOut(n.text)),
      };
    }
    case "notice":
      return { ...state, notices: addNotice(state.notices, { kind: event.kind, text: event.text }) };
    case "exited": {
      const ended = { ...state, open: false, cards: [], live: state.live ? { ...live, status: "idle" as Status } : null };
      if (event.expected) return ended;
      const text = `The tutor's program stopped. ${event.detail ?? ""} Send your message again to restart it.`.replace(/\s+/g, " ");
      return { ...ended, notices: addNotice(state.notices, { kind: "turn", text }) };
    }
    // A mark belongs to the last message the learner sent. The plan is read by the page, which then says what changed.
    case "marked": {
      if (!state.live) return state;
      const marks = [...(state.live.marks ?? []).filter((mark) => mark.id !== event.mark.id), event.mark];
      return { ...state, live: { ...state.live, marks } };
    }
    case "plan_changed":
    case "settled":
      return state;
    case "open_task": {
      const task = { text: event.task };
      if (!event.posed || !event.task || !state.live) return { ...state, task };
      return { ...state, task, live: { ...state.live, posed: event.task } };
    }
  }
}

export function reduce(state: SessionState, action: Action): SessionState {
  switch (action.type) {
    case "opened": {
      // A resumed lesson opens on its last exchange, with the rest folded.
      const turns = action.turns.map(plainTurn);
      const last = turns.length > 0 ? turns[turns.length - 1] : null;
      return {
        ...state,
        open: true,
        earlier: turns.slice(0, -1),
        cards: [],
        live: last
          ? { ...blankLive(last.learner, last.ms, "idle"), done: last.tutor ? [last.tutor] : [], interrupted: last.interrupted }
          : null,
      };
    }
    case "shown": {
      const turns = action.turns;
      const last = turns.length > 0 ? turns[turns.length - 1] : null;
      return {
        ...state,
        open: false,
        earlier: turns.slice(0, -1),
        cards: [],
        ...(action.task ? { task: action.task } : {}),
        live: last
          ? {
              ...blankLive(last.learner, last.ms, "idle"),
              done: last.tutor ? [last.tutor] : [],
              interrupted: last.interrupted,
              ...(last.said ? { said: last.said } : {}),
              ...(last.marks ? { marks: last.marks } : {}),
              ...(last.change ? { change: last.change } : {}),
              ...(last.posed ? { posed: last.posed } : {}),
              ...(last.question ? { question: last.question } : {}),
              ...(last.unmarked ? { unmarked: last.unmarked } : {}),
            }
          : null,
      };
    }
    case "reopened":
      // The agent was restarted under a lesson that is still on screen.
      return { ...state, open: true };
    case "released":
      // Another tab's session took the tutor's program: this one shows what it said, and starts again when written to.
      return { ...state, open: false, cards: [], live: state.live && state.live.status !== "idle" ? { ...state.live, status: "idle" } : state.live };
    case "sent": {
      const previous = state.live ? asTurn(state.live) : null;
      return {
        ...state,
        earlier: previous ? [...state.earlier, previous] : state.earlier,
        live: {
          ...blankLive(action.shown, action.now, "waiting"),
          ...(action.said ? { said: action.said } : {}),
          ...(action.question ? { question: action.question } : {}),
        },
        cards: [],
        // Notices about the turn before go when the learner writes again; one that still stands stays.
        notices: state.notices.filter((n) => !aboutTheTurn(n)),
        startedMs: state.startedMs ?? action.now,
      };
    }
    case "unmarked": {
      // Only when the record still has no mark for that answer.
      const mark = (turn: { marks?: Mark[] }) => (turn.marks ?? []).length > 0;
      if (state.live && state.live.since === action.since) {
        return mark(state.live) ? state : { ...state, live: { ...state.live, unmarked: action.reason } };
      }
      return {
        ...state,
        earlier: state.earlier.map((turn) => (turn.ms === action.since && turn.said === "answer" && !mark(turn) ? { ...turn, unmarked: action.reason } : turn)),
      };
    }
    case "planned": {
      if (!state.live) return state;
      const changes = [...(state.live.change?.changes ?? []), ...action.change.changes];
      const merged: PlanDiff = {
        changes,
        moved: { ...(state.live.change?.moved ?? {}), ...action.change.moved },
        examFrom: state.live.change?.examFrom ?? action.change.examFrom,
      };
      return { ...state, live: { ...state.live, change: merged } };
    }
    case "picked": {
      // An option chosen on a card is what the learner said: it stands in the
      // transcript as their note, and the tutor's reply to it is the exchange
      // that follows. The card itself is taken away by the tutor's own event.
      const previous = state.live ? asTurn(state.live) : null;
      return {
        ...state,
        earlier: previous ? [...state.earlier, previous] : state.earlier,
        live: { ...blankLive(action.shown, action.now, "waiting"), pick: true },
        startedMs: state.startedMs ?? action.now,
      };
    }
    case "event":
      return onEvent(state, action.event);
    case "note":
      return { ...state, notices: addNotice(state.notices, action.notice) };
    case "dismiss":
      return { ...state, notices: state.notices.filter((n) => n.text !== action.text) };
    case "failed":
      return {
        ...state,
        live: state.live ? { ...state.live, status: "idle", error: action.error } : blankLive("", Date.now(), "idle"),
        notices: addNotice(state.notices, { kind: "turn", text: action.error }),
      };
    case "stopped":
      return { ...emptySession };
    case "reset":
      return { ...emptySession };
  }
}

export function busy(state: SessionState): boolean {
  return state.live !== null && state.live.status !== "idle";
}

/**
 * The words the window sends for the learner when a lesson opens. They are
 * plain words, as a learner would type them in a terminal. The runtime opens
 * the sitting with what is due, so the window does not ask for it. Only the
 * learner's own saved line is quoted back: the tutor's note of what comes
 * next is already in its record.
 */
export function openingWords(last: { stoppedAt: string | null; studiedBefore: boolean; title: string }, skipChecks: boolean): string {
  if (skipChecks) return "No checks right now. Let's continue where I stopped.";
  if (last.stoppedAt) return `I'm back. Let's continue. I stopped at: ${last.stoppedAt}`;
  if (last.studiedBefore) return "I'm back. Let's continue.";
  return `Let's work on ${last.title}.`;
}

const OPENING = /^(?:I'm back\. Let's continue\.(?: I stopped at: ([\s\S]+))?|No checks right now\. Let's continue where I stopped\.|Let's work on ([\s\S]+)\.)$/;

/**
 * Whether a line is one of those opening lines, the stop line it quotes, and
 * the topic a "Let's work on" names. The learner did not type it, so the
 * lesson shows it as one quiet line and not as something they said.
 */
export function opening(text: string): { stoppedAt: string | null; topic: string | null } | null {
  const found = OPENING.exec(text.trim());
  return found ? { stoppedAt: found[1]?.trim() || null, topic: found[2]?.trim() || null } : null;
}

/** The line the core writes when a paper is handed in: "Mark my Paper 2. I used 41 of 50 minutes. My work: …". */
const HANDED_IN = /^Mark my /;

/** Whether the window wrote these words for the learner (an opening line, a paper handed in), and not the learner. */
export function saidByTheWindow(text: string): boolean {
  return opening(text) !== null || HANDED_IN.test(text.trim());
}

/**
 * The plain words the three help buttons send. The tutor gives the smallest
 * help that lets the learner take the next step, and the runtime asks a skill
 * got right with a hint again, cold, the next day.
 */
export const HELP = { hint: "Can I have a hint?", unknown: "I don't know", lost: "I'm lost" } as const;

/**
 * What the learner said, apart from how sure they said they were. The window
 * sends the answer, then "Confidence: sure" (or unsure, or guess) as a line of
 * its own; the lesson shows that as a tag, not as words the learner wrote.
 */
export function splitConfidence(text: string): { text: string; confidence: "Sure" | "Unsure" | "Guess" | null } {
  const found = /(?:^|\n\n)Confidence: (sure|unsure|guess)(?=\n\n|$)/i.exec(text);
  if (!found) return { text, confidence: null };
  const word = found[1].toLowerCase();
  const rest = `${text.slice(0, found.index)}${text.slice(found.index + found[0].length)}`.trim();
  return { text: rest, confidence: word === "sure" ? "Sure" : word === "unsure" ? "Unsure" : "Guess" };
}

/** Whether a line the learner sent is a request for a hint. */
export function isHint(text: string): boolean {
  return text.trim() === HELP.hint;
}

/** Whether a line the learner sent is a request for help (a hint, "I don't know", "I'm lost"): not an answer, and never marked. */
export function isHelp(text: string): boolean {
  return (Object.values(HELP) as string[]).includes(splitConfidence(text).text.trim());
}

/**
 * What an answer given on a card says, as the transcript shows it: the options
 * chosen and the words typed, one line for each question.
 */
export function pickedWords(answers: Answer[]): string {
  return answers
    .map((answer) => answer.chosen.join(", "))
    .filter((line) => line.trim() !== "")
    .join("\n");
}

/**
 * The words that name files sent with a message. A file sent with an answer,
 * or while a task is open, is the learner's work, handed in with it. Any other
 * is a file for the tutor to read, and the tutor is told plainly that it may
 * be working or course material, so it can ask when it cannot tell.
 */
export function attachedWords(paths: string[], working: boolean): string {
  const list = paths.join(", ");
  const photos = paths.every((path) => !/\.pdf$/i.test(path));
  if (working) return `${paths.length === 1 ? (photos ? "Photo" : "File") : photos ? "Photos" : "Files"} of my working: ${list}`;
  return paths.length === 1
    ? `A file for you to read: ${list}. It may be my working or course material.`
    : `Files for you to read: ${list}. They may be my working or course material.`;
}

const ATTACHED = /(?:^|\n\n)(?:(?:Photos?|Files?) of my working: (.+)|(?:A file|Files) for you to read: (.+?)\. (?:It|They) may be my working or course material\.)\s*$/;

/** What the learner said, apart from the files that went with it, which the lesson names by their file names alone. */
export function splitAttached(text: string): { text: string; files: string[]; working: boolean } {
  const found = ATTACHED.exec(text);
  if (!found) return { text, files: [], working: false };
  const list = (found[1] ?? found[2] ?? "").split(", ").map((each) => each.trim()).filter(Boolean);
  return { text: text.slice(0, found.index).trim(), files: list, working: found[1] !== undefined };
}
