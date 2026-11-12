// A stand-in core for a plain browser, with made-up data, so the workspace
// can be worked on and photographed without a window or an agent. Nothing
// here is a real course, a real grade, a real institution or a real
// learner's words.
//
// The address picks a state (see STATES below and scripts/shots.mjs): the
// demo seeds what the window keeps on this Mac before the page opens the
// folder, then drives the page through `startFromAddress`.

import type { Actions } from "./App";
import { demoAfterTurn, demoBaseReply, demoMarked, demoMayMark, demoPlan, demoResetPlan } from "./demoBase";
import { tinyPdf } from "./demoPdf";
import { exKey, type Meta } from "./page";
import type {
  Account,
  AgentEvent,
  Card,
  Core,
  Engine,
  Home,
  LessonInfo,
  Mark,
  Paper,
  PaperSitting,
  Payment,
  Plan,
  Progress,
  RuntimeSetup,
  Setup,
  Source,
  StudyFiles,
  Turn,
} from "./types";

const wait = (ms: number) => new Promise((done) => window.setTimeout(done, ms));
const address = () => location.hash.replace(/^#/, "");

const ROOT = "/Users/you/Study";
const FOLDER = `${ROOT}/Biochemistry 210`;
const FOLDERS = [FOLDER, `${ROOT}/Statistics 201`, `${ROOT}/Linear Algebra`];

const DAY = 24 * 3600 * 1000;
/** A time `days` ago, at a given hour and minute. */
function ago(days: number, hour: number, minute: number): number {
  const date = new Date(Date.now() - days * DAY);
  date.setHours(hour, minute, 0, 0);
  return date.getTime();
}
const stamp = (ms: number) => {
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
};

// --- What the address asks for ------------------------------------------------

const state = address();
const away = state.startsWith("s5") ? 7 : 2;
const firstOpen = state === "s1-cold-first";
const signedOut = state === "setup-signed-out";
const noClaude = state === "setup-no-claude";
const replaceRuntime = state === "setup-replace";
const upgrade = state === "upgrade";
const newFolder = state === "new-lesson";

// --- The lesson that was stopped ------------------------------------------------

const GOAL = "Reading $V_{max}$ and $K_m$ from a Lineweaver–Burk plot";
const TASK_VMAX = "**Task.** The line meets the y-axis at $0.25$ s/µM. What is $V_{max}$, with its units?";
const TASK_KM = "**Task.** The same line crosses the x-axis at $-0.5$ µM⁻¹. What is $K_m$?";

const START_TURN_MS = ago(away, 16, 12);
const STOP_TURN_MS = ago(away, 16, 40);

const earlierTurns: Turn[] = [
  {
    session: "demo-current",
    ms: START_TURN_MS,
    learner: `Let's work on ${GOAL}.`,
    tutor:
      "A Lineweaver–Burk plot turns the Michaelis–Menten curve into a straight line, so you can read both constants off it with a ruler.\n\n" +
      "Start from the rate equation:\n\n$$v = \\frac{V_{max}[S]}{K_m + [S]}$$\n\n" +
      "**Task.** Take the reciprocal of both sides and write $\\frac{1}{v}$ as a line in $\\frac{1}{[S]}$. What do you get?",
    interrupted: false,
  },
  {
    session: "demo-current",
    ms: STOP_TURN_MS,
    learner: "$\\frac{1}{v} = \\frac{K_m}{V_{max}} \\cdot \\frac{1}{[S]} + \\frac{1}{V_{max}}$",
    tutor:
      "That is the line: the slope is $K_m/V_{max}$ and the y-intercept is $1/V_{max}$. Everything else on the plot follows from those two.\n\n" +
      "The y-intercept is where $\\frac{1}{[S]} = 0$, an infinitely high substrate concentration, which is exactly when the enzyme runs at $V_{max}$.\n\n" +
      TASK_VMAX,
    interrupted: false,
  },
];

const RECIPROCAL_MARK: Mark = { id: "demo-1", at: stamp(STOP_TURN_MS - 60_000), label: "taking reciprocals of the rate equation", mark: "right", help: "none", cold: true, paper: null };

const records: Mark[] = [
  { id: "demo-0a", at: stamp(ago(7, 18, 20)), label: "units of a rate", mark: "right", help: "none", cold: true, paper: null },
  { id: "demo-0b", at: stamp(ago(7, 18, 26)), label: "slope and $K_m$", mark: "wrong", help: "none", cold: true, paper: null },
  { id: "demo-0c", at: stamp(ago(5, 9, 4)), label: "slope and $K_m$", mark: "right", help: "hint", cold: false, paper: null },
  RECIPROCAL_MARK,
];

// --- What the window keeps on this Mac, seeded before the page opens the folder ----

function seed(): void {
  try {
    if (firstOpen) return;
    const layout = {
      panes: [{ tabs: ["lesson", "plan"], active: "lesson" }],
      focus: 0,
      sidebar: state === "sidebar",
      scroll: {},
      pages: {},
      drafts: { answer: state.startsWith("s1") || state === "s2-start" || state.startsWith("s5") ? "$V_{max} = 1/0.25 =$" : "" },
      folds: {},
    };
    if (!newFolder) localStorage.setItem(`mg.ws:${FOLDER}`, JSON.stringify(layout));
    const kept: Record<string, Meta> = {
      [exKey(earlierTurns[0].learner, earlierTurns[0].tutor)]: { said: "opening", marks: [] },
      [exKey(earlierTurns[1].learner, earlierTurns[1].tutor)]: { said: "answer", marks: [RECIPROCAL_MARK] },
    };
    localStorage.setItem(`mg.ex:${FOLDER}`, JSON.stringify(kept));
    localStorage.removeItem(`mg.planBase:${FOLDER}`);
    if (state === "settings") localStorage.removeItem("mg.settings");
  } catch {
    // A page with no storage shows the defaults.
  }
}

// --- The plan, as the file holds it ---------------------------------------------

const dayName = (iso: string | null) => {
  if (!iso) return "";
  return new Date(`${iso}T12:00:00`).toLocaleDateString("en-GB", { weekday: "short", day: "numeric", month: "short" });
};

let changes: string[] = [`${stamp(Date.now() - 6 * DAY).slice(0, 10)}: plan built from the diagnostic paper and the week 4 to 6 slides.`];

function planText(plan: Plan): string {
  const rows = plan.items.map((item, n) => `| ${n + 1} | ${item.title} | ${item.note ?? ""} | ${item.status} | ${dayName(item.day)} |`).join("\n");
  return [
    `# ${plan.title}: study plan`,
    "",
    "## 1. Facts",
    "",
    "| Item | Value |",
    "|---|---|",
    `| Target | ${plan.title} |`,
    `| When | ${plan.exam_date} (${dayName(plan.exam_date).split(" ")[0]}) 10:00 to 11:30 |`,
    "| Length | 90 |",
    "| Aids | a calculator, no notes |",
    "| Coverage | Weeks 4 to 6: enzyme kinetics, inhibition, the linear plots |",
    "",
    "## 2. The instructor's words",
    "",
    '- "Expect one question where you read the constants off a plot." (lecture 11)',
    "",
    "## 5. Method",
    "",
    "1. Sit a paper timed, with the permitted aids only.",
    "2. Mark it straight after, and log what lost marks.",
    "3. Redo each gap cold, on three different days.",
    "",
    "## 6. Blocks",
    "",
    "| # | Block | Purpose | Status | Day |",
    "|---|---|---|---|---|",
    rows,
    "",
    "Never drop: Paper B, and every cold redo.",
    "",
    "## 7. Sessions",
    "",
    "- If it is Monday to Friday at 08:30, then I do the due redos at the library before class.",
    "",
    "## 14. Changes",
    "",
    ...changes.map((line) => `- ${line}`),
    "",
  ].join("\n");
}

let planMs = Date.now() - 6 * DAY;
function plan(): Plan {
  const base = demoPlan();
  // As the tutor is told to: after a reorder the days are re-spaced, so they still run forward.
  const days = base.items.map((item) => item.day).filter((day): day is string => day !== null).sort();
  const items = base.items.map((item) => (item.day === null ? item : { ...item, day: days.shift() ?? item.day }));
  const shaped: Plan = { ...base, items, goal: GOAL, changed_ms: planMs };
  return { ...shaped, text: planText(shaped) };
}

// --- Files under Study/ ---------------------------------------------------------

const FILES: StudyFiles = {
  files: [
    { path: "Study/midterm-1/plan.md", changed_ms: Date.now() - 6 * DAY },
    { path: "Study/materials/Week 5 slides.pdf", changed_ms: Date.now() - 8 * DAY },
    { path: "Study/materials/lineweaver-burk.md", changed_ms: Date.now() - 9 * DAY },
    { path: "Study/materials/michaelis-menten-derivation.md", changed_ms: Date.now() - 10 * DAY },
    { path: "Study/materials/inhibition-summary.md", changed_ms: Date.now() - 11 * DAY },
    { path: "Study/midterm-1/papers/Paper B.pdf", changed_ms: Date.now() - 5 * DAY },
    { path: "Study/midterm-1/gap-log.md", changed_ms: Date.now() - 6 * DAY },
    { path: "Study/handed-in/Diagnostic/IMG_0412.jpg", changed_ms: Date.now() - 6 * DAY },
  ],
  keys: 1,
  more: false,
};

const TEXTS: Record<string, string> = {
  "Study/materials/lineweaver-burk.md":
    "# The Lineweaver–Burk plot\n\nPlot $1/v$ against $1/[S]$. The rate equation becomes a line:\n\n$$\\frac{1}{v} = \\frac{K_m}{V_{max}} \\cdot \\frac{1}{[S]} + \\frac{1}{V_{max}}$$\n\n" +
    "- The **y-intercept** is $1/V_{max}$.\n- The **x-intercept** is $-1/K_m$.\n- The **slope** is $K_m/V_{max}$.\n\n" +
    "## Why it misleads\n\nTaking reciprocals stretches the points measured at low $[S]$, which carry the largest errors, across most of the plot. A fit by eye then leans on the least reliable data.\n",
  "Study/materials/michaelis-menten-derivation.md":
    "# Deriving Michaelis–Menten\n\nAssume the enzyme–substrate complex is at steady state: it forms as fast as it breaks down.\n\n$$k_1[E][S] = (k_{-1} + k_2)[ES]$$\n\nWith $[E]_T = [E] + [ES]$ and $K_m = (k_{-1} + k_2)/k_1$, the rate $v = k_2[ES]$ becomes\n\n$$v = \\frac{V_{max}[S]}{K_m + [S]}$$\n",
  "Study/materials/inhibition-summary.md": "# Inhibition at a glance\n\n| Kind | $V_{max}$ | $K_m$ |\n|---|---|---|\n| Competitive | same | larger |\n| Uncompetitive | smaller | smaller |\n| Mixed | smaller | either |\n",
  "Study/midterm-1/gap-log.md": "# Midterm 1: gap log\n\n| Paper | Question | Tag | What went wrong |\n|---|---|---|---|\n| Diagnostic | Q4 | slope and $K_m$ | read the slope as $K_m$ |\n",
};

const PHOTO =
  "data:image/svg+xml;utf8," +
  encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" width="480" height="360"><rect width="480" height="360" fill="#f4f2ec"/><g stroke="#3a4a6b" stroke-width="2" fill="none"><path d="M60 300 L60 60 M60 300 L420 300"/><path d="M60 250 L400 90"/></g><text x="70" y="40" font-family="sans-serif" font-size="18" fill="#3a4a6b">1/v against 1/[S], my working</text></svg>',
  );

