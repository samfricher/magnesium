// A stand-in core for a plain browser, with made-up data. It exists so the
// screens can be worked on and photographed without a window or an agent.
// Nothing here ships a real course, a real grade or a real learner's words.

import type { Actions } from "./App";
import { tinyPdf } from "./demoPdf";
import type {
  Added,
  Account,
  Activity,
  AgentEvent,
  Card,
  Core,
  Engine,
  Home,
  Opened,
  Paper,
  PaperSitting,
  Payment,
  Progress,
  Proofs,
  Rail,
  Row,
  RuntimeSetup,
  Setup,
  Source,
  Turn,
} from "./types";

const wait = (ms: number) => new Promise((done) => window.setTimeout(done, ms));

const paperB: Paper = { target: "midterm-1", name: "Paper B", path: "Study/midterm-1/papers/PaperB.pdf", minutes: 50, sat: false };
const paperA: Paper = { target: "midterm-1", name: "Paper A", path: "Study/midterm-1/papers/PaperA.pdf", minutes: 50, sat: true };

let variant = "ready";
let sitting: PaperSitting | null = null;
let blockMinutes = 50;
// What the library shows: "" for four courses, "empty" for none yet.
let shelf = "";
let forgotten: string[] = [];
let found = false;
let studiedToday = true;
// A sitting earns a proof when a check or a redo it opened with was passed.
let sittingsDone = 0;
let stoppedAt: string | null = "PS3 Q3, step 3: what the y-intercept gives. The line through 0.25 is next.";
// Answers the tutor marked today. Each answer sent in the demo is marked,
// except at #summary:unmarked, which shows a sitting with none marked.
let markedToday = 4;
let marking = true;
// What runs lessons, and the engine the demo course's conversation began on.
let engineChosen: Engine = "plugin";
let conversationEngine: Engine | null = "plugin";

function home(): Home {
  const base: Home = {
    folder: "/Users/you/Study/Demo-Course",
    name: "Demo-Course",
    state: "ready",
    message: null,
    exam: { target: "midterm-1", title: "Midterm 1", date: "2026-11-17", day: "Tue 17 Nov", days_left: 6 },
    last: {
      days_ago: 3,
      goal: "PS3 Q3 · reading Vmax and Km from a Lineweaver-Burk plot",
      stopped_at: stoppedAt,
      next_action: "Find Vmax from the intercept 0.25.",
    },
    checks_due: 2,
    redos_due: 3,
    first_redo: "Diagnostic Q4",
    papers: [paperB, paperA],
    can_resume: true,
    sitting,
    block_minutes: blockMinutes,
  };
  if (variant === "new") {
    return { ...base, state: "not_set_up", exam: null, last: null, checks_due: 0, redos_due: 0, first_redo: null, papers: [], can_resume: false };
  }
  if (variant === "quiet") return { ...base, checks_due: 0, redos_due: 0, first_redo: null };
  if (variant === "upgrade") {
    return {
      ...base,
      state: "needs_upgrade",
      message:
        "An earlier version of Magnesium set this folder up. Upgrade it to study here with this version. Magnesium keeps a copy of your record in .magnesium/backups before it changes it.",
      exam: null,
      last: null,
      checks_due: 0,
      redos_due: 0,
      first_redo: null,
      papers: [],
    };
  }
  if (variant === "broken") {
    return { ...base, state: "no_runtime", message: "The magnesium program is not installed, or the window cannot find it. Install Magnesium, then open the window again." };
  }
  return base;
}

// What the setup screen shows, chosen by the address: #welcome:replace,
// #welcome:signed-out, #welcome:no-claude, #welcome:key or #welcome:first.
// #setup shows it as opened from the library, with everything ready.
let setupVariant = "";
let payment: Payment = "plan";
let signedIn = true;
const program = (version: string) => ({ shown_path: "~/.local/bin/magnesium", version });

