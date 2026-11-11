// The window: which screen is up, and the few things that move between them.
//
// The way through it is a line: the library of courses, one course, the
// lesson, and the end of the sitting. Progress, a timed paper and this Mac's
// setup hang off that line, and each goes back to where it was opened from.

import { useCallback, useEffect, useReducer, useRef, useState } from "react";

import { core, isDemo } from "./bridge";
import { busy, emptySession, openingWords, reduce } from "./lesson";
import { Course } from "./screens/Course";
import { Library, displayName } from "./screens/Library";
import { Palette, type Command } from "./screens/Palette";
import { Paper } from "./screens/Paper";
import { Progress } from "./screens/Progress";
import { Session } from "./screens/Session";
import { Setup, aboutText, setupReady } from "./screens/Setup";
import { Summary, type Sitting } from "./screens/Summary";
import { DropVeil, PasteSheet } from "./ui/Material";
import type {
  About,
  Activity,
  AgentEvent,
  Answer,
  Card,
  Core,
  Decision,
  Home,
  Paper as PaperInfo,
  Payment,
  Progress as ProgressData,
  Rail,
  Row,
  RuntimeSetup,
  Setup as SetupData,
} from "./types";

type View = "loading" | "setup" | "library" | "course" | "session" | "summary" | "progress" | "paper";

const BLOCKS = [25, 50, 90];
/** How often the setup screen asks Claude Code whether a sign-in finished, and for how long. */
const SIGN_IN_POLL_MS = 2000;
const SIGN_IN_LIMIT_MS = 15 * 60_000;

const stepsDone = (rail: Rail | null) => rail?.steps.filter((step) => step.status === "done").length ?? 0;

const proofsOf = (row: Row) => row.dots.filter((dot) => dot === 2).length;

/** How many proofs each skill holds, by the skill's name. */
function tally(progress: ProgressData): Map<string, number> {
  const counts = new Map<string, number>();
  [...progress.confirmed, ...progress.in_progress].forEach((row) => counts.set(row.name, proofsOf(row)));
  return counts;
}

