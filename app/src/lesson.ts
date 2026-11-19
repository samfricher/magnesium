import type { Answer, AgentEvent, Intent, Mark, NoticeKind, Question, StepBegun, Turn } from "./types";
import type { PlanDiff } from "./words";

export type Card =
  | { kind: "permission"; id: string; title: string; detail: string; exact: string | null; canAlways: boolean; always: string | null }
  | { kind: "question"; id: string; questions: Question[] };

export type Status = "idle" | "waiting" | "writing" | "working";

export type Said = Intent | "help" | "opening";

export type LessonTurn = Omit<Turn, "posed"> & {
  pick?: boolean;
  said?: Said;
  marks?: Mark[];
  change?: PlanDiff;
  posed?: string;
  question?: string;
  closing?: string[];
};

export type Live = {
  learner: string;
  done: string[];
  streaming: string;
  status: Status;
  doing: string | null;
  since: number;
  error: string | null;
  interrupted: boolean;
  pick?: boolean;
  said?: Said;
  marks?: Mark[];
  change?: PlanDiff;
  posed?: string;
  question?: string;
  closing?: string[];
  step?: StepBegun;
};

export type Notice = { kind: NoticeKind; text: string };

export type SessionState = {
  open: boolean;
  earlier: LessonTurn[];
  live: Live | null;
  cards: Card[];
  notices: Notice[];
  startedMs: number | null;
  task?: { text: string | null; fallback?: boolean };
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
  | { type: "shown"; turns: LessonTurn[]; task?: { text: string | null } }
  | { type: "reopened" }
  | { type: "released" }
  | { type: "sent"; shown: string; now: number; said?: Said; question?: string }
  | { type: "planned"; change: PlanDiff }
  | { type: "picked"; shown: string; now: number }
  | { type: "event"; event: AgentEvent }
  | { type: "note"; notice: Notice }
  | { type: "dismiss"; text: string }
  | { type: "failed"; error: string }
  | { type: "unsent" }
  | { type: "stopped" }
  | { type: "reset" };

export function plainTurn(turn: Turn): LessonTurn {
  return { session: turn.session, ms: turn.ms, learner: turn.learner, tutor: turn.tutor, interrupted: turn.interrupted };
}

export function tutorText(live: Live): string {
  return [...live.done, live.streaming].filter((part) => part.trim() !== "").join("\n");
}

export function unanswered(live: Live | null, ended: { ok: boolean; interrupted: boolean }): live is Live {
  return live !== null && !ended.ok && !ended.interrupted && tutorText(live) === "" && live.learner.trim() !== "";
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
    ...(live.closing ? { closing: live.closing } : {}),
    ...(live.step ? { step: live.step } : {}),
  };
}

const waitingOut = (text: string) => /waiting to try again/i.test(text);

function limitNotice(old: Notice, next: Notice): Notice {
  if (waitingOut(old.text)) return next;
  if (waitingOut(next.text)) return old;
  const resets = (text: string) => /\bresets?\b/i.test(text);
  const base = resets(old.text) ? old : next;
  if (/send your message again/i.test(base.text)) return base;
  return { ...base, text: `${base.text.trim()} ${resets(base.text) ? "Send your message again once it resets." : "Send your message again later."}` };
}

const aboutTheTurn = (notice: Notice) => notice.kind === "turn" || notice.kind === "retry" || notice.kind === "usage_limit" || notice.kind === "model_unavailable";

function addNotice(notices: Notice[], notice: Notice): Notice[] {
  if (notice.kind === "usage_limit") {
    const at = notices.findIndex((n) => n.kind === "usage_limit");
    if (at !== -1) return notices.map((n, i) => (i === at ? limitNotice(n, notice) : n));
  }
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
      if (live.done.includes(event.text)) return { ...state, live: { ...live, streaming: "", status: "writing", doing: null } };
      return { ...state, live: { ...live, done: [...live.done, event.text], streaming: "", status: "writing", doing: null } };
    case "working":
      return { ...state, live: { ...live, status: "working", doing: event.doing ?? live.doing } };
    case "permission":
      return {
        ...state,
        cards: [
          ...state.cards.filter((card) => card.id !== event.id),
          {
            kind: "permission",
            id: event.id,
            title: event.title,
            detail: event.detail,
            exact: event.exact ?? null,
            canAlways: event.can_always,
            always: event.always ?? null,
          },
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
      const done = live.streaming.trim() !== "" ? [...live.done, live.streaming] : live.done;
      return {
        ...state,
        cards: [],
        live: { ...live, done, streaming: "", status: "idle", error: event.error, interrupted: event.interrupted },
        notices: event.error ? addNotice(state.notices.filter((n) => !waitingOut(n.text)), { kind: "turn", text: event.error }) : state.notices.filter((n) => !waitingOut(n.text)),
      };
    }
    case "notice":
      return { ...state, notices: addNotice(state.notices, { kind: event.kind, text: event.text }) };
    case "exited": {
      const ended = { ...state, open: false, cards: [], live: state.live ? { ...live, status: "idle" as Status } : null };
      if (event.expected) return ended;
      const text = `The tutor’s program stopped. ${event.detail ?? ""} Send your message again to restart it.`.replace(/\s+/g, " ");
      return { ...ended, notices: addNotice(state.notices, { kind: "turn", text }) };
    }
    case "marked": {
      if (!state.live) return state;
      const marks = [...(state.live.marks ?? []).filter((mark) => mark.id !== event.mark.id), event.mark];
      return { ...state, live: { ...state.live, marks } };
    }
    case "closing":
      return state.live ? { ...state, live: { ...state.live, closing: event.lines } } : state;
    case "step_begun":
      return state.live ? { ...state, live: { ...state.live, step: { n: event.n, of: event.of, name: event.name } } } : state;
    case "plan_changed":
    case "settled":
      return state;
    case "open_task": {
      const task = event.fallback ? { text: event.task, fallback: true } : { text: event.task };
      if (!event.posed || !event.task || !state.live) return { ...state, task };
      return { ...state, task, live: { ...state.live, posed: event.task } };
    }
  }
}