function runtimeSetup(): RuntimeSetup {
  const bundled = program("0.3.0+4f1c2ad9e0b7");
  if (setupVariant === "replace") {
    return { state: "different", installed: program("0.2.0"), bundled, installed_now: false, message: null };
  }
  // Worded as the core words it: install.rs.
  if (setupVariant === "too-old") {
    return {
      state: "too_old",
      installed: program("0.2.0"),
      bundled,
      installed_now: false,
      message:
        "The magnesium program on this Mac, version 0.2.0, is older than this window needs. Replace it with the copy inside this app, version 0.3.0. Until then no lesson starts.",
    };
  }
  if (setupVariant === "too-old-source") {
    return {
      state: "too_old",
      installed: program("0.3.0"),
      bundled: null,
      installed_now: false,
      message:
        "The magnesium program on this Mac, version 0.3.0, is older than this window needs. Ask Claude Code to install the newest Magnesium with the install guide, docs/install.md in the Magnesium repository, then check again. Until then no lesson starts.",
    };
  }
  return { state: "ready", installed: bundled, bundled, installed_now: setupVariant === "first", message: null };
}

function account(): Account {
  const base: Account = {
    name: "Claude Code",
    installed: true,
    signed_in: true,
    paid_with: "plan",
    detail: "your Claude Max plan",
    problem: null,
    install_page: null,
  };
  if (setupVariant === "no-claude") {
    return {
      ...base,
      installed: false,
      signed_in: false,
      paid_with: null,
      detail: null,
      problem: "Claude Code is not on this Mac, or the window cannot find it. Install it, then check again.",
      install_page: "https://code.claude.com/docs/en/setup",
    };
  }
  if (!signedIn) {
    return { ...base, signed_in: false, paid_with: null, detail: null, problem: "Claude Code is not signed in. Sign in with your Claude account." };
  }
  if (setupVariant === "key" && payment === "plan") {
    return {
      ...base,
      paid_with: "api_key",
      detail: "an API key set in ANTHROPIC_API_KEY, per use",
      problem:
        "Claude Code would pay with an API key set in ANTHROPIC_API_KEY, per use, not your Claude plan. Sign in with your Claude account to use the plan.",
    };
  }
  if (payment === "api_key") return { ...base, paid_with: "api_key", detail: "your Anthropic Console sign-in, per use" };
  return base;
}

const demoSetup = (): Setup => ({ runtime: runtimeSetup(), account: account(), payment });

const rail: Rail = {
  goal: "PS3 Q3 · reading Vmax and Km from a Lineweaver-Burk plot",
  steps: [
    { step: "Write the rate equation", status: "done" },
    { step: "Take reciprocals of both sides", status: "done" },
    { step: "What the y-intercept gives", status: "doing" },
    { step: "What the slope gives", status: "todo" },
    { step: "Check the units", status: "todo" },
  ],
  next_action: "PS3 Q3, step 3: what the y-intercept gives. The line through 0.25 is next.",
};

const isoDay = (ahead: number) => {
  const date = new Date();
  date.setDate(date.getDate() + ahead);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
};

const row = (dots: [number, number, number], name: string, what: string, note: string, ahead: number | null = null): Row => ({
  dots,
  name,
  what,
  note,
  due: note.startsWith("due"),
  next_day: note.startsWith("due") ? isoDay(0) : ahead === null ? null : isoDay(ahead),
});

const proofs = (earned: number, proven: number, skills: number): Proofs => {
  const total = skills * 3;
  const level = total === 0 ? 1 : 1 + Math.floor((earned * 9) / total);
  return { earned, total, proven, skills, level, to_next: total === 0 || level >= 10 ? 0 : Math.ceil((level * total) / 9) - earned, levelled: true };
};

/** A course with no plan: its skills are counted, with no level. */
const counted = (proven: number, skills: number): Proofs => ({ earned: 0, total: 0, proven, skills, level: 0, to_next: 0, levelled: false });

const card = (name: string, more: Partial<Card>): Card => ({
  folder: `/Users/you/Study/${name}`,
  name,
  state: "ready",
  exam: null,
  goal: null,
  days_ago: null,
  proofs: proofs(0, 0, 0),
  due: 0,
  sitting: null,
  ...more,
});

