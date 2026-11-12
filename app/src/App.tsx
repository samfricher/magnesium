// The window is the editor of one study folder. The folder's documents (its
// lesson, its plan, its files, the record) open in tabs, in one pane or two,
// and are found with ⌘P. There are no screens to move between: the window
// reopens on the documents, the places and the drafts it was left with.

import { useCallback, useEffect, useLayoutEffect, useMemo, useReducer, useRef, useState } from "react";

import { core, isDemo } from "./bridge";
import { FileDoc } from "./docs/FileDoc";
import { LessonDoc, type Confidence, type HelpKind, type Place } from "./docs/Lesson";
import { PlanDoc } from "./docs/Plan";
import { RecordDoc } from "./docs/Record";
import { SettingsDoc } from "./docs/Settings";
import { SetupDoc, setupReady } from "./docs/Setup";
import type { WriterHandle } from "./docs/Writer";
import { ready as harnessReady } from "./harness";
import { emptySession, HELP, openingWords, reduce, type LessonTurn, type SessionState } from "./lesson";
import { readPage, type Meta, type Said } from "./page";
import { applyPrefs, readPrefs, savePrefs, SIZE_MAX, SIZE_MIN, type Prefs } from "./prefs";
import type { SourceRef } from "./sources";
import { splitTask, taskLine } from "./task";
import type {
  About,
  AgentEvent,
  Answer,
  Core,
  Decision,
  Engine,
  EngineChoice,
  Home,
  LessonInfo,
  Mark,
  Paper,
  Payment,
  Plan,
  Progress,
  RuntimeSetup,
  Setup,
  StudyFiles,
  Turn,
} from "./types";
import { Sheet, type SheetItem } from "./ui/Sheet";
import {
  closeTab,
  fileKind,
  fileName,
  findDoc,
  flushLayout,
  freshLayout,
  openBeside,
  openIn,
  readLayout,
  saveLayout,
  toggleSplit,
  type DocId,
  type Layout,
} from "./workspace";

const SIGN_IN_POLL_MS = 2000;
const SIGN_IN_LIMIT_MS = 15 * 60_000;
/** Below this width a second pane becomes a tab. */
const SPLIT_MIN = 880;

type SheetKind = "open" | "commands" | "folders" | "symbols";

