// One session, in its own tab: the one-surface lesson of #67, as it was.
//
// A session not yet begun shows the next thing, already chosen, and the line
// to write on; Return starts it. A session begun fills the same surface: the
// tutor's words in the reading face, the learner's in the face they type in,
// an answer written inside the task it answers, the record's lines small and
// in ink, and one line at the foot. An earlier session opens with its saved
// turns and no tutor running; the first thing written continues it.
//
// The window holds one tutor's program at a time. The tab that holds it is
// "live": the tutor's events come to it. Writing in another tab makes that
// one live, after any reply under way has finished.

import { forwardRef, useCallback, useEffect, useImperativeHandle, useLayoutEffect, useMemo, useReducer, useRef, useState } from "react";

import { busy as isBusyNow, emptySession, isHelp, openingWords, opening, pickedWords, reduce, tutorText, type LessonTurn, type Said, type SessionState } from "../lesson";
import { keyName, keepPlace, placeKey as keyOf, placeOf, type Prefs } from "../prefs";
import { hasTask, poses } from "../render";
import type { SourceRef } from "../sources";
import type { AgentEvent, Answer, Core, Decision, Home, Plan, Setup, Turn as SavedTurn } from "../types";
import { displayName, firstWords, markWords, planDiff, shortDay, withoutPaths, type PlanDiff } from "../words";
import { Composer, type ComposerHandle, type Sending } from "./Composer";
import { Cards, Notices, Turn, Working } from "./Conversation";
import { PlanPanel } from "./PlanPanel";
import { Line } from "./Rich";

/** What a session tab starts with, once, when it is opened. */
export type Pending =
  /** Carry on where the learner stopped: the window's opening words. */
  | { kind: "continue" }
  /** Start the plan's next thing, in a session not yet begun. */
  | { kind: "start" }
  /** Ask the tutor to mark a paper that was handed in. */
  | { kind: "paper"; shown: string; sent: string };

export type SessionTab = { key: string; kind: "session"; folder: string; session: string | null; title: string; pending?: Pending };

/** What the window can ask of a session from outside it. */
export type SessionHandle = {
  event: (event: AgentEvent) => void;
  /** A menu item or a key: "hint", "lost", "unknown", "symbols", "attach", "photo", "stop-reply". */
  command: (command: string) => boolean;
  /** Esc: step back. Whether it did something. */
  escape: () => boolean;
  drop: (paths: string[]) => void;
  /** Before the tab closes: end the sitting where it is, if its tutor runs. */
  close: () => Promise<void>;
  /** Another tab took the tutor's program. */
  release: () => void;
  state: () => SessionState;
  say: (text: string, said?: Said) => Promise<void>;
  start: (words?: string) => Promise<boolean>;
  taskOpen: () => boolean;
  busy: () => boolean;
};

type Props = {
  tab: SessionTab;
  /** This tab is in front. */
  active: boolean;
  /** This tab holds the tutor's program. */
  live: boolean;
  api: Core;
  setup: Setup | null;
  /** The look at this Mac under way at startup, for a start that comes before it is done. */
  setupWait: Promise<Setup> | null;
  prefs: Prefs;
  /** The plan is shown beside the lesson. */
  panel: boolean;
  /** A layer is over the window: the line does not take the focus. */
  covered: boolean;
  /** The tutor is writing in another tab: a message here waits for it. */
  elsewhereBusy: boolean;
  claim: (key: string) => void;
  register: (key: string, handle: SessionHandle | null) => void;
  onSession: (key: string, session: string) => void;
  onTitle: (key: string, title: string) => void;
  onBusy: (key: string, busy: boolean) => void;
  onEngine: () => void;
  onSource: (folder: string, source: SourceRef) => void;
  onTaskChange: (key: string, open: boolean) => void;
  /** Done with a pending start. */
  onStarted: (key: string) => void;
};

/** Lessons start only when this Mac is set up: the program is usable and Claude Code is signed in. */
export function setupReady(setup: Setup | null): boolean {
  if (!setup) return false;
  const runtime = setup.runtime.state === "ready" || setup.runtime.state === "kept";
  return runtime && setup.account.installed && setup.account.signed_in;
}

/** The turns of a sitting, from the last time the window's opening words were sent. */
function sittingStartOf(turns: SavedTurn[]): number {
  for (let i = turns.length - 1; i >= 0; i--) if (opening(turns[i].learner)) return i;
  return 0;
}

/**
 * Where the open task is: the last reply that set one, unless an answer was
 * given after it, the reply was stopped part way, or a new sitting began
 * since. -1 when no task is open.
 */
export function openTaskAt(turns: LessonTurn[]): number {
  for (let i = turns.length - 1; i >= 0; i--) {
    const turn = turns[i];
    if (hasTask(turn.tutor)) return turn.interrupted ? -1 : i;
    if (turn.said === "answer") return -1;
    if (turn.said === "opening" || opening(turn.learner)) return -1;
  }
  return -1;
}

/** The open task: its words, and the turn whose reply posed it (-1 when that turn is not in the list). */
export type OpenTask = { text: string; at: number };

