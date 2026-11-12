// The window: one surface, and the layers a key brings over it.
//
// At rest the surface holds the one next thing, already chosen, and the line
// to write on. Return starts it, and the lesson fills the same surface below
// it. Everything else (courses, the plan, material, past work, settings, all
// commands) is a layer summoned by a key and put away with Esc. Nothing here
// is a screen to move between.

import { useCallback, useEffect, useLayoutEffect, useMemo, useReducer, useRef, useState } from "react";

import { core, isDemo } from "./bridge";
import { ready } from "./harness";
import { busy, emptySession, isHelp, openingWords, opening, pickedWords, reduce, tutorText, type LessonTurn, type Said, type SessionState } from "./lesson";
import { applyPrefs, flushPlaces, keepPlace, lastCourse, loadPrefs, placeOf, savePrefs, SITTING_GAP_MS, SIZES, type Prefs } from "./prefs";
import { hasTask } from "./render";
import type { SourceRef } from "./sources";
import { Composer, type Sending } from "./surface/Composer";
import { Cards, Notices, Turn, Working } from "./surface/Conversation";
import { Layers, type LayerKind } from "./surface/Layers";
import { Line } from "./surface/Rich";
import { PasteSheet, SourceSheet } from "./surface/Sheets";
import type { About, AgentEvent, Answer, Card, Core, Decision, Engine, EngineChoice, Home, Paper, Payment, Plan, Setup, Turn as SavedTurn } from "./types";
import { displayName, firstWords, planDiff, stoppedWords, type PlanDiff } from "./words";

type Phase = "loading" | "rest" | "lesson";

export type Overlay = { kind: LayerKind } | { kind: "source"; source: SourceRef } | { kind: "paste"; text: string } | null;

/** Lessons start only when this Mac is set up: the program is usable and Claude Code is signed in. */
export function setupReady(setup: Setup | null): boolean {
  if (!setup) return false;
  const runtime = setup.runtime.state === "ready" || setup.runtime.state === "kept";
  return runtime && setup.account.installed && setup.account.signed_in;
}

/** "0.3.0" from "0.3.0+<commit>". */
const versionNumber = (version: string) => version.split("+")[0] ?? version;

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
    for (let i = turns.length - 1; i >= 0; i--) if (turns[i].posed === text) return { text, at: i };
    return { text, at: -1 };
  }
  const at = openTaskAt(turns);
  return at >= 0 ? { text: taskText(turns[at].tutor), at } : null;
}

/** The task's own words in a reply: from its bold opening word to the end. */
function taskText(reply: string): string {
  const found = [...reply.matchAll(/\*\*(?:task|check|quick check|question|your turn|try it|try this|redo|paper redo|now you)\b/gi)].pop();
  return found?.index !== undefined ? reply.slice(found.index) : reply;
}

/** One rest line: what the surface offers when no lesson is under way. */
type Rest = {
  course: string | null;
  next: string;
  where: string | null;
  status: string | null;
  /** Typing on the line starts a lesson with those words. */
  line: boolean;
  /** What Return does with an empty line, and a click on the next thing. */
  start: (() => void) | null;
  choices: { label: string; run: () => void }[];
  keys: string;
};