function source(path: string): Source {
  if (/key|solution|answer/i.test(path)) throw new Error("That looks like an answer key, so the window leaves it closed.");
  if (path.startsWith("/") || path.includes("..")) throw new Error("That file is outside the study folder.");
  const name = path.split("/").pop() ?? path;
  if (path.endsWith(".pdf")) return { name, kind: "pdf", text: null, truncated: false, image: null };
  if (/\.(jpe?g|png)$/i.test(path)) return { name, kind: "image", text: null, truncated: false, image: PHOTO };
  const text = TEXTS[path] ?? (path.endsWith("plan.md") ? (plan().text ?? "") : null);
  if (text === null) throw new Error("That file does not exist.");
  return { name, kind: "text", text, truncated: false, image: null };
}

// --- The tutor -----------------------------------------------------------------

let emit: (event: AgentEvent) => void = () => {};
let open = false;
let openTask = TASK_VMAX;
/** For a photograph of a reply being written: it stops part way and stays so. */
const holdHalfway = state === "s3-before";

async function stream(text: string, doing: string | null = null): Promise<void> {
  emit({ type: "turn_started" });
  if (doing) {
    emit({ type: "working", doing });
    await wait(260);
  }
  const step = 9;
  const stopAt = holdHalfway && text.includes("Midterm 1 is now") ? Math.floor(text.length * 0.42) : text.length;
  for (let at = 0; at < stopAt; at += step) {
    emit({ type: "text_delta", text: text.slice(at, Math.min(at + step, stopAt)) });
    await wait(5);
  }
  if (stopAt < text.length) return;
  emit({ type: "turn_ended", ok: true, interrupted: false, error: null });
}

