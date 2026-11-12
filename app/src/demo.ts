// A stand-in core for a plain browser, with made-up data, so the page can be
// worked on and photographed without a window or an agent. No real course,
// grade, person or institution is in it.
//
// The address picks a state: #s1-cold, #s1-cold-first, #s2-start, #s2-answer,
// #s3-before, #s3-after, #s4-offplan, #s4-return, #s5-week, #s5-week-next,
// #settings, and the extras listed in `SCRIPTS` below. When the state is on
// screen the page's root gets data-shot="ready", which scripts/shots.mjs waits for.

import type { Actions } from "./App";
import { daysBefore, today } from "./days";
import { demoAfterTurn, demoBaseReply, demoMarked, demoMayMark, demoPlan, demoResetPlan } from "./demoBase";
import { tinyPdf } from "./demoPdf";
import type { Account, AgentEvent, Card, Core, Engine, Home, Intent, JournalCourse, Mark, Paper, PaperSitting, Payment, Progress, RuntimeSetup, Setup, Turn } from "./types";

const wait = (ms: number) => new Promise((done) => window.setTimeout(done, ms));
const pad = (n: number) => String(n).padStart(2, "0");

const BIO = "/Users/you/Study/Biochemistry";
const LIN = "/Users/you/Study/Linear-algebra";
const HAR = "/Users/you/Study/Harmony";

