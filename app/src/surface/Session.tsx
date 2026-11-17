// One session, in its own tab: the one-surface lesson of #67, as it was.
//
// A session not yet begun shows the next thing, already chosen, and the line
// to write on; Return starts it. A session begun fills the same surface: the
// tutor's words in the reading face, the learner's in the face they type in,
// the record's lines small and in ink, and one line to write on: the page's
// last line, which rises to the foot of the window when the learner types
// while reading higher up. An earlier session opens with its saved
// turns and no tutor running; the first thing written continues it.
//
// The window holds one tutor's program at a time. The tab that holds it is
// "live": the tutor's events come to it. Writing in another tab makes that
// one live, after any reply under way has finished.

import { forwardRef, useCallback, useEffect, useImperativeHandle, useLayoutEffect, useMemo, useReducer, useRef, useState } from "react";

import { busy as isBusyNow, CHECKS_WORDS, emptySession, isHelp, openingWords, opening, pickedWords, reduce, tutorText, unanswered, type LessonTurn, type Said, type SessionState } from "../lesson";
import { marksOfTurns } from "../marks";
import { actionOf, changeOf, keepChange, keyName, keepPlace, movePlace, placeKey as keyOf, placeOf, type KeyAction, type Prefs } from "../prefs";
import { hasTask, heldAnswerGoes, poses } from "../render";
import { sentHomeDraft } from "../homeDraft";
import { setupReady } from "../setup";
import type { SourceRef } from "../sources";
import type { AgentEvent, Answer, Core, Decision, Home, Mark, Plan, Setup, Turn as SavedTurn } from "../types";
import { displayName, firstWords, goalName, markWords, namedByItsWords, planDiff, shortDay, withoutPaths, type PlanDiff } from "../words";
import { Composer, type ComposerHandle, type Sending } from "./Composer";
import { Cards, Notices, Turn, Working } from "./Conversation";
import { tabId } from "../pages/Tabs";
import { PlanPanel } from "./PlanPanel";
import { SidePanel } from "./SidePanel";
import type { DocHandle } from "./DocView";
import { docsOf, keepDocs, moveDocs, sessionDocsKey, withClosed, withOpened, type Docs } from "../docs";
import { Line, Rich } from "./Rich";
import type { LineRow } from "./LineMenu";
import { useModelPicker } from "./ModelPicker";

/** What a session tab starts with, once, when it is opened. */
export type Pending =
  /** Start the plan's next thing, in a session not yet begun. */
  | { kind: "start" }
  /**
   * Just the quick checks that are due, in a session not yet begun. `answer`: words the learner wrote on Home under
   * the first check's question (`question`, as Home showed it), to go as the answer to that check once it is posed.
   */
  | { kind: "checks"; answer?: HomeAnswer }
  /** Ask the tutor to mark a paper that was handed in. */
  | { kind: "paper"; shown: string; sent: string };

/**
 * How the page says that Home's answer was put on the line because the check posed is not the one Home showed: under
 * the line ("under"), in the lesson under the reply ("lesson"), or over the line with Home's question ("question").
 * Three options photographed for Sam; set at build time with VITE_OTHER_CHECK_NOTE.
 */
const OTHER_CHECK_NOTE: "under" | "lesson" | "question" = ((value) => (value === "lesson" || value === "question" ? value : "under"))(
  import.meta.env.VITE_OTHER_CHECK_NOTE as string | undefined,
);

/** Words written on Home under the first quick check's question, and that question as Home showed it. */
export type HomeAnswer = { words: string; question: string; id: string | null };

export type SessionTab = { key: string; kind: "session"; folder: string; session: string | null; title: string; pending?: Pending };

/**
 * The page while a task is open, the tutor is not writing and no card waits:
 * "folded" to the task and the line, or "open" on the whole lesson. Null at
 * any other time.
 */
export type AnswerPage = "folded" | "open" | null;

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
  /** Just the quick checks that are due, in this session not yet begun. */
  /** `answer`: words written on Home under the first check's question, to answer it once it is posed. */
  checks: (answer?: HomeAnswer) => Promise<boolean>;
  taskOpen: () => boolean;
  /** The page with a task open: folded to the task, opened on the whole lesson, or null when there is no such page. */
  answerPage: () => AnswerPage;
  /** The document in front is closed while the page is the task. */
  docClosed: () => boolean;
  busy: () => boolean;
  /** The documents open beside the lesson, for the menu. */
  docs: () => Docs;
  /** Where the learner is, for the evidence harness: the phase, where the line is ("end", "raised" over the page, or "away" with the end out of view), the open task. */
  probe: () => { phase: string; line: "end" | "raised" | "away"; task: string | null; open: boolean; busy: boolean; page: AnswerPage };
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
  /** The panel (the plan and this session's documents) is shown beside the lesson. */
  panel: boolean;
  /** The panel's width, in pixels, as the learner dragged it. */
  panelWidth: number;
  onPanelWidth: (width: number) => void;
  /** Show the panel: a document was opened. */
  onShowPanel: () => void;
  /** The documents changed: the menu follows. */
  onDocsChange: () => void;
  /** A layer is over the window: the line does not take the focus. */
  covered: boolean;
  /** The tutor is writing in another tab: a message here waits for it. */
  elsewhereBusy: boolean;
  claim: (key: string) => void;
  register: (key: string, handle: SessionHandle | null) => void;
  onSession: (key: string, session: string) => void;
  onTitle: (key: string, title: string) => void;
  onBusy: (key: string, busy: boolean) => void;
  /** A card waits on the learner (a permission, a question): the tutor is not writing meanwhile. */
  onAsking: (key: string, asking: boolean) => void;
  onEngine: () => void;
  /** Start Claude Code's own sign-in. */
  onSignIn: () => void;
  onTaskChange: (key: string, open: boolean) => void;
  /** The page with a task open changed: the menu's item follows. */
  onAnswerPage: (key: string, page: AnswerPage) => void;
  /** Done with a pending start. */
  onStarted: (key: string) => void;
};

export { setupReady };

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
 * The task open now. The record says so after each turn, in the tutor's own
 * words: Magnesium's engine keeps it, and on Claude Code's the window reads
 * it from the runtime. A task is found by the bold word a reply opens it
 * with when the runtime is too old to say, and on Claude Code's engine when
 * the record names none: its tutor can set a task and not save it.
 */