let marked: Mark[] = [];

function replyTo(text: string): { reply: string; mark: Mark | null; doing: string | null } {
  const lower = text.toLowerCase();
  if (lower.startsWith("i'm back") || lower.startsWith("let's work on") || lower.startsWith("no checks")) {
    if (state.startsWith("s5")) {
      // Back after a week: the tutor picks up exactly where the screen said, with the draft still under the task.
      openTask = TASK_VMAX;
      return {
        reply: "Welcome back. You stopped on reading $V_{max}$ off the y-intercept, with your working half done. Finish it:\n\n" + TASK_VMAX,
        mark: null,
        doing: "reading your record",
      };
    }
    openTask = TASK_VMAX;
    return {
      reply:
        "You were reading $V_{max}$ off the y-intercept. The intercept is $1/V_{max}$, so $V_{max}$ is its reciprocal, and its units are those of a rate.\n\n" +
        TASK_VMAX,
      mark: null,
      doing: null,
    };
  }
  if (lower.includes("teach me from")) {
    openTask = TASK_VMAX;
    return { reply: "Here is the idea of the plot in one line: a curve you cannot read becomes a line you can.\n\n" + TASK_VMAX, mark: null, doing: "reading the file" };
  }
  if (lower.includes("back to")) {
    return { reply: `Back to it.\n\n${openTask}`, mark: null, doing: null };
  }
  const shared = demoBaseReply(text);
  if (shared) return { reply: shared, mark: null, doing: lower.includes("plan") ? "changing your plan" : null };
  if (text === "Can I have a hint?") return { reply: "Where does the line meet the y-axis, and what is $\\frac{1}{[S]}$ there?", mark: null, doing: null };
  if (text === "I'm lost") return { reply: "Let's find where. On the plot, what is on each axis?", mark: null, doing: null };
  if (/\b4\b/.test(text) && openTask === TASK_VMAX) {
    openTask = TASK_KM;
    return {
      reply:
        "Right: $V_{max} = 1/0.25 = 4$ µM/s. Reading an intercept is always a reciprocal on this plot.\n\nNow the other axis. The x-intercept is where $\\frac{1}{v} = 0$.\n\n" +
        TASK_KM,
      mark: { id: `demo-${Date.now()}`, at: stamp(Date.now()), label: "reading $V_{max}$ from the intercept", mark: "right", help: "none", cold: true, paper: null },
      doing: null,
    };
  }
  if (/\b2\b/.test(text) && openTask === TASK_KM) {
    return {
      reply: "Right: $K_m = 2$ µM. With both constants read, you can draw the original curve back from the line.",
      mark: { id: `demo-${Date.now()}`, at: stamp(Date.now()), label: "reading $K_m$ from the intercept", mark: "right", help: "none", cold: true, paper: null },
      doing: null,
    };
  }
  return {
    reply: "Not quite: on this plot the intercept is a reciprocal. Take one over it.",
    mark: { id: `demo-${Date.now()}`, at: stamp(Date.now()), label: "reading $V_{max}$ from the intercept", mark: "wrong", help: "none", cold: true, paper: null },
    doing: null,
  };
}