function library(): Card[] {
  if (shelf === "empty") return [];
  const now = home();
  const cards = [
    card("Demo-Course", {
      state: now.state,
      exam: now.exam,
      goal: now.last?.goal ?? null,
      days_ago: now.last?.days_ago ?? null,
      proofs: now.state === "ready" ? proofs(15, 3, 11) : proofs(0, 0, 0),
      due: now.redos_due + (now.checks_due ?? 0),
      sitting: now.sitting,
    }),
    card("Linear-Algebra", {
      exam: { target: "final", title: "Final", date: "2026-12-04", day: "Fri 4 Dec", days_left: 23 },
      goal: "Diagonalising a matrix: when $A = PDP^{-1}$ exists",
      days_ago: 1,
      proofs: proofs(31, 8, 18),
      due: 1,
    }),
    card("Probability", {
      exam: { target: "test-2", title: "Test 2", date: "2026-12-12", day: "Sat 12 Dec", days_left: 31 },
      goal: "The cdf method for $U = \\sqrt{Y}$",
      days_ago: 6,
      proofs: proofs(6, 1, 14),
    }),
    card("Music-Theory", {
      goal: "Reading figured bass",
      days_ago: 3,
      proofs: counted(2, 5),
    }),
    card("Organic-Chemistry", { state: "not_set_up" }),
  ];
  if (found) cards.push(card("Thermodynamics", { state: "empty" }));
  return cards.filter((each) => !forgotten.includes(each.folder));
}

function activity(): Activity {
  const pad = (n: number) => String(n).padStart(2, "0");
  const day = (back: number) => {
    const date = new Date();
    date.setDate(date.getDate() - back);
    return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
  };
  // Studied on these days before today, and today once a sitting is done.
  const before = [1, 2, 3, 5, 6, 8, 9, 10, 13, 15, 16, 17, 20, 22, 23, 27, 29, 30];
  const days = before.map((back) => day(back));
  if (studiedToday) days.push(day(0));
  days.sort();
  return { today: day(0), days, run: studiedToday ? 4 : 3, studied_today: studiedToday };
}

function progress(): Progress {
  if (variant === "new" || variant === "upgrade" || variant === "broken") {
    return {
      targets: [],
      exam: null,
      in_progress: [],
      confirmed: [],
      untested: [],
      scores: [],
      helped: [],
      helped_count: 0,
      unreadable: 0,
      checks_unknown: false,
      proofs: proofs(0, 0, 0),
      level_unknown: false,
      today: null,
      message: "Nothing is proved yet. Progress fills in after your first marked paper or quick check.",
    };
  }
  return {
    targets: [
      { target: "midterm-1", title: "Midterm 1", date: "2026-11-17", day: "Tue 17 Nov", days_left: 6 },
      { target: "final", title: "Final", date: "2026-12-15", day: "Tue 15 Dec", days_left: 35 },
    ],
    exam: { target: "midterm-1", title: "Midterm 1", date: "2026-11-17", day: "Tue 17 Nov", days_left: 6 },
    in_progress: [
      // After a sitting in the demo, the first skill has earned its second proof.
      sittingsDone > 0
        ? row([2, 2, 0], "intercept and $V_{max}$", "took the intercept as $V_{max}$", "next in 3 days", 3)
        : row([2, 0, 0], "intercept and $V_{max}$", "took the intercept as $V_{max}$", "due today"),
      row([1, 1, 0], "slope and $K_m$", "read the slope as $K_m$ · found before the exam, restarted 3 days ago", "due today"),
      row([2, 2, 0], "units of a rate", "gave $V_{max}$ in µM", "next in 3 days", 3),
      row([2, 0, 0], "competitive inhibition", "moved $V_{max}$, not $K_m$", "next tomorrow", 1),
      row([0, 0, 0], "Hill coefficient", "read $n$ from the wrong axis", "fix it first"),
      // Never got wrong: answered right, alone and cold, on two papers.
      row([2, 2, 0], "substrate concentration", "cold in Paper A Q2", "–"),
    ],
    confirmed: [
      row([2, 2, 2], "rate equation", "swapped Km and [S]", "confirmed Fri 6 Nov"),
      row([2, 2, 2], "initial rates", "used the rate at the end", "confirmed Thu 5 Nov"),
      row([2, 2, 2], "enzyme units", "mixed up mol and µmol", "confirmed Wed 4 Nov"),
    ],
    untested: [
      row([0, 0, 0], "non-competitive inhibition", "lower Vmax, same Km", "–"),
      row([0, 0, 0], "pH and activity", "the optimum and its causes", "–"),
    ],
    scores: [
      { paper: "Diagnostic", date: "2026-10-28", score: "15/30", sure_right: null, sure_total: null },
      { paper: "Paper A", date: "2026-11-04", score: "20/30", sure_right: 8, sure_total: 11 },
      { paper: "Paper B", date: "2026-11-08", score: "24/30", sure_right: 9, sure_total: 10 },
    ],
    helped: ["PS3 Q4 to Q6 model answers"],
    helped_count: 3,
    unreadable: 0,
    checks_unknown: false,
    proofs: proofs(15 + (sittingsDone > 0 ? 1 : 0), 3, 11),
    level_unknown: false,
    today: { day: isoDay(0), answered: markedToday, started: 2, proofs: sittingsDone > 0 ? 1 : 0, due_tomorrow: 3 },
    message: null,
  };
}

