// The window: a quiet row of tabs over three levels.
//
//   Home       every workspace in the library, and the most recent session
//              first, so Return carries on. A workspace's page opens in the
//              Home tab: its sessions under a visible "New session", its
//              material, papers, and the folder itself.
//   Session    the one-surface lesson of #67, in a tab of its own.
//   Settings   a readable page, in a tab of its own.
//
// Every command is a visible control, an item in the menu bar, and, for the
// few the window itself has, a row in ⌘K. The menu sends what a click sends.
// How the window was left (the tabs, the one in front, the plan beside the
// lesson) is kept in the window's own state, as fields an older build skips.

import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";

import { core, isDemo } from "./bridge";
import { harnessOn, mark, ready, setProbe } from "./harness";
import { HomePage, mostRecent, setupStep } from "./pages/Home";
import { SettingsPage } from "./pages/Settings";
import { Tabs, tabId, type TabView } from "./pages/Tabs";
import { WorkspacePage } from "./pages/Workspace";
import { actionOf, applyPrefs, flushPlaces, keyName, loadPrefs, savePrefs, SITTING_GAP_MS, type KeyAction, type Prefs } from "./prefs";
import type { SourceRef } from "./sources";
import { Commands } from "./surface/Commands";
import { Session, setupReady, type Pending, type SessionHandle, type SessionTab } from "./surface/Session";
import { PasteSheet, SourceSheet } from "./surface/Sheets";
import type { About, AgentEvent, Core, Engine, EngineChoice, MenuChoice, MenuSpec, Payment, Setup, Workspace } from "./types";
import { displayName, withoutPaths } from "./words";

type HomeTab = { key: "home"; kind: "home" };
type SettingsTab = { key: "settings"; kind: "settings" };
export type Tab = HomeTab | SessionTab | SettingsTab;

type Overlay = { kind: "commands" } | { kind: "source"; folder: string; source: SourceRef } | { kind: "paste"; folder: string } | null;

/** How the window was left, as kept in its state file. */
type Saved = {
  v: 1;
  tabs: ({ kind: "session"; key: string; folder: string; session: string | null; title: string } | { kind: "settings" })[];
  active: number;
  /** The workspace whose page the Home tab showed, or null for Home itself. */
  home: string | null;
  panel: boolean;
  seen_ms: number;
};

const HOME: HomeTab = { key: "home", kind: "home" };
const SETTINGS: SettingsTab = { key: "settings", kind: "settings" };

/** A session not begun yet is named by its workspace: "Intro Statistics, new". */
const newTitle = (name: string) => `${displayName(name)}, new`;

let counter = 0;
const newKey = () => `s${Date.now().toString(36)}${(counter++).toString(36)}`;

/** What the self-check and the demo drive the window with. */
export type Actions = {
  run: (command: string, arg?: string) => void;
  /** Home's first row: carry on the most recent session, or start one. */
  continueRecent: () => void;
  newSession: (folder?: string, pending?: Pending) => void;
  openSession: (folder: string, session: string, title: string) => void;
  goWorkspace: (folder: string | null) => void;
  /** The session in front, or null. */
  session: () => SessionHandle | null;
  tabs: () => { key: string; kind: Tab["kind"]; title: string }[];
  active: () => string;
  workspaces: () => Workspace[];
  reload: () => Promise<void>;
};

