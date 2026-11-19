import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";

import { core, isDemo } from "./bridge";
import { harnessOn, mark, ready, setProbe } from "./harness";
import { whenDrawn } from "./reveal";
import { HomePage, leadOf, mostRecent, programNote, readAhead, setupStep } from "./pages/Home";
import { checksAlone } from "./next";
import { SettingsPage } from "./pages/Settings";
import { TabPage } from "./pages/TabPage";
import { Tabs, tabId, type TabView } from "./pages/Tabs";
import { OPEN_WAIT_MS, readWorkspace, WorkspacePage, type Reading } from "./pages/Workspace";
import { wantGlass } from "./glass";
import { actionOf, applyPrefs, flushPlaces, keepPanelWidth, keyName, loadPrefs, panelWidthKept, savePrefs, SITTING_GAP_MS, type KeyAction, type Prefs } from "./prefs";
import { docsOf, fitPanelWidth, workspaceDocsKey } from "./docs";
import { Commands } from "./surface/Commands";
import { Session, setupReady, type AnswerPage, type Pending, type SessionHandle, type SessionTab } from "./surface/Session";
import { PasteSheet } from "./surface/Sheets";
import type { About, AgentEvent, Core, Engine, EngineChoice, MenuChoice, MenuSpec, Payment, Permissions, Setup, Workspace } from "./types";
import { displayName, withoutPaths } from "./words";

type HomeTab = { key: string; kind: "home"; folder: string | null };
type SettingsTab = { key: "settings"; kind: "settings" };
export type Tab = HomeTab | SessionTab | SettingsTab;

type Overlay = { kind: "commands" } | { kind: "paste"; folder: string } | null;

type Saved = {
  v: 1;
  tabs: ({ kind: "home"; folder: string | null } | { kind: "session"; key: string; folder: string; session: string | null; title: string } | { kind: "settings" })[];
  active: number;
  home: string | null;
  homes?: boolean;
  panel: boolean;
  seen_ms: number;
};

const HOME: HomeTab = { key: "home", kind: "home", folder: null };
const SETTINGS: SettingsTab = { key: "settings", kind: "settings" };

const NEW_TITLE = "New session";
const PAGES_QUIET_MS = 1500;

let counter = 0;
const newKey = () => `s${Date.now().toString(36)}${(counter++).toString(36)}`;

function insertTab(tabs: Tab[], tab: Tab): Tab[] {
  const settings = tabs.findIndex((each) => each.kind === "settings");
  const next = [...tabs];
  next.splice(settings === -1 ? next.length : settings, 0, tab);
  return next;
}