const address = () => decodeURIComponent(window.location.hash.replace(/^#/, ""));
let state = address() || "s1-cold";
/** How many days ago the learner last worked: two, or eight after a week away. */
const away = () => (state.startsWith("s5") ? 8 : 2);

// ----------------------------------------------------------------- record

const at = (daysAgo: number, hh: number, mm: number) => {
  const date = new Date();
  date.setDate(date.getDate() - daysAgo);
  date.setHours(hh, mm, 0, 0);
  return date;
};
const stamp = (date: Date) => `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;

let markId = 0;
const mark = (date: Date, label: string, verdict: string, question: string | null, help = "none", cold = true, paper: string | null = null): Mark => ({
  id: `demo-${++markId}`,
  at: stamp(date),
  label,
  mark: verdict,
  help,
  cold,
  paper,
  question,
});

type Said = { m: [number, number]; learner: string; tutor: string; mark?: [string, string, string | null, string?, boolean?] };

function course(folder: string, name: string, daysAgo: number, said: Said[]): JournalCourse {
  const turns: Turn[] = [];
  const marks: Mark[] = [];
  for (const s of said) {
    const date = at(daysAgo, s.m[0], s.m[1]);
    turns.push({ session: "demo", ms: date.getTime(), learner: s.learner, tutor: s.tutor, interrupted: false });
    if (s.mark) {
      const [label, verdict, question, help, cold] = s.mark;
      const marked = new Date(date.getTime() + 40_000);
      marks.push(mark(marked, label, verdict, question, help ?? "none", cold ?? true));
    }
  }
  return { folder, name: folder.split("/").pop() ?? name, turns, marks };
}

let kept: JournalCourse[] | null = null;
function journal(): JournalCourse[] {
  kept ??= written();
  return kept.map((c) => (c.folder === BIO ? { ...c, marks: [...(c.marks ?? []), ...liveMarks] } : c));
}

function written(): JournalCourse[] {
  if (state === "s1-cold-first") return [];
  const last = away();
  const bio = course(BIO, "Biochemistry", last, [
    { m: [18, 4], learner: "I'm back. Let's continue.", tutor: "Welcome back. Quick check first: what does the slope of a Lineweaver–Burk plot give?" },
    {
      m: [18, 6],
      learner: "$K_m / V_{max}$",
      tutor: "Right. And where does the line cross the x-axis?",
      mark: ["slope of a Lineweaver–Burk plot", "right", "What does the slope of a Lineweaver–Burk plot give?"],
    },
    { m: [18, 9], learner: "At $K_m$", tutor: "Not quite: it crosses at $-1/K_m$. Set $1/v = 0$ and solve for $1/[S]$.", mark: ["x-intercept of a Lineweaver–Burk plot", "wrong", "Where does the line cross the x-axis?"] },
    { m: [18, 14], learner: "Can I have a hint?", tutor: "Start from $\\frac{1}{v} = \\frac{K_m}{V_{max}}\\cdot\\frac{1}{[S]} + \\frac{1}{V_{max}}$ and put $\\frac{1}{v} = 0$." },
    {
      m: [18, 17],
      learner: "$1/[S] = -1/K_m$",
      tutor: "That's it. Next time: what the y-intercept gives.",
      mark: ["x-intercept of a Lineweaver–Burk plot", "right", "Solve $1/v = 0$ for $1/[S]$.", "hint", false],
    },
  ]);
  const lin = course(LIN, "Linear algebra", last + 1, [
    { m: [21, 12], learner: "Let's work on the final.", tutor: "Find the eigenvalues of $A = \\begin{pmatrix} 2 & 1 \\\\ 0 & 3 \\end{pmatrix}$." },
    { m: [21, 15], learner: "$\\lambda = 2, 3$", tutor: "Right: a triangular matrix shows them on its diagonal. Why?", mark: ["eigenvalues of a triangular matrix", "right", "Find the eigenvalues of A."] },
    { m: [21, 19], learner: "because det(A − λI) is the product of the diagonal", tutor: "Yes, that is the reason.", mark: ["why a triangular matrix shows its eigenvalues", "partly", "Why does the diagonal give them?"] },
  ]);
  const har = course(HAR, "Harmony", last + 3, [
    { m: [9, 31], learner: "What does 6/4 under a bass note mean?", tutor: "A sixth and a fourth above the bass: the chord in second inversion." },
    { m: [9, 36], learner: "second inversion, so the fifth is in the bass", tutor: "Right.", mark: ["figured bass: 6/4", "right", "Which note of the chord is in the bass under 6/4?"] },
  ]);
  const bioOld = course(BIO, "Biochemistry", last + 4, [
    { m: [17, 40], learner: "Let's work on Midterm 1.", tutor: "Units first. What are the units of $V_{max}$?" },
    { m: [17, 43], learner: "µM/s", tutor: "Right.", mark: ["units of a rate", "right", "What are the units of $V_{max}$?"] },
  ]);
  return [
    { ...bio, turns: [...bioOld.turns, ...bio.turns], marks: [...(bioOld.marks ?? []), ...(bio.marks ?? [])] },
    lin,
    har,
  ];
}

// ------------------------------------------------------------------ lesson

let handlers: ((event: AgentEvent) => void)[] = [];
const emit = (event: AgentEvent) => handlers.forEach((handler) => handler(event));
let running = false;
let held: (() => void) | null = null;
let openTask: string | null = null;
const liveMarks: Mark[] = [];
let sitting: PaperSitting | null = state === "paper" ? { target: "midterm-1", name: "Paper B", path: "Study/midterm-1/papers/PaperB.pdf", minutes: 50, started_ms: Date.now() - 12 * 60_000 } : null;
let engineChosen: Engine = "direct";
let payment: Payment = "plan";

const VMAX = "The line meets the y-axis at $0.25$ s/µM. What is $V_{max}$, with its units?";
const KM = "The slope of the same line is $0.5$ s. What is $K_m$?";
const CHECK = "Quick check 1: a Lineweaver–Burk line has slope $2$ s and meets the y-axis at $0.5$ s/µM. What is $K_m$?";

function replyTo(text: string, intent: Intent | undefined, opener: boolean): { reply: string; task: string | null | undefined; mark?: Mark } {
  if (opener) {
    if (state.startsWith("s5")) {
      return { reply: `Welcome back. One check from last time before the y-intercept.\n\n${CHECK}`, task: CHECK };
    }
    return {
      reply:
        "You stopped at the y-intercept. In the double-reciprocal form\n\n$$\\frac{1}{v} = \\frac{K_m}{V_{max}}\\cdot\\frac{1}{[S]} + \\frac{1}{V_{max}}$$\n\nthe line meets the y-axis where $1/[S] = 0$, so all that is left there is $1/V_{max}$.\n\n" +
        VMAX,
      task: VMAX,
    };
  }
  const base = demoBaseReply(text);
  if (base !== null) {
    // Back to the task: the demo poses it again, in bold, as a tutor may.
    if (text.toLowerCase().includes("back to")) return { reply: base, task: base.split("**Task.**")[1]?.trim() ?? openTask };
    return { reply: base, task: undefined };
  }
  if (text === "Can I have a hint?") return { reply: "Look at what the intercept stands for: the line there is $1/V_{max}$.", task: undefined };
  if (text === "I'm lost") return { reply: "What do you see on the y-axis of this plot: $v$, or $1/v$?", task: undefined };
  if (intent === "talk" || !demoMayMark(text, intent) || !openTask) return { reply: "Go on.", task: undefined };
  const now = new Date();
  if (openTask === VMAX) {
    return {
      reply: "Right: the intercept is $1/V_{max}$, so $V_{max} = 1/0.25 = 4$ µM/s.\n\n" + KM,
      task: KM,
      mark: mark(now, "reading $V_{max}$ from the y-intercept", "right", VMAX),
    };
  }
  if (openTask === KM || openTask === CHECK) {
    return { reply: "Right: $K_m = \\text{slope} \\times V_{max} = 0.5 \\times 4 = 2$ µM.", task: null, mark: mark(now, "reading $K_m$ from the slope", "right", openTask) };
  }
  return { reply: "Not quite. Try it from the intercept again.", task: undefined, mark: mark(now, "reading the plot", "wrong", openTask) };
}

async function turn(text: string, intent: Intent | undefined, opener: boolean) {
  running = true;
  const { reply, task, mark: marked } = replyTo(text, intent, opener);
  emit({ type: "turn_started" });
  await wait(state === "s3-before" ? 120 : 60);
  const half = state === "s3-before" && text.toLowerCase().includes("change the plan") ? Math.floor(reply.length * 0.45) : reply.length;
  for (let i = 0; i < half; i += 9) {
    emit({ type: "text_delta", text: reply.slice(i, Math.min(half, i + 9)) });
    await wait(8);
  }
  if (half < reply.length) {
    // Photographed while the tutor is still writing.
    await new Promise<void>((done) => (held = done));
    emit({ type: "text_delta", text: reply.slice(half) });
  }
  emit({ type: "text_done", text: reply });
  if (task !== undefined) openTask = task;
  emit({ type: "turn_ended", ok: true, interrupted: false, error: null });
  running = false;
  if (marked) {
    await wait(150);
    liveMarks.push(marked);
    demoMarked(marked, emit);
  }
  demoAfterTurn(text, emit);
}

// -------------------------------------------------------------------- core

const card = (folder: string, name: string, goal: string, daysAgo: number, exam: Card["exam"]): Card => ({
  folder,
  name,
  state: "ready",
  exam,
  goal,
  days_ago: daysAgo,
  proofs: { earned: 0, total: 0, proven: 0, skills: 0, level: 0, to_next: 0, levelled: false },
  due: 0,
  sitting: null,
});

const examOn = (title: string, ahead: number) => {
  const day = daysBefore(today(), -ahead);
  return { target: title.toLowerCase().replace(/\s+/g, "-"), title, date: day, day, days_left: ahead };
};

const papers: Paper[] = [
  { target: "midterm-1", name: "Paper A", path: "Study/midterm-1/papers/PaperA.pdf", minutes: 50, sat: true },
  { target: "midterm-1", name: "Paper B", path: "Study/midterm-1/papers/PaperB.pdf", minutes: 50, sat: false },
];

function homeOf(folder: string): Home {
  const name = folder.split("/").pop() ?? folder;
  const bio = folder === BIO;
  return {
    folder,
    name,
    state: "ready",
    message: null,
    exam: bio ? { target: "midterm-1", title: "Midterm 1", date: demoPlan().exam_date, day: null, days_left: null } : folder === LIN ? examOn("Final", 22) : null,
    last: {
      days_ago: bio ? away() : folder === LIN ? away() + 1 : away() + 3,
      goal: bio ? "Reading $V_{max}$ and $K_m$ from a Lineweaver–Burk plot" : folder === LIN ? "Eigenvalues and diagonalising" : "Reading figured bass",
      stopped_at: bio ? "what the y-intercept gives" : null,
      next_action: null,
    },
    checks_due: 1,
    redos_due: 0,
    first_redo: null,
    papers: bio ? papers : [],
    can_resume: true,
    sitting: bio ? sitting : null,
    block_minutes: 50,
  };
}

let open: string | null = null;

const runtime = (): RuntimeSetup => {
  const program = { shown_path: "~/.local/bin/magnesium", version: "0.3.0+demo" };
  if (state === "setup-replace") return { state: "different", installed: { shown_path: program.shown_path, version: "0.2.0+older" }, bundled: program, installed_now: false, message: null };
  return { state: "ready", installed: program, bundled: program, installed_now: state === "s1-cold-first", message: null };
};

const account = (): Account => ({
  name: "Claude Code",
  installed: state !== "setup-no-claude",
  signed_in: state !== "setup-signed-out" && state !== "setup-no-claude",
  paid_with: payment,
  detail: "Claude Max",
  problem: payment === "api_key" ? "No API key is set, so lessons would use your Claude plan." : null,
  install_page: "https://code.claude.com/docs/en/setup",
});

const progress = (): Progress => ({
  targets: [],
  exam: null,
  confirmed: [{ dots: [2, 2, 2], name: "units of a rate", what: "", note: "confirmed Fri 6 Nov", due: false, next_day: null }],
  in_progress: [
    { dots: [2, 0, 0], name: "slope of a Lineweaver–Burk plot", what: "", note: "next Fri 13 Nov", due: false, next_day: null },
    { dots: [0, 0, 0], name: "x-intercept of a Lineweaver–Burk plot", what: "took $K_m$ for the intercept", note: "due today", due: true, next_day: null },
  ],
  untested: [{ dots: [0, 0, 0], name: "reading $V_{max}$ from the y-intercept", what: "", note: "–", due: false, next_day: null }],
  scores: [],
  helped: [],
  helped_count: 0,
  unreadable: 0,
  checks_unknown: false,
  proofs: { earned: 4, total: 12, proven: 1, skills: 4, level: 3, to_next: 1, levelled: true },
  level_unknown: false,
  today: null,
  message: null,
});

export function demoCore(): Core {
  const folders = () => (state === "s1-cold-first" ? [] : [BIO, LIN, HAR]);
  return {
    initialFolder: async () => null,
    recent: async () => folders(),
    library: async () =>
      state === "s1-cold-first"
        ? []
        : [
            card(BIO, "Biochemistry", "Reading $V_{max}$ and $K_m$ from a Lineweaver–Burk plot", away(), { target: "midterm-1", title: "Midterm 1", date: demoPlan().exam_date, day: null, days_left: null }),
            card(LIN, "Linear-algebra", "Eigenvalues and diagonalising", away() + 1, examOn("Final", 22)),
            card(HAR, "Harmony", "Reading figured bass", away() + 3, null),
          ],
    journal: async () => journal(),
    forgetFolder: async () => {},
    findFolders: async () => 0,
    activity: async () => ({ today: today(), days: [], run: 0, studied_today: false }),
    pickFolder: async () => null,
    pickFiles: async () => ["/Users/you/Desktop/working.jpg"],
    pickMaterial: async () => [],
    pickNewFolder: async () => null,
    openFolder: async (path) => {
      open = path;
      return homeOf(path);
    },
    newFolder: async (path) => {
      open = path;
      return { ...homeOf(path), state: "empty", last: null, exam: null, papers: [] };
    },
    home: async () => homeOf(open ?? BIO),
    setUp: async () => homeOf(open ?? BIO),
    upgradeFolder: async () => homeOf(open ?? BIO),
    setup: async (): Promise<Setup> => ({ runtime: runtime(), account: account(), payment }),
    replaceRuntime: async () => ({ ...runtime(), state: "ready" }),
    keepRuntime: async () => ({ ...runtime(), state: "kept" }),
    account: async () => account(),
    setPayment: async (choice) => {
      payment = choice;
      return account();
    },
    engine: async () => ({ chosen: engineChosen, conversation: "direct" }),
    setEngine: async (choice) => {
      engineChosen = choice;
      return { chosen: choice, conversation: "direct" };
    },
    signIn: async () => {},
    about: async () => ({ version: "0.3.0", commit: null }),
    openSession: async () => ({ resumed: true, turns: [], engine: "direct" }),
    send: async (text, intent) => void turn(text, intent, false),
    sendNewLesson: async (goal) => void turn(goal, undefined, true),
    sendShown: async (shown) => void turn(shown, undefined, true),
    interrupt: async () => {},
    decide: async () => {},
    answer: async () => {},
    stop: async () => {},
    rail: async () => ({ goal: demoPlan().goal, steps: demoPlan().steps, next_action: null, task: openTask }),
    plan: async () => demoPlan(),
    progress: async () => progress(),
    setBlockMinutes: async () => {},
    attach: async (path) => `Study/handed-in/working/${today()}/${path.split("/").pop()}`,
    addMaterial: async (paths) => ({ saved: paths, keys: 0, lines: [`${paths.length} files added.`] }),
    addPastedText: async () => ({ saved: ["Study/materials/Pasted text.txt"], keys: 0, lines: ["Saved as Pasted text.txt."] }),
    materialList: async () => ({ names: ["Week 4 slides.pdf", "week3-enzyme-kinetics.md", "Lineweaver-Burk worked examples.pdf"], files: 3, keys: 1 }),
    marks: async () => [...(journal().find((c) => c.folder === BIO)?.marks ?? [])],
    onDrop: async () => () => {},
    source: async (path) => ({ name: path.split("/").pop() ?? path, kind: path.endsWith(".pdf") ? "pdf" : "text", text: "Week 4: enzyme kinetics.", truncated: false, image: null }),
    sourcePdf: async () => tinyPdf(["Week 4: enzyme kinetics", "The Lineweaver–Burk plot", "Reading the intercepts"]).buffer as ArrayBuffer,
    openFile: async () => {},
    openUrl: async () => {},
    image: async () => "data:,",
    paperStart: async (paper, minutes) => (sitting = { target: paper.target, name: paper.name, path: paper.path, minutes, started_ms: Date.now() - 12 * 60_000 }),
    paperEnd: async () => {
      sitting = null;
      return null;
    },
    onEvent: async (handler) => {
      handlers.push(handler);
      return () => (handlers = handlers.filter((h) => h !== handler));
    },
    selftest: async () => false,
    selftestReport: async () => {},
  };
}

// ----------------------------------------------------------------- scripts

const idle = async () => {
  await wait(30);
  while (running) await wait(20);
  await wait(260);
};

const shot = () => document.documentElement.setAttribute("data-shot", "ready");

async function lesson(actions: Actions) {
  await actions.start();
  await idle();
}

async function answered(actions: Actions) {
  await lesson(actions);
  await actions.say("$V_{max} = 4$ µM/s", "task");
  await idle();
}

async function planChanged(actions: Actions, hold: boolean) {
  await answered(actions);
  actions.setPlace("page");
  await wait(40);
  const said = actions.say("I skipped three days and the midterm moved, change the plan", "page");
  if (hold) {
    await said;
    while (!held) await wait(20);
    await wait(120);
    return;
  }
  await said;
  await idle();
}

async function offPlan(actions: Actions) {
  await planChanged(actions, false);
  await actions.say("By the way, what's a Hanes–Woolf plot? It came up in the reading.", "page");
  await idle();
}

const SCRIPTS: Record<string, (actions: Actions) => Promise<void>> = {
  "s1-cold": async () => {},
  "s1-cold-first": async () => {},
  "s2-start": lesson,
  "s2-answer": answered,
  "s3-before": (a) => planChanged(a, true),
  "s3-after": (a) => planChanged(a, false),
  "s4-offplan": offPlan,
  "s4-return": async (a) => {
    await offPlan(a);
    await a.say("Back to the task", "page");
    await idle();
  },
  "s5-week": async () => {},
  "s5-week-next": lesson,
  "narrow-lesson": answered,
  settings: async (a) => a.openSheet("settings"),
  // Extras.
  courses: async (a) => a.openSheet("courses"),
  course: async (a) => {
    a.openSheet("course");
    await wait(200);
  },
  plan: async (a) => {
    await answered(a);
    a.openSheet("plan");
  },
  commands: async (a) => a.openSheet("commands"),
  source: async (a) => {
    await answered(a);
    a.openSource({ path: "Study/materials/Week 4 slides.pdf", page: 2 });
    await wait(900);
  },
  stop: async (a) => {
    await answered(a);
    a.beginStop();
  },
  paper: async () => {},
  "setup-signed-out": async () => {},
  "setup-no-claude": async () => {},
  "setup-replace": async () => {},
  "face-iowan": async (a) => {
    a.setPrefs({ face: "iowan", size: 19, scheme: "system", width: "narrow" });
    await answered(a);
  },
};

export async function startFromAddress(actions: Actions) {
  demoResetPlan();
  const script = SCRIPTS[state] ?? SCRIPTS["s1-cold"];
  try {
    await wait(120);
    await script(actions);
    await wait(250);
  } finally {
    shot();
  }
}

/** Let a held reply finish: for a person looking at #s3-before by hand. */
export function release() {
  held?.();
  held = null;
}

window.addEventListener("hashchange", () => {
  state = address();
  window.location.reload();
});
