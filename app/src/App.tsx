// The window: which screen is up, and the few things that move between them.

import { useCallback, useEffect, useReducer, useRef, useState } from "react";

import { core, isDemo } from "./bridge";
import { Entrance, Welcome } from "./Entrance";
import { Palette, type Command } from "./Palette";
import { Paper } from "./Paper";
import { Progress } from "./Progress";
import { Session } from "./Session";
import { busy, emptySession, reduce } from "./lesson";
import type { AgentEvent, Answer, Core, Decision, Home, Paper as PaperInfo, Progress as ProgressData, Rail } from "./types";

type View = "loading" | "welcome" | "entrance" | "session" | "progress" | "paper";

const BLOCKS = [25, 50, 90];

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

  // The newest values, for handlers that outlive a render.
  const now = useRef({ api, view, home, session, palette });
  now.current = { api, view, home, session, palette };

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

  const openFolder = useCallback(async (api: Core, path: string) => {
    setError(null);
    try {
      const opened = await api.openFolder(path);
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
      const first = await api.initialFolder();
      if (first) await openFolder(api, first);
      else setView("welcome");
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
    if (!inLesson) {
      list.push({ label: "Open another folder…", run: () => void pickFolder() });
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
  if (view === "welcome" || (view !== "loading" && !home)) {
    screen = <Welcome recent={recent} error={error} onPick={() => void pickFolder()} onOpen={(path) => api && void openFolder(api, path)} />;
  } else if (home && view === "entrance") {
    screen = (
      <Entrance
        home={home}
        asking={asking}
        error={error}
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
      {palette && <Palette commands={commands()} onClose={() => setPalette(false)} />}
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
  setView: (view: string) => void;
};