async function reply(text: string, markable: boolean): Promise<void> {
  const { reply: words, mark, doing } = replyTo(text);
  await wait(120);
  await stream(words, doing);
  if (holdHalfway && words.includes("Midterm 1 is now")) return;
  // As the Magnesium engine does: which task is open now, in the tutor's own words, and whether this reply posed it.
  const posed = words.includes(openTask);
  emit({ type: "open_task", task: openTask.replace(/^\*\*[^*]+\*\*\s*/, ""), posed });
  demoAfterTurn(text, (event) => {
    if (event.type === "plan_changed") {
      planMs = Date.now();
      changes = [...changes, `${stamp(Date.now()).slice(0, 10)}: moved Midterm 1 a week later and put the cold redos before Paper B, because the midterm moved and three days were skipped.`];
    }
    emit(event);
  });
  if (mark && markable) {
    await wait(260);
    marked = [...marked, mark];
    demoMarked(mark, emit);
  }
}

// --- This Mac -------------------------------------------------------------------

let payment: Payment = "plan";
let engine: Engine = "direct";
const program = (version: string) => ({ shown_path: "~/.local/bin/magnesium", version });
function runtime(): RuntimeSetup {
  const bundled = program("0.3.0+4f1c2ad9e0b7");
  if (replaceRuntime) return { state: "different", installed: program("0.2.0"), bundled, installed_now: false, message: null };
  return { state: "ready", installed: program("0.3.0+4f1c2ad9e0b7"), bundled, installed_now: firstOpen, message: null };
}
function account(): Account {
  if (noClaude) return { name: "Claude Code", installed: false, signed_in: false, paid_with: null, detail: null, problem: "Claude Code is not installed on this Mac.", install_page: "https://code.claude.com/docs/en/setup" };
  if (signedOut) return { name: "Claude Code", installed: true, signed_in: false, paid_with: null, detail: null, problem: null, install_page: null };
  return { name: "Claude Code", installed: true, signed_in: true, paid_with: payment, detail: payment === "plan" ? "your Claude plan" : "an API key", problem: null, install_page: null };
}
const setup = (): Setup => ({ runtime: runtime(), account: account(), payment });