export function App() {
  const [api, setApi] = useState<Core | null>(null);
  const [setup, setSetup] = useState<Setup | null>(null);
  const [about, setAbout] = useState<About | null>(null);
  const [engine, setEngine] = useState<EngineChoice | null>(null);
  const [prefs, setPrefs] = useState<Prefs>(() => loadPrefs());
  const [workspaces, setWorkspaces] = useState<Workspace[] | null>(null);
  const [tabs, setTabs] = useState<Tab[]>([HOME]);
  const [active, setActive] = useState<string>("home");
  const [homeFolder, setHomeFolder] = useState<string | null>(null);
  const [panel, setPanel] = useState(false);
  const [overlay, setOverlay] = useState<Overlay>(null);
  const [liveKey, setLiveKey] = useState<string | null>(null);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [taskOpen, setTaskOpen] = useState<Record<string, boolean>>({});
  const [notes, setNotes] = useState<string[]>([]);
  const [signingIn, setSigningIn] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [restored, setRestored] = useState(false);
  const [settingsSection, setSettingsSection] = useState<string | null>(null);
  const [blockMinutes, setBlockMinutes] = useState(50);
  const [materialChanged, setMaterialChanged] = useState(0);
  const handles = useRef<Record<string, SessionHandle>>({});
  const setupWait = useRef<Promise<Setup> | null>(null);
  // The commands a key just ran: the menu's echo of the same press is not run twice.
  const pressed = useRef<Record<string, number>>({});

  // The newest values, for handlers that outlive a render.
  const now = useRef({ api, setup, prefs, workspaces, tabs, active, homeFolder, panel, overlay, liveKey, busyKey });
  now.current = { api, setup, prefs, workspaces, tabs, active, homeFolder, panel, overlay, liveKey, busyKey };

  useLayoutEffect(() => applyPrefs(prefs), [prefs]);

  const changePrefs = useCallback((change: Partial<Prefs>) => {
    setPrefs((old) => {
      const next = { ...old, ...change };
      savePrefs(next);
      applyPrefs(next, change.scheme !== undefined);
      return next;
    });
  }, []);

  const note = useCallback((said: string) => {
    const text = withoutPaths(said.replace(/^Error: /, ""));
    setNotes((old) => [...old.filter((n) => n !== text), text].slice(-3));
  }, []);

  const loadWorkspaces = useCallback(async () => {
    const found = await now.current.api?.workspaces().catch(() => null);
    if (found) setWorkspaces(found);
  }, []);

  const loadEngine = useCallback(async () => {
    try {
      const fresh = await now.current.api?.engine();
      if (fresh) setEngine(fresh);
    } catch {
      setEngine(null);
    }
  }, []);

  // ---------------------------------------------------------------------------
  // Tabs

  const activate = useCallback((key: string) => {
    setOverlay(null);
    setActive(key);
    const tab = now.current.tabs.find((each) => each.key === key);
    if (tab?.kind === "session" && tab.session) void now.current.api?.touchSession(tab.folder, tab.session).catch(() => {});
  }, []);

  const goWorkspace = useCallback(
    (folder: string | null) => {
      setHomeFolder(folder);
      activate("home");
      if (folder) void now.current.api?.visitFolder(folder).then(() => loadWorkspaces(), () => {});
    },
    [activate, loadWorkspaces],
  );

  const addSession = useCallback(
    (tab: Omit<SessionTab, "key" | "kind">) => {
      const key = newKey();
      setTabs((old) => {
        const settings = old.findIndex((each) => each.kind === "settings");
        const next = [...old];
        // A session goes after the sessions, before Settings.
        next.splice(settings === -1 ? next.length : settings, 0, { key, kind: "session", ...tab });
        return next;
      });
      setOverlay(null);
      setActive(key);
      return key;
    },
    [],
  );

  /** The workspace in front: the page in the Home tab, or the session's. */
  const folderInFront = (): string | null => {
    const { tabs, active, homeFolder, workspaces } = now.current;
    const tab = tabs.find((each) => each.key === active);
    if (tab?.kind === "session") return tab.folder;
    if (tab?.kind === "home" && homeFolder) return homeFolder;
    return workspaces?.[0]?.folder ?? null;
  };

  const newSession = useCallback(
    (folder?: string, pending?: Pending) => {
      const where = folder ?? folderInFront();
      if (!where) {
        goWorkspace(null);
        note("Open a workspace first: a folder your course is in.");
        return;
      }
      // A workspace has one session not begun at a time: New session goes to it.
      const waiting = now.current.tabs.find((each) => each.kind === "session" && each.folder === where && each.session === null && !each.pending && !handles.current[each.key]?.state().live);
      if (waiting) {
        activate(waiting.key);
        if (pending?.kind === "start") void handles.current[waiting.key]?.start();
        return;
      }
      const name = now.current.workspaces?.find((each) => each.folder === where)?.name ?? where.split("/").pop() ?? "";
      addSession({ folder: where, session: null, title: newTitle(name), ...(pending ? { pending } : {}) });
      void loadWorkspaces();
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [activate, addSession, goWorkspace, note, loadWorkspaces],
  );

  /** A kept session, in its tab as it was left: its turns, its open task and the line in it. Nothing is sent. */
  const openSession = useCallback(
    (folder: string, session: string, title: string) => {
      const open = now.current.tabs.find((each) => each.kind === "session" && each.folder === folder && each.session === session);
      if (open) return activate(open.key);
      void now.current.api?.touchSession(folder, session).catch(() => {});
      addSession({ folder, session, title });
    },
    [activate, addSession],
  );

  const continueRecent = useCallback(() => {
    const list = now.current.workspaces ?? [];
    const recent = mostRecent(list);
    if (recent?.latest) openSession(recent.folder, recent.latest.session, recent.latest.title ?? "");
    else if (list[0]) newSession(list[0].folder, { kind: "start" });
  }, [openSession, newSession]);

  const closeTab = useCallback(
    async (key: string) => {
      if (key === "home") return;
      const { tabs, active } = now.current;
      const at = tabs.findIndex((each) => each.key === key);
      if (at === -1) return;
      await handles.current[key]?.close();
      if (now.current.liveKey === key) setLiveKey(null);
      if (now.current.busyKey === key) setBusyKey(null);
      setTabs((old) => old.filter((each) => each.key !== key));
      if (active === key) setActive(tabs[Math.max(0, at - 1)]?.key ?? "home");
      void loadWorkspaces();
    },
    [loadWorkspaces],
  );

  const openSettings = useCallback((section: string | null = null) => {
    setTabs((old) => (old.some((each) => each.kind === "settings") ? old : [...old, SETTINGS]));
    setSettingsSection(section);
    setOverlay(null);
    setActive("settings");
  }, []);

  const step = (by: number) => {
    const { tabs, active } = now.current;
    const at = tabs.findIndex((each) => each.key === active);
    activate(tabs[(at + by + tabs.length) % tabs.length].key);
  };

  // ---------------------------------------------------------------------------
  // Commands: one place for the menu, the keys, ⌘K and the visible controls

  const pickFolder = async () => {
    const { api } = now.current;
    if (!api) return;
    const path = await api.pickFolder();
    if (!path) return;
    try {
      await api.visitFolder(path);
      await loadWorkspaces();
      goWorkspace(path);
    } catch (problem) {
      note(String(problem));
    }
  };

  const newFolder = async () => {
    const { api } = now.current;
    if (!api) return;
    const path = await api.pickNewFolder();
    if (!path) return;
    try {
      const made = await api.newFolder(path);
      await loadWorkspaces();
      goWorkspace(made.folder);
    } catch (problem) {
      note(String(problem));
    }
  };

  const findFolders = async () => {
    const { api } = now.current;
    if (!api) return;
    try {
      const added = await api.findFolders();
      note(added === 0 ? "No other study folder was found on this Mac." : `${added} ${added === 1 ? "folder" : "folders"} added.`);
      await loadWorkspaces();
    } catch (problem) {
      note(String(problem));
    }
  };

  const run = useCallback(
    (command: string, arg?: string | null) => {
      const { tabs, active, prefs } = now.current;
      const tab = tabs.find((each) => each.key === active);
      const inFront = tab?.kind === "session" ? handles.current[tab.key] : null;
      switch (command) {
        case "home":
          return goWorkspace(null);
        case "go":
          return arg ? goWorkspace(arg) : undefined;
        case "open-workspace":
          return void pickFolder();
        case "new-session":
          return newSession(arg ?? undefined);
        case "close-session":
          return tab && tab.kind !== "home" ? void closeTab(tab.key) : undefined;
        case "settings":
          return openSettings(arg ?? null);
        case "about":
          return openSettings("about");
        case "keys-help":
          return openSettings("keys");
        case "commands":
          setOverlay((old) => (old?.kind === "commands" ? null : { kind: "commands" }));
          return;
        case "plan":
          setPanel((shown) => !shown);
          return;
        case "bigger":
        case "smaller":
        case "actual-size": {
          const size = command === "actual-size" ? 17 : Math.max(12, Math.min(28, prefs.size + (command === "smaller" ? -1 : 1)));
          return changePrefs({ size });
        }
        case "tab":
          return arg ? activate(arg) : undefined;
        case "previous-tab":
          return step(-1);
        case "next-tab":
          return step(1);
        case "stop-reply": {
          const live = now.current.liveKey ? handles.current[now.current.liveKey] : null;
          return void live?.command("stop-reply");
        }
        case "attach":
        case "photo":
        case "hint":
        case "lost":
        case "unknown":
        case "symbols":
          if (inFront) inFront.command(command);
          else note("Open a session first.");
          return;
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [activate, changePrefs, closeTab, goWorkspace, newSession, note, openSettings],
  );
  const runRef = useRef(run);
  runRef.current = run;

  // ---------------------------------------------------------------------------
  // Starting up

  useEffect(() => {
    let alive = true;
    const stops: (() => void)[] = [];
    void (async () => {
      mark("page");
      const api = await core();
      mark("core");
      // The first calls go together: nothing waits on another it does not need.
      const [offEvents, offMenu, initial, kept, found] = await Promise.all([
        api.onEvent((event: AgentEvent) => onEventRef.current(event)),
        api.onMenu((choice: MenuChoice) => onMenuRef.current(choice)),
        api.initialFolder(),
        api.windowState().catch(() => null),
        api.workspaces().catch(() => [] as Workspace[]),
      ]);
      mark("first-calls");
      stops.push(offEvents, offMenu);
      if (!alive) return stops.forEach((stop) => stop());
      now.current.api = api;
      setApi(api);
      const looking = api.setup();
      setupWait.current = looking;
      void api.about().then(setAbout, () => {});
      // The tabs as they were left: sessions of folders still in the library, and Settings.
      const library = new Set(found.map((each) => each.folder));
      const saved = (kept ?? null) as Saved | null;
      const back: Tab[] = [HOME];
      if (saved?.v === 1 && Array.isArray(saved.tabs)) {
        for (const each of saved.tabs) {
          if (each.kind === "settings") back.push(SETTINGS);
          else if (each.kind === "session" && library.has(each.folder) && each.session)
            back.push({ key: each.key || newKey(), kind: "session", folder: each.folder, session: each.session, title: each.title ?? "" });
        }
        setPanel(Boolean(saved.panel));
      }
      setTabs(back);
      setWorkspaces(found);
      now.current.workspaces = found;
      // In front: the tab left in front, within the runtime's gap for a new
      // sitting; after it, Home, so starting is one key again.
      const recently = saved ? Date.now() - (saved.seen_ms ?? 0) < SITTING_GAP_MS : false;
      const wanted = saved && recently ? back[Math.min(Math.max(0, saved.active ?? 0), back.length - 1)] : HOME;
      if (initial) {
        await api.visitFolder(initial).catch(() => null);
        setHomeFolder(initial);
        setActive("home");
        void loadWorkspaces();
      } else {
        setHomeFolder(saved && recently && saved.home && library.has(saved.home) ? saved.home : null);
        setActive(wanted?.key ?? "home");
      }
      setRestored(true);
      const fresh = await looking;
      now.current.setup = fresh;
      setSetup(fresh);
      void loadEngine();
      if (isDemo) {
        await (await import("./demo")).startFromAddress(actions);
        document.documentElement.dataset.shot = "ready";
      } else if (await api.selftest()) {
        try {
          await (await import("./selftest")).run(api, actions);
        } catch (problem) {
          await api.selftestReport({ ok: false, failure: `the check could not start: ${String(problem)}`, errors: [...window.__mgErrors], steps: {} });
        }
      }
    })();
    return () => {
      alive = false;
      stops.forEach((stop) => stop());
    };
    // Runs once: every handler it uses reads the newest values through refs.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // The first screen is painted with its real content: tell the harness.
  useEffect(() => {
    if (restored && workspaces !== null) requestAnimationFrame(() => ready());
  }, [restored, workspaces]);

  // While Claude Code's sign-in waits for the browser, look every few seconds whether it finished.
  useEffect(() => {
    if (!signingIn) return;
    const started = Date.now();
    const timer = window.setInterval(() => {
      const { api, setup } = now.current;
      if (!api || !setup || Date.now() - started > 15 * 60_000) return setSigningIn(false);
      void api.account().then((account) => {
        setSetup({ ...setup, account });
        if (account.signed_in) setSigningIn(false);
      });
    }, 2000);
    return () => window.clearInterval(timer);
  }, [signingIn]);

  const checkAgain = async () => {
    const { api } = now.current;
    if (!api) return;
    setSetup(await api.setup());
    void loadWorkspaces();
  };

  const signIn = async () => {
    try {
      await now.current.api?.signIn();
      setSigningIn(true);
    } catch (problem) {
      note(String(problem));
    }
  };

  const answerRuntime = async (work: (api: Core) => Promise<Setup["runtime"]>) => {
    const { api, setup } = now.current;
    if (!api || !setup) return;
    setSetup({ ...setup, runtime: await work(api) });
  };

  const choosePayment = async (payment: Payment) => {
    const { api, setup } = now.current;
    if (!api || !setup) return;
    setSetup({ ...setup, payment });
    const account = await api.setPayment(payment);
    setSetup({ ...setup, payment, account });
  };

  const lessonOpen = () => Object.values(handles.current).some((handle) => handle.state().open);

  /** Pick the engine for the next lesson. Saved at once; refused while a lesson is open. */
  const chooseEngine = async (chosen: Engine) => {
    const { api } = now.current;
    if (!api || !engine || lessonOpen() || engine.chosen === chosen) return;
    setEngine({ ...engine, chosen });
    try {
      setEngine(await api.setEngine(chosen));
    } catch (problem) {
      setEngine(engine);
      note(String(problem));
    }
  };

  const chooseBlock = (minutes: number) => {
    setBlockMinutes(minutes);
    void now.current.api?.setBlockMinutes(minutes);
  };

  // Settings reads a paper's time from a workspace, where the core keeps it.
  useEffect(() => {
    if (active !== "settings" || !api || !workspaces?.[0]) return;
    void api.home(workspaces[0].folder).then((home) => setBlockMinutes(home.block_minutes), () => {});
  }, [active, api, workspaces]);

  // ---------------------------------------------------------------------------
  // The tutor's events go to the session that holds its program

  const onEventRef = useRef<(event: AgentEvent) => void>(() => {});
  onEventRef.current = (event: AgentEvent) => {
    const key = now.current.liveKey;
    if (key) handles.current[key]?.event(event);
  };

  const claim = useCallback((key: string) => {
    const before = now.current.liveKey;
    if (before && before !== key) {
      handles.current[before]?.release();
      if (now.current.busyKey === before) setBusyKey(null);
    }
    now.current.liveKey = key;
    setLiveKey(key);
  }, []);

  const register = useCallback((key: string, handle: SessionHandle | null) => {
    if (handle) handles.current[key] = handle;
    else delete handles.current[key];
  }, []);

  const onSession = useCallback(
    (key: string, session: string) => {
      setTabs((old) => old.map((each) => (each.key === key && each.kind === "session" ? { ...each, session } : each)));
      void loadWorkspaces();
    },
    [loadWorkspaces],
  );
  const onTitle = useCallback((key: string, title: string) => {
    setTabs((old) => old.map((each) => (each.key === key && each.kind === "session" ? { ...each, title } : each)));
  }, []);
  const onBusy = useCallback((key: string, busy: boolean) => setBusyKey((old) => (busy ? key : old === key ? null : old)), []);
  const [asking, setAsking] = useState<Record<string, boolean>>({});
  const onAsking = useCallback((key: string, now: boolean) => setAsking((old) => (Boolean(old[key]) === now ? old : { ...old, [key]: now })), []);
  const onStarted = useCallback((key: string) => {
    setTabs((old) => old.map((each) => (each.key === key && each.kind === "session" && each.pending ? { ...each, pending: undefined } : each)));
  }, []);
  const onTaskChange = useCallback((key: string, open: boolean) => setTaskOpen((old) => (old[key] === open ? old : { ...old, [key]: open })), []);
  const onSource = useCallback((folder: string, source: SourceRef) => setOverlay({ kind: "source", folder, source }), []);
  const onEngine = useCallback(() => void loadEngine(), [loadEngine]);

  // ---------------------------------------------------------------------------
  // The menu bar: what it holds follows the window, and a choice runs the same command

  const onMenuRef = useRef<(choice: MenuChoice) => void>(() => {});
  onMenuRef.current = (choice: MenuChoice) => {
    // The same press, seen by the page and then by the menu, runs once.
    if (Date.now() - (pressed.current[choice.command] ?? 0) < 400) return;
    runRef.current(choice.command, choice.arg);
  };

  const activeTab = tabs.find((each) => each.key === active) ?? HOME;
  const inSession = activeTab.kind === "session";
  // Each tab's name; two sessions with one name are told apart by a number, in the bar and the Window menu.
  const tabTitles = useMemo(() => {
    const seen: Record<string, number> = {};
    const named: Record<string, string> = {};
    for (const tab of tabs) {
      const title = tab.kind === "home" ? "Home" : tab.kind === "settings" ? "Settings" : tab.title || newTitle(tab.folder.split("/").pop() ?? "");
      seen[title] = (seen[title] ?? 0) + 1;
      named[tab.key] = seen[title] > 1 ? `${title} (${seen[title]})` : title;
    }
    return named;
  }, [tabs]);
  const spec = useMemo((): MenuSpec => {
    const disabled: string[] = [];
    if (!inSession) disabled.push("attach", "photo", "symbols", "hint", "lost", "unknown");
    else if (!taskOpen[activeTab.key]) disabled.push("hint", "lost", "unknown");
    if (activeTab.kind === "home") disabled.push("close-session");
    if (!inSession) disabled.push("plan");
    if (busyKey === null) disabled.push("stop-reply");
    return {
      keys: Object.fromEntries(Object.entries(prefs.keys)),
      workspaces: (workspaces ?? []).map((each) => ({ id: each.folder, title: displayName(each.name) })),
      tabs: tabs.map((tab) => ({ id: tab.key, title: tabTitles[tab.key] })),
      disabled,
      titles: { plan: panel ? "Hide Plan" : "Show Plan", ...(activeTab.kind === "settings" ? { "close-session": "Close Settings" } : {}) },
    };
  }, [prefs.keys, workspaces, tabs, tabTitles, inSession, activeTab, taskOpen, busyKey, panel]);
  useEffect(() => {
    if (!api || isDemo) return;
    const timer = window.setTimeout(() => void api.setMenu(spec).catch(() => {}), 80);
    return () => window.clearTimeout(timer);
  }, [api, spec]);

  // ---------------------------------------------------------------------------
  // How the window was left, kept as it changes

  useEffect(() => {
    if (!api || !restored || isDemo) return;
    const timer = window.setTimeout(() => {
      const saved: Saved = {
        v: 1,
        tabs: tabs
          .filter((tab) => tab.kind !== "home")
          .map((tab) => (tab.kind === "settings" ? { kind: "settings" as const } : { kind: "session" as const, key: tab.key, folder: tab.folder, session: tab.session, title: tab.title })),
        active: Math.max(0, tabs.findIndex((tab) => tab.key === active) - 0),
        home: homeFolder,
        panel,
        seen_ms: Date.now(),
      };
      // The saved list has no Home: its positions are one less, and Home is 0.
      const at = tabs.findIndex((tab) => tab.key === active);
      saved.active = at;
      void api.setWindowState(saved).catch(() => {});
    }, 300);
    return () => window.clearTimeout(timer);
  }, [api, restored, tabs, active, homeFolder, panel]);

  useEffect(() => {
    const save = () => flushPlaces();
    window.addEventListener("pagehide", save);
    return () => window.removeEventListener("pagehide", save);
  }, []);

  // ---------------------------------------------------------------------------
  // Keys

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;
      // A key an input method is composing with is its own: Esc there cancels the composition, not the reply or the task.
      if (event.isComposing || event.keyCode === 229) return;
      const { prefs, overlay, tabs, active } = now.current;
      const action: KeyAction | null = actionOf(event, prefs.keys);
      if (action) {
        // ⌘K is the one command a layer over the window leaves to the window.
        if (overlay && action !== "commands") return;
        event.preventDefault();
        pressed.current[action] = Date.now();
        runRef.current(action);
        return;
      }
      if (overlay) return;
      const command = event.metaKey && !event.ctrlKey && !event.altKey;
      if (command && !event.shiftKey && /^[1-9]$/.test(event.key)) {
        const tab = tabs[Number(event.key) - 1];
        if (tab) {
          event.preventDefault();
          pressed.current.tab = Date.now();
          activate(tab.key);
        }
        return;
      }
      if (command && event.shiftKey && (event.code === "BracketLeft" || event.code === "BracketRight")) {
        event.preventDefault();
        const which = event.code === "BracketLeft" ? "previous-tab" : "next-tab";
        pressed.current[which] = Date.now();
        runRef.current(which);
        return;
      }
      if (event.key === "Escape") {
        const tab = tabs.find((each) => each.key === active);
        if (tab?.kind === "session" && handles.current[tab.key]?.escape()) event.preventDefault();
        else if (tab?.kind === "home" && now.current.homeFolder) {
          event.preventDefault();
          goWorkspace(null);
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // What the evidence harness may ask of the session in front: inert without it.
  useEffect(() => {
    setProbe({
      session: () => {
        const tab = now.current.tabs.find((each) => each.key === now.current.active);
        return tab?.kind === "session" ? (handles.current[tab.key]?.probe() ?? null) : null;
      },
      attach: (paths) => {
        if (!harnessOn()) return false;
        const tab = now.current.tabs.find((each) => each.key === now.current.active);
        const handle = tab?.kind === "session" ? handles.current[tab.key] : null;
        handle?.drop(paths);
        return Boolean(handle);
      },
    });
  }, []);

  // ---------------------------------------------------------------------------
  // Files dropped on the window: on a session they join its line; on a
  // workspace's page they are its material.

  const dropRef = useRef<(paths: string[]) => void>(() => {});
  dropRef.current = (paths: string[]) => {
    const { api, tabs, active, homeFolder } = now.current;
    const tab = tabs.find((each) => each.key === active);
    if (!api) return;
    if (tab?.kind === "session") handles.current[tab.key]?.drop(paths);
    else if (tab?.kind === "home" && homeFolder) {
      void api.addMaterial(paths, homeFolder).then(
        (added) => {
          added.lines.forEach(note);
          setMaterialChanged((n) => n + 1);
        },
        (problem) => note(String(problem)),
      );
    } else note("Open a workspace, then drop its files on its page.");
  };
  useEffect(() => {
    if (!api) return;
    let off = () => {};
    let alive = true;
    void api
      .onDrop(
        (paths) => dropRef.current(paths),
        (over) => setDragging(over),
      )
      .then((stop) => (alive ? (off = stop) : stop()));
    return () => {
      alive = false;
      off();
    };
  }, [api]);

  // ---------------------------------------------------------------------------
  // What the window shows

  const actions: Actions = {
    run: (command, arg) => runRef.current(command, arg),
    continueRecent,
    newSession,
    openSession: (folder, session, title) => openSession(folder, session, title),
    goWorkspace,
    session: () => {
      const tab = now.current.tabs.find((each) => each.key === now.current.active);
      return tab?.kind === "session" ? (handles.current[tab.key] ?? null) : null;
    },
    tabs: () => now.current.tabs.map((tab) => ({ key: tab.key, kind: tab.kind, title: tab.kind === "session" ? tab.title : tab.kind })),
    active: () => now.current.active,
    workspaces: () => now.current.workspaces ?? [],
    reload: loadWorkspaces,
  };

  const step_ = setupStep(setup, signingIn, {
    replace: () => void answerRuntime((api) => api.replaceRuntime()),
    keep: () => void answerRuntime((api) => api.keepRuntime()),
    checkAgain: () => void checkAgain(),
    openUrl: (url) => void api?.openUrl(url),
    signIn: () => void signIn(),
  });

  const tabViews: TabView[] = tabs.map((tab) => ({
    key: tab.key,
    kind: tab.kind,
    title: tabTitles[tab.key],
    busy: tab.key === busyKey && !asking[tab.key],
    folder: tab.kind === "session" ? tab.folder : null,
  }));
  const k = (action: KeyAction) => keyName(prefs.keys[action]);
  const covered = overlay !== null;
  const homeName = homeFolder ? (workspaces?.find((each) => each.folder === homeFolder)?.name ?? homeFolder.split("/").pop() ?? "") : null;
  const sessionRunning = (folder: string) => tabs.some((tab) => tab.kind === "session" && tab.folder === folder && handles.current[tab.key]?.state().open);

  return (
    <div className="app" data-active={activeTab.kind} data-panel={panel ? "shown" : "hidden"}>
      <Tabs
        tabs={tabViews}
        active={active}
        plan={inSession ? panel : null}
        planKey={k("plan")}
        settingsKey={k("settings")}
        newKey={k("new-session")}
        canNew={(workspaces?.length ?? 0) > 0}
        onPick={(key) => (key === "home" && active === "home" ? goWorkspace(null) : activate(key))}
        onClose={(key) => void closeTab(key)}
        onNew={() => newSession()}
        onPlan={() => run("plan")}
        onSettings={() => openSettings()}
      />
      <div className="views" aria-hidden={covered ? true : undefined}>
        {activeTab.kind === "home" && api && (
          <div className="view" role="tabpanel" aria-labelledby={tabId("home")}>
            {homeFolder ? (
              <WorkspacePage
                key={homeFolder}
                api={api}
                folder={homeFolder}
                focus={!covered}
                running={sessionRunning(homeFolder)}
                refresh={materialChanged}
                onBack={() => goWorkspace(null)}
                onNewSession={(folder) => newSession(folder)}
                onOpenSession={(folder, session, title) => openSession(folder, session, title)}
                onHandedIn={(folder, words, title) => addSession({ folder, session: null, title, pending: { kind: "paper", shown: words.shown, sent: words.sent } })}
                onForget={(folder) =>
                  void api.forgetFolder(folder).then(
                    async () => {
                      await loadWorkspaces();
                      goWorkspace(null);
                    },
                    (problem) => note(String(problem)),
                  )
                }
                onPaste={(folder) => setOverlay({ kind: "paste", folder })}
                onOpenMaterial={(folder, path) => setOverlay({ kind: "source", folder, source: { path, page: null } })}
                onNote={note}
              />
            ) : (
              <HomePage
                api={api}
                workspaces={workspaces}
                step={step_}
                focus={!covered && restored}
                notes={notes}
                onContinue={(folder, session, title) => openSession(folder, session, title)}
                onStartIn={(folder) => newSession(folder, { kind: "start" })}
                onWorkspace={(folder) => goWorkspace(folder)}
                onPickFolder={() => void pickFolder()}
                onNewFolder={() => void newFolder()}
                onFindFolders={() => void findFolders()}
              />
            )}
            {homeFolder && notes.length > 0 && (
              <div className="page-notes notices">
                {notes.map((text) => (
                  <p key={text} className="record note">
                    {text}{" "}
                    <button className="plain" onClick={() => setNotes((old) => old.filter((n) => n !== text))}>
                      Dismiss
                    </button>
                  </p>
                ))}
              </div>
            )}
          </div>
        )}
        {activeTab.kind === "settings" && (
          <div className="view" role="tabpanel" aria-labelledby={tabId("settings")}>
            <SettingsPage
              prefs={prefs}
              change={changePrefs}
              setup={setup}
              engine={engine}
              about={about}
              lessonOpen={lessonOpen()}
              blockMinutes={blockMinutes}
              section={settingsSection}
              onEngine={(chosen) => void chooseEngine(chosen)}
              onPayment={(payment) => void choosePayment(payment)}
              onBlock={chooseBlock}
              onSignIn={() => void signIn()}
              onCheckAgain={() => void checkAgain()}
              onReplaceRuntime={() => void answerRuntime((api) => api.replaceRuntime())}
            />
          </div>
        )}
        {api &&
          tabs.map((tab) =>
            tab.kind === "session" ? (
              <Session
                key={tab.key}
                tab={tab}
                active={tab.key === active}
                live={tab.key === liveKey}
                api={api}
                setup={setup}
                setupWait={setupWait.current}
                prefs={prefs}
                panel={panel}
                covered={covered}
                elsewhereBusy={busyKey !== null && busyKey !== tab.key}
                claim={claim}
                register={register}
                onSession={onSession}
                onTitle={onTitle}
                onBusy={onBusy}
                onAsking={onAsking}
                onEngine={onEngine}
                onSource={onSource}
                onTaskChange={onTaskChange}
                onStarted={onStarted}
              />
            ) : null,
          )}
      </div>
      {dragging && (inSession || (activeTab.kind === "home" && homeFolder)) && (
        <div className="veil" aria-hidden="true">
          <p>{inSession ? "Drop to send with your next message" : `Drop to add to ${displayName(homeName ?? "")}'s material`}</p>
        </div>
      )}
      {overlay?.kind === "commands" ? (
        <Commands
          workspaces={workspaces ?? []}
          keys={{ home: k("home"), newSession: k("new-session"), closeSession: k("close-session"), settings: k("settings"), plan: k("plan"), openWorkspace: k("open-workspace") }}
          inSession={inSession}
          planShown={panel}
          onClose={() => setOverlay(null)}
          run={(command, arg) => run(command, arg)}
        />
      ) : overlay?.kind === "source" ? (
        <SourceSheet source={overlay.source} folder={overlay.folder} onClose={() => setOverlay(null)} onNote={note} />
      ) : overlay?.kind === "paste" ? (
        <PasteSheet
          initial=""
          saving={false}
          onSave={(text) => {
            const folder = overlay.folder;
            setOverlay(null);
            void api?.addPastedText(text, folder).then(
              (added) => {
                added.lines.forEach(note);
                setMaterialChanged((n) => n + 1);
              },
              (problem) => note(String(problem)),
            );
          }}
          onClose={() => setOverlay(null)}
        />
      ) : null}
    </div>
  );
}

export { setupReady };
