// A stand-in core for a plain browser, with made-up data. It exists so the
// screens can be worked on and photographed without a window or an agent.
// Nothing here ships a real course, a real grade or a real learner's words.

import type { Actions } from "./App";
import type { Account, AgentEvent, Core, Home, Opened, Paper, PaperSitting, Payment, Progress, Rail, Row, RuntimeSetup, Setup, Turn } from "./types";

const wait = (ms: number) => new Promise((done) => window.setTimeout(done, ms));

const paperB: Paper = { target: "midterm-1", name: "Paper B", path: "Study/midterm-1/papers/PaperB.pdf", minutes: 50, sat: false };
const paperA: Paper = { target: "midterm-1", name: "Paper A", path: "Study/midterm-1/papers/PaperA.pdf", minutes: 50, sat: true };

let variant = "ready";
let sitting: PaperSitting | null = null;
let blockMinutes = 50;
let progressView = "A";
let stoppedAt: string | null = "PS3 Q3, step 3: what the y-intercept gives. The line through 0.25 is next.";

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
let setupVariant = "";
let payment: Payment = "plan";
let signedIn = true;
const program = (version: string) => ({ shown_path: "~/.local/bin/magnesium", version });

function runtimeSetup(): RuntimeSetup {
  const bundled = program("0.3.0+4f1c2ad9e0b7");
  if (setupVariant === "replace") {
    return { state: "different", installed: program("0.2.0"), bundled, installed_now: false, message: null };
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

const row = (dots: [number, number, number], name: string, what: string, note: string): Row => ({ dots, name, what, note });

function progress(): Progress {
  return {
    targets: [
      { target: "midterm-1", title: "Midterm 1", date: "2026-11-17", day: "Tue 17 Nov", days_left: 6 },
      { target: "final", title: "Final", date: "2026-12-15", day: "Tue 15 Dec", days_left: 35 },
    ],
    exam: { target: "midterm-1", title: "Midterm 1", date: "2026-11-17", day: "Tue 17 Nov", days_left: 6 },
    in_progress: [
      row([2, 0, 0], "intercept and Vmax", "took the intercept as Vmax", "due today"),
      row([1, 1, 0], "slope and Km", "read the slope as Km · found before the exam, restarted Sun 8 Nov", "due today"),
      row([2, 2, 0], "units of a rate", "gave Vmax in µM", "next Sat 14 Nov"),
      row([2, 0, 0], "competitive inhibition", "moved Vmax, not Km", "next Thu 12 Nov"),
      row([0, 0, 0], "Hill coefficient", "read n from the wrong axis", "repair first"),
    ],
    confirmed: [
      row([2, 2, 2], "rate equation", "swapped Km and [S]", "confirmed Fri 6 Nov"),
      row([2, 2, 2], "initial rates", "used the rate at the end", "confirmed Thu 5 Nov"),
      row([2, 2, 2], "enzyme units", "mixed up mol and µmol", "confirmed Wed 4 Nov"),
    ],
    untested: [
      row([0, 0, 0], "allosteric enzymes", "sigmoid rate curves", "–"),
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
    view: progressView,
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
    "The line meets the y-axis where $1/[S] = 0$, so the intercept is $1/V_{max}$, not $V_{max}$.\n\n**Task.** The line meets the y-axis at $0.25$ s/µM. What is $V_{max}$, with its units?",
  ],
].map(([learner, tutor], index) => ({ session: "demo", ms: Date.now() - (7 - index) * 60_000, learner, tutor, interrupted: false }));

const REPLY =
  "Right: $V_{max} = 1 / 0.25 = 4$ µM/s. The intercept is the reciprocal of the top speed, because at very high [S] the enzyme runs at $V_{max}$.\n\n$$\\frac{1}{v} = \\frac{K_m}{V_{max}} \\cdot \\frac{1}{[S]} + \\frac{1}{V_{max}}$$\n\n**Task.** In one sentence: why does the intercept sit where $1/[S] = 0$?";

const LOST = "One smaller case first. Take the reciprocal of one number.\n\n**Task.** $1 / 0.25 = {?}$";

const DIAGRAM =
  "Here is the reading as a picture.\n\n```mermaid\ngraph LR\n  A[rate equation] --> B[take reciprocals]\n  B --> C{is 1/S zero?}\n  C -- yes --> D[intercept is 1 over Vmax]\n  C -- no --> E[slope is Km over Vmax]\n  D --> F[Vmax]\n  E --> G[Km]\n  F --> G\n```\n\n| 1/[S] | 1/v |\n|---|---|\n| 0 | 0.25 |\n| 1 | 0.75 |\n| 2 | 1.25 |\n| 4 | 2.25 |\n\n**Task.** Which column would change if the enzyme were twice as fast?";

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
    if (lower.includes("don't know")) {
      emit({ type: "working" });
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
      emit({ type: "working" });
      emit({ type: "permission", id: "p1", title: "Run a command", detail: "tectonic Study/midterm-1/papers/PaperC.tex", can_always: true });
      const decision = await new Promise<string>((done) => (waiting = done));
      return write(decision === "refuse" ? "No problem. I'll leave the paper unbuilt for now." : "Paper C is built. Sit it when you are ready.");
    }
    if (lower.includes("diagram")) return write(DIAGRAM);
    if (lower.includes("slow")) {
      emit({ type: "working" });
      await wait(60_000);
      return write("Done.");
    }
    if (lower.includes("stream")) return write(REPLY, true);
    if (lower.includes("i'm back") || lower.includes("no checks")) {
      return write(
        lower.includes("no checks")
          ? "No checks now; they stay due.\n\nYou stopped at step 3. The line meets the y-axis where $1/[S] = 0$.\n\n**Task.** The line meets the y-axis at $0.25$ s/µM. What is $V_{max}$, with its units?"
          : "Welcome back. Two quick checks first, then step 3.\n\n**Check 1 of 2.** A rate doubles when [S] doubles. Is [S] well below or well above Km? One line, with the reason.",
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
    initialFolder: async () => (location.hash.startsWith("#welcome") ? null : "/Users/you/Study/Demo-Course"),
    recent: async () => ["/Users/you/Study/Demo-Course", "/Users/you/Study/Another-Course"],
    pickFolder: async () => "/Users/you/Study/Demo-Course",
    pickFiles: async () => ["/Users/you/Desktop/IMG_0412.heic"],
    pickNewFolder: async () => "/Users/you/Study/New-Course",
    openFolder: async () => home(),
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
    openSession: async (resume): Promise<Opened> => ({ resumed: resume, turns: resume && variant !== "new" ? earlier : [] }),
    send: async (text) => void reply(text),
    sendNewLesson: async (goal) => void reply(goal),
    sendShown: async (shown) => void reply(shown),
    interrupt: async () => {
      stopped = true;
    },
    decide: async (_id, decision) => waiting?.(decision),
    answer: async (_id, answers) => waiting?.(answers[0]?.chosen.join(", ") ?? ""),
    stop: async (line) => {
      stoppedAt = line;
    },
    rail: async () => (variant === "new" ? { goal: null, steps: [], next_action: null } : rail),
    progress: async () => progress(),
    setProgressView: async (view) => {
      progressView = view;
    },
    setBlockMinutes: async (minutes) => {
      blockMinutes = minutes;
    },
    attach: async () => "Study/handed-in/working/2026-11-10/IMG_0412.jpg",
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
 * #progress:B. Used to photograph each screen.
 */
export async function startFromAddress(actions: Actions): Promise<void> {
  const [screen, state] = (location.hash.slice(1) || "entrance").split(":");
  await wait(50);
  if (screen === "session") {
    if (state === "fresh") {
      await actions.newLesson("teach me enzyme kinetics");
      return;
    }
    await actions.continueLesson(false);
    // Wait for the opening reply to finish before saying anything more.
    await wait(300);
    for (let n = 0; n < 100 && document.querySelector(".win")?.getAttribute("data-busy") === "yes"; n++) await wait(50);
    const words: Record<string, string> = {
      lost: "I'm lost",
      idk: "I don't know",
      permission: "please build paper C (permission)",
      diagram: "show me a diagram",
      stream: "Vmax is 4 µM/s (stream)",
      slow: "build my plan (slow)",
      limit: "one more (limit)",
      answer: "Vmax is 4 µM/s",
    };
    if (state && words[state]) await actions.say(words[state]);
    if (state === "stop") document.querySelector<HTMLButtonElement>(".clock .btn")?.click();
    if (state === "earlier") document.querySelector<HTMLButtonElement>(".fold")?.click();
  } else if (screen === "progress") {
    await actions.openProgress(null);
    if (state === "B" || state === "C") actions.setProgressView(state);
  } else if (screen === "paper") {
    actions.openPaper(null);
  } else if (screen === "entrance" && state === "ask") {
    actions.setAsking(true);
  } else if (screen === "palette") {
    actions.setPalette(true);
  }
}
