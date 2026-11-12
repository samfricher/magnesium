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
export type LessonTurn = Turn & {
  pick?: boolean;
  said?: Said;
  marks?: Mark[];
  change?: PlanDiff;
  /** The task this turn's reply posed, as the engine read it from the tutor's record. */
  posed?: string;
  /** For an answer: the task it answered, as it stood when it was sent. */
  question?: string;
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
  | { type: "reopened" }
  | { type: "sent"; shown: string; now: number; said?: Said; question?: string }
  | { type: "planned"; change: PlanDiff }
  | { type: "picked"; shown: string; now: number }
  | { type: "event"; event: AgentEvent }
  | { type: "note"; notice: Notice }
  | { type: "dismiss"; text: string }
  | { type: "failed"; error: string }
  | { type: "stopped" }
  | { type: "reset" };

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
  };
}

function addNotice(notices: Notice[], notice: Notice): Notice[] {
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
      };
    }
    case "notice":
      return { ...state, notices: addNotice(state.notices, { kind: event.kind, text: event.text }) };
    case "exited": {
      const ended = { ...state, open: false, cards: [], live: state.live ? { ...live, status: "idle" as Status } : null };
      if (event.expected) return ended;
      const text = `The tutor's program stopped. ${event.detail ?? ""} Send your message again to restart it.`.replace(/\s+/g, " ");
      return { ...ended, notices: addNotice(state.notices, { kind: "other", text }) };
    }
    // A mark belongs to the last message the learner sent. The plan is read by the page, which then says what changed.
    case "marked": {
      if (!state.live) return state;
      const marks = [...(state.live.marks ?? []).filter((mark) => mark.id !== event.mark.id), event.mark];
      return { ...state, live: { ...state.live, marks } };
    }
    case "plan_changed":
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
      const turns = action.turns;
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
    case "reopened":
      // The agent was restarted under a lesson that is still on screen.
      return { ...state, open: true };
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
        startedMs: state.startedMs ?? action.now,
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
        notices: addNotice(state.notices, { kind: "other", text: action.error }),
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

const OPENING = /^(?:I'm back\. Let's continue\.(?: I stopped at: ([\s\S]+))?|No checks right now\. Let's continue where I stopped\.|Let's work on [\s\S]+\.)$/;

/**
 * Whether a line is one of those opening lines, and the stop line it quotes.
 * The learner did not type it, so the lesson shows it as one quiet line and
 * not as something they said.
 */
export function opening(text: string): { stoppedAt: string | null } | null {
  const found = OPENING.exec(text.trim());
  return found ? { stoppedAt: found[1]?.trim() || null } : null;
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
