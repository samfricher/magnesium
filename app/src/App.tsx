// The window: one long page of days. Today is on top, with the one line to
// pick up; a lesson is written into it as it happens; earlier days run below.
// Everything else opens over the page as a sheet, on a key.
//
// Where the learner writes decides what a message is. Inside the open task it
// is an answer; on the page it is talk, sent so that the record can never mark
// it. The window knows a task is open from the record (the checkpoint the
// direct engine writes), or from a task the tutor set apart in bold.

import { useCallback, useEffect, useLayoutEffect, useMemo, useReducer, useRef, useState } from "react";

import { core, isDemo } from "./bridge";
import { clock, dayOf, daysBefore, longDay, today, whenSaid } from "./days";
import * as harness from "./harness";
import { busy, emptySession, HELP, isHelp, openingWords, pickedWords, reduce, tutorText, type SessionState } from "./lesson";
import { courseName, pagesFrom } from "./log";
import { plainSkill } from "./marks";
import { applyPrefs, flushLeft, keepLeft, loadLeft, loadPrefs, SIZES, type Prefs } from "./prefs";
import { taskText } from "./render";
import type { SourceRef } from "./sources";
import { Agenda, agendaLine, planChanges } from "./page/Agenda";
import { Composer, type ComposerHandle, type Place } from "./page/Composer";
import { EarlierPage } from "./page/Earlier";
import { Commands, Course, Courses, PaperSheet, PlanSheet, Settings, type Command, type SheetName } from "./page/Sheets";
import { blankMeta, Entries, type Item, type Meta } from "./page/Today";
import { Line } from "./ui/Rich";
import { SourcePanel } from "./ui/SourcePanel";
import type {
  About,
  AgentEvent,
  Answer,
  Card,
  Core,
  Decision,
  Engine,
  EngineChoice,
  Home,
  Intent,
  JournalCourse,
  Mark,
  Materials,
  Paper,
  Plan,
  Progress,
  Question,
  Setup,
} from "./types";

/** How far back the log reads at first. */
const LOG_DAYS = 120;
/** When, after a reply, the record is read again for the task it set and its mark. */
const LOOKS = [250, 1200, 3500, 9000];

/** Whether this Mac can run a lesson: the program, and Claude Code signed in. */
export function setupReady(setup: Setup | null): boolean {
  if (!setup) return false;
  const runtime = setup.runtime.state === "ready" || setup.runtime.state === "kept";
  return runtime && setup.account.installed && setup.account.signed_in;
}

/** What the window keeps of today's exchanges beyond their words, so the page reopens as it was. */
type Kept = { folder: string; ms: number; learner: string; intent: Intent | null; task: string | null; posed: string | null; plan: string[] | null };
type Stop = { folder: string; ms: number; text: string };
const TODAY_KEY = "logbook.today";

function loadKept(day: string): { items: Kept[]; stops: Stop[] } {
  try {
    const saved = JSON.parse(window.localStorage.getItem(TODAY_KEY) ?? "null") as { day: string; items: Kept[]; stops: Stop[] } | null;
    if (saved && saved.day === day) return { items: saved.items ?? [], stops: saved.stops ?? [] };
  } catch {
    // Started as new.
  }
  return { items: [], stops: [] };
}

function saveKept(day: string, items: Kept[], stops: Stop[]) {
  try {
    window.localStorage.setItem(TODAY_KEY, JSON.stringify({ day, items: items.slice(-200), stops }));
  } catch {
    // Kept for this run only.
  }
}

