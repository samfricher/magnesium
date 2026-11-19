import { forwardRef, useCallback, useEffect, useImperativeHandle, useLayoutEffect, useMemo, useReducer, useRef, useState } from "react";

import { busy as isBusyNow, CHECKS_WORDS, emptySession, isHelp, openingWords, opening, pickedWords, reduce, tutorText, unanswered, type LessonTurn, type Said, type SessionState } from "../lesson";
import { marksOfTurns } from "../marks";
import { drawHeadNow, stillHeld } from "../stepHead";
import { actionOf, changeOf, keepChange, keyName, keepPlace, movePlace, placeKey as keyOf, placeOf, type KeyAction, type Prefs } from "../prefs";
import { hasTask, heldAnswerGoes, poses } from "../render";
import { sentHomeDraft } from "../homeDraft";
import { setupReady } from "../setup";
import type { SourceRef } from "../sources";
import type { AgentEvent, Answer, Core, Decision, Home, Mark, Plan, Setup, Turn as SavedTurn } from "../types";
import { displayName, firstWords, goalName, markWords, namedByItsWords, passDaySpoken, passWords, planDiff, shortDay, withoutPaths, type PlanDiff } from "../words";
import { Composer, type ComposerHandle, type Sending } from "./Composer";
import { Cards, markArrived, Notices, Turn, Working } from "./Conversation";
import { tabId } from "../pages/Tabs";
import { PlanPanel } from "./PlanPanel";
import { SidePanel } from "./SidePanel";
import type { DocHandle } from "./DocView";
import { docsOf, keepDocs, moveDocs, sessionDocsKey, withClosed, withOpened, type Docs } from "../docs";
import { Line, Rich } from "./Rich";
import type { LineRow } from "./LineMenu";
import { useModelPicker } from "./ModelPicker";
import { pageClock, wordsBatch, type WordsBatch } from "../wordsBatch";
import { inTask, quotable, selectedWords, selecting } from "../selection";
import { readingSpot } from "../reading";
import { taskBox } from "../taskRule";

export type Pending =
  | { kind: "start" }
  | { kind: "checks"; answer?: HomeAnswer }
  | { kind: "paper"; shown: string; sent: string };

export type HomeAnswer = { words: string; question: string; id: string | null };

export type SessionTab = { key: string; kind: "session"; folder: string; session: string | null; title: string; pending?: Pending };

export type AnswerPage = "folded" | "open" | null;

export type SessionHandle = {
  event: (event: AgentEvent) => void;
  command: (command: string) => boolean;
  escape: () => boolean;
  drop: (paths: string[]) => void;
  close: () => Promise<void>;
  release: () => void;
  state: () => SessionState;
  say: (text: string, said?: Said) => Promise<void>;
  start: (words?: string) => Promise<boolean>;
  checks: (answer?: HomeAnswer) => Promise<boolean>;
  taskOpen: () => boolean;
  answerPage: () => AnswerPage;
  docClosed: () => boolean;
  busy: () => boolean;
  docs: () => Docs;
  probe: () => { phase: string; line: "end" | "raised" | "away"; task: string | null; open: boolean; busy: boolean; page: AnswerPage };
};

type Props = {
  tab: SessionTab;
  active: boolean;
  live: boolean;
  api: Core;
  setup: Setup | null;
  setupWait: Promise<Setup> | null;
  prefs: Prefs;
  panel: boolean;
  panelWidth: number;
  onPanelWidth: (width: number) => void;
  onShowPanel: () => void;
  onDocsChange: () => void;
  covered: boolean;
  elsewhereBusy: boolean;
  claim: (key: string) => void;
  register: (key: string, handle: SessionHandle | null) => void;
  onSession: (key: string, session: string) => void;
  onTitle: (key: string, title: string) => void;
  onBusy: (key: string, busy: boolean) => void;
  onAsking: (key: string, asking: boolean) => void;
  onEngine: () => void;
  onSignIn: () => void;
  onTaskChange: (key: string, open: boolean) => void;
  onAnswerPage: (key: string, page: AnswerPage) => void;
  onStarted: (key: string) => void;
};

export { setupReady };

export function openTaskAt(turns: LessonTurn[]): number {
  for (let i = turns.length - 1; i >= 0; i--) {
    const turn = turns[i];
    if (hasTask(turn.tutor)) return turn.interrupted ? -1 : i;
    if (turn.said === "answer") return -1;
    if (turn.said === "opening" || opening(turn.learner)) return -1;
  }
  return -1;
}

export type OpenTask = { text: string; at: number };