const paperB: Paper = { target: "midterm-1", name: "Paper B", path: "Study/midterm-1/papers/Paper B.pdf", minutes: 50, sat: false };
let sitting: PaperSitting | null = state === "paper-running" ? { target: "midterm-1", name: "Paper B", path: paperB.path, minutes: 50, started_ms: Date.now() - 12 * 60_000 } : null;
let blockMinutes = 50;

function home(): Home {
  const base: Home = {
    folder: FOLDER,
    name: "Biochemistry 210",
    state: upgrade ? "needs_upgrade" : newFolder ? "empty" : "ready",
    message: upgrade ? "An earlier version of Magnesium set this folder up. Upgrade it to study here; Magnesium keeps a copy of your record in .magnesium/backups first." : null,
    exam: { target: "midterm-1", title: "Midterm 1", date: demoPlan().exam_date, day: dayName(demoPlan().exam_date), days_left: 5 },
    last: newFolder ? null : { days_ago: away, goal: GOAL, stopped_at: "What is $V_{max}$, with its units?", next_action: "Read V_max off the intercept." },
    checks_due: 1,
    redos_due: 0,
    first_redo: null,
    papers: [paperB],
    can_resume: !newFolder,
    sitting,
    block_minutes: blockMinutes,
  };
  return base;
}