/** Words for the page: mathematics and markup taken out, for a tab or a title bar. */
export function plainText(text: string): string {
  return text
    .replace(/\\text\{([^}]*)\}/g, "$1")
    .replace(/_\{([^}]*)\}/g, "$1")
    .replace(/\\([A-Za-z]+)/g, "$1")
    .replace(/[$*_`{}^]/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

const folderName = (path: string) => path.replace(/\/+$/, "").split("/").pop() ?? path;

/** The persisted meta of an exchange, by its words: what it was, and the record's marks for it. */
const exKey = (learner: string, tutor: string) => `${learner}␟${tutor.slice(0, 80)}`;

function readKept(folder: string): Record<string, Meta> {
  try {
    return JSON.parse(localStorage.getItem(`mg.ex:${folder}`) ?? "{}") as Record<string, Meta>;
  } catch {
    return {};
  }
}

function writeKept(folder: string, kept: Record<string, Meta>): void {
  try {
    const entries = Object.entries(kept);
    localStorage.setItem(`mg.ex:${folder}`, JSON.stringify(Object.fromEntries(entries.slice(-400))));
  } catch {
    // Nothing kept in a private window.
  }
}

const SYMBOLS: [string, string][] = [
  ["x̄", "mean, x bar"],
  ["σ", "sigma"],
  ["μ", "mu"],
  ["√", "square root"],
  ["≤", "less than or equal"],
  ["≥", "greater than or equal"],
  ["±", "plus or minus"],
  ["×", "times"],
  ["÷", "divided by"],
  ["∑", "sum"],
  ["∞", "infinity"],
  ["≠", "not equal"],
  ["≈", "about equal"],
  ["$x_{}$", "subscript"],
  ["$x^{}$", "superscript"],
  ["$\\frac{}{}$", "fraction"],
];

export type Actions = {
  continueLesson: () => Promise<void>;
  newLesson: (goal: string) => Promise<void>;
  answer: (text: string, confidence?: Confidence | null) => Promise<boolean>;
  talk: (text: string) => Promise<boolean>;
  help: (kind: HelpKind) => Promise<void>;
  stop: () => Promise<void>;
  openDoc: (doc: DocId, beside?: boolean) => void;
  setSheet: (sheet: SheetKind | null) => void;
  setPlace: (place: Place) => void;
  setLayout: (layout: Layout) => void;
  sitPaper: (paper: Paper) => Promise<void>;
  addMaterial: (paths: string[]) => Promise<void>;
  setDragging: (over: boolean) => void;
  setPrefs: (prefs: Prefs) => void;
  freshLesson: () => void;
  session: () => SessionState;
};

export function App() {
  const [api, setApi] = useState<Core | null>(null);
  const [setup, setSetup] = useState<Setup | null>(null);
  const [about, setAbout] = useState<About | null>(null);
  const [engine, setEngine] = useState<EngineChoice | null>(null);
  const [signingIn, setSigningIn] = useState(false);
  const [folder, setFolder] = useState<string | null>(null);
  const [home, setHome] = useState<Home | null>(null);
  const [layout, setLayoutState] = useState<Layout>(freshLayout);
  const [prefs, setPrefsState] = useState<Prefs>(readPrefs);
  const [session, dispatch] = useReducer(reduce, emptySession);
  const [metaVersion, bumpMeta] = useReducer((n: number) => n + 1, 0);
  const [plan, setPlan] = useState<Plan | null>(null);
  const [planBefore, setPlanBefore] = useState<string | null>(null);
  const [planChangedAt, setPlanChangedAt] = useState<number | null>(null);
  const [lessons, setLessons] = useState<LessonInfo[]>([]);
  const [files, setFiles] = useState<StudyFiles | null>(null);
  const [recordMarks, setRecordMarks] = useState<Mark[] | null>(null);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [earlier, setEarlier] = useState<Record<string, Turn[]>>({});
  const [title, setTitle] = useState<string | null>(null);
  const [sheet, setSheet] = useState<SheetKind | null>(null);
  const [place, setPlaceState] = useState<Place>("talk");
  const [focusTick, bumpFocus] = useReducer((n: number) => n + 1, 0);
  const [fresh, setFresh] = useState(false);
  const [flash, setFlash] = useState<string[] | null>(null);
  const [dragging, setDragging] = useState(false);
  const [recent, setRecent] = useState<string[]>([]);
  const [finding, setFinding] = useState(false);
  const [findNote, setFindNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [booted, setBooted] = useState(false);
  const [width, setWidth] = useState(() => window.innerWidth);
  const [now, setNow] = useState(() => Date.now());

  const meta = useRef(new Map<number, Meta>());
  const kept = useRef<Record<string, Meta>>({});
  const writers = useRef<Record<Place, WriterHandle | null>>({ answer: null, talk: null });
  const scrollers = useRef<Record<number, HTMLElement | null>>({});
  const accountBefore = useRef("");

  // The newest values, for handlers that outlive a render.
  const latest = useRef({ api, setup, folder, home, layout, session, plan, place, engine, prefs, title, sheet, fresh, width });
  latest.current = { api, setup, folder, home, layout, session, plan, place, engine, prefs, title, sheet, fresh, width };

  const setLayout = useCallback((next: Layout) => {
    setLayoutState(next);
    const f = latest.current.folder;
    if (f) saveLayout(f, next);
  }, []);

  /** Change the layout without a new render: drafts, scroll and pages are kept, not drawn. */
  const keepQuietly = useCallback((change: (layout: Layout) => Layout) => {
    const next = change(latest.current.layout);
    latest.current.layout = next;
    const f = latest.current.folder;
    if (f) saveLayout(f, next);
  }, []);

  const setPrefs = useCallback((next: Prefs) => {
    setPrefsState((old) => {
      applyPrefs(next, old);
      return next;
    });
    savePrefs(next);
  }, []);

  useLayoutEffect(() => applyPrefs(prefs), []); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    const onResize = () => setWidth(window.innerWidth);
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  const setPlace = useCallback((next: Place) => {
    setPlaceState(next);
    bumpFocus();
  }, []);

  const note = useCallback((text: string) => dispatch({ type: "note", notice: { kind: "other", text } }), []);

  // ---------------------------------------------------------------------------
  // Reading the folder

  const refreshPlan = useCallback(async (changed: boolean) => {
    const { api } = latest.current;
    if (!api) return;
    try {
      const before = latest.current.plan?.text ?? null;
      const fresh = await api.plan();
      setPlan(fresh);
      if (changed && before !== null && fresh.text !== null && fresh.text !== before) {
        setPlanBefore(before);
        setPlanChangedAt(Date.now());
        const f = latest.current.folder;
        try {
          if (f) localStorage.setItem(`mg.planBase:${f}`, JSON.stringify({ before, at: Date.now(), after: fresh.changed_ms }));
        } catch {
          // Not kept.
        }
      }
      return fresh;
    } catch {
      return null;
    }
  }, []);

  const refreshFiles = useCallback(async () => {
    const { api } = latest.current;
    if (!api) return;
    try {
      setFiles(await api.studyFiles());
    } catch {
      setFiles(null);
    }
  }, []);

  const refreshLessons = useCallback(async (): Promise<LessonInfo[]> => {
    const { api } = latest.current;
    if (!api) return [];
    try {
      const list = await api.lessons();
      setLessons(list);
      return list;
    } catch {
      return [];
    }
  }, []);

  const refreshHome = useCallback(async () => {
    const { api } = latest.current;
    if (!api) return null;
    try {
      const fresh = await api.home();
      setHome(fresh);
      return fresh;
    } catch {
      return null;
    }
  }, []);

  const refreshRecord = useCallback(async () => {
    const { api } = latest.current;
    if (!api) return;
    const [marks, progress] = await Promise.all([api.marks().catch(() => null), api.progress(null).catch(() => null)]);
    setRecordMarks(marks);
    setProgress(progress);
  }, []);

  const loadEngine = useCallback(async () => {
    const { api } = latest.current;
    if (!api) return;
    try {
      setEngine(await api.engine());
    } catch {
      setEngine(null);
    }
  }, []);

  /** Open a study folder: its documents as they were left, and its lesson's page, without starting the agent. */
  const openFolder = useCallback(
    async (path: string, open: (api: Core) => Promise<Home> = (api) => api.openFolder(path)) => {
      const { api } = latest.current;
      if (!api) return false;
      setError(null);
      flushLayout();
      try {
        const opened = await open(api);
        const folderPath = opened.folder;
        meta.current.clear();
        kept.current = readKept(folderPath);
        const saved = readLayout(folderPath);
        setFolder(folderPath);
        latest.current.folder = folderPath;
        setHome(opened);
        setLayoutState(saved);
        latest.current.layout = saved;
        setFresh(false);
        setPlanBefore(null);
        setPlanChangedAt(null);
        setTitle(opened.last?.goal ?? null);
        setPlaceState("talk");
        // The plan as it was before a change the learner has not looked at yet.
        try {
          const base = JSON.parse(localStorage.getItem(`mg.planBase:${folderPath}`) ?? "null") as { before: string; at: number } | null;
          if (base) {
            setPlanBefore(base.before);
            setPlanChangedAt(base.at);
          }
        } catch {
          // Nothing kept.
        }
        const [list] = await Promise.all([refreshLessons(), refreshPlan(false)]);
        const current = list.find((lesson) => lesson.current);
        const turns = current ? await api.lessonTurns(current.session).catch(() => [] as Turn[]) : [];
        dispatch({ type: "loaded", turns });
        void refreshFiles();
        void loadEngine();
        setRecent(await api.recent().catch(() => [] as string[]));
        return true;
      } catch (problem) {
        setError(String(problem));
        return false;
      }
    },
    [loadEngine, refreshFiles, refreshLessons, refreshPlan],
  );

  // ---------------------------------------------------------------------------
  // Setup

  const takeSetup = useCallback(async (fresh: Setup) => {
    setSetup(fresh);
  }, []);

  const checkAgain = useCallback(async () => {
    const { api } = latest.current;
    if (!api) return;
    setError(null);
    await takeSetup(await api.setup());
  }, [takeSetup]);

  const answerRuntime = async (work: (api: Core) => Promise<RuntimeSetup>) => {
    const { api, setup } = latest.current;
    if (!api || !setup) return;
    const runtime = await work(api);
    await takeSetup({ ...setup, runtime });
  };

  const signIn = async () => {
    const { api } = latest.current;
    if (!api) return;
    try {
      accountBefore.current = JSON.stringify(latest.current.setup?.account ?? null);
      await api.signIn();
      setSigningIn(true);
    } catch (problem) {
      setError(String(problem));
    }
  };

  useEffect(() => {
    if (!signingIn) return;
    const started = Date.now();
    const timer = window.setInterval(() => {
      const { api, setup } = latest.current;
      if (!api || !setup) return;
      if (Date.now() - started > SIGN_IN_LIMIT_MS) {
        setSigningIn(false);
        return;
      }
      void api.account().then((account) => {
        if (account.signed_in && (!account.problem || JSON.stringify(account) !== accountBefore.current)) setSigningIn(false);
        void takeSetup({ ...(latest.current.setup ?? setup), account });
      });
    }, SIGN_IN_POLL_MS);
    return () => window.clearInterval(timer);
  }, [signingIn, takeSetup]);

  const choosePayment = async (payment: Payment) => {
    const { api, setup } = latest.current;
    if (!api || !setup) return;
    setSetup({ ...setup, payment });
    const account = await api.setPayment(payment);
    await takeSetup({ ...setup, payment, account });
  };

  const chooseEngine = async (chosen: Engine) => {
    const { api, engine, session } = latest.current;
    if (!api || !engine || session.open || engine.chosen === chosen) return;
    setEngine({ ...engine, chosen });
    try {
      setEngine(await api.setEngine(chosen));
    } catch (problem) {
      setEngine(engine);
      setError(String(problem));
    }
  };

  // ---------------------------------------------------------------------------
  // The lesson

  /** Note what a message was before it is sent, so the page sets it where it belongs. */
  const sent = (shown: string, said: Said) => {
    const at = Date.now();
    meta.current.set(at, { said, marks: [] });
    dispatch({ type: "sent", shown, now: at });
    bumpMeta();
  };

  /** Keep what each finished exchange was, by its words, so it reads the same when the folder opens again. */
  const keepExchanges = useCallback(() => {
    const { session, folder } = latest.current;
    if (!folder) return;
    const turns: LessonTurn[] = [...session.earlier];
    if (session.live && session.live.status === "idle") {
      const live = session.live;
      turns.push({ session: "", ms: live.since, learner: live.learner, tutor: [...live.done, live.streaming].join("\n"), interrupted: live.interrupted });
    }
    let changed = false;
    for (const turn of turns) {
      const m = meta.current.get(turn.ms);
      if (!m) continue;
      const k = exKey(turn.learner, turn.tutor);
      if (JSON.stringify(kept.current[k]) !== JSON.stringify(m)) {
        kept.current[k] = m;
        changed = true;
      }
    }
    if (changed) writeKept(folder, kept.current);
  }, []);

  const ensureOpen = async (api: Core) => {
    if (latest.current.session.open) return;
    await api.openSession(true);
    dispatch({ type: "reopened" });
  };

  const guardReady = () => {
    if (!setupReady(latest.current.setup)) {
      openDoc("setup");
      return false;
    }
    const state = latest.current.home?.state;
    if (state === "needs_upgrade" || state === "no_runtime" || state === "unreadable") return false;
    return true;
  };

  const begin = async (resume: boolean, shown: string, said: Said, send: (api: Core) => Promise<void>) => {
    const { api } = latest.current;
    if (!api || !guardReady()) return;
    setError(null);
    try {
      const opened = await api.openSession(resume);
      if (!resume) {
        meta.current.clear();
        dispatch({ type: "opened", turns: [] });
      } else if (!latest.current.session.live && opened.turns.length > 0) {
        dispatch({ type: "opened", turns: opened.turns });
      } else {
        dispatch({ type: "reopened" });
      }
      setFresh(false);
      sent(shown, said);
      showLesson();
      await send(api);
      void loadEngine();
      if (!resume) void refreshLessons();
    } catch (problem) {
      dispatch({ type: "failed", error: String(problem) });
    }
  };

  const continueLesson = async () => {
    const { home, plan, title } = latest.current;
    const words = openingWords(
      { stoppedAt: home?.last?.stopped_at ?? null, studiedBefore: Boolean(home?.last), title: plan?.title ?? title ?? "this course" },
      false,
    );
    await begin(true, words, "opening", (api) => api.sendNewLesson(words));
  };

  const newLesson = async (goal: string) => {
    const { api, home } = latest.current;
    if (!api) return;
    const words = goal.trim() || `Let's work on ${latest.current.plan?.title ?? "this course"}.`;
    try {
      if (home?.state === "not_set_up" || home?.state === "empty") setHome(await api.setUp());
    } catch (problem) {
      setError(String(problem));
      return;
    }
    setTitle(goal.trim() || latest.current.plan?.title || null);
    await begin(false, words, "talk", (api) => api.sendNewLesson(words));
  };

  const answer = async (text: string, confidence: Confidence | null = null, photos: string[] = []): Promise<boolean> => {
    const { api } = latest.current;
    if (!api || (text === "" && photos.length === 0)) return false;
    let words = text;
    if (photos.length > 0) words = `${words}\n\n${photos.length === 1 ? "Photo" : "Photos"} of my working: ${photos.join(", ")}`.trim();
    if (confidence) words = `${words}\n\nConfidence: ${confidence}`;
    try {
      await ensureOpen(api);
      sent(words, "answer");
      await api.send(words, "answer");
      return true;
    } catch (problem) {
      dispatch({ type: "failed", error: String(problem) });
      return false;
    }
  };

  const talk = async (text: string): Promise<boolean> => {
    const { api } = latest.current;
    if (!api || text === "") return false;
    if (!guardReady()) return false;
    try {
      await ensureOpen(api);
      sent(text, "talk");
      await api.send(text, "talk");
      return true;
    } catch (problem) {
      dispatch({ type: "failed", error: String(problem) });
      return false;
    }
  };

  const help = async (kind: HelpKind) => {
    const { api, session } = latest.current;
    if (!api || (session.live && session.live.status !== "idle")) return;
    try {
      await ensureOpen(api);
      sent(HELP[kind], "help");
      // The engine reads these exact words, as it always has.
      await api.send(HELP[kind]);
    } catch (problem) {
      dispatch({ type: "failed", error: String(problem) });
    }
  };

  const page = useMemo(
    () =>
      readPage(session, (turn) => {
        const m = meta.current.get(turn.ms);
        if (m) return m;
        return kept.current[exKey(turn.learner, turn.tutor)] ?? null;
      }),
    // metaVersion: a mark or a kind was noted.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [session, metaVersion],
  );

  /** Close the lesson: the stop line is the open task, so the next sitting starts from it. */
  const stop = async () => {
    const { api, session } = latest.current;
    if (!api || !session.open) return;
    const open = page.open !== null ? page.exchanges[page.open]?.split?.task : null;
    const line = open ? taskLine(open) : null;
    keepExchanges();
    try {
      await api.stop(line);
    } catch (problem) {
      setError(String(problem));
    }
    dispatch({ type: "closed" });
    void refreshHome();
    void refreshLessons();
  };

  // The record's mark for the last message goes beside it.
  const takeMark = useCallback((mark: Mark) => {
    const { session } = latest.current;
    const keys = [...session.earlier.map((turn) => turn.ms), ...(session.live ? [session.live.since] : [])].reverse();
    const target =
      keys.find((key) => {
        const said = meta.current.get(key)?.said;
        return said === "answer" || said === "help";
      }) ?? keys[0];
    if (target === undefined) return;
    const m = meta.current.get(target) ?? { said: "answer" as Said, marks: [] };
    if (m.marks.some((row) => row.id === mark.id)) return;
    meta.current.set(target, { ...m, marks: [...m.marks, mark] });
    bumpMeta();
    window.setTimeout(keepExchanges, 0);
  }, [keepExchanges]);

  const onPlanChanged = useCallback(async () => {
    const fresh = await refreshPlan(true);
    if (!fresh) return;
    const { layout, width } = latest.current;
    const where = findDoc(layout, "plan");
    const visible = where !== null && layout.panes[where].active === "plan" && (layout.panes.length > 1 || where === layout.focus);
    if (visible) return;
    if (width >= SPLIT_MIN) setLayout({ ...openBeside(layout, "plan"), focus: findDoc(layout, "lesson") ?? 0 });
    else setLayout(openIn(layout, "plan", layout.focus));
    // The lesson keeps the keyboard: the plan opened beside it.
    if (width >= SPLIT_MIN) bumpFocus();
  }, [refreshPlan, setLayout]);

  // ---------------------------------------------------------------------------
  // Documents

  const showLesson = () => {
    const { layout } = latest.current;
    const where = findDoc(layout, "lesson");
    if (where === null) setLayout(openIn(layout, "lesson", 0));
    else setLayout({ ...layout, focus: where, panes: layout.panes.map((p, i) => (i === where ? { ...p, active: "lesson" } : p)) });
    bumpFocus();
  };

  const openDoc = useCallback(
    (doc: DocId, beside = false) => {
      const { layout, width } = latest.current;
      if (doc === "record") void refreshRecord();
      if (doc.startsWith("lesson:")) {
        const sid = doc.slice(7);
        void latest.current.api?.lessonTurns(sid).then((turns) => setEarlier((old) => ({ ...old, [sid]: turns })));
      }
      if (beside && width >= SPLIT_MIN) setLayout({ ...openBeside(layout, doc), focus: 1 });
      else {
        const where = findDoc(layout, doc);
        if (where !== null) setLayout({ ...layout, focus: where, panes: layout.panes.map((p, i) => (i === where ? { ...p, active: doc } : p)) });
        else setLayout(openIn(layout, doc));
      }
      if (doc === "lesson") bumpFocus();
    },
    [refreshRecord, setLayout],
  );

  const closeDoc = (doc: DocId, pane: number) => {
    if (doc === "lesson" && latest.current.session.open) void stop();
    setLayout(closeTab(latest.current.layout, doc, pane));
  };

  const onSource = useCallback(
    (source: SourceRef) => {
      const doc = `file:${source.path}`;
      if (source.page) keepQuietly((layout) => ({ ...layout, pages: { ...layout.pages, [doc]: source.page ?? 1 } }));
      openDoc(doc, true);
    },
    [keepQuietly, openDoc],
  );

  // ---------------------------------------------------------------------------
  // Material, papers, photos

  const addMaterial = async (paths: string[]) => {
    const { api, folder } = latest.current;
    if (!api || !folder || paths.length === 0) return;
    try {
      const added = await api.addMaterial(paths);
      setFlash(added.lines);
      void refreshFiles();
    } catch (problem) {
      setFlash([String(problem)]);
    }
  };

  const pickMaterial = async () => {
    const { api } = latest.current;
    if (!api) return;
    await addMaterial(await api.pickMaterial());
  };

  const sitPaper = async (paper: Paper) => {
    const { api, home } = latest.current;
    if (!api) return;
    try {
      await api.paperStart(paper, paper.minutes ?? home?.block_minutes ?? 50);
      await refreshHome();
    } catch (problem) {
      setError(String(problem));
    }
  };

  const handIn = async () => {
    const { api } = latest.current;
    if (!api) return;
    try {
      const files = await api.pickFiles();
      if (files.length === 0) return;
      const words = await api.paperEnd(files);
      await refreshHome();
      if (words) await begin(true, words.shown, "answer", (api) => api.sendShown(words.shown, words.sent));
    } catch (problem) {
      setError(String(problem));
    }
  };

  const endPaper = async () => {
    const { api } = latest.current;
    if (!api) return;
    try {
      await api.paperEnd([]);
    } catch (problem) {
      setError(String(problem));
    }
    await refreshHome();
  };

  const photos = useCallback(async (): Promise<string[]> => {
    const { api } = latest.current;
    if (!api) return [];
    try {
      const picked = await api.pickFiles();
      const group = new Date().toISOString().slice(0, 10);
      const saved: string[] = [];
      for (const path of picked) saved.push(await api.attach(path, group));
      return saved;
    } catch (problem) {
      note(String(problem));
      return [];
    }
  }, [note]);

  // ---------------------------------------------------------------------------
  // Folders

  const pickFolder = async () => {
    const { api } = latest.current;
    if (!api) return;
    const path = await api.pickFolder();
    if (path) await openFolder(path);
  };

  const newFolder = async () => {
    const { api } = latest.current;
    if (!api) return;
    const path = await api.pickNewFolder();
    if (path) await openFolder(path, (api) => api.newFolder(path));
  };

  const findFolders = async () => {
    const { api } = latest.current;
    if (!api || finding) return;
    setFinding(true);
    try {
      const added = await api.findFolders();
      setFindNote(added === 0 ? "No other study folder was found." : `${added} found.`);
      setRecent(await api.recent());
      if (added > 0) setSheet("folders");
    } finally {
      setFinding(false);
    }
  };

  const upgrade = async () => {
    const { api } = latest.current;
    if (!api) return;
    try {
      setHome(await api.upgradeFolder());
      setFlash(["This folder is upgraded. If its record changed, the copy from before is in .magnesium/backups."]);
    } catch (problem) {
      setError(String(problem));
    }
  };

  // ---------------------------------------------------------------------------
  // Start

  useEffect(() => {
    let alive = true;
    let off = () => {};
    void (async () => {
      const api = await core();
      off = await api.onEvent((event: AgentEvent) => {
        if (event.type === "marked") {
          takeMark(event.mark);
          return;
        }
        if (event.type === "plan_changed") {
          void onPlanChanged();
          return;
        }
        dispatch({ type: "event", event });
        if (event.type === "turn_ended") {
          window.setTimeout(() => {
            keepExchanges();
            // A reply that set a task puts the writing place inside it, unless the learner is writing elsewhere.
            const live = latest.current.session.live;
            const tutor = live ? [...live.done, live.streaming].join("\n") : "";
            const talking = (writers.current.talk?.value() ?? "").trim() !== "";
            if (live && !live.interrupted && splitTask(tutor) && !talking) {
              setPlaceState("answer");
              bumpFocus();
            }
          }, 0);
        }
        if (event.type === "ready" && event.plugin_missing) void api.interrupt().catch(() => {});
      });
      if (!alive) return off();
      setApi(api);
      latest.current.api = api;
      const [fresh, built, list] = await Promise.all([api.setup(), api.about(), api.recent().catch(() => [] as string[])]);
      setAbout(built);
      setSetup(fresh);
      setRecent(list);
      const first = (await api.initialFolder()) ?? list[0] ?? null;
      if (first && setupReady(fresh)) await openFolder(first);
      setBooted(true);
      if (isDemo) (await import("./demo")).startFromAddress(actions);
      else if (await api.selftest()) {
        try {
          await (await import("./selftest")).run(api, actions);
        } catch (problem) {
          await api.selftestReport({ ok: false, failure: `the check could not start: ${String(problem)}`, errors: [...window.__mgErrors], steps: {} });
        }
      }
    })();
    return () => {
      alive = false;
      off();
    };
    // Runs once: every handler reads the newest values through refs.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // The first screen's real content is on the page: the harness may time it.
  useEffect(() => {
    if (!booted) return;
    requestAnimationFrame(() => harnessReady());
  }, [booted]);

  // Files dropped on the window are added to the folder's material.
  useEffect(() => {
    if (!api) return;
    let stop = () => {};
    let alive = true;
    void api
      .onDrop(
        (paths) => void addMaterial(paths),
        (over) => setDragging(over && latest.current.folder !== null),
      )
      .then((off) => (alive ? (stop = off) : off()));
    return () => {
      alive = false;
      stop();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [api]);

  // Text pasted with nothing to type in is added as material.
  useEffect(() => {
    const onPaste = (event: ClipboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (target?.closest("input, textarea, [contenteditable]") || !latest.current.folder) return;
      const text = event.clipboardData?.getData("text/plain") ?? "";
      if (text.trim() === "") return;
      event.preventDefault();
      void latest.current.api
        ?.addPastedText(text)
        .then((added) => {
          setFlash(added.lines);
          void refreshFiles();
        })
        .catch((problem) => setFlash([String(problem)]));
    };
    window.addEventListener("paste", onPaste);
    return () => window.removeEventListener("paste", onPaste);
  }, [refreshFiles]);

  // A running paper's clock.
  useEffect(() => {
    if (!home?.sitting) return;
    const timer = window.setInterval(() => setNow(Date.now()), 15_000);
    return () => window.clearInterval(timer);
  }, [home?.sitting]);

  // ---------------------------------------------------------------------------
  // Keys

  const panesShown = width >= SPLIT_MIN ? layout.panes : [{ tabs: [...new Set(layout.panes.flatMap((p) => p.tabs))], active: layout.panes[layout.focus]?.active ?? layout.panes[0].active }];
  const focusIndex = Math.min(layout.focus, panesShown.length - 1);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;
      const command = event.metaKey || event.ctrlKey;
      const key = event.key.toLowerCase();
      const { layout, session, folder, width } = latest.current;
      const shownPanes = width >= SPLIT_MIN ? layout.panes : [{ tabs: [...new Set(layout.panes.flatMap((p) => p.tabs))], active: layout.panes[layout.focus]?.active ?? "lesson" }];
      const pane = shownPanes[Math.min(layout.focus, shownPanes.length - 1)];
      const run = (work: () => void) => {
        event.preventDefault();
        work();
      };
      if (command && !event.shiftKey && key === "p") run(() => folder && setSheet("open"));
      else if ((command && !event.shiftKey && key === "k") || (command && event.shiftKey && key === "p")) run(() => setSheet("commands"));
      else if (command && key === "o") run(() => setSheet("folders"));
      else if (command && key === ",") run(() => openDoc("settings"));
      else if (command && event.shiftKey && key === "m") run(() => setSheet("symbols"));
      else if (command && event.shiftKey && key === "a") run(() => window.dispatchEvent(new Event("mg:photos")));
      else if (command && event.shiftKey && (key === "h" || key === "l" || key === "d")) run(() => void help(key === "h" ? "hint" : key === "l" ? "lost" : "unknown"));
      else if (command && key === "n" && folder) run(() => freshLesson());
      else if (command && key === "w") run(() => closeDoc(pane.active, Math.min(layout.focus, layout.panes.length - 1)));
      else if (command && key === "." && session.live && session.live.status !== "idle") run(() => void latest.current.api?.interrupt());
      else if (command && key === "\\") run(() => setLayout(toggleSplit(layout, pane.active === "plan" ? "lesson" : "plan")));
      else if (command && key === "b" && folder) run(() => setLayout({ ...layout, sidebar: !layout.sidebar }));
      else if (command && key === "l" && folder) run(() => showLesson());
      else if (command && event.altKey && (event.key === "ArrowLeft" || event.key === "ArrowRight") && layout.panes.length > 1)
        run(() => setLayout({ ...layout, focus: event.key === "ArrowLeft" ? 0 : 1 }));
      else if (command && (key === "=" || key === "+")) run(() => setPrefs({ ...latest.current.prefs, size: Math.min(SIZE_MAX, latest.current.prefs.size + 1) }));
      else if (command && key === "-") run(() => setPrefs({ ...latest.current.prefs, size: Math.max(SIZE_MIN, latest.current.prefs.size - 1) }));
      else if (command && key === "0") run(() => setPrefs({ ...latest.current.prefs, size: 17 }));
      else if (command && /^[1-9]$/.test(key)) {
        const tab = pane.tabs[Number(key) - 1];
        if (tab) run(() => openDoc(tab));
      } else if (event.ctrlKey && event.key === "Tab") {
        const at = pane.tabs.indexOf(pane.active);
        const tab = pane.tabs[(at + (event.shiftKey ? -1 : 1) + pane.tabs.length) % pane.tabs.length];
        run(() => openDoc(tab));
      } else if (event.key === "Escape" && latest.current.fresh && latest.current.session.earlier.length + (latest.current.session.live ? 1 : 0) > 0) {
        run(() => setFresh(false));
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [openDoc, setLayout, setPrefs]);

  const freshLesson = () => {
    setFresh(true);
    showLesson();
  };

  // What the demo and the self-check may drive.
  const actions: Actions = {
    continueLesson,
    newLesson,
    answer: (text, confidence) => answer(text, confidence ?? null, []),
    talk,
    help,
    stop,
    openDoc,
    setSheet,
    setPlace,
    setLayout,
    sitPaper,
    addMaterial,
    setDragging,
    setPrefs,
    freshLesson,
    session: () => latest.current.session,
  };

  // ---------------------------------------------------------------------------
  // What the sheets list

  const lessonTitle = plainText(title ?? plan?.goal ?? home?.last?.goal ?? "") || "Lesson";
  const hasLesson = session.earlier.length > 0 || session.live !== null;
  const showFresh = fresh || (!hasLesson && !session.open);
  const papers = home?.papers ?? [];

  const docTitle = (doc: DocId): string => {
    if (doc === "lesson") return showFresh ? "New lesson" : lessonTitle;
    if (doc.startsWith("lesson:")) {
      const info = lessons.find((l) => l.session === doc.slice(7));
      return info ? plainText(info.opening) || new Date(info.first_ms).toDateString() : "Lesson";
    }
    if (doc === "plan") return plan?.title ? `${plan.title} plan` : "Plan";
    if (doc === "record") return "Record";
    if (doc === "settings") return "Settings";
    if (doc === "setup") return "This Mac";
    if (doc.startsWith("file:")) return fileName(doc.slice(5)).replace(/\.[a-z0-9]+$/i, "");
    return doc;
  };

  const openItems = (): SheetItem[] => {
    const items: SheetItem[] = [];
    items.push({ id: "lesson", label: showFresh ? "New lesson" : lessonTitle, detail: session.open ? "open" : "", group: "Lessons", run: () => openDoc("lesson"), beside: () => openDoc("lesson", true) });
    for (const lesson of lessons.filter((l) => !l.current)) {
      const doc = `lesson:${lesson.session}`;
      items.push({ id: doc, label: plainText(lesson.opening) || "Lesson", detail: new Date(lesson.last_ms).toDateString().slice(4), group: "Lessons", run: () => openDoc(doc), beside: () => openDoc(doc, true) });
    }
    if (plan?.path) items.push({ id: "plan", label: docTitle("plan"), detail: plan.path, group: "Plan", run: () => openDoc("plan"), beside: () => openDoc("plan", true) });
    for (const file of files?.files ?? []) {
      if (file.path === plan?.path) continue;
      const kind = fileKind(file.path);
      const group = kind === "material" ? "Material" : kind === "paper" ? "Papers" : kind === "handed-in" ? "Handed in" : kind === "plan" ? "Plan" : "Other files";
      const doc = `file:${file.path}`;
      items.push({ id: doc, label: fileName(file.path), detail: file.path.replace(/^Study\//, "").replace(/[^/]+$/, ""), group, run: () => openDoc(doc), beside: () => openDoc(doc, true) });
    }
    items.push({ id: "record", label: "Record", detail: "every marked answer", group: "Yours", run: () => openDoc("record"), beside: () => openDoc("record", true) });
    items.push({ id: "settings", label: "Settings", group: "Yours", keys: "⌘,", run: () => openDoc("settings") });
    const order = ["Lessons", "Plan", "Material", "Papers", "Handed in", "Other files", "Yours"];
    return items.sort((a, b) => order.indexOf(a.group ?? "") - order.indexOf(b.group ?? ""));
  };

  const commandItems = (): SheetItem[] => {
    const list: SheetItem[] = [];
    const add = (label: string, run: () => void, keys?: string, words?: string) => list.push({ id: label, label, run, keys, words });
    const running = session.open;
    const busy = session.live !== null && session.live.status !== "idle";
    if (folder) {
      if (!running && hasLesson) add("Continue the lesson", () => void continueLesson(), "Return", "start resume");
      add("New lesson", freshLesson, "⌘N", "start");
      if (running) add("Close the lesson", () => void stop(), "⌘W", "stop end finish");
      if (busy) add("Stop the reply", () => void api?.interrupt(), "⌘.");
      if (page.open !== null && running) {
        add("Hint", () => void help("hint"), "⌘⇧H");
        add("I'm lost", () => void help("lost"), "⌘⇧L");
        add("I don't know", () => void help("unknown"), "⌘⇧D");
        add("Photos of my working", () => window.dispatchEvent(new Event("mg:photos")), "⌘⇧A", "attach image pdf");
      }
      add("Maths symbols", () => setSheet("symbols"), "⌘⇧M", "insert");
      add("Open the plan", () => openDoc("plan"), undefined, "plan");
      add("Edit the plan as text", () => openDoc("plan"), "⌘E", "edit");
      add("Open the record", () => openDoc("record"), undefined, "progress marks proofs");
      add("Open a document", () => setSheet("open"), "⌘P");
      add("Add course material", () => void pickMaterial(), undefined, "files slides notes import");
      for (const paper of papers) {
        if (!home?.sitting) add(`Sit ${paper.name}`, () => void sitPaper(paper), undefined, "paper timed");
      }
      if (home?.sitting) {
        add(`Hand in ${home.sitting.name}`, () => void handIn(), undefined, "paper done finish");
        add(`End ${home.sitting.name} without handing in`, () => void endPaper(), undefined, "paper stop");
      }
      add(layout.sidebar ? "Hide the folder's files" : "Show the folder's files", () => setLayout({ ...layout, sidebar: !layout.sidebar }), "⌘B", "sidebar");
      add(layout.panes.length > 1 ? "One pane" : "Two panes", () => setLayout(toggleSplit(layout, "plan")), "⌘\\", "split");
      if (folder) add("Take this folder off the list", () => void api?.forgetFolder(folder).then(() => api.recent().then(setRecent)), undefined, "remove forget");
    }
    add("Another study folder", () => setSheet("folders"), "⌘O", "library switch");
    add("Open a folder", () => void pickFolder(), undefined, "choose");
    add("New study folder", () => void newFolder());
    add("Find study folders on this Mac", () => void findFolders());
    if (engine && !running) {
      add(engine.chosen === "plugin" ? "Run lessons on Magnesium" : "Run lessons on Claude Code", () => void chooseEngine(engine.chosen === "plugin" ? "direct" : "plugin"), undefined, "engine");
    }
    if (setup) add(setup.payment === "plan" ? "Pay for lessons with an API key" : "Pay for lessons with your Claude plan", () => void choosePayment(setup.payment === "plan" ? "api_key" : "plan"), undefined, "payment");
    if (setup && !setup.account.signed_in) add("Sign in to Claude Code", () => void signIn());
    add("This Mac", () => openDoc("setup"), undefined, "setup program version");
    add("Settings", () => openDoc("settings"), "⌘,", "font theme scheme size keys");
    add("Larger text", () => setPrefs({ ...prefs, size: Math.min(SIZE_MAX, prefs.size + 1) }), "⌘=");
    add("Smaller text", () => setPrefs({ ...prefs, size: Math.max(SIZE_MIN, prefs.size - 1) }), "⌘−");
    for (const scheme of ["light", "dark", "system"] as const) {
      if (prefs.scheme !== scheme) add(scheme === "system" ? "Scheme: same as macOS" : `Scheme: ${scheme}`, () => setPrefs({ ...prefs, scheme }), undefined, "theme colour");
    }
    return list;
  };

  const folderItems = (): SheetItem[] => [
    ...recent.map((path) => ({
      id: path,
      label: folderName(path),
      detail: path.replace(/^\/Users\/[^/]+/, "~"),
      group: "Study folders",
      run: () => void openFolder(path),
      remove: () => void api?.forgetFolder(path).then(() => api.recent().then(setRecent)),
    })),
    { id: "open", label: "Open a folder…", group: "More", run: () => void pickFolder() },
    { id: "new", label: "New study folder…", group: "More", run: () => void newFolder() },
    { id: "find", label: "Find study folders on this Mac", group: "More", run: () => void findFolders() },
  ];

  const symbolItems = (): SheetItem[] =>
    SYMBOLS.map(([symbol, name]) => ({
      id: symbol,
      label: symbol.startsWith("$") ? symbol.replace(/\{\}/g, "{\\square}") : symbol,
      math: symbol.startsWith("$"),
      detail: name,
      words: name,
      run: () => {
        const writer = writers.current[latest.current.place] ?? writers.current.talk;
        window.setTimeout(() => writer?.insert(symbol), 0);
      },
    }));

  // ---------------------------------------------------------------------------
  // Drawing

  const registerWriter = useCallback((which: Place, handle: WriterHandle | null) => {
    writers.current[which] = handle;
  }, []);

  const onDraft = useCallback((which: string, text: string) => keepQuietly((layout) => ({ ...layout, drafts: { ...layout.drafts, [which]: text } })), [keepQuietly]);

  const suggestions = (plan?.items ?? []).filter((item) => item.status !== "done").map((item) => plainText(item.title));

  const renderDoc = (doc: DocId, paneIndex: number, active: boolean) => {
    if (doc === "setup" || (!folder && doc === "lesson")) {
      return (
        <SetupDoc
          setup={setup}
          about={about}
          noFolder={!folder}
          signingIn={signingIn}
          error={error}
          onReplace={() => void answerRuntime((api) => api.replaceRuntime())}
          onKeep={() => void answerRuntime((api) => api.keepRuntime())}
          onCheck={() => void checkAgain()}
          onSignIn={() => void signIn()}
          onPayment={(p) => void choosePayment(p)}
          onLink={(url) => void api?.openUrl(url)}
          onOpenFolder={() => setSheet("folders")}
          onNewFolder={() => void newFolder()}
          onFindFolders={() => void findFolders()}
          finding={finding}
          findNote={findNote}
        />
      );
    }
    if (doc === "lesson") {
      return (
        <>
          {home && home.state === "needs_upgrade" && (
            <div className="folder-notice" data-role="upgrade">
              <p>{home.message}</p>
              <button className="primary" onClick={() => void upgrade()}>
                Upgrade this folder
              </button>
            </div>
          )}
          {home && (home.state === "no_runtime" || home.state === "unreadable") && (
            <div className="folder-notice">
              <p>{home.message}</p>
              <button onClick={() => openDoc("setup")}>This Mac</button>
            </div>
          )}
          <LessonDoc
            page={page}
            session={session}
            title={showFresh ? "New lesson" : lessonTitle}
            running={session.open}
            fresh={showFresh}
            suggestions={suggestions}
            place={place}
            setPlace={setPlace}
            focusTick={active && paneIndex === focusIndex ? focusTick : -1}
            drafts={latest.current.layout.drafts}
            onDraft={onDraft}
            onAnswer={(text, confidence, pics) => answer(text, confidence, pics)}
            onTalk={talk}
            onHelp={(kind) => void help(kind)}
            onContinue={() => void continueLesson()}
            onNewLesson={(goal) => void newLesson(goal)}
            onSource={onSource}
            onDecide={(id: string, decision: Decision) => void api?.decide(id, decision)}
            onCardAnswer={(id: string, answers: Answer[], shown: string) => {
              dispatch({ type: "picked", shown, now: Date.now() });
              void api?.answer(id, answers);
            }}
            onDismiss={(text) => dispatch({ type: "dismiss", text })}
            onPhotos={photos}
            onNote={note}
            registerWriter={registerWriter}
            scroller={() => scrollers.current[paneIndex] ?? null}
          />
        </>
      );
    }
    if (doc.startsWith("lesson:")) {
      const turns = earlier[doc.slice(7)] ?? [];
      const old: SessionState = { ...emptySession, earlier: turns };
      const oldPage = readPage(old, (turn) => kept.current[exKey(turn.learner, turn.tutor)] ?? null);
      return (
        <LessonDoc
          page={oldPage}
          session={old}
          title={docTitle(doc)}
          running={false}
          readOnly
          fresh={false}
          suggestions={[]}
          place="talk"
          setPlace={() => {}}
          focusTick={-1}
          drafts={{}}
          onDraft={() => {}}
          onAnswer={async () => false}
          onTalk={async () => false}
          onHelp={() => {}}
          onContinue={() => {}}
          onNewLesson={() => {}}
          onSource={onSource}
          onDecide={() => {}}
          onCardAnswer={() => {}}
          onDismiss={() => {}}
          onPhotos={async () => []}
          onNote={note}
          registerWriter={() => {}}
          scroller={() => scrollers.current[paneIndex] ?? null}
        />
      );
    }
    if (doc === "plan") {
      return (
        <PlanDoc
          plan={plan}
          before={planBefore}
          changedAt={planChangedAt}
          active={active && paneIndex === focusIndex}
          folds={layout.folds}
          onFold={(heading, open) => setLayout({ ...layout, folds: { ...layout.folds, [heading]: open } })}
          onSeen={() => {
            setPlanBefore(null);
            try {
              if (folder) localStorage.removeItem(`mg.planBase:${folder}`);
            } catch {
              // Nothing kept.
            }
          }}
          onSave={async (text) => {
            if (!api || !plan?.path || plan.changed_ms === null) return "This plan cannot be saved from here.";
            try {
              await api.writePlan(plan.path, text, plan.changed_ms);
              await refreshPlan(false);
              setPlanBefore(null);
              return null;
            } catch (problem) {
              return String(problem).replace(/^Error: /, "");
            }
          }}
          onTalk={() => {
            showLesson();
            setPlace("talk");
          }}
        />
      );
    }
    if (doc === "record") return <RecordDoc marks={recordMarks} progress={progress} />;
    if (doc === "settings") {
      return (
        <SettingsDoc
          prefs={prefs}
          setPrefs={setPrefs}
          engine={engine}
          lessonOpen={session.open}
          onEngine={(e) => void chooseEngine(e)}
          setup={setup}
          onPayment={(p) => void choosePayment(p)}
          onSignIn={() => void signIn()}
          blockMinutes={home?.block_minutes ?? null}
          onBlock={(minutes) => {
            void api?.setBlockMinutes(minutes);
            setHome((old) => (old ? { ...old, block_minutes: minutes } : old));
          }}
          about={about}
        />
      );
    }
    if (doc.startsWith("file:")) {
      const path = doc.slice(5);
      const paper = papers.find((p) => p.path === path) ?? null;
      return (
        <FileDoc
          path={path}
          page={layout.pages[doc] ?? 1}
          onPage={(n) => setLayout({ ...latest.current.layout, pages: { ...latest.current.layout.pages, [doc]: n } })}
          active={active && paneIndex === focusIndex}
          paper={paper}
          sitting={home?.sitting ?? null}
          onTeach={(name) => void newLesson(`Teach me from ${name}`)}
          onSit={(p) => void sitPaper(p)}
          onOpenOutside={(p) => void api?.openFile(p).catch((problem) => note(String(problem)))}
          onNote={note}
        />
      );
    }
    return null;
  };

  const sitting = home?.sitting ?? null;
  const left = sitting ? Math.max(0, Math.ceil((sitting.started_ms + sitting.minutes * 60_000 - now) / 60_000)) : 0;
  const live = session.live;
  const tutorState = live && live.status !== "idle" ? (live.doing ? live.doing : live.status === "writing" ? "The tutor is writing" : "The tutor is thinking") : null;
  const engineName = engine ? ((session.open ? (engine.conversation ?? engine.chosen) : engine.chosen) === "direct" ? "Magnesium" : "Claude Code") : null;
  const noFolder = !folder;
  const shownPanes = noFolder ? [{ tabs: ["setup"], active: "setup" }] : panesShown;

  return (
    <div className={`window${layout.sidebar && !noFolder ? " with-sidebar" : ""}`} data-split={shownPanes.length > 1 || undefined}>
      {layout.sidebar && !noFolder && (
        <nav className="sidebar" aria-label="The folder's files">
          <div className="sidebar-head" data-tauri-drag-region />
          <button className="sidebar-folder" onClick={() => setSheet("folders")}>
            {folderName(folder ?? "")}
          </button>
          {(["Lessons", "Plan", "Material", "Papers", "Handed in", "Other files", "Yours"] as const).map((group) => {
            const items = openItems().filter((item) => item.group === group);
            if (items.length === 0) return null;
            return (
              <div key={group} className="sidebar-group">
                <p className="sidebar-label">{group}</p>
                {items.map((item) => (
                  <button key={item.id} className={shownPanes[focusIndex]?.active === item.id ? "sidebar-item here" : "sidebar-item"} onClick={() => item.run()}>
                    {item.label}
                  </button>
                ))}
              </div>
            );
          })}
        </nav>
      )}
      <main className="panes">
        {shownPanes.map((pane, index) => (
          <section key={index} className={`pane${index === focusIndex ? " focused" : ""}`} onMouseDown={() => index !== layout.focus && !noFolder && setLayout({ ...layout, focus: index })}>
            <header className={`tabs${index === 0 && !(layout.sidebar && !noFolder) ? " under-lights" : ""}`} data-tauri-drag-region>
              {pane.tabs.map((doc, n) => (
                <button
                  key={doc}
                  role="tab"
                  aria-selected={doc === pane.active}
                  className={doc === pane.active ? "tab here" : "tab"}
                  onClick={() => (noFolder ? undefined : openDoc(doc))}
                  title={n < 9 ? `⌘${n + 1}` : undefined}
                >
                  {doc === "lesson" && session.live && session.live.status !== "idle" && <span className="live-dot" aria-label="The tutor is writing" />}
                  <span className="tab-name">{noFolder ? (setupReady(setup) ? "Welcome" : "This Mac") : docTitle(doc)}</span>
                </button>
              ))}
            </header>
            <DocScroller
              docId={pane.active}
              pane={index}
              stick={pane.active === "lesson"}
              initial={layout.scroll[pane.active]}
              register={(el) => (scrollers.current[index] = el)}
              onScroll={(top) => keepQuietly((l) => ({ ...l, scroll: { ...l.scroll, [pane.active]: top } }))}
            >
              {renderDoc(pane.active, index, true)}
            </DocScroller>
          </section>
        ))}
      </main>
      <footer className="status" role="status">
        {folder ? (
          <button className="status-item folder" onClick={() => setSheet("folders")} title="Another study folder (⌘O)">
            {folderName(folder)}
          </button>
        ) : (
          <span className="status-item">No study folder open</span>
        )}
        {sitting && (
          <span className="status-item paper" data-role="paper-clock">
            {sitting.name}: {left} {left === 1 ? "minute" : "minutes"} left
          </span>
        )}
        <span className="status-spacer" />
        {tutorState && <span className="status-item tutor">{tutorState}</span>}
        {engineName && folder && <span className="status-item">{engineName}</span>}
        <button className="status-item keys" onClick={() => setSheet("commands")} title="Every command">
          ⌘K
        </button>
      </footer>
      {flash && (
        <div className="flash" role="status">
          {flash.map((line) => (
            <p key={line}>{line}</p>
          ))}
          <button className="dismiss" aria-label="Dismiss" onClick={() => setFlash(null)}>
            ×
          </button>
        </div>
      )}
      {dragging && (
        <div className="drop-veil" aria-hidden="true">
          <p>Drop to add to {folder ? folderName(folder) : "this folder"}'s material</p>
        </div>
      )}
      {sheet === "open" && <Sheet label="Open a document" placeholder="Open a document" items={openItems()} onClose={() => setSheet(null)} />}
      {sheet === "commands" && <Sheet label="Commands" placeholder="Run a command" items={commandItems()} onClose={() => setSheet(null)} />}
      {sheet === "folders" && <Sheet label="Study folders" placeholder="Open a study folder" items={folderItems()} onClose={() => setSheet(null)} />}
      {sheet === "symbols" && <Sheet label="Maths symbols" placeholder="Insert a symbol" items={symbolItems()} onClose={() => setSheet(null)} />}
    </div>
  );
}

/** The scrolling part of a pane. It gives a document back at the place it was left, and keeps a lesson at its end while it grows. */
function DocScroller(props: {
  docId: DocId;
  pane: number;
  stick: boolean;
  initial: number | undefined;
  register: (el: HTMLElement | null) => void;
  onScroll: (top: number) => void;
  children: React.ReactNode;
}) {
  const el = useRef<HTMLDivElement>(null);
  const inner = useRef<HTMLDivElement>(null);
  const atEnd = useRef(true);
  const timer = useRef<number | null>(null);
  const latest = useRef(props);
  latest.current = props;

  useLayoutEffect(() => {
    props.register(el.current);
    const box = el.current;
    if (!box) return;
    const initial = latest.current.initial;
    if (initial === undefined) {
      box.scrollTop = latest.current.stick ? box.scrollHeight : 0;
      atEnd.current = true;
    } else {
      box.scrollTop = initial;
      atEnd.current = box.scrollTop + box.clientHeight >= box.scrollHeight - 24;
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.docId]);

  useEffect(() => {
    const box = el.current;
    const content = inner.current;
    if (!box || !content) return;
    const watch = new ResizeObserver(() => {
      if (latest.current.stick && atEnd.current) box.scrollTop = box.scrollHeight;
    });
    watch.observe(content);
    return () => watch.disconnect();
  }, []);

  return (
    <div
      className="doc-scroll"
      ref={el}
      data-pane={props.pane}
      onScroll={(event) => {
        const box = event.currentTarget;
        atEnd.current = box.scrollTop + box.clientHeight >= box.scrollHeight - 24;
        if (timer.current !== null) window.clearTimeout(timer.current);
        timer.current = window.setTimeout(() => latest.current.onScroll(box.scrollTop), 200);
      }}
    >
      <div className="column" ref={inner}>
        {props.children}
      </div>
    </div>
  );
}