export function openTaskOf(session: SessionState, turns: LessonTurn[]): OpenTask | null {
  if (session.task !== undefined && !(session.task.text === null && session.task.fallback)) {
    const text = session.task.text;
    if (!text) return null;
    for (let i = turns.length - 1; i >= 0; i--) if (turns[i].posed === text || poses(turns[i].tutor, text)) return { text, at: i };
    return { text, at: -1 };
  }
  const at = openTaskAt(turns);
  return at >= 0 ? { text: taskText(turns[at].tutor), at } : null;
}

export function taskMayBeOpen(session: SessionState): boolean {
  return session.task === undefined || session.task.fallback === true;
}

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
    if (turn.step) lesson.step = turn.step;
    const change = session ? changeOf(session, turn.learner, turn.tutor) : null;
    if (change) lesson.change = change as PlanDiff;
    if (turn.noted) open = turn.task ?? null;
    out.push(lesson);
  });
  return out;
}

function taskText(reply: string): string {
  const found = [...reply.matchAll(/\*\*(?:task|check|quick check|question|your turn|try it|try this|redo|paper redo|now you)\b/gi)].pop();
  return found?.index !== undefined ? reply.slice(found.index) : reply;
}

type Rest = {
  course: string | null;
  next: string;
  where: string | null;
  line: boolean;
  start: (() => void) | null;
  choices: { label: string; run: () => void }[];
  keys: string;
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
    ...(live.step ? { step: live.step } : {}),
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
  const [raised, setRaisedState] = useState(false);
  const [atEnd, setAtEnd] = useState(true);
  const [notes, setNotes] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [changed, setChanged] = useState<PlanDiff | null>(null);
  const [draft, setDraft] = useState("");
  const [painted, setPainted] = useState(false);
  const [headGone, setHeadGone] = useState(false);
  const [scrolled, setScrolled] = useState(false);
  const [dueTopic, setDueTopic] = useState<string | null>(null);
  const [fact, setFact] = useState<{ kind: "mark" | "plan"; text: string; title?: string; since: number } | null>(null);
  const [factSeen, setFactSeen] = useState(true);
  const [markSaid, setMarkSaid] = useState("");
  const [reading, setReading] = useState(false);
  const [lookedBack, setLookedBack] = useState(false);
  const [settling, setSettling] = useState(false);
  const [acted, setActed] = useState(0);
  const act = useCallback(() => setActed((n) => n + 1), []);
  const [copy, setCopy] = useState<string | null>(null);
  const decided = useRef("");
  const surface = useRef<HTMLElement>(null);
  const [heldStep, setHeldStep] = useState<number | null>(null);
  const nowRef = useRef<HTMLElement>(null);
  const box = useRef<HTMLDivElement>(null);
  const lineRef = useRef<ComposerHandle>(null);
  const fromLine = useRef<string | null>(null);
  const modelAt = useMemo(() => ({ folder, session: props.tab.session, live: props.live }), [folder, props.tab.session, props.live]);
  const picker = useModelPicker({ api, at: modelAt, onClose: () => lineRef.current?.focus() });
  const spot = useRef(readingSpot(() => surface.current, () => nowRef.current)).current;
  const restoring = useRef<number | null>(null);
  const started = useRef(false);
  const [homeAnswer, setHomeAnswerState] = useState<
    (HomeAnswer & { ms: number; ended: boolean; sending: boolean; reading: boolean; posed?: string | null }) | null
  >(null);
  const [otherCheck, setOtherCheck] = useState<string | null>(null);
  const homeAnswerNow = useRef<typeof homeAnswer>(null);
  const setHomeAnswer = useCallback((next: typeof homeAnswer | ((old: typeof homeAnswer) => typeof homeAnswer)) => {
    const value = typeof next === "function" ? next(homeAnswerNow.current) : next;
    homeAnswerNow.current = value;
    setHomeAnswerState(value);
  }, []);
  const withdrawHomeAnswer = useCallback((another = false) => {
    const held = homeAnswerNow.current;
    if (!held || held.sending) return;
    setHomeAnswer(null);
    withdrawn.current = held;
    if (another) setOtherCheck(held.question);
    let tries = 0;
    const put = () => {
      const line = lineRef.current;
      if (line && line.holds() && tries++ < 20) return void window.setTimeout(put, 50);
      line?.write(line.holds() ? ` ${held.words}` : held.words);
    };
    window.setTimeout(put, 0);
  }, [setHomeAnswer]);

  const withdrawn = useRef<HomeAnswer | null>(null);

  const provisionalTitle = useRef(false);
  const typedAtStart = useRef<string>("");

  const place = keyOf(folder, tab.session, tab.key);
  const placeWas = useRef(place);
  useEffect(() => {
    if (placeWas.current === place) return;
    movePlace(placeWas.current, place);
    placeWas.current = place;
  }, [place]);

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

  const now = useRef({ props, phase, home, plan, session, raised, atEnd, place, lookedBack, task: null as OpenTask | null, page: null as AnswerPage });
  now.current = { props, phase, home, plan, session, raised, atEnd, place, lookedBack, task: now.current?.task ?? null, page: now.current?.page ?? null };

  useImperativeHandle(outer, () => box.current!);

  const note = useCallback((said: string) => {
    const text = withoutPaths(said.replace(/^Error: /, ""));
    if (now.current.phase === "lesson") dispatch({ type: "note", notice: { kind: "turn", text } });
    else setNotes((old) => [...old.filter((n) => n !== text), text].slice(-3));
  }, []);

  const setRaised = useCallback(
    (up: boolean) => {
      if (now.current.raised !== up) act();
      now.current.raised = up;
      setRaisedState(up);
    },
    [act],
  );
  const raise = useCallback(() => {
    if (now.current.phase === "lesson" && !now.current.atEnd && !now.current.raised) setRaised(true);
  }, [setRaised]);

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

  const readDueTopic = useCallback(async () => {
    try {
      const [progress, marks] = await Promise.all([api.progress(null, folder), api.marks(folder).catch(() => null)]);
      const row = [...progress.in_progress, ...progress.confirmed].find((each) => each.due);
      if (!row) return;
      const last = [...(marks ?? [])].reverse().find((mark) => mark.label.trim() === row.name.trim());
      setDueTopic(last ? `${row.name}, from ${shortDay(last.at)}` : row.name);
    } catch {
    }
  }, [api, folder]);

  useEffect(() => {
    let alive = true;
    void (async () => {
      const opened = await refreshHome();
      if (!alive) return;
      if (opened && (opened.checks_due ?? 0) + opened.redos_due > 0) void readDueTopic();
      const planning = refreshPlan();
      const kept = placeOf(place);
      setDraft(kept.draft || kept.answerDraft);
      if (tab.session) {
        const [saved, marks] = await Promise.all([api.lessonTurns(folder, tab.session).catch(() => [] as SavedTurn[]), api.marks(folder).catch(() => null)]);
        if (!alive) return;
        const noted = [...saved].reverse().find((turn) => turn.noted);
        const shown = rebuild(saved, marks, tab.session);
        dispatch({ type: "shown", turns: shown, ...(noted ? { task: { text: noted.task ?? null } } : {}) });
        setSittingStart(0);
        if (!tab.title && opened?.last?.goal) setSittingTitle(opened.last.goal);
        restoring.current = kept.seenMs > 0 ? kept.fromFoot : 0;
        setPhase("lesson");
      } else {
        await planning;
        if (!alive) return;
        setPhase("rest");
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  const placeHead = (ms: number) => {
    const reply = replyOf(ms);
    const view = surface.current;
    const place = { folded: now.current.page === "folded", shown: now.current.props.active && view !== null && view.clientHeight > 0, replyTop: reply ? reply.getBoundingClientRect().top : null, viewTop: view ? view.getBoundingClientRect().top : 0 };
    if (!drawHeadNow(place)) setHeldStep(ms);
  };
  const replyOf = (ms: number) => surface.current?.querySelector<HTMLElement>(`article.turn[data-ms="${ms}"] .tutor`) ?? null;

  const onEvent = (event: AgentEvent) => {
    const before = now.current.session.live;
    if (event.type === "step_begun" && before) placeHead(before.since);
    dispatch({ type: "event", event });
    const { props } = now.current;
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
    if (event.type === "turn_ended" && now.current.session.task === undefined) setSettling(false);
    const held = homeAnswerNow.current;
    if (held && !held.ended && !held.sending && (event.type === "turn_ended" || event.type === "exited")) {
      if (event.type === "exited" || event.interrupted || unanswered(before, event)) withdrawHomeAnswer();
      else setHomeAnswer({ ...held, ended: true });
    }
    if (event.type === "turn_ended" || event.type === "exited") props.onBusy(props.tab.key, false);
    if (event.type === "turn_ended") void refreshHome();
    if (event.type === "turn_started") setMarkSaid("");
    if (event.type === "marked") {
      const since = now.current.session.live?.since ?? 0;
      setFact({ kind: "mark", text: markWords(event.mark), since });
      markArrived(event.mark.id);
      const words = [markWords(event.mark), passDaySpoken(event.mark), passWords(event.mark)].filter(Boolean).join(" ");
      setMarkSaid((said) => (said ? `${said} ${words}` : words));
    }
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

  const begin = async (resume: boolean, shown: string, said: Said, title: string, send: (api: Core) => Promise<void>): Promise<boolean> => {
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
    spot.follow();
    setPhase("lesson");
    keepPlace(now.current.place, { inLesson: true });
    props.claim(props.tab.key);
    try {
      const opened = await api.openSession(resume, { folder, session: resume ? props.tab.session : null, title });
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

  const checks = async (answer?: HomeAnswer): Promise<boolean> => {
    if (!now.current.home) return false;
    const words = answer?.words.trim() ?? "";
    if (answer && words !== "") setHomeAnswer({ words, question: answer.question, id: answer.id, ms: Date.now(), ended: false, sending: false, reading: false });
    const begun = await begin(false, CHECKS_WORDS, "opening", "Quick checks", (api) => api.sendNewLesson(CHECKS_WORDS));
    if (!begun) withdrawHomeAnswer();
    return begun;
  };

  const say = async (text: string, said?: Said, question?: string, at?: number) => {
    const { session, props } = now.current;
    if (at === undefined) withdrawHomeAnswer();
    setOtherCheck(null);
    const fromHome = at !== undefined ? homeAnswerNow.current : withdrawn.current?.words === text.trim() ? withdrawn.current : null;
    try {
      if (!session.open || !props.live) {
        props.claim(props.tab.key);
        await api.openSession(true, { folder, session: props.tab.session });
        dispatch({ type: "reopened" });
        props.onEngine();
      }
      spot.follow();
      setFact(null);
      dispatch({ type: "sent", shown: text, now: at ?? Date.now(), ...(said ? { said } : {}), ...(question ? { question } : {}) });
      act();
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
    setRaised(false);
    fromLine.current = sending.text;
    const task = now.current.task?.text;
    if (isHelp(sending.text)) void sayRef.current(sending.text, "help", task);
    else if (sending.talk) void sayRef.current(sending.text, "talk");
    else if (task !== undefined) void sayRef.current(sending.text, "answer", task);
    else void sayRef.current(sending.text, taskMayBeOpen(now.current.session) ? undefined : "talk");
  }, [setRaised]);
  const startRef = useRef(start);
  startRef.current = start;
  const sayRef = useRef(say);
  sayRef.current = say;

  const askHelp = useCallback((words: string) => void sayRef.current(words, "help"), []);

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

  useEffect(() => {
    const pending = tab.pending;
    if (!pending || started.current || phase === "loading") return;
    started.current = true;
    props.onStarted(tab.key);
    if (pending.kind === "start") void start();
    else if (pending.kind === "checks") void checks(pending.answer);
    else if (pending.kind === "paper") void begin(true, pending.shown, "talk", sittingTitle || "The paper", (api) => api.sendShown(pending.shown, pending.sent));
  }, [phase, tab.pending]);

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

  const turnPage = (): boolean => {
    const { page } = now.current;
    if (page === null) return false;
    if (page === "folded") setLookedBack(true);
    else {
      setLookedBack(false);
      setReading(false);
    }
    setRaised(false);
    spot.follow();
    act();
    requestAnimationFrame(() => lineRef.current?.focus());
    return true;
  };

  const panelCommand = (command: string): boolean => {
    const docs = docsNow.current;
    const { props } = now.current;
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
        if (hadKeys) window.setTimeout(() => (left.shown ? docView.current?.focus() : lineRef.current?.focus()), 120);
        return true;
      }
      case "share-document":
        box.current?.querySelector<HTMLElement>('[data-drive="share-document"]')?.click();
        return true;
      case "open-document":
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

  const deliverWords = useRef((text: string) => onEvent({ type: "text_delta", text }));
  deliverWords.current = (text: string) => onEvent({ type: "text_delta", text });
  const words = useRef<WordsBatch | null>(null);
  words.current ??= wordsBatch((text) => deliverWords.current(text), pageClock);
  useEffect(() => () => words.current?.drop(), []);

  const handle: SessionHandle = {
    event: (event) => {
      if (event.type === "text_delta") return words.current?.add(event.text);
      words.current?.flush();
      onEvent(event);
    },
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
  }, [tab.key]);

  useLayoutEffect(() => {
    box.current?.toggleAttribute("inert", !props.active);
  }, [props.active]);

  useLayoutEffect(() => {
    const el = surface.current;
    const section = nowRef.current;
    if (!el || !section) return;
    if (phase === "rest") {
      el.scrollTop = section.offsetTop;
      return;
    }
    if (restoring.current !== null) {
      spot.restore(restoring.current);
      restoring.current = null;
    }
  }, [phase, painted]);

  useEffect(() => {
    const el = surface.current;
    const section = nowRef.current;
    if (!el || !section || phase !== "lesson") return;
    const settle = () => {
      spot.settle();
      keepLineInView();
    };
    const watch = new ResizeObserver(settle);
    watch.observe(section);
    watch.observe(el);
    settle();
    return () => watch.disconnect();
  }, [phase]);

  useLayoutEffect(() => {
    if (phase !== "lesson") return;
    spot.settle();
    keepLineInView();
  });

  useEffect(() => {
    if (phase !== "loading" && props.active && !painted) requestAnimationFrame(() => setPainted(true));
  }, [phase, props.active, painted]);

  const asking = session.cards.length > 0;
  useEffect(() => {
    props.onAsking(tab.key, asking);
  }, [asking]);

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
    if (head && nowRef.current) setHeadGone(el.scrollTop > nowRef.current.offsetTop + head.offsetTop + head.offsetHeight - 66);
    setScrolled(el.scrollTop > 4);
    if (spot.scrolled()) act();
    const end = measureEnd();
    if (end === false && !now.current.raised && !lineRef.current?.holds() && document.activeElement?.closest(".session.in-front .line.lesson")) {
      (document.activeElement as HTMLElement).blur();
    }
    keepPlace(now.current.place, { fromFoot: Math.round(fromFoot) });
  };

  const measureEnd = (): boolean | null => {
    const el = surface.current;
    if (!el || el.clientHeight === 0 || now.current.phase !== "lesson") return null;
    const field = nowRef.current?.querySelector<HTMLElement>(".foot .field-row");
    const end = !field || field.getBoundingClientRect().bottom <= el.getBoundingClientRect().bottom;
    if (end !== now.current.atEnd) {
      now.current.atEnd = end;
      setAtEnd(end);
    }
    return end;
  };
  const keepLineInView = () => {
    if (measureEnd() === false && lineRef.current?.holds() && document.activeElement?.closest(".session.in-front .line.lesson")) raise();
  };
  useEffect(() => {
    if (!props.active) return;
    const frame = requestAnimationFrame(() => measureEnd());
    return () => cancelAnimationFrame(frame);
  }, [props.active, phase]);

  const turns: LessonTurn[] = useMemo(() => {
    const live = liveTurn(session);
    return live ? [...session.earlier, live] : session.earlier;
  }, [session]);
  const task = useMemo(() => openTaskOf(session, turns), [session, turns]);
  const openAt = task?.at ?? -1;
  now.current.task = task;
  const homeAnswerSent = homeAnswer !== null && turns.some((turn) => turn.ms === homeAnswer.ms);
  useEffect(() => {
    const held = homeAnswerNow.current;
    if (!held) return;
    if (turns.some((turn) => turn.ms === held.ms)) return setHomeAnswer(null);
    if (!held.ended || held.sending || held.reading || settling || isBusyNow(session)) return;
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
    const goes = heldAnswerGoes(held, { posed: held.id ? held.posed : undefined, task: task?.text ?? null, taskUnseen: task === null && taskMayBeOpen(session), reply: turns[turns.length - 1]?.tutor ?? "" });
    if (goes === "line") return withdrawHomeAnswer(true);
    const { words, ms } = held;
    setHomeAnswer({ ...held, sending: true });
    fromLine.current = words;
    void sayRef.current(words, goes === "answer" ? "answer" : undefined, task?.text, ms).then(() => {
      if (!now.current.session.earlier.some((turn) => turn.ms === ms) && now.current.session.live?.since !== ms) {
        setHomeAnswer(null);
        lineRef.current?.write(words);
      }
    });
  }, [homeAnswer, homeAnswerSent, settling, task, session, turns, setHomeAnswer, withdrawHomeAnswer]);
  const taskText_ = task?.text ?? null;
  useEffect(() => {
    props.onTaskChange(tab.key, taskText_ !== null);
  }, [taskText_, phase]);
  const inView = useRef(false);
  const questionInView = (): boolean => {
    const root = surface.current;
    const box = root && task !== null ? taskBox(root, task.text) : null;
    if (!root || !box) return (inView.current = false);
    const foot = root.querySelector<HTMLElement>(".foot");
    const pinned = foot?.querySelector<HTMLElement>(".parked");
    const view = root.getBoundingClientRect();
    const lineTop = (foot?.getBoundingClientRect().top ?? view.bottom) + (pinned?.offsetHeight ?? 0);
    const was = inView.current;
    return (inView.current = was ? box.top >= view.top + 40 && box.bottom <= lineTop + 4 : box.top >= view.top + 56 && box.bottom <= lineTop - 24);
  };
  useEffect(() => {
    window.addEventListener("resize", act);
    return () => window.removeEventListener("resize", act);
  }, [act]);
  const firstSaid = phase === "lesson" ? (turns[sittingStart]?.learner ?? "") : "";
  const titleIsFirstWords = firstSaid.trim() !== "" && sittingTitle.trim() !== "" && firstWords(firstSaid, 80) === sittingTitle;
  const ownBusy = isBusyNow(session);
  const isBusy = ownBusy || props.elsewhereBusy;
  const page: AnswerPage = phase === "lesson" && task !== null && !ownBusy && session.cards.length === 0 ? (reading || lookedBack ? "open" : "folded") : null;
  const folded = page === "folded";
  now.current.page = page;
  const live = session.live;
  const streaming = ownBusy && live?.status === "writing";
  const moment = `${acted} ${phase} ${props.panel} ${props.active} ${folded}`;
  useLayoutEffect(() => {
    if (!streaming && decided.current === moment) return;
    decided.current = moment;
    const hidden =
      task === null || raised || folded || (ownBusy && live?.said === "answer") || (settling && !streaming) || (ownBusy && live !== null && hasTask(tutorText(live)));
    setCopy(hidden || questionInView() ? null : task.text);
  });
  const turnsDrawn = !folded && turns.length > 0;
  useEffect(() => {
    const section = nowRef.current;
    if (!section) return;
    let frame = 0;
    const measure = () => {
      cancelAnimationFrame(frame);
      delete section.dataset.measured;
      if (!turnsDrawn) return;
      frame = requestAnimationFrame(() => {
        frame = requestAnimationFrame(() => (section.dataset.measured = ""));
      });
    };
    const at = () => `${section.clientWidth} ${getComputedStyle(section).fontSize} ${document.documentElement.style.cssText}`;
    let measuredAt = at();
    const check = () => {
      const now = at();
      if (now === measuredAt) return;
      measuredAt = now;
      measure();
    };
    const sized = new ResizeObserver(check);
    sized.observe(section);
    const styled = new MutationObserver(check);
    styled.observe(document.documentElement, { attributes: true, attributeFilter: ["style", "class"] });
    measure();
    return () => {
      cancelAnimationFrame(frame);
      sized.disconnect();
      styled.disconnect();
    };
  }, [turnsDrawn]);
  useEffect(() => {
    props.onAnswerPage(tab.key, page);
  }, [page]);
  useEffect(() => {
    if (!props.active) return;
    const frame = requestAnimationFrame(() => measureEnd());
    return () => cancelAnimationFrame(frame);
  }, [folded]);

  const rest = restView();
  const holdingStep = stillHeld(heldStep, session.live?.since ?? null, folded);
  useLayoutEffect(() => {
    if (heldStep !== null && !holdingStep) setHeldStep(null);
  }, [heldStep, holdingStep]);
  const stepAt = plan?.steps.findIndex((step) => step.status === "doing") ?? -1;
  const currentStep = stepAt >= 0 ? plan!.steps[stepAt].step : null;
  const restVerb = rest?.start ? (/^↩ ([a-z]+(?: on)?)/.exec(rest.keys)?.[1] ?? "start") : null;
  const restOthers = rest ? (rest.start ? rest.keys.replace(/^↩ [a-z]+(?: on)?\s*/, "") : rest.keys) : "";

  function restView(): Rest | null {
    if (phase !== "rest") return null;
    const course = home ? displayName(home.name) : null;
    const base = { course, line: false, start: null, choices: [], keys: "", skip: null };
    if (!home) return { ...base, next: "Reading this space…", where: null };
    if (home.sitting) {
      const startedAt = new Date(home.sitting.started_ms);
      const clock = (when: Date) => `${String(when.getHours()).padStart(2, "0")}:${String(when.getMinutes()).padStart(2, "0")}`;
      const ends = new Date(home.sitting.started_ms + home.sitting.minutes * 60_000);
      return {
        ...base,
        next: home.sitting.name,
        where: `Started at ${clock(startedAt)}, for ${home.sitting.minutes} minutes, until ${clock(ends)}. Write ? where you are stuck, and S, U or G after each answer for sure, unsure or a guess. Hand it in from the space’s page.`,
      };
    }
    if (home.state === "needs_upgrade") return { ...base, next: "This space needs an upgrade", where: withoutPaths(`${home.message ?? ""} Its page on Home has the upgrade.`.trim()) };
    if (home.state === "no_runtime" || home.state === "unreadable") {
      return { ...base, next: displayName(home.name), where: home.message ? withoutPaths(home.message) : null, choices: [{ label: "Check again", run: () => void refreshHome() }], keys: "" };
    }
    const item = plan?.items.find((each) => each.status === "doing") ?? plan?.items.find((each) => each.status === "todo") ?? null;
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
      where: "Say what it is about. The tutor takes it as the goal, or drop files on this window.",
      line: true,
      keys: "↩ start",
      skip,
    };
  }

  const restSend = (sending: Sending) => {
    if (sending.text.trim() !== "" || rest?.start) onSend(sending);
  };

  const putAway = useCallback(() => setRaised(false), [setRaised]);

  useEffect(() => {
    if (!props.active || phase !== "lesson") return;
    const onKey = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;
      const { props: current, session: shown } = now.current;
      if (current.covered || shown.cards.length > 0) return;
      const target = event.target instanceof HTMLElement ? event.target : null;
      if (target?.closest("input, textarea, select, [contenteditable], .layer, .card")) return;
      if (actionOf(event, current.prefs.keys)) return;
      const command = event.metaKey || event.ctrlKey;
      const paste = command && !event.altKey && !event.shiftKey && event.key.toLowerCase() === "v";
      const composing = !command && (event.isComposing || event.keyCode === 229 || event.key === "Process" || event.key === "Dead");
      const erase = !command && (event.key === "Backspace" || event.key === "Delete") && lineRef.current?.holds() === true;
      const enter = !command && event.key === "Enter" && !event.altKey && !event.shiftKey && !target?.closest("button, a, summary, [role='button'], [role='option']");
      const char = !command && !composing && event.key.length === 1 && event.key !== " ";
      if (!paste && !composing && !erase && !enter && !char) return;
      raise();
      if (paste || erase) return lineRef.current?.focus();
      const selection = window.getSelection();
      const range = selection && !selection.isCollapsed && selection.rangeCount > 0 && box.current?.contains(selection.anchorNode) ? selection.getRangeAt(0) : null;
      const selected = range ? quotable(selectedWords(range)) : "";
      const open = now.current.task;
      const passage = selected ? { text: selected, fromTask: range !== null && open !== null && inTask(range, open.text) } : undefined;
      if (composing) return lineRef.current?.write("", passage);
      event.preventDefault();
      lineRef.current?.write(char ? event.key : "", passage);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [props.active, phase, raise]);

  const onWrite = useCallback((answering: boolean) => {
    const { page, lookedBack } = now.current;
    if (page !== null && !lookedBack && answering) {
      setReading(false);
      setRaised(false);
      spot.follow();
      return;
    }
    raise();
  }, [raise, setRaised]);

  const onDismiss = useCallback((text: string) => dispatch({ type: "dismiss", text }), []);
  const again = useRef({ turns, sittingStart, sittingTitle });
  again.current = { turns, sittingStart, sittingTitle };
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
  const nameSession = (name: string) => {
    setSittingTitle(name);
    props.onTitle(props.tab.key, name);
    const id = now.current.props.tab.session;
    if (id) void api.renameSession(folder, id, name).catch(() => {});
  };
  const ownGoal = async (): Promise<{ goal: string | null; opening: string; title: string | null } | null> => {
    const id = now.current.props.tab.session;
    if (!id) return null;
    const info = (await api.lessons(folder).catch(() => [])).find((each) => each.session === id);
    return info ? { goal: info.goal ?? null, opening: info.opening, title: info.title } : null;
  };
  const nameByOwnGoal = async () => {
    const own = await ownGoal();
    const name = own ? goalName(own.goal, typedAtStart.current) : null;
    if (!provisionalTitle.current || !name || name === sittingTitle) return;
    provisionalTitle.current = false;
    nameSession(name);
  };

  const renamedOnce = useRef(false);
  useEffect(() => {
    if (renamedOnce.current || !props.tab.session) return;
    renamedOnce.current = true;
    void ownGoal().then((own) => {
      if (!own || !namedByItsWords(own)) return;
      const name = goalName(own.goal, own.opening);
      if (name && name !== own.title) nameSession(name);
    });
  }, [props.tab.session]);

  const paperPaths = useMemo(() => new Set((home?.papers ?? []).map((paper) => paper.path)), [home?.papers]);
  const isPaper = useCallback((path: string) => paperPaths.has(path) || /(^|\/)papers?\//i.test(path), [paperPaths]);
  const closesDoc = (open: Docs) => open.shown !== null && !isPaper(open.shown);
  const docClosed = folded && props.panel && closesDoc(docs);
  const docClosedNow = useRef(docClosed);
  docClosedNow.current = docClosed;
  const shownBefore = useRef(docs.shown);
  useEffect(() => {
    const was = shownBefore.current;
    shownBefore.current = docs.shown;
    if (docs.shown !== null && docs.shown !== was && now.current.page === "folded" && closesDoc(docs)) setLookedBack(true);
  }, [docs.shown]);

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
    withdrawHomeAnswer();
    interrupt();
  }, [interrupt, withdrawHomeAnswer]);
  const shownFrom = Math.max(sittingStart, turns.length - 150);
  const sittingTurns = phase === "lesson" ? turns.slice(shownFrom) : [];
  const stopKey = keyName(props.prefs.keys["stop-reply"]) || "esc";
  const keys = props.prefs.keys;
  const keyFor = useCallback((action: KeyAction) => keyName(keys[action]), [keys]);
  const moreRows = useMemo<LineRow[]>(
    () => [{ id: "model", label: "Model", detail: picker.name ?? undefined, leads: true, group: "session", run: () => pickerRef.current.toggle() }],
    [picker.name],
  );
  const paused = props.covered || !props.active || session.cards.length > 0;
  const footFact = fact && task === null && !factSeen ? fact : null;
  const lookKey = keyName(props.prefs.keys["look-back"]);
  const turnLine = (where: string) =>
    page === null ? null : (
      <button type="button" className={`plain page-turn at-${where}`} data-drive={where === "head" ? "look-back" : `look-back-${where}`} aria-expanded={!folded} onMouseDown={(event) => event.preventDefault()} onClick={() => turnPage()}>
        {lookKey && <span className="keys">{lookKey}</span>} {folded ? "Show the lesson" : "Hide the lesson"}
      </button>
    );
  const closedWords = `Closed while you answer. It opens again when you send your answer, or with ${lookKey || "“Show the lesson”"}.`;

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
        <div className="top-edge" aria-hidden={!headGone}>
          {headGone && (
            <div className="inner">
              {currentStep && !props.panel && (
                <span className="top-count" data-role="top-count">{`${stepAt + 1} of ${plan!.steps.length}`}</span>
              )}
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
                  <div className="answer-task" data-role="answer-task" onClick={(event) => !(event.target as HTMLElement).closest("a, button") && !selecting() && lineRef.current?.focus()}>
                    <Rich text={task.text} final linksOpen={false} opensNow={isPaper} onSource={onSource} onNote={note} />
                  </div>
                )}
                {[
                  ...sittingTurns,
                  ...(homeAnswer && !homeAnswerSent ? [{ session: "", ms: homeAnswer.ms, learner: homeAnswer.words, tutor: "", interrupted: false, said: "answer" } satisfies LessonTurn] : []),
                ].map((turn, at) => {
                  const index = shownFrom + at;
                  const live = index === turns.length - 1 && session.live !== null;
                  const before = index > 0 ? turns[index - 1] : null;
                  return (
                    <Turn
                      key={`${turn.ms}-${index}`}
                      turn={holdingStep && turn.step && turn.ms === heldStep ? { ...turn, step: undefined } : turn}
                      open={index === openAt}
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
                      waiting={homeAnswer !== null && !homeAnswerSent && turn.ms === homeAnswer.ms}
                    />
                  );
                })}
                {otherCheck !== null && !isBusyNow(session) && (
                  <p className="record quiet other-check" data-role="other-check">
                    Your answer from Home was not sent: this may not be the check Home asked. It is on the line, to send for this one or to change.
                  </p>
                )}
                <p className="visually-hidden" role="status" aria-live="polite" data-role="mark-said">
                  {markSaid}
                </p>
                {error && <p className="record note">{withoutPaths(error)}</p>}
                <Notices notices={session.notices} onDismiss={onDismiss} onStartAgain={startAgain} onSignIn={props.onSignIn} onPickModel={() => pickerRef.current.toggle()} />
                <Cards cards={session.cards} onDecide={onDecide} onAnswer={onAnswer} />
                {home && (
                  <div className={`foot${raised ? " raised" : ""}${session.cards.length > 0 ? " asking" : ""}`}>
                    {session.live && session.cards.length === 0 && <Working live={session.live} />}
                    {copy !== null && (
                      <button className="parked" data-drive="open-task" onMouseDown={(event) => event.preventDefault()} onClick={() => lineRef.current?.focus()}>
                        <Line text={copy} />
                      </button>
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
                      onEdit={act}
                      onPutAway={putAway}
                      onLeftEmpty={putAway}
                    />
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