export function openTaskOf(session: SessionState, turns: LessonTurn[]): OpenTask | null {
  if (session.task !== undefined && !(session.task.text === null && session.task.fallback)) {
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
 * Whether a task may be open that the page cannot see. The engine's word that none is open settles it. Claude
 * Code's engine reads the tutor's record, which names only a task the tutor saved: with none named, or no lesson
 * in the record yet, the tutor may still have set one.
 */
export function taskMayBeOpen(session: SessionState): boolean {
  return session.task === undefined || session.task.fallback === true;
}

/**
 * A session's kept turns as the lesson draws them: how each message was
 * sent, the task it answered, the task a reply posed, the record's marks
 * (the rows the window saw appear for the turn's message; for a turn kept
 * without them, rows whose time falls between the turn before and this one),
 * and the plan card the window read after a turn. Nothing is read from the tutor's words.
 */
export function rebuild(saved: SavedTurn[], marks: Mark[] | null, session: string | null): LessonTurn[] {
  const saids = saved.map((turn): Said | undefined => turn.intent ?? (opening(turn.learner) ? "opening" : isHelp(turn.learner) ? "help" : undefined));
  const owned = marksOfTurns(saved, marks ?? [], (at) => saids[at] === "answer" || saids[at] === "help" || saids[at] === undefined);
  const out: LessonTurn[] = [];
  let open: string | null = null;
  saved.forEach((turn, at) => {
    const said = saids[at];
    const lesson: LessonTurn = { session: turn.session, ms: turn.ms, learner: turn.learner, tutor: turn.tutor, interrupted: turn.interrupted, ...(said ? { said } : {}) };
    if (said === "answer" && open) lesson.question = open;
    if (turn.noted && turn.posed && turn.task) lesson.posed = turn.task;
    if (owned[at].length > 0) lesson.marks = owned[at];
    if (turn.closing && turn.closing.length > 0) lesson.closing = turn.closing;
    const change = session ? changeOf(session, turn.learner, turn.tutor) : null;
    if (change) lesson.change = change as PlanDiff;
    if (turn.noted) open = turn.task ?? null;
    out.push(lesson);
  });
  return out;
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
    ...(live.closing ? { closing: live.closing } : {}),
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
  // The line is up at the foot of the window, over the page: the learner began writing while reading higher up.
  const [raised, setRaisedState] = useState(false);
  // The page's end is in view, and the line with it.
  const [atEnd, setAtEnd] = useState(true);
  const [notes, setNotes] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  // What the tutor changed in the plan in this sitting, for the panel.
  const [changed, setChanged] = useState<PlanDiff | null>(null);
  const [draft, setDraft] = useState("");
  const [painted, setPainted] = useState(false);
  // The lesson's heading has scrolled away: the top edge names the lesson and its step.
  const [headGone, setHeadGone] = useState(false);
  // Scrolled from the top: only then does the top edge fade what passes under the tabs.
  const [scrolled, setScrolled] = useState(false);
  // What the first question from earlier is about, and the day it was last answered, from the record.
  const [dueTopic, setDueTopic] = useState<string | null>(null);
  // The record's latest fact (a mark, a change to the plan), kept in view at the foot until the next message.
  const [fact, setFact] = useState<{ kind: "mark" | "plan"; text: string; title?: string; since: number } | null>(null);
  // The fact's own line is in view: the foot does not repeat it.
  const [factSeen, setFactSeen] = useState(true);
  // A reply came in this window and the learner has not written since: the page stays whole, so the reply is read
  // where it was written. Writing on the line turns the page to the task. A session opened from its kept turns has
  // nothing new to read, so its open task is the page at once.
  const [reading, setReading] = useState(false);
  // The learner opened the whole lesson over the task (⌘↑, or "the lesson so far"): it stays open until they fold it,
  // or the next reply comes. Nothing of it goes to the runtime or the record.
  const [lookedBack, setLookedBack] = useState(false);
  // From a turn's start until the window has heard which task is open after it. On Claude Code's engine that is read
  // from the record after the reply has ended, and until then the task already answered was still the open one.
  const [settling, setSettling] = useState(false);
  const surface = useRef<HTMLElement>(null);
  const nowRef = useRef<HTMLElement>(null);
  const box = useRef<HTMLDivElement>(null);
  const lineRef = useRef<ComposerHandle>(null);
  /** The last message sent from the line, as it was sent. */
  const fromLine = useRef<string | null>(null);
  // The model the session runs on, or starts on: More's Model row names it, on the first line and in the lesson.
  const modelAt = useMemo(() => ({ folder, session: props.tab.session, live: props.live }), [folder, props.tab.session, props.live]);
  const picker = useModelPicker({ api, at: modelAt, onClose: () => lineRef.current?.focus() });
  const stuck = useRef(true);
  // Where the lesson was scrolled at the last scroll event: only a move up lets it go of its foot.
  const lastTop = useRef(0);
  const restoring = useRef<number | null>(null);
  const started = useRef(false);
  // Words written on Home as the answer to the first quick check, held until the reply that poses it has ended and the
  // window knows which task is open. They are kept here, not on the line: the line is drawn again, empty, once the
  // session has its conversation, which happens during that reply. Meanwhile the page shows them where they will
  // stand once sent, as the turn they will be (`ms` is that turn's time), so the learner sees they were not lost.
  //
  // They go only as the answer to the check Home showed, and only if nothing else was sent first. Otherwise (the learner
  // sent something from the line, stopped the reply, the reply failed, or it posed another question) they are never
  // sent: they are put on the line as a draft. Home keeps its own copy until they are sent (src/homeDraft.ts).
  //
  // `posed`: which check the record says the reply posed, once read after it (undefined until then).
  const [homeAnswer, setHomeAnswerState] = useState<
    (HomeAnswer & { ms: number; ended: boolean; sending: boolean; reading: boolean; posed?: string | null }) | null
  >(null);
  // Home's question, when Home's words were put on the line because the check posed was another one: the page says so,
  // until the learner next sends anything.
  const [otherCheck, setOtherCheck] = useState<string | null>(null);
  // The same, at once: an effect and an event in one pass must not both act on it.
  const homeAnswerNow = useRef<typeof homeAnswer>(null);
  const setHomeAnswer = useCallback((next: typeof homeAnswer | ((old: typeof homeAnswer) => typeof homeAnswer)) => {
    const value = typeof next === "function" ? next(homeAnswerNow.current) : next;
    homeAnswerNow.current = value;
    setHomeAnswerState(value);
  }, []);
  /** Home's words withdrawn: onto the line as a draft, after anything being sent from it has gone. */
  const withdrawHomeAnswer = useCallback((another = false) => {
    const held = homeAnswerNow.current;
    if (!held || held.sending) return;
    setHomeAnswer(null);
    withdrawn.current = held;
    if (another) setOtherCheck(held.question);
    // A message being sent from the line is still on it until the line has been drawn again: wait for it to clear,
    // then put Home's words there. Words the learner has written since are kept, before them.
    let tries = 0;
    const put = () => {
      const line = lineRef.current;
      if (line && line.holds() && tries++ < 20) return void window.setTimeout(put, 50);
      line?.write(line.holds() ? ` ${held.words}` : held.words);
    };
    window.setTimeout(put, 0);
  }, [setHomeAnswer]);

  // Home's words last put on the line, so that sending them from it later lets Home's own copy go.
  const withdrawn = useRef<HomeAnswer | null>(null);

  // The session's name stands in until the tutor sets the lesson's goal (a start from typed words).
  const provisionalTitle = useRef(false);
  // The words typed to start it: a goal that only says them again does not name the session. Started from typed
  // words, the record's goal for the lesson is those words ("Continue").
  const typedAtStart = useRef<string>("");

  // Where this session's draft, line and scroll are kept: its conversation once it has one.
  const place = keyOf(folder, tab.session, tab.key);
  // A session just begun gets its conversation's id during its first reply. What the line holds then (words the
  // learner wrote, or Home's words put back on it) is kept under the new key: the line itself stays as it is, since it
  // is keyed by the tab, not by the place, and drawing it again empty lost those words.
  const placeWas = useRef(place);
  useEffect(() => {
    if (placeWas.current === place) return;
    movePlace(placeWas.current, place);
    placeWas.current = place;
  }, [place]);

  // The documents open in this session, kept with it: each comes back at its page and zoom.
  const docsKey = sessionDocsKey(folder, tab.session, tab.key);
  const [docs, setDocsState] = useState<Docs>(() => docsOf(docsKey));
  const [goTo, setGoTo] = useState(0);
  const [picking, setPicking] = useState(false);
  useEffect(() => {
    if (!props.active) setPicking(false);
  }, [props.active]);
  const docView = useRef<DocHandle | null>(null);
  const keptDocsKey = useRef(docsKey);
  useEffect(() => {
    // A session just begun has its conversation's id now: its documents move to it.
    if (keptDocsKey.current !== docsKey) moveDocs(keptDocsKey.current, docsKey);
    keptDocsKey.current = docsKey;
  }, [docsKey]);
  const setDocs = useCallback((next: Docs) => {
    setDocsState(next);
    docsNow.current = next;
    keepDocs(keptDocsKey.current, next);
    now.current.props.onDocsChange();
  }, []);
  const docsNow = useRef(docs);
  docsNow.current = docs;

  // The newest values, for handlers that outlive a render.
  const now = useRef({ props, phase, home, plan, session, raised, atEnd, place, lookedBack, task: null as OpenTask | null, page: null as AnswerPage });
  now.current = { props, phase, home, plan, session, raised, atEnd, place, lookedBack, task: now.current?.task ?? null, page: now.current?.page ?? null };

  useImperativeHandle(outer, () => box.current!);

  const note = useCallback((said: string) => {
    const text = withoutPaths(said.replace(/^Error: /, ""));
    // The window's own notes are about what was just done (a key link refused, a page not drawn): they go when the
    // learner writes again, as the turn's notices do.
    if (now.current.phase === "lesson") dispatch({ type: "note", notice: { kind: "turn", text } });
    else setNotes((old) => [...old.filter((n) => n !== text), text].slice(-3));
  }, []);

  const setRaised = useCallback((up: boolean) => {
    now.current.raised = up;
    setRaisedState(up);
  }, []);
  /** The learner is writing: away from the page's end, the line comes up to the foot of the window. */
  const raise = useCallback(() => {
    if (now.current.phase === "lesson" && !now.current.atEnd && !now.current.raised) setRaised(true);
  }, [setRaised]);

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
      // Words an older window kept on the task's own line are this line's draft.
      setDraft(kept.draft || kept.answerDraft);
      if (tab.session) {
        const [saved, marks] = await Promise.all([api.lessonTurns(folder, tab.session).catch(() => [] as SavedTurn[]), api.marks(folder).catch(() => null)]);
        if (!alive) return;
        // The open task: what the engine said after the last turn it noted. An engine that never says it (Claude
        // Code's) has it found in the turns, as while the lesson runs. A task kept with the place was once given here
        // as the engine's word: nothing ever replaced it, so it stayed open after it was answered, was pinned over the
        // line, and every later task was drawn as closed.
        const noted = [...saved].reverse().find((turn) => turn.noted);
        const shown = rebuild(saved, marks, tab.session);
        dispatch({ type: "shown", turns: shown, ...(noted ? { task: { text: noted.task ?? null } } : {}) });
        setSittingStart(0);
        if (!tab.title && opened?.last?.goal) setSittingTitle(opened.last.goal);
        restoring.current = kept.seenMs > 0 ? kept.fromFoot : 0;
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
    const before = now.current.session.live;
    dispatch({ type: "event", event });
    const { props } = now.current;
    // A message from the line whose reply failed before its first word (Claude Code signed out, a request refused)
    // reached no tutor: it goes back to the line, to send again with Return, unless something else was typed since.
    if (event.type === "turn_ended" && unanswered(before, event) && fromLine.current === before.learner && lineRef.current && !lineRef.current.holds()) {
      fromLine.current = null;
      dispatch({ type: "unsent" });
      lineRef.current.write(before.learner);
    }
    if (event.type === "ready") {
      if (event.plugin_missing) void api.interrupt().catch(() => {});
      if (event.session_id && event.session_id !== props.tab.session) props.onSession(props.tab.key, event.session_id);
    }
    if (event.type === "turn_started") {
      props.onBusy(props.tab.key, true);
      setReading(true);
      setLookedBack(false);
      setSettling(true);
    }
    if (event.type === "open_task" || event.type === "settled" || event.type === "exited") setSettling(false);
    // An engine that has never said which task is open (Claude Code's, when the record has no lesson for this
    // conversation) will not say after this reply either, and the page finds the task in the turns, where an answer
    // closes it at once. Waiting on "settled" there held the copy back for the 20 s the window looks for a late mark.
    if (event.type === "turn_ended" && now.current.session.task === undefined) setSettling(false);
    // The reply that was to pose the first check has ended. One stopped, or failed before its first word, may have
    // posed nothing, or not all of it: Home's words go on the line, unsent. Otherwise what it posed decides (below).
    const held = homeAnswerNow.current;
    if (held && !held.ended && !held.sending && (event.type === "turn_ended" || event.type === "exited")) {
      if (event.type === "exited" || event.interrupted || unanswered(before, event)) withdrawHomeAnswer();
      else setHomeAnswer({ ...held, ended: true });
    }
    if (event.type === "turn_ended" || event.type === "exited") props.onBusy(props.tab.key, false);
    if (event.type === "turn_ended") void refreshHome();
    if (event.type === "marked") {
      const since = now.current.session.live?.since ?? 0;
      setFact({ kind: "mark", text: markWords(event.mark), since });
    }
    // A session named before its goal was known: the goal names it once the tutor has set one.
    if (event.type === "turn_ended" && provisionalTitle.current) void nameByOwnGoal();
    if (event.type === "plan_changed") {
      const was = now.current.plan;
      void refreshPlan().then((fresh) => {
        if (!fresh) return;
        const diff = planDiff(was, fresh);
        if (diff.changes.length === 0) return;
        dispatch({ type: "planned", change: diff });
        const live = now.current.session.live;
        const sessionId = now.current.props.tab.session;
        // Kept with the turn, so a reopened session shows its plan card again.
        if (live && sessionId) keepChange(sessionId, live.learner, tutorText(live), { ...(live.change ?? {}), ...diff, changes: [...(live.change?.changes ?? []), ...diff.changes] });
        const moved = diff.changes.filter((each) => !each.route);
        if (moved.length > 0) setFact({ kind: "plan", text: `The plan changed: ${moved.map((each) => `${each.what}, ${each.detail}`).join("; ")}.`, since: live?.since ?? 0 });
        setChanged((old) => ({
          changes: [...(old?.changes ?? []), ...diff.changes],
          moved: { ...(old?.moved ?? {}), ...diff.moved },
          examFrom: old?.examFrom ?? diff.examFrom,
        }));
      });
    }
  };

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
      // A session continued keeps its kept turns under its heading, as they were drawn.
      if (!resume) dispatch({ type: "opened", turns: opened.turns });
      else dispatch({ type: "reopened" });
      setSittingStart(0);
      dispatch({ type: "sent", shown, now: Date.now(), said });
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
      // Words typed first are the start: the tutor takes them as the lesson's goal. The session is named as a
      // Return start is, by the plan's item in hand, then by a goal the tutor sets: the words typed are often
      // one like "Continue", which names nothing a learner could find again.
      if (!continuing) {
        provisionalTitle.current = true;
        typedAtStart.current = typed;
      }
      return begin(continuing, typed, "talk", continuing ? sittingTitle : (item?.title ?? displayName(current.name)), (api) => api.sendNewLesson(typed));
    }
    const title = continuing ? sittingTitle || current.last?.goal || displayName(current.name) : (item?.title ?? displayName(current.name));
    const shown = openingWords(
      { stoppedAt: continuing ? (current.last?.stopped_at ?? null) : null, studiedBefore: continuing, title: continuing ? title : (item?.title ?? displayName(current.name)) },
      skipChecks,
    );
    return begin(continuing, shown, "opening", title, (api) => api.sendNewLesson(shown));
  };

  /**
   * Just the quick checks that are due. The runtime takes the words as the ask, poses the checks alone and ends the
   * sitting after the last, as it does for the same words in a terminal.
   */
  const checks = async (answer?: HomeAnswer): Promise<boolean> => {
    if (!now.current.home) return false;
    const words = answer?.words.trim() ?? "";
    if (answer && words !== "") setHomeAnswer({ words, question: answer.question, id: answer.id, ms: Date.now(), ended: false, sending: false, reading: false });
    const begun = await begin(false, CHECKS_WORDS, "opening", "Quick checks", (api) => api.sendNewLesson(CHECKS_WORDS));
    // Not begun: nothing was posed. The words stay on Home, and go on this line as a draft.
    if (!begun) withdrawHomeAnswer();
    return begun;
  };

  /** Send the learner's words, with what they are. With nothing said of them, the tutor decides. */
  /** `at`: the turn's time, when the page already shows these words as a turn to come (Home's answer). */
  const say = async (text: string, said?: Said, question?: string, at?: number) => {
    const { session, props } = now.current;
    // Anything else sent first takes the place of Home's answer, which then waits on the line.
    if (at === undefined) withdrawHomeAnswer();
    setOtherCheck(null);
    // Home's words, sent now as Home's answer or from the line where they were put: Home's own copy goes.
    const fromHome = at !== undefined ? homeAnswerNow.current : withdrawn.current?.words === text.trim() ? withdrawn.current : null;
    try {
      if (!session.open || !props.live) {
        props.claim(props.tab.key);
        await api.openSession(true, { folder, session: props.tab.session });
        dispatch({ type: "reopened" });
        props.onEngine();
      }
      stuck.current = true;
      setFact(null);
      dispatch({ type: "sent", shown: text, now: at ?? Date.now(), ...(said ? { said } : {}), ...(question ? { question } : {}) });
      const intent = said === "answer" ? "answer" : said === "talk" ? "talk" : undefined;
      await api.send(text, intent);
      if (fromHome) {
        sentHomeDraft(folder, fromHome.question, fromHome.words);
        if (fromHome === withdrawn.current) withdrawn.current = null;
      }
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
    // Sent, the line goes back to the page's end, where the reply comes.
    setRaised(false);
    fromLine.current = sending.text;
    // One line for everything: with a task open, a message may be its answer, and the record decides.
    // Help asked for, ⌥↩ and a quoted passage are never an answer.
    const task = now.current.task?.text;
    if (isHelp(sending.text)) void sayRef.current(sending.text, "help", task);
    else if (sending.talk) void sayRef.current(sending.text, "talk");
    else if (task !== undefined) void sayRef.current(sending.text, "answer", task);
    // No task that the page can see. Sent as talk, the runtime refuses any attempt on the message. Claude Code's
    // tutor can set a task without saving it (a redo it words its own way), and the page then sees none: so
    // unless the engine itself says no task is open, nothing is said of the message and the tutor decides.
    else void sayRef.current(sending.text, taskMayBeOpen(now.current.session) ? undefined : "talk");
  }, [setRaised]);
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
    if (pending.kind === "start") void start();
    else if (pending.kind === "checks") void checks(pending.answer);
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
      void onAttach(attach, now.current.task !== null).then((saved) => lineRef.current?.add(saved));
    }
    if (other.length > 0) {
      void api.addMaterial(other, folder).then(
        (added) => added.lines.forEach(note),
        (problem) => note(String(problem)),
      );
    }
  };

  // ---------------------------------------------------------------------------
  // The panel's commands: expand it, flip between the lesson and the document, close or zoom the document

  /** ⌘↑, or the line under the heading: the whole lesson over the task, or the task alone again. */
  const turnPage = (): boolean => {
    const { page } = now.current;
    if (page === null) return false;
    if (page === "folded") setLookedBack(true);
    else {
      setLookedBack(false);
      setReading(false);
    }
    setRaised(false);
    // Opened, the lesson shows its end, where the task was set; folded, the task is at the top.
    stuck.current = true;
    requestAnimationFrame(() => {
      lineRef.current?.focus();
      // Opened, whether the task's reply is in view decides whether its copy is pinned over the line.
      requestAnimationFrame(() => measureQuestion.current());
    });
    return true;
  };

  const panelCommand = (command: string): boolean => {
    const docs = docsNow.current;
    const { props } = now.current;
    // A document that filled the session stepped aside for the task: switching to it opens the lesson over the task,
    // which brings the document back.
    if (command === "flip-panel" && now.current.page === "folded" && props.panel && docs.expanded && !docs.lessonFront && closesDoc(docs)) return turnPage();
    switch (command) {
      case "expand-panel": {
        if (!props.panel) props.onShowPanel();
        const filling = !docs.expanded || docs.lessonFront;
        setDocs({ ...docs, expanded: filling, lessonFront: false });
        window.setTimeout(() => (filling ? docView.current?.focus() : lineRef.current?.focus()), 80);
        return true;
      }
      case "flip-panel": {
        if (!props.panel) {
          props.onShowPanel();
          setDocs({ ...docs, lessonFront: false });
          // The panel mounts and the document opens: try until it is there to take the keyboard.
          for (const ms of [80, 400, 1200]) window.setTimeout(() => docView.current?.focus(), ms);
          return true;
        }
        if (docs.expanded) {
          const toLesson = !docs.lessonFront;
          setDocs({ ...docs, lessonFront: toLesson });
          window.setTimeout(() => (toLesson ? lineRef.current?.focus() : docView.current?.focus()), 60);
          return true;
        }
        const inPanel = box.current?.querySelector(".panel.side")?.contains(document.activeElement);
        if (inPanel) lineRef.current?.focus();
        else docView.current?.focus();
        return true;
      }
      case "close-document": {
        if (!docs.shown) return false;
        const hadKeys = (document.activeElement as HTMLElement | null)?.closest(".panel.side") !== null;
        const left = withClosed(docs, docs.shown);
        setDocs(left);
        // The keyboard goes where the eye goes: the next document, or the line when none is left.
        if (hadKeys) window.setTimeout(() => (left.shown ? docView.current?.focus() : lineRef.current?.focus()), 120);
        return true;
      }
      case "share-document":
        // The panel's own control: the sheet points from it.
        box.current?.querySelector<HTMLElement>('[data-drive="share-document"]')?.click();
        return true;
      case "open-document":
        // The panel first: shown, and in front of the lesson if it fills the session.
        if (!props.panel) props.onShowPanel();
        if (docs.expanded && docs.lessonFront) setDocs({ ...docs, lessonFront: false });
        setPicking(true);
        return true;
      case "zoom-in-document":
      case "zoom-out-document":
      case "zoom-fit-document":
        docView.current?.zoom(command === "zoom-in-document" ? 1 : command === "zoom-out-document" ? -1 : 0);
        return docView.current !== null;
    }
    return false;
  };

  // ---------------------------------------------------------------------------
  // What the window can ask of this session

  const handle: SessionHandle = {
    event: (event) => onEvent(event),
    command: (command) => {
      const { phase } = now.current;
      if (command === "hint" || command === "lost" || command === "unknown") {
        if (phase !== "lesson") return false;
        askHelp(command === "hint" ? "Can I have a hint?" : command === "lost" ? "I'm lost" : "I don't know");
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
      if (command === "model") {
        pickerRef.current.toggle();
        return true;
      }
      if (command === "stop-reply") {
        if (isBusyNow(now.current.session)) interrupt();
        return true;
      }
      if (command === "look-back") return turnPage();
      return panelCommand(command);
    },
    escape: () => {
      // Out of the document, back to the line; from a document that fills the session, back to the lesson.
      const filled = now.current.props.panel && docsNow.current.expanded && !docsNow.current.lessonFront;
      if (filled || box.current?.querySelector(".panel.side")?.contains(document.activeElement)) {
        if (docsNow.current.expanded && !docsNow.current.lessonFront) setDocs({ ...docsNow.current, lessonFront: true });
        lineRef.current?.focus();
        return true;
      }
      const { session, raised } = now.current;
      if (isBusyNow(session)) {
        interrupt();
        return true;
      }
      // The line up over the page goes back down to the end, its words kept.
      if (raised) {
        setRaised(false);
        return true;
      }
      return false;
    },
    drop,
    close: stop,
    release: () => {
      dispatch({ type: "released" });
      // Another tab has the tutor now: nothing more of this turn comes here.
      setSettling(false);
    },
    state: () => now.current.session,
    say: (text, said) => say(text, said ?? "talk"),
    start: (words) => start(words),
    checks: (answer) => checks(answer),
    taskOpen: () => now.current.task !== null,
    answerPage: () => now.current.page,
    docClosed: () => docClosedNow.current,
    busy: () => isBusyNow(now.current.session),
    docs: () => docsNow.current,
    probe: () => {
      const { phase, raised, atEnd, task, session, page } = now.current;
      return { phase, line: raised ? "raised" : atEnd ? "end" : "away", task: task?.text ?? null, open: session.open, busy: isBusyNow(session), page };
    },
  };
  const pickerRef = useRef(picker);
  pickerRef.current = picker;
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
      checks: (a) => handleRef.current.checks(a),
      taskOpen: () => handleRef.current.taskOpen(),
      answerPage: () => handleRef.current.answerPage(),
      docClosed: () => handleRef.current.docClosed(),
      busy: () => handleRef.current.busy(),
      docs: () => handleRef.current.docs(),
      probe: () => handleRef.current.probe(),
    };
    props.register(tab.key, stable);
    return () => props.register(tab.key, null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tab.key]);

  // A tab not in front is out of reach of the keyboard and of a screen reader.
  // Set before any effect runs, so the line of a tab brought to the front can take the focus.
  useLayoutEffect(() => {
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
      // Reopened where the learner was reading: the lesson stays there as it grows.
      stuck.current = restoring.current < 6;
      lastTop.current = el.scrollTop;
      restoring.current = null;
    }
  }, [phase, painted]);

  useEffect(() => {
    const el = surface.current;
    const section = nowRef.current;
    if (!el || !section || phase !== "lesson") return;
    const pin = () => {
      if (stuck.current) {
        el.scrollTop = el.scrollHeight;
        lastTop.current = el.scrollTop;
      }
    };
    const watch = new ResizeObserver(pin);
    watch.observe(section);
    // The surface too: a window or panel resized changes its frame before its content reflows.
    watch.observe(el);
    pin();
    return () => watch.disconnect();
  }, [phase]);

  // The same, in the commit that changes the lesson's height (a reply's words, the page folding or opening, the
  // task's copy over the line coming or going), with the place kept as the last one seen. The resize above is only
  // delivered with the next frame, and so is a scroll event. A page that shrank and then grew between two frames
  // (a send from the folded page draws the task's copy over the line for one commit, then the reply's first words
  // come) reached `onScroll` as a move up with the foot out of reach: the lesson was let go, and stayed short by the
  // height of that reply (91 px in the self-test, one run in six on a busy Mac).
  // A place above both the last one seen and the end is the learner's own move, its scroll event still to come: it
  // is left alone. A tab behind another measures nothing and says nothing.
  useLayoutEffect(() => {
    const el = surface.current;
    if (!el || phase !== "lesson" || !stuck.current || el.clientHeight === 0) return;
    if (el.scrollTop < Math.min(lastTop.current, el.scrollHeight - el.clientHeight) - 1) return;
    el.scrollTop = el.scrollHeight;
    lastTop.current = el.scrollTop;
  });

  // The conversation left, above the first screen and out of view, is drawn after it.
  useEffect(() => {
    if (phase !== "loading" && props.active && !painted) requestAnimationFrame(() => setPainted(true));
  }, [phase, props.active, painted]);

  // While a card waits on the learner, the tutor is not writing: the tab says so.
  const asking = session.cards.length > 0;
  useEffect(() => {
    props.onAsking(tab.key, asking);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [asking]);

  // Whether the fact's own line (under the answer, or the plan card) is in view.
  useEffect(() => {
    const root = surface.current;
    if (!fact || !root) return;
    const own = root.querySelector<HTMLElement>(`article[data-ms="${fact.since}"] .record`);
    if (!own) {
      setFactSeen(false);
      return;
    }
    const watch = new IntersectionObserver((entries) => setFactSeen(entries.some((entry) => entry.isIntersecting)), { root, threshold: 0.5 });
    watch.observe(own);
    return () => watch.disconnect();
  }, [fact, session]);

  const onScroll = () => {
    const el = surface.current;
    if (!el || now.current.phase !== "lesson") return;
    const fromFoot = Math.max(0, el.scrollHeight - el.clientHeight - el.scrollTop);
    const head = nowRef.current?.querySelector<HTMLElement>(".head");
    // The name takes over once the heading's last line reaches the top edge's fade (surface.css .top-edge), so there
    // is no stretch where the heading is under the edge and nothing names the lesson.
    if (head && nowRef.current) setHeadGone(el.scrollTop > nowRef.current.offsetTop + head.offsetTop + head.offsetHeight - 66);
    setScrolled(el.scrollTop > 4);
    // At the foot within a few pixels: any scroll up to read lets the lesson go, so a resize does not pull it back.
    // Only a move up does. A scroll event is delivered a frame late, and a reply still coming can have grown the
    // page under it by then; read as a scroll away, that would leave the line under the window at the end of a reply.
    const up = el.scrollTop < lastTop.current - 1;
    lastTop.current = el.scrollTop;
    if (up) stuck.current = fromFoot < 6;
    else if (fromFoot < 6) stuck.current = true;
    const end = measureEnd();
    // Read away from the end with nothing on the line, and the keyboard is the page's again: Space pages down.
    if (end === false && !now.current.raised && !lineRef.current?.holds() && document.activeElement?.closest(".session.in-front .line.lesson")) {
      (document.activeElement as HTMLElement).blur();
    }
    keepPlace(now.current.place, { fromFoot: Math.round(fromFoot) });
  };

  /**
   * Whether the line, the page's last line, is in view, kept for the render.
   * A tab behind another one is not drawn, and WebKit can still send it a
   * scroll then, with every measure at nothing: that says nothing, so it is
   * not read, and the tab is measured again when it comes to the front.
   */
  const measureEnd = (): boolean | null => {
    const el = surface.current;
    if (!el || el.clientHeight === 0 || now.current.phase !== "lesson") return null;
    const line = nowRef.current?.querySelector<HTMLElement>(".foot");
    const end = !line || line.getBoundingClientRect().top < el.getBoundingClientRect().bottom - 8;
    if (end !== now.current.atEnd) {
      now.current.atEnd = end;
      setAtEnd(end);
    }
    return end;
  };
  useEffect(() => {
    if (!props.active) return;
    const frame = requestAnimationFrame(() => measureEnd());
    return () => cancelAnimationFrame(frame);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.active, phase]);

  // ---------------------------------------------------------------------------
  // What the surface shows

  const turns: LessonTurn[] = useMemo(() => {
    const live = liveTurn(session);
    return live ? [...session.earlier, live] : session.earlier;
  }, [session]);
  const task = useMemo(() => openTaskOf(session, turns), [session, turns]);
  const openAt = task?.at ?? -1;
  now.current.task = task;
  // Once the reply that poses the check has ended and the window knows which task is open, Home's answer goes as the
  // answer to that task, as words sent from the line then would. With no task the page sees and none the tutor may
  // hold, nothing was posed: the words are put on the line, unsent, for the learner to send or change.
  const homeAnswerSent = homeAnswer !== null && turns.some((turn) => turn.ms === homeAnswer.ms);
  useEffect(() => {
    const held = homeAnswerNow.current;
    if (!held) return;
    // Sent: the turn it became stands where it was shown.
    if (turns.some((turn) => turn.ms === held.ms)) return setHomeAnswer(null);
    if (!held.ended || held.sending || held.reading || settling || isBusyNow(session)) return;
    // Which check the reply posed, as the record says once it has noted the reply. The engine notes it as the reply
    // ends, which can be just after the window hears that it ended, so the record is read a few times before it is
    // taken to say none.
    if (held.id && held.posed === undefined) {
      setHomeAnswer({ ...held, reading: true });
      void (async () => {
        let posed: string | null = null;
        for (let tries = 0; tries < 8 && posed === null; tries++) {
          if (tries > 0) await new Promise((done) => window.setTimeout(done, 400));
          posed = (await api.home(folder).catch(() => null))?.posed_check ?? null;
        }
        const still = homeAnswerNow.current;
        if (still && still.ms === held.ms) setHomeAnswer({ ...still, reading: false, posed });
      })();
      return;
    }
    // Only the check Home showed takes the words (render.ts, heldAnswerGoes).
    const goes = heldAnswerGoes(held, { posed: held.id ? held.posed : undefined, task: task?.text ?? null, taskUnseen: task === null && taskMayBeOpen(session), reply: turns[turns.length - 1]?.tutor ?? "" });
    if (goes === "line") return withdrawHomeAnswer(true);
    const { words, ms } = held;
    setHomeAnswer({ ...held, sending: true });
    fromLine.current = words;
    void sayRef.current(words, goes === "answer" ? "answer" : undefined, task?.text, ms).then(() => {
      // It never became a turn (the session could not be opened again): the words go on the line, unsent.
      if (!now.current.session.earlier.some((turn) => turn.ms === ms) && now.current.session.live?.since !== ms) {
        setHomeAnswer(null);
        lineRef.current?.write(words);
      }
    });
  }, [homeAnswer, homeAnswerSent, settling, task, session, turns, setHomeAnswer, withdrawHomeAnswer]);
  const taskText_ = task?.text ?? null;
  useEffect(() => {
    props.onTaskChange(tab.key, taskText_ !== null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [taskText_, phase]);
  // Whether the reply that asks the open task is in view above the line: while it is, the foot
  // does not say the question again; out of view, it does.
  // Whether the question is in view: while it is, the foot does not pin or restate it.
  const [questionInView, setQuestionInView] = useState(false);
  const measureQuestion = useRef<() => void>(() => {});
  measureQuestion.current = () => {
    const root = surface.current;
    const asked = root?.querySelector<HTMLElement>(".turn.open .tutor .prose div.task") ?? root?.querySelector<HTMLElement>(".turn.open .tutor");
    // A task drawn with a line over its reply: the question is the line's height.
    const ruled = root?.querySelector<HTMLElement>(".turn.open .tutor .prose.has-task-rule");
    if (!root || !asked || task === null) return setQuestionInView(false);
    const foot = root.querySelector<HTMLElement>(".foot");
    const pinned = foot?.querySelector<HTMLElement>(".parked");
    const view = root.getBoundingClientRect();
    const whole = asked.getBoundingClientRect();
    const top = ruled ? ruled.getBoundingClientRect().top + (parseFloat(ruled.style.getPropertyValue("--task-top")) || 0) : whole.top;
    const box = ruled ? { top, bottom: top + (parseFloat(ruled.style.getPropertyValue("--task-height")) || 0) } : whole;
    // The line's top, as it would be with no question pinned over it.
    const lineTop = (foot?.getBoundingClientRect().top ?? view.bottom) + (pinned?.offsetHeight ?? 0);
    setQuestionInView((was) => (was ? box.top >= view.top + 40 && box.bottom <= lineTop + 4 : box.top >= view.top + 56 && box.bottom <= lineTop - 24));
  };
  useEffect(() => {
    const root = surface.current;
    if (!root) return;
    let frame = 0;
    const again = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => measureQuestion.current());
    };
    root.addEventListener("scroll", again, { passive: true });
    window.addEventListener("resize", again);
    return () => {
      cancelAnimationFrame(frame);
      root.removeEventListener("scroll", again);
      window.removeEventListener("resize", again);
    };
  }, []);
  // A session named by the learner's first words: its heading would only say them again.
  const firstSaid = phase === "lesson" ? (turns[sittingStart]?.learner ?? "") : "";
  const titleIsFirstWords = firstSaid.trim() !== "" && sittingTitle.trim() !== "" && firstWords(firstSaid, 80) === sittingTitle;
  const ownBusy = isBusyNow(session);
  const isBusy = ownBusy || props.elsewhereBusy;
  // Measured before the page is painted: measured after, a task just named was pinned over the line for one frame
  // while its own reply was in view, and the lesson jumped by the copy's height and back.
  useLayoutEffect(() => {
    measureQuestion.current();
  }, [task, openAt, session, raised, props.panel, ownBusy, settling]);
  // With a task open, the page is the task: the lesson folds to one line under the heading, and the task's own words
  // stand over the line. Only while the tutor is not writing and no card waits, and not while a reply just written
  // here is still to be read (see `reading`).
  const page: AnswerPage = phase === "lesson" && task !== null && !ownBusy && session.cards.length === 0 ? (reading || lookedBack ? "open" : "folded") : null;
  const folded = page === "folded";
  now.current.page = page;
  useEffect(() => {
    props.onAnswerPage(tab.key, page);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [page]);
  // The page changed its height under the line: whether the line is in view is read again.
  useEffect(() => {
    if (!props.active) return;
    const frame = requestAnimationFrame(() => measureEnd());
    return () => cancelAnimationFrame(frame);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [folded]);

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
      const clock = (when: Date) => `${String(when.getHours()).padStart(2, "0")}:${String(when.getMinutes()).padStart(2, "0")}`;
      const ends = new Date(home.sitting.started_ms + home.sitting.minutes * 60_000);
      return {
        ...base,
        next: home.sitting.name,
        where: `Started at ${clock(startedAt)}, for ${home.sitting.minutes} minutes, until ${clock(ends)}. Write ? where you are stuck, and S, U or G after each answer for sure, unsure or a guess. Hand it in from the workspace’s page.`,
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
        keys: "↩ start",
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

  const putAway = useCallback(() => setRaised(false), [setRaised]);

  // Words typed anywhere in the lesson, or in a document beside it, are the
  // learner's: they go to the line, which rises if the page's end is out of
  // view, and a passage selected when the first key is typed is quoted with
  // them. Space and the arrows still move the page, a control keeps its
  // Return, and a key bound to a command keeps its command.
  useEffect(() => {
    if (!props.active || phase !== "lesson") return;
    const onKey = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.metaKey || event.ctrlKey || event.isComposing || event.keyCode === 229) return;
      const { props: current, session: shown } = now.current;
      if (current.covered || shown.cards.length > 0) return;
      const target = event.target instanceof HTMLElement ? event.target : null;
      if (target?.closest("input, textarea, select, [contenteditable], .layer, .card")) return;
      const enter = event.key === "Enter" && !event.altKey && !event.shiftKey;
      const char = event.key.length === 1 && event.key !== " ";
      if (!enter && !char) return;
      if (enter && target?.closest("button, a, summary, [role='button'], [role='option']")) return;
      if (actionOf(event, current.prefs.keys)) return;
      const selection = window.getSelection();
      const selected = selection && !selection.isCollapsed && selection.anchorNode && box.current?.contains(selection.anchorNode) ? selection.toString() : "";
      event.preventDefault();
      raise();
      lineRef.current?.write(char ? event.key : "", selected);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [props.active, phase, raise]);

  // Writing on the line once the reply is read turns the page to the task, so the answer is written from the task
  // alone. A lesson the learner opened over the task stays open: they opened it to answer with it in view.
  const onWrite = useCallback(() => {
    const { page, lookedBack } = now.current;
    if (page !== null && !lookedBack) {
      setReading(false);
      setRaised(false);
      stuck.current = true;
      return;
    }
    raise();
  }, [raise, setRaised]);

  // A press on the open task is a start on it: the line takes the keyboard.
  const onHold = useCallback(() => {
    raise();
    lineRef.current?.focus();
  }, [raise]);
  const onDismiss = useCallback((text: string) => dispatch({ type: "dismiss", text }), []);
  const again = useRef({ turns, sittingStart, sittingTitle });
  again.current = { turns, sittingStart, sittingTitle };
  /**
   * A lesson started at a picked effort never opened the record: start it again, with the same first words, at
   * the agent's own effort. The pick keeps the model and takes Usual, so the start line says so next time too.
   */
  const startAgain = useCallback(
    async (text: string, atUsualEffort: boolean) => {
      dispatch({ type: "dismiss", text });
      const { turns: shownTurns, sittingStart: from, sittingTitle: title } = again.current;
      const first = shownTurns[from]?.learner ?? "";
      if (atUsualEffort) {
        try {
          const choice = await api.models();
          if (choice.pick) await api.setPick(choice.pick.model, null);
        } catch (problem) {
          note(String(problem));
          return;
        }
      }
      if (first.trim() === "") return;
      void begin(false, first, "opening", title, (core) => core.sendNewLesson(first));
    },
    // `begin` reads the tab's state through `now`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [api, note],
  );
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
  // The goal names the session once the tutor has set it, in its tab and in the workspace's list.
  const nameSession = (name: string) => {
    setSittingTitle(name);
    props.onTitle(props.tab.key, name);
    const id = now.current.props.tab.session;
    if (id) void api.renameSession(folder, id, name).catch(() => {});
  };
  // The goal this session's own lesson has in the record, as the record binds the conversation to it. Not the
  // folder's last goal: a lesson in a terminal, in the same folder, may have set that one.
  const ownGoal = async (): Promise<{ goal: string | null; opening: string; title: string | null } | null> => {
    const id = now.current.props.tab.session;
    if (!id) return null;
    const info = (await api.lessons(folder).catch(() => [])).find((each) => each.session === id);
    return info ? { goal: info.goal ?? null, opening: info.opening, title: info.title } : null;
  };
  // A session started from typed words is named by its goal once the tutor has set one that is not those words.
  const nameByOwnGoal = async () => {
    const own = await ownGoal();
    const name = own ? goalName(own.goal, typedAtStart.current) : null;
    if (!provisionalTitle.current || !name || name === sittingTitle) return;
    provisionalTitle.current = false;
    nameSession(name);
  };

  // A session reopened still named by the words it was started with ("Continue") takes its own lesson's goal,
  // as a new one does.
  const renamedOnce = useRef(false);
  useEffect(() => {
    if (renamedOnce.current || !props.tab.session) return;
    renamedOnce.current = true;
    void ownGoal().then((own) => {
      if (!own || !namedByItsWords(own)) return;
      const name = goalName(own.goal, own.opening);
      if (name && name !== own.title) nameSession(name);
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.tab.session]);

  // A past paper: the folder's papers, or a file in a papers folder. A link to one opens at once, even beside
  // the task that asks about it: it holds the questions, not their answers.
  const paperPaths = useMemo(() => new Set((home?.papers ?? []).map((paper) => paper.path)), [home?.papers]);
  const isPaper = useCallback((path: string) => paperPaths.has(path) || /(^|\/)papers?\//i.test(path), [paperPaths]);
  // While the page is the task, a document beside it is closed: all but a past paper, which stays open for the same
  // reason its links do, and may be the paper the learner is working from.
  const closesDoc = (open: Docs) => open.shown !== null && !isPaper(open.shown);
  const docClosed = folded && props.panel && closesDoc(docs);
  const docClosedNow = useRef(docClosed);
  docClosedNow.current = docClosed;
  // A document the learner opens or picks while the page is the task is a look back, as ⌘↑ is: it opens, and the
  // lesson with it, rather than being closed in their face.
  const shownBefore = useRef(docs.shown);
  useEffect(() => {
    const was = shownBefore.current;
    shownBefore.current = docs.shown;
    if (docs.shown !== null && docs.shown !== was && now.current.page === "folded" && closesDoc(docs)) setLookedBack(true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [docs.shown]);

  // A file the tutor named opens beside the lesson, at its page; the panel shows if it was hidden.
  // The core reads it first: a file it refuses (an answer key, a path outside the folder) never
  // gets a place in the panel, so its name is not shown there either.
  const onSource = useCallback(
    (source: SourceRef) => {
      void api.source(source.path, folder).then(
        () => {
          setDocs(withOpened(docsNow.current, source.path, source.page));
          setGoTo((n) => n + 1);
          now.current.props.onShowPanel();
        },
        (problem) => note(String(problem)),
      );
    },
    [api, folder, note, setDocs],
  );
  const stopReply = useCallback(() => {
    // Stopped while it poses the check: Home's words are not sent after it.
    withdrawHomeAnswer();
    interrupt();
  }, [interrupt, withdrawHomeAnswer]);
  // The whole session sits under its heading; a very long one shows its last 150 turns.
  const shownFrom = Math.max(sittingStart, turns.length - 150);
  const sittingTurns = phase === "lesson" ? turns.slice(shownFrom) : [];
  const stopKey = keyName(props.prefs.keys["stop-reply"]) || "esc";
  const keys = props.prefs.keys;
  const keyFor = useCallback((action: KeyAction) => keyName(keys[action]), [keys]);
  // The session's own rows under More, on its first line and in the lesson.
  const moreRows = useMemo<LineRow[]>(
    () => [{ id: "model", label: "Model", detail: picker.name ?? undefined, leads: true, group: "session", run: () => pickerRef.current.toggle() }],
    [picker.name],
  );
  const paused = props.covered || !props.active || session.cards.length > 0;
  // The record's latest fact is said once, beside what it marks. The foot repeats it only while that line is out of view, and never under an open task.
  const footFact = fact && task === null && !factSeen ? fact : null;
  const lookKey = keyName(props.prefs.keys["look-back"]);
  // The one line that turns the page: under the heading, and at the top edge once the heading has scrolled away.
  const turnLine = (where: string) =>
    page === null ? null : (
      <button type="button" className={`plain page-turn at-${where}`} data-drive={where === "head" ? "look-back" : `look-back-${where}`} aria-expanded={!folded} onMouseDown={(event) => event.preventDefault()} onClick={() => turnPage()}>
        {lookKey && <span className="keys">{lookKey}</span>} {folded ? "the lesson so far" : "only the task"}
      </button>
    );
  const closedWords = `Closed while you answer. It opens again when you send your answer, or with ${lookKey || "“the lesson so far”"}.`;

  return (
    <div
      ref={box}
      className={`session${props.active ? " in-front" : ""}${props.panel ? " with-panel" : ""}${props.panel && docs.expanded ? " expanded" : ""}${props.panel && docs.expanded && docs.lessonFront ? " lesson-front" : ""}${docClosed ? " doc-closed" : ""}`}
      style={props.panel ? ({ "--side-w": `${props.panelWidth}px` } as React.CSSProperties) : undefined}
      data-phase={phase}
      data-busy={ownBusy ? "yes" : "no"}
      data-tab={tab.key}
      role="tabpanel"
      aria-labelledby={tabId(tab.key)}
      aria-hidden={props.active ? undefined : true}
    >
      {phase === "lesson" && scrolled && (
        // A top edge once the lesson is scrolled: text fades out under the tabs, and once the heading
        // has scrolled away, the lesson's name and its step are said here, in the window's quiet voice.
        // At the top of the lesson there is none, so the course's name above the heading reads whole.
        <div className="top-edge" aria-hidden={!headGone}>
          {headGone && (
            <div className="inner">
              {/* The tab names the session already: the step in hand is what this line adds. */}
              {currentStep && !props.panel ? <Line text={currentStep} className="top-step" /> : !currentStep ? <Line text={sittingTitle} className="top-name" /> : null}
              {turnLine("top")}
            </div>
          )}
        </div>
      )}
      <main className={scrolled ? "surface scrolled" : "surface"} ref={surface} onScroll={onScroll}>
        <div className="column">
          <section className={`now ${phase}${page !== null ? " turnable" : ""}${folded ? " folded" : ""}`} ref={nowRef}>
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
                    placeholder={rest.start ? "↩ to start, or say what to work on" : undefined}
                    end={restVerb ? { keys: "↩", what: restVerb } : null}
                    toolBeside={picker.list}
                    more={moreRows}
                    moreHeld={picker.open}
                    keyFor={keyFor}
                    active
                    busy={false}
                    paused={paused}
                    taskOpen={false}
                    onSend={restSend}
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
                <header className={`head${titleIsFirstWords ? " bare" : ""}${headGone ? " gone" : ""}`}>
                  {home && <p className="course">{displayName(home.name)}</p>}
                  {/* A session named by the learner's first words is headed by its course alone: the words follow just under it, and the tab names it. */}
                  {titleIsFirstWords ? (
                    <h1 className="visually-hidden">
                      <Line text={sittingTitle} />
                    </h1>
                  ) : (
                    <h1 className="next">
                      <Line text={sittingTitle} />
                    </h1>
                  )}
                  {turnLine("head")}
                </header>
                {shownFrom > 0 && <p className="record quiet earlier-note">Earlier turns of this session are kept, and not drawn here.</p>}
                {folded && task && (
                  // The task's own words, as the record keeps them. The lesson under it stays drawn, out of view, so
                  // opening it again is at once. Its links stay shut, as they are in the reply that set it.
                  <div className="answer-task" data-role="answer-task" onClick={(event) => !(event.target as HTMLElement).closest("a, button") && lineRef.current?.focus()}>
                    <Rich text={task.text} final linksOpen={false} opensNow={isPaper} onSource={onSource} onNote={note} />
                  </div>
                )}
                {[
                  ...sittingTurns,
                  // Home's answer, held: drawn as the turn it becomes, under the same key, so sending it moves nothing.
                  ...(homeAnswer && !homeAnswerSent ? [{ session: "", ms: homeAnswer.ms, learner: homeAnswer.words, tutor: "", interrupted: false, said: "answer" } satisfies LessonTurn] : []),
                ].map((turn, at) => {
                  const index = shownFrom + at;
                  const live = index === turns.length - 1 && session.live !== null;
                  const before = index > 0 ? turns[index - 1] : null;
                  return (
                    <Turn
                      key={`${turn.ms}-${index}`}
                      turn={turn}
                      open={index === openAt}
                      held={false}
                      linksOpen={index !== openAt}
                      opensNow={isPaper}
                      writing={live && session.live!.status !== "idle"}
                      divider={at > 0 && (turn.said === "opening" || opening(turn.learner) !== null)}
                      task={index === openAt ? task?.text : undefined}
                      showQuestion={!before || !(before.posed === turn.question || hasTask(before.tutor) || (turn.question !== undefined && poses(before.tutor, turn.question)))}
                      settled
                      planKey={props.panel ? null : keyName(props.prefs.keys.plan) || null}
                      onSource={onSource}
                      onNote={note}
                      onHold={onHold}
                      waiting={homeAnswer !== null && !homeAnswerSent && turn.ms === homeAnswer.ms}
                    />
                  );
                })}
                {otherCheck !== null && OTHER_CHECK_NOTE === "lesson" && !isBusyNow(session) && (
                  <p className="record quiet other-check in-lesson" data-role="other-check">
                    This is not the check Home asked, so your answer from Home was not sent. It is on the line, to send for this one or to change.
                  </p>
                )}
                {session.live && session.cards.length === 0 && <Working live={session.live} />}
                {error && <p className="record note">{withoutPaths(error)}</p>}
                <Notices notices={session.notices} onDismiss={onDismiss} onStartAgain={startAgain} onSignIn={props.onSignIn} onPickModel={() => pickerRef.current.toggle()} />
                <Cards cards={session.cards} onDecide={onDecide} onAnswer={onAnswer} />
                {home && (
                  // The line: the page's last line, under the reply. Typed into while the learner reads
                  // higher up, it rises to the foot of the window over the page, and goes back down when
                  // the words are sent or put away. The record's latest fact sits under it at the end.
                  <div className={`foot${raised ? " raised" : ""}${session.cards.length > 0 ? " asking" : ""}`}>
                    {/* Not while the tutor writes: what was sent is the answer to it, and a copy under the answer read
                        as the task set again, under the reply that set it. Nor after the reply, until the window has
                        heard which task is open now: until then it is the one just answered. */}
                    {task && !raised && !questionInView && !folded && !ownBusy && !settling && (
                      <button className="parked" data-drive="open-task" onMouseDown={(event) => event.preventDefault()} onClick={() => lineRef.current?.focus()}>
                        <Line text={task.text} />
                      </button>
                    )}
                    {otherCheck !== null && OTHER_CHECK_NOTE === "question" && (
                      <p className="record quiet other-check asked-on-home" data-role="other-check">
                        Not sent. Written on Home for another check: <Line text={otherCheck} />
                      </p>
                    )}
                    <Composer
                      key={`line-${tab.key}`}
                      ref={lineRef}
                      place="lesson"
                      placeKey={place}
                      initial={draft}
                      active={atEnd || raised}
                      busy={isBusy}
                      paused={paused}
                      taskOpen={task !== null}
                      taskUnseen={task === null && taskMayBeOpen(session)}
                      stopKey={stopKey}
                      toolBeside={picker.list}
                      more={moreRows}
                      moreHeld={picker.open}
                      keyFor={keyFor}
                      onSend={onSend}
                      onNote={note}
                      onAttach={onAttach}
                      onPasteImage={onPasteImage}
                      onPick={onPick}
                      onHelp={askHelp}
                      onStop={ownBusy ? stopReply : undefined}
                      stopWords={session.cards.length > 0 ? "stop the tutor" : "stop the reply"}
                      cardWaits={session.cards.length > 0}
                      onWrite={onWrite}
                      onPutAway={putAway}
                      onLeftEmpty={putAway}
                    />
                    {otherCheck !== null && OTHER_CHECK_NOTE === "under" && (
                      <p className="record quiet other-check" data-role="other-check">
                        Your answer from Home is on the line, not sent: the check above is not the one Home asked.
                      </p>
                    )}
                    {footFact && !raised && (
                      <p className={`fact ${footFact.kind}`} title={footFact.title} data-role="fact">
                        <Line text={footFact.text} />
                      </p>
                    )}
                  </div>
                )}
              </>
            )}
          </section>
        </div>
      </main>
      {props.panel && (
        <SidePanel
          plan={<PlanPanel plan={plan} changed={changed} />}
          docs={docs}
          folder={folder}
          goTo={goTo}
          width={props.panelWidth}
          expanded={docs.expanded}
          onDocs={setDocs}
          onWidth={props.onPanelWidth}
          onExpand={(expanded) => {
            setDocs({ ...docsNow.current, expanded, lessonFront: false });
            window.setTimeout(() => (expanded ? docView.current?.focus() : lineRef.current?.focus()), 80);
          }}
          onNote={note}
          handle={(found) => (docView.current = found)}
          picking={picking && props.active}
          onPicking={setPicking}
          closed={docClosed ? closedWords : null}
          id={props.active ? "plan-panel" : undefined}
        />
      )}
    </div>
  );
});