const progress = (): Progress => ({
  targets: [home().exam!],
  exam: home().exam,
  confirmed: [],
  in_progress: [
    { dots: [2, 0, 0], name: "taking reciprocals of the rate equation", what: "", note: "next Fri 13 Nov", due: false, next_day: null },
    { dots: [0, 1, 0], name: "slope and $K_m$", what: "read the slope as $K_m$", note: "due today", due: true, next_day: null },
    { dots: [2, 0, 0], name: "units of a rate", what: "", note: "next Mon 16 Nov", due: false, next_day: null },
  ],
  untested: [],
  scores: [{ paper: "Diagnostic", date: stamp(Date.now() - 6 * DAY).slice(0, 10), score: "31/50", sure_right: null, sure_total: null }],
  helped: [],
  helped_count: 0,
  unreadable: 0,
  checks_unknown: false,
  proofs: { earned: 4, total: 9, proven: 0, skills: 3, level: 2, to_next: 1, levelled: true },
  level_unknown: false,
  today: null,
  message: null,
});

const lessonList = (): LessonInfo[] =>
  newFolder
    ? []
    : [
        { session: "demo-current", first_ms: START_TURN_MS, last_ms: STOP_TURN_MS, turns: 2, opening: `Let's work on ${GOAL}.`, current: true },
        { session: "demo-mm", first_ms: ago(5, 9, 0), last_ms: ago(5, 9, 40), turns: 6, opening: "Michaelis–Menten from the steady state", current: false },
        { session: "demo-diag", first_ms: ago(6, 19, 0), last_ms: ago(6, 19, 30), turns: 4, opening: "Mark my Diagnostic. I used 47 of 50 minutes.", current: false },
      ];

const OLD_TURNS: Record<string, Turn[]> = {
  "demo-mm": [
    {
      session: "demo-mm",
      ms: ago(5, 9, 5),
      learner: "Michaelis–Menten from the steady state",
      tutor: "The steady state says the complex $ES$ forms as fast as it breaks down.\n\n**Task.** Write that balance as an equation in $[E]$, $[S]$ and $[ES]$.",
      interrupted: false,
    },
    { session: "demo-mm", ms: ago(5, 9, 12), learner: "$k_1[E][S] = (k_{-1} + k_2)[ES]$", tutor: "Right. That is the whole derivation's first line.", interrupted: false },
  ],
  "demo-diag": [
    { session: "demo-diag", ms: ago(6, 19, 5), learner: "Mark my Diagnostic. I used 47 of 50 minutes.", tutor: "Marked: 31 of 50. The gap that cost most is reading the slope as $K_m$; it is in the gap log.", interrupted: false },
  ],
};

// --- The core ---------------------------------------------------------------------