const sameText = (a: string, b: string) => a.replace(/\s+/g, " ").trim() === b.replace(/\s+/g, " ").trim();
const holds = (reply: string, task: string) => {
  const flat = (s: string) => s.replace(/[\s$*_`]+/g, " ").trim().toLowerCase();
  const needle = flat(task).slice(0, 28);
  return needle.length >= 12 && flat(reply).includes(needle);
};

export type Actions = {
  start: () => Promise<void>;
  say: (text: string, place?: Place) => Promise<void>;
  setPlace: (place: Place) => void;
  newLesson: (goal: string) => Promise<void>;
  stop: (line: string) => Promise<void>;
  openSheet: (sheet: SheetName | null) => void;
  openSource: (source: SourceRef | null) => void;
  openCourse: (folder: string) => Promise<void>;
  setPrefs: (prefs: Prefs) => void;
  beginStop: () => void;
};

export function App() {
  const day = useMemo(today, []);
  const [api, setApi] = useState<Core | null>(null);
  const [phase, setPhase] = useState<"loading" | "ready">("loading");
  const [setup, setSetup] = useState<Setup | null>(null);
  const [about, setAbout] = useState<About | null>(null);
  const [engine, setEngine] = useState<EngineChoice | null>(null);
  const [home, setHome] = useState<Home | null>(null);
  const [folders, setFolders] = useState<string[]>([]);
  const [cards, setCards] = useState<Card[] | null>(null);
  const [journal, setJournal] = useState<JournalCourse[] | null>(null);
  const [session, dispatch] = useReducer(reduce, emptySession);
  const [meta, setMeta] = useState<Record<number, Meta>>({});
  const [sitting, setSitting] = useState<{ folder: string; course: string; start: number } | null>(null);
  const [convEngine, setConvEngine] = useState<Engine | null>(null);
  const [openTask, setOpenTask] = useState<string | null>(null);
  // With Claude Code the window cannot always tell whether a reply set a task: the line under it then takes the reply.
  const [replyOpen, setReplyOpen] = useState(false);
  const [place, setPlaceState] = useState<Place>(() => loadLeft(day).place);
  const [plan, setPlan] = useState<Plan | null>(null);
  const [moved, setMoved] = useState<Record<string, string>>(() => loadLeft(day).moved);
  const [examWas, setExamWas] = useState<string | null>(() => loadLeft(day).examWas);
  const [sheet, setSheet] = useState<SheetName | null>(null);
  const [source, setSource] = useState<SourceRef | null>(null);
  const [prefs, setPrefsState] = useState<Prefs>(loadPrefs);
  const [notes, setNotes] = useState<string[]>([]);
  const [starting, setStarting] = useState(false);
  const [fresh, setFresh] = useState(false);
  const [stopping, setStopping] = useState<string | null>(null);
  const [kept, setKept] = useState(() => loadKept(day));
  const [materials, setMaterials] = useState<Materials | null>(null);
  const [materialLines, setMaterialLines] = useState<string[] | null>(null);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [adding, setAdding] = useState(false);
  const [finding, setFinding] = useState(false);
  const [coursesNote, setCoursesNote] = useState<string | null>(null);
  const [paper, setPaper] = useState<Paper | null>(null);
  const [dragging, setDragging] = useState(false);
  const [signingIn, setSigningIn] = useState(false);
  const [narrow, setNarrow] = useState(() => window.innerWidth < 860);

  // The two lines the learner writes on: inside the open exercise, and on the page.
  const answerBox = useRef<ComposerHandle>(null);
  const pageBox = useRef<ComposerHandle>(null);
  const composer = {
    current: {
      focus: () => {
        const wanted = now.current.place === "task" ? answerBox.current ?? pageBox.current : pageBox.current ?? answerBox.current;
        wanted?.focus();
      },
    },
  };
  const scroller = useRef<HTMLElement>(null);
  const left = useRef(loadLeft(day));
  // The learner moved the cursor, or began writing, since the last reply: the window no longer moves it.
  const touched = useRef(false);
  const answerOpenRef = useRef(false);
  // The record's mark ids the window has accounted for, and the reading taken before the last answer.
  const known = useRef<Set<string> | null>(null);
  const shownMarks = useRef(new Set<string>());

  const now = useRef({ api, home, session, setup, openTask, place, plan, convEngine, sheet, source, stopping, sitting, meta, engine, kept });
  now.current = { api, home, session, setup, openTask, place, plan, convEngine, sheet, source, stopping, sitting, meta, engine, kept };

  const note = useCallback((text: string) => setNotes((old) => [...old.filter((t) => t !== text), text].slice(-3)), []);

  const setPlace = useCallback((next: Place) => {
    touched.current = true;
    setPlaceState(next);
    now.current.place = next;
    left.current = { ...left.current, place: next };
    keepLeft(left.current);
    window.requestAnimationFrame(() => composer.current.focus());
  }, []);

  const patchMeta = useCallback((since: number, change: (old: Meta) => Meta) => {
    setMeta((all) => ({ ...all, [since]: change(all[since] ?? blankMeta()) }));
  }, []);

  // ------------------------------------------------------------ prefs

  const setPrefs = useCallback((next: Prefs) => {
    setPrefsState(next);
    applyPrefs(next);
  }, []);
  useLayoutEffect(() => applyPrefs(prefs, false), []); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    const onResize = () => setNarrow(window.innerWidth < 860);
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  // ----------------------------------------------------------- reading

  const readPlan = useCallback(async (): Promise<Plan | null> => {
    const { api } = now.current;
    if (!api) return null;
    try {
      const fresh = await api.plan();
      setPlan(fresh);
      return fresh;
    } catch {
      setPlan(null);
      return null;
    }
  }, []);

  const readJournal = useCallback(async () => {
    const { api } = now.current;
    if (!api) return;
    try {
      setJournal(await api.journal(Date.now() - LOG_DAYS * 86_400_000));
    } catch {
      setJournal([]);
    }
  }, []);

  const readLibrary = useCallback(async () => {
    const { api } = now.current;
    if (!api) return;
    try {
      const [list, all] = await Promise.all([api.recent(), api.library()]);
      setFolders(list);
      setCards(all);
    } catch (problem) {
      setCards((old) => old ?? []);
      note(String(problem));
    }
  }, [note]);

  const readEngine = useCallback(async () => {
    const { api } = now.current;
    if (!api) return;
    try {
      setEngine(await api.engine());
    } catch {
      setEngine(null);
    }
  }, []);

  /** Open a folder as the course the page works in. A lesson open in another folder is ended; it stays on disk. */
  const enterFolder = useCallback(
    async (open: (api: Core) => Promise<Home>): Promise<Home | null> => {
      const { api } = now.current;
      if (!api) return null;
      try {
        const opened = await open(api);
        // What was left belongs to the folder it was left in: another folder starts clean.
        if (left.current.folder !== opened.folder) {
          left.current = { ...left.current, folder: opened.folder, draft: "", answer: "", scroll: 0, moved: {}, examWas: null };
          setMoved({});
          setExamWas(null);
        }
        setHome(opened);
        now.current.home = opened;
        dispatch({ type: "reset" });
        setSitting(null);
        setOpenTask(null);
        setReplyOpen(false);
        setPlaceState("task");
        setProgress(null);
        setMaterials(null);
        setMaterialLines(null);
        left.current = { ...left.current, folder: opened.folder };
        keepLeft(left.current);
        void readPlan();
        void readEngine();
        return opened;
      } catch (problem) {
        note(String(problem));
        return null;
      }
    },
    [note, readEngine, readPlan],
  );

  // ------------------------------------------------------------- start

  const takeSetup = useCallback(async (api: Core, fresh: Setup) => {
    setSetup(fresh);
    if (!setupReady(fresh)) return;
    if (now.current.home) return;
    const named = await api.initialFolder();
    const list = await api.recent();
    setFolders(list);
    const first = named ?? (left.current.folder && list.includes(left.current.folder) ? left.current.folder : null) ?? list[0] ?? null;
    if (first) await enterFolder((api) => api.openFolder(first));
  }, [enterFolder]);

  useEffect(() => {
    let alive = true;
    let off = () => {};
    void (async () => {
      const api = await core();
      const unlisten = await api.onEvent((event) => onEvent.current(event));
      if (!alive) return unlisten();
      off = unlisten;
      now.current.api = api;
      setApi(api);
      const [fresh, built] = await Promise.all([api.setup(), api.about()]);
      setAbout(built);
      await takeSetup(api, fresh);
      setPhase("ready");
      void readEngine();
      if (isDemo) (await import("./demo")).startFromAddress(actions);
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
      off();
    };
    // Runs once; everything it calls reads the newest values through refs.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // The first real page is painted: the log and the courses are read behind it.
  useEffect(() => {
    if (phase !== "ready") return;
    const frame = window.requestAnimationFrame(() => {
      harness.ready();
      void readJournal();
      void readLibrary();
    });
    return () => window.cancelAnimationFrame(frame);
  }, [phase, readJournal, readLibrary]);

  // The page reopens where it was left, once the log is on it.
  const restored = useRef(false);
  useLayoutEffect(() => {
    if (restored.current || journal === null) return;
    restored.current = true;
    const el = scroller.current;
    const todayRows = el?.querySelectorAll(".today [data-ms]").length ?? 0;
    // Only a page with today's work on it is put back where it was; otherwise today's top is where the day starts.
    if (el && todayRows > 0 && left.current.day === day && left.current.scroll > 0) {
      const end = el.querySelector<HTMLElement>(".today")?.offsetHeight ?? 0;
      el.scrollTop = Math.min(left.current.scroll, Math.max(0, end - el.clientHeight + 120));
    }
  }, [journal, day]);

  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const onScroll = () => {
      left.current = { ...left.current, scroll: el.scrollTop, day };
      keepLeft(left.current);
    };
    el.addEventListener("scroll", onScroll, { passive: true });
    const hide = () => flushLeft();
    window.addEventListener("pagehide", hide);
    document.addEventListener("visibilitychange", hide);
    return () => {
      el.removeEventListener("scroll", onScroll);
      window.removeEventListener("pagehide", hide);
      document.removeEventListener("visibilitychange", hide);
    };
  }, [day]);

  // While Claude Code's sign-in waits on the browser, look every few seconds whether it finished.
  useEffect(() => {
    if (!signingIn) return;
    const started = Date.now();
    const timer = window.setInterval(() => {
      const { api, setup } = now.current;
      if (!api || !setup || Date.now() - started > 15 * 60_000) return setSigningIn(false);
      void api.account().then((account) => {
        if (account.signed_in) setSigningIn(false);
        void takeSetup(api, { ...setup, account });
      });
    }, 2000);
    return () => window.clearInterval(timer);
  }, [signingIn, takeSetup]);

  // ------------------------------------------------------------- marks

  const readMarks = async (): Promise<Mark[] | null> => {
    try {
      return (await now.current.api?.marks()) ?? null;
    } catch {
      return null;
    }
  };

  /** A mark the record holds for this exchange. Each row is shown once, beside the words it marks. */
  const addMark = useCallback(
    (since: number, mark: Mark) => {
      if (shownMarks.current.has(mark.id)) return;
      shownMarks.current.add(mark.id);
      known.current?.add(mark.id);
      patchMeta(since, (old) => ({ ...old, marks: [...old.marks, mark], unmarked: false }));
    },
    [patchMeta],
  );

  /** Read the record for the task the reply set, and where the cursor goes. */
  const settle = useCallback(
    async (since: number) => {
      const { api } = now.current;
      if (!api) return;
      const fresh = await api.rail().catch(() => null);
      const { session, convEngine, meta } = now.current;
      const live = session.live;
      if (!live || live.since !== since) return;
      const reply = tutorText(live);
      const m = meta[since] ?? blankMeta();
      const boxed = taskText(reply);
      const recorded = fresh?.task ?? null;
      let posed: string | null = null;
      if (recorded) {
        if (boxed || (m.task === null || !sameText(recorded, m.task)) || holds(reply, recorded)) posed = recorded;
      } else if (boxed) posed = boxed;
      // The record says which task is open on Magnesium; with Claude Code the
      // window knows only a task set apart in bold.
      const open = recorded ?? (convEngine === "direct" ? null : boxed);
      patchMeta(since, (old) => ({ ...old, posed: old.posed ?? posed }));
      setOpenTask(open);
      now.current.openTask = open;
      const unknown = convEngine !== "direct" && open === null && reply.trim() !== "";
      setReplyOpen(unknown);
      if (!touched.current) {
        const next: Place = posed || unknown || (open && m.intent !== "talk") ? "task" : "page";
        setPlaceState(next);
        now.current.place = next;
        window.requestAnimationFrame(() => composer.current.focus());
      }
    },
    [patchMeta],
  );

  const afterReply = useCallback(
    (since: number, intent: Intent | null) => {
      touched.current = false;
      let found = false;
      LOOKS.forEach((wait, i) =>
        window.setTimeout(async () => {
          const live = now.current.session.live;
          if (!live || live.since !== since || busy(now.current.session)) return;
          if (i < 3) void settle(since);
          if (found) return;
          const rows = await readMarks();
          if (rows === null) return;
          const before = known.current;
          known.current = new Set([...(before ?? []), ...rows.map((r) => r.id)]);
          const fresh = before ? rows.filter((row) => !before.has(row.id) && !shownMarks.current.has(row.id)) : [];
          fresh.forEach((row) => addMark(since, row));
          if (fresh.length > 0 || (now.current.meta[since]?.marks.length ?? 0) > 0) found = true;
          else if (i === LOOKS.length - 1 && intent === "answer") patchMeta(since, (old) => ({ ...old, unmarked: true }));
        }, wait),
      );
    },
    [addMark, patchMeta, settle],
  );

  // ------------------------------------------------------------ events

  const onEvent = useRef<(event: AgentEvent) => void>(() => {});
  onEvent.current = (event: AgentEvent) => {
    dispatch({ type: "event", event });
    const live = now.current.session.live;
    if (event.type === "turn_ended" && live) {
      if (event.ok && !event.interrupted) afterReply(live.since, now.current.meta[live.since]?.intent ?? null);
      else {
        touched.current = false;
        setPlaceState(now.current.openTask ? "task" : "page");
        window.requestAnimationFrame(() => composer.current.focus());
      }
    } else if (event.type === "marked" && live) {
      addMark(live.since, event.mark);
    } else if (event.type === "plan_changed") {
      const before = now.current.plan;
      void readPlan().then((after) => {
        if (!after) return;
        const { lines, moved: shifts } = planChanges(before, after);
        const since = now.current.session.live?.since;
        if (since !== undefined && lines.length > 0) patchMeta(since, (old) => ({ ...old, plan: lines }));
        if (before?.exam_date && after.exam_date !== before.exam_date) {
          setExamWas((was) => was ?? before.exam_date);
          left.current = { ...left.current, examWas: left.current.examWas ?? before.exam_date };
        }
        setMoved((old) => ({ ...old, ...shifts }));
        left.current = { ...left.current, moved: { ...left.current.moved, ...shifts }, day };
        keepLeft(left.current);
      });
      if (live) void settle(live.since);
    } else if (event.type === "ready" && event.plugin_missing) {
      void now.current.api?.interrupt().catch(() => {});
    }
  };

  // Keep what today's exchanges were, so the page reopens as it was.
  useEffect(() => {
    const { sitting } = now.current;
    if (!sitting) return;
    const turns = [...session.earlier, ...(session.live ? [{ ms: session.live.since, learner: session.live.learner }] : [])];
    const items: Kept[] = turns.flatMap((turn) => {
      const m = meta[turn.ms];
      if (!m) return [];
      return [{ folder: sitting.folder, ms: turn.ms, learner: turn.learner, intent: m.intent, task: m.task, posed: m.posed, plan: m.plan }];
    });
    if (items.length === 0) return;
    const others = kept.items.filter((k) => !items.some((i) => i.ms === k.ms && i.folder === k.folder));
    saveKept(day, [...others, ...items], kept.stops);
  }, [session.earlier, session.live, meta, kept, day]);

  // ------------------------------------------------------------ lesson

  /** Open the conversation if it is not, and send the first words of a sitting. */
  const begin = async (shown: string, send: (api: Core) => Promise<void>, resume: boolean) => {
    const { api, home } = now.current;
    if (!api || !home) return;
    if (!setupReady(now.current.setup)) {
      setSheet("settings");
      return;
    }
    setStarting(true);
    try {
      let folder = home;
      if (home.state === "not_set_up") {
        folder = await api.setUp();
        setHome(folder);
      }
      const opened = await api.openSession(resume);
      setConvEngine(opened.engine);
      dispatch({ type: "opened", turns: [] });
      const at = Date.now();
      known.current = null;
      void readMarks().then((rows) => rows && (known.current = new Set(rows.map((r) => r.id))));
      setSitting({ folder: folder.folder, course: courseName(folder.name), start: at });
      setMeta((all) => ({ ...all, [at]: { ...blankMeta(), intent: null } }));
      dispatch({ type: "sent", shown, now: at });
      touched.current = false;
      setFresh(false);
      await send(api);
      void readEngine();
    } catch (problem) {
      dispatch({ type: "failed", error: String(problem) });
      note(String(problem));
    } finally {
      setStarting(false);
      window.requestAnimationFrame(() => composer.current.focus());
    }
  };

  /** The one thing on the cold page: pick up where they stopped. */
  const start = async () => {
    const { home, api, session } = now.current;
    if (!api) return;
    if (!home) {
      const path = await api.pickFolder();
      if (path) await enterFolder((api) => api.openFolder(path));
      return;
    }
    if (home.state === "needs_upgrade") return upgrade();
    if (home.sitting || session.live || starting) return;
    const words = openingWords(
      { stoppedAt: home.last?.stopped_at ?? null, studiedBefore: Boolean(home.last), title: home.exam?.title ?? "this course" },
      false,
    );
    await begin(words, (api) => api.sendNewLesson(words), true);
  };

  const newLesson = async (goal: string) => {
    if (goal.trim() === "") return;
    await begin(goal, (api) => api.sendNewLesson(goal), true);
  };

  /** Send what the learner wrote. Its place says what it is. */
  const say = async (text: string, at: Place = now.current.place) => {
    const { api, session, openTask, convEngine } = now.current;
    if (!api) return;
    if (!session.open && !session.live) {
      // Written on the cold page: their own first line starts the lesson.
      return newLesson(text);
    }
    const help = isHelp(text);
    // The line inside the exercise takes answers; the page takes talk, which is never marked.
    const intent: Intent | undefined = help ? undefined : at === "page" ? "talk" : openTask ? "answer" : convEngine === "direct" ? "talk" : undefined;
    const since = Date.now();
    try {
      if (!session.open) {
        const opened = await api.openSession(true);
        setConvEngine(opened.engine);
        dispatch({ type: "reopened" });
      }
      void readMarks().then((rows) => rows && (known.current = new Set([...(known.current ?? []), ...rows.map((r) => r.id)])));
      setMeta((all) => ({ ...all, [since]: { ...blankMeta(), intent: intent ?? null, task: openTask } }));
      dispatch({ type: "sent", shown: text, now: since });
      touched.current = false;
      if (intent === "answer" && !help) setOpenTask(null);
      if (at === "task") setReplyOpen(false);
      if (at === "task" && !help) {
        setPlaceState("page");
        now.current.place = "page";
        window.requestAnimationFrame(() => composer.current.focus());
      }
      await api.send(text, intent);
    } catch (problem) {
      dispatch({ type: "failed", error: String(problem) });
    }
  };

  const help = (words: string) => void say(words, "task");

  const interrupt = () => {
    const { api, session } = now.current;
    if (api && busy(session)) void api.interrupt().catch(() => {});
  };

  const beginStop = () => {
    const { session, openTask, home } = now.current;
    if (!session.open && !session.live) return;
    setStopping(plainSkill(openTask ?? home?.last?.next_action ?? ""));
  };

  const stop = async (line: string) => {
    const { api, sitting, kept } = now.current;
    if (!api) return;
    const saved = line.trim() === "" ? null : line.trim();
    try {
      await api.stop(saved);
    } catch (problem) {
      note(String(problem));
    }
    setStopping(null);
    if (sitting && saved) {
      const stops = [...kept.stops, { folder: sitting.folder, ms: Date.now(), text: saved }];
      setKept((old) => ({ ...old, stops }));
      saveKept(day, loadKept(day).items, stops);
    }
    // The log now holds this sitting: read it again, and the page draws it from there.
    setKept(loadKept(day));
    await readJournal();
    dispatch({ type: "stopped" });
    setSitting(null);
    setOpenTask(null);
    setMeta({});
    const { home: was } = now.current;
    if (was) {
      try {
        setHome(await api.home());
      } catch {
        // The page keeps what it had.
      }
    }
    window.requestAnimationFrame(() => composer.current.focus());
  };

  const decide = (id: string, decision: Decision) => void now.current.api?.decide(id, decision).catch((p) => note(String(p)));
  const answerCard = (id: string, answers: Answer[]) => {
    const words = pickedWords(answers);
    if (words) dispatch({ type: "picked", shown: words, now: Date.now() });
    void now.current.api?.answer(id, answers).catch((p) => note(String(p)));
  };

  // ----------------------------------------------------------- courses

  const openCourse = async (folder: string) => {
    setSheet(null);
    if (now.current.home?.folder === folder) return;
    await enterFolder((api) => api.openFolder(folder));
    window.requestAnimationFrame(() => composer.current.focus());
  };

  const pickFolder = async () => {
    const path = await now.current.api?.pickFolder();
    if (path) await openCourse(path);
    void readLibrary();
  };

  const newFolder = async () => {
    const path = await now.current.api?.pickNewFolder();
    if (!path) return;
    setSheet(null);
    await enterFolder((api) => api.newFolder(path));
    setFresh(true);
    void readLibrary();
  };

  const findFolders = async () => {
    const { api } = now.current;
    if (!api || finding) return;
    setFinding(true);
    setCoursesNote(null);
    try {
      const added = await api.findFolders();
      setCoursesNote(added === 0 ? "No other course was found on this Mac." : `${added === 1 ? "One course" : `${added} courses`} added.`);
      await readLibrary();
    } catch (problem) {
      setCoursesNote(String(problem));
    } finally {
      setFinding(false);
    }
  };

  const forgetFolder = async (folder: string) => {
    const { api, home } = now.current;
    if (!api) return;
    await api.forgetFolder(folder).catch((p) => note(String(p)));
    if (home?.folder === folder) setHome(null);
    await readLibrary();
  };

  const upgrade = async () => {
    const { api } = now.current;
    if (!api) return;
    try {
      setHome(await api.upgradeFolder());
      note("This folder is upgraded. A copy of its record from before is in .magnesium/backups.");
    } catch (problem) {
      note(String(problem));
    }
  };

  const addMaterial = async (paths: string[]) => {
    const { api, home } = now.current;
    if (!api || !home || paths.length === 0) return;
    setAdding(true);
    try {
      const added = await api.addMaterial(paths);
      setMaterialLines(added.lines);
      note(`Added to ${courseName(home.name)}'s material. ${added.lines.join(" ")}`.trim());
      setMaterials(await api.materialList().catch(() => null));
    } catch (problem) {
      note(String(problem));
    } finally {
      setAdding(false);
    }
  };

  const addPasted = async (text: string) => {
    const { api } = now.current;
    if (!api) return;
    setAdding(true);
    try {
      setMaterialLines((await api.addPastedText(text)).lines);
      setMaterials(await api.materialList().catch(() => null));
    } catch (problem) {
      setMaterialLines([String(problem)]);
    } finally {
      setAdding(false);
    }
  };

  const openSheet = useCallback((next: SheetName | null) => {
    const { api } = now.current;
    setSheet(next);
    if (next === "course" && api) {
      void api.materialList().then(setMaterials, () => setMaterials(null));
      void api.progress(null).then(setProgress, () => setProgress(null));
    }
    if (next === "courses") void readLibrary();
    if (next === "plan") void readPlan();
    if (next === null) window.requestAnimationFrame(() => composer.current.focus());
  }, [readLibrary, readPlan]);

  const startPaper = async (minutes: number) => {
    const { api } = now.current;
    if (!api || !paper) return;
    try {
      await api.paperStart(paper, minutes);
      setHome(await api.home());
    } catch (problem) {
      note(String(problem));
    }
    setPaper(null);
    setSheet(null);
  };

  const handIn = async () => {
    const { api } = now.current;
    if (!api) return;
    try {
      const files = await api.pickFiles();
      if (files.length === 0) return;
      const words = await api.paperEnd(files);
      setHome(await api.home());
      if (words) await begin(words.shown, (api) => api.sendShown(words.shown, words.sent), true);
    } catch (problem) {
      note(String(problem));
    }
  };

  const endPaper = async () => {
    const { api } = now.current;
    if (!api) return;
    await api.paperEnd([]).catch((p) => note(String(p)));
    setHome(await api.home());
  };

  // Files dropped on the window are added to the open course's material.
  useEffect(() => {
    if (!api) return;
    let off = () => {};
    let alive = true;
    void api
      .onDrop(
        (paths) => void addMaterial(paths),
        (over) => setDragging(over && Boolean(now.current.home)),
      )
      .then((stop) => (alive ? (off = stop) : stop()));
    return () => {
      alive = false;
      off();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [api]);

  const chooseEngine = async (chosen: Engine) => {
    const { api, engine, session } = now.current;
    if (!api || !engine || session.open || session.live) return;
    setEngine({ ...engine, chosen });
    try {
      setEngine(await api.setEngine(chosen));
    } catch (problem) {
      setEngine(engine);
      note(String(problem));
    }
  };

  const choosePayment = async (payment: "plan" | "api_key") => {
    const { api, setup } = now.current;
    if (!api || !setup) return;
    const account = await api.setPayment(payment);
    setSetup({ ...setup, payment, account });
  };

  const signIn = async () => {
    try {
      await now.current.api?.signIn();
      setSigningIn(true);
    } catch (problem) {
      note(String(problem));
    }
  };

  const checkAgain = async () => {
    const { api } = now.current;
    if (!api) return;
    await takeSetup(api, await api.setup());
  };

  const runtimeAnswer = async (keep: boolean) => {
    const { api, setup } = now.current;
    if (!api || !setup) return;
    const runtime = keep ? await api.keepRuntime() : await api.replaceRuntime();
    await takeSetup(api, { ...setup, runtime });
  };

  // ------------------------------------------------------------- keys

  const scrollToDay = (direction: 1 | -1) => {
    const el = scroller.current;
    if (!el) return;
    if (direction === -1) return el.scrollTo({ top: 0 });
    const days = Array.from(el.querySelectorAll<HTMLElement>("section.day"));
    const next = days.find((d) => d.offsetTop > el.scrollTop + 8);
    if (next) el.scrollTo({ top: next.offsetTop - 12 });
  };

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const command = event.metaKey || event.ctrlKey;
      if (!command || event.altKey) {
        if (event.key === "Escape" && !event.defaultPrevented) {
          if (now.current.stopping !== null) setStopping(null);
          else if (now.current.source) setSource(null);
        }
        return;
      }
      const key = event.key.toLowerCase();
      const take = (run: () => void) => {
        event.preventDefault();
        run();
      };
      if (event.shiftKey) {
        const words = event.code === "KeyH" ? HELP.hint : event.code === "KeyL" ? HELP.lost : event.code === "KeyD" ? HELP.unknown : null;
        const { session, openTask } = now.current;
        if (words && (openTask !== null || answerOpenRef.current) && !busy(session)) take(() => help(words));
        return;
      }
      if (key === "k") take(() => openSheet(now.current.sheet === "commands" ? null : "commands"));
      else if (key === "o") take(() => openSheet("courses"));
      else if (key === "i") take(() => now.current.home && openSheet("course"));
      else if (key === "p") take(() => openSheet("plan"));
      else if (key === ",") take(() => openSheet("settings"));
      else if (key === ".") take(interrupt);
      else if (key === "e") take(beginStop);
      else if (key === "n")
        take(() => {
          setFresh(true);
          window.requestAnimationFrame(() => composer.current.focus());
        });
      else if (key === "arrowup") take(() => scrollToDay(-1));
      else if (key === "arrowdown") take(() => scrollToDay(1));
      else if (key === "=" || key === "+" || key === "-") {
        take(() => {
          const { size } = loadPrefs();
          const at = SIZES.indexOf(size);
          const next = SIZES[Math.max(0, Math.min(SIZES.length - 1, at + (key === "-" ? -1 : 1)))];
          setPrefs({ ...loadPrefs(), size: next });
        });
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // The page follows the lesson: the newest words come into view, but the
  // learner's own last line is never pushed above the top while they read.
  const followed = useRef<number | null>(null);
  useLayoutEffect(() => {
    const el = scroller.current;
    const live = session.live;
    if (!el || !live) return;
    const said = el.querySelector<HTMLElement>(`[data-ms="${live.since}"]`);
    const lines = el.querySelectorAll<HTMLElement>(".today .row.write, .today .row.stopping");
    const write = lines[lines.length - 1] ?? null;
    const top = said ? said.offsetTop - 96 : el.scrollTop;
    if (followed.current !== live.since) {
      followed.current = live.since;
      if (said && said.offsetTop > el.scrollTop + el.clientHeight * 0.6) el.scrollTop = Math.max(0, said.offsetTop - el.clientHeight * 0.3);
    }
    if (!write) return;
    const want = Math.min(write.offsetTop + write.offsetHeight + 28 - el.clientHeight, top);
    if (want > el.scrollTop) el.scrollTop = want;
  }, [session.live, meta]);

  // ---------------------------------------------------------- the page

  const actions: Actions = {
    start,
    say,
    setPlace,
    newLesson,
    stop,
    openSheet,
    openSource: setSource,
    openCourse,
    setPrefs,
    beginStop,
  };

  const pages = useMemo(() => (journal ? pagesFrom(journal) : []), [journal]);
  const todayPage = pages.find((page) => page.day === day) ?? null;
  const earlier = useMemo(() => pages.filter((page) => page.day !== day).slice(0, 60), [pages, day]);

  const items: Item[] = useMemo(() => {
    const out: Item[] = [];
    const match = (folder: string, ms: number, learner: string) =>
      kept.items.find((k) => k.folder === folder && Math.abs(k.ms - ms) < 120_000 && sameText(k.learner, learner));
    for (const part of todayPage?.sittings ?? []) {
      part.entries.forEach((entry, i) => {
        const k = match(entry.folder, entry.ms, entry.learner);
        out.push({
          key: `j-${entry.folder}-${entry.ms}`,
          ms: entry.ms,
          folder: entry.folder,
          course: entry.course,
          learner: entry.learner,
          tutor: entry.tutor,
          final: true,
          interrupted: entry.interrupted,
          meta: { ...blankMeta(), ...(k ? { intent: k.intent, task: k.task, posed: k.posed, plan: k.plan } : {}), marks: entry.marks },
          sitting: i === 0,
          status: "idle",
          doing: null,
          error: null,
        });
      });
    }
    if (sitting) {
      const last = out[out.length - 1];
      const turns: { ms: number; learner: string; tutor: string; interrupted: boolean; live: SessionState["live"] }[] = [
        ...session.earlier.map((t) => ({ ms: t.ms, learner: t.learner, tutor: t.tutor, interrupted: t.interrupted, live: null })),
        ...(session.live ? [{ ms: session.live.since, learner: session.live.learner, tutor: tutorText(session.live), interrupted: session.live.interrupted, live: session.live }] : []),
      ];
      turns.forEach((turn, i) => {
        const live = turn.live;
        out.push({
          key: `l-${turn.ms}`,
          ms: turn.ms,
          folder: sitting.folder,
          course: sitting.course,
          learner: turn.learner,
          tutor: turn.tutor,
          final: !live || live.status === "idle",
          interrupted: turn.interrupted,
          meta: meta[turn.ms] ?? blankMeta(),
          sitting: i === 0 && (!last || last.folder !== sitting.folder || sitting.start - last.ms > 3 * 3600_000),
          status: live?.status ?? "idle",
          doing: live?.doing ?? null,
          error: live?.error ?? null,
        });
      });
    }
    return out;
  }, [todayPage, kept, sitting, session.earlier, session.live, meta]);

  const lesson = Boolean(session.open || session.live);
  // What moved is said beside the agenda only while today's page shows the change.
  const changedToday = items.some((item) => (item.meta.plan?.length ?? 0) > 0);
  const writing = busy(session);
  // The open task's own line: there while a task is open (or a reply might have set one).
  const answerOpen = lesson && !starting && stopping === null && session.cards.length === 0 && (openTask !== null || (replyOpen && !writing));
  answerOpenRef.current = answerOpen;
  const photo = async () => {
    const api = now.current.api;
    if (!api) return [];
    const saved: string[] = [];
    for (const path of await api.pickFiles()) saved.push(await api.attach(path, "working"));
    return saved;
  };
  const lastItem = items[items.length - 1] ?? null;
  const lastSetsTask = lastItem !== null && lastItem.final && (lastItem.meta.posed !== null || taskText(lastItem.tutor) !== null);
  const dock = openTask !== null && lesson && !(lastSetsTask && lastItem?.final) ? openTask : null;
  // Only this library's stops: the page's storage is shared by every window home on this Mac.
  const stopsToday = useMemo(() => kept.stops.filter((s) => dayOf(s.ms) === day && folders.includes(s.folder)), [kept.stops, day, folders]);

  // The one line to pick up, when no lesson is running in this window.
  const suggestion = useMemo(() => {
    if (lesson || !setupReady(setup)) return null;
    if (!home) return folders.length === 0 ? { kind: "first" as const } : null;
    if (home.sitting) return null;
    const course = courseName(home.name);
    if (home.state === "needs_upgrade") return { kind: "upgrade" as const, course };
    if (home.state === "no_runtime" || home.state === "unreadable") return { kind: "broken" as const, course, text: home.message };
    if (home.last) {
      const lastDay = daysBefore(day, home.last.days_ago ?? 0);
      const line = home.last.goal || home.last.next_action || home.last.stopped_at;
      const stoppedAt = home.last.goal ? home.last.stopped_at : null;
      return { kind: "pick" as const, course, when: whenSaid(lastDay, day), line, at: stoppedAt };
    }
    return { kind: "begin" as const, course, line: null, exam: home.exam?.title ?? null };
  }, [lesson, setup, home, folders, day]);

  const composerHint = (() => {
    if (!lesson) return suggestion?.kind === "pick" || suggestion?.kind === "begin" ? (folders.length > 1 ? "Your other courses: ⌘O" : null) : null;
    if (writing) return "⌘. stops the reply";
    if (answerOpen) return "Esc: back to the task";
    return null;
  })();
  const composerShut = home?.sitting ? "The tutor waits while the paper runs." : !setupReady(setup) ? null : null;
  const placeholder = !lesson ? (fresh ? "What do you want to work on?" : "Or write your own first line") : answerOpen ? "Or write on the page" : "Write to the tutor";

  const commands: Command[] = [
    ...(suggestion && (suggestion.kind === "pick" || suggestion.kind === "begin") ? [{ label: "Pick up the lesson", key: "↩", run: () => void start() }] : []),
    { label: "New lesson in this course", key: "⌘N", run: () => setFresh(true) },
    { label: "End the sitting", key: "⌘E", run: beginStop },
    { label: "Stop the reply", key: "⌘.", run: interrupt },
    { label: "Courses", key: "⌘O", keywords: "library folders switch", run: () => openSheet("courses") },
    { label: "This course: material, papers, the record", key: "⌘I", keywords: "progress proofs skills level material papers", run: () => openSheet("course") },
    { label: "The plan", key: "⌘P", keywords: "agenda exam midterm schedule", run: () => openSheet("plan") },
    { label: "Settings", key: "⌘,", keywords: "font face size theme scheme dark light keys engine payment", run: () => openSheet("settings") },
    { label: "Add material", keywords: "files pdf notes slides", run: () => void now.current.api?.pickMaterial().then(addMaterial) },
    { label: "Add a photo of my working", key: "⌘⇧A", run: () => composer.current.focus() },
    { label: "Open a folder", run: () => void pickFolder() },
    { label: "New folder", run: () => void newFolder() },
    { label: "Find folders on this Mac", run: () => void findFolders() },
    { label: "Today", key: "⌘↑", run: () => scrollToDay(-1) },
    ...(home?.papers ?? []).map((p) => ({ label: `Sit ${p.name}`, keywords: "paper exam timed", run: () => (setPaper(p), setSheet("paper")) })),
  ];

  const startLabel =
    suggestion?.kind === "pick" ? `Pick up where you stopped ${suggestion.when}` : suggestion?.kind === "begin" ? (suggestion.exam ? `Start on ${suggestion.exam}` : `Start ${suggestion.course}`) : null;

  return (
    <main className="window" data-narrow={narrow ? "yes" : undefined}>
      <div className="drag" data-tauri-drag-region />
      <div className="scroller" ref={scroller as React.RefObject<HTMLDivElement>}>
        <div className="spread">
          <section className="day today" aria-label={`Today, ${longDay(day)}`} data-day={day}>
            <div className="row dateline">
              <div className="gut" />
              <h1 className="txt">{longDay(day)}</h1>
            </div>
            {narrow && plan && lesson && agendaLine(plan) && (
              <div className="row agenda-line">
                <div className="gut" />
                <button className="txt text-btn" onClick={() => openSheet("plan")} title="⌘P">
                  {agendaLine(plan)}
                </button>
              </div>
            )}

            {phase === "loading" ? null : !setupReady(setup) ? (
              <SetupRows setup={setup} signingIn={signingIn} onSignIn={signIn} onCheck={checkAgain} onRuntime={runtimeAnswer} onPayment={choosePayment} onLink={(url) => void api?.openUrl(url)} />
            ) : null}

            <Entries items={items} stops={stopsToday} openTask={openTask} onSource={(ref) => setSource(ref)} onNote={note} />

            {session.cards.map((card) => (
              <div className="row card" key={card.id}>
                <div className="gut" />
                <div className="txt">
                  {card.kind === "permission" ? (
                    <div className="ask" role="group" aria-label={card.title}>
                      <p>
                        The tutor asks to {card.title.charAt(0).toLowerCase() + card.title.slice(1)}: <code>{card.detail}</code>
                      </p>
                      <div className="actions">
                        <button className="btn" onClick={() => decide(card.id, "allow_once")}>
                          Allow once
                        </button>
                        {card.canAlways && (
                          <button className="text-btn" onClick={() => decide(card.id, "allow_always")}>
                            Always allow
                          </button>
                        )}
                        <button className="text-btn" onClick={() => decide(card.id, "refuse")}>
                          Refuse
                        </button>
                      </div>
                    </div>
                  ) : (
                    <QuestionCard id={card.id} questions={card.questions} onAnswer={answerCard} />
                  )}
                </div>
              </div>
            ))}

            {writing && session.live?.status !== "writing" && (
              <div className="row working" aria-live="polite">
                <div className="gut" />
                <div className="txt">{session.live?.doing ? `The tutor is ${session.live.doing}.` : "The tutor is thinking."}</div>
              </div>
            )}

            {[...session.notices.map((n) => n.text), ...notes].map((text) => (
              <div className="row note" key={text} role="status">
                <div className="gut" />
                <div className="txt">
                  {text}
                  <button className="text-btn quiet" onClick={() => (setNotes((old) => old.filter((t) => t !== text)), dispatch({ type: "dismiss", text }))}>
                    Dismiss
                  </button>
                </div>
              </div>
            ))}

            {home?.sitting && <PaperRows home={home} onOpen={() => void api?.openFile(home.sitting?.path ?? "")} onHandIn={handIn} onEnd={endPaper} />}

            {suggestion && !fresh && (
              <div className="row suggestion">
                <div className="gut" />
                <div className="txt">
                  {suggestion.kind === "first" ? (
                    <>
                      <button className="pick" data-drive="start" onClick={() => void start()} autoFocus>
                        <span className="pick-title">Choose the folder your course is in</span>
                        <span className="pick-line">Notes, slides, past papers: whatever you have. Nothing leaves this Mac.</span>
                      </button>
                      <div className="actions">
                        <button className="text-btn" onClick={() => void newFolder()}>
                          Make a new folder
                        </button>
                        <button className="text-btn" onClick={() => void findFolders()} disabled={finding}>
                          {finding ? "Looking…" : "Find folders already set up on this Mac"}
                        </button>
                      </div>
                      {coursesNote && <p className="quiet">{coursesNote}</p>}
                      {setup && (
                        <p className="quiet small first-note">
                          Lessons run on your own Claude Code, {setup.account.detail ? `signed in (${setup.account.detail})` : "signed in"}, paid with {setup.payment === "plan" ? "your Claude plan" : "an API key"}. Change it in Settings, ⌘,
                        </p>
                      )}
                    </>
                  ) : suggestion.kind === "upgrade" ? (
                    <button className="pick" data-drive="start" onClick={() => void upgrade()}>
                      <span className="pick-title">{suggestion.course} was set up by an earlier Magnesium</span>
                      <span className="pick-line">Upgrade it to start. A copy of its record is kept first.</span>
                    </button>
                  ) : suggestion.kind === "broken" ? (
                    <p className="quiet">{suggestion.text}</p>
                  ) : (
                    <button className="pick" data-drive="start" onClick={() => void start()} disabled={starting} aria-keyshortcuts="Enter">
                      <span className="pick-title">{startLabel}</span>
                      {suggestion.line && (
                        <span className="pick-line">
                          <LineText text={suggestion.line} />
                        </span>
                      )}
                      <span className="pick-course">
                        {suggestion.course}
                        {suggestion.kind === "pick" && suggestion.at ? `, at ${suggestion.at}` : ""}
                      </span>
                    </button>
                  )}
                </div>
              </div>
            )}

            {dock && (
              <div className="row exercise dock open" data-role="open-task">
                <div className="gut" />
                <div className="txt">
                  <button className="dock-btn" onClick={() => setPlace("task")} title="The task you left open">
                    <LineText text={dock} />
                  </button>
                </div>
              </div>
            )}

            {stopping !== null ? (
              <div className="row stopping">
                <div className="gut" />
                <div className="txt">
                  <label>
                    <span className="quiet">Where did you stop? One line for next time.</span>
                    <input
                      autoFocus
                      value={stopping}
                      onChange={(e) => setStopping(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") void stop(stopping);
                        if (e.key === "Escape") {
                          e.preventDefault();
                          setStopping(null);
                          composer.current.focus();
                        }
                      }}
                      data-drive="stop-line"
                    />
                  </label>
                </div>
              </div>
            ) : (
              <>
                {answerOpen && (
                  <div className="row write in-task" key="answer-line">
                    <div className="gut" />
                    <div className="txt">
                      <Composer
                        ref={answerBox}
                        place="task"
                        canMove
                        onMove={() => setPlace("page")}
                        onFocus={() => setPlaceState("task")}
                        tools
                        busy={writing || starting}
                        shut={composerShut}
                        placeholder=""
                        hint={writing ? null : "Esc: write on the page"}
                        draft={left.current.answer}
                        onDraft={(text) => {
                          if (text !== left.current.answer) touched.current = true;
                          left.current = { ...left.current, answer: text, day };
                          keepLeft(left.current);
                        }}
                        onSend={(text, at) => {
                          left.current = { ...left.current, answer: "" };
                          keepLeft(left.current);
                          void say(text, at);
                        }}
                        onHelp={help}
                        onPhoto={photo}
                        onNote={note}
                      />
                    </div>
                  </div>
                )}
                {(home || lesson) && setupReady(setup) && suggestion?.kind !== "upgrade" && (
                  <div className={answerOpen ? "row write page beside" : "row write page"} key="page-line">
                    <div className="gut" />
                    <div className="txt">
                      <Composer
                        ref={pageBox}
                        place="page"
                        canMove={answerOpen}
                        onMove={() => setPlace("task")}
                        onFocus={() => setPlaceState("page")}
                        tools={lesson && !answerOpen}
                        busy={writing || starting}
                        shut={composerShut}
                        placeholder={placeholder}
                        hint={composerHint}
                        draft={left.current.draft}
                        onDraft={(text) => {
                          if (text !== left.current.draft) touched.current = true;
                          left.current = { ...left.current, draft: text, day };
                          keepLeft(left.current);
                        }}
                        onSend={(text, at) => {
                          left.current = { ...left.current, draft: "" };
                          keepLeft(left.current);
                          void say(text, at);
                        }}
                        onEmptyReturn={!lesson && suggestion && (suggestion.kind === "pick" || suggestion.kind === "begin") && !fresh ? () => void start() : undefined}
                        onHelp={help}
                        onPhoto={photo}
                        onNote={note}
                      />
                    </div>
                  </div>
                )}
              </>
            )}
          </section>

          {!narrow && lesson && (
            <aside className="margin">
              <Agenda plan={plan} moved={changedToday ? moved : {}} examWas={changedToday ? examWas : null} onOpen={() => openSheet("plan")} />
            </aside>
          )}

          {earlier.map((page) => (
            <EarlierPage key={page.day} page={page} />
          ))}
          {journal !== null && earlier.length === 0 && items.length > 0 && <p className="end-of-log">Your earlier days will be here.</p>}
        </div>
      </div>

      {dragging && (
        <div className="veil" aria-hidden="true">
          <p>Add to {home ? courseName(home.name) : "this course"}'s material</p>
        </div>
      )}

      {source && <SourcePanel source={source} sheet onClose={() => (setSource(null), composer.current.focus())} onNote={note} />}
      {sheet === "commands" && <Commands commands={commands} onClose={() => openSheet(null)} />}
      {sheet === "courses" && (
        <Courses
          cards={cards}
          open={home?.folder ?? null}
          finding={finding}
          notice={coursesNote}
          onOpen={(folder) => void openCourse(folder)}
          onPick={() => void pickFolder()}
          onNew={() => void newFolder()}
          onFind={() => void findFolders()}
          onForget={(folder) => void forgetFolder(folder)}
          onClose={() => openSheet(null)}
        />
      )}
      {sheet === "course" && home && (
        <Course
          home={home}
          materials={materials}
          materialLines={materialLines}
          progress={progress}
          adding={adding}
          onAddFiles={() => void api?.pickMaterial().then(addMaterial)}
          onPaste={(text) => void addPasted(text)}
          onSitPaper={(p) => (setPaper(p), setSheet("paper"))}
          onUpgrade={() => void upgrade()}
          onClose={() => openSheet(null)}
        />
      )}
      {sheet === "plan" && <PlanSheet plan={plan} onClose={() => openSheet(null)} />}
      {sheet === "paper" && paper && <PaperSheet paper={paper} blockMinutes={home?.block_minutes ?? 50} onStart={(m) => void startPaper(m)} onClose={() => openSheet(null)} />}
      {sheet === "settings" && (
        <Settings
          prefs={prefs}
          onPrefs={setPrefs}
          engine={engine}
          inLesson={lesson}
          onEngine={(e) => void chooseEngine(e)}
          setup={setup}
          onPayment={(p) => void choosePayment(p)}
          onSignIn={() => void signIn()}
          signingIn={signingIn}
          about={about}
          onClose={() => openSheet(null)}
        />
      )}
    </main>
  );
}

// ------------------------------------------------------------ small parts

function LineText({ text }: { text: string }) {
  return <Line text={text} />;
}

function QuestionCard(props: { id: string; questions: Question[]; onAnswer: (id: string, answers: Answer[]) => void }) {
  const [chosen, setChosen] = useState<string[][]>(() => props.questions.map(() => []));
  const complete = chosen.every((c) => c.length > 0);
  return (
    <div className="ask" role="group" aria-label="The tutor asks">
      {props.questions.map((q, i) => (
        <div key={q.question} className="ask-q">
          <p>
            <Line text={q.question} />
          </p>
          <div className="options">
            {q.options.map((option) => {
              const on = chosen[i].includes(option.label);
              return (
                <button
                  key={option.label}
                  className={on ? "option chosen" : "option"}
                  aria-pressed={on}
                  onClick={() =>
                    setChosen((old) => old.map((c, j) => (j !== i ? c : q.multi ? (on ? c.filter((x) => x !== option.label) : [...c, option.label]) : [option.label])))
                  }
                >
                  <Line text={option.label} />
                  {option.description && <span className="quiet"> {option.description}</span>}
                </button>
              );
            })}
          </div>
        </div>
      ))}
      <div className="actions">
        <button className="btn" disabled={!complete} onClick={() => props.onAnswer(props.id, props.questions.map((q, i) => ({ question: q.question, chosen: chosen[i] })))}>
          Answer
        </button>
      </div>
    </div>
  );
}

function PaperRows(props: { home: Home; onOpen: () => void; onHandIn: () => void; onEnd: () => void }) {
  const sitting = props.home.sitting!;
  const [nowMs, setNow] = useState(Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 15_000);
    return () => window.clearInterval(timer);
  }, []);
  const used = Math.max(0, Math.floor((nowMs - sitting.started_ms) / 60_000));
  return (
    <div className="row paper">
      <div className="gut">{clock(sitting.started_ms)}</div>
      <div className="txt">
        <p className="paper-head">
          {sitting.name}, {sitting.minutes} minutes. {used < sitting.minutes ? `${used} used.` : "The time is up."}
        </p>
        <p className="quiet">Write ? where you are stuck, and S, U or G after each answer for how sure you were. Hand in photos or a PDF of your work when you are done.</p>
        <div className="actions">
          <button className="btn" onClick={props.onHandIn}>
            Hand in
          </button>
          {sitting.path && (
            <button className="text-btn" onClick={props.onOpen}>
              Open the paper
            </button>
          )}
          <button className="text-btn" onClick={props.onEnd}>
            End without handing in
          </button>
        </div>
      </div>
    </div>
  );
}

function SetupRows(props: {
  setup: Setup | null;
  signingIn: boolean;
  onSignIn: () => void;
  onCheck: () => void;
  onRuntime: (keep: boolean) => void;
  onPayment: (payment: "plan" | "api_key") => void;
  onLink: (url: string) => void;
}) {
  const { setup } = props;
  if (!setup) return null;
  const { runtime, account } = setup;
  const rows: React.ReactNode[] = [];
  if (runtime.state === "missing" || runtime.state === "failed") {
    rows.push(
      <p key="rt">
        Magnesium needs its program, <code>magnesium</code>, on this Mac. {runtime.message}{" "}
        <button className="text-btn" onClick={props.onCheck} data-drive="start">
          Check again
        </button>
      </p>,
    );
  } else if (runtime.state === "different") {
    rows.push(
      <div key="rt">
        <p>
          Another version of the magnesium program is installed{runtime.installed ? `, ${runtime.installed.version.split("+")[0]}` : ""}. This window comes with {runtime.bundled ? runtime.bundled.version.split("+")[0] : "its own"}. A newer program upgrades each study folder's record when it opens it, keeping a copy first.
        </p>
        <div className="actions">
          <button className="btn" onClick={() => props.onRuntime(false)} data-drive="start">
            Use the window's copy
          </button>
          <button className="text-btn" onClick={() => props.onRuntime(true)}>
            Keep the installed one
          </button>
        </div>
      </div>,
    );
  } else if (runtime.state === "too_old") {
    rows.push(
      <div key="rt">
        <p>{runtime.message ?? "The installed magnesium program is older than this window needs, so no lesson can start."}</p>
        <div className="actions">
          {runtime.bundled ? (
            <button className="btn" onClick={() => props.onRuntime(false)} data-drive="start">
              Replace it with the window's copy
            </button>
          ) : (
            <button className="text-btn" onClick={() => props.onLink("https://github.com/samfricher/magnesium/blob/main/docs/install.md")}>
              Open the install guide
            </button>
          )}
          <button className="text-btn" onClick={props.onCheck}>
            Check again
          </button>
        </div>
      </div>,
    );
  }
  if (!account.installed) {
    rows.push(
      <div key="cc">
        <p>Lessons run on Claude Code, which is not on this Mac yet. Install it, then check again.</p>
        <div className="actions">
          {account.install_page && (
            <button className="btn" onClick={() => props.onLink(account.install_page!)} data-drive={rows.length === 0 ? "start" : undefined}>
              Open the install page
            </button>
          )}
          <button className="text-btn" onClick={props.onCheck}>
            Check again
          </button>
        </div>
      </div>,
    );
  } else if (!account.signed_in) {
    rows.push(
      <div key="cc">
        <p>Claude Code is here but signed out. Signing in happens in your browser, on Anthropic's page; this window never sees your password.</p>
        <div className="actions">
          <button className="btn" onClick={props.onSignIn} disabled={props.signingIn} data-drive={rows.length === 0 ? "start" : undefined}>
            {props.signingIn ? "Waiting for the browser…" : "Sign in to Claude Code"}
          </button>
        </div>
      </div>,
    );
  }
  return (
    <div className="row setup">
      <div className="gut" />
      <div className="txt">{rows}</div>
    </div>
  );
}