function Choices({ choices }: { choices: Rest["choices"] }) {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  useEffect(() => refs.current[0]?.focus({ preventScroll: true }), [choices.length]);
  return (
    <div className="choices-rest" role="group">
      {choices.map((choice, at) => (
        <button
          key={choice.label}
          ref={(el) => (refs.current[at] = el)}
          className="choice-rest"
          data-drive={at === 0 ? "start" : undefined}
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

export function App() {
  const [api, setApi] = useState<Core | null>(null);
  const [phase, setPhase] = useState<Phase>("loading");
  const [setup, setSetup] = useState<Setup | null>(null);
  const [about, setAbout] = useState<About | null>(null);
  const [engine, setEngine] = useState<EngineChoice | null>(null);
  const [home, setHome] = useState<Home | null>(null);
  const [plan, setPlan] = useState<Plan | null>(null);
  const [cards, setCards] = useState<Card[] | null>(null);
  const [session, dispatch] = useReducer(reduce, emptySession);
  // The turns of the conversation left, drawn above the next thing.
  const [before, setBefore] = useState<SavedTurn[]>([]);
  // Where this sitting's turns begin in the session's list.
  const [sittingStart, setSittingStart] = useState(0);
  const [sittingTitle, setSittingTitle] = useState<string>("");
  const [line, setLineState] = useState<"task" | "talk">("talk");
  const [overlay, setOverlay] = useState<Overlay>(null);
  const [prefs, setPrefs] = useState<Prefs>(() => loadPrefs());
  const [notes, setNotes] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const [adding, setAdding] = useState(false);
  const [signingIn, setSigningIn] = useState(false);
  // What the tutor changed in the plan in this sitting, for the plan's layer.
  const [changed, setChanged] = useState<PlanDiff | null>(null);
  // Photos dropped while the line is in a task, for the line to take.
  const [incoming, setIncoming] = useState<string[]>([]);
  const [draft, setDraft] = useState("");
  const [answerDraft, setAnswerDraft] = useState("");
  const surface = useRef<HTMLElement>(null);
  const nowRef = useRef<HTMLElement>(null);
  const stuck = useRef(true);
  const restoring = useRef<number | null>(null);
  // The look at this Mac under way at startup, for a start that comes before it is done.
  const setupWait = useRef<Promise<Setup> | null>(null);
  // The exchange whose finished reply last placed the learner's line.
  const settled = useRef<number | null>(null);

  // The newest values, for handlers that outlive a render.
  const now = useRef({ api, phase, setup, home, plan, session, overlay, line, engine, prefs, task: null as OpenTask | null });
  now.current = { api, phase, setup, home, plan, session, overlay, line, engine, prefs, task: now.current?.task ?? null };

  useLayoutEffect(() => applyPrefs(prefs), [prefs]);

  const changePrefs = useCallback((change: Partial<Prefs>) => {
    setPrefs((old) => {
      const next = { ...old, ...change };
      savePrefs(next);
      applyPrefs(next, change.scheme !== undefined);
      return next;
    });
  }, []);

  const note = useCallback((text: string) => {
    if (now.current.phase === "lesson") dispatch({ type: "note", notice: { kind: "other", text } });
    else setNotes((old) => [...old.filter((n) => n !== text), text].slice(-3));
  }, []);

  const setLine = useCallback((place: "task" | "talk") => {
    setLineState(place);
    const folder = now.current.home?.folder;
    if (folder) keepPlace(folder, { line: place });
  }, []);

  // ---------------------------------------------------------------------------
  // Reading

  const refreshHome = useCallback(async (): Promise<Home | null> => {
    const { api } = now.current;
    if (!api) return null;
    try {
      const fresh = await api.home();
      now.current.home = fresh;
      setHome(fresh);
      return fresh;
    } catch (problem) {
      setError(String(problem));
      return null;
    }
  }, []);

  const refreshPlan = useCallback(async (): Promise<Plan | null> => {
    const { api } = now.current;
    if (!api) return null;
    try {
      const fresh = await api.plan();
      now.current.plan = fresh;
      setPlan(fresh);
      return fresh;
    } catch {
      return null;
    }
  }, []);

  const loadEngine = useCallback(async () => {
    try {
      const fresh = await now.current.api?.engine();
      if (fresh) setEngine(fresh);
    } catch {
      setEngine(null);
    }
  }, []);

  const loadLibrary = useCallback(async () => {
    try {
      setCards((await now.current.api?.library()) ?? []);
    } catch (problem) {
      setError(String(problem));
    }
  }, []);

  /** Open a course on the surface, at rest, or inside the lesson it was left in. */
  const showCourse = useCallback(async (api: Core, open: () => Promise<Home>, mayReopen: boolean) => {
    setError(null);
    setNotes([]);
    try {
      const opened = await open();
      now.current.home = opened;
      setHome(opened);
      dispatch({ type: "reset" });
      setChanged(null);
      const place = placeOf(opened.folder);
      keepPlace(opened.folder, {});
      setDraft(place.draft);
      setAnswerDraft(place.answerDraft);
      const [turns, fresh] = await Promise.all([api.turns().catch(() => [] as SavedTurn[]), api.plan().catch(() => null)]);
      now.current.plan = fresh;
      setPlan(fresh);
      // Inside the lesson again only when it was left within the runtime's own
      // sitting gap, and the record agrees the lesson is from today.
      const recent = Date.now() - place.seenMs < SITTING_GAP_MS && (opened.last?.days_ago ?? 99) === 0;
      if (mayReopen && place.inLesson && recent && opened.state === "ready" && !opened.sitting && turns.length > 0) {
        dispatch({ type: "opened", turns });
        // Where the line was is put back as it was, not worked out again.
        settled.current = turns[turns.length - 1].ms;
        // The session is not running: the first message sent starts it again.
        setBefore([]);
        const start = sittingStartOf(turns);
        setSittingStart(start);
        setSittingTitle(opened.last?.goal ?? displayName(opened.name));
        setLineState(place.line);
        restoring.current = place.fromFoot;
        setPhase("lesson");
      } else {
        setBefore(turns);
        setLineState("talk");
        setPhase("rest");
        keepPlace(opened.folder, { inLesson: false });
      }
      void loadEngine();
    } catch (problem) {
      setError(String(problem));
      setHome(null);
      setPhase("rest");
    }
  }, [loadEngine]);

  const openFolder = useCallback((api: Core, path: string) => showCourse(api, () => api.openFolder(path), false), [showCourse]);

  // ---------------------------------------------------------------------------
  // Starting up

  useEffect(() => {
    let alive = true;
    let unlisten = () => {};
    void (async () => {
      const api = await core();
      const off = await api.onEvent((event: AgentEvent) => onEventRef.current(event));
      if (!alive) return off();
      unlisten = off;
      now.current.api = api;
      setApi(api);
      // The program is set up, or a question about it answered, while the
      // course is read: the digest that reads a course never upgrades it.
      const [initial, recent] = await Promise.all([api.initialFolder(), api.recent().catch(() => [] as string[])]);
      const remembered = lastCourse();
      const folder = initial ?? (remembered && recent.includes(remembered) ? remembered : (recent[0] ?? null));
      // The course is read and drawn while this Mac is looked at: the digest
      // that reads it never upgrades a record. Nothing starts until the look
      // is done, and a Mac that needs a step shows that step instead; the
      // course is read again if the program was only just put in place.
      const looking = api.setup();
      setupWait.current = looking;
      void api.about().then(setAbout, () => {});
      if (folder) await showCourse(api, () => api.openFolder(folder), true);
      const fresh = await looking;
      now.current.setup = fresh;
      setSetup(fresh);
      if (!setupReady(fresh)) {
        dispatch({ type: "reset" });
        setPhase("rest");
      } else if (folder && fresh.runtime.installed_now) await showCourse(api, () => api.openFolder(folder), true);
      else if (!folder) setPhase("rest");
      void loadEngine();
      if (isDemo) {
        await (await import("./demo")).startFromAddress(actions);
        document.documentElement.dataset.shot = "ready";
      }
      else if (await api.selftest()) {
        try {
          await (await import("./selftest")).run(api, actions, () => now.current.session);
        } catch (problem) {
          await api.selftestReport({ ok: false, failure: `the check could not start: ${String(problem)}`, errors: [...window.__mgErrors], steps: {} });
        }
      }
    })();
    return () => {
      alive = false;
      unlisten();
    };
    // Runs once: every handler it uses reads the newest values through refs.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // The first screen is painted with its real content: tell the harness.
  useEffect(() => {
    if (phase !== "loading" && (home !== null || setup !== null)) requestAnimationFrame(() => ready());
  }, [phase, home, setup]);

  // While Claude Code's sign-in waits for the browser, look every few seconds whether it finished.
  useEffect(() => {
    if (!signingIn) return;
    const started = Date.now();
    const timer = window.setInterval(() => {
      const { api, setup } = now.current;
      if (!api || !setup || Date.now() - started > 15 * 60_000) return setSigningIn(false);
      void api.account().then(async (account) => {
        const fresh = { ...setup, account };
        setSetup(fresh);
        if (account.signed_in) {
          setSigningIn(false);
          await afterSetup(api, fresh);
        }
      });
    }, 2000);
    return () => window.clearInterval(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signingIn]);

  /** Once this Mac is set up, the course comes up on the surface. */
  const afterSetup = async (api: Core, fresh: Setup) => {
    if (!setupReady(fresh) || now.current.home) return;
    const recent = await api.recent().catch(() => [] as string[]);
    const folder = lastCourse() ?? recent[0] ?? null;
    if (folder) await showCourse(api, () => api.openFolder(folder), false);
  };

  const checkAgain = async () => {
    const { api } = now.current;
    if (!api) return;
    const fresh = await api.setup();
    setSetup(fresh);
    await afterSetup(api, fresh);
  };

  // ---------------------------------------------------------------------------
  // The tutor's events

  const onEventRef = useRef<(event: AgentEvent) => void>(() => {});
  onEventRef.current = (event: AgentEvent) => {
    dispatch({ type: "event", event });
    if (event.type === "ready" && event.plugin_missing) void now.current.api?.interrupt().catch(() => {});
    if (event.type === "turn_ended") void refreshHome();
    // The engine says which task is open: a task the reply just posed takes
    // the learner's line; after help with it, the line goes back in; after
    // anything else it is on the conversation's line.
    if (event.type === "open_task" && now.current.phase === "lesson") {
      const said = now.current.session.live?.said;
      if (event.task && event.posed) setLine("task");
      else if (event.task && said === "help") setLine("task");
      else setLine("talk");
    }
    if (event.type === "plan_changed") {
      const was = now.current.plan;
      void refreshPlan().then((fresh) => {
        if (!fresh) return;
        const diff = planDiff(was, fresh);
        if (diff.changes.length === 0) return;
        dispatch({ type: "planned", change: diff });
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
    if (!now.current.setup && setupWait.current) await setupWait.current.catch(() => null);
    const { api, setup, home } = now.current;
    if (!api || !home) return false;
    if (!setupReady(setup)) {
      setError("This Mac is not set up for lessons yet.");
      return false;
    }
    setError(null);
    setNotes([]);
    setSittingTitle(title);
    setChanged(null);
    stuck.current = true;
    setPhase("lesson");
    keepPlace(home.folder, { inLesson: true });
    try {
      const opened = await api.openSession(resume);
      dispatch({ type: "opened", turns: opened.turns });
      setBefore([]);
      setSittingStart(opened.turns.length);
      dispatch({ type: "sent", shown, now: Date.now(), said });
      setLineState("talk");
      await send(api);
      void loadEngine();
      return true;
    } catch (problem) {
      dispatch({ type: "failed", error: String(problem) });
      return false;
    }
  };

  /** The one next thing: carry on where the learner stopped, or start what the plan has next. */
  const start = async (words?: string, skipChecks = false): Promise<boolean> => {
    const { api, home, plan } = now.current;
    if (!api || !home) return false;
    let current = home;
    if (home.state === "not_set_up") {
      try {
        current = await api.setUp();
        setHome(current);
      } catch (problem) {
        setError(String(problem));
        return false;
      }
    }
    const typed = words?.trim() ?? "";
    const item = plan?.items.find((each) => each.status === "doing") ?? plan?.items.find((each) => each.status === "todo") ?? null;
    const title = current.last?.goal ?? item?.title ?? displayName(current.name);
    if (typed !== "") {
      // Words typed first are the start: the tutor takes them as the lesson's goal.
      return begin(current.can_resume, typed, "talk", title, (api) => api.sendNewLesson(typed));
    }
    const shown = openingWords(
      { stoppedAt: current.last?.stopped_at ?? null, studiedBefore: Boolean(current.last), title: current.last ? title : (item?.title ?? displayName(current.name)) },
      skipChecks,
    );
    return begin(current.can_resume, shown, "opening", title, (api) => api.sendNewLesson(shown));
  };

  const newLesson = async (goal: string) => {
    if (goal.trim() === "") return;
    await begin(false, goal.trim(), "talk", goal.trim(), (api) => api.sendNewLesson(goal.trim()));
  };

  /** Send the learner's words, with what they are. */
  const say = async (text: string, said: Said = "talk", question?: string) => {
    const { api, session } = now.current;
    if (!api) return;
    try {
      if (!session.open) {
        await api.openSession(true);
        dispatch({ type: "reopened" });
      }
      stuck.current = true;
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

  const askHelp = (words: string) => {
    setOverlay(null);
    void say(words, "help");
  };

  /** End the sitting where it is. The line saved for next time is the open task's first words. */
  const stop = async () => {
    const { api, session } = now.current;
    if (!api) return;
    const turns = [...session.earlier, ...(liveTurn(session) ? [liveTurn(session)!] : [])];
    const open = openTaskOf(session, turns);
    const line = open ? firstWords(open.text) : null;
    try {
      await api.stop(line);
    } catch (problem) {
      setError(String(problem));
    }
    dispatch({ type: "stopped" });
    setBefore(turns);
    if (now.current.home) keepPlace(now.current.home.folder, { inLesson: false });
    await refreshHome();
    setPhase("rest");
  };

  const interrupt = () => void now.current.api?.interrupt().catch((problem) => note(String(problem)));

  // ---------------------------------------------------------------------------
  // Courses, material, papers, setup

  const switchCourse = async (path: string) => {
    const { api } = now.current;
    if (!api) return;
    setOverlay(null);
    await openFolder(api, path);
  };

  const pickFolder = async () => {
    const { api } = now.current;
    if (!api) return;
    setOverlay(null);
    const path = await api.pickFolder();
    if (path) await openFolder(api, path);
  };

  const newFolder = async () => {
    const { api } = now.current;
    if (!api) return;
    setOverlay(null);
    const path = await api.pickNewFolder();
    if (path) await showCourse(api, () => api.newFolder(path), false);
  };

  const findFolders = async () => {
    const { api } = now.current;
    if (!api) return;
    try {
      const added = await api.findFolders();
      note(added === 0 ? "No other course was found on this Mac." : `${added} ${added === 1 ? "course" : "courses"} added.`);
      await loadLibrary();
    } catch (problem) {
      note(String(problem));
    }
  };

  const forgetFolder = async (path: string) => {
    const { api, home } = now.current;
    if (!api) return;
    await api.forgetFolder(path).catch((problem) => note(String(problem)));
    if (home?.folder === path) {
      setHome(null);
      setPhase("rest");
    }
    await loadLibrary();
  };

  const canAdd = () => {
    const home = now.current.home;
    return Boolean(home) && ["ready", "empty", "not_set_up"].includes(home?.state ?? "") && !home?.sitting;
  };

  const addMaterial = async (paths: string[]) => {
    const { api } = now.current;
    if (!api || paths.length === 0 || !canAdd()) return;
    setAdding(true);
    try {
      const added = await api.addMaterial(paths);
      added.lines.forEach(note);
    } catch (problem) {
      note(String(problem));
    } finally {
      setAdding(false);
    }
  };

  const chooseMaterial = async () => {
    const { api } = now.current;
    if (!api || !canAdd()) return;
    setOverlay(null);
    try {
      await addMaterial(await api.pickMaterial());
    } catch (problem) {
      note(String(problem));
    }
  };

  const savePasted = async (text: string) => {
    const { api } = now.current;
    if (!api) return;
    setAdding(true);
    try {
      const added = await api.addPastedText(text);
      setOverlay(null);
      added.lines.forEach(note);
    } catch (problem) {
      setOverlay(null);
      note(String(problem));
    } finally {
      setAdding(false);
    }
  };

  // Files dropped on the window: with the answer when the line is in a task, into the course's material otherwise.
  const dropRef = useRef<(paths: string[]) => void>(() => {});
  dropRef.current = (paths: string[]) => {
    const { api, phase, line, session } = now.current;
    if (!api) return;
    const turns = [...session.earlier, ...(liveTurn(session) ? [liveTurn(session)!] : [])];
    if (phase === "lesson" && line === "task" && openTaskOf(session, turns) !== null) {
      void (async () => {
        try {
          const saved: string[] = [];
          for (const path of paths) saved.push(await api.attach(path, "working"));
          setIncoming(saved);
        } catch (problem) {
          note(String(problem));
        }
      })();
    } else void addMaterial(paths);
  };
  useEffect(() => {
    if (!api) return;
    let off = () => {};
    let alive = true;
    void api
      .onDrop(
        (paths) => dropRef.current(paths),
        (over) => setDragging(over && canAdd()),
      )
      .then((stop) => (alive ? (off = stop) : stop()));
    return () => {
      alive = false;
      off();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [api]);

  const upgrade = async () => {
    const { api } = now.current;
    if (!api) return;
    try {
      setHome(await api.upgradeFolder());
      note("This course is upgraded. The copy of its record from before is in .magnesium/backups.");
    } catch (problem) {
      setError(String(problem));
    }
  };

  const sitPaper = async (paper: Paper) => {
    const { api, home } = now.current;
    if (!api || !home) return;
    setOverlay(null);
    try {
      await api.paperStart(paper, paper.minutes ?? home.block_minutes);
      await refreshHome();
    } catch (problem) {
      note(String(problem));
    }
  };

  const handIn = async () => {
    const { api, home } = now.current;
    if (!api) return;
    try {
      const files = await api.pickFiles();
      if (files.length === 0) return;
      const words = await api.paperEnd(files);
      const fresh = await refreshHome();
      if (words && fresh) await begin(true, words.shown, "talk", home?.sitting?.name ?? "The paper", (api) => api.sendShown(words.shown, words.sent));
    } catch (problem) {
      note(String(problem));
    }
  };

  const endPaper = async () => {
    try {
      await now.current.api?.paperEnd([]);
    } catch (problem) {
      note(String(problem));
    }
    await refreshHome();
  };

  const signIn = async () => {
    const { api } = now.current;
    if (!api) return;
    try {
      await api.signIn();
      setSigningIn(true);
    } catch (problem) {
      setError(String(problem));
    }
  };

  const answerRuntime = async (work: (api: Core) => Promise<Setup["runtime"]>) => {
    const { api, setup } = now.current;
    if (!api || !setup) return;
    const runtime = await work(api);
    const fresh = { ...setup, runtime };
    setSetup(fresh);
    await afterSetup(api, fresh);
  };

  const choosePayment = async (payment: Payment) => {
    const { api, setup } = now.current;
    if (!api || !setup) return;
    setSetup({ ...setup, payment });
    const account = await api.setPayment(payment);
    setSetup({ ...setup, payment, account });
  };

  /** Pick the engine for the next lesson. Saved at once; refused while a lesson is open. */
  const chooseEngine = async (chosen: Engine) => {
    const { api, engine, session } = now.current;
    if (!api || !engine || session.open || session.live !== null || engine.chosen === chosen) return;
    setEngine({ ...engine, chosen });
    try {
      setEngine(await api.setEngine(chosen));
    } catch (problem) {
      setEngine(engine);
      note(String(problem));
    }
  };

  const openSource = useCallback((source: SourceRef) => setOverlay({ kind: "source", source }), []);

  // ---------------------------------------------------------------------------
  // Keys

  const toggle = useCallback((kind: LayerKind) => {
    setOverlay((old) => (old && old.kind === kind ? null : { kind }));
    if (kind === "courses" || kind === "commands") void loadLibrary();
    if (kind === "plan") void refreshPlan();
  }, [loadLibrary, refreshPlan]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const { phase, overlay, session, line, prefs } = now.current;
      const command = event.metaKey || event.ctrlKey;
      const key = event.key.toLowerCase();
      if (command && !event.altKey && !event.shiftKey) {
        const layers: Record<string, LayerKind> = { k: "commands", o: "courses", p: "plan", e: "material", y: "work", ",": "settings" };
        if (layers[key]) {
          event.preventDefault();
          if (now.current.setup && setupReady(now.current.setup)) toggle(layers[key]);
          else if (key === "," || key === "k") toggle(layers[key]);
          return;
        }
        if (key === "/") {
          event.preventDefault();
          if (phase === "lesson") toggle("help");
          return;
        }
        if (key === ".") {
          event.preventDefault();
          if (busy(session)) interrupt();
          return;
        }
        if (key === "=" || key === "+" || key === "-" || key === "0") {
          event.preventDefault();
          const at = SIZES.indexOf(prefs.size);
          const size = key === "0" ? 17 : SIZES[Math.max(0, Math.min(SIZES.length - 1, at + (key === "-" ? -1 : 1)))];
          changePrefs({ size });
          return;
        }
        if (key === "n" && phase === "rest" && now.current.home) {
          event.preventDefault();
          setOverlay(null);
          setNewLesson(true);
          return;
        }
      }
      if (event.key === "Escape" && !event.defaultPrevented) {
        if (overlay) {
          event.preventDefault();
          setOverlay(null);
        } else if (busy(session)) {
          event.preventDefault();
          interrupt();
        } else if (phase === "lesson" && line === "task") {
          event.preventDefault();
          setLine("talk");
        } else if (newLessonRef.current) {
          setNewLesson(false);
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const [newLessonAsked, setNewLesson] = useState(false);
  const newLessonRef = useRef(false);
  newLessonRef.current = newLessonAsked;

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
  }, [phase, before.length]);

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

  const onScroll = () => {
    const el = surface.current;
    const folder = now.current.home?.folder;
    if (!el || now.current.phase !== "lesson") return;
    const fromFoot = Math.max(0, el.scrollHeight - el.clientHeight - el.scrollTop);
    stuck.current = fromFoot < 48;
    if (folder) keepPlace(folder, { fromFoot: Math.round(fromFoot) });
  };

  useEffect(() => {
    const save = () => flushPlaces();
    window.addEventListener("pagehide", save);
    return () => window.removeEventListener("pagehide", save);
  }, []);

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
  // The task's question is said above its line unless the reply just above posed it.
  const restated = task && openAt !== turns.length - 1 ? firstWords(task.text, 160) : null;
  const isBusy = busy(session);

  const rest = restView();

  function restView(): Rest | null {
    if (phase !== "rest") return null;
    const course = home ? displayName(home.name) : null;
    const base = { course, status: null, line: false, start: null, choices: [], keys: "" };
    if (!setup && !home) return { ...base, next: "Looking at this Mac…", where: null };
    // While this Mac is still being looked at, the course is drawn as it will be.
    const runtime = setup?.runtime ?? { state: "ready" as const, installed: null, bundled: null, installed_now: false, message: null };
    const account = setup?.account ?? { name: "Claude Code", installed: true, signed_in: true, paid_with: null, detail: null, problem: null, install_page: null };
    if (runtime.state === "different" && runtime.installed && runtime.bundled) {
      return {
        ...base,
        next: "Another magnesium program is on this Mac",
        where: `Version ${versionNumber(runtime.installed.version)} is at ${runtime.installed.shown_path}. This app carries ${versionNumber(runtime.bundled.version)}. A newer program upgrades each study folder's record when it opens it, after keeping a copy.`,
        choices: [
          { label: `Use this app's ${versionNumber(runtime.bundled.version)}`, run: () => void answerRuntime((api) => api.replaceRuntime()) },
          { label: `Keep ${versionNumber(runtime.installed.version)}`, run: () => void answerRuntime((api) => api.keepRuntime()) },
        ],
        keys: "↑ ↓ choose   ↩ do it",
      };
    }
    if (runtime.state === "too_old" || runtime.state === "missing" || runtime.state === "failed") {
      const guide = "https://github.com/samfricher/magnesium/blob/main/docs/install.md";
      return {
        ...base,
        next: runtime.state === "too_old" ? "The magnesium program on this Mac is too old for this window" : "The magnesium program could not be set up",
        where: runtime.message,
        choices: [
          ...(runtime.bundled && runtime.state === "too_old" ? [{ label: "Replace it with this app's copy", run: () => void answerRuntime((api) => api.replaceRuntime()) }] : []),
          ...(!runtime.bundled ? [{ label: "Open the install guide", run: () => void api?.openUrl(guide) }] : []),
          { label: "Check again", run: () => void checkAgain() },
        ],
        keys: "↑ ↓ choose   ↩ do it",
      };
    }
    if (!account.installed) {
      return {
        ...base,
        next: `Magnesium teaches through ${account.name}, which is not on this Mac`,
        where: `${account.problem ?? ""} It needs a paid Claude plan or an Anthropic Console account.`.trim(),
        choices: [
          ...(account.install_page ? [{ label: "Open Anthropic's install page", run: () => void api?.openUrl(account.install_page!) }] : []),
          { label: "Check again", run: () => void checkAgain() },
        ],
        keys: "↑ ↓ choose   ↩ do it",
      };
    }
    if (!account.signed_in) {
      return {
        ...base,
        next: `Sign in to ${account.name}`,
        where: signingIn
          ? "Waiting for the sign-in to finish in your browser."
          : (account.problem ?? `${account.name}'s own sign-in opens Anthropic's page in your browser. This window never sees your password.`),
        choices: [
          { label: "Sign in", run: () => void signIn() },
          { label: "Check again", run: () => void checkAgain() },
        ],
        keys: "↑ ↓ choose   ↩ do it",
      };
    }
    const paid = account.paid_with === "api_key" ? "an API key" : "your Claude plan";
    const ready = `${account.name} is signed in. Lessons are paid with ${paid}.`;
    if (!home) {
      return {
        ...base,
        next: "Open the folder your course is in",
        where: "Your notes, slides and past papers can be in it already, or be added later.",
        status: account.problem ?? ready,
        choices: [
          { label: "Choose a folder…", run: () => void pickFolder() },
          { label: "Make a new folder…", run: () => void newFolder() },
          { label: "Find folders on this Mac", run: () => void findFolders() },
        ],
        keys: "↑ ↓ choose   ↩ do it   ⌘, settings",
      };
    }
    if (home.sitting) {
      const started = new Date(home.sitting.started_ms);
      const at = `${String(started.getHours()).padStart(2, "0")}:${String(started.getMinutes()).padStart(2, "0")}`;
      return {
        ...base,
        next: home.sitting.name,
        where: `Started at ${at}, for ${home.sitting.minutes} minutes. Write ? where you are stuck, and S, U or G after each answer for sure, unsure or a guess.`,
        choices: [
          { label: "Hand in your work…", run: () => void handIn() },
          ...(home.sitting.path ? [{ label: "Open the paper again", run: () => void api?.openFile(home.sitting!.path!) }] : []),
          { label: "End without handing in", run: () => void endPaper() },
        ],
        keys: "↑ ↓ choose   ↩ do it",
      };
    }
    if (home.state === "needs_upgrade") {
      return {
        ...base,
        next: "An earlier Magnesium set this folder up",
        where: home.message,
        choices: [
          { label: "Upgrade it", run: () => void upgrade() },
          { label: "Not now", run: () => toggle("courses") },
        ],
        keys: "↑ ↓ choose   ↩ do it",
      };
    }
    if (home.state === "no_runtime" || home.state === "unreadable") {
      return { ...base, next: displayName(home.name), where: home.message, choices: [{ label: "Check again", run: () => void refreshHome() }], keys: "⌘O another course" };
    }
    if (newLessonAsked) {
      return { ...base, next: "A new lesson", where: "Say what it is about. The tutor takes it as the goal.", line: true, keys: "↩ start it   esc not now" };
    }
    const item = plan?.items.find((each) => each.status === "doing") ?? plan?.items.find((each) => each.status === "todo") ?? null;
    if (home.state === "ready" && home.last) {
      const where = home.last.stopped_at ? stoppedWords(home.last.stopped_at) : home.last.next_action ? `Next: ${home.last.next_action}` : null;
      return { ...base, next: home.last.goal, where, line: true, start: () => void start(), keys: "↩ carry on   ⌘K everything else" };
    }
    if (item) return { ...base, next: item.title, where: item.note, line: true, start: () => void start(), keys: "↩ start   ⌘K everything else" };
    return {
      ...base,
      next: displayName(home.name),
      where: "Type what you want to work on, or drop your notes and slides on this window.",
      line: true,
      keys: "↩ start   ⌘E material   ⌘K everything else",
    };
  }

  const actions: Actions = {
    start,
    newLesson,
    say,
    stop,
    open: (kind) => (kind ? (setOverlay({ kind }), kind === "courses" || kind === "commands" ? void loadLibrary() : undefined) : setOverlay(null)),
    setLine,
    switchCourse,
    addMaterial,
    pasteText: (text) => setOverlay({ kind: "paste", text }),
    openSource,
    chooseEngine,
    sitPaper,
    setDragging,
    askHelp,
    session: () => now.current.session,
  };

  /** Return on the rest line: the next thing, or the words typed as the start. */
  const restSend = (sending: Sending) => {
    if (newLessonAsked) {
      setNewLesson(false);
      void newLesson(sending.text);
    } else if (sending.text.trim() !== "" || rest?.start) onSend(sending);
  };

  const onHold = useCallback(() => setLine("task"), [setLine]);
  const onDismiss = useCallback((text: string) => dispatch({ type: "dismiss", text }), []);
  const onDecide = useCallback((id: string, decision: Decision) => {
    void now.current.api?.decide(id, decision).then(
      () => dispatch({ type: "event", event: { type: "withdrawn", id } }),
      (problem) => note(String(problem)),
    );
  }, [note]);
  const onAnswer = useCallback((id: string, answers: Answer[]) => {
    dispatch({ type: "picked", shown: pickedWords(answers), now: Date.now() });
    void now.current.api?.answer(id, answers).then(
      () => dispatch({ type: "event", event: { type: "withdrawn", id } }),
      (problem) => note(String(problem)),
    );
  }, [note]);

  const shownBefore: LessonTurn[] = phase === "rest" ? before.slice(-30) : turns.slice(Math.max(0, sittingStart - 30), sittingStart);
  const sittingTurns = phase === "lesson" ? turns.slice(sittingStart) : [];

  return (
    <div className="app" data-phase={phase} data-busy={isBusy ? "yes" : "no"}>
      <div className="drag" data-tauri-drag-region />
      <main className="surface" ref={surface} onScroll={onScroll} aria-hidden={overlay !== null ? true : undefined}>
        <div className="column">
          {shownBefore.length > 0 && (
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
                  onSource={openSource}
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
                  {rest.course && <p className="course">{rest.course}</p>}
                  {rest.start ? (
                    <button className="next" data-drive="start" onClick={rest.start}>
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
                    key={`rest-${home.folder}-${newLessonAsked}`}
                    place="rest"
                    folder={home.folder}
                    initial={draft}
                    placeholder={rest.start ? "or say what you would rather do" : undefined}
                    active
                    busy={false}
                    paused={overlay !== null}
                    canAnswer={false}
                    onSend={restSend}
                    onMove={() => {}}
                    onNote={note}
                  />
                )}
                {rest.choices.length > 0 && <Choices choices={rest.choices} />}
                {rest.status && <p className="status">{rest.status}</p>}
                {(error || notes.length > 0) && (
                  <div className="notices">
                    {error && <p className="record note">{error}</p>}
                    {notes.map((text) => (
                      <p key={text} className="record note">
                        {text}
                      </p>
                    ))}
                  </div>
                )}
                {rest.keys && <p className="keys">{rest.keys}</p>}
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
                      showQuestion={!before || !(before.posed === turn.question || hasTask(before.tutor))}
                      onSource={openSource}
                      onNote={note}
                      onHold={onHold}
                    />
                  );
                })}
                {session.live && <Working live={session.live} />}
                <Notices notices={session.notices} onDismiss={onDismiss} />
                <Cards cards={session.cards} onDecide={onDecide} onAnswer={onAnswer} />
                {home && task && (
                  <div className={`task-line${held ? " on" : ""}${restated ? "" : " joined"}`}>
                    {restated && (
                      <p className="restated" onClick={() => setLine("task")}>
                        <Line text={restated} />
                      </p>
                    )}
                    <Composer
                      key={`answer-${home.folder}-${task.text}`}
                      place="task"
                      folder={home.folder}
                      initial={answerDraft}
                      active={held}
                      busy={isBusy}
                      paused={overlay !== null || session.cards.length > 0}
                      canAnswer
                      incoming={incoming}
                      onSend={onSend}
                      onMove={setLine}
                      onNote={note}
                    />
                  </div>
                )}
                {home && (
                  <Composer
                    key={`talk-${home.folder}`}
                    place="talk"
                    folder={home.folder}
                    initial={draft}
                    active={!held}
                    busy={isBusy}
                    paused={overlay !== null || session.cards.length > 0}
                    canAnswer={task !== null}
                    placeholder={task ? "or say something else" : undefined}
                    onSend={onSend}
                    onMove={setLine}
                    onNote={note}
                  />
                )}
              </>
            )}
          </section>
        </div>
      </main>
      {dragging && home && (
        <div className="veil" aria-hidden="true">
          <p>{phase === "lesson" && held ? "Drop to hand in with your answer" : `Drop to add to ${displayName(home.name)}'s material`}</p>
        </div>
      )}
      {overlay?.kind === "source" ? (
        <SourceSheet source={overlay.source} onClose={() => setOverlay(null)} onNote={note} />
      ) : overlay?.kind === "paste" ? (
        <PasteSheet initial={overlay.text} saving={adding} onSave={(text) => void savePasted(text)} onClose={() => setOverlay(null)} />
      ) : overlay ? (
        <Layers
          kind={overlay.kind}
          api={api}
          home={home}
          plan={plan}
          changed={changed}
          cards={cards}
          setup={setup}
          about={about}
          engine={engine}
          prefs={prefs}
          inLesson={phase === "lesson"}
          lessonOpen={session.open || session.live !== null}
          taskOpen={task !== null}
          busy={isBusy}
          sources={sourcesIn(turns)}
          onClose={() => setOverlay(null)}
          open={(kind) => setOverlay({ kind })}
          prefsChange={changePrefs}
          act={{
            start: () => (setOverlay(null), void start()),
            startSkip: () => (setOverlay(null), void start(undefined, true)),
            setBlock: (minutes) => {
              void api?.setBlockMinutes(minutes);
              setHome((old) => (old ? { ...old, block_minutes: minutes } : old));
            },
            say: (text) => {
              setOverlay(null);
              if (now.current.phase === "lesson") void say(text, "talk");
              else void start(text);
            },
            sayKeep: (text) => {
              if (now.current.phase === "lesson") void say(text, "talk");
              else void start(text);
            },
            help: askHelp,
            stop: () => (setOverlay(null), void stop()),
            interrupt,
            newLesson: () => (setOverlay(null), setNewLesson(true)),
            switchCourse: (path) => void switchCourse(path),
            pickFolder: () => void pickFolder(),
            newFolder: () => void newFolder(),
            findFolders: () => void findFolders(),
            forgetFolder: (path) => void forgetFolder(path),
            chooseMaterial: () => void chooseMaterial(),
            paste: () => setOverlay({ kind: "paste", text: "" }),
            openFile: (path) => void api?.openFile(path).catch((problem) => note(String(problem))),
            sitPaper: (paper) => void sitPaper(paper),
            handIn: () => (setOverlay(null), void handIn()),
            endPaper: () => (setOverlay(null), void endPaper()),
            chooseEngine: (chosen) => void chooseEngine(chosen),
            choosePayment: (payment) => void choosePayment(payment),
            signIn: () => (setOverlay(null), void signIn()),
            openSource: (source) => setOverlay({ kind: "source", source }),
          }}
        />
      ) : null}
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
  };
}

/** The sources cited in the lesson's replies, newest first, for ⌘K. */
function sourcesIn(turns: LessonTurn[]): { text: string; href: string }[] {
  const found: { text: string; href: string }[] = [];
  for (const turn of turns.slice(-12).reverse()) {
    for (const match of turn.tutor.matchAll(/\[([^\]]+)\]\(([^)\s]+)\)/g)) {
      if (!found.some((each) => each.href === match[2])) found.push({ text: match[1], href: match[2] });
    }
  }
  return found;
}

export type Actions = {
  start: (words?: string) => Promise<boolean>;
  newLesson: (goal: string) => Promise<void>;
  say: (text: string, said?: Said) => Promise<void>;
  stop: () => Promise<void>;
  open: (kind: LayerKind | null) => void;
  setLine: (place: "task" | "talk") => void;
  switchCourse: (path: string) => Promise<void>;
  addMaterial: (paths: string[]) => Promise<void>;
  pasteText: (text: string) => void;
  openSource: (source: SourceRef) => void;
  chooseEngine: (engine: Engine) => Promise<void>;
  sitPaper: (paper: Paper) => Promise<void>;
  setDragging: (over: boolean) => void;
  askHelp: (words: string) => void;
  session: () => SessionState;
};
