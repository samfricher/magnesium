// A stand-in core for a plain browser, with made-up data, so the window can
// be worked on and photographed without a shell or an agent. Nothing here is
// a real course, a real grade or a real learner's words.
//
// The address picks a state. The five scenarios of the evidence contract:
//   #s1-cold (Home)  #s1-cold-first  #s2-start  #s2-answer  #s3-before
//   #s3-after  #s4-offplan  #s4-return  #s5-week  #s5-week-next  #narrow-lesson
// and the new screens of the combined window:
//   #workspace  #material  #new-session  #new-session-start  #settings  #settings-colour  #settings-keys  #panel  #panel-changed  #attach
//   #tabs  #commands  #switch  #session-earlier  #source  #working
//   #permission  #permission-command  #permission-unknown  #model-unavailable
//   #paper  #paper-last  #paper-over  #paper-hours  #paper-days  #paper-long  #upgrade  #not-set-up  #help-row  #symbols
//   #line-stuck  #line-stuck-typing  #line-more  #line-more-typing  (the lesson's line: Stuck · More, each open)
//   #model-lesson-word  #model-lesson  #model-lesson-effort  #model-lesson-refused  (the model, mid-lesson)
//   #setup-signed-out  #setup-no-claude  #setup-replace  #setup-replace-newer  #setup-too-old

import type { Actions } from "./App";
import { demoAfterTurn, demoBaseReply, demoChangePlan, demoMarked, demoMayMark, demoPlan } from "./demoBase";
import { tinyPdf } from "./demoPdf";
import type { Account, Added, AgentEvent, Card, Core, Engine, Home, Intent, LessonInfo, Mark, ModelAt, ModelChoice, ModelOption, Opened, Paper, PaperSitting, Payment, Permissions, Pick, Progress, RuntimeSetup, Setup, Source, Turn, Workspace } from "./types";

const wait = (ms: number) => new Promise<void>((done) => window.setTimeout(done, ms));

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
let permissions: Permissions = "auto";
let engineChosen: Engine = "direct";
// The list Claude Code sends, as stand-in data in its own words. A new window has none kept and asks for it:
// "model-first-run" gets it a moment later, "model-asking" is still waiting, and "model-failed" is sent none ("model-list-failed" with its list open).
const ALL_EFFORTS = ["low", "medium", "high", "xhigh", "max"];
const MODEL_LIST: ModelOption[] = [
  { value: "default", resolved: "claude-opus-5-5", name: "Default (recommended)", description: "Use the default model (currently Opus 5.5)", efforts: ALL_EFFORTS, current: true },
  { value: "opus", resolved: "claude-opus-5-5", name: "Opus 5.5", description: "Best for everyday, complex tasks", efforts: ALL_EFFORTS, current: true },
  { value: "fable", resolved: "claude-fable-5-2", name: "Fable 5.2", description: "Most capable for your hardest and longest-running tasks", efforts: ALL_EFFORTS, current: true },
  { value: "sonnet", resolved: "claude-sonnet-5-5", name: "Sonnet 5.5", description: "Efficient for routine tasks", efforts: ALL_EFFORTS, current: true },
  { value: "haiku", resolved: "claude-haiku-5-5", name: "Haiku 5.5", description: "Fastest for quick answers", efforts: ["low", "medium", "high"], current: true },
  { value: "claude-fable-5-1", resolved: "claude-fable-5-1", name: "Fable 5.1", description: "Newer version available · select fable for Fable 5.2", efforts: ALL_EFFORTS, current: false },
  { value: "claude-opus-4-6", resolved: "claude-opus-4-6", name: "Opus 4.6", description: "Newer version available · select opus for Opus 5.5", efforts: ["low", "medium", "high", "max"], current: false },
  { value: "claude-haiku-4-5", resolved: "claude-haiku-4-5", name: "Haiku 4.5", description: "Newer version available · select haiku for Haiku 5.5", current: false },
];
// Asked without a lesson, the demo's agent sends its list, except in "model-failed", where it sends none.
let modelsNone = has("model-first-run", "model-list-first-run", "model-failed", "model-list-failed", "model-asking");
const modelsFailed = has("model-failed", "model-list-failed");
let pick: Pick | null = has("model-first-run", "model-list-first-run", "model-asking")
  ? null
  : has("model-not-offered")
    ? { model: "claude-fable-4", name: "Fable 4" }
    : has("model-effort", "record-unopened", "model-lesson", "model-lesson-effort", "model-lesson-word", "model-lesson-refused")
      ? { model: "claude-sonnet-5-5", name: "Sonnet 5.5", effort: "low" }
      : has("record-unopened-usual")
        ? { model: "claude-haiku-5-5", name: "Haiku 5.5" }
        : { model: "claude-fable-5-2", name: "Fable 5.2" };