const earlier: Turn[] = [
  ["Teach me PS3 Q3.", "Before I explain: what do you think the line's y-intercept stands for?"],
  ["Vmax?", "Close. Is it Vmax itself, or something made from it? Write the rate equation first."],
  ["v = Vmax[S]/(Km + [S])", "Yes. Now take the reciprocal of both sides. What do you get?"],
  ["1/v = Km/(Vmax[S]) + 1/Vmax", "Right. Which part is the slope, and which part is left when $1/[S] = 0$?"],
  ["the slope is Km/Vmax", "Correct: the slope is $K_m / V_{max}$. Next, what is left when $1/[S] = 0$?"],
  ["1/Vmax", "Yes, $1/V_{max}$."],
  [
    "ok so what is Vmax",
    "The line meets the y-axis where $1/[S] = 0$, so the intercept is $1/V_{max}$, not $V_{max}$.\n\nThe slide that draws it is [Week 4 slides, p. 12](Study/midterm-1/week4-slides.pdf#page=12).\n\n**Task.** The line meets the y-axis at $0.25$ s/µM. What is $V_{max}$, with its units?",
  ],
].map(([learner, tutor], index) => ({ session: "demo", ms: Date.now() - (7 - index) * 60_000, learner, tutor, interrupted: false }));

const REPLY =
  "Right: $V_{max} = 1 / 0.25 = 4$ µM/s. The intercept is the reciprocal of the top speed, because at very high [S] the enzyme runs at $V_{max}$.\n\n$$\\frac{1}{v} = \\frac{K_m}{V_{max}} \\cdot \\frac{1}{[S]} + \\frac{1}{V_{max}}$$\n\n**Task.** In one sentence: why does the intercept sit where $1/[S] = 0$?";

const LOST = "One smaller case first. Take the reciprocal of one number.\n\n**Task.** $1 / 0.25 = {?}$";

const DIAGRAM =
  "Here is the reading as a picture.\n\n```mermaid\ngraph LR\n  A[rate equation] --> B[take reciprocals]\n  B --> C{is 1/S zero?}\n  C -- yes --> D[intercept is 1 over Vmax]\n  C -- no --> E[slope is Km over Vmax]\n  D --> F[Vmax]\n  E --> G[Km]\n  F --> G\n```\n\n| 1/[S] | 1/v |\n|---|---|\n| 0 | 0.25 |\n| 1 | 0.75 |\n| 2 | 1.25 |\n| 4 | 2.25 |\n\n**Task.** Which column would change if the enzyme were twice as fast?";

const SLIDE_LINES = [
  "Enzymes speed up reactions",
  "The rate equation",
  "Michaelis and Menten",
  "Initial rates",
  "Saturation",
  "Vmax and Km",
  "Fitting a curve by eye",
  "Why straight lines help",
  "Taking reciprocals",
  "Reciprocals of both sides",
  "The Lineweaver-Burk plot",
  "1/v against 1/[S]: the intercept is 1/Vmax",
  "The slope is Km / Vmax",
  "Worked example",
];
const SLIDES = () => tinyPdf(SLIDE_LINES);

/** What the core would say of a cited file, for the demo's made-up course. */
function demoSource(path: string): Source {
  const name = path.split("/").pop() ?? path;
  if (/key|solution|answer/i.test(path)) throw new Error("That looks like an answer key, so the window leaves it closed.");
  if (/\.pdf$/i.test(path)) return { name, kind: "pdf", text: null, truncated: false, image: null };
  if (/\.(md|txt)$/i.test(path)) {
    return { name, kind: "text", text: "# Week 4\n\nThe intercept of the Lineweaver-Burk line is 1/Vmax.\nThe slope is Km/Vmax.", truncated: false, image: null };
  }
  throw new Error("The window shows PDFs, text and images beside a lesson. Other files stay closed.");
}