/** The day after a day written as "2026-11-10". */
function dayAfter(day: string): string {
  const date = new Date(`${day}T12:00:00`);
  date.setDate(date.getDate() + 1);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

export function App() {
  const [api, setApi] = useState<Core | null>(null);
  const [view, setView] = useState<View>("loading");
  // Where Progress and Setup go back to.
  const [from, setFrom] = useState<View>("library");
  const [folders, setFolders] = useState<string[]>([]);
  const [cards, setCards] = useState<Card[] | null>(null);
  const [activity, setActivity] = useState<Activity | null>(null);
  const [home, setHome] = useState<Home | null>(null);
  const [session, dispatch] = useReducer(reduce, emptySession);
  const [rail, setRail] = useState<Rail | null>(null);
  const [progress, setProgress] = useState<ProgressData | null>(null);
  const [summary, setSummary] = useState<Sitting | null>(null);
  const [palette, setPalette] = useState(false);
  const [asking, setAsking] = useState(false);
  const [starting, setStarting] = useState(false);
  const [finding, setFinding] = useState(false);
  const [chosenPaper, setChosenPaper] = useState<PaperInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [setup, setSetup] = useState<SetupData | null>(null);
  const [about, setAbout] = useState<About | null>(null);
  const [signingIn, setSigningIn] = useState(false);
  // What Claude Code said before the sign-in, to see when its answer changes.
  const accountBefore = useRef("");
  const [upgrading, setUpgrading] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  // Adding course material: what it said, whether it is under way, whether
  // files are being dragged over the window, and the text being pasted.
  const [material, setMaterial] = useState<string[] | null>(null);
  const [adding, setAdding] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [pasting, setPasting] = useState<string | null>(null);
  // Until the first screen is up, a finished setup opens it.
  const waitingForSetup = useRef(true);
  // What this sitting has counted so far, for the screen that ends it.
  const sitting = useRef({ answers: 0, alreadyStudied: false });
  // The proofs each skill held when the sitting began, or null when they
  // could not be read. The end of the sitting shows what was earned since.
  const proofsBefore = useRef<Map<string, number> | null>(null);
  const opening = useRef(false);

  // The newest values, for handlers that outlive a render.
  const now = useRef({ api, view, from, home, session, palette, setup, rail, activity });
  now.current = { api, view, from, home, session, palette, setup, rail, activity };
  // The newest copies of two handlers, for the listeners that outlive a render.
  const addMaterialRef = useRef<(paths: string[]) => Promise<void>>(async () => {});
  const canAddRef = useRef<() => boolean>(() => false);

  const refreshRail = useCallback(async () => {
    const { api } = now.current;
    if (!api) return;
    try {
      setRail(await api.rail());
    } catch {
      // The steps are a convenience. A lesson goes on without them.
    }
  }, []);

  const refreshHome = useCallback(async (): Promise<Home | null> => {
    const { api } = now.current;
    if (!api) return null;
    try {
      const fresh = await api.home();
      setHome(fresh);
      return fresh;
    } catch (problem) {
      setError(String(problem));
      return null;
    }
  }, []);

  const refreshActivity = useCallback(async () => {
    try {
      const fresh = await now.current.api?.activity();
      if (fresh) setActivity(fresh);
    } catch {
      // The run is a convenience too.
    }
  }, []);

  /** Read the proofs for the open course. Quiet on the course page, which only draws a summary of them. */
  const loadProgress = useCallback(async (target: string | null, loud: boolean) => {
    const { api } = now.current;
    if (!api) return;
    try {
      setProgress(await api.progress(target));
    } catch (problem) {
      if (loud) setError(String(problem));
    }
  }, []);

  const loadLibrary = useCallback(async () => {
    const { api } = now.current;
    if (!api) return;
    try {
      const [list, fresh] = await Promise.all([api.recent(), api.library()]);
      setFolders(list);
      setCards(fresh);
    } catch (problem) {
      setError(String(problem));
      setCards((old) => old ?? []);
    }
    void refreshActivity();
  }, [refreshActivity]);

  const showLibrary = useCallback(() => {
    setError(null);
    setNotice(null);
    setMaterial(null);
    setAsking(false);
    setView("library");
    void loadLibrary();
  }, [loadLibrary]);

  const showFolder = useCallback(
    async (api: Core, open: () => Promise<Home>) => {
      if (opening.current) return;
      opening.current = true;
      setError(null);
      setNotice(null);
      setMaterial(null);
      try {
        const opened = await open();
        waitingForSetup.current = false;
        setHome(opened);
        dispatch({ type: "reset" });
        setRail(null);
        setProgress(null);
        setSummary(null);
        setAsking(false);
        setView(opened.sitting ? "paper" : "course");
        void loadProgress(null, false);
        void refreshActivity();
        setFolders(await api.recent());
      } catch (problem) {
        setError(String(problem));
        setView("library");
        void loadLibrary();
      } finally {
        opening.current = false;
      }
    },
    [loadLibrary, loadProgress, refreshActivity],
  );

  const openFolder = useCallback((api: Core, path: string) => showFolder(api, () => api.openFolder(path)), [showFolder]);

  /** Take in what the setup check found. The first time it is ready, go to the first screen. */
  const takeSetup = useCallback(
    async (api: Core, fresh: SetupData) => {
      setSetup(fresh);
      if (!setupReady(fresh)) {
        if (now.current.view === "loading") setView("setup");
        return;
      }
      if (!waitingForSetup.current) return;
      waitingForSetup.current = false;
      const first = await api.initialFolder();
      if (first) await openFolder(api, first);
      else showLibrary();
    },
    [openFolder, showLibrary],
  );

  useEffect(() => {
    let alive = true;
    let unlisten = () => {};
    void (async () => {
      const api = await core();
      const off = await api.onEvent((event: AgentEvent) => {
        dispatch({ type: "event", event });
        if (event.type === "turn_ended") void refreshRail();
        // Without the plugin this would be a plain chat with no teaching
        // rules, so the reply is stopped and the notice says what to run.
        if (event.type === "ready" && event.plugin_missing) void api.interrupt().catch(() => {});
      });
      if (!alive) {
        off();
        return;
      }
      unlisten = off;
      setApi(api);
      now.current.api = api;
      setFolders(await api.recent());
      // The program is installed, or a question about it answered, before
      // any folder is read: a newer program upgrades a folder's record.
      const [fresh, built] = await Promise.all([api.setup(), api.about()]);
      setAbout(built);
      await takeSetup(api, fresh);
      if (isDemo) (await import("./demo")).startFromAddress(actions);
      else if (await api.selftest()) void (await import("./selftest")).run(api, actions, () => now.current.session);
    })();
    return () => {
      alive = false;
      unlisten();
    };
    // Runs once: every handler it uses reads the newest values through refs.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // While Claude Code's sign-in waits for the browser, look every few
  // seconds whether it finished.
  useEffect(() => {
    if (!signingIn) return;
    const started = Date.now();
    const timer = window.setInterval(() => {
      const { api, setup } = now.current;
      if (!api || !setup) return;
      if (Date.now() - started > SIGN_IN_LIMIT_MS) {
        setSigningIn(false);
        return;
      }
      void api.account().then((account) => {
        // Done when it is signed in as chosen, or says anything new, such as
        // a sign-in to another kind of account.
        if (account.signed_in && (!account.problem || JSON.stringify(account) !== accountBefore.current)) setSigningIn(false);
        void takeSetup(api, { ...(now.current.setup ?? setup), account });
      });
    }, SIGN_IN_POLL_MS);
    return () => window.clearInterval(timer);
  }, [signingIn, takeSetup]);

  // Files dropped on the window are added to the open course, on its page and
  // in a lesson. Anywhere else a drop does nothing.
  useEffect(() => {
    if (!api) return;
    let off = () => {};
    let alive = true;
    void api
      .onDrop(
        (paths) => void addMaterialRef.current(paths),
        (over) => setDragging(over && canAddRef.current()),
      )
      .then((stop) => (alive ? (off = stop) : stop()));
    return () => {
      alive = false;
      off();
    };
  }, [api]);

  // Text pasted on the course page, with nothing selected to type in, is
  // offered as course material. Pasting in the answer box is an answer.
  useEffect(() => {
    const onPaste = (event: ClipboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (target?.closest("input, textarea, [contenteditable]")) return;
      if (now.current.view !== "course" || !canAddRef.current()) return;
      if ((event.clipboardData?.files.length ?? 0) > 0) return;
      const text = event.clipboardData?.getData("text/plain") ?? "";
      if (text.trim() === "") return;
      event.preventDefault();
      setPasting(text);
    };
    window.addEventListener("paste", onPaste);
    return () => window.removeEventListener("paste", onPaste);
  }, []);

  const checkAgain = async () => {
    const { api } = now.current;
    if (!api) return;
    setError(null);
    await takeSetup(api, await api.setup());
  };

  const answerRuntime = async (work: (api: Core) => Promise<RuntimeSetup>) => {
    const { api, setup } = now.current;
    if (!api || !setup) return;
    setError(null);
    const runtime = await work(api);
    await takeSetup(api, { ...setup, runtime });
  };

  const signIn = async () => {
    const { api } = now.current;
    if (!api) return;
    setError(null);
    try {
      accountBefore.current = JSON.stringify(now.current.setup?.account ?? null);
      await api.signIn();
      setSigningIn(true);
    } catch (problem) {
      setError(String(problem));
    }
  };

  const choosePayment = async (payment: Payment) => {
    const { api, setup } = now.current;
    if (!api || !setup) return;
    setSetup({ ...setup, payment });
    const account = await api.setPayment(payment);
    await takeSetup(api, { ...setup, payment, account });
  };

  /** Whether course material can be added now: a course is open, on its page or in a lesson. */
  const canAdd = () => {
    const { home, view } = now.current;
    return Boolean(home) && (view === "course" || view === "session") && ["ready", "empty", "not_set_up"].includes(home?.state ?? "") && !home?.sitting;
  };

  const addMaterial = async (paths: string[]) => {
    const { api } = now.current;
    if (!api || paths.length === 0 || adding || !canAdd()) return;
    setAdding(true);
    setMaterial(null);
    try {
      setMaterial((await api.addMaterial(paths)).lines);
    } catch (problem) {
      setMaterial([String(problem)]);
    } finally {
      setAdding(false);
    }
  };

  addMaterialRef.current = addMaterial;
  canAddRef.current = canAdd;

  const chooseMaterial = async () => {
    const { api } = now.current;
    if (!api || !canAdd()) return;
    try {
      await addMaterial(await api.pickMaterial());
    } catch (problem) {
      setMaterial([String(problem)]);
    }
  };

  const savePasted = async (text: string) => {
    const { api } = now.current;
    if (!api || adding) return;
    setAdding(true);
    setMaterial(null);
    try {
      setMaterial((await api.addPastedText(text)).lines);
      setPasting(null);
    } catch (problem) {
      setPasting(null);
      setMaterial([String(problem)]);
    } finally {
      setAdding(false);
    }
  };

  const openLink = (url: string) => void now.current.api?.openUrl(url).catch((problem) => setError(String(problem)));

  const upgrade = async () => {
    const { api } = now.current;
    if (!api) return;
    setError(null);
    setUpgrading(true);
    try {
      setHome(await api.upgradeFolder());
      setNotice("This folder is upgraded. If its record changed, the copy from before is in .magnesium/backups.");
      void loadProgress(null, false);
    } catch (problem) {
      setError(String(problem));
    } finally {
      setUpgrading(false);
    }
  };

  const pickFolder = async () => {
    const { api } = now.current;
    if (!api) return;
    const path = await api.pickFolder();
    if (path) await openFolder(api, path);
  };

  const newFolder = async () => {
    const { api } = now.current;
    if (!api) return;
    const path = await api.pickNewFolder();
    if (path) await showFolder(api, () => api.newFolder(path));
  };

  const findFolders = async () => {
    const { api } = now.current;
    if (!api || finding) return;
    setError(null);
    setNotice(null);
    setFinding(true);
    try {
      const added = await api.findFolders();
      setNotice(added === 0 ? "No other study folder was found on this Mac." : `${added} study ${added === 1 ? "folder" : "folders"} added.`);
      await loadLibrary();
    } catch (problem) {
      setError(String(problem));
    } finally {
      setFinding(false);
    }
  };

  const forgetFolder = async (path: string) => {
    const { api, home } = now.current;
    if (!api) return;
    try {
      await api.forgetFolder(path);
      if (home?.folder === path) setHome(null);
      await loadLibrary();
    } catch (problem) {
      setError(String(problem));
    }
  };

  /** Open the setup screen, with a fresh look at this Mac. */
  const openSetup = () => {
    const { view } = now.current;
    if (view !== "setup") setFrom(view === "loading" ? "library" : view);
    setView("setup");
    void checkAgain();
  };

  /** Open the lesson screen and send the first words of the sitting. */
  const begin = async (resume: boolean, shown: string, send: (api: Core) => Promise<void>) => {
    const { api } = now.current;
    if (!api) return;
    setError(null);
    setStarting(true);
    try {
      const opened = await api.openSession(resume);
      dispatch({ type: "opened", turns: opened.turns });
      sitting.current = { answers: 0, alreadyStudied: now.current.activity?.studied_today ?? false };
      proofsBefore.current = null;
      void api
        .progress(null)
        .then((before) => {
          proofsBefore.current = tally(before);
        })
        .catch(() => {});
      setView("session");
      dispatch({ type: "sent", shown, now: Date.now() });
      await send(api);
      void refreshRail();
    } catch (problem) {
      if (now.current.view === "session") dispatch({ type: "failed", error: String(problem) });
      else setError(String(problem));
    } finally {
      setStarting(false);
    }
  };

  const continueLesson = (skipChecks: boolean) => {
    const home = now.current.home;
    const words = openingWords(
      { stoppedAt: home?.last?.stopped_at ?? null, studiedBefore: Boolean(home?.last), title: home?.exam?.title ?? "this course" },
      skipChecks,
    );
    // The window wrote these words, so they are not sent as an answer.
    return begin(true, words, (api) => api.sendShown(words, words));
  };

  const newLesson = async (goal: string) => {
    const { api, home } = now.current;
    if (!api) return;
    setError(null);
    try {
      if (home?.state === "not_set_up") setHome(await api.setUp());
    } catch (problem) {
      setError(String(problem));
      return;
    }
    await begin(false, goal, (api) => api.sendNewLesson(goal));
    setAsking(false);
  };

  const say = async (text: string) => {
    const { api, session } = now.current;
    if (!api) return;
    try {
      if (!session.open) {
        await api.openSession(true);
        dispatch({ type: "reopened" });
      }
      sitting.current.answers += 1;
      dispatch({ type: "sent", shown: text, now: Date.now() });
      await api.send(text);
    } catch (problem) {
      dispatch({ type: "failed", error: String(problem) });
    }
  };

  const onNote = useCallback((text: string) => dispatch({ type: "note", notice: { kind: "other", text } }), []);

  const guard = (work: (api: Core) => Promise<unknown>) => async () => {
    const { api } = now.current;
    if (!api) return;
    try {
      await work(api);
    } catch (problem) {
      dispatch({ type: "note", notice: { kind: "other", text: String(problem) } });
    }
  };

  /** End the sitting: save the line, close the lesson, and show what was done. */
  const stop = async (line: string) => {
    const { api, session, rail } = now.current;
    if (!api) return;
    const saved = line.trim() === "" ? null : line.trim();
    const minutes = session.startedMs === null ? 0 : Math.max(1, Math.round((Date.now() - session.startedMs) / 60_000));
    const done: Sitting = {
      minutes,
      answers: sitting.current.answers,
      stepsDone: stepsDone(rail),
      stepsTotal: rail?.steps.length ?? 0,
      stoppedAt: saved,
      alreadyStudied: sitting.current.alreadyStudied,
      earned: [],
      tomorrow: 0,
    };
    try {
      await api.stop(saved);
    } catch (problem) {
      setError(String(problem));
    }
    dispatch({ type: "stopped" });
    setRail(null);
    await Promise.all([refreshHome(), refreshActivity()]);
    try {
      // What the runtime's record holds now, set beside what it held when
      // the sitting began. The window claims no proof the record does not show.
      const after = await api.progress(null);
      setProgress(after);
      const before = proofsBefore.current;
      if (before) {
        done.earned = [...after.confirmed, ...after.in_progress]
          .map((row) => ({ row, proofs: proofsOf(row) - (before.get(row.name) ?? 0) }))
          .filter((each) => each.proofs > 0);
      }
      const today = now.current.activity?.today;
      if (today) {
        const tomorrow = dayAfter(today);
        done.tomorrow = after.in_progress.filter((row) => row.next_day !== null && row.next_day <= tomorrow).length;
      }
    } catch {
      // Without the record the end screen says what it can.
    }
    // A sitting with no answer in it has nothing to sum up.
    if (done.answers > 0) {
      setSummary(done);
      setView("summary");
    } else {
      setView("course");
    }
  };

  const openProgress = async (target: string | null) => {
    const { view } = now.current;
    if (view !== "progress") setFrom(view === "summary" ? "course" : view);
    setError(null);
    setView("progress");
    await loadProgress(target, true);
  };

  const openPaper = (paper: PaperInfo | null) => {
    setChosenPaper(paper);
    setError(null);
    setView("paper");
  };

  const startPaper = async (paper: PaperInfo, minutes: number) => {
    const { api } = now.current;
    if (!api) return;
    try {
      await api.paperStart(paper, minutes);
      await refreshHome();
    } catch (problem) {
      setError(String(problem));
    }
  };

  const handIn = async () => {
    const { api } = now.current;
    if (!api) return;
    setError(null);
    try {
      const files = await api.pickFiles();
      if (files.length === 0) return;
      const words = await api.paperEnd(files);
      await refreshHome();
      if (words) await begin(true, words.shown, (api) => api.sendShown(words.shown, words.sent));
      else setView("course");
    } catch (problem) {
      setError(String(problem));
    }
  };

  const endPaper = async () => {
    const { api } = now.current;
    if (!api) return;
    try {
      await api.paperEnd([]);
    } catch (problem) {
      setError(String(problem));
    }
    await refreshHome();
    setView("course");
  };

  /** Where "back" goes: the paper or the lesson if one is running, else the page this one was opened from. */
  const back = () => {
    const { home, session, from, view } = now.current;
    setError(null);
    if (home?.sitting && view !== "paper") setView("paper");
    else if (session.open || session.live) setView("session");
    else if (!home || from === "library" || view === "course") showLibrary();
    else {
      void refreshHome();
      setView("course");
    }
  };

  const setBlock = async (minutes: number) => {
    await now.current.api?.setBlockMinutes(minutes);
    setHome((old) => (old ? { ...old, block_minutes: minutes } : old));
  };

  const askNewLesson = () => {
    setView("course");
    setAsking(true);
  };

  // What the demo and the self-check may drive.
  const actions = {
    continueLesson,
    newLesson,
    say,
    stop,
    openProgress,
    openPaper,
    startPaper,
    setPalette,
    setAsking,
    openSetup,
    showLibrary,
    setView: (next: string) => setView(next as View),
    addMaterial,
    setDragging,
    pasteText: (text: string) => setPasting(text),
  };

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const command = event.metaKey || event.ctrlKey;
      const { view, session, home, api, setup } = now.current;
      const inLesson = session.open || session.live !== null;
      const key = event.key.toLowerCase();
      if (command && key === "k") {
        event.preventDefault();
        if (view !== "loading" && (view !== "setup" || setupReady(setup))) setPalette((open) => !open);
      } else if (command && event.key === "." && view === "session" && busy(session)) {
        event.preventDefault();
        void api?.interrupt();
      } else if (command && key === "n" && home && !home.sitting && !inLesson && view !== "setup") {
        event.preventDefault();
        askNewLesson();
      } else if (command && key === "o" && !inLesson && (view === "library" || view === "course")) {
        event.preventDefault();
        void pickFolder();
      } else if (event.key === "Escape" && !event.defaultPrevented && !now.current.palette) {
        if (view === "progress" || view === "course") back();
        else if (view === "paper" && !home?.sitting) setView("course");
        else if (view === "summary") setView("course");
        else if (view === "setup" && setupReady(setup)) back();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const commands = (): Command[] => {
    const list: Command[] = [];
    if (home?.sitting) return [{ label: "Back to the paper", hint: home.sitting.name, icon: "paper", run: () => setView("paper") }];
    const inLesson = session.open || session.live !== null;
    if (view !== "session" && inLesson) list.push({ label: "Back to the lesson", icon: "back", run: () => setView("session") });
    if (!inLesson && view !== "library") list.push({ label: "Library", hint: "all courses", icon: "list", run: showLibrary });
    if (home) {
      if (!inLesson && view !== "course") list.push({ label: displayName(home.name), hint: "this course", icon: "back", run: () => setView("course") });
      list.push({ label: "Proofs", hint: "what you have proved", icon: "chart", run: () => void openProgress(null) });
      (progress?.targets ?? [])
        .filter((exam) => exam.target !== (progress?.exam?.target ?? home.exam?.target))
        .forEach((exam) => list.push({ label: `Proofs: ${exam.title}`, hint: exam.day ?? undefined, icon: "chart", run: () => void openProgress(exam.target) }));
      if (["ready", "empty", "not_set_up"].includes(home.state) && !home.sitting && (view === "course" || view === "session")) {
        list.push({ label: "Add course material…", hint: "or drop files on the window", icon: "clip", run: () => void chooseMaterial() });
        list.push({ label: "Paste text as course material…", icon: "clip", run: () => setPasting("") });
      }
      if (!inLesson && (home.checks_due ?? 0) > 0 && home.state === "ready") {
        list.push({ label: "Continue without the quick checks", hint: "they stay due", icon: "play", run: () => void continueLesson(true) });
      }
      if (!inLesson) {
        const next = home.papers.find((paper) => !paper.sat) ?? home.papers[0];
        list.push({
          label: "Sit a paper",
          hint: next ? `${next.name}${next.minutes ? ` · ${next.minutes} min` : ""}` : undefined,
          icon: "paper",
          run: () => openPaper(null),
        });
        list.push({ label: "New lesson", hint: "⌘N", icon: "plus", run: askNewLesson });
      }
      BLOCKS.filter((minutes) => minutes !== home.block_minutes).forEach((minutes) =>
        list.push({ label: `Block length: ${minutes} min`, hint: `now ${home.block_minutes}`, icon: "history", run: () => void setBlock(minutes) }),
      );
    }
    const account = setup?.account;
    if (account && (!account.signed_in || account.problem)) {
      list.push({ label: `Sign in to ${account.name}`, hint: account.detail ?? "signed out", icon: "warn", run: () => openSetup() });
    }
    if (!inLesson && setup) {
      const other: Payment = setup.payment === "plan" ? "api_key" : "plan";
      list.push({
        label: other === "plan" ? "Pay for lessons with your Claude plan" : "Pay for lessons with an API key",
        hint: `now ${account?.detail ?? (setup.payment === "plan" ? "your Claude plan" : "an API key")}`,
        icon: "gear",
        run: () => {
          openSetup();
          void choosePayment(other);
        },
      });
      list.push({ label: "Magnesium and Claude Code on this Mac", hint: aboutText(about) ?? undefined, icon: "gear", run: () => openSetup() });
    }
    if (!inLesson) {
      list.push({ label: "Open a folder…", hint: "⌘O", icon: "folder", run: () => void pickFolder() });
      list.push({ label: "New study folder…", icon: "plus", run: () => void newFolder() });
      list.push({ label: "Find courses on this Mac", icon: "search", run: () => (showLibrary(), void findFolders()) });
      if (home) {
        const folder = home.folder;
        list.push({
          label: `Remove ${displayName(home.name)} from the library`,
          hint: "the folder stays on your Mac",
          icon: "close",
          run: () => void forgetFolder(folder).then(showLibrary),
        });
      }
      folders
        .filter((path) => path !== home?.folder)
        .slice(0, 6)
        .forEach((path) =>
          list.push({
            label: `Open ${displayName(path.split("/").filter(Boolean).pop() ?? path)}`,
            hint: path,
            icon: "folder",
            run: () => api && void openFolder(api, path),
          }),
        );
    }
    return list;
  };

  let screen: JSX.Element | null = null;
  if (view === "loading") {
    screen = (
      <section className="screen loading">
        <div className="bar" data-tauri-drag-region />
        <div className="center">
          <span className="mark breathing" aria-label="Loading">
            <i />
            <i />
            <i />
          </span>
        </div>
      </section>
    );
  } else if (view === "setup") {
    screen = (
      <Setup
        setup={setup}
        about={about}
        error={error}
        signingIn={signingIn}
        canGoBack={!waitingForSetup.current}
        onBack={back}
        onReplace={() => void answerRuntime((api) => api.replaceRuntime())}
        onKeep={() => void answerRuntime((api) => api.keepRuntime())}
        onCheck={() => void checkAgain()}
        onSignIn={() => void signIn()}
        onPayment={(payment) => void choosePayment(payment)}
        onLink={openLink}
      />
    );
  } else if (view === "library" || !home) {
    screen = (
      <Library
        cards={cards}
        folders={folders}
        activity={activity}
        error={error}
        notice={notice}
        finding={finding}
        ready={setupReady(setup)}
        onOpen={(path) => api && void openFolder(api, path)}
        onPick={() => void pickFolder()}
        onNew={() => void newFolder()}
        onFind={() => void findFolders()}
        onForget={(path) => void forgetFolder(path)}
        onSetup={openSetup}
        onPalette={() => setPalette(true)}
      />
    );
  } else if (view === "course") {
    screen = (
      <Course
        home={home}
        progress={progress}
        asking={asking}
        error={error}
        notice={notice}
        upgrading={upgrading}
        starting={starting}
        onUpgrade={() => void upgrade()}
        onContinue={(skip) => void continueLesson(skip)}
        onNewLesson={(goal) => void newLesson(goal)}
        onAsk={setAsking}
        onPaper={openPaper}
        onProgress={() => void openProgress(null)}
        onRetry={() => {
          setError(null);
          void refreshHome();
        }}
        onLibrary={showLibrary}
        onPalette={() => setPalette(true)}
        material={material}
        adding={adding}
        onDismissMaterial={() => setMaterial(null)}
        onAddFiles={() => void chooseMaterial()}
        onPasteText={() => setPasting("")}
      />
    );
  } else if (view === "session") {
    screen = (
      <Session
        state={session}
        rail={rail}
        blockMinutes={home.block_minutes}
        onSend={(text) => void say(text)}
        onInterrupt={() => void guard((api) => api.interrupt())()}
        onDecide={(id: string, decision: Decision) =>
          void guard(async (api) => {
            await api.decide(id, decision);
            dispatch({ type: "event", event: { type: "withdrawn", id } });
          })()
        }
        onAnswer={(id: string, answers: Answer[]) =>
          void guard(async (api) => {
            await api.answer(id, answers);
            sitting.current.answers += 1;
            dispatch({ type: "event", event: { type: "withdrawn", id } });
          })()
        }
        onStop={(line) => void stop(line)}
        onNote={onNote}
        onDismiss={(text) => dispatch({ type: "dismiss", text })}
        onPalette={() => setPalette(true)}
        material={material}
        onDismissMaterial={() => setMaterial(null)}
        onAddFiles={() => void chooseMaterial()}
        onPasteText={() => setPasting("")}
      />
    );
  } else if (view === "summary" && summary) {
    screen = (
      <Summary
        sitting={summary}
        activity={activity}
        courseName={displayName(home.name)}
        onDone={() => setView("course")}
        onProgress={() => void openProgress(null)}
      />
    );
  } else if (view === "progress") {
    const inLesson = session.open || session.live !== null;
    screen = (
      <Progress
        data={progress}
        activity={activity}
        error={error}
        folderName={home.name}
        backLabel={inLesson ? "Lesson" : displayName(home.name)}
        onTarget={(target) => void loadProgress(target, true)}
        onStart={!inLesson && !home.sitting && home.state === "ready" ? () => void continueLesson(false) : undefined}
        onBack={back}
        onPalette={() => setPalette(true)}
      />
    );
  } else if (view === "paper") {
    screen = (
      <Paper
        key={home.sitting ? "running" : "choosing"}
        home={home}
        chosen={chosenPaper}
        error={error}
        onStart={(paper, minutes) => void startPaper(paper, minutes)}
        onHandIn={() => void handIn()}
        onEnd={() => void endPaper()}
        onBack={() => setView("course")}
      />
    );
  }

  return (
    <div className="app" data-busy={busy(session) ? "yes" : "no"} data-view={view}>
      <div className="view" key={view}>
        {screen}
      </div>
      {dragging && canAdd() && home && <DropVeil course={displayName(home.name)} />}
      {pasting !== null && <PasteSheet initial={pasting} saving={adding} onSave={(text) => void savePasted(text)} onCancel={() => setPasting(null)} />}
      {palette && <Palette commands={commands()} footer={aboutText(about)} onClose={() => setPalette(false)} />}
    </div>
  );
}

export type Actions = {
  continueLesson: (skipChecks: boolean) => Promise<void>;
  newLesson: (goal: string) => Promise<void>;
  say: (text: string) => Promise<void>;
  stop: (line: string) => Promise<void>;
  openProgress: (target: string | null) => Promise<void>;
  openPaper: (paper: PaperInfo | null) => void;
  startPaper: (paper: PaperInfo, minutes: number) => Promise<void>;
  setPalette: (open: boolean) => void;
  setAsking: (asking: boolean) => void;
  openSetup: () => void;
  showLibrary: () => void;
  setView: (view: string) => void;
  addMaterial: (paths: string[]) => Promise<void>;
  setDragging: (over: boolean) => void;
  pasteText: (text: string) => void;
};
