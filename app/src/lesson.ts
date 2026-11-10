// What the session screen holds, and how each event from the tutor changes it.

import type { AgentEvent, NoticeKind, Question, Turn } from "./types";

export type Card =
  | { kind: "permission"; id: string; title: string; detail: string; canAlways: boolean }
  | { kind: "question"; id: string; questions: Question[] };

export type Status = "idle" | "waiting" | "writing" | "working";

/** The exchange on screen: what the learner said and the tutor's reply to it. */
export type Live = {
  learner: string;
  done: string[];
  streaming: string;
  status: Status;
  since: number;
  error: string | null;
  interrupted: boolean;
};

export type Notice = { kind: NoticeKind; text: string };

export type SessionState = {
  /** Whether an agent process is running for this folder. */
  open: boolean;
  earlier: Turn[];
  live: Live | null;
  cards: Card[];
  notices: Notice[];
  /** When this sitting's clock started, or null before the first message. */
  startedMs: number | null;
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
  | { type: "sent"; shown: string; now: number }
  | { type: "event"; event: AgentEvent }
  | { type: "note"; notice: Notice }
  | { type: "dismiss"; text: string }
  | { type: "failed"; error: string }
  | { type: "stopped" }
  | { type: "reset" };

export function tutorText(live: Live): string {
  return [...live.done, live.streaming].filter((part) => part.trim() !== "").join("\n\n");
}

function blankLive(learner: string, now: number, status: Status): Live {
  return { learner, done: [], streaming: "", status, since: now, error: null, interrupted: false };
}

function asTurn(live: Live): Turn | null {
  const tutor = tutorText(live);
  if (tutor === "" && live.learner.trim() === "") return null;
  return { session: "", ms: live.since, learner: live.learner, tutor, interrupted: live.interrupted };
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
      return { ...state, live: { ...live, streaming: live.streaming + event.text, status: "writing" } };
    case "text_done":
      return { ...state, live: { ...live, done: [...live.done, event.text], streaming: "", status: "writing" } };
    case "working":
      return { ...state, live: { ...live, status: "working" } };
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
        live: blankLive(action.shown, action.now, "waiting"),
        cards: [],
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