/** What adding these made-up files would say, worded as the core words it. */
function material(paths: string[], pasted = false): Added {
  const names = paths.map((path) => path.split("/").pop() ?? path);
  const keys = names.filter((name) => /key|solution|answer/i.test(name) && !/tokens?/i.test(name));
  const shown = names.filter((name) => !keys.includes(name));
  const lines: string[] = [];
  if (shown.length === 1) lines.push(pasted ? `Saved your pasted text as "${shown[0]}".` : `Added "${shown[0]}" to Study/materials.`);
  else if (shown.length > 1) lines.push(`Added ${shown.length} files to Study/materials.`);
  if (keys.length > 0) lines.push(`${keys.length} ${keys.length === 1 ? "file looks" : "files look"} like an answer ${keys.length === 1 ? "key" : "keys"}. ${keys.length === 1 ? "It is" : "They are"} added, and this window keeps ${keys.length === 1 ? "it" : "them"} closed.`);
  const trouble = shown.flatMap((name) => {
    if (/scan/i.test(name)) return [`"${name}" is a scan with no text, so its text cannot be searched.`];
    if (/tokens?/i.test(name)) return [`Search leaves out "${name}" because its name has the word "tokens". Rename it and add it again.`];
    if (/\.pptx?$/i.test(name)) return [`Search does not read .pptx files, so "${name}" cannot be searched. A PDF of it can be.`];
    return [];
  });
  if (shown.length > 0) lines.push(...(trouble.length > 0 ? trouble : [shown.length === 1 ? "Search can read it." : "Search can read all of it."]));
  return { saved: shown.map((name) => `Study/materials/${name}`), keys: keys.length, lines };
}

