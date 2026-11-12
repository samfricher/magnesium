// A stand-in core for a plain browser, with made-up data, so the surface can
// be worked on and photographed without a window or an agent. Nothing here is
// a real course, a real grade or a real learner's words.
//
// The address picks a state. The five scenarios of the evidence contract:
//   #s1-cold  #s1-cold-first  #s2-start  #s2-answer  #s3-before  #s3-after
//   #s4-offplan  #s4-return  #s5-week  #s5-week-next  #narrow-lesson  #settings
// and the rest of the window:
//   #plan  #commands  #courses  #material  #work  #help  #source  #reopen
//   #working  #permission  #new-course  #paper  #upgrade
//   #setup-signed-out  #setup-no-claude  #setup-replace  #setup-too-old

import type { Actions } from "./App";
import { demoAfterTurn, demoBaseReply, demoChangePlan, demoMarked, demoMayMark, demoPlan } from "./demoBase";
import { tinyPdf } from "./demoPdf";
import type { Account, Added, AgentEvent, Card, Core, Engine, Home, Intent, Mark, Opened, Paper, PaperSitting, Payment, Progress, RuntimeSetup, Setup, Source, Turn } from "./types";

const wait = (ms: number) => new Promise((done) => window.setTimeout(done, ms));

const address = () => decodeURIComponent(window.location.hash.replace(/^#/, ""));
const state = address();
const has = (...names: string[]) => names.includes(state);

const FOLDER = "/Users/you/Study/Biochemistry";
const GOAL = "Reading $V_{max}$ and $K_m$ from a Lineweaver–Burk plot";
const paperB: Paper = { target: "midterm-1", name: "Paper B", path: "Study/midterm-1/papers/Paper B.pdf", minutes: 50, sat: false };
const paperA: Paper = { target: "midterm-1", name: "Paper A", path: "Study/midterm-1/papers/Paper A.pdf", minutes: 50, sat: true };

// What the addresses change.
const week = has("s5-week", "s5-week-next");
const firstOpen = has("s1-cold-first");
let signedIn = !has("setup-signed-out");
let payment: Payment = "plan";
let engineChosen: Engine = "direct";
let sitting: PaperSitting | null = has("paper") ? { target: "midterm-1", name: "Paper B", path: paperB.path, minutes: 50, started_ms: Date.now() - 12 * 60_000 } : null;
let upgraded = !has("upgrade");
let folders = firstOpen ? [] : [FOLDER, "/Users/you/Study/Linear-Algebra", "/Users/you/Study/Statistics"];
let open: string | null = null;
let stoppedAt: string | null = week ? "The slope of the same line is 0.5 s. What is $K_m$?" : "step 3, what the y-intercept gives";
let materialNames = ["Week 4 slides.pdf", "enzyme-kinetics-notes.md", "Problem set 3.pdf"];
const holdStream = has("s3-before");

if (week) demoChangePlan();

// The record's marks, oldest first. An answer sent in the demo adds one a moment later.
const marks: Mark[] = [
  { id: "m1", at: "2026-11-06 18:20", label: "units of a rate", mark: "right", help: "none", cold: true, paper: null },
  { id: "m2", at: "2026-11-09 18:26", label: "the slope gives $K_m / V_{max}$", mark: "wrong", help: "none", cold: true, paper: null },
  { id: "m3", at: "2026-11-10 17:02", label: "taking reciprocals of the rate equation", mark: "right", help: "hint", cold: false, paper: null },
  { id: "m4", at: "2026-11-05 18:40", label: "reading $V_{max}$ from the y-intercept", mark: "wrong", help: "none", cold: true, paper: null },
];
let markCount = marks.length;
let helpAsked = false;

// The open task in the demo lesson: the y-intercept, then the slope.
let task: "vmax" | "km" = week ? "km" : "vmax";

// Tasks are written in prose, as the tutor on Magnesium writes them; the
// engine names each one in its record, and says so with open_task.
const VMAX_TASK = "The line meets the y-axis at $0.25$ s/µM. What is $V_{max}$, with its units?";
const KM_TASK = "The slope of the same line is $0.5$ s. What is $K_m$?";
const CHECK_TASK = "A line meets the y-axis at $0.2$ s/µM. What is $V_{max}$?";
const TASKS = [VMAX_TASK, KM_TASK, CHECK_TASK, "A second line meets the y-axis at $0.5$ s/µM. What is its $V_{max}$?"];

const OPENING =
  "You had the reciprocal form last time:\n\n$$\\frac{1}{v} = \\frac{K_m}{V_{max}}\\cdot\\frac{1}{[S]} + \\frac{1}{V_{max}}$$\n\n" +
  "It is a straight line in $\\frac{1}{[S]}$, so each part of the line means something. Where it meets the y-axis, $\\frac{1}{[S]} = 0$, and what is left is $\\frac{1}{V_{max}}$ (see [Week 4 slides, p. 12](Study/materials/Week%204%20slides.pdf#page=2)).\n\n" +
  VMAX_TASK;

const WEEK_OPENING = `Quick check 1: ${CHECK_TASK}`;

const RIGHT =
  "Yes. The intercept is $\\frac{1}{V_{max}}$, so $V_{max} = \\frac{1}{0.25} = 4$ µM/s; the units come from taking the reciprocal of s/µM.\n\n" +
  "The slope is the other half of the line. It is $\\frac{K_m}{V_{max}}$, so it carries both constants at once.\n\n" +
  KM_TASK;

const WRONG =
  "Not quite. The intercept is $\\frac{1}{V_{max}}$, not $V_{max}$ itself, so take the reciprocal: $V_{max} = \\frac{1}{0.25}$.\n\n" + KM_TASK;

const KM_RIGHT = "Yes: $K_m = 0.5 \\times 4 = 2$ µM. You read both constants off one line.\n\nA second line meets the y-axis at $0.5$ s/µM. What is its $V_{max}$?";

const HINT = "The slope is $\\frac{K_m}{V_{max}}$, and you have $V_{max}$ already. What do you multiply the slope by?";

const LOST = "Let's find where it slipped. When you look at the line, which part do you read first: where it crosses the y-axis, or how steep it is?";

const BACK_TO = () => `Back to it.\n\n${task === "km" ? KM_TASK : VMAX_TASK}`;

function home(): Home {
  if (open === null) throw new Error("No course is open.");
  if (open !== FOLDER) {
    const name = open.split("/").pop() ?? open;
    return {
      folder: open,
      name,
      state: name === "Statistics" ? "empty" : "ready",
      message: null,
      exam: null,
      last: name === "Statistics" ? null : { days_ago: 1, goal: "Diagonalising a matrix with a repeated eigenvalue", stopped_at: null, next_action: "Find the eigenvectors for $\\lambda = 2$." },
      checks_due: 0,
      redos_due: 0,
      first_redo: null,
      papers: [],
      can_resume: name !== "Statistics",
      sitting: null,
      block_minutes: 50,
    };
  }
  const plan = demoPlan();
  const day = (iso: string) => new Date(`${iso}T12:00:00`).toLocaleDateString("en-GB", { weekday: "short", day: "numeric", month: "short" }).replace(",", "");
  return {
    folder: FOLDER,
    name: "Biochemistry",
    state: upgraded ? "ready" : "needs_upgrade",
    message: upgraded
      ? null
      : "An earlier version of Magnesium set this folder up. Upgrade it to study here with this version. Magnesium keeps a copy of your record in .magnesium/backups before it changes it.",
    exam: plan.exam_date ? { target: "midterm-1", title: "Midterm 1", date: plan.exam_date, day: day(plan.exam_date), days_left: 5 } : null,
    last: upgraded ? { days_ago: week ? 7 : has("reopen") ? 0 : 2, goal: GOAL, stopped_at: stoppedAt, next_action: "Find $V_{max}$ from the intercept 0.25." } : null,
    checks_due: week ? 1 : 0,
    redos_due: 0,
    first_redo: "Diagnostic Q4",
    papers: [paperB, paperA],
    can_resume: true,
    sitting,
    block_minutes: 50,
  };
}

const card = (folder: string, more: Partial<Card>): Card => ({
  folder,
  name: folder.split("/").pop() ?? folder,
  state: "ready",
  exam: null,
  goal: null,
  days_ago: null,
  proofs: { earned: 0, total: 0, proven: 0, skills: 0, level: 0, to_next: 0, levelled: false },
  due: 0,
  sitting: null,
  ...more,
});

function library(): Card[] {
  return folders.map((folder) =>
    folder === FOLDER
      ? card(folder, { goal: GOAL, days_ago: 2, sitting })
      : folder.endsWith("Linear-Algebra")
        ? card(folder, { goal: "Diagonalising a matrix with a repeated eigenvalue", days_ago: 1 })
        : card(folder, { state: "empty" }),
  );
}

function runtime(): RuntimeSetup {
  const program = (version: string) => ({ shown_path: "~/.local/bin/magnesium", version });
  const bundled = program("0.3.0+4f1c2ad9e0b7");
  if (has("setup-replace")) return { state: "different", installed: program("0.2.0"), bundled, installed_now: false, message: null };
  if (has("setup-too-old")) {
    return {
      state: "too_old",
      installed: program("0.2.0"),
      bundled,
      installed_now: false,
      message: "The magnesium program on this Mac, version 0.2.0, is older than this window needs. Replace it with the copy inside this app, version 0.3.0. Until then no lesson starts.",
    };
  }
  return { state: "ready", installed: bundled, bundled, installed_now: firstOpen, message: null };
}

function account(): Account {
  const base: Account = { name: "Claude Code", installed: true, signed_in: true, paid_with: "plan", detail: "your Claude Max plan", problem: null, install_page: null };
  if (has("setup-no-claude")) {
    return { ...base, installed: false, signed_in: false, paid_with: null, detail: null, problem: "Claude Code is not on this Mac, or the window cannot find it.", install_page: "https://code.claude.com/docs/en/setup" };
  }
  if (!signedIn) return { ...base, signed_in: false, paid_with: null, detail: null, problem: null };
  if (payment === "api_key") return { ...base, paid_with: "api_key", detail: "your Anthropic Console sign-in, per use" };
  return base;
}

const setup = (): Setup => ({ runtime: runtime(), account: account(), payment });

/** The conversation left, as the window keeps it: what `turns` reads at rest. */
function savedTurns(): Turn[] {
  if (open !== FOLDER || firstOpen) return [];
  const at = Date.now() - (week ? 7 : 2) * 86_400_000;
  const turn = (n: number, learner: string, tutor: string): Turn => ({ session: "demo", ms: at + n * 60_000, learner, tutor, interrupted: false });
  const turns = [
    turn(0, "I'm back. Let's continue.", "Last time ended at the rate equation. Take reciprocals of both sides and see what kind of line it gives.\n\n**Task.** Write $\\frac{1}{v}$ in terms of $\\frac{1}{[S]}$."),
    turn(1, "1/v = (K_m/V_max)(1/[S]) + 1/V_max", "Right, and that is the whole of the Lineweaver–Burk form: a straight line, slope $\\frac{K_m}{V_{max}}$, intercept $\\frac{1}{V_{max}}$."),
  ];
  if (has("reopen")) turns.push(turn(2, "I'm back. Let's continue. I stopped at: step 3, what the y-intercept gives", OPENING));
  if (week) {
    turns.push(
      turn(2, "I'm back. Let's continue. I stopped at: step 3, what the y-intercept gives", OPENING),
      turn(3, "V_max = 4 µM/s", RIGHT),
      turn(4, "I skipped three days and the midterm moved, change the plan", demoBaseReply("change the plan") ?? ""),
    );
  }
  return turns;
}

let emit: (event: AgentEvent) => void = () => {};
// The task open in the demo lesson, as the direct engine would say it after each turn.
let openTask: string | null = null;
let replying = false;
let holdRelease: (() => void) | null = null;

/** Write a reply as the tutor would: in pieces, then whole. */
async function reply(text: string, after?: () => void, hold = false) {
  replying = true;
  emit({ type: "turn_started" });
  await wait(has("working") ? 400 : 160);
  if (has("working")) {
    emit({ type: "working", doing: "searching your course material" });
    await new Promise<void>(() => {});
  }
  let shown = "";
  const pieces = text.match(/[\s\S]{1,14}/g) ?? [];
  for (let i = 0; i < pieces.length; i++) {
    if (hold && i === Math.floor(pieces.length * 0.55)) await new Promise<void>((done) => (holdRelease = done));
    shown += pieces[i];
    emit({ type: "text_delta", text: pieces[i] });
    await wait(8);
  }
  emit({ type: "text_done", text: shown });
  emit({ type: "turn_ended", ok: true, interrupted: false, error: null });
  replying = false;
  // The engine says which task is open, in the tutor's words.
  const posed = TASKS.find((each) => text.trim().endsWith(each)) ?? null;
  if (posed) openTask = posed;
  emit({ type: "open_task", task: openTask, posed: posed !== null });
  after?.();
}

function answerReply(text: string): { words: string; mark: Mark["mark"] | null } {
  const lower = text.toLowerCase().replace(/\s+/g, "");
  if (task === "vmax") {
    if (/=?4/.test(lower) && !lower.includes("0.25")) {
      task = "km";
      return { words: RIGHT, mark: "right" };
    }
    task = "km";
    return { words: WRONG, mark: "wrong" };
  }
  if (/2/.test(lower)) return { words: KM_RIGHT, mark: "right" };
  return { words: `Not quite: multiply the slope by $V_{max}$. $K_m = 0.5 \\times 4$.\n\n${KM_TASK}`, mark: "wrong" };
}

async function respond(text: string, intent: Intent | undefined) {
  const lower = text.toLowerCase();
  const base = demoBaseReply(text);
  if (lower.includes("into my plan")) {
    emit({ type: "turn_started" });
    emit({ type: "permission", id: "p1", title: "Change a file", detail: "Study/midterm-1/plan.md", can_always: true });
    return;
  }
  if (text.trim() === "Can I have a hint?") {
    helpAsked = true;
    return reply(HINT);
  }
  if (text.trim() === "I'm lost") {
    helpAsked = true;
    return reply(LOST);
  }
  if (text.trim() === "I don't know") {
    await reply(`That's fine. The slope is $\\frac{K_m}{V_{max}}$, so $K_m = 0.5 \\times 4 = 2$ µM.\n\n${KM_TASK.replace("0.5", "0.25")}`);
    return addMark("declined");
  }
  if (lower.includes("back to")) return reply(BACK_TO());
  if (base) return reply(base, () => demoAfterTurn(text, emit), holdStream && lower.includes("change the plan"));
  if (intent === "answer" && demoMayMark(text, intent)) {
    const { words, mark } = answerReply(text);
    await reply(words);
    if (mark) addMark(mark);
    return;
  }
  return reply(`You said: ${text}\n\nThat is talk, so nothing is marked. Say when you want to go on.`);
}

function addMark(mark: Mark["mark"]) {
  window.setTimeout(() => {
    markCount += 1;
    const row: Mark = {
      id: `m${markCount}`,
      at: new Date().toISOString().slice(0, 16).replace("T", " "),
      label: task === "km" ? "reading $V_{max}$ from the y-intercept" : "reading $K_m$ from the slope",
      mark,
      help: helpAsked ? "hint" : "none",
      cold: !helpAsked,
      paper: null,
    };
    helpAsked = false;
    marks.push(row);
    demoMarked(row, emit);
  }, 220);
}

export function demoCore(): Core {
  if (has("reopen")) {
    try {
      window.localStorage.setItem(
        "mg.place",
        JSON.stringify({
          course: FOLDER,
          by: { [FOLDER]: { draft: "", answerDraft: "the intercept is 1/V_max, so V_max =", line: "task", fromFoot: 0, inLesson: true, task: VMAX_TASK, seenMs: Date.now() - 20 * 60_000 } },
        }),
      );
    } catch {
      // A demo with no storage opens at rest.
    }
  }
  return {
    initialFolder: async () => (firstOpen ? null : FOLDER),
    recent: async () => [...folders],
    library: async () => library(),
    forgetFolder: async (path) => {
      folders = folders.filter((folder) => folder !== path);
    },
    findFolders: async () => 0,
    activity: async () => ({ today: "2026-11-12", days: [], run: 0, studied_today: false }),
    pickFolder: async () => FOLDER,
    pickFiles: async () => ["/Users/you/Desktop/working.jpg"],
    pickMaterial: async () => ["/Users/you/Downloads/Week 5 slides.pdf"],
    pickNewFolder: async () => null,
    openFolder: async (path) => {
      open = path;
      if (!folders.includes(path)) folders = [path, ...folders];
      return home();
    },
    newFolder: async (path) => {
      open = path;
      return home();
    },
    home: async () => home(),
    setUp: async () => home(),
    upgradeFolder: async () => {
      upgraded = true;
      return home();
    },
    setup: async () => setup(),
    replaceRuntime: async () => runtime(),
    keepRuntime: async () => runtime(),
    account: async () => account(),
    setPayment: async (chosen) => {
      payment = chosen;
      return account();
    },
    engine: async () => ({ chosen: engineChosen, conversation: engineChosen }),
    setEngine: async (chosen) => {
      engineChosen = chosen;
      return { chosen, conversation: null };
    },
    signIn: async () => {
      signedIn = true;
    },
    about: async () => ({ version: "0.3.0", commit: "4f1c2ad9e0b7" }),
    turns: async () => savedTurns(),
    openSession: async (resume): Promise<Opened> => ({ resumed: resume, turns: resume ? savedTurns() : [], engine: engineChosen }),
    send: async (text, intent) => {
      void respond(text, intent);
    },
    sendNewLesson: async (goal) => {
      if (/^(I'm back|No checks|Let's work on)/.test(goal)) void reply(week ? WEEK_OPENING : OPENING);
      else void respond(goal, "talk");
    },
    sendShown: async (shown) => {
      void reply(`Marking ${shown.slice(0, 40)}…`);
    },
    interrupt: async () => {
      if (replying) emit({ type: "turn_ended", ok: true, interrupted: true, error: null });
      replying = false;
    },
    decide: async () => {
      void reply("Thank you. The plan file is changed.");
    },
    answer: async () => {},
    stop: async (line) => {
      stoppedAt = line;
    },
    rail: async () => ({ goal: GOAL, steps: demoPlan().steps, next_action: null }),
    plan: async () => {
      // Another course has a plan of its own; this demo keeps only Biochemistry's.
      if (open !== FOLDER) return { target: null, title: null, exam_date: null, goal: null, steps: [], items: [], text: null, path: null, changed_ms: null };
      const plan = demoPlan();
      // The route follows the lesson: the slope is in hand once the intercept is done.
      const steps = task === "km" ? plan.steps.map((step, at) => ({ ...step, status: at < 3 ? "done" : at === 3 ? "doing" : "todo" })) : plan.steps;
      return { ...plan, goal: GOAL, steps };
    },
    progress: async (): Promise<Progress> => ({
      targets: [],
      exam: null,
      confirmed: [],
      in_progress: week
        ? [{ dots: [0, 0, 0], name: "reading $V_{max}$ from the y-intercept", what: "", note: "due today", due: true, next_day: null }]
        : [],
      untested: [],
      scores: [{ paper: "Paper A", date: "2026-11-07", score: "31/50", sure_right: null, sure_total: null }],
      helped: [],
      helped_count: 0,
      unreadable: 0,
      checks_unknown: false,
      proofs: { earned: 0, total: 0, proven: 0, skills: 0, level: 0, to_next: 0, levelled: false },
      level_unknown: true,
      today: null,
      message: null,
    }),
    setBlockMinutes: async () => {},
    attach: async (path) => `Study/handed-in/working/2026-11-12/${path.split("/").pop()}`,
    addMaterial: async (paths): Promise<Added> => {
      const names = paths.map((path) => path.split("/").pop() ?? path);
      materialNames = [...names, ...materialNames];
      return { saved: names.map((name) => `Study/materials/${name}`), keys: 0, lines: [`Added ${names.join(", ")} to the course material. The tutor's search can read it.`] };
    },
    addPastedText: async (): Promise<Added> => ({ saved: ["Study/materials/Pasted text 10.41.txt"], keys: 0, lines: ["Kept the pasted text with the course material."] }),
    materialList: async () => ({ names: materialNames, files: materialNames.length + 4, keys: 1 }),
    marks: async () => [...marks],
    onDrop: async () => () => {},
    source: async (path): Promise<Source> => {
      if (/key|solution/i.test(path)) throw new Error("That looks like an answer key, so the window does not open it.");
      return { name: path.split("/").pop() ?? path, kind: path.endsWith(".pdf") ? "pdf" : "text", text: path.endsWith(".pdf") ? null : "Notes.", truncated: false, image: null };
    },
    sourcePdf: async () => tinyPdf(["The rate equation", "Where the line meets the y-axis: 1/Vmax", "The slope: Km/Vmax"]).buffer as ArrayBuffer,
    openFile: async () => {},
    openUrl: async () => {},
    image: async () => "",
    paperStart: async (paper, minutes) => {
      sitting = { target: paper.target, name: paper.name, path: paper.path, minutes, started_ms: Date.now() };
      return sitting;
    },
    paperEnd: async (files) => {
      sitting = null;
      return files.length > 0 ? { shown: `Mark my Paper B. My work: ${files.join(", ")}`, sent: "Mark my Paper B." } : null;
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

/** Wait until the tutor has finished, and a moment for the record. */
async function settle(ms = 500) {
  await wait(120);
  while (replying) await wait(40);
  await wait(ms);
}

async function lesson(actions: Actions) {
  await actions.start();
  await settle();
}

async function answered(actions: Actions) {
  await lesson(actions);
  await actions.say("$V_{max}$ = 4 µM/s", "answer");
  await settle(700);
}

async function changed(actions: Actions, hold = false) {
  await answered(actions);
  actions.setLine("talk");
  await actions.say("I skipped three days and the midterm moved, change the plan", "talk");
  if (!hold) await settle(700);
  else await wait(500);
}

async function offPlan(actions: Actions) {
  await changed(actions);
  await actions.say("By the way, why do some courses use a Hanes–Woolf plot instead?", "talk");
  await settle();
}

/** Drive the surface to the state the address names, as a learner would. */
export async function startFromAddress(actions: Actions): Promise<void> {
  await wait(60);
  switch (state) {
    case "s2-start":
      return lesson(actions);
    case "s2-answer":
    case "narrow-lesson":
      return answered(actions);
    case "settings":
      await answered(actions);
      return actions.open("settings");
    case "help":
      await answered(actions);
      return actions.open("help");
    case "source":
      await lesson(actions);
      await actions.say("V_max = 4 µM/s", "answer");
      await settle();
      return actions.openSource({ path: "Study/materials/Week 4 slides.pdf", page: 2 });
    case "s3-before":
      return changed(actions, true);
    case "s3-after":
      return changed(actions);
    case "plan":
      await changed(actions);
      return actions.open("plan");
    case "s4-offplan":
      return offPlan(actions);
    case "s4-return":
      await offPlan(actions);
      await actions.say("OK, back to the task", "talk");
      return settle();
    case "s5-week-next":
      return lesson(actions);
    case "working":
      await actions.start();
      return;
    case "permission":
      await lesson(actions);
      actions.setLine("talk");
      await actions.say("Can you write the lab report into my plan for Friday?", "talk");
      return;
    case "commands":
      return actions.open("commands");
    case "courses":
      return actions.open("courses");
    case "material":
      return actions.open("material");
    case "work":
      await answered(actions);
      return actions.open("work");
    case "new-course":
      return actions.switchCourse("/Users/you/Study/Statistics");
    default:
      return;
  }
}

/** For the self-check's "hold the stream" case: let a held reply finish. */
export function releaseStream(): void {
  holdRelease?.();
}