/**
 * The task open now. An engine that knows it (Magnesium's) says so after each
 * turn, in the tutor's own words; otherwise a task is found by the bold word
 * a reply opens it with.
 */
export function openTaskOf(session: SessionState, turns: LessonTurn[]): OpenTask | null {
  if (session.task !== undefined) {
    const text = session.task.text;
    if (!text) return null;
    // The reply that posed it last: the engine says so, or the reply holds
    // nearly all of its words, as a tutor does when it poses it again.
    for (let i = turns.length - 1; i >= 0; i--) if (turns[i].posed === text || poses(turns[i].tutor, text)) return { text, at: i };
    return { text, at: -1 };
  }
  const at = openTaskAt(turns);
  return at >= 0 ? { text: taskText(turns[at].tutor), at } : null;
}

/**
 * Why the record kept no mark for an answer, in the record's terms. Only a
 * first try at a task is marked, so a second answer to the same task is not.
 */
function unmarkedWhy(session: SessionState, since: number): string | null {
  const turns = [...session.earlier, ...(session.live ? [session.live] : [])] as { since?: number; ms?: number; said?: Said; question?: string; learner: string }[];
  const at = turns.findIndex((turn) => (turn.since ?? turn.ms) === since);
  const question = at >= 0 ? turns[at].question : undefined;
  const earlier = question !== undefined && turns.slice(0, Math.max(0, at)).some((turn) => turn.said === "answer" && turn.question === question);
  // Another try is not a first try, and only a first try is marked: nothing to say.
  return earlier ? null : "The record kept no mark for this answer. The workspace page lists what it holds.";
}

/** The task's own words in a reply: from its bold opening word to the end. */
function taskText(reply: string): string {
  const found = [...reply.matchAll(/\*\*(?:task|check|quick check|question|your turn|try it|try this|redo|paper redo|now you)\b/gi)].pop();
  return found?.index !== undefined ? reply.slice(found.index) : reply;
}

/** What a session not yet begun offers: the next thing, and how to start it. */
type Rest = {
  course: string | null;
  next: string;
  where: string | null;
  /** Typing on the line starts a lesson with those words. */
  line: boolean;
  /** What Return does with an empty line, and a click on the next thing. */
  start: (() => void) | null;
  choices: { label: string; run: () => void }[];
  keys: string;
  /** Start without what is due from earlier: it stays due. */
  skip: (() => void) | null;
};

function Choices({ choices, focus }: { choices: Rest["choices"]; focus: boolean }) {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  useEffect(() => {
    if (focus) refs.current[0]?.focus({ preventScroll: true });
  }, [choices.length, focus]);
  return (
    <div className="choices-rest" role="group">
      {choices.map((choice, at) => (
        <button
          key={choice.label}
          ref={(el) => (refs.current[at] = el)}
          className="choice-rest"
          onClick={choice.run}
          onKeyDown={(event) => {
            const step = event.key === "ArrowDown" ? 1 : event.key === "ArrowUp" ? -1 : 0;
            if (step === 0) return;
            event.preventDefault();
            refs.current[(at + step + choices.length) % choices.length]?.focus();
          }}
        >
          {choice.label}
        </button>
      ))}
    </div>
  );
}