export function demoCore(): Core {
  seed();
  demoResetPlan();
  const known = firstOpen ? [] : [...FOLDERS];
  const card = (folder: string): Card => ({
    folder,
    name: folder.split("/").pop() ?? folder,
    state: "ready",
    exam: null,
    goal: null,
    days_ago: null,
    proofs: { earned: 0, total: 0, proven: 0, skills: 0, level: 0, to_next: 0, levelled: false },
    due: 0,
    sitting: null,
  });
  return {
    initialFolder: async () => null,
    recent: async () => [...known],
    library: async () => known.map(card),
    forgetFolder: async (path) => {
      known.splice(known.indexOf(path), 1);
    },
    findFolders: async () => {
      await wait(400);
      return 0;
    },
    activity: async () => ({ today: stamp(Date.now()).slice(0, 10), days: [], run: 0, studied_today: false }),
    pickFolder: async () => null,
    pickFiles: async () => ["/Users/you/Desktop/IMG_0420.HEIC"],
    pickMaterial: async () => [],
    pickNewFolder: async () => null,
    openFolder: async () => home(),
    newFolder: async () => home(),
    home: async () => home(),
    setUp: async () => ({ ...home(), state: "ready" }),
    upgradeFolder: async () => ({ ...home(), state: "ready", message: null }),
    setup: async () => setup(),
    replaceRuntime: async () => ({ ...runtime(), state: "ready" }),
    keepRuntime: async () => ({ ...runtime(), state: "kept" }),
    account: async () => account(),
    setPayment: async (chosen) => {
      payment = chosen;
      return account();
    },
    engine: async () => ({ chosen: engine, conversation: newFolder ? null : "direct" }),
    setEngine: async (chosen) => {
      engine = chosen;
      return { chosen, conversation: "direct" };
    },
    signIn: async () => {},
    about: async () => ({ version: "0.3.0", commit: "4f1c2ad9e0b7" }),
    openSession: async (resume) => {
      open = true;
      return { resumed: resume, turns: resume && !newFolder ? earlierTurns : [], engine };
    },
    send: async (text, intent) => {
      void reply(text, demoMayMark(text, intent));
    },
    sendNewLesson: async (goal) => {
      void reply(goal, false);
    },
    sendShown: async (shown) => {
      void reply(shown, false);
    },
    interrupt: async () => {},
    decide: async () => {},
    answer: async () => {},
    stop: async () => {
      open = false;
    },
    rail: async () => ({ goal: GOAL, steps: demoPlan().steps, next_action: null }),
    plan: async () => plan(),
    progress: async () => progress(),
    setBlockMinutes: async (minutes) => {
      blockMinutes = minutes;
    },
    attach: async () => "Study/handed-in/2026-11-12/IMG_0420.jpg",
    addMaterial: async (paths) => {
      await wait(300);
      return { saved: paths, keys: 0, lines: [`Added ${paths.length} ${paths.length === 1 ? "file" : "files"} to Study/materials. The tutor's search reads ${paths.length === 1 ? "it" : "them"}.`] };
    },
    addPastedText: async () => ({ saved: ["Study/materials/Pasted text 14.02.txt"], keys: 0, lines: ["Saved the pasted text as Study/materials/Pasted text 14.02.txt."] }),
    materialList: async () => ({ names: FILES.files.filter((f) => f.path.startsWith("Study/materials/")).map((f) => f.path.slice(16)), files: 4, keys: 1 }),
    marks: async () => [...records, ...marked],
    lessons: async () => lessonList(),
    turns: async () => (newFolder ? [] : earlierTurns),
    lessonTurns: async (session) => (session === "demo-current" ? (newFolder ? [] : earlierTurns) : (OLD_TURNS[session] ?? [])),
    studyFiles: async () => FILES,
    writePlan: async (_path, text) => {
      planMs = Date.now();
      void text;
      return planMs;
    },
    onDrop: async (_handler, hover) => {
      if (state === "drop") window.setTimeout(() => hover(true), 300);
      return () => {};
    },
    source: async (path) => source(path),
    sourcePdf: async (path) =>
      (path.includes("Paper B")
        ? tinyPdf(["1. Read Vmax and Km from the plot.", "2. A competitive inhibitor: what moves?", "3. Units of kcat."], "Paper B, 50 minutes")
        : tinyPdf(["Michaelis-Menten kinetics", "The Lineweaver-Burk plot", "Reading the intercepts", "Inhibition"], "Week 5 slides")
      ).buffer as ArrayBuffer,
    openFile: async () => {},
    openUrl: async () => {},
    image: async () => PHOTO,
    paperStart: async (paper, minutes) => {
      sitting = { target: paper.target, name: paper.name, path: paper.path, minutes, started_ms: Date.now() };
      return sitting;
    },
    paperEnd: async () => {
      sitting = null;
      return null;
    },
    onEvent: async (handler) => {
      emit = handler;
      return () => {
        emit = () => {};
      };
    },
    selftest: async () => false,
    selftestReport: async () => {},
  };
}