// What each session is on, by its tab's session id or "live" before the agent named it. A session the demo starts
// is on the pick at the time; "model-lesson-refused" is one whose agent turns a change down.
const sessionsOn: Record<string, { model: string; effort?: string }> = {};
const onKey = (at: ModelAt) => at.session ?? (at.live ? "live" : null);
const sessionChoice = (at: ModelAt): ModelChoice => {
  const key = onKey(at);
  if (key && !sessionsOn[key] && pick) sessionsOn[key] = { model: pick.model, ...(pick.effort ? { effort: pick.effort } : {}) };
  const on = key ? sessionsOn[key] : undefined;
  return { ...modelChoice(), on: on ? { ...on, live: at.live } : null };
};
const modelChoice = (): ModelChoice => {
  const models = modelsNone ? [] : MODEL_LIST;
  const offered = models.length === 0 || pick === null || models.some((m) => (m.resolved ?? m.value) === pick!.model || m.value === pick!.model);
  return { models, failed: modelsFailed, pick, not_offered: !offered };
};
// #paper-over has run past its end, as a paper does when the window was closed and opened again later, and
// #paper-hours and #paper-days further; #paper-last is in its last minute; #paper-long has a long name.
const sittingAgo: Record<string, number> = { paper: 12, "paper-long": 12, "paper-last": 49.5, "paper-over": 64, "paper-hours": 50 + 190, "paper-days": 50 + 2 * 1440 + 30 };
let sitting: PaperSitting | null =
  state in sittingAgo
    ? {
        target: "midterm-1",
        name: has("paper-long") ? "Paper B, the full midterm from last year with every part of question four" : "Paper B",
        path: paperB.path,
        minutes: 50,
        started_ms: Date.now() - sittingAgo[state] * 60_000,
      }
    : null;
let upgraded = !has("upgrade");
let notSetUp = has("not-set-up");
let folders = firstOpen ? [] : [FOLDER, "/Users/you/Study/Linear-Algebra", "/Users/you/Study/Statistics"];
let open: string | null = FOLDER;
const DAY_MS = 86_400_000;
// When each folder was last opened, and its sessions: the current one, and an earlier one.
const openedMs: Record<string, number> = {
  [FOLDER]: Date.now() - (week ? 7 : 2) * DAY_MS,
  "/Users/you/Study/Linear-Algebra": Date.now() - (week ? 9 : 3) * DAY_MS,
  "/Users/you/Study/Statistics": Date.now() - 12 * DAY_MS,
};
const sessionsOf = (folder: string): LessonInfo[] => {
  if (firstOpen) return [];
  const at = openedMs[folder] ?? Date.now();
  const info = (session: string, title: string | null, opening: string, ms: number, current: boolean): LessonInfo => ({
    session,
    first_ms: ms - 40 * 60_000,
    last_ms: ms,
    turns: 6,
    opening,
    current,
    title,
    opened_ms: ms,
  });
  if (folder === FOLDER)
    return [
      info("demo", GOAL, "I'm back. Let's continue.", at, true),
      info("demo-0", null, "Let's work on taking reciprocals of the rate equation.", at - 3 * DAY_MS, false),
    ];
  if (folder.endsWith("Linear-Algebra")) return [info("la-1", "Diagonalising a matrix with a repeated eigenvalue", "Teach me diagonalising", at, true)];
  return [];
};
let stoppedAt: string | null = week ? "The slope of the same line is 0.5 s. What is $K_m$?" : "step 3, what the y-intercept gives";
let materialNames = ["Week 4 slides.pdf", "enzyme-kinetics-notes.md", "Problem set 3.pdf"];
const holdStream = has("s3-before");

if (week) demoChangePlan();