/** The exchange on screen as a turn, or null before the first message. */
function liveTurn(session: SessionState): LessonTurn | null {
  const live = session.live;
  if (!live) return null;
  return {
    session: "",
    ms: live.since,
    learner: live.learner,
    tutor: tutorText(live),
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

export const Session = forwardRef<HTMLDivElement, Props>(function Session(props, outer) {
  const { tab, api } = props;
  const folder = tab.folder;
  const [phase, setPhase] = useState<"loading" | "rest" | "lesson">("loading");
  const [home, setHome] = useState<Home | null>(null);
  const [plan, setPlan] = useState<Plan | null>(null);
  const [session, dispatch] = useReducer(reduce, emptySession);
  const [sittingStart, setSittingStart] = useState(0);
  const [sittingTitle, setSittingTitle] = useState<string>(tab.title);
  const [line, setLineState] = useState<"task" | "talk">("talk");
  const [notes, setNotes] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  // What the tutor changed in the plan in this sitting, for the panel.
  const [changed, setChanged] = useState<PlanDiff | null>(null);
  const [draft, setDraft] = useState("");
  const [answerDraft, setAnswerDraft] = useState("");
  const [painted, setPainted] = useState(false);
  // The lesson's heading has scrolled away: the top edge names the lesson and its step.
  const [headGone, setHeadGone] = useState(false);
  // What the first question from earlier is about, and the day it was last answered, from the record.
  const [dueTopic, setDueTopic] = useState<string | null>(null);
  // The record's latest fact (a mark, a change to the plan, no mark), kept in view at the foot until the next message.
  const [fact, setFact] = useState<{ kind: "mark" | "plan" | "none"; text: string; title?: string } | null>(null);
  const surface = useRef<HTMLElement>(null);
  const nowRef = useRef<HTMLElement>(null);
  const box = useRef<HTMLDivElement>(null);
  const lineRef = useRef<ComposerHandle>(null);
  const stuck = useRef(true);
  const restoring = useRef<number | null>(null);
  // The exchange whose finished reply last placed the learner's line.
  const settled = useRef<number | null>(null);
  const started = useRef(false);

  // Where this session's draft, line and scroll are kept: its conversation once it has one.
  const place = keyOf(folder, tab.session, tab.key);

  // The newest values, for handlers that outlive a render.
  const now = useRef({ props, phase, home, plan, session, line, place, task: null as OpenTask | null });
  now.current = { props, phase, home, plan, session, line, place, task: now.current?.task ?? null };

  useImperativeHandle(outer, () => box.current!);

  const note = useCallback((said: string) => {
    const text = withoutPaths(said.replace(/^Error: /, ""));
    if (now.current.phase === "lesson") dispatch({ type: "note", notice: { kind: "other", text } });
    else setNotes((old) => [...old.filter((n) => n !== text), text].slice(-3));
  }, []);

  const setLine = useCallback((where: "task" | "talk") => {
    setLineState(where);
    keepPlace(now.current.place, { line: where });
  }, []);

  // ---------------------------------------------------------------------------
  // Reading

  const refreshHome = useCallback(async (): Promise<Home | null> => {
    try {
      const fresh = await api.home(folder);
      now.current.home = fresh;
      setHome(fresh);
      return fresh;
    } catch (problem) {
      setError(String(problem));
      return null;
    }
  }, [api, folder]);

  const refreshPlan = useCallback(async (): Promise<Plan | null> => {
    try {
      const fresh = await api.plan(folder);
      now.current.plan = fresh;
      setPlan(fresh);
      return fresh;
    } catch {
      return null;
    }
  }, [api, folder]);

  /** The skill of the first question due, from the record, and when it was last answered. */
  const readDueTopic = useCallback(async () => {
    try {
      const [progress, marks] = await Promise.all([api.progress(null, folder), api.marks(folder).catch(() => null)]);
      const row = [...progress.in_progress, ...progress.confirmed].find((each) => each.due);
      if (!row) return;
      const last = [...(marks ?? [])].reverse().find((mark) => mark.label.trim() === row.name.trim());
      setDueTopic(last ? `${row.name}, from ${shortDay(last.at)}` : row.name);
    } catch {
      // Without the record the line names no topic.
    }
  }, [api, folder]);

  // Opened: the folder, the plan, and the session's saved turns, read without starting its tutor.
  useEffect(() => {
    let alive = true;
    void (async () => {
      const opened = await refreshHome();
      if (!alive) return;
      if (opened && (opened.checks_due ?? 0) + opened.redos_due > 0) void readDueTopic();
      const planning = refreshPlan();
      const kept = placeOf(place);
      setDraft(kept.draft);
      setAnswerDraft(kept.answerDraft);
      if (tab.session) {
        const turns = await api.lessonTurns(folder, tab.session).catch(() => [] as SavedTurn[]);
        if (!alive) return;
        dispatch({ type: "shown", turns });
        // The task that was open is the open task again, with the line in it if it was there.
        if (kept.task) dispatch({ type: "event", event: { type: "open_task", task: kept.task, posed: false } });
        if (turns.length > 0) settled.current = turns[turns.length - 1].ms;
        setSittingStart(sittingStartOf(turns));
        if (!tab.title && opened?.last?.goal) setSittingTitle(opened.last.goal);
        setLineState(kept.line);
        restoring.current = kept.fromFoot;
        setPhase("lesson");
      } else {
        // The next thing is drawn at once; with no lesson to carry on, it waits for the plan's next item.
        await planning;
        if (!alive) return;
        setPhase("rest");
      }
    })();
    return () => {
      alive = false;
    };
    // Read once, when the tab opens.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ---------------------------------------------------------------------------
  // The tutor's events, when this tab holds its program

  const onEvent = (event: AgentEvent) => {
    dispatch({ type: "event", event });
    const { props } = now.current;
    if (event.type === "ready") {
      if (event.plugin_missing) void api.interrupt().catch(() => {});
      if (event.session_id && event.session_id !== props.tab.session) props.onSession(props.tab.key, event.session_id);
    }
    if (event.type === "turn_started") props.onBusy(props.tab.key, true);
    if (event.type === "turn_ended" || event.type === "exited") props.onBusy(props.tab.key, false);
    if (event.type === "turn_ended") void refreshHome();
    if (event.type === "marked") setFact({ kind: "mark", text: markWords(event.mark) });
    // The engine says which task is open: a task the reply just posed takes
    // the learner's line; after help with it, the line goes back in; after
    // anything else it is on the conversation's line.
    if (event.type === "open_task" && now.current.phase === "lesson") {
      const live = now.current.session.live;
      const reply = live ? tutorText(live) : "";
      if (event.task && (event.posed || poses(reply, event.task))) setLine("task");
      // After help with it, or when a lesson opens with it still open, the task is the live one.
      else if (event.task && (live?.said === "help" || live?.said === "opening")) setLine("task");
      else setLine("talk");
      // An answer the record kept no mark for says why, once the record has had its moment.
      if (live && live.said === "answer") {
        const since = live.since;
        window.setTimeout(() => {
          const reason = unmarkedWhy(now.current.session, since);
          if (!reason) return;
          dispatch({ type: "unmarked", since, reason });
          const turn = now.current.session.live;
          if (turn && turn.since === since && (turn.marks ?? []).length === 0) setFact({ kind: "none", text: "no mark", title: reason });
        }, 1800);
      }
    }
    if (event.type === "plan_changed") {
      const was = now.current.plan;
      void refreshPlan().then((fresh) => {
        if (!fresh) return;
        const diff = planDiff(was, fresh);
        if (diff.changes.length === 0) return;
        dispatch({ type: "planned", change: diff });
        const moved = diff.changes.filter((each) => !each.route);
        if (moved.length > 0) setFact({ kind: "plan", text: `The plan changed: ${moved.map((each) => `${each.what}, ${each.detail}`).join("; ")}.` });
        setChanged((old) => ({
          changes: [...(old?.changes ?? []), ...diff.changes],
          moved: { ...(old?.moved ?? {}), ...diff.moved },
          examFrom: old?.examFrom ?? diff.examFrom,
        }));
      });
    }
  };

  // When a reply is whole, the line goes where the next thing is: into the
  // task the reply set, back into the task after help with it, and otherwise
  // onto the conversation's line.
  useEffect(() => {
    const live = session.live;
    if (!live || live.status !== "idle" || settled.current === live.since || phase !== "lesson" || session.task !== undefined) return;
    settled.current = live.since;
    const reply = tutorText(live);
    if (reply === "" && !live.error) return;
    const turns = [...session.earlier, liveTurn(session)!];
    const open = openTaskAt(turns);
    if (hasTask(reply) && !live.interrupted) setLine("task");
    else if (live.said === "help" && open >= 0) setLine("task");
    else setLine("talk");
  }, [session, phase, setLine]);

  // ---------------------------------------------------------------------------
  // Lessons

  /** Fill the surface with a lesson, and send its first words. */
  const begin = async (resume: boolean, shown: string, said: Said, title: string, send: (api: Core) => Promise<void>): Promise<boolean> => {
    // Return pressed before the look at this Mac is done waits for it.
    const { props } = now.current;
    let setup = props.setup;
    if (!setup && props.setupWait) setup = await props.setupWait.catch(() => null);
    if (!setupReady(setup)) {
      setError("This Mac is not set up for lessons yet. Home says what it needs.");
      return false;
    }
    setError(null);
    setNotes([]);
    setSittingTitle(title);
    props.onTitle(props.tab.key, title);
    setChanged(null);
    stuck.current = true;
    setPhase("lesson");
    keepPlace(now.current.place, { inLesson: true });
    props.claim(props.tab.key);
    try {
      const opened = await api.openSession(resume, { folder, session: resume ? props.tab.session : null, title });
      dispatch({ type: "opened", turns: opened.turns });
      setSittingStart(opened.turns.length);
      dispatch({ type: "sent", shown, now: Date.now(), said });
      setLineState("talk");
      await send(api);
      props.onEngine();
      return true;
    } catch (problem) {
      dispatch({ type: "failed", error: String(problem) });
      return false;
    }
  };

  /** The one next thing: carry on where the learner stopped, or start what the plan has next. */
  const start = async (words?: string, skipChecks = false): Promise<boolean> => {
    const { home, plan, props } = now.current;
    if (!home) return false;
    let current = home;
    if (home.state === "not_set_up") {
      try {
        current = await api.setUp(folder);
        setHome(current);
      } catch (problem) {
        setError(String(problem));
        return false;
      }
    }
    const continuing = props.tab.session !== null;
    const typed = words?.trim() ?? "";
    const item = plan?.items.find((each) => each.status === "doing") ?? plan?.items.find((each) => each.status === "todo") ?? null;
    if (typed !== "") {
      // Words typed first are the start: the tutor takes them as the lesson's goal.
      return begin(continuing, typed, "talk", continuing ? sittingTitle : firstWords(typed, 80), (api) => api.sendNewLesson(typed));
    }
    const title = continuing ? sittingTitle || current.last?.goal || displayName(current.name) : (item?.title ?? displayName(current.name));
    const shown = openingWords(
      { stoppedAt: continuing ? (current.last?.stopped_at ?? null) : null, studiedBefore: continuing, title: continuing ? title : (item?.title ?? displayName(current.name)) },
      skipChecks,
    );
    return begin(continuing, shown, "opening", title, (api) => api.sendNewLesson(shown));
  };

  /** Send the learner's words, with what they are. */
  const say = async (text: string, said: Said = "talk", question?: string) => {
    const { session, props } = now.current;
    try {
      if (!session.open || !props.live) {
        props.claim(props.tab.key);
        await api.openSession(true, { folder, session: props.tab.session });
        dispatch({ type: "reopened" });
        props.onEngine();
      }
      stuck.current = true;
      setFact(null);
      dispatch({ type: "sent", shown: text, now: Date.now(), said, ...(question ? { question } : {}) });
      const intent = said === "answer" ? "answer" : said === "talk" ? "talk" : undefined;
      await api.send(text, intent);
    } catch (problem) {
      dispatch({ type: "failed", error: String(problem) });
    }
  };

  const onSend = useCallback((sending: Sending) => {
    const { phase } = now.current;
    if (sending.place === "rest") {
      if (phase === "rest") void startRef.current(sending.text);
      return;
    }
    if (sending.place === "task") void sayRef.current(sending.text, isHelp(sending.text) ? "help" : "answer", now.current.task?.text);
    else void sayRef.current(sending.text, "talk");
  }, []);
  const startRef = useRef(start);
  startRef.current = start;
  const sayRef = useRef(say);
  sayRef.current = say;

  const askHelp = useCallback((words: string) => void sayRef.current(words, "help"), []);

  /** End the sitting where it is. The line saved for next time is the open task's first words. */
  const stop = async () => {
    const { session, props } = now.current;
    if (!props.live || !session.open) return;
    const turns = [...session.earlier, ...(liveTurn(session) ? [liveTurn(session)!] : [])];
    const open = openTaskOf(session, turns);
    const line = open ? firstWords(open.text) : null;
    try {
      await api.stop(line);
    } catch (problem) {
      setError(String(problem));
    }
    keepPlace(now.current.place, { inLesson: false });
  };

  const interrupt = useCallback(() => {
    if (now.current.props.live) void api.interrupt().catch((problem) => note(String(problem)));
  }, [api, note]);

  // A start the tab was opened with: carry on, the plan's next thing, or a paper to mark.
  useEffect(() => {
    const pending = tab.pending;
    if (!pending || started.current || phase === "loading") return;
    started.current = true;
    props.onStarted(tab.key);
    if (pending.kind === "continue" || pending.kind === "start") void start();
    else if (pending.kind === "paper") void begin(true, pending.shown, "talk", sittingTitle || "The paper", (api) => api.sendShown(pending.shown, pending.sent));
    // Runs once the tab has read its folder.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [phase, tab.pending]);

  // ---------------------------------------------------------------------------
  // Files: the clip, a drop on the session, a pasted image

  const onPick = useCallback(() => api.pickFiles(), [api]);
  const onAttach = useCallback(
    async (paths: string[], working: boolean) => {
      const saved: string[] = [];
      for (const path of paths) {
        try {
          saved.push(await api.attach(path, working ? "working" : "attached", folder));
        } catch (problem) {
          note(`${path.split("/").pop()}: ${String(problem).replace(/^Error: /, "")}`);
        }
      }
      return saved;
    },
    [api, folder, note],
  );
  const onPasteImage = useCallback((name: string, bytes: Uint8Array, working: boolean) => api.attachBytes(name, bytes, working ? "working" : "attached", folder), [api, folder]);

  const drop = (paths: string[]) => {
    // Photos and PDFs join the line. Anything else is course material.
    const kinds = /\.(jpe?g|png|heic|heif|tiff?|webp|bmp|pdf)$/i;
    const attach = paths.filter((path) => kinds.test(path));
    const other = paths.filter((path) => !kinds.test(path));
    if (attach.length > 0) {
      const working = now.current.line === "task" || now.current.task !== null;
      void onAttach(attach, working).then((saved) => lineRef.current?.add(saved));
    }
    if (other.length > 0) {
      void api.addMaterial(other, folder).then(
        (added) => added.lines.forEach(note),
        (problem) => note(String(problem)),
      );
    }
  };

  // ---------------------------------------------------------------------------
  // What the window can ask of this session

  const handle: SessionHandle = {
    event: (event) => onEvent(event),
    command: (command) => {
      const { phase, task } = now.current;
      if (command === "hint" || command === "lost" || command === "unknown") {
        if (phase !== "lesson") return false;
        askHelp(command === "hint" ? "Can I have a hint?" : command === "lost" ? "I'm lost" : "I don't know");
        if (task) setLine("task");
        return true;
      }
      if (command === "attach" || command === "photo") {
        lineRef.current?.pick();
        return true;
      }
      if (command === "symbols") {
        lineRef.current?.toggleSymbols();
        return true;
      }
      if (command === "stop-reply") {
        if (isBusyNow(now.current.session)) interrupt();
        return true;
      }
      return false;
    },
    escape: () => {
      const { session, phase, line } = now.current;
      if (isBusyNow(session)) {
        interrupt();
        return true;
      }
      if (phase === "lesson" && line === "task") {
        setLine("talk");
        return true;
      }
      return false;
    },
    drop,
    close: stop,
    release: () => dispatch({ type: "released" }),
    state: () => now.current.session,
    say: (text, said) => say(text, said),
    start: (words) => start(words),
    taskOpen: () => now.current.task !== null,
    busy: () => isBusyNow(now.current.session),
  };
  const handleRef = useRef(handle);
  handleRef.current = handle;
  useEffect(() => {
    const stable: SessionHandle = {
      event: (e) => handleRef.current.event(e),
      command: (c) => handleRef.current.command(c),
      escape: () => handleRef.current.escape(),
      drop: (p) => handleRef.current.drop(p),
      close: () => handleRef.current.close(),
      release: () => handleRef.current.release(),
      state: () => handleRef.current.state(),
      say: (t, s) => handleRef.current.say(t, s),
      start: (w) => handleRef.current.start(w),
      taskOpen: () => handleRef.current.taskOpen(),
      busy: () => handleRef.current.busy(),
    };
    props.register(tab.key, stable);
    return () => props.register(tab.key, null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tab.key]);

  // A tab not in front is out of reach of the keyboard and of a screen reader.
  useEffect(() => {
    box.current?.toggleAttribute("inert", !props.active);
  }, [props.active]);

  // ---------------------------------------------------------------------------
  // Scrolling: the lesson keeps its foot in view while it grows, unless the
  // learner scrolled up to read. Where they were is kept for next time.

  useLayoutEffect(() => {
    const el = surface.current;
    const section = nowRef.current;
    if (!el || !section) return;
    if (phase === "rest") {
      el.scrollTop = section.offsetTop;
      return;
    }
    if (restoring.current !== null) {
      el.scrollTop = el.scrollHeight - el.clientHeight - restoring.current;
      restoring.current = null;
    }
  }, [phase, painted]);

  useEffect(() => {
    const el = surface.current;
    const section = nowRef.current;
    if (!el || !section || phase !== "lesson") return;
    const pin = () => {
      if (stuck.current) el.scrollTop = el.scrollHeight;
    };
    const watch = new ResizeObserver(pin);
    watch.observe(section);
    pin();
    return () => watch.disconnect();
  }, [phase]);

  // The conversation left, above the first screen and out of view, is drawn after it.
  useEffect(() => {
    if (phase !== "loading" && props.active && !painted) requestAnimationFrame(() => setPainted(true));
  }, [phase, props.active, painted]);

  const onScroll = () => {
    const el = surface.current;
    if (!el || now.current.phase !== "lesson") return;
    const fromFoot = Math.max(0, el.scrollHeight - el.clientHeight - el.scrollTop);
    const head = nowRef.current?.querySelector<HTMLElement>(".head");
    if (head && nowRef.current) setHeadGone(el.scrollTop > nowRef.current.offsetTop + head.offsetTop + head.offsetHeight - 40);
    stuck.current = fromFoot < 48;
    keepPlace(now.current.place, { fromFoot: Math.round(fromFoot) });
  };

  // ---------------------------------------------------------------------------
  // What the surface shows

  const turns: LessonTurn[] = useMemo(() => {
    const live = liveTurn(session);
    return live ? [...session.earlier, live] : session.earlier;
  }, [session]);
  const task = useMemo(() => openTaskOf(session, turns), [session, turns]);
  const openAt = task?.at ?? -1;
  now.current.task = task;
  const held = phase === "lesson" && line === "task" && task !== null;
  // The task's question is said above its line unless the reply just above
  // posed it: in full while the learner is in it, else on one quiet line.
  const parked = task !== null && openAt !== turns.length - 1;
  // The open task is kept with where the learner was, so a reopened window has it as a task.
  const taskText_ = task?.text ?? null;
  useEffect(() => {
    if (phase === "lesson") keepPlace(now.current.place, { task: taskText_ });
    props.onTaskChange(tab.key, taskText_ !== null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [taskText_, phase]);
  const restated = parked ? (held ? task.text : firstWords(task.text, 160)) : null;
  const ownBusy = isBusyNow(session);
  const isBusy = ownBusy || props.elsewhereBusy;

  const commandsKey = keyName(props.prefs.keys.commands);
  const rest = restView();
  const currentStep = plan?.steps.find((step) => step.status === "doing")?.step ?? null;
  // The one action is the field's own end, in the text's colour; the window's other keys are one quiet line.
  const restVerb = rest?.start ? (/^↩ ([a-z]+(?: on)?)/.exec(rest.keys)?.[1] ?? "start") : null;
  const restOthers = rest ? (rest.start ? rest.keys.replace(/^↩ [a-z]+(?: on)?\s*/, "") : rest.keys) : "";

  function restView(): Rest | null {
    if (phase !== "rest") return null;
    const course = home ? displayName(home.name) : null;
    const base = { course, line: false, start: null, choices: [], keys: "", skip: null };
    if (!home) return { ...base, next: "Reading this workspace…", where: null };
    if (home.sitting) {
      const startedAt = new Date(home.sitting.started_ms);
      const at = `${String(startedAt.getHours()).padStart(2, "0")}:${String(startedAt.getMinutes()).padStart(2, "0")}`;
      return {
        ...base,
        next: home.sitting.name,
        where: `Started at ${at}, for ${home.sitting.minutes} minutes. Write ? where you are stuck, and S, U or G after each answer for sure, unsure or a guess. Hand it in from the workspace's page.`,
      };
    }
    if (home.state === "needs_upgrade") return { ...base, next: "This folder needs an upgrade", where: withoutPaths(`${home.message ?? ""} Its page on Home has the upgrade.`.trim()) };
    if (home.state === "no_runtime" || home.state === "unreadable") {
      return { ...base, next: displayName(home.name), where: home.message ? withoutPaths(home.message) : null, choices: [{ label: "Check again", run: () => void refreshHome() }], keys: "" };
    }
    const item = plan?.items.find((each) => each.status === "doing") ?? plan?.items.find((each) => each.status === "todo") ?? null;
    // The line says what Return opens, and nothing it cannot keep. What is
    // due from earlier is posed first, so the line says so, with no count.
    const due = (home.checks_due ?? 0) + home.redos_due > 0;
    const skip = due ? () => void start(undefined, true) : null;
    if (item) {
      return {
        ...base,
        next: item.title,
        where: due ? `First: ${dueTopic ?? "a question from earlier"}.` : item.note,
        line: true,
        start: () => void start(),
        keys: `↩ start   ${commandsKey} commands`,
        skip,
      };
    }
    return {
      ...base,
      next: "A new session",
      where: "Say what it is about. The tutor takes it as the goal, or drop your notes and slides on this window.",
      line: true,
      keys: "↩ start",
      skip,
    };
  }

  /** Return on the rest line: the next thing, or the words typed as the start. */
  const restSend = (sending: Sending) => {
    if (sending.text.trim() !== "" || rest?.start) onSend(sending);
  };

  const onHold = useCallback(() => setLine("task"), [setLine]);
  const onDismiss = useCallback((text: string) => dispatch({ type: "dismiss", text }), []);
  const onDecide = useCallback(
    (id: string, decision: Decision) => {
      void api.decide(id, decision).then(
        () => dispatch({ type: "event", event: { type: "withdrawn", id } }),
        (problem) => note(String(problem)),
      );
    },
    [api, note],
  );
  const onAnswer = useCallback(
    (id: string, answers: Answer[]) => {
      dispatch({ type: "picked", shown: pickedWords(answers), now: Date.now() });
      void api.answer(id, answers).then(
        () => dispatch({ type: "event", event: { type: "withdrawn", id } }),
        (problem) => note(String(problem)),
      );
    },
    [api, note],
  );
  const onSource = useCallback((source: SourceRef) => now.current.props.onSource(folder, source), [folder]);
  const stopReply = useCallback(() => interrupt(), [interrupt]);
  // A session not running, with nothing on its line: Return carries on where the learner stopped.
  const carryOn = useCallback(() => {
    const { session, phase } = now.current;
    if (phase === "lesson" && !session.open && !isBusyNow(session)) void startRef.current();
  }, []);

  const shownBefore: LessonTurn[] = phase === "lesson" ? turns.slice(Math.max(0, sittingStart - 30), sittingStart) : [];
  const sittingTurns = phase === "lesson" ? turns.slice(sittingStart) : [];
  const stopKey = keyName(props.prefs.keys["stop-reply"]) || "esc";
  const paused = props.covered || !props.active || session.cards.length > 0;
  const lineEnd = task && !held ? { keys: "↑", what: "answer" } : !session.open && phase === "lesson" && !isBusy ? { keys: "↩", what: "carry on" } : null;

  return (
    <div
      ref={box}
      className={`session${props.active ? " in-front" : ""}${props.panel ? " with-panel" : ""}`}
      data-phase={phase}
      data-busy={ownBusy ? "yes" : "no"}
      data-tab={tab.key}
      aria-hidden={props.active ? undefined : true}
    >
      {phase === "lesson" && (
        // A fixed top edge: text fades out under the tabs, and once the heading has
        // scrolled away, the lesson's name and its step are said here, in the window's quiet voice.
        <div className="top-edge" aria-hidden={!headGone}>
          {headGone && (
            <div className="inner">
              <Line text={sittingTitle} className="top-name" />
              {currentStep && <Line text={currentStep} className="top-step" />}
            </div>
          )}
        </div>
      )}
      <main className="surface" ref={surface} onScroll={onScroll}>
        <div className="column">
          {painted && shownBefore.length > 0 && (
            <div className="before" aria-label="Earlier in this conversation">
              {shownBefore.map((turn, at) => (
                <Turn
                  key={`b${turn.ms}-${at}`}
                  turn={turn}
                  open={false}
                  held={false}
                  linksOpen
                  writing={false}
                  divider={at > 0 && opening(turn.learner) !== null}
                  showQuestion={at === 0 || !(shownBefore[at - 1].posed === turn.question || hasTask(shownBefore[at - 1].tutor))}
                  onSource={onSource}
                  onNote={note}
                  onHold={onHold}
                />
              ))}
            </div>
          )}
          <section className={`now ${phase}`} ref={nowRef}>
            {phase === "loading" ? null : rest ? (
              <>
                <header className="head">
                  {rest.course && rest.course !== rest.next && <p className="course">{rest.course}</p>}
                  {rest.start ? (
                    <button className="next" data-drive="start-session" onClick={rest.start}>
                      <Line text={rest.next} />
                    </button>
                  ) : (
                    <h1 className="next">
                      <Line text={rest.next} />
                    </h1>
                  )}
                  {rest.where && (
                    <p className="where">
                      <Line text={rest.where} />
                    </p>
                  )}
                </header>
                {rest.line && home && (
                  <Composer
                    key={`rest-${place}`}
                    ref={lineRef}
                    place="rest"
                    placeKey={place}
                    initial={draft}
                    placeholder={rest.start ? "or say what you would rather do" : undefined}
                    end={restVerb ? { keys: "↩", what: restVerb } : null}
                    active
                    busy={false}
                    paused={paused}
                    canAnswer={false}
                    taskOpen={false}
                    onSend={restSend}
                    onMove={() => {}}
                    onNote={note}
                    onAttach={onAttach}
                    onPasteImage={onPasteImage}
                    onPick={onPick}
                  />
                )}
                {rest.choices.length > 0 && <Choices choices={rest.choices} focus={props.active && !props.covered} />}
                {(error || notes.length > 0) && (
                  <div className="notices">
                    {error && <p className="record note">{withoutPaths(error)}</p>}
                    {notes.map((text) => (
                      <p key={text} className="record note">
                        {text}
                      </p>
                    ))}
                  </div>
                )}
                {(restOthers || rest.skip) && (
                  <p className="keys">
                    {restOthers}
                    {rest.skip && (
                      <>
                        {restOthers ? "   " : ""}
                        <button className="plain skip" onClick={rest.skip}>
                          start without the questions from earlier
                        </button>
                      </>
                    )}
                  </p>
                )}
              </>
            ) : (
              <>
                <header className="head">
                  {home && <p className="course">{displayName(home.name)}</p>}
                  <h1 className="next">
                    <Line text={sittingTitle} />
                  </h1>
                </header>
                {sittingTurns.map((turn, at) => {
                  const index = sittingStart + at;
                  const live = index === turns.length - 1 && session.live !== null;
                  const before = index > 0 ? turns[index - 1] : null;
                  return (
                    <Turn
                      key={`${turn.ms}-${index}`}
                      turn={turn}
                      open={index === openAt}
                      held={index === openAt && held}
                      linksOpen={index !== openAt}
                      writing={live && session.live!.status !== "idle"}
                      divider={false}
                      task={index === openAt ? task?.text : undefined}
                      showQuestion={!before || !(before.posed === turn.question || hasTask(before.tutor) || (turn.question !== undefined && poses(before.tutor, turn.question)))}
                      settled={index < turns.length - 1}
                      onSource={onSource}
                      onNote={note}
                      onHold={onHold}
                    />
                  );
                })}
                {session.live && session.cards.length === 0 && <Working live={session.live} />}
                {error && <p className="record note">{withoutPaths(error)}</p>}
                <Notices notices={session.notices} onDismiss={onDismiss} />
                <Cards cards={session.cards} onDecide={onDecide} onAnswer={onAnswer} />
                {home && (
                  // The foot: one band that stays in view whatever the scroll.
                  // The record's latest fact, then the one line, whose role
                  // follows where it sits: inside the open task it is that
                  // task's answer, else it is talk.
                  <div className={`foot${held ? " in-task" : ""}`}>
                    {task && !held && (
                      <button
                        className="parked"
                        data-drive="answer"
                        onFocus={() => setLine("task")}
                        onMouseDown={(event) => {
                          event.preventDefault();
                          setLine("task");
                        }}
                      >
                        <Line text={task.text} />
                      </button>
                    )}
                    <div className={`task-line${held ? " on" : " off"}${held && !restated ? " joined" : ""}`}>
                      {held && restated && (
                        <p className="restated">
                          <Line text={restated} />
                        </p>
                      )}
                      <Composer
                        key={`line-${place}`}
                        ref={lineRef}
                        place={held ? "task" : "talk"}
                        placeKey={place}
                        initial={answerDraft || draft}
                        active
                        busy={isBusy}
                        paused={paused}
                        canAnswer={task !== null}
                        taskOpen={task !== null}
                        end={lineEnd}
                        stopKey={stopKey}
                        onSend={onSend}
                        onMove={setLine}
                        onNote={note}
                        onAttach={onAttach}
                        onPasteImage={onPasteImage}
                        onPick={onPick}
                        onHelp={askHelp}
                        onStop={ownBusy ? stopReply : undefined}
                        onEmptyReturn={!session.open ? carryOn : undefined}
                      />
                    </div>
                    {fact && (
                      <p className={`fact ${fact.kind}`} title={fact.title} data-role="fact">
                        {fact.text}
                      </p>
                    )}
                  </div>
                )}
              </>
            )}
          </section>
        </div>
      </main>
      {props.panel && <PlanPanel plan={plan} changed={changed} />}
    </div>
  );
});