export type Actions = {
  run: (command: string, arg?: string) => void;
  continueRecent: () => void;
  newSession: (folder?: string, pending?: Pending) => void;
  openSession: (folder: string, session: string, title: string) => void;
  goWorkspace: (folder: string | null) => void;
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
  const [firsts, setFirsts] = useState<Record<string, { folder: string; first: Reading | Promise<Reading> }>>({});
  const [panel, setPanel] = useState(false);
  const [panelWanted, setPanelWanted] = useState(panelWidthKept);
  const [room, setRoom] = useState(() => window.innerWidth);
  useEffect(() => {
    const seen = () => setRoom(window.innerWidth);
    window.addEventListener("resize", seen);
    return () => window.removeEventListener("resize", seen);
  }, []);
  const panelWidth = fitPanelWidth(panelWanted, room);
  const [docsSeen, setDocsSeen] = useState(0);
  const onDocsChange = useCallback(() => setDocsSeen((n) => n + 1), []);
  const [overlay, setOverlay] = useState<Overlay>(null);
  const [liveKey, setLiveKey] = useState<string | null>(null);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [taskOpen, setTaskOpen] = useState<Record<string, boolean>>({});
  const [answerPages, setAnswerPages] = useState<Record<string, AnswerPage>>({});
  const [notes, setNotes] = useState<string[]>([]);
  const [signingIn, setSigningIn] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [restored, setRestored] = useState(false);
  const [homeShown, setHomeShown] = useState(false);
  const onHomeShown = useCallback(() => setHomeShown(true), []);
  const [settingsSection, setSettingsSection] = useState<{ name: string | null; asked: number }>({ name: null, asked: 0 });
  const [blockMinutes, setBlockMinutes] = useState(50);
  const [readAgain, setReadAgain] = useState(0);
  const [checksDue, setChecksDue] = useState<Record<string, number>>({});
  const onChecksDue = useCallback((folder: string, due: number) => setChecksDue((old) => (old[folder] === due ? old : { ...old, [folder]: due })), []);
  const handles = useRef<Record<string, SessionHandle>>({});
  const workspaceDocs = useRef<((command: string) => boolean) | null>(null);
  const registerWorkspacePanel = useCallback((run: ((command: string) => boolean) | null) => (workspaceDocs.current = run), []);
  const paperIn = useRef(new Set<string>());
  const onPaper = useCallback((folder: string, running: boolean) => {
    if (running) paperIn.current.add(folder);
    else paperIn.current.delete(folder);
  }, []);
  const setupWait = useRef<Promise<Setup> | null>(null);
  const pressed = useRef<Record<string, number>>({});

  const now = useRef({ api, setup, prefs, workspaces, tabs, active, panel, overlay, liveKey, busyKey });
  now.current = { api, setup, prefs, workspaces, tabs, active, panel, overlay, liveKey, busyKey };

  useLayoutEffect(() => {
    applyPrefs(prefs);
    wantGlass(prefs.glass);
  }, [prefs]);

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

  const lastHome = useRef("home");
  const activate = useCallback((key: string) => {
    setOverlay(null);
    setActive(key);
    const tab = now.current.tabs.find((each) => each.key === key);
    if (tab?.kind === "home") lastHome.current = key;
    if (tab?.kind === "session" && tab.session) void now.current.api?.touchSession(tab.folder, tab.session).catch(() => {});
  }, []);

  const opening = useRef<{ asked: number; key: string | null; folder: string | null } | null>(null);
  const asks = useRef(0);
  const justRead = useRef<string | null>(null);
  const navigate = useCallback(
    (key: string | null, folder: string | null) => {
      if (opening.current && opening.current.key === key && opening.current.folder === folder) return;
      const asked = ++asks.current;
      opening.current = null;
      const { api, active, workspaces, tabs } = now.current;
      const tab = key === null ? null : tabs.find((each): each is HomeTab => each.key === key && each.kind === "home");
      if (key !== null && !tab) return;
      const show = (first?: Reading | Promise<Reading>) => {
        if (key !== null && !now.current.tabs.some((each) => each.key === key)) return;
        const into = key ?? newKey();
        justRead.current = first !== undefined && now.current.active !== into ? into : null;
        setFirsts((old) => {
          const { [into]: _, ...rest } = old;
          return folder && first ? { ...rest, [into]: { folder, first } } : rest;
        });
        if (key === null) now.current.tabs = insertTab(now.current.tabs, { key: into, kind: "home", folder });
        setTabs((old) => (key === null ? insertTab(old, { key: into, kind: "home", folder }) : old.map((each) => (each.key === into && each.kind === "home" ? { ...each, folder } : each))));
        activate(into);
      };
      if (!api || (tab && folder === tab.folder)) return show();
      let read: Reading | null = null;
      let reading: Promise<Reading> | null = null;
      let ready: Promise<unknown> | null;
      if (folder) {
        reading = readWorkspace(api, folder, true).then((done) => (read = done));
        void reading.then(() => loadWorkspaces());
        ready = reading;
      } else {
        const lead = workspaces ? leadOf(workspaces) : null;
        ready = lead ? readAhead(api, lead.folder) : null;
      }
      if (!ready) return show();
      opening.current = { asked, key, folder };
      void Promise.race([ready, new Promise((done) => window.setTimeout(done, OPEN_WAIT_MS))]).then(() => {
        if (asked !== asks.current) return;
        opening.current = null;
        if (now.current.active === active) show(read ?? reading ?? undefined);
      });
    },
    [activate, loadWorkspaces],
  );

  const goWorkspace = useCallback(
    (folder: string | null) => {
      const { tabs, active } = now.current;
      const homes = tabs.filter((each): each is HomeTab => each.kind === "home");
      const open = folder === null ? undefined : homes.find((each) => each.folder === folder);
      if (open) return open.key === active ? undefined : activate(open.key);
      const front = homes.find((each) => each.key === active);
      if (front) return navigate(front.key, folder);
      const last = homes.find((each) => each.key === lastHome.current) ?? homes[homes.length - 1];
      navigate(last?.key ?? null, folder);
    },
    [activate, navigate],
  );

  const newTab = useCallback(() => {
    const atHome = now.current.tabs.find((each) => each.kind === "home" && each.folder === null);
    if (atHome) activate(atHome.key);
    else navigate(null, null);
  }, [activate, navigate]);

  const addSession = useCallback(
    (tab: Omit<SessionTab, "key" | "kind">) => {
      const key = newKey();
      setTabs((old) => insertTab(old, { key, kind: "session", ...tab }));
      setOverlay(null);
      setActive(key);
      return key;
    },
    [],
  );

  const folderInFront = (): string | null => {
    const { tabs, active, workspaces } = now.current;
    const tab = tabs.find((each) => each.key === active);
    if (tab?.kind === "session") return tab.folder;
    if (tab?.kind === "home" && tab.folder) return tab.folder;
    return workspaces?.[0]?.folder ?? null;
  };

  const newSession = useCallback(
    (folder?: string, pending?: Pending) => {
      const where = folder ?? folderInFront();
      if (!where) {
        goWorkspace(null);
        note("Open a space first.");
        return;
      }
      const waiting = now.current.tabs.find((each) => each.kind === "session" && each.folder === where && each.session === null && !each.pending && !handles.current[each.key]?.state().live);
      if (waiting) {
        activate(waiting.key);
        if (pending?.kind === "start") void handles.current[waiting.key]?.start();
        else if (pending?.kind === "checks") void handles.current[waiting.key]?.checks(pending.answer);
        return;
      }
      addSession({ folder: where, session: null, title: NEW_TITLE, ...(pending ? { pending } : {}) });
      void loadWorkspaces();
    },
    [activate, addSession, goWorkspace, note, loadWorkspaces],
  );

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

  const leaving = useRef(false);
  const closeWindow = useCallback(async () => {
    const { api, panel } = now.current;
    if (!api) return;
    leaving.current = true;
    const left: Saved = { v: 1, tabs: [], active: 0, home: null, homes: true, panel, seen_ms: Date.now() };
    if (!isDemo) await api.setWindowState(left).catch(() => {});
    await api.closeWindow().catch(() => {
      leaving.current = false;
    });
  }, []);

  const closeTab = useCallback(
    async (key: string) => {
      if (!now.current.tabs.some((each) => each.key === key)) return;
      await handles.current[key]?.close();
      if (now.current.liveKey === key) setLiveKey(null);
      if (now.current.busyKey === key) setBusyKey(null);
      const { tabs } = now.current;
      const at = tabs.findIndex((each) => each.key === key);
      if (at === -1) return;
      if (tabs.length === 1) return void closeWindow();
      now.current.tabs = tabs.filter((each) => each.key !== key);
      setTabs((old) => old.filter((each) => each.key !== key));
      setFirsts((old) => {
        if (!(key in old)) return old;
        const { [key]: _, ...rest } = old;
        return rest;
      });
      if (now.current.active === key) activate((tabs[at - 1] ?? tabs[at + 1]).key);
      void loadWorkspaces();
    },
    [activate, closeWindow, loadWorkspaces],
  );

  const openSettings = useCallback((section: string | null = null) => {
    setTabs((old) => (old.some((each) => each.kind === "settings") ? old : [...old, SETTINGS]));
    setSettingsSection((old) => ({ name: section, asked: old.asked + 1 }));
    setOverlay(null);
    setActive("settings");
  }, []);

  const step = (by: number) => {
    const { tabs, active } = now.current;
    const at = tabs.findIndex((each) => each.key === active);
    activate(tabs[(at + by + tabs.length) % tabs.length].key);
  };

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
      note(added === 0 ? "No other space was found on this Mac." : `${added} ${added === 1 ? "space" : "spaces"} added.`);
      await loadWorkspaces();
    } catch (problem) {
      note(String(problem));
    }
  };

  const justChecks = async (folder: string | null) => {
    const { api, workspaces } = now.current;
    if (!api || !folder) return note("Open a space first.");
    const home = await api.home(folder).catch(() => null);
    const due = checksAlone(home);
    onChecksDue(folder, due);
    const name = displayName(workspaces?.find((each) => each.folder === folder)?.name ?? folder.split("/").pop() ?? "");
    if (home?.sitting) return note(`A paper is running in ${name}. Hand it in or end it first.`);
    if (home && home.checks_alone == null) return note("The magnesium program on this Mac cannot give the quick checks alone. A newer one can.");
    if (due === 0) return note(`No quick check is due in ${name} now.`);
    newSession(folder, { kind: "checks" });
  };

  const run = useCallback(
    (command: string, arg?: string | null) => {
      const { tabs, active, prefs } = now.current;
      const tab = tabs.find((each) => each.key === active);
      const inFront = tab?.kind === "session" ? handles.current[tab.key] : null;
      switch (command) {
        case "home":
          return goWorkspace(null);
        case "new-tab":
          return newTab();
        case "go":
          return arg ? goWorkspace(arg) : undefined;
        case "open-workspace":
          return void pickFolder();
        case "new-session":
          return newSession(arg ?? undefined);
        case "just-checks":
          return void justChecks(arg ?? folderInFront());
        case "close-session":
          return tab ? void closeTab(tab.key) : undefined;
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
        case "look-back":
        case "hint":
        case "lost":
        case "unknown":
        case "symbols":
        case "model":
          if (inFront) inFront.command(command);
          else note("Open a session first.");
          return;
        case "expand-panel":
        case "flip-panel":
        case "close-document":
        case "open-document":
        case "zoom-in-document":
        case "zoom-out-document":
        case "zoom-fit-document":
        case "share-document":
          if (inFront) inFront.command(command);
          else if (tab?.kind === "home") workspaceDocs.current?.(command);
          return;
      }
    },
    [activate, changePrefs, closeTab, goWorkspace, newSession, newTab, note, openSettings],
  );
  const runRef = useRef(run);
  runRef.current = run;

  useEffect(() => {
    let alive = true;
    const stops: (() => void)[] = [];
    void (async () => {
      mark("page");
      const api = await core();
      mark("core");
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
      const library = new Set(found.map((each) => each.folder));
      const saved = (kept ?? null) as Saved | null;
      const recently = saved ? Date.now() - (saved.seen_ms ?? 0) < SITTING_GAP_MS : false;
      const page = (folder: string | null | undefined) => (recently && folder && library.has(folder) ? folder : null);
      let back: Tab[] = [];
      let left: Tab | undefined;
      const addHome = (folder: string | null) => {
        if (!recently && back.some((each) => each.kind === "home")) return;
        back.push({ key: back.some((each) => each.key === "home") ? newKey() : "home", kind: "home", folder });
      };
      if (saved?.v === 1 && Array.isArray(saved.tabs)) {
        const list: Saved["tabs"] = saved.homes ? saved.tabs : [{ kind: "home", folder: saved.home }, ...saved.tabs];
        list.forEach((each, at) => {
          if (each.kind === "settings") back.push(SETTINGS);
          else if (each.kind === "home") addHome(page(each.folder));
          else if (each.kind === "session" && library.has(each.folder) && each.session)
            back.push({ key: each.key || newKey(), kind: "session", folder: each.folder, session: each.session, title: each.title ?? "" });
          if (at <= (saved.active ?? 0)) left = back[back.length - 1];
        });
        setPanel(Boolean(saved.panel));
      }
      let wanted = saved && recently ? (left ?? back[0]) : undefined;
      if (initial) {
        await api.visitFolder(initial).catch(() => null);
        const home = back.find((each) => each.kind === "home" && each.folder === initial) ?? back.find((each) => each.kind === "home");
        if (home?.kind === "home") back = back.map((each) => (each === home ? { ...home, folder: initial } : each));
        else back.unshift({ key: "home", kind: "home", folder: initial });
        wanted = back.find((each) => each.kind === "home" && each.folder === initial);
        void loadWorkspaces();
      } else if (!wanted) {
        const home = back.find((each) => each.kind === "home" && each.folder === null);
        if (!home) back.unshift({ key: back.some((each) => each.key === "home") ? newKey() : "home", kind: "home", folder: null });
        wanted = home ?? back[0];
      }
      now.current.tabs = back;
      setTabs(back);
      setWorkspaces(found);
      now.current.workspaces = found;
      if (wanted?.kind === "home") lastHome.current = wanted.key;
      setActive(wanted?.key ?? back[0].key);
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
  }, []);

  const shown = useRef(false);
  const [firstScreen, setFirstScreen] = useState(false);
  useEffect(() => {
    const front = tabs.find((each) => each.key === active);
    const homeWaits = front?.kind === "home" && front.folder === null && !homeShown;
    if (!restored || workspaces === null || homeWaits || shown.current) return;
    shown.current = true;
    void whenDrawn().then(() => {
      ready();
      void now.current.api?.showWindow().catch(() => {});
      setFirstScreen(true);
    });
  }, [restored, workspaces, active, tabs, homeShown]);

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

  const choosePermissions = async (permissions: Permissions) => {
    const { api, setup } = now.current;
    if (!api || !setup) return;
    setSetup({ ...setup, permissions });
    await api.setPermissions(permissions).catch(() => undefined);
  };

  const lessonOpen = () => Object.values(handles.current).some((handle) => handle.state().open);

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

  useEffect(() => {
    if (active !== "settings" || !api || !workspaces?.[0]) return;
    void api.home(workspaces[0].folder).then((home) => setBlockMinutes(home.block_minutes), () => {});
  }, [active, api, workspaces]);

  const pagesTimer = useRef<number | null>(null);
  const pagesFolder = useRef<string | null | undefined>(undefined);
  const readPagesAgain = useCallback(
    (folder: string | null = null) => {
      if (pagesTimer.current !== null) window.clearTimeout(pagesTimer.current);
      pagesTimer.current = null;
      pagesFolder.current = undefined;
      void loadWorkspaces();
      if (folder === null || now.current.tabs.some((each) => each.kind === "home" && each.folder === folder)) setReadAgain((n) => n + 1);
    },
    [loadWorkspaces],
  );
  const readPagesSoon = useCallback(
    (folder: string | null) => {
      if (pagesTimer.current !== null) window.clearTimeout(pagesTimer.current);
      const waiting = pagesFolder.current === undefined || pagesFolder.current === folder ? folder : null;
      pagesFolder.current = waiting;
      pagesTimer.current = window.setTimeout(() => readPagesAgain(waiting), PAGES_QUIET_MS);
    },
    [readPagesAgain],
  );

  const lastActive = useRef(active);
  useEffect(() => {
    const was = lastActive.current;
    lastActive.current = active;
    if (was === active || now.current.tabs.find((each) => each.key === active)?.kind !== "home") return;
    if (justRead.current === active) justRead.current = null;
    else readPagesAgain();
  }, [active, readPagesAgain]);
  useEffect(() => () => {
    if (pagesTimer.current !== null) window.clearTimeout(pagesTimer.current);
  }, []);

  const onEventRef = useRef<(event: AgentEvent) => void>(() => {});
  onEventRef.current = (event: AgentEvent) => {
    const key = now.current.liveKey;
    if (key) handles.current[key]?.event(event);
    if (["turn_ended", "marked", "plan_changed", "closing", "exited"].includes(event.type)) {
      const tab = now.current.tabs.find((each) => each.key === key);
      readPagesSoon(tab?.kind === "session" ? tab.folder : null);
    }
    if (event.type === "notice" && event.kind === "sign_in") {
      const { api } = now.current;
      void api?.account().then((account) => {
        const { setup } = now.current;
        if (setup) setSetup({ ...setup, account });
      }, () => {});
    }
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
  const onSessionRenamed = useCallback((folder: string, session: string, title: string) => {
    setTabs((old) => old.map((each) => (each.kind === "session" && each.folder === folder && each.session === session ? { ...each, title } : each)));
  }, []);
  const onBusy = useCallback((key: string, busy: boolean) => setBusyKey((old) => (busy ? key : old === key ? null : old)), []);
  const [asking, setAsking] = useState<Record<string, boolean>>({});
  const onAsking = useCallback((key: string, now: boolean) => setAsking((old) => (Boolean(old[key]) === now ? old : { ...old, [key]: now })), []);
  const onStarted = useCallback((key: string) => {
    setTabs((old) => old.map((each) => (each.key === key && each.kind === "session" && each.pending ? { ...each, pending: undefined } : each)));
  }, []);
  const onTaskChange = useCallback((key: string, open: boolean) => setTaskOpen((old) => (old[key] === open ? old : { ...old, [key]: open })), []);
  const onAnswerPage = useCallback((key: string, page: AnswerPage) => setAnswerPages((old) => (old[key] === page ? old : { ...old, [key]: page })), []);
  const onPanelWidth = useCallback((width: number) => {
    const fitted = fitPanelWidth(width);
    setPanelWanted(fitted);
    keepPanelWidth(fitted);
  }, []);
  const onShowPanel = useCallback(() => {
    setPanel(true);
    widenPanel();
  }, []);
  const widenPanel = useCallback(() => {
    setPanelWanted((old) => {
      if (fitPanelWidth(old) >= window.innerWidth * 0.5) return old;
      const wide = fitPanelWidth(window.innerWidth * 0.5);
      keepPanelWidth(wide);
      return wide;
    });
  }, []);
  const onEngine = useCallback(() => void loadEngine(), [loadEngine]);
  const onSignIn = useCallback(() => {
    void now.current.api?.signIn().then(
      () => setSigningIn(true),
      (problem) => note(String(problem)),
    );
  }, [note]);

  const onMenuRef = useRef<(choice: MenuChoice) => void>(() => {});
  onMenuRef.current = (choice: MenuChoice) => {
    if (Date.now() - (pressed.current[choice.command] ?? 0) < 400) return;
    runRef.current(choice.command, choice.arg);
  };

  const activeTab = tabs.find((each) => each.key === active) ?? tabs[0] ?? HOME;
  const inSession = activeTab.kind === "session";
  const pageFolder = activeTab.kind === "home" ? activeTab.folder : null;
  const opened = useRef(new Set<string>()).current;
  if (activeTab.kind !== "session") opened.add(activeTab.key);
  if (firstScreen) for (const tab of tabs) if (tab.kind === "home") opened.add(tab.key);
  for (const key of opened) if (!tabs.some((tab) => tab.key === key)) opened.delete(key);
  const folderName = useCallback((folder: string) => displayName(workspaces?.find((each) => each.folder === folder)?.name ?? folder.split("/").pop() ?? ""), [workspaces]);
  const tabTitles = useMemo(() => {
    const seen: Record<string, number> = {};
    const named: Record<string, string> = {};
    for (const tab of tabs) {
      const title = tab.kind === "home" ? (tab.folder ? folderName(tab.folder) : "Home") : tab.kind === "settings" ? "Settings" : tab.title || NEW_TITLE;
      seen[title] = (seen[title] ?? 0) + 1;
      named[tab.key] = seen[title] > 1 ? `${title} (${seen[title]})` : title;
    }
    return named;
  }, [tabs, folderName]);
  const spec = useMemo((): MenuSpec => {
    const disabled: string[] = [];
    if (!inSession) disabled.push("attach", "symbols", "model", "hint", "lost", "unknown");
    else if (!taskOpen[activeTab.key]) disabled.push("hint", "lost", "unknown");
    const page = inSession ? (answerPages[activeTab.key] ?? null) : null;
    if (!page) disabled.push("look-back");
    if (!inSession) disabled.push("plan");
    const docs = inSession ? (handles.current[activeTab.key]?.docs() ?? null) : pageFolder ? docsOf(workspaceDocsKey(pageFolder)) : null;
    const withDoc = docs !== null && docs.shown !== null && (inSession ? panel : true) && !(docs.expanded && docs.lessonFront);
    if (!inSession && !(docs && docs.open.length > 0)) disabled.push("expand-panel", "flip-panel");
    else if (!(docs && docs.shown !== null) && !docs?.expanded) disabled.push("flip-panel");
    if (!withDoc) disabled.push("close-document", "share-document");
    if (!withDoc || !/\.pdf$/i.test(docs?.shown ?? "")) disabled.push("zoom-in-document", "zoom-out-document", "zoom-fit-document");
    if (!inSession && !pageFolder) disabled.push("open-document");
    if (busyKey === null) disabled.push("stop-reply");
    const front = inSession ? activeTab.folder : (pageFolder ?? (workspaces ? (mostRecent(workspaces) ?? workspaces[0])?.folder : undefined));
    if (!front || checksDue[front] === 0) disabled.push("just-checks");
    return {
      keys: Object.fromEntries(Object.entries(prefs.keys)),
      workspaces: (workspaces ?? []).map((each) => ({ id: each.folder, title: displayName(each.name) })),
      tabs: tabs.map((tab) => ({ id: tab.key, title: tabTitles[tab.key] })),
      disabled,
      titles: {
        plan: panel ? "Hide Plan" : "Show Plan",
        "expand-panel": docs?.expanded && !docs.lessonFront ? (inSession ? "Put the Panel Beside the Lesson" : "Put the Panel Beside the Page") : "Expand Panel",
        "flip-panel":
          docs?.expanded && !docs.lessonFront && !(page === "folded" && handles.current[activeTab.key]?.docClosed())
            ? inSession
              ? "Switch to the Lesson"
              : "Switch to the Page"
            : "Switch to the Document",
        "look-back": page === "open" ? "Hide the Lesson" : "Show the Lesson",
        ...(activeTab.kind === "settings" ? { "close-session": "Close Settings" } : activeTab.kind === "home" ? { "close-session": "Close Tab" } : {}),
      },
    };
  }, [prefs.keys, workspaces, tabs, tabTitles, inSession, activeTab, taskOpen, answerPages, busyKey, panel, pageFolder, docsSeen, checksDue]);
  useEffect(() => {
    if (!api || isDemo) return;
    const timer = window.setTimeout(() => void api.setMenu(spec).catch(() => {}), 80);
    return () => window.clearTimeout(timer);
  }, [api, spec]);

  useEffect(() => {
    if (!api || !restored || isDemo) return;
    const timer = window.setTimeout(() => {
      if (leaving.current) return;
      const saved: Saved = {
        v: 1,
        tabs: tabs.map((tab) =>
          tab.kind === "settings"
            ? { kind: "settings" as const }
            : tab.kind === "home"
              ? { kind: "home" as const, folder: tab.folder }
              : { kind: "session" as const, key: tab.key, folder: tab.folder, session: tab.session, title: tab.title },
        ),
        active: Math.max(0, tabs.findIndex((tab) => tab.key === active)),
        home: tabs.find((tab): tab is HomeTab => tab.kind === "home")?.folder ?? null,
        homes: true,
        panel,
        seen_ms: Date.now(),
      };
      void api.setWindowState(saved).catch(() => {});
    }, 300);
    return () => window.clearTimeout(timer);
  }, [api, restored, tabs, active, panel]);

  useEffect(() => {
    const save = () => flushPlaces();
    window.addEventListener("pagehide", save);
    return () => window.removeEventListener("pagehide", save);
  }, []);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;
      if (event.isComposing || event.keyCode === 229) return;
      const { prefs, overlay, tabs, active } = now.current;
      const action: KeyAction | null = actionOf(event, prefs.keys);
      if (action) {
        if (overlay && action !== "commands") return;
        if (action === "look-back") {
          const tab = tabs.find((each) => each.key === active);
          if (tab?.kind !== "session" || handles.current[tab.key]?.answerPage() == null) return;
        }
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
        else if (tab?.kind === "home" && tab.folder) {
          event.preventDefault();
          if (!workspaceDocs.current?.("escape")) goWorkspace(null);
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

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

  const dropRef = useRef<(paths: string[]) => void>(() => {});
  dropRef.current = (paths: string[]) => {
    const { api, tabs, active } = now.current;
    const tab = tabs.find((each) => each.key === active);
    const homeFolder = tab?.kind === "home" ? tab.folder : null;
    if (!api) return;
    if (tab?.kind === "session") handles.current[tab.key]?.drop(paths);
    else if (homeFolder && paperIn.current.has(homeFolder)) {
      note("While a paper runs, nothing is added to the material. Your working goes in when you hand the paper in.");
    } else if (homeFolder) {
      void api.addMaterial(paths, homeFolder).then(
        (added) => {
          added.lines.forEach(note);
          setReadAgain((n) => n + 1);
        },
        (problem) => note(String(problem)),
      );
    } else note("Open a space, then drop its files on its page.");
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
    checkAgain: () => void checkAgain(),
    openUrl: (url) => void api?.openUrl(url),
    signIn: () => void signIn(),
  });
  const note_ = programNote(setup, {
    replace: () => void answerRuntime((api) => api.replaceRuntime()),
    keep: () => void answerRuntime((api) => api.keepRuntime()),
  });

  const commandKeys = new Set(Object.values(prefs.keys));
  const tabViews: TabView[] = tabs.map((tab, at) => ({
    keyName: at < 9 && !commandKeys.has(`cmd+${at + 1}`) ? `⌘${at + 1}` : tab.kind === "home" && tab.folder === null ? keyName(prefs.keys.home) : "",
    key: tab.key,
    kind: tab.kind,
    title: tabTitles[tab.key],
    busy: tab.key === busyKey && !asking[tab.key],
    asking: Boolean(asking[tab.key]),
    folder: tab.kind === "session" ? tab.folder : null,
  }));
  const k = (action: KeyAction) => keyName(prefs.keys[action]);
  void docsSeen;
  const frontDocs = activeTab.kind === "session" ? (panel ? (handles.current[activeTab.key]?.docs() ?? null) : null) : pageFolder ? docsOf(workspaceDocsKey(pageFolder)) : null;
  const flipState: string | null =
    frontDocs && frontDocs.expanded && (activeTab.kind === "session" || frontDocs.open.length > 0)
      ? frontDocs.lessonFront || (activeTab.kind === "session" && answerPages[activeTab.key] === "folded" && handles.current[activeTab.key]?.docClosed())
        ? "Document"
        : activeTab.kind === "session"
          ? "Lesson"
          : "Page"
      : null;
  const covered = overlay !== null;
  const sessionRunning = (folder: string) => tabs.some((tab) => tab.kind === "session" && tab.folder === folder && handles.current[tab.key]?.state().open);

  return (
    <div className="app" data-active={activeTab.kind} data-panel={panel ? "shown" : "hidden"}>
      <Tabs
        tabs={tabViews}
        active={active}
        plan={inSession ? panel : null}
        planKey={k("plan")}
        flip={flipState}
        flipKey={k("flip-panel")}
        onFlip={() => run("flip-panel")}
        settingsKey={k("settings")}
        newKey={k("new-tab")}
        onPick={(key) => (key === active && activeTab.kind === "home" && activeTab.folder ? navigate(key, null) : activate(key))}
        onClose={(key) => void closeTab(key)}
        onNew={newTab}
        onPlan={() => run("plan")}
        onSettings={() => openSettings()}
      />
      <div className="views" aria-hidden={covered ? true : undefined}>
        {api &&
          tabs.map((tab) => {
            if (tab.kind !== "home" || !opened.has(tab.key)) return null;
            const front = tab.key === active;
            const first = firsts[tab.key];
            return (
          <TabPage key={tab.key} front={front} labelledBy={tabId(tab.key)}>
            {tab.folder ? (
              <WorkspacePage
                key={tab.folder}
                api={api}
                folder={tab.folder}
                first={first?.folder === tab.folder ? first.first : undefined}
                front={front}
                focus={!covered && front}
                running={sessionRunning(tab.folder)}
                refresh={readAgain}
                onBack={() => goWorkspace(null)}
                onNewSession={(folder) => newSession(folder)}
                onStartSession={(folder) => newSession(folder, { kind: "start" })}
                onChecks={(folder) => newSession(folder, { kind: "checks" })}
                onChecksDue={onChecksDue}
                checksKey={k("just-checks")}
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
                panelWidth={panelWidth}
                onPanelWidth={onPanelWidth}
                onShowPanel={widenPanel}
                onSessionRenamed={onSessionRenamed}
                onDocsChange={onDocsChange}
                registerPanel={registerWorkspacePanel}
                onPaper={onPaper}
                onNote={note}
              />
            ) : (
              <HomePage
                api={api}
                workspaces={workspaces}
                step={step_}
                note={note_}
                focus={!covered && restored && front}
                notes={notes}
                front={front}
                onContinue={(folder, session, title) => openSession(folder, session, title)}
                onStartIn={(folder) => newSession(folder, { kind: "start" })}
                onNewSession={(folder) => newSession(folder)}
                onChecks={(folder, answer) => newSession(folder, { kind: "checks", ...(answer ? { answer } : {}) })}
                onChecksDue={onChecksDue}
                checksKey={k("just-checks")}
                onWorkspace={(folder) => goWorkspace(folder)}
                onPickFolder={() => void pickFolder()}
                onNewFolder={() => void newFolder()}
                onFindFolders={() => void findFolders()}
                onShown={onHomeShown}
              />
            )}
            {tab.folder && notes.length > 0 && (
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
          </TabPage>
            );
          })}
        {tabs.some((tab) => tab.kind === "settings") && opened.has(SETTINGS.key) && (
          <TabPage front={activeTab.kind === "settings"} labelledBy={tabId("settings")}>
            <SettingsPage
              prefs={prefs}
              change={changePrefs}
              setup={setup}
              engine={engine}
              about={about}
              lessonOpen={lessonOpen()}
              blockMinutes={blockMinutes}
              section={settingsSection.name}
              sectionAsked={settingsSection.asked}
              onEngine={(chosen) => void chooseEngine(chosen)}
              onPayment={(payment) => void choosePayment(payment)}
              onPermissions={(permissions) => void choosePermissions(permissions)}
              onBlock={chooseBlock}
              onSignIn={() => void signIn()}
              onCheckAgain={() => void checkAgain()}
              onReplaceRuntime={() => void answerRuntime((api) => api.replaceRuntime())}
            />
          </TabPage>
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
                onSignIn={onSignIn}
                panelWidth={panelWidth}
                onPanelWidth={onPanelWidth}
                onShowPanel={onShowPanel}
                onDocsChange={onDocsChange}
                onTaskChange={onTaskChange}
                onAnswerPage={onAnswerPage}
                onStarted={onStarted}
              />
            ) : null,
          )}
      </div>
      {dragging && (inSession || pageFolder) && (
        <div className="veil" aria-hidden="true">
          <p>{inSession ? "Drop to send with your next message" : `Drop to add to ${pageFolder ? folderName(pageFolder) : ""}'s material`}</p>
        </div>
      )}
      {overlay?.kind === "commands" ? (
        <Commands
          workspaces={workspaces ?? []}
          keys={{ home: k("home"), newTab: k("new-tab"), newSession: k("new-session"), closeSession: k("close-session"), settings: k("settings"), plan: k("plan"), openWorkspace: k("open-workspace"), model: k("model") }}
          inSession={inSession}
          planShown={panel}
          onClose={() => setOverlay(null)}
          run={(command, arg) => run(command, arg)}
        />
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
                setReadAgain((n) => n + 1);
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