export function reduce(state: SessionState, action: Action): SessionState {
  switch (action.type) {
    case "opened": {
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
              ...(last.closing ? { closing: last.closing } : {}),
              ...(last.step ? { step: last.step } : {}),
            }
          : null,
      };
    }
    case "reopened":
      return { ...state, open: true };
    case "released":
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
        notices: state.notices.filter((n) => !aboutTheTurn(n)),
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
    case "unsent":
      return { ...state, live: null };
    case "stopped":
      return { ...emptySession };
    case "reset":
      return { ...emptySession };
  }
}

export function busy(state: SessionState): boolean {
  return state.live !== null && state.live.status !== "idle";
}

export function openingWords(last: { stoppedAt: string | null; studiedBefore: boolean; title: string }, skipChecks: boolean): string {
  if (skipChecks) return "No checks right now. Let's continue where I stopped.";
  if (last.stoppedAt) return `I'm back. Let's continue. I stopped at: ${last.stoppedAt}`;
  if (last.studiedBefore) return "I'm back. Let's continue.";
  return `Let's work on ${last.title}.`;
}

export const CHECKS_WORDS = "Just the quick checks.";

const OPENING = /^(?:I'm back\. Let's continue\.(?: I stopped at: ([\s\S]+))?|No checks right now\. Let's continue where I stopped\.|Just the quick checks\.|Let's work on ([\s\S]+)\.)$/;

export function opening(text: string): { stoppedAt: string | null; topic: string | null } | null {
  const found = OPENING.exec(text.trim());
  return found ? { stoppedAt: found[1]?.trim() || null, topic: found[2]?.trim() || null } : null;
}

const HANDED_IN = /^Mark my /;

export function saidByTheWindow(text: string): boolean {
  return opening(text) !== null || HANDED_IN.test(text.trim());
}

export const HELP = { hint: "Can I have a hint?", unknown: "I don't know", lost: "I'm lost" } as const;

export function withQuote(quote: string, words: string): string {
  return `> ${quote}\n\n${words}`;
}

export function splitQuote(text: string): { quote: string | null; text: string } {
  const found = /^> ([^\n]+)\n\n/.exec(text);
  return found ? { quote: found[1], text: text.slice(found[0].length) } : { quote: null, text };
}

export function splitConfidence(text: string): { text: string; confidence: "Sure" | "Unsure" | "Guess" | null } {
  const found = /(?:^|\n\n)Confidence: (sure|unsure|guess)(?=\n\n|$)/i.exec(text);
  if (!found) return { text, confidence: null };
  const word = found[1].toLowerCase();
  const rest = `${text.slice(0, found.index)}${text.slice(found.index + found[0].length)}`.trim();
  return { text: rest, confidence: word === "sure" ? "Sure" : word === "unsure" ? "Unsure" : "Guess" };
}

export function isHint(text: string): boolean {
  return text.trim() === HELP.hint;
}

export function isHelp(text: string): boolean {
  return (Object.values(HELP) as string[]).includes(splitConfidence(text).text.trim());
}

export function pickedWords(answers: Answer[]): string {
  return answers
    .map((answer) => answer.chosen.join(", "))
    .filter((line) => line.trim() !== "")
    .join("\n");
}

export function attachedWords(paths: string[], working: boolean): string {
  const list = paths.join(", ");
  const photos = paths.every((path) => !/\.pdf$/i.test(path));
  if (working) return `${paths.length === 1 ? (photos ? "Photo" : "File") : photos ? "Photos" : "Files"} of my working: ${list}`;
  return paths.length === 1
    ? `A file for you to read: ${list}. It may be my working or material I’m learning from.`
    : `Files for you to read: ${list}. They may be my working or material I’m learning from.`;
}

const ATTACHED = /(?:^|\n\n)(?:(?:Photos?|Files?) of my working: (.+)|(?:A file|Files) for you to read: (.+?)\. (?:It|They) may be my working or (?:course material|material I’m learning from)\.)\s*$/;

export function splitAttached(text: string): { text: string; files: string[]; working: boolean } {
  const found = ATTACHED.exec(text);
  if (!found) return { text, files: [], working: false };
  const list = (found[1] ?? found[2] ?? "").split(", ").map((each) => each.trim()).filter(Boolean);
  return { text: text.slice(0, found.index).trim(), files: list, working: found[1] !== undefined };
}