// The record's marks, oldest first. An answer sent in the demo adds one a moment later.
const marks: Mark[] = [
  { id: "m0", at: "2026-11-04 18:02", label: "what $V_{max}$ means", mark: "right", help: "none", cold: true, paper: null, pass: 3 },
  { id: "m1", at: "2026-11-06 18:20", label: "units of a rate", mark: "right", help: "none", cold: true, paper: null, pass: 1 },
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
const VMAX_TASK = "A Lineweaver-Burk line meets the y-axis at $0.25$ s/µM. What is $V_{max}$, with its units?";
const KM_TASK = "A Lineweaver-Burk line meets the y-axis at $0.25$ s/µM, so $V_{max} = 4$ µM/s, and its slope is $0.5$ s. What is $K_m$?";
const CHECK_TASK = "A line meets the y-axis at $0.2$ s/µM. What is $V_{max}$?";
const NEXT_TASK = "A Lineweaver-Burk line meets the y-axis at $0.5$ s/µM. What is its $V_{max}$?";
const TASKS = [VMAX_TASK, KM_TASK, CHECK_TASK, NEXT_TASK];

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

const KM_RIGHT = "Yes: $K_m = 0.5 \\times 4 = 2$ µM. You read both constants off one line.\n\n" + NEXT_TASK;

const HINT = "The slope is $\\frac{K_m}{V_{max}}$, and you have $V_{max}$ already. What do you multiply the slope by?";

const LOST = "Let's find where it slipped. When you look at the line, which part do you read first: where it crosses the y-axis, or how steep it is?";

const BACK_TO = () => `Back to it.\n\n${task === "km" ? KM_TASK : VMAX_TASK}`;

function home(folder: string | null = open): Home {
  if (folder === null) throw new Error("No course is open.");
  if (folder !== FOLDER) {
    const name = folder.split("/").pop() ?? folder;
    return {
      folder,
      name,
      state: name === "Statistics" ? "empty" : "ready",
      message: null,
      exam: null,
      last: name === "Statistics" ? null : { days_ago: 1, goal: "Diagonalising a matrix with a repeated eigenvalue", stopped_at: null, next_action: "Find the eigenvectors for $\\lambda = 2$." },
      checks_due: 0,
      redos_due: 0,
      opening: null,
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
    state: !upgraded ? "needs_upgrade" : notSetUp ? "not_set_up" : "ready",
    message: !upgraded
      ? "This folder’s record is in an older form. Upgrade it to study here. Magnesium keeps a copy of your record before it changes it."
      : notSetUp
        ? "This folder isn’t set up for Magnesium yet. Setting it up adds a hidden folder for the record of your answers and leaves your own files as they are."
        : null,
    exam: plan.exam_date ? { target: "midterm-1", title: "Midterm 1", date: plan.exam_date, day: day(plan.exam_date), days_left: 5 } : null,
    last: upgraded ? { days_ago: week ? 7 : has("reopen") ? 0 : 2, goal: GOAL, stopped_at: stoppedAt, next_action: "Find $V_{max}$ from the intercept 0.25." } : null,
    checks_due: week ? 1 : 0,
    checks_alone: week ? 1 : 0,
    redos_due: 0,
    opening: week ? { checks: 1, redos: 0 } : null,
    first_check: week ? "An enzyme has $K_m = 2$ mM. At $[S] = 6$ mM, what fraction of $V_{max}$ does it reach?" : null,
    can_count_today: week ? 2 : 0,
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
  if (has("setup-replace-newer")) return { state: "different", installed: program("0.4.1"), bundled, installed_now: false, message: null };
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

const setup = (): Setup => ({ runtime: runtime(), account: account(), payment, permissions });

/** The conversation left, as the window keeps it: what `turns` reads at rest. */
function savedTurns(folder: string | null = open, session = "demo"): Turn[] {
  if (folder !== FOLDER || firstOpen) return [];
  // Each kept turn says how it was sent and, as Magnesium's engine notes it, the task it left open.
  const noted = (task: string | null, posed = false) => ({ noted: true, task, posed });
  if (session === "demo-0") {
    const at = Date.now() - 5 * DAY_MS;
    const RECIPROCAL = "Take the reciprocal of both sides.";
    return [
      { session, ms: at, learner: "Let's work on taking reciprocals of the rate equation.", tutor: `Start from $v = \\frac{V_{max}[S]}{K_m + [S]}$. ${RECIPROCAL}`, interrupted: false, ...noted(RECIPROCAL, true) },
      { session, ms: at + 60_000, learner: "1/v = (K_m + [S]) / (V_max [S])", tutor: "Right. Split the fraction and you have a straight line in $\\frac{1}{[S]}$.", interrupted: false, intent: "answer", ...noted(null) },
    ];
  }
  const at = Date.now() - (week ? 7 : 2) * 86_400_000;
  const turn = (n: number, learner: string, tutor: string, more: Partial<Turn> = {}): Turn => ({ session: "demo", ms: at + n * 60_000, learner, tutor, interrupted: false, ...more });
  const FORM = "Write $\\frac{1}{v}$ in terms of $\\frac{1}{[S]}$.";
  const turns = [
    turn(0, "I'm back. Let's continue.", `Last time ended at the rate equation. Take reciprocals of both sides and see what kind of line it gives.\n\n${FORM}`, noted(FORM, true)),
    turn(1, "1/v = (K_m/V_max)(1/[S]) + 1/V_max", "Right, and that is the whole of the Lineweaver–Burk form: a straight line, slope $\\frac{K_m}{V_{max}}$, intercept $\\frac{1}{V_{max}}$.", { intent: "answer", ...noted(null) }),
  ];
  if (has("reopen")) turns.push(turn(2, "I'm back. Let's continue. I stopped at: step 3, what the y-intercept gives", OPENING, noted(VMAX_TASK, true)));
  if (week) {
    turns.push(
      turn(2, "I'm back. Let's continue. I stopped at: step 3, what the y-intercept gives", OPENING, noted(VMAX_TASK, true)),
      turn(3, "V_max = 4 µM/s", RIGHT, { intent: "answer", ...noted(KM_TASK, true) }),
      turn(4, "I skipped three days and the midterm moved, change the plan", demoBaseReply("change the plan") ?? "", { intent: "talk", ...noted(KM_TASK) }),
    );
  }
  return turns;
}

/** The record's rows for the kept answers of the demo's sessions, at the minute each was given. */
function keptMarks(): Mark[] {
  const minute = (ms: number) => {
    const d = new Date(ms);
    const pad = (n: number) => String(n).padStart(2, "0");
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
  };
  return ["demo", "demo-0"].flatMap((session) =>
    savedTurns(FOLDER, session)
      .filter((turn) => turn.intent === "answer")
      .map((turn, n) => ({ id: `k-${session}-${n}`, at: minute(turn.ms), label: n === 0 ? "taking reciprocals of the rate equation" : "reading $V_{max}$ from the y-intercept", mark: "right", help: "none", cold: true, paper: null })),
  );
}

let emit: (event: AgentEvent) => void = () => {};
// The task open in the demo lesson, as the direct engine would say it after each turn.
let openTask: string | null = null;
let replying = false;
let unopenedSaid = false;
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
  if (has("record-unopened", "record-unopened-usual") && !unopenedSaid) {
    unopenedSaid = true;
    const unopened = "This lesson started without opening your record, so its answers may not be marked.";
    emit(
      has("record-unopened")
        ? { type: "notice", kind: "record_unopened_at_effort", text: `${unopened} It ran at low effort.` }
        : { type: "notice", kind: "record_unopened", text: `${unopened} It ran on Haiku 5.5 at Claude Code’s usual effort.` },
    );
  }
  // The engine says which task is open, in the tutor's words.
  const posed = TASKS.find((each) => text.trim().endsWith(each)) ?? null;
  if (posed) openTask = posed;
  emit({ type: "open_task", task: openTask, posed: posed !== null });
  // The record is read once a mark has had its moment.
  window.setTimeout(() => emit({ type: "settled" }), 500);
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
    // As the core words what Claude Code 2.1.322 sends for each (asks.rs): the plan, a command, a tool it does not know.
    if (has("permission-command"))
      emit({
        type: "permission",
        id: "p1",
        title: "The tutor asks to run a command",
        detail: "Build the lab report as a PDF",
        exact: "latexmk -pdf Study/midterm-1/lab-report.tex",
        can_always: true,
        always: "Always allow latexmk commands",
      });
    else if (has("permission-unknown"))
      emit({
        type: "permission",
        id: "p1",
        title: "The tutor asks to use a tool this window doesn’t know",
        detail: "Adds an event to your calendar",
        exact: 'create_event {"title":"Lab report due","date":"2026-11-20"}',
        can_always: false,
      });
    else emit({ type: "permission", id: "p1", title: "The tutor asks to change your plan", detail: "Midterm 1", can_always: true, always: "Allow changes to files this session" });
    return;
  }
  if (lower.includes("nomodel")) {
    emit({ type: "turn_started" });
    emit({ type: "notice", kind: "model_unavailable", text: "Opus 4.6 isn’t available to you. Pick another model, then send your message again." });
    emit({ type: "turn_ended", ok: false, interrupted: false, error: null });
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

/** The stand-in runtime's receipt: the y-intercept was not right on Thu 5 Nov (m4), and nothing on it since. */
function demoReceipt(): { receipt?: string } {
  if (task !== "km") return {};
  const days = Math.max(1, Math.round((Date.now() - new Date(2026, 10, 5, 12).getTime()) / 86_400_000));
  return { receipt: `Not right on Thu 5 Nov: reading $V_{max}$ from the y-intercept. It held through the ${days === 1 ? "day" : `${days} days`} since you last answered it.` };
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
      // The stand-in record: a right answer alone is the skill's first day, and the next one the same day earns none.
      ...(mark === "right" && !helpAsked && !marks.some((kept) => kept.pass && kept.label === (task === "km" ? "reading $V_{max}$ from the y-intercept" : "reading $K_m$ from the slope")) ? { pass: 1, next_day: dayFromNow(1), ...demoReceipt() } : {}),
    };
    helpAsked = false;
    marks.push(row);
    demoMarked(row, emit);
  }, 220);
}

export function demoCore(): Core {
  const plan = (folder?: string) => {
    // Another course has a plan of its own; this demo keeps only Biochemistry's.
    if ((folder ?? open) !== FOLDER) return { target: null, title: null, exam_date: null, goal: null, steps: [], items: [], text: null, path: null, changed_ms: null };
    const kept = demoPlan();
    // The route follows the lesson: the slope is in hand once the intercept is done.
    const steps = task === "km" ? kept.steps.map((step, at) => ({ ...step, status: at < 3 ? "done" : at === 3 ? "doing" : "todo" })) : kept.steps;
    return { ...kept, goal: GOAL, steps };
  };
  const workspaces = (): Workspace[] =>
    folders
      .map((folder) => ({ folder, name: folder.split("/").pop() ?? folder, opened_ms: openedMs[folder] ?? null, latest: sessionsOf(folder)[0] ?? null }))
      .sort((a, b) => (b.opened_ms ?? 0) - (a.opened_ms ?? 0));
  return {
    initialFolder: async () => null,
    recent: async () => [...folders],
    library: async () => library(),
    workspaces: async () => workspaces(),
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
    visitFolder: async (path) => {
      if (!folders.includes(path)) folders = [path, ...folders];
      return home(path);
    },
    newFolder: async (path) => {
      open = path;
      return home();
    },
    home: async (folder) => home(folder ?? open),
    setUp: async (folder) => {
      notSetUp = false;
      return home(folder ?? open);
    },
    upgradeFolder: async (folder) => {
      upgraded = true;
      return home(folder ?? open);
    },
    setup: async () => setup(),
    replaceRuntime: async () => runtime(),
    keepRuntime: async () => runtime(),
    account: async () => account(),
    setPayment: async (chosen) => {
      payment = chosen;
      return account();
    },
    setPermissions: async (chosen) => {
      permissions = chosen;
    },
    engine: async () => ({ chosen: engineChosen, conversation: engineChosen }),
    models: async () => modelChoice(),
    askModels: async () => {
      await wait(has("model-asking") ? 600_000 : 400);
      if (!modelsFailed) {
        modelsNone = false;
        // A window with no pick yet takes the agent's default from its first list.
        pick ??= { model: "claude-opus-5-5", name: "Opus 5.5" };
      }
      return modelChoice();
    },
    setPick: async (model, effort) => {
      const row = MODEL_LIST.find((m) => m.value === model || m.resolved === model);
      pick = { model: row?.resolved ?? model, name: row?.name ?? pick?.name, ...(effort ? { effort } : {}) };
      return modelChoice();
    },
    sessionModels: async (at) => sessionChoice(at),
    setSessionModel: async (at, model, effort) => {
      await wait(120);
      if (has("model-lesson-refused") && at.live) throw new Error("Claude Code did not change the model: Model 'claude-opus-4-6' not found.");
      const row = MODEL_LIST.find((m) => m.value === model || m.resolved === model);
      pick = { model: row?.resolved ?? model, name: row?.name ?? pick?.name, ...(effort ? { effort } : {}) };
      const key = onKey(at);
      if (key) sessionsOn[key] = { model: pick.model, ...(effort ? { effort } : {}) };
      return sessionChoice(at);
    },
    setEngine: async (chosen) => {
      engineChosen = chosen;
      return { chosen, conversation: null };
    },
    signIn: async () => {
      signedIn = true;
    },
    about: async () => ({ version: "0.3.0", commit: "4f1c2ad9e0b7" }),
    showWindow: async () => {},
    closeWindow: async () => {},
    turns: async () => savedTurns(),
    lessons: async (folder) => sessionsOf(folder),
    lessonTurns: async (folder, session) => savedTurns(folder, session),
    touchSession: async () => {},
    renameSession: async () => {},
    shareFile: async () => {
      throw new Error("Sharing works in the window on the Mac, not in this demo.");
    },
    openSession: async (resume, where): Promise<Opened> => {
      if (where?.folder) open = where.folder;
      return { resumed: resume, turns: resume ? savedTurns(open, where?.session ?? "demo") : [], engine: engineChosen };
    },
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
      void reply("Thank you. The plan is changed.");
    },
    answer: async () => {},
    stop: async (line) => {
      stoppedAt = line;
    },
    rail: async () => ({ goal: GOAL, steps: demoPlan().steps, next_action: null }),
    plan: async (folder) => plan(folder),
    progress: async (): Promise<Progress> => ({
      targets: [],
      exam: null,
      confirmed: [{ dots: [2, 2, 2], name: "what $V_{max}$ means", what: "", note: "confirmed", due: false, next_day: null, last_answer: "m0" }],
      // The demo record's skills, each with the newest answer on it and the first day its next pass can count, as
      // the runtime gives them.
      in_progress: [
        { dots: [0, 0, 0], name: "reading $V_{max}$ from the y-intercept", what: "", note: week ? "due today" : "–", due: week, next_day: week ? dayFromNow(0) : dayFromNow(1), last_answer: "m4" },
        { dots: [2, 0, 0], name: "units of a rate", what: "", note: "–", due: false, next_day: dayFromNow(0), last_answer: "m1" },
        { dots: [1, 0, 0], name: "the slope gives $K_m / V_{max}$ when the plot is drawn the other way round", what: "", note: "–", due: false, next_day: dayFromNow(3), last_answer: "m2" },
        { dots: [0, 0, 0], name: "taking reciprocals of the rate equation", what: "", note: "–", due: false, next_day: dayFromNow(1), last_answer: "m3" },
      ],
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
    attach: async (path, group) => `Study/handed-in/${group}/2026-11-12/${path.split("/").pop()}`,
    attachBytes: async (name, _bytes, group) => `Study/handed-in/${group}/2026-11-12/${name}`,
    addMaterial: async (paths): Promise<Added> => {
      const names = paths.map((path) => path.split("/").pop() ?? path);
      materialNames = [...names, ...materialNames];
      return { saved: names.map((name) => `Study/materials/${name}`), keys: 0, lines: [`Added ${names.join(", ")} to the course material. The tutor's search can read it.`] };
    },
    addPastedText: async (): Promise<Added> => ({ saved: ["Study/materials/Pasted text 10.41.txt"], keys: 0, lines: ["Kept the pasted text with the course material."] }),
    materialList: async () => ({ names: materialNames, files: materialNames.length + 4, keys: 1 }),
    marks: async () => [...keptMarks(), ...marks],
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
    windowState: async () => null,
    setWindowState: async () => {},
    setMenu: async () => {},
    onMenu: async () => () => {},
    onEvent: async (handler) => {
      emit = handler;
      return () => {
        emit = () => {};
      };
    },
    selftest: async () => false,
    selftestReport: async () => {},
    // A browser has nothing behind its page to frost: the demo's window stays solid.
    glassState: async () => ({ can: false, liquid: false, reduce: false }),
    setGlass: async () => {},
  };
}

/** Wait until the tutor has finished, and a moment for the record. */
async function settle(ms = 500) {
  await wait(120);
  while (replying) await wait(40);
  await wait(ms);
}

const session = (actions: Actions) => {
  const handle = actions.session();
  if (!handle) throw new Error("no session is in front");
  return handle;
};

/** Home's first row: carry on the most recent session. */
async function lesson(actions: Actions) {
  actions.continueRecent();
  await wait(200);
  await settle();
}

async function answered(actions: Actions) {
  await lesson(actions);
  await session(actions).say("$V_{max}$ = 4 µM/s", "answer");
  await settle(700);
}

async function changed(actions: Actions, hold = false) {
  await answered(actions);
  await session(actions).say("I skipped three days and the midterm moved, change the plan", "talk");
  if (!hold) await settle(700);
  else await wait(500);
}

async function offPlan(actions: Actions) {
  await changed(actions);
  await session(actions).say("By the way, why do some courses use a Hanes–Woolf plot instead?", "talk");
  await settle();
}

/** A day from today, as the runtime writes one: "2026-11-17". */
function dayFromNow(days: number): string {
  const date = new Date();
  date.setDate(date.getDate() + days);
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

/** Drive the window to the state the address names, as a learner would. */
export async function startFromAddress(actions: Actions): Promise<void> {
  await wait(60);
  switch (state) {
    case "s2-start":
    case "s5-week-next":
      return lesson(actions);
    case "s2-answer":
    case "narrow-lesson":
      return answered(actions);
    case "panel":
      await answered(actions);
      return actions.run("plan");
    case "panel-changed":
      await changed(actions);
      return actions.run("plan");
    case "help-row":
      return lesson(actions);
    case "symbols":
      await lesson(actions);
      actions.run("symbols");
      return;
    case "attach":
      await lesson(actions);
      actions.run("attach");
      return wait(200);
    case "settings":
      return actions.run("settings");
    case "settings-keys":
      return actions.run("keys-help");
    case "settings-colour":
      actions.run("settings");
      await wait(150);
      document.querySelector<HTMLElement>("[data-drive='colour-ink-light']")?.click();
      await wait(100);
      document.querySelector<HTMLElement>(".colour-picker")?.scrollIntoView({ block: "center" });
      return wait(100);
    case "material":
      actions.goWorkspace(FOLDER);
      await wait(300);
      document.querySelector<HTMLElement>("[data-drive='material']")?.click();
      await wait(50);
      document.querySelector<HTMLElement>(".file-row")?.click();
      return wait(400);
    case "workspace":
      return actions.goWorkspace(FOLDER);
    case "new-session":
    case "model-first-run":
    case "model-failed":
    case "model-not-offered":
      actions.newSession(FOLDER);
      return wait(300);
    case "model-list":
    case "model-effort":
    case "model-older":
    case "model-list-first-run":
    case "model-list-failed":
    case "model-asking":
      actions.newSession(FOLDER);
      await wait(300);
      document.querySelector<HTMLElement>(".line.lesson [data-drive='more']")?.click();
      await wait(100);
      document.querySelector<HTMLElement>(".line-menu [data-drive='model']")?.click();
      await wait(150);
      if (has("model-older")) document.querySelector<HTMLElement>("[data-drive='model-older']")?.click();
      return wait(150);
    case "first-more":
      actions.newSession(FOLDER);
      await wait(300);
      document.querySelector<HTMLElement>(".line.lesson [data-drive='more']")?.click();
      return wait(150);
    case "line-stuck":
    case "line-stuck-typing":
      await answered(actions);
      if (has("line-stuck-typing")) {
        const field = document.querySelector<HTMLTextAreaElement>(".line.lesson [data-drive='composer']");
        if (field) {
          Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(field, "Kₘ = 4 × 0.5 =");
          field.dispatchEvent(new Event("input", { bubbles: true }));
        }
        await wait(100);
      }
      document.querySelector<HTMLElement>(".line.lesson [data-drive='stuck']")?.click();
      return wait(150);
    case "line-more-typing": {
      await answered(actions);
      const field = document.querySelector<HTMLTextAreaElement>(".line.lesson [data-drive='composer']");
      if (field) {
        Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(field, "Kₘ = 4 × 0.5 =");
        field.dispatchEvent(new Event("input", { bubbles: true }));
      }
      await wait(100);
      document.querySelector<HTMLElement>(".line.lesson [data-drive='more']")?.click();
      return wait(150);
    }
    case "line-more":
    case "model-lesson-word":
      await answered(actions);
      document.querySelector<HTMLElement>(".line.lesson [data-drive='more']")?.click();
      return wait(150);
    case "model-lesson":
    case "model-lesson-effort":
    case "model-lesson-refused":
      await answered(actions);
      await wait(200);
      document.querySelector<HTMLElement>(".line.lesson [data-drive='more']")?.click();
      await wait(100);
      document.querySelector<HTMLElement>(".line-menu [data-drive='model']")?.click();
      await wait(200);
      if (has("model-lesson-effort")) {
        document.querySelector<HTMLElement>("[data-drive='model-effort'][data-level='high']")?.click();
        await wait(300);
      }
      if (has("model-lesson-refused")) {
        document.querySelector<HTMLElement>("[data-drive='model-row'][data-value='claude-opus-4-6']")?.click();
        document.querySelector<HTMLElement>("[data-drive='model-older']")?.click();
        await wait(150);
        document.querySelector<HTMLElement>("[data-drive='model-row'][data-value='claude-opus-4-6']")?.click();
        await wait(400);
      }
      return wait(150);
    case "record-unopened":
    case "record-unopened-usual":
      actions.newSession(FOLDER, { kind: "start" });
      await wait(200);
      return settle();
    case "new-session-start":
      actions.newSession(FOLDER, { kind: "start" });
      await wait(200);
      return settle();
    case "paper":
    case "paper-last":
    case "paper-over":
    case "paper-hours":
    case "paper-days":
    case "paper-long":
      return actions.goWorkspace(FOLDER);
    case "upgrade":
      return actions.goWorkspace(FOLDER);
    case "session-earlier":
      actions.openSession(FOLDER, "demo-0", "Taking reciprocals of the rate equation");
      return wait(300);
    case "tabs":
      await answered(actions);
      actions.openSession(FOLDER, "demo-0", "Taking reciprocals of the rate equation");
      await wait(200);
      actions.run("settings");
      return actions.run("tab", actions.tabs()[1].key);
    case "source":
      // The opening reply cites the slides: a new session, so the citation is on the page to open.
      actions.newSession(FOLDER, { kind: "start" });
      await wait(200);
      await settle();
      // A citation opens once the task near it is answered.
      await session(actions).say("V_max = 4 µM/s", "answer");
      await settle(700);
      document.querySelector<HTMLElement>(".tutor a.source[href*='Week']")?.click();
      return wait(300);
    case "s3-before":
      return changed(actions, true);
    case "s3-after":
      return changed(actions);
    case "s4-offplan":
      return offPlan(actions);
    case "s4-return":
      await offPlan(actions);
      await session(actions).say("OK, back to the task", "talk");
      return settle();
    case "working":
      await lesson(actions);
      // The tutor's reply never comes, so the page stays on "working".
      void session(actions).say("Why is the intercept the reciprocal?", "talk");
      return wait(900);
    case "permission":
    case "permission-command":
    case "permission-unknown":
      await lesson(actions);
      await session(actions).say("Can you write the lab report into my plan for Friday?", "talk");
      return;
    case "model-unavailable":
      await lesson(actions);
      await session(actions).say("NOMODEL Why is the intercept the reciprocal?", "talk");
      return wait(200);
    case "not-set-up":
      return actions.goWorkspace(FOLDER);
    case "commands":
      return actions.run("commands");
    case "switch":
      actions.run("commands");
      await wait(80);
      document.querySelector<HTMLElement>("[data-drive='command-switch']")?.click();
      return;
    default:
      return;
  }
}

/** For the self-check's "hold the stream" case: let a held reply finish. */
export function releaseStream(): void {
  holdRelease?.();
}