// --- Driving the page to the address's state ---------------------------------------

const CHANGE = "I skipped three days and the midterm moved, change the plan";
const OFF_PLAN = "By the way, why do people still draw Lineweaver–Burk plots if they stretch the errors?";

async function idle(actions: Actions): Promise<void> {
  const started = Date.now();
  await wait(60);
  while (Date.now() - started < 8000) {
    const live = actions.session().live;
    if (!live || live.status === "idle") break;
    await wait(25);
  }
  await wait(320);
}

/** Every state the shots photograph, by its address. */
export const STATES = [
  "s1-cold",
  "s1-cold-first",
  "s2-start",
  "s2-answer",
  "s3-before",
  "s3-after",
  "s4-offplan",
  "s4-return",
  "s5-week",
  "s5-week-next",
  "settings",
  "open",
  "commands",
  "folders",
  "sidebar",
  "material",
  "paper",
  "paper-running",
  "record",
  "earlier",
  "plan",
  "plan-edit",
  "new-lesson",
  "hint",
  "setup-signed-out",
  "setup-no-claude",
  "setup-replace",
  "upgrade",
  "drop",
];

export async function startFromAddress(actions: Actions): Promise<void> {
  void open;
  const at = state;
  const lesson = async (upTo: number) => {
    await actions.continueLesson();
    await idle(actions);
    if (upTo < 1) return;
    await actions.answer("$V_{max} = 1/0.25 = 4$ µM/s");
    await idle(actions);
    if (upTo < 2) return;
    actions.setPlace("talk");
    await actions.talk(CHANGE);
    await idle(actions);
    if (upTo < 3) return;
    await actions.talk(OFF_PLAN);
    await idle(actions);
    if (upTo < 4) return;
    await actions.talk("OK, back to the task.");
    await idle(actions);
  };
  switch (at) {
    case "s2-start":
    case "s5-week-next":
      await lesson(0);
      break;
    case "s2-answer":
      await lesson(1);
      break;
    case "s3-before":
      await lesson(1);
      actions.setPlace("talk");
      await actions.talk(CHANGE);
      break;
    case "s3-after":
      await lesson(2);
      break;
    case "s4-offplan":
      await lesson(3);
      break;
    case "s4-return":
      await lesson(4);
      break;
    case "hint":
      await lesson(0);
      await actions.help("hint");
      await idle(actions);
      break;
    case "settings":
      actions.openDoc("settings");
      break;
    case "open":
      actions.setSheet("open");
      break;
    case "commands":
      actions.setSheet("commands");
      break;
    case "folders":
      actions.setSheet("folders");
      break;
    case "material":
      actions.openDoc("file:Study/materials/lineweaver-burk.md", true);
      break;
    case "paper":
    case "paper-running":
      actions.openDoc("file:Study/midterm-1/papers/Paper B.pdf");
      break;
    case "record":
      actions.openDoc("record");
      break;
    case "earlier":
      actions.openDoc("lesson:demo-mm");
      break;
    case "plan":
      actions.openDoc("plan");
      break;
    case "plan-edit":
      actions.openDoc("plan");
      await wait(200);
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "e", metaKey: true }));
      break;
    case "drop":
      actions.setDragging(true);
      break;
    default:
      break;
  }
}