export function demoCore(): Core {
  const handlers = new Set<(event: AgentEvent) => void>();
  const emit = (event: AgentEvent) => handlers.forEach((handler) => handler(event));
  let stopped = false;
  let waiting: ((value: string) => void) | null = null;

  const write = async (text: string, hold = false) => {
    emit({ type: "turn_started" });
    const pieces = text.match(/\S+\s*/g) ?? [];
    for (const piece of pieces) {
      if (stopped) break;
      emit({ type: "text_delta", text: piece });
      await wait(hold ? 400 : 14);
    }
    if (!stopped) emit({ type: "text_done", text });
    emit({ type: "turn_ended", ok: true, interrupted: stopped, error: null });
  };

  const reply = async (text: string) => {
    stopped = false;
    emit({ type: "ready", session_id: "demo", problems: [], plugin_missing: false });
    const lower = text.toLowerCase();
    await wait(250);
    if (lower.includes("lost")) return write(LOST);
    if (lower.includes("hint")) return write("Look at where the line meets the $1/v$ axis. What is $1/[S]$ there?");
    if (lower.includes("don't know")) {
      emit({ type: "working", doing: null });
      emit({
        type: "question",
        id: "q1",
        questions: [
          {
            question: "Which of these was in your head?",
            header: null,
            multi: false,
            options: [
              { label: "the intercept is Vmax", description: null },
              { label: "the intercept is 1/Vmax", description: null },
              { label: "not sure what the intercept is", description: null },
            ],
          },
        ],
      });
      const chosen = await new Promise<string>((done) => (waiting = done));
      return write(`You said: ${chosen}. A short repair now, and this returns tomorrow as a question.\n\n**Task.** At $1/[S] = 0$, what is $1/v$?`);
    }
    if (lower.includes("permission")) {
      emit({ type: "working", doing: "building the papers" });
      emit({ type: "permission", id: "p1", title: "Run a command", detail: "tectonic Study/midterm-1/papers/PaperC.tex", can_always: true });
      const decision = await new Promise<string>((done) => (waiting = done));
      return write(decision === "refuse" ? "No problem. I'll leave the paper unbuilt for now." : "Paper C is built. Sit it when you are ready.");
    }
    if (lower.includes("(source)")) {
      return write("Right, $V_{max} = 4$ µM/s. Look at the slide again while I set the next step: [Week 4 slides, p. 12](Study/midterm-1/week4-slides.pdf#page=12).");
    }
    if (lower.includes("(key)")) {
      return write("The marking guide for this is [the answer key](Study/midterm-1/papers/PaperB-key.pdf).");
    }
    if (lower.includes("diagram")) return write(DIAGRAM);
    if (lower.includes("slow")) {
      emit({ type: "working", doing: "writing paper 2" });
      await wait(30_000);
      emit({ type: "working", doing: "writing paper 3" });
      await wait(30_000);
      return write("Done.");
    }
    if (lower.includes("stream")) return write(REPLY, true);
    if (lower.includes("i'm back") || lower.includes("no checks")) {
      return write(
        lower.includes("no checks")
          ? "No checks now; they stay due.\n\nYou stopped at step 3. The line meets the y-axis where $1/[S] = 0$.\n\n**Task.** The line meets the y-axis at $0.25$ s/µM. What is $V_{max}$, with its units?"
          : "Welcome back. Two quick checks first, then step 3. The slide behind them is [Week 4 slides, p. 12](Study/midterm-1/week4-slides.pdf#page=12).\n\n**Check 1 of 2.** A rate doubles when [S] doubles. Is [S] well below or well above Km? One line, with the reason.",
      );
    }
    if (lower.includes("limit")) {
      emit({ type: "notice", kind: "usage_limit", text: "You reached your Claude usage limit. It resets at 15:00 on Tue 10 Nov." });
      emit({ type: "turn_ended", ok: false, interrupted: false, error: null });
      return;
    }
    return write(REPLY);
  };

  return {
    // The window opens on the library. Every other address opens the demo course first.
    initialFolder: async () => (/^#?(library|welcome|setup)?(:|$)/.test(location.hash) ? null : "/Users/you/Study/Demo-Course"),
    recent: async () => library().map((each) => each.folder),
    library: async () => {
      await wait(120);
      return library();
    },
    forgetFolder: async (path) => {
      forgotten = [...forgotten, path];
    },
    findFolders: async () => {
      await wait(900);
      const added = found ? 0 : 1;
      found = true;
      return added;
    },
    activity: async () => activity(),
    pickFolder: async () => "/Users/you/Study/Demo-Course",
    pickFiles: async () => ["/Users/you/Desktop/IMG_0412.heic"],
    pickMaterial: async () => ["/Users/you/Desktop/Week 4 slides.pdf"],
    pickNewFolder: async () => "/Users/you/Study/New-Course",
    openFolder: async (path) => {
      const name = path.split("/").filter(Boolean).pop() ?? "Demo-Course";
      if (name === "Demo-Course") return home();
      const picked = library().find((each) => each.folder === path);
      if (picked?.state === "not_set_up" || picked?.state === "empty") variant = "new";
      else variant = "quiet";
      return { ...home(), folder: path, name, exam: picked?.exam ?? null, papers: [] };
    },
    newFolder: async () => {
      variant = "new";
      return { ...home(), folder: "/Users/you/Study/New-Course", name: "New-Course" };
    },
    home: async () => home(),
    setUp: async () => {
      variant = "quiet";
      return { ...home(), state: "empty", last: null, exam: null, papers: [] };
    },
    upgradeFolder: async () => {
      await wait(600);
      variant = "ready";
      return home();
    },
    setup: async () => demoSetup(),
    replaceRuntime: async () => {
      setupVariant = "";
      return { ...runtimeSetup(), installed_now: true };
    },
    keepRuntime: async () => ({
      state: "kept",
      installed: program("0.2.0"),
      bundled: program("0.3.0+4f1c2ad9e0b7"),
      installed_now: false,
      message:
        "Lessons use the Magnesium already on this Mac, version 0.2.0. They teach with that version's rules, and it cannot open a folder that a newer Magnesium has upgraded.",
    }),
    account: async () => account(),
    setPayment: async (chosen) => {
      payment = chosen;
      return account();
    },
    signIn: async () => {
      window.setTimeout(() => (signedIn = true), 3000);
    },
    about: async () => ({ version: "0.3.0-rc.1", commit: "4f1c2ad9e0b7" }),
    // A conversation stays on the engine it began on. Another pick starts a new one.
    openSession: async (resume): Promise<Opened> => {
      const carries = resume && conversationEngine === engineChosen;
      conversationEngine = engineChosen;
      return { resumed: carries, turns: carries && variant !== "new" ? earlier : [], engine: engineChosen };
    },
    engine: async () => ({ chosen: engineChosen, conversation: conversationEngine }),
    setEngine: async (chosen) => {
      engineChosen = chosen;
      return { chosen: engineChosen, conversation: conversationEngine };
    },
    send: async (text) => {
      if (marking) markedToday += 1;
      void reply(text);
    },
    sendNewLesson: async (goal) => void reply(goal),
    sendShown: async (shown) => void reply(shown),
    interrupt: async () => {
      stopped = true;
    },
    decide: async (_id, decision) => waiting?.(decision),
    answer: async (_id, answers) => waiting?.(answers[0]?.chosen.join(", ") ?? ""),
    // A sitting with no answer marked earns no proof and makes no study day.
    stop: async (line) => {
      stoppedAt = line;
      if (!marking) return;
      studiedToday = true;
      sittingsDone += 1;
    },
    rail: async () => (variant === "new" ? { goal: null, steps: [], next_action: null } : rail),
    progress: async () => progress(),
    setBlockMinutes: async (minutes) => {
      blockMinutes = minutes;
    },
    attach: async () => "Study/handed-in/working/2026-11-10/IMG_0412.jpg",
    addMaterial: async (paths) => {
      await wait(500);
      return material(paths);
    },
    addPastedText: async () => {
      await wait(300);
      return material(["Pasted text 2026-11-11 14-03-22.txt"], true);
    },
    onDrop: async (handler, hover) => {
      // A browser drops a file with a name and no path, which is enough to show the screens.
      const over = (event: DragEvent) => {
        if (!event.dataTransfer?.types.includes("Files")) return;
        event.preventDefault();
        hover(true);
      };
      const leave = () => hover(false);
      const drop = (event: DragEvent) => {
        event.preventDefault();
        hover(false);
        const names = Array.from(event.dataTransfer?.files ?? []).map((file) => `/Users/you/Desktop/${file.name}`);
        if (names.length > 0) handler(names);
      };
      window.addEventListener("dragover", over);
      window.addEventListener("dragleave", leave);
      window.addEventListener("drop", drop);
      return () => {
        window.removeEventListener("dragover", over);
        window.removeEventListener("dragleave", leave);
        window.removeEventListener("drop", drop);
      };
    },
    source: async (path) => demoSource(path),
    sourcePdf: async () => SLIDES().buffer as ArrayBuffer,
    openFile: async (path) => {
      if (/key|solution/i.test(path)) throw new Error("That looks like an answer key, so the window leaves it closed.");
    },
    openUrl: async () => {},
    image: async () => {
      throw new Error("the demo has no files");
    },
    paperStart: async (paper, minutes) => {
      sitting = { target: paper.target, name: paper.name, path: paper.path, minutes, started_ms: Date.now() - (12 * 60 + 48) * 1000 };
      return sitting;
    },
    paperEnd: async (files) => {
      const name = sitting?.name ?? "the paper";
      sitting = null;
      return files.length > 0 ? { shown: `Mark my ${name}. I used 37 of 50 minutes.`, sent: `Mark my ${name}.` } : null;
    },
    onEvent: async (handler) => {
      handlers.add(handler);
      return () => handlers.delete(handler);
    },
    selftest: async () => false,
    selftestReport: async () => {},
  };
}

/** Choose the demo's data before the page first asks for it. */
export function chooseVariant(): void {
  const [screen, state] = location.hash.slice(1).split(":");
  if (["new", "quiet", "broken", "upgrade"].includes(state)) variant = state;
  if (screen === "library" && state === "empty") shelf = "empty";
  // The end of a sitting shows today's day being earned.
  if (screen === "summary") studiedToday = false;
  if (screen === "summary" && state === "unmarked") marking = false;
  // Magnesium picked, with the demo course's conversation begun on Claude Code.
  if (state === "magnesium") engineChosen = "direct";
  if (screen === "welcome" && state) {
    setupVariant = state;
    signedIn = state !== "signed-out";
  }
  if (location.hash.startsWith("#paper:running")) {
    sitting = { target: "midterm-1", name: "Paper B", path: paperB.path, minutes: 50, started_ms: Date.now() - (12 * 60 + 48) * 1000 };
  }
}
chooseVariant();

/**
 * Put the page in the state its address names, such as #session:lost or
 * #progress. Used to photograph each screen.
 */
export async function startFromAddress(actions: Actions): Promise<void> {
  const [screen, state] = (location.hash.slice(1) || "library").split(":");
  const quiet = async () => {
    for (let n = 0; n < 200 && document.querySelector(".app")?.getAttribute("data-busy") === "yes"; n++) await wait(50);
  };
  await wait(50);
  if (screen === "library" && state === "lesson") {
    // A lesson open behind the library: the engine choice is disabled.
    await actions.continueFolder("/Users/you/Study/Demo-Course");
    await wait(300);
    await quiet();
    actions.showLibrary();
    return;
  }
  if (screen === "library" && state === "continue") {
    // Enter on the main card goes straight into its lesson.
    await wait(300);
    document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    return;
  }
  if (screen === "session" || screen === "summary") {
    if (state === "fresh") {
      await actions.newLesson("teach me enzyme kinetics");
      return;
    }
    await actions.continueLesson(false);
    // Wait for the opening reply to finish before saying anything more.
    await wait(300);
    await quiet();
    if (screen === "summary") {
      await actions.say("Vmax is 4 µM/s");
      await wait(300);
      await quiet();
      await actions.stop("PS3 Q3, step 4: what the slope gives.");
      return;
    }
    const words: Record<string, string> = {
      lost: "I'm lost",
      idk: "I don't know",
      hint: "Can I have a hint?",
      permission: "please build paper C (permission)",
      diagram: "show me a diagram",
      stream: "Vmax is 4 µM/s (stream)",
      slow: "build my plan (slow)",
      limit: "one more (limit)",
      answer: "Vmax is 4 µM/s",
      source: "Vmax is 4 µM/s (source)",
      key: "show me the key (key)",
      math: "$V_{max} = \\frac{1}{0.25} = 4$ µM/s",
    };
    if (state && words[state]) await actions.say(words[state]);
    if (state === "source" || state === "key") {
      await wait(300);
      await quiet();
      if (state === "source") document.querySelector<HTMLButtonElement>('[data-role="source-chip"]')?.click();
      else document.querySelector<HTMLAnchorElement>(".tutor a.source")?.click();
    }
    if (state === "material") {
      await actions.addMaterial(["/Users/you/Desktop/Week 4 slides.pdf", "/Users/you/Desktop/Scanned lecture notes.pdf"]);
    }
    if (state === "drop") actions.setDragging(true);
    if (state === "stop") document.querySelector<HTMLButtonElement>('[data-role="end"]')?.click();
    if (state === "earlier") document.querySelector<HTMLButtonElement>('[data-role="earlier"]')?.click();
    if (state === "plan") document.querySelector<HTMLButtonElement>('[data-role="track"]')?.click();
    if (state === "typing") {
      // What the answer box looks like with mathematics typed in it.
      const box = document.querySelector<HTMLTextAreaElement>(".composer textarea");
      if (box) {
        const set = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")?.set;
        set?.call(box, "The intercept is $\\frac{1}{V_{max}}$, so $V_{max} = 4$");
        box.dispatchEvent(new Event("input", { bubbles: true }));
      }
    }
  } else if (screen === "progress") {
    await actions.openProgress(null);
  } else if (screen === "paper") {
    actions.openPaper(null);
  } else if (screen === "course" && state === "ask") {
    actions.setAsking(true);
  } else if (screen === "course" && state === "material") {
    await actions.addMaterial([
      "/Users/you/Desktop/Week 4 slides.pdf",
      "/Users/you/Desktop/Scanned lecture notes.pdf",
      "/Users/you/Desktop/api tokens.md",
      "/Users/you/Desktop/Week 4 deck.pptx",
      "/Users/you/Desktop/Midterm answer key.pdf",
    ]);
  } else if (screen === "course" && state === "drop") {
    actions.setDragging(true);
  } else if (screen === "course" && state === "paste") {
    actions.pasteText("Bayes' rule: the posterior is proportional to the prior times the likelihood.");
  } else if (screen === "palette") {
    actions.setPalette(true);
  } else if (screen === "setup" || (screen === "welcome" && !state)) {
    actions.openSetup();
  }
}
