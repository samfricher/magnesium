// The window: which screen is up, and the few things that move between them.

import { useCallback, useEffect, useReducer, useRef, useState } from "react";

import { core, isDemo } from "./bridge";
import { Entrance } from "./Entrance";
import { Palette, type Command } from "./Palette";
import { Paper } from "./Paper";
import { Progress } from "./Progress";
import { Session } from "./Session";
import { Welcome, aboutText, setupReady } from "./Setup";
import { busy, emptySession, reduce } from "./lesson";
import type {
  About,
  AgentEvent,
  Answer,
  Core,
  Decision,
  Home,
  Paper as PaperInfo,
  Payment,
  Progress as ProgressData,
  Rail,
  RuntimeSetup,
  Setup,
} from "./types";

type View = "loading" | "welcome" | "entrance" | "session" | "progress" | "paper";

const BLOCKS = [25, 50, 90];
/** How often the setup screen asks Claude Code whether a sign-in finished, and for how long. */
const SIGN_IN_POLL_MS = 2000;
const SIGN_IN_LIMIT_MS = 15 * 60_000;

export function App() {
  const [api, setApi] = useState<Core | null>(null);
  const [view, setView] = useState<View>("loading");
  const [home, setHome] = useState<Home | null>(null);
  const [recent, setRecent] = useState<string[]>([]);
  const [session, dispatch] = useReducer(reduce, emptySession);
  const [rail, setRail] = useState<Rail | null>(null);
  const [progress, setProgress] = useState<ProgressData | null>(null);
  const [palette, setPalette] = useState(false);
  const [asking, setAsking] = useState(false);
  const [chosenPaper, setChosenPaper] = useState<PaperInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [setup, setSetup] = useState<Setup | null>(null);
  const [about, setAbout] = useState<About | null>(null);
  const [signingIn, setSigningIn] = useState(false);
  // What Claude Code said before the sign-in, to see when its answer changes.
  const accountBefore = useRef("");
  const [upgrading, setUpgrading] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  // Until the first folder opens, a finished setup opens it.
  const waitingForSetup = useRef(true);

  // The newest values, for handlers that outlive a render.
  const now = useRef({ api, view, home, session, palette, setup });
  now.current = { api, view, home, session, palette, setup };

  const refreshRail = useCallback(async () => {
    const { api } = now.current;
    if (!api) return;
    try {
      setRail(await api.rail());
    } catch {
      // The rail is a convenience. A lesson goes on without it.
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

  const showFolder = useCallback(async (api: Core, open: () => Promise<Home>) => {
    setError(null);
    setNotice(null);
    try {
      const opened = await open();
      waitingForSetup.current = false;
      setHome(opened);
      dispatch({ type: "reset" });
      setRail(null);
      setProgress(null);
      setAsking(false);
      setView(opened.sitting ? "paper" : "entrance");
      setRecent(await api.recent());
    } catch (problem) {
      setError(String(problem));
      setView("welcome");
    }
  }, []);

  const openFolder = useCallback((api: Core, path: string) => showFolder(api, () => api.openFolder(path)), [showFolder]);

  /** Take in what the setup check found. The first time it is ready, open the folder to study in. */
  const takeSetup = useCallback(
    async (api: Core, fresh: Setup) => {
      setSetup(fresh);
      if (!setupReady(fresh)) {
        if (now.current.view === "loading") setView("welcome");
        return;
      }
      if (!waitingForSetup.current) return;
      waitingForSetup.current = false;
      const first = await api.initialFolder();
      if (first) await openFolder(api, first);
      else setView("welcome");
    },
    [openFolder],
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
      setRecent(await api.recent());
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

  const openLink = (url: string) => void now.current.api?.openUrl(url).catch((problem) => setError(String(problem)));

  const upgrade = async () => {
    const { api } = now.current;
    if (!api) return;
    setError(null);
    setUpgrading(true);
    try {
      setHome(await api.upgradeFolder());
      setNotice("This folder is upgraded. If its record changed, the copy from before is in .magnesium/backups.");
    } catch (problem) {
      setError(String(problem));
    } finally {
      setUpgrading(false);
    }
  };

  const newFolder = async () => {
    const { api } = now.current;
    if (!api) return;
    const path = await api.pickNewFolder();
    if (path) await showFolder(api, () => api.newFolder(path));
  };

  /** Open the setup screen from ⌘K, with a fresh look at this Mac. */
  const openSetup = () => {
    setView("welcome");
    void checkAgain();
  };

  /** Open the lesson screen and send the first words of the sitting. */
  const begin = async (resume: boolean, shown: string, send: (api: Core) => Promise<void>) => {
    const { api } = now.current;
    if (!api) return;
    setError(null);
    try {
      const opened = await api.openSession(resume);
      dispatch({ type: "opened", turns: opened.turns });
      setView("session");
      dispatch({ type: "sent", shown, now: Date.now() });
      await send(api);
      void refreshRail();
    } catch (problem) {
      if (now.current.view === "session") dispatch({ type: "failed", error: String(problem) });
      else setError(String(problem));
    }
  };

  const continueLesson = (skipChecks: boolean) => {
    // Only the learner's own saved line is quoted back. The tutor's note of
    // what comes next is already in its record.
    const home = now.current.home;
    const stopped = home?.last?.stopped_at;
    // Plain words, as a learner would type them in a terminal. The runtime
    // opens the sitting with what is due, so the window does not ask for it.
    const words = skipChecks
      ? "No checks right now. Let's continue where I stopped."
      : stopped
        ? `I'm back. Let's continue. I stopped at: ${stopped}`
        : home?.last
          ? "I'm back. Let's continue."
          : `Let's work on ${home?.exam?.title ?? "this course"}.`;
    return begin(true, words, (api) => api.send(words));
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
    setAsking(false);
    await begin(false, goal, (api) => api.sendNewLesson(goal));
  };

  const say = async (text: string) => {
    const { api, session } = now.current;
    if (!api) return;
    try {
      if (!session.open) {
        await api.openSession(true);
        dispatch({ type: "reopened" });
      }
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

  const stop = async (line: string) => {
    const { api } = now.current;
    if (!api) return;
    try {
      await api.stop(line.trim() === "" ? null : line.trim());
    } catch (problem) {
      setError(String(problem));
    }
    dispatch({ type: "stopped" });
    setRail(null);
    await refreshHome();
    setView("entrance");
  };

  const openProgress = async (target: string | null) => {
    const { api } = now.current;
    if (!api) return;
    setError(null);
    setView("progress");
    try {
      setProgress(await api.progress(target));
    } catch (problem) {
      setError(String(problem));
    }
  };

  const setProgressView = (view: string) => {
    setProgress((old) => (old ? { ...old, view } : old));
    void now.current.api?.setProgressView(view);
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
      else setView("entrance");
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
    setView("entrance");
  };

  const pickFolder = async () => {
    const { api } = now.current;
    if (!api) return;
    const path = await api.pickFolder();
    if (path) await openFolder(api, path);
  };

  /** Where "back" goes: the lesson if one is on screen, else the entrance. */
  const back = () => {
    const { home, session } = now.current;
    if (home?.sitting) setView("paper");
    else if (session.open || session.live) setView("session");
    else {
      void refreshHome();
      setView(home ? "entrance" : "welcome");
    }
  };

  const setBlock = async (minutes: number) => {
    await now.current.api?.setBlockMinutes(minutes);
    setHome((old) => (old ? { ...old, block_minutes: minutes } : old));
  };

  // What the demo and the self-check may drive.
  const actions = {
    continueLesson,
    newLesson,
    say,
    openProgress,
    setProgressView,
    openPaper,
    startPaper,
    setPalette,
    setAsking,
    openSetup,
    setView: (next: string) => setView(next as View),
  };

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const command = event.metaKey || event.ctrlKey;
      const { view, session, home, api } = now.current;
      if (command && event.key.toLowerCase() === "k") {
        event.preventDefault();
        if (view !== "loading" && view !== "welcome") setPalette((open) => !open);
      } else if (command && event.key === "." && view === "session" && busy(session)) {
        event.preventDefault();
        void api?.interrupt();
      } else if (command && event.key.toLowerCase() === "n" && home && !home.sitting && !session.open) {
        event.preventDefault();
        setView("entrance");
        setAsking(true);
      } else if (event.key === "Escape" && view === "progress" && !event.defaultPrevented && !now.current.palette) {
        back();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const commands = (): Command[] => {
    if (!home) return [];
    if (home.sitting) return [{ label: "Back to the paper", hint: home.sitting.name, run: () => setView("paper") }];
    const list: Command[] = [];
    const inLesson = session.open || session.live !== null;
    if (view !== "session" && inLesson) list.push({ label: "Back to the lesson", run: () => setView("session") });
    if (view !== "entrance" && !inLesson) list.push({ label: "Back to the start", run: back });
    list.push({ label: "Progress", hint: "what you have proved", run: () => void openProgress(null) });
    (progress?.targets ?? [])
      .filter((exam) => exam.target !== (progress?.exam?.target ?? home.exam?.target))
      .forEach((exam) =>
        list.push({ label: `Progress: ${exam.title}`, hint: exam.day ?? undefined, run: () => void openProgress(exam.target) }),
      );
    const next = home.papers.find((paper) => !paper.sat) ?? home.papers[0];
    list.push({
      label: "Sit a paper",
      hint: next ? `${next.name}${next.minutes ? ` · ${next.minutes} min` : ""}` : undefined,
      run: () => openPaper(null),
    });
    if (!inLesson)
      list.push({
        label: "New lesson",
        hint: "⌘N",
        run: () => {
          setView("entrance");
          setAsking(true);
        },
      });
    BLOCKS.filter((minutes) => minutes !== home.block_minutes).forEach((minutes) =>
      list.push({ label: `Block length: ${minutes} min`, hint: `now ${home.block_minutes}`, run: () => void setBlock(minutes) }),
    );
    const account = setup?.account;
    if (account && (!account.signed_in || account.problem)) {
      list.push({ label: `Sign in to ${account.name}`, hint: account.detail ?? "signed out", run: () => openSetup() });
    }
    if (!inLesson && setup) {
      const other: Payment = setup.payment === "plan" ? "api_key" : "plan";
      list.push({
        label: other === "plan" ? "Pay for lessons with your Claude plan" : "Pay for lessons with an API key",
        hint: `now ${account?.detail ?? (setup.payment === "plan" ? "your Claude plan" : "an API key")}`,
        run: () => {
          setView("welcome");
          void choosePayment(other);
        },
      });
      list.push({ label: "Magnesium and Claude Code on this Mac", hint: aboutText(about) ?? undefined, run: () => openSetup() });
    }
    if (!inLesson) {
      list.push({ label: "Open another folder…", run: () => void pickFolder() });
      list.push({ label: "New study folder…", run: () => void newFolder() });
      recent
        .filter((path) => path !== home.folder)
        .slice(0, 4)
        .forEach((path) =>
          list.push({
            label: `Open ${path.split("/").filter(Boolean).pop()}`,
            hint: path,
            run: () => api && void openFolder(api, path),
          }),
        );
    }
    return list;
  };

  let screen: JSX.Element | null = null;
  if (view === "loading" && !setup) {
    screen = null;
  } else if (view === "welcome" || (view !== "loading" && !home)) {
    screen = (
      <Welcome
        setup={setup}
        about={about}
        recent={recent}
        error={error}
        signingIn={signingIn}
        canGoBack={home !== null}
        onPick={() => void pickFolder()}
        onNew={() => void newFolder()}
        onOpen={(path) => api && void openFolder(api, path)}
        onBack={back}
        onReplace={() => void answerRuntime((api) => api.replaceRuntime())}
        onKeep={() => void answerRuntime((api) => api.keepRuntime())}
        onCheck={() => void checkAgain()}
        onSignIn={() => void signIn()}
        onPayment={(payment) => void choosePayment(payment)}
        onLink={openLink}
      />
    );
  } else if (home && view === "entrance") {
    screen = (
      <Entrance
        home={home}
        asking={asking}
        error={error}
        notice={notice}
        upgrading={upgrading}
        onUpgrade={() => void upgrade()}
        onContinue={(skip) => void continueLesson(skip)}
        onNewLesson={(goal) => void newLesson(goal)}
        onAsk={setAsking}
        onPaper={openPaper}
        onRetry={() => {
          setError(null);
          void refreshHome();
        }}
        onPalette={() => setPalette(true)}
      />
    );
  } else if (home && view === "session") {
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
            dispatch({ type: "event", event: { type: "withdrawn", id } });
          })()
        }
        onStop={(line) => void stop(line)}
        onNote={onNote}
        onDismiss={(text) => dispatch({ type: "dismiss", text })}
        onPalette={() => setPalette(true)}
      />
    );
  } else if (home && view === "progress") {
    screen = <Progress data={progress} error={error} folderName={home.name} onView={setProgressView} onPalette={() => setPalette(true)} />;
  } else if (home && view === "paper") {
    screen = (
      <Paper
        key={home.sitting ? "running" : "choosing"}
        home={home}
        chosen={chosenPaper}
        error={error}
        onStart={(paper, minutes) => void startPaper(paper, minutes)}
        onHandIn={() => void handIn()}
        onEnd={() => void endPaper()}
        onBack={back}
      />
    );
  }

  return (
    <div className="win" data-busy={busy(session) ? "yes" : "no"}>
      {screen}
      {palette && <Palette commands={commands()} footer={aboutText(about)} onClose={() => setPalette(false)} />}
    </div>
  );
}

export type Actions = {
  continueLesson: (skipChecks: boolean) => Promise<void>;
  newLesson: (goal: string) => Promise<void>;
  say: (text: string) => Promise<void>;
  openProgress: (target: string | null) => Promise<void>;
  setProgressView: (view: string) => void;
  openPaper: (paper: PaperInfo | null) => void;
  startPaper: (paper: PaperInfo, minutes: number) => Promise<void>;
  setPalette: (open: boolean) => void;
  setAsking: (asking: boolean) => void;
  openSetup: () => void;
  setView: (view: string) => void;
};
