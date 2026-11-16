// Photograph every page of the real window, in both schemes, through the
// harness (src/harness.ts). Builds nothing and starts no model.
//
//   node scripts/pages.mjs <app-binary> <out-dir>
//
// <app-binary> is the window's program, such as target/release/magnesium-window.
// It works in a scratch course made by scripts/scratch-course.sh, with a
// scratch window home, so none of your study folders, settings or agent
// sessions are touched. Lessons run on Magnesium with the stand-in model
// (crates/core/tests/stand_in_model.py).
//
// It writes <stem>-dark.png and <stem>-light.png for each page and state it
// visits (the session's line in both of its looks, soft and bare), menu.json
// (the menu bar, which a photograph of the window does not show) and run.json
// (each step, and any problem). It exits 1 when a step failed, after doing
// every step it can.
//
// PAGES_ONLY=work photographs only the workspace page over a record of
// several skills (scripts/scratch-record.py), in a few minutes.

import { spawn, execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, openSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { drive } from "./drive.mjs";

const APP = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const [binary, outArg] = process.argv.slice(2);
if (!binary || !outArg) {
  console.error("usage: node scripts/pages.mjs <app-binary> <out-dir>");
  process.exit(64);
}
if (!existsSync(binary)) {
  console.error(`pages: no window program at ${binary}`);
  process.exit(66);
}
const out = resolve(outArg);
mkdirSync(out, { recursive: true });

const TURN_MS = 60_000;
const ANSWER = "The standard error is s/√n = 10/√25 = 2 minutes.";
const CHANGE = "I skipped three days and the midterm moved, change the plan";
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

const run = { started: new Date().toISOString(), binary, steps: [], problems: [], shots: [] };
const problem = (what) => {
  run.problems.push(what);
  console.error(`pages: ${what}`);
};
const save = () => writeFileSync(join(out, "run.json"), JSON.stringify(run, null, 2));

const scratch = mkdtempSync(join(tmpdir(), "mg-pages-"));
run.scratch = scratch;
execFileSync("sh", [join(APP, "scripts/scratch-course.sh"), scratch], { encoding: "utf8" });
const runtime = join(scratch, "magnesium");
const agent = join(scratch, "claude");
// A photo of working, to put on the line.
const photo = join(scratch, "working-photo.png");
execFileSync("cp", [join(APP, "src-tauri/app-icon.png"), photo]);
writeFileSync(agent, `#!/bin/sh\nexport STAND_IN_LOG='${join(scratch, "model.log")}'\nexec python3 '${join(APP, "crates/core/tests/stand_in_model.py")}' "$@"\n`, { mode: 0o755 });

const pluginAgent = join(scratch, "claude-plugin");
mkdirSync(join(scratch, "agent"), { recursive: true });
writeFileSync(
  pluginAgent,
  `#!/bin/sh\nexport STAND_IN_LOG='${join(scratch, "agent.log")}'\nexport STAND_IN_STATE='${join(scratch, "agent")}'\nexec python3 '${join(APP, "crates/core/tests/stand_in_agent.py")}' "$@"\n`,
  { mode: 0o755 },
);

async function launch(name, home, withAgent = agent, more = {}) {
  const driveDir = join(scratch, "drive", name);
  mkdirSync(driveDir, { recursive: true });
  const env = { ...process.env, MAGNESIUM_APP_HOME: home, MAGNESIUM_APP_RUNTIME: runtime, MAGNESIUM_APP_DRIVE: driveDir, MAGNESIUM_APP_AGENT: withAgent, ...more };
  delete env.MAGNESIUM_APP_SELFTEST;
  delete env.MAGNESIUM_APP_FOLDER;
  const log = openSync(join(out, `window-${name}.log`), "w");
  const child = spawn(binary, [], { env, stdio: ["ignore", log, log] });
  const exited = new Promise((done) => child.on("exit", (code) => done(code)));
  const window = { name, child, exited, op: (op, options) => drive(driveDir, op, options) };
  const start = Date.now();
  for (;;) {
    const answer = await window.op({ op: "ready" }, { timeoutMs: 5_000 });
    if (answer.ok && answer.ready) break;
    if (Date.now() - start > 60_000) {
      problem(`${name}: the window never called ready()`);
      break;
    }
    await sleep(250);
  }
  await step(window, `${name}: settle`, { op: "wait_idle", timeout_ms: 60_000 });
  return window;
}

async function step(window, label, op) {
  const answer = await window.op(op);
  run.steps.push({ label, op, answer: op.op === "text" ? "(kept apart)" : answer });
  if (!answer.ok) problem(`${label}: ${answer.error ?? "not ok"}`);
  save();
  return answer;
}

async function shoot(window, stem, what) {
  for (const scheme of ["dark", "light"]) {
    await window.op({ op: "scheme", scheme });
    await sleep(150);
    const file = join(out, `${stem}-${scheme}.png`);
    try {
      execFileSync("sh", [join(APP, "scripts/capture.sh"), String(window.child.pid), file], { stdio: ["ignore", "ignore", "pipe"] });
    } catch (error) {
      problem(`photo ${stem}-${scheme}: ${String(error.stderr ?? error).trim()}`);
    }
  }
  await window.op({ op: "scheme", scheme: "system" });
  run.shots.push({ stem, what });
  save();
}

async function quit(window) {
  await window.op({ op: "quit" }, { timeoutMs: 10_000 });
  const code = await Promise.race([window.exited, sleep(15_000).then(() => "timeout")]);
  if (code === "timeout") window.child.kill("SIGKILL");
}

const settle = (window, ms = 500) => window.op({ op: "wait", ms });

/** A timed paper running, in each of its states, with the page as the paper alone until it is handed in or ended.
 * PAGES_ONLY=paper photographs only these. */
async function paperStates() {
  const kept = JSON.parse(readFileSync(join(scratch, "window", "state.json"), "utf8"));
  // While a paper runs the page is that paper: none of what the folder holds is drawn, and all of it is back after.
  const rest = ["Your work", "Material", "Take this folder off the list"];
  const pageShows = async (window, label, running) => {
    const read = await window.op({ op: "text" });
    const text = String(read.text ?? "");
    const drawn = rest.filter((words) => text.includes(words));
    run.steps.push({ label, drawn });
    if (running && drawn.length > 0) problem(`${label}: the page draws ${drawn.join(", ")} while the paper runs`);
    if (!running && drawn.length !== rest.length) problem(`${label}: the page lacks ${rest.filter((w) => !drawn.includes(w)).join(", ")} after the paper`);
  };
  // A timed paper running, as a window opened again finds it: its clock is the start time kept on disk.
  // Sitting one from the page would open the paper in the Mac's own viewer, so the sitting is written here.
  const course = realpathSync(join(scratch, "Intro-Statistics"));
  const sitting = (minutesAgo, name) => ({ target: "midterm-2", name, path: "Study/midterm-2/papers/Midterm 2023.pdf", minutes: 75, started_ms: Date.now() - minutesAgo * 60_000 });
  const paperWindow = (name, paper) => {
    const home = join(scratch, `window-${name}`);
    mkdirSync(home, { recursive: true });
    writeFileSync(join(home, "state.json"), JSON.stringify({ recent: kept.recent, folders: { [course]: { paper } } }, null, 2));
    return launch(name, home);
  };
  const q = await paperWindow("paper", sitting(12, "Midterm 2023"));
  await step(q, "open the workspace", { op: "click", target: "workspace-Intro Statistics", by: "drive" });
  await settle(q, 800);
  await shoot(q, "workspace-paper", "Workspace: a timed paper running, the page's lead");
  await pageShows(q, "the paper alone on the page", true);
  await step(q, "open the paper again", { op: "click", target: "sitting-open", by: "drive" });
  await settle(q, 1800);
  await shoot(q, "workspace-paper-open", "Workspace: the running paper with the paper itself open beside the page");
  await closeDocument(q, "Midterm 2023");
  await step(q, "end without handing in", { op: "click", target: "sitting-end", by: "drive" });
  await settle(q, 300);
  await shoot(q, "workspace-paper-end", "Workspace: ending a paper without handing it in asks first, in place");
  await step(q, "keep going, with Esc", { op: "key", key: "Escape" });
  await settle(q, 200);
  // A key that does nothing, to read where the keyboard is once the block is back.
  const back = await step(q, "where the keyboard is", { op: "key", key: "Shift" });
  if (!/data-drive=sitting-end\b/.test(String(back.focused ?? ""))) problem(`Esc did not put the keyboard back on End without handing in: ${JSON.stringify(back.focused)}`);
  await step(q, "end without handing in again", { op: "click", target: "sitting-end", by: "drive" });
  await step(q, "end the paper", { op: "click", target: "sitting-end-now", by: "drive" });
  await settle(q, 800);
  await shoot(q, "workspace-paper-ended", "Workspace: the paper ended without handing in, the page as before it");
  await pageShows(q, "the page back after the paper", false);
  const after = await step(q, "where the keyboard is after the paper", { op: "key", key: "Shift" });
  if (!/data-drive=next\b/.test(String(after.focused ?? ""))) problem(`after the paper, the keyboard is not on Next: ${JSON.stringify(after.focused)}`);
  await quit(q);
  const r = await paperWindow("paper-over", sitting(100, "Midterm 2023, all three questions with every part, from the paper the course set last spring"));
  await step(r, "open the workspace", { op: "click", target: "workspace-Intro Statistics", by: "drive" });
  await settle(r, 800);
  await shoot(r, "workspace-paper-over", "Workspace: a paper with a long name, past its end");
  await step(r, "720 by 560", { op: "frame", width: 720, height: 560 });
  await settle(r, 400);
  await shoot(r, "workspace-paper-narrow", "Workspace: the running paper at 720 by 560");
  for (let n = 0; n < 11; n++) await r.op({ op: "menu_click", path: ["View", "Bigger Text"] });
  await settle(r, 400);
  await shoot(r, "workspace-paper-big", "Workspace: the running paper at 720 by 560, the largest text");
  // The text size is kept with the window's settings, which outlast this run: put it back.
  await step(r, "the text back to its own size", { op: "menu_click", path: ["View", "Actual Size"] });
  await quit(r);
  // The paper's other clocks: its last minute, and past its end by hours, and by days from a start on an earlier day.
  for (const [stem, minutesAgo, what] of [
    ["paper-last", 74.3, "Workspace: a paper in its last minute"],
    ["paper-hours", 75 + 190, "Workspace: a paper three hours past its end"],
    ["paper-days", 75 + 2 * 1440 + 30, "Workspace: a paper started two days ago, long past its end"],
  ]) {
    const s = await paperWindow(stem, sitting(minutesAgo, "Midterm 2023"));
    await step(s, "open the workspace", { op: "click", target: "workspace-Intro Statistics", by: "drive" });
    await settle(s, 800);
    await shoot(s, `workspace-${stem}`, what);
    if (stem === "paper-hours") {
      await step(s, "end without handing in, past the end", { op: "click", target: "sitting-end", by: "drive" });
      await settle(s, 300);
      await shoot(s, "workspace-paper-hours-end", "Workspace: ending a paper past its end asks first, in place");
    }
    await quit(s);
  }

}


/** Close a document: from the panel beside the page or lesson where there is one, else the sheet over it, with Esc. */
async function closeDocument(window, title) {
  const closed = await window.op({ op: "click", target: `close-doc-${title}`, by: "drive" });
  if (!closed.ok) await step(window, `close ${title}`, { op: "key", key: "Escape" });
  else run.steps.push({ label: `close ${title} in the panel` });
  await settle(window, 300);
}

/**
 * Next's line saying what the session starts with, from the record (PAGES_ONLY=due): on a course with nothing due; over
 * a record with a quick check due, while its sitting carries on and once three hours have passed; with a paper redo
 * due too; with a long item; with every item done; at 720 by 560 and the largest text; and back on Home after a lesson
 * has begun the sitting.
 */
async function dueStates() {
  const shotsOf = async (window, stem, what) => {
    await step(window, `${stem}: Home`, { op: "click", target: "home", by: "drive" });
    await settle(window, 800);
    await shoot(window, `home-${stem}`, `Home: ${what}`);
    await step(window, `${stem}: the workspace`, { op: "click", target: "workspace-Intro Statistics", by: "drive" });
    await settle(window, 800);
    await shoot(window, `workspace-${stem}`, `Workspace: ${what}`);
    await step(window, `${stem}: ‹ Home`, { op: "click", target: "‹ Home" });
    await settle(window, 600);
  };
  const says = async (window, label, line) => {
    const read = await window.op({ op: "text" });
    const text = String(read.text ?? "");
    run.steps.push({ label, line, found: text.includes(line ?? "Starts with") });
    if (line && !text.includes(line)) problem(`${label}: "${line}" is not on the page`);
    if (!line && text.includes("Starts with")) problem(`${label}: the page says what the session starts with, and should not`);
  };

  // Nothing due: the scratch course, never studied in.
  const plain = await launch("due-none", join(scratch, "window"));
  await says(plain, "nothing due, Home", null);
  await shotsOf(plain, "nothing-due", "nothing due, so the row says nothing more");
  await quit(plain);

  // A record with a quick check due (scripts/scratch-record.py), its last answer a moment ago.
  const dir = join(scratch, "due-record");
  mkdirSync(dir, { recursive: true });
  execFileSync("sh", [join(APP, "scripts/scratch-course.sh"), dir], { encoding: "utf8" });
  execFileSync("python3", [join(APP, "scripts/scratch-record.py"), dir], { encoding: "utf8" });
  const course = join(dir, "Intro-Statistics");
  const listed = JSON.parse(readFileSync(join(dir, "window", "state.json"), "utf8"));
  const recordWindow = (name) => {
    const home = join(scratch, `window-${name}`);
    mkdirSync(home, { recursive: true });
    writeFileSync(join(home, "state.json"), JSON.stringify({ ...listed, folders: {} }, null, 2));
    return launch(name, home, agent, { MAGNESIUM_APP_RUNTIME: join(dir, "magnesium") });
  };
  // The runtime's note of the learner's last message, moved back: past three hours, the next message begins a sitting.
  const hoursAgo = (hours) =>
    execFileSync("python3", ["-I", "-c", "import sqlite3,sys,time; c=sqlite3.connect(sys.argv[1]); c.execute('UPDATE meta SET last_learner_ms=?', (int((time.time()-float(sys.argv[2])*3600)*1000),)); c.commit()", join(course, ".magnesium", "sittings.sqlite"), String(hours)]);
  const same = await recordWindow("due-same-sitting");
  await says(same, "a check due, the sitting carrying on", null);
  await shotsOf(same, "due-same-sitting", "a quick check due while the sitting carries on: the row says nothing, the page says it is due");
  await quit(same);

  hoursAgo(4);
  const k = await recordWindow("due-check");
  await says(k, "a check due, a new sitting", "Starts with 1 quick check · about a minute");
  await shotsOf(k, "due-check", "a quick check due and a new sitting next");
  await step(k, "720 by 560", { op: "frame", width: 720, height: 560 });
  for (let n = 0; n < 11; n++) await k.op({ op: "menu_click", path: ["View", "Bigger Text"] });
  await settle(k, 400);
  await shotsOf(k, "due-check-big", "a quick check due, at 720 by 560 and the largest text");
  await step(k, "the text back to its own size", { op: "menu_click", path: ["View", "Actual Size"] });
  await quit(k);

  // A long item and note, and a paper redo due as well.
  const planFile = join(course, "Study", "midterm-2", "plan.md");
  const planText = readFileSync(planFile, "utf8");
  writeFileSync(
    planFile,
    planText.replace(
      /\| 1 \| Sampling distributions \|[^|]*\|/,
      "| 1 | Sampling distributions of a mean and of a proportion, from a small sample | the standard error of a mean, when x̄ is normal, and why the sample size matters more than the population's |",
    ),
  );
  const day = (offset) => {
    const at = new Date(Date.now() + offset * 86_400_000);
    return `${at.getFullYear()}-${String(at.getMonth() + 1).padStart(2, "0")}-${String(at.getDate()).padStart(2, "0")}`;
  };
  writeFileSync(
    join(course, "Study", "midterm-2", "gap-log.md"),
    `## Gaps\n\n| # | Date | Paper | Q | Tag | What went wrong | Type | Repair done | Passes | Next redo | Status |\n|---|---|---|---|---|---|---|---|---|---|---|\n| 1 | ${day(-3)} | Midterm 2023 | Q1 | standard error | used s for the standard error | A | tighten | | ${day(-1)} | open |\n`,
  );
  const r = await recordWindow("due-redo");
  await says(r, "a check and a redo due", "Starts with 1 quick check and 1 paper redo");
  await shotsOf(r, "due-redo-long", "a quick check and a paper redo due, under a long item");
  await quit(r);
  writeFileSync(planFile, planText.replace(/\| todo \|/g, "| done |"));
  const d = await recordWindow("due-all-done");
  await says(d, "every item done", "Starts with 1 quick check and 1 paper redo");
  await shotsOf(d, "due-all-done", "every item done: New session, and what it starts with");
  await quit(d);
  writeFileSync(planFile, planText);

  // A lesson begun from Next: its first message begins the sitting, so back on Home the row says nothing more.
  const s = await recordWindow("due-begun");
  await step(s, "start from Next", { op: "click", target: "start", by: "drive" });
  await step(s, "the first reply", { op: "wait_idle", timeout_ms: TURN_MS, quiet_ms: 2500 });
  await shoot(s, "session-due-begun", "Session: begun from Next with a quick check and a paper redo due");
  await step(s, "Home after the lesson began", { op: "click", target: "home", by: "drive" });
  await settle(s, 800);
  await says(s, "Home after the lesson began", null);
  await shoot(s, "home-due-begun", "Home after a lesson began the sitting: the row says nothing more");
  await quit(s);
}

if (process.env.PAGES_ONLY === "due") {
  await dueStates().catch((error) => problem(`stopped: ${error?.stack ?? error}`));
  run.finished = new Date().toISOString();
  save();
  console.log(run.problems.length === 0 ? `pages: ${run.shots.length} pages in ${out}` : `pages: ${run.problems.length} problem(s); see ${join(out, "run.json")}`);
  process.exit(run.problems.length === 0 ? 0 : 1);
}

/** The workspace page over a record of several skills, each with its own next day: at the window's size, at 720 by
 * 560, and at 720 by 560 with the largest text. */
async function workRecord() {
  const dir = join(scratch, "record");
  mkdirSync(dir, { recursive: true });
  execFileSync("sh", [join(APP, "scripts/scratch-course.sh"), dir], { encoding: "utf8" });
  execFileSync("python3", [join(APP, "scripts/scratch-record.py"), dir], { encoding: "utf8" });
  const home = join(scratch, "window-record");
  mkdirSync(home, { recursive: true });
  const listed = JSON.parse(readFileSync(join(dir, "window", "state.json"), "utf8"));
  writeFileSync(join(home, "state.json"), JSON.stringify({ ...listed, folders: {} }, null, 2));
  const k = await launch("record", home, agent, { MAGNESIUM_APP_RUNTIME: join(dir, "magnesium") });
  await step(k, "open the workspace", { op: "click", target: "workspace-Intro Statistics", by: "drive" });
  await settle(k, 800);
  await shoot(k, "workspace-work-many", "Workspace over a record of six skills: Your work under Next, by the day each can count again");
  await step(k, "720 by 560", { op: "frame", width: 720, height: 560 });
  await settle(k, 400);
  await shoot(k, "workspace-work-narrow", "Workspace over six skills at 720 by 560");
  await step(k, "to Your work", { op: "scroll", target: "Your work" });
  await settle(k, 300);
  await shoot(k, "workspace-work-narrow-list", "Workspace over six skills at 720 by 560, scrolled to Your work");
  for (let n = 0; n < 11; n++) await k.op({ op: "menu_click", path: ["View", "Bigger Text"] });
  await settle(k, 400);
  await step(k, "to Your work, larger", { op: "scroll", target: "Your work" });
  await settle(k, 300);
  await shoot(k, "workspace-work-big", "Workspace over six skills at 720 by 560, the largest text");
  // The text size is kept with the window's settings, which outlast this run: put it back.
  await step(k, "the text back to its own size", { op: "menu_click", path: ["View", "Actual Size"] });
  await quit(k);
}

try {
  if (process.env.PAGES_ONLY === "paper") {
    await paperStates();
    throw "only";
  }

  if (process.env.PAGES_ONLY === "work") {
    await workRecord();
    throw "only";
  }
  // Home on a Mac with no study folder yet.
  const emptyHome = join(scratch, "window-empty");
  mkdirSync(emptyHome, { recursive: true });
  const empty = await launch("empty", emptyHome);
  await shoot(empty, "home-empty", "Home, before any workspace");
  await quit(empty);

  const w = await launch("main", join(scratch, "window"));
  await shoot(w, "home", "Home, three workspaces, no session yet");

  // A workspace's page, its material and the paste sheet.
  await step(w, "open the workspace", { op: "click", target: "workspace-Intro Statistics", by: "drive" });
  await settle(w, 800);
  await shoot(w, "workspace", "Workspace: Intro Statistics, its past papers and no other material");
  // A note of the material: Linear Algebra has one.
  await step(w, "back home for a note", { op: "click", target: "‹ Home" });
  await step(w, "open Linear Algebra", { op: "click", target: "workspace-Linear Algebra", by: "drive" });
  await settle(w, 800);
  await shoot(w, "workspace-no-plan", "Workspace with no plan: New session first, the material folded to its count");
  await step(w, "open the material", { op: "click", target: "material", by: "drive" });
  await settle(w, 300);
  await step(w, "open its note", { op: "click", target: "Week 3 eigenvalues" });
  await settle(w, 1000);
  await shoot(w, "material", "A note of the material, opened from the workspace");
  await closeDocument(w, "Week 3 eigenvalues");
  await step(w, "back to Home", { op: "click", target: "‹ Home" });
  await step(w, "Intro Statistics again", { op: "click", target: "workspace-Intro Statistics", by: "drive" });
  await settle(w, 600);
  await step(w, "paste text", { op: "click", target: "Paste text…" });
  await settle(w);
  await shoot(w, "paste", "Paste text as material");
  await step(w, "close the paste sheet", { op: "key", key: "Escape" });

  // A new session: at rest, then the tutor's first task, then a marked answer.
  await step(w, "new session", { op: "click", target: "new-session", by: "drive" });
  await settle(w, 800);
  await shoot(w, "session-rest", "Session: a new session before it starts");
  await step(w, "focus the line", { op: "focus", target: "composer", by: "drive" });
  await step(w, "start", { op: "key", key: "Enter" });
  await step(w, "the first task", { op: "wait_idle", timeout_ms: TURN_MS, quiet_ms: 2500 });
  await shoot(w, "session-task", "Session: the tutor's first task, the answer line open");
  await step(w, "stuck", { op: "click", target: "stuck", by: "drive" });
  await settle(w, 300);
  await shoot(w, "session-stuck", "Session: Stuck's row open in the line, under the open task");
  await step(w, "close Stuck", { op: "key", key: "Escape" });
  // The open task's line is the composer itself.
  await step(w, "focus the answer", { op: "focus", target: "composer", by: "drive" });
  await step(w, "type the answer", { op: "type", text: ANSWER });
  await step(w, "send", { op: "key", key: "Enter" });
  await step(w, "marked", { op: "wait_idle", timeout_ms: TURN_MS });
  await shoot(w, "session-marked", "Session: an answer marked by the record, the next task open");
  await step(w, "more", { op: "click", target: "more", by: "drive" });
  await step(w, "symbols", { op: "click", target: "symbols", by: "drive" });
  await settle(w, 300);
  await shoot(w, "session-symbols", "Session: the symbol row under the answer line");
  await step(w, "more again", { op: "click", target: "more", by: "drive" });
  await step(w, "close the symbols", { op: "click", target: "symbols", by: "drive" });

  // The plan beside the lesson, then changed by the tutor.
  await step(w, "show the plan", { op: "click", target: "plan-toggle", by: "drive" });
  await settle(w, 600);
  await shoot(w, "session-plan", "Session with the plan panel shown");
  await step(w, "focus the line", { op: "focus", target: "composer", by: "drive" });
  await step(w, "ask for a change", { op: "type", text: CHANGE });
  await step(w, "send the change", { op: "key", key: "Enter" });
  await step(w, "the plan changes", { op: "wait_idle", timeout_ms: TURN_MS });
  await shoot(w, "session-plan-changed", "Session after the tutor changed the plan, panel shown");
  // A page the tutor cites, opened from its link: beside the lesson, or over it in an older window.
  await step(w, "focus the line to ask for a citation", { op: "focus", target: "composer", by: "drive" });
  await step(w, "ask for a citation", { op: "type", text: "CITE: where is that in the slides?" });
  await step(w, "send the citation request", { op: "key", key: "Enter" });
  await step(w, "the citation", { op: "wait_idle", timeout_ms: TURN_MS });
  await step(w, "open the cited page", { op: "click", target: "Midterm 2023, p. 2" });
  await settle(w, 1800);
  await shoot(w, "session-document", "Session: a page the tutor cited, opened from its link, while a task is open");
  const expanded = await w.op({ op: "click", target: "panel-expand", by: "drive" });
  if (expanded.ok) {
    await settle(w, 900);
    await shoot(w, "session-document-expanded", "Session: the document expanded to fill the session");
    await step(w, "put it beside again", { op: "click", target: "panel-expand", by: "drive" });
    await settle(w, 400);
  }
  await closeDocument(w, "Midterm 2023");
  await step(w, "scroll up", { op: "scroll", to: "top" });
  await settle(w, 300);
  await shoot(w, "session-earlier", "Session scrolled to its start");
  await step(w, "scroll down", { op: "scroll", to: "bottom" });
  await step(w, "hide the plan", { op: "click", target: "plan-toggle", by: "drive" });

  // A reply arriving: the stand-in writes slowly when asked to.
  // After talk, the line is talk's already.
  await step(w, "focus the line again", { op: "focus", target: "composer", by: "drive" });
  await step(w, "ask for a slow reply", { op: "type", text: "VERYSLOW: say a little more about the standard error" });
  await step(w, "send it", { op: "key", key: "Enter" });
  await step(w, "the reply begins", { op: "wait_reply", timeout_ms: TURN_MS });
  await settle(w, 700);
  await shoot(w, "session-reply", "Session: the tutor's reply arriving");
  await step(w, "the reply ends", { op: "wait_idle", timeout_ms: TURN_MS });

  // A file on the line, before it is sent.
  await step(w, "attach a photo", { op: "attach", path: photo });
  await settle(w, 400);
  await shoot(w, "session-attached", "Session: a photo on the line, before sending");
  await step(w, "take it off", { op: "click", target: "×" });

  // ⌘K and its workspace switcher.
  await step(w, "⌘K", { op: "key", key: "k", meta: true });
  await settle(w, 300);
  await shoot(w, "commands", "⌘K over a session");
  await step(w, "switch workspace", { op: "click", target: "command-switch", by: "drive" });
  await settle(w, 300);
  await shoot(w, "switch", "⌘K: switch workspace");
  await step(w, "close ⌘K", { op: "key", key: "Escape" });

  // Settings, section by section.
  await step(w, "settings", { op: "click", target: "settings", by: "drive" });
  await settle(w, 500);
  await shoot(w, "settings", "Settings, from the top: Reading");
  for (const [section, stem] of [
    ["Appearance", "settings-appearance"],
    ["Keys", "settings-keys"],
    ["Lessons", "settings-lessons"],
  ]) {
    await step(w, `settings: ${section}`, { op: "scroll", target: section });
    await w.op({ op: "scroll", by: -40 });
    await settle(w, 300);
    await shoot(w, stem, `Settings: ${section}`);
  }
  await step(w, "settings: the end", { op: "scroll", to: "bottom" });
  await settle(w, 300);
  await shoot(w, "settings-about", "Settings: Lessons and About, at the end");
  // A colour well's grid open, under the ground and text rows with their light and dark columns.
  await step(w, "settings: Appearance again", { op: "scroll", target: "Appearance" });
  await w.op({ op: "scroll", by: -40 });
  await step(w, "open the ground's colours", { op: "click", target: "colour-ground-light", by: "drive" });
  await settle(w, 300);
  await shoot(w, "settings-colour", "Settings: the ground's colours open");
  await step(w, "close the ground's colours", { op: "click", target: "colour-ground-light", by: "drive" });

  // The line drawn bare. A new window draws it as a soft field, so every photograph above has that look;
  // these are the same session with the other one, chosen in Settings as a learner chooses it.
  await step(w, "your line: bare", { op: "click", target: "Bare line" });
  await step(w, "the session", { op: "click", target: "tab-2", by: "drive" });
  await settle(w, 500);
  await shoot(w, "session-bare", "Session: the line drawn bare");
  await step(w, "focus the bare line", { op: "focus", target: "composer", by: "drive" });
  await step(w, "write on it", { op: "type", text: ANSWER });
  await settle(w, 300);
  await shoot(w, "session-bare-typed", "Session: words on the bare line");
  await step(w, "stuck", { op: "click", target: "stuck", by: "drive" });
  await settle(w, 300);
  await shoot(w, "session-bare-stuck", "Session: Stuck open on the bare line");
  await step(w, "close Stuck", { op: "key", key: "Escape" });
  await step(w, "more on the bare line", { op: "click", target: "more", by: "drive" });
  await step(w, "symbols on the bare line", { op: "click", target: "symbols", by: "drive" });
  await step(w, "stuck with the symbols open", { op: "click", target: "stuck", by: "drive" });
  await settle(w, 300);
  await shoot(w, "session-bare-stuck-symbols", "Session: Stuck open on the bare line, the symbols open under it");
  await step(w, "close Stuck again", { op: "key", key: "Escape" });
  await step(w, "close the symbols on the bare line", { op: "key", key: "Escape" });
  await step(w, "clear the line", { op: "type", text: "", replace: true });
  await step(w, "attach a photo on the bare line", { op: "attach", path: photo });
  await settle(w, 400);
  await shoot(w, "session-bare-attached", "Session: a photo on the bare line, before sending");
  await step(w, "take it off the bare line", { op: "click", target: "×" });
  await step(w, "focus the bare line for a slow reply", { op: "focus", target: "composer", by: "drive" });
  await step(w, "ask for a slow reply on the bare line", { op: "type", text: "VERYSLOW: and the margin of error?" });
  await step(w, "send it from the bare line", { op: "key", key: "Enter" });
  await step(w, "the reply begins over the bare line", { op: "wait_reply", timeout_ms: TURN_MS });
  await settle(w, 700);
  await shoot(w, "session-bare-reply", "Session: the tutor's reply arriving, the bare line under it saying how to stop it");
  await step(w, "the reply ends over the bare line", { op: "wait_idle", timeout_ms: TURN_MS });
  // The bare foot in a small window with the largest text: the open question, Stuck's row and the words.
  const wide = await step(w, "the window's size", { op: "frame" });
  await step(w, "720 by 560, bare", { op: "frame", width: 720, height: 560 });
  for (let n = 0; n < 11; n++) await w.op({ op: "menu_click", path: ["View", "Bigger Text"] });
  await step(w, "focus the small bare line", { op: "focus", target: "composer", by: "drive" });
  await step(w, "write on the small bare line", { op: "type", text: ANSWER });
  await step(w, "stuck on the small bare line", { op: "click", target: "stuck", by: "drive" });
  await settle(w, 400);
  await shoot(w, "session-bare-narrow-stuck", "Session: Stuck open on the bare line at 720 by 560, the largest text");
  await step(w, "close Stuck on the small bare line", { op: "key", key: "Escape" });
  await step(w, "clear the small bare line", { op: "type", text: "", replace: true });
  // The text size is kept with the window's settings, which outlast this run: put it back.
  await step(w, "the text back to its own size, bare", { op: "menu_click", path: ["View", "Actual Size"] });
  if (wide.ok) await step(w, "the window's size again", { op: "frame", width: wide.frame.width, height: wide.frame.height });
  // Before a lesson the line ends in More alone.
  await step(w, "a new session on the bare line", { op: "click", target: "new-session", by: "drive" });
  await settle(w, 800);
  await shoot(w, "session-rest-bare", "Session: a new session before it starts, the line bare");
  await step(w, "close the new session", { op: "key", key: "w", meta: true });
  await settle(w, 300);
  // Settings still has its tab, the third, so the bar's Settings button is not there: the tab goes back to it.
  await step(w, "settings again", { op: "click", target: "tab-3", by: "drive" });
  await step(w, "your line: soft", { op: "click", target: "Soft field" });
  await settle(w, 300);

  // Home again, with a session to carry on, and three tabs open.
  // The Home tab still shows the workspace's page: ‹ Home goes back to Home itself.
  await step(w, "home", { op: "click", target: "tab-1", by: "drive" });
  await settle(w, 300);
  await step(w, "‹ Home", { op: "click", target: "‹ Home" });
  await settle(w, 600);
  await shoot(w, "home-recent", "Home with a session to carry on; three tabs open");
  await step(w, "workspace again", { op: "click", target: "workspace-Intro Statistics", by: "drive" });
  await settle(w, 800);
  await shoot(w, "workspace-next", "Workspace with a session: the plan's next item, then the earlier sessions");
  await step(w, "to the foot", { op: "scroll", to: "bottom" });
  await settle(w, 300);
  await shoot(w, "workspace-work", "Workspace with a session, at its foot: each skill's days in Your work");
  await step(w, "back to the top", { op: "scroll", to: "top" });
  await step(w, "open the past paper", { op: "click", target: "Midterm 2023" });
  await settle(w, 1800);
  await shoot(w, "workspace-document", "Workspace: a past paper opened from its name");
  await closeDocument(w, "Midterm 2023");

  // Every item on the plan done: Home and the workspace page fall back to the session carried on, else New session.
  const planFile = join(scratch, "Intro-Statistics", "Study", "midterm-2", "plan.md");
  const planText = readFileSync(planFile, "utf8");
  writeFileSync(planFile, planText.replace(/\| todo \|/g, "| done |"));
  await step(w, "‹ Home, every item done", { op: "click", target: "‹ Home" });
  await settle(w, 800);
  await shoot(w, "home-all-done", "Home when every item on the plan is done: the session carried on");
  await step(w, "the workspace, every item done", { op: "click", target: "workspace-Intro Statistics", by: "drive" });
  await settle(w, 800);
  await shoot(w, "workspace-all-done", "Workspace when every item on the plan is done");
  writeFileSync(planFile, planText);
  await step(w, "‹ Home, small window", { op: "click", target: "‹ Home" });
  await step(w, "720 by 560", { op: "frame", width: 720, height: 560 });
  await settle(w, 800);
  await shoot(w, "home-narrow", "Home at 720 by 560");
  for (let n = 0; n < 11; n++) await w.op({ op: "menu_click", path: ["View", "Bigger Text"] });
  await settle(w, 400);
  await shoot(w, "home-big", "Home at 720 by 560, the largest text");
  // The text size is kept with the window's settings, which outlast this run: put it back.
  await step(w, "the text back to its own size", { op: "menu_click", path: ["View", "Actual Size"] });

  const menu = await step(w, "the menu bar", { op: "menu" });
  writeFileSync(join(out, "menu.json"), JSON.stringify(menu.menu ?? menu, null, 2));
  const errors = await step(w, "errors", { op: "errors" });
  if (errors.ok && errors.errors.length > 0) problem(`errors on the page: ${errors.errors.join(" | ")}`);
  await quit(w);

  // Lessons on the Claude Code engine, whose stand-in asks permission and can hit a usage limit.
  const pluginHome = join(scratch, "window-plugin");
  mkdirSync(pluginHome, { recursive: true });
  const kept = JSON.parse(readFileSync(join(scratch, "window", "state.json"), "utf8"));
  // A folder of notes that Magnesium was never set up in.
  const unset = join(scratch, "Reading-Group");
  mkdirSync(unset, { recursive: true });
  writeFileSync(join(unset, "chapter-1-notes.md"), "# Chapter 1\n\nMade-up notes.\n");
  writeFileSync(join(pluginHome, "state.json"), JSON.stringify({ recent: [...kept.recent, unset], folders: {}, engine: "plugin" }, null, 2));
  const p = await launch("plugin", pluginHome, pluginAgent);
  await step(p, "open the workspace", { op: "click", target: "workspace-Intro Statistics", by: "drive" });
  await settle(p, 800);
  await step(p, "new session", { op: "click", target: "new-session", by: "drive" });
  await settle(p, 800);
  await step(p, "focus the line", { op: "focus", target: "composer", by: "drive" });
  await step(p, "ask for something that needs permission", { op: "type", text: "PERMISSION: make the paper into a PDF" });
  await step(p, "send", { op: "key", key: "Enter" });
  await settle(p, 3000);
  // Each card in both of the line's looks, chosen in Settings as a learner chooses them, bare first. The stand-in
  // asks within a moment of the Return that sent the words, so the card leaves the keyboard on the line; ⇧⇥ takes
  // it to the card. Settings keeps its tab, the third, once it has one.
  let settingsTab = null;
  const lineLook = async (which) => {
    await step(p, `settings for the ${which}`, { op: "click", target: settingsTab ?? "settings", by: "drive" });
    settingsTab = "tab-3";
    await step(p, `your line: ${which}`, { op: "click", target: which });
    await step(p, "the session", { op: "click", target: "tab-2", by: "drive" });
    await settle(p, 500);
  };
  const cardShots = async (stem, what) => {
    for (const [which, suffix, words] of [
      ["Bare line", "-bare", ", on the bare line"],
      ["Soft field", "", ""],
    ]) {
      await lineLook(which);
      await step(p, `${stem}: the keyboard on the line`, { op: "focus", target: "composer", by: "drive" });
      await settle(p, 300);
      await shoot(p, `${stem}-line${suffix}`, `${what}, the keyboard still on the line${words}`);
      await step(p, `${stem}: ⇧⇥ to the card`, { op: "key", key: "Tab", shift: true });
      await settle(p, 300);
      await shoot(p, `${stem}${suffix}`, `${what}, the keyboard on the card${words}`);
    }
  };
  await cardShots("session-permission", "Session: the tutor asks permission (Claude Code engine)");
  await step(p, "allow once", { op: "click", target: "allow", by: "drive" });
  await step(p, "the reply ends", { op: "wait_idle", timeout_ms: TURN_MS });
  // The tutor asks to change the plan, which its rules did not cover.
  await step(p, "focus the line for the plan", { op: "focus", target: "composer", by: "drive" });
  await step(p, "ask to change the plan", { op: "type", text: "PLANEDIT: move the lab report to Friday" });
  await step(p, "send the plan change", { op: "key", key: "Enter" });
  await settle(p, 3000);
  await cardShots("session-permission-plan", "Session: the tutor asks to change the plan");
  await step(p, "refuse the plan change", { op: "click", target: "Refuse" });
  await step(p, "the plan reply ends", { op: "wait_idle", timeout_ms: TURN_MS });
  // The tutor asks to read a file outside the course.
  await step(p, "focus the line for outside", { op: "focus", target: "composer", by: "drive" });
  await step(p, "ask to read outside", { op: "type", text: "OUTSIDE: read my reading list" });
  await step(p, "send the outside read", { op: "key", key: "Enter" });
  await settle(p, 3000);
  await cardShots("session-permission-outside", "Session: the tutor asks to read a file outside the course");
  await step(p, "refuse the outside read", { op: "click", target: "Refuse" });
  await step(p, "the outside reply ends", { op: "wait_idle", timeout_ms: TURN_MS });
  await lineLook("Bare line");
  // A model the account cannot use: Claude Code's own line for it is a notice, with the list of models one press away.
  await step(p, "focus the line for the model", { op: "focus", target: "composer", by: "drive" });
  await step(p, "a model the account cannot use", { op: "type", text: "NOMODEL: and the intercept?" });
  await step(p, "send it", { op: "key", key: "Enter" });
  await step(p, "the notice", { op: "wait_idle", timeout_ms: TURN_MS });
  await settle(p, 500);
  await shoot(p, "session-model-unavailable-bare", "Session: a model the account cannot use, on the bare line");
  await step(p, "pick a model", { op: "click", target: "pick-model", by: "drive" });
  await settle(p, 500);
  await shoot(p, "session-model-unavailable-list-bare", "Session: Pick a model opens the list of models");
  await step(p, "close the list", { op: "key", key: "Escape" });
  // Settings still has its tab, the third, so the bar's Settings button is not there: the tab goes back to it.
  await step(p, "settings again", { op: "click", target: "tab-3", by: "drive" });
  await step(p, "your line: soft", { op: "click", target: "Soft field" });
  await step(p, "the session again", { op: "click", target: "tab-2", by: "drive" });
  await settle(p, 500);
  await shoot(p, "session-model-unavailable", "Session: a model the account cannot use");
  await step(p, "clear the line", { op: "type", text: "", replace: true });
  await step(p, "focus the line again", { op: "focus", target: "composer", by: "drive" });
  await step(p, "hit the usage limit", { op: "type", text: "LIMIT" });
  await step(p, "send it", { op: "key", key: "Enter" });
  await step(p, "the error", { op: "wait_idle", timeout_ms: TURN_MS });
  await settle(p, 500);
  await shoot(p, "session-error", "Session: the reply stopped by an error (a usage limit)");
  // A folder Magnesium was never set up in: its page says what setting it up does.
  await step(p, "home for the unset folder", { op: "click", target: "tab-1", by: "drive" });
  await settle(p, 300);
  // The Home tab still shows the workspace's page: ‹ Home goes back to Home itself.
  await step(p, "‹ Home for the unset folder", { op: "click", target: "‹ Home" });
  await settle(p, 500);
  await step(p, "open the folder", { op: "click", target: "Reading" });
  await settle(p, 800);
  await shoot(p, "workspace-not-set-up", "Workspace: a folder Magnesium is not set up in");
  await quit(p);

  await paperStates();

  await workRecord();

  // An app whose own magnesium program is newer than the one on this Mac: Home asks which to use. The app's copy
  // here only answers --version; it is never chosen.
  const newer = join(scratch, "bundled-magnesium");
  writeFileSync(newer, `#!/bin/sh\n[ "$1" = "--version" ] && { echo "magnesium 99.0.0"; exit 0; }\nexec '${runtime}' "$@"\n`, { mode: 0o755 });
  const replaceHome = join(scratch, "window-replace");
  mkdirSync(replaceHome, { recursive: true });
  writeFileSync(join(replaceHome, "state.json"), JSON.stringify({ recent: kept.recent, folders: {}, engine: "direct" }, null, 2));
  const n = await launch("replace", replaceHome, agent, { MAGNESIUM_APP_BUNDLED: newer, MAGNESIUM_APP_INSTALL_TO: join(scratch, "never-installed") });
  await settle(n, 800);
  await shoot(n, "home-newer-program", "Home: this app carries a newer magnesium program");
  // Before the learner chooses, a lesson runs on the copy already on this Mac, as Home says.
  await step(n, "open the workspace before choosing", { op: "click", target: "workspace-Intro Statistics", by: "drive" });
  await settle(n, 800);
  await step(n, "a new session before choosing", { op: "click", target: "new-session", by: "drive" });
  await settle(n, 800);
  await step(n, "focus the line", { op: "focus", target: "composer", by: "drive" });
  await step(n, "start the lesson", { op: "key", key: "Enter" });
  await step(n, "the first task, on the installed copy", { op: "wait_idle", timeout_ms: TURN_MS, quiet_ms: 2500 });
  const before = await n.op({ op: "text" });
  if (/not set up for lessons/.test(String(before.text ?? ""))) problem("replace: the lesson did not start before choosing a copy");
  const opened = await n.op({ op: "session" });
  if (!opened.ok || opened.phase !== "lesson" || !opened.task) problem(`replace: no task in the lesson before choosing a copy: ${JSON.stringify(opened)}`);
  await shoot(n, "session-different-program", "Session: a lesson on the copy already on this Mac, before choosing");
  await quit(n);

  // Home as each state of this Mac draws it, on the course with its plan and no session yet.
  const homeWith = async (name, stem, what, { withAgent = agent, more = {}, recent = kept.recent } = {}) => {
    const home = join(scratch, `window-${name}`);
    mkdirSync(home, { recursive: true });
    writeFileSync(join(home, "state.json"), JSON.stringify({ recent, folders: {}, engine: "direct" }, null, 2));
    const h = await launch(name, home, withAgent, more);
    await settle(h, 800);
    await shoot(h, stem, what);
    await quit(h);
  };
  const never = { MAGNESIUM_APP_INSTALL_TO: join(scratch, "never-installed") };
  const fake = (name, body) => {
    const path = join(scratch, name);
    writeFileSync(path, `#!/bin/sh\n${body}\n`, { mode: 0o755 });
    return path;
  };
  const older = fake("bundled-older", `[ "$1" = "--version" ] && { echo "magnesium 0.0.1"; exit 0; }\nexec '${runtime}' "$@"`);
  const sameNumber = fake("bundled-same", `[ "$1" = "--version" ] && { '${runtime}' --version | sed 's/+.*//; s/$/+0000000/'; exit 0; }\nexec '${runtime}' "$@"`);
  const tooOld = fake("magnesium-too-old", `[ "$1" = digest ] && [ "$PWD" = / ] && { echo '{"window_features": 1}'; exit 0; }\nexec '${runtime}' "$@"`);
  const signedOut = fake("claude-signed-out", `[ "$1 $2" = "auth status" ] && { echo '{"loggedIn": false, "authMethod": "none", "apiProvider": "firstParty"}'; exit 0; }\nexec '${agent}' "$@"`);
  await homeWith("older", "home-older-program", "Home: this Mac has a newer magnesium program than this app", { more: { ...never, MAGNESIUM_APP_BUNDLED: older } });
  await homeWith("same", "home-same-program", "Home: this app carries another build of the same version", { more: { ...never, MAGNESIUM_APP_BUNDLED: sameNumber } });
  await homeWith("too-old", "home-too-old", "Home: the program on this Mac is too old for this window", { more: { ...never, MAGNESIUM_APP_RUNTIME: tooOld, MAGNESIUM_APP_BUNDLED: newer } });
  await homeWith("signed-out", "home-signed-out", "Home: Claude Code is not signed in", { withAgent: signedOut });
  await homeWith("signed-out-newer", "home-signed-out-newer", "Home: not signed in, and this app carries a newer program", { withAgent: signedOut, more: { ...never, MAGNESIUM_APP_BUNDLED: newer } });
  await homeWith("no-claude", "home-no-claude", "Home: Claude Code is not on this Mac", { withAgent: join(scratch, "no-claude-here") });
  // The first open with one workspace: with its plan, and without one.
  await homeWith("one", "home-one", "Home: one workspace, with a plan, never studied in", { recent: [kept.recent[0]] });
  await homeWith("one-no-plan", "home-one-no-plan", "Home: one workspace, no plan, never studied in", { recent: [kept.recent[1]] });
} catch (error) {
  if (error !== "only") problem(`stopped: ${error?.stack ?? error}`);
}

run.finished = new Date().toISOString();
save();
console.log(run.problems.length === 0 ? `pages: ${run.shots.length} pages in ${out}` : `pages: ${run.problems.length} problem(s); see ${join(out, "run.json")}`);
process.exit(run.problems.length === 0 ? 0 : 1);
