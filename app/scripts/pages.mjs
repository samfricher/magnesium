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
// PAGES_ONLY=answer photographs only the page while a task is open (answer-*).

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

/** Close a document: from the panel beside the page or lesson where there is one, else the sheet over it, with Esc. */
async function closeDocument(window, title) {
  const closed = await window.op({ op: "click", target: `close-doc-${title}`, by: "drive" });
  if (!closed.ok) await step(window, `close ${title}`, { op: "key", key: "Escape" });
  else run.steps.push({ label: `close ${title} in the panel` });
  await settle(window, 300);
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

/** A window of its own on the course, with its own window home: the course's folders listed, no session yet. */
async function ownWindow(name, withAgent = agent, engine = "direct") {
  const home = join(scratch, `window-${name}`);
  mkdirSync(home, { recursive: true });
  const listed = JSON.parse(readFileSync(join(scratch, "window", "state.json"), "utf8"));
  // Not the tabs the main window left: the session here is always the second tab.
  const { window: _tabs, ...kept } = listed;
  writeFileSync(join(home, "state.json"), JSON.stringify({ ...kept, folders: {}, engine }, null, 2));
  return { home, window: await launch(name, home, withAgent) };
}

/** The line's look, chosen in Settings as a learner chooses it: from the bar's Settings, or Settings' own tab once it has one. */
async function lineLook(w, look) {
  const bar = await w.op({ op: "click", target: "settings", by: "drive" });
  if (!bar.ok) await step(w, `settings for the ${look}`, { op: "click", target: "Settings" });
  await step(w, `your line: ${look}`, { op: "click", target: look });
  // Settings closed again, so a session is always the second tab.
  await step(w, `close Settings after the ${look}`, { op: "key", key: "w", meta: true });
  await settle(w, 300);
}

/** The page while a task is open (the blank page): the bare line first, then the soft field; each in light and dark. */
async function answerPages() {
  // A note of the course's own, so a document that is not a past paper can be open beside the task.
  const notes = join(scratch, "Intro-Statistics", "Study", "materials", "week6-notes.md");
  writeFileSync(notes, "# Week 6: intervals for a mean\n\nA 95% interval for a mean is x̄ ± t* · s/√n, with t* from the t table at n − 1 degrees of freedom.\n\nThe standard error s/√n shrinks as the sample grows.\n");
  const looked = async (w, want, label) => {
    const at = await w.op({ op: "session" });
    if (at.page !== want) problem(`${label}: the page is ${JSON.stringify(at.page)}, not ${JSON.stringify(want)}`);
  };
  const turn = async (w, label) => {
    await step(w, label, { op: "key", key: "ArrowUp", meta: true });
    await settle(w, 400);
  };

  const { home, window: a } = await ownWindow("answer");
  await lineLook(a, "Bare line");
  await step(a, "the workspace", { op: "click", target: "tab-1", by: "drive" });
  await step(a, "open the workspace", { op: "click", target: "workspace-Intro Statistics", by: "drive" });
  await settle(a, 800);
  await step(a, "a new session", { op: "click", target: "new-session", by: "drive" });
  await settle(a, 800);
  await step(a, "focus the line", { op: "focus", target: "composer", by: "drive" });
  await step(a, "start", { op: "key", key: "Enter" });
  await step(a, "the first task", { op: "wait_idle", timeout_ms: TURN_MS, quiet_ms: 2500 });
  await looked(a, "open", "the first reply");
  await shoot(a, "answer-bare-first-reply", "Answer: the reply that set the task, read where it was written (the page stays whole)");
  await turn(a, "⌘↑: only the task");
  await looked(a, "folded", "⌘↑ after the first reply");
  await shoot(a, "answer-bare-rest", "Answer: a task open at rest, the page is the task (bare line)");
  // The menu bar while the page is the task: Session > Only the Task / The Lesson So Far, with its key.
  const menu = await step(a, "the menu bar, folded", { op: "menu" });
  writeFileSync(join(out, "answer-menu.json"), JSON.stringify(menu.menu ?? menu, null, 2));
  await step(a, "write an answer", { op: "type", text: ANSWER });
  await settle(a, 300);
  await shoot(a, "answer-bare-typed", "Answer: words on the bare line, under the task alone");
  await step(a, "send it", { op: "key", key: "Enter" });
  await step(a, "the reply", { op: "wait_idle", timeout_ms: TURN_MS });
  await settle(a, 600);
  await looked(a, "open", "the reply that sets the next task");
  await shoot(a, "answer-bare-next-task", "Answer: the reply ends with a new task open; the page stays whole to be read, the mark under the answer");
  await step(a, "write on the line", { op: "type", text: "2.064 × 2" });
  await settle(a, 400);
  await looked(a, "folded", "writing after the reply");
  await shoot(a, "answer-bare-next-written", "Answer: writing on the line turns the page to the new task");
  await step(a, "clear the line", { op: "type", text: "", replace: true });
  await step(a, "stuck", { op: "click", target: "stuck", by: "drive" });
  await settle(a, 300);
  await shoot(a, "answer-bare-stuck", "Answer: Stuck's row open, under the task alone");
  await step(a, "close Stuck", { op: "key", key: "Escape" });
  await step(a, "more", { op: "click", target: "more", by: "drive" });
  await step(a, "symbols", { op: "click", target: "symbols", by: "drive" });
  await settle(a, 300);
  await shoot(a, "answer-bare-symbols", "Answer: the symbols open under the line");
  await step(a, "more again", { op: "click", target: "more", by: "drive" });
  await step(a, "close the symbols", { op: "click", target: "symbols", by: "drive" });
  await step(a, "attach a photo", { op: "attach", path: photo });
  await settle(a, 400);
  await shoot(a, "answer-bare-attached", "Answer: a photo of working on the line");
  await step(a, "take it off", { op: "click", target: "×" });
  await step(a, "focus the line again", { op: "focus", target: "composer", by: "drive" });
  await turn(a, "⌘↑: the lesson so far");
  await looked(a, "open", "⌘↑ on the folded page");
  await shoot(a, "answer-bare-looked-back", "Answer: ⌘↑ opens the whole lesson over the task, at its end");
  await step(a, "fold it from its line", { op: "click", target: "look-back", by: "drive" });
  await settle(a, 400);
  await looked(a, "folded", "the line under the heading");
  await shoot(a, "answer-bare-folded-again", "Answer: folded again from “only the task”");

  // Talk while the task is open: not an answer, and the task stays.
  await step(a, "focus for talk", { op: "focus", target: "composer", by: "drive" });
  await step(a, "a question off the task", { op: "type", text: "by the way, why does s divide by n − 1?" });
  await step(a, "send it as talk", { op: "key", key: "Enter", alt: true });
  await step(a, "the talk's reply", { op: "wait_idle", timeout_ms: TURN_MS });
  await settle(a, 600);
  await looked(a, "open", "the reply to talk");
  await shoot(a, "answer-bare-talk-reply", "Answer: talk while a task is open, and its reply; the task stays open");
  await turn(a, "⌘↑ after the talk");
  await shoot(a, "answer-bare-talk-folded", "Answer: after the talk, the same task is the page again");

  // The plan and documents beside it.
  await step(a, "show the panel", { op: "click", target: "plan-toggle", by: "drive" });
  await settle(a, 600);
  await shoot(a, "answer-bare-plan", "Answer: the plan beside the task; the plan stays open");
  await step(a, "open a document", { op: "click", target: "open-document", by: "drive" });
  await settle(a, 800);
  await step(a, "the week's notes", { op: "click", target: "Week 6 notes" });
  await settle(a, 1000);
  await looked(a, "open", "a document opened while the page was the task");
  await shoot(a, "answer-bare-document-opened", "Answer: a document picked while the page is the task opens, with the lesson");
  await step(a, "focus before folding", { op: "focus", target: "composer", by: "drive" });
  await turn(a, "⌘↑ with the notes open");
  await looked(a, "folded", "⌘↑ with the notes open");
  await shoot(a, "answer-bare-document", "Answer: the notes beside the task are closed, and say how to have them back");
  const expand = await step(a, "expand the panel", { op: "click", target: "panel-expand", by: "drive" });
  if (expand.ok) {
    await settle(a, 800);
    await shoot(a, "answer-bare-document-expanded", "Answer: notes that filled the session step aside for the task");
    await step(a, "focus in the expanded state", { op: "focus", target: "composer", by: "drive" });
    await turn(a, "⌘↑ with the notes expanded");
    await settle(a, 600);
    await shoot(a, "answer-bare-document-expanded-open", "Answer: ⌘↑ brings the expanded notes back");
    await step(a, "the notes beside again", { op: "click", target: "panel-expand", by: "drive" });
    await settle(a, 400);
  }
  await closeDocument(a, "Week 6 notes");
  await step(a, "open the past paper", { op: "click", target: "open-document", by: "drive" });
  await settle(a, 800);
  await step(a, "Midterm 2023", { op: "click", target: "open-Midterm 2023", by: "drive" });
  await settle(a, 1800);
  await step(a, "focus with the paper", { op: "focus", target: "composer", by: "drive" });
  const paperPage = await a.op({ op: "session" });
  if (paperPage.page === "open") await turn(a, "⌘↑ with the paper open");
  await shoot(a, "answer-bare-paper", "Answer: a past paper beside the task stays open, as its links do");
  await closeDocument(a, "Midterm 2023");
  await step(a, "hide the panel", { op: "click", target: "plan-toggle", by: "drive" });

  // A small window with the largest text: folded, written on, and opened.
  const wide = await step(a, "the window's size", { op: "frame" });
  await step(a, "720 by 560", { op: "frame", width: 720, height: 560 });
  for (let n = 0; n < 11; n++) await a.op({ op: "menu_click", path: ["View", "Bigger Text"] });
  await settle(a, 400);
  await shoot(a, "answer-bare-narrow-big", "Answer: the task alone at 720 by 560, the largest text");
  await step(a, "focus the small line", { op: "focus", target: "composer", by: "drive" });
  await step(a, "write on the small line", { op: "type", text: ANSWER });
  await settle(a, 300);
  await shoot(a, "answer-bare-narrow-big-typed", "Answer: words on the line at 720 by 560, the largest text");
  await turn(a, "⌘↑ in the small window");
  await settle(a, 400);
  await shoot(a, "answer-bare-narrow-big-open", "Answer: the lesson opened at 720 by 560, “only the task” at the top edge");
  await turn(a, "⌘↑ again in the small window");
  await step(a, "clear the small line", { op: "type", text: "", replace: true });
  await step(a, "the text back to its own size", { op: "menu_click", path: ["View", "Actual Size"] });
  if (wide.ok) await step(a, "the window's size again", { op: "frame", width: wide.frame.width, height: wide.frame.height });
  await settle(a, 400);

  // The soft field.
  await lineLook(a, "Soft field");
  await step(a, "back to the session", { op: "click", target: "tab-2", by: "drive" });
  await settle(a, 500);
  await shoot(a, "answer-soft-rest", "Answer: a task open at rest, the soft field");
  await step(a, "focus the soft line", { op: "focus", target: "composer", by: "drive" });
  await step(a, "write on the soft line", { op: "type", text: ANSWER });
  await settle(a, 300);
  await shoot(a, "answer-soft-typed", "Answer: words on the soft field, under the task alone");
  await step(a, "stuck on the soft line", { op: "click", target: "stuck", by: "drive" });
  await settle(a, 300);
  await shoot(a, "answer-soft-stuck", "Answer: Stuck's row open on the soft field");
  await step(a, "close Stuck on the soft line", { op: "key", key: "Escape" });
  await step(a, "clear the soft line", { op: "type", text: "", replace: true });
  await lineLook(a, "Bare line");
  await step(a, "the session, bare", { op: "click", target: "tab-2", by: "drive" });
  await settle(a, 400);

  // A reply arriving: the whole lesson, as before, until it is read.
  await step(a, "focus for a slow reply", { op: "focus", target: "composer", by: "drive" });
  await step(a, "ask for a slow reply", { op: "type", text: "VERYSLOW: and what does the 95% mean?" });
  await step(a, "send it as talk", { op: "key", key: "Enter", alt: true });
  await step(a, "the reply begins", { op: "wait_reply", timeout_ms: TURN_MS });
  await settle(a, 700);
  await shoot(a, "answer-bare-reply-arriving", "Answer: the tutor writing; the whole lesson is on the page");
  await step(a, "the slow reply ends", { op: "wait_idle", timeout_ms: TURN_MS });
  // A session closed and opened again with its task open: the page is the task at once.
  await step(a, "pose the task again", { op: "focus", target: "composer", by: "drive" });
  await step(a, "back to it", { op: "type", text: "back to the task" });
  await step(a, "send", { op: "key", key: "Enter", alt: true });
  await step(a, "the task again", { op: "wait_idle", timeout_ms: TURN_MS });
  await quit(a);
  const again = await launch("answer-again", home);
  await step(again, "the session again", { op: "click", target: "tab-2", by: "drive" });
  await settle(again, 1200);
  await looked(again, "folded", "a session opened again with its task open");
  await shoot(again, "answer-bare-reopened", "Answer: a session closed and opened again with its task open");
  await lineLook(again, "Soft field");
  await quit(again);

  // Claude Code's engine: a task the tutor set but did not save (#98), and a permission card waiting.
  const { window: p } = await ownWindow("answer-plugin", pluginAgent, "plugin");
  await lineLook(p, "Bare line");
  await step(p, "the workspace", { op: "click", target: "tab-1", by: "drive" });
  await step(p, "open the workspace", { op: "click", target: "workspace-Intro Statistics", by: "drive" });
  await settle(p, 800);
  await step(p, "a new session", { op: "click", target: "new-session", by: "drive" });
  await settle(p, 800);
  await step(p, "focus the line", { op: "focus", target: "composer", by: "drive" });
  await step(p, "a task the record does not name", { op: "type", text: "A task, please.\n\n**Task.** A sample of 49 has s = 14. What is the standard error of the mean?" });
  await step(p, "send", { op: "key", key: "Enter" });
  await step(p, "the reply", { op: "wait_idle", timeout_ms: TURN_MS });
  await settle(p, 1500);
  await turn(p, "⌘↑ on the unsaved task");
  await looked(p, "folded", "a task found by its bold word");
  await shoot(p, "answer-bare-unsaved-task", "Answer: a task the tutor set but did not save (#98), found by its bold word");
  await step(p, "focus for permission", { op: "focus", target: "composer", by: "drive" });
  await step(p, "ask for something that needs permission", { op: "type", text: "PERMISSION: make the paper into a PDF" });
  await step(p, "send it as talk", { op: "key", key: "Enter", alt: true });
  await settle(p, 3000);
  await looked(p, null, "a permission card waiting");
  await shoot(p, "answer-bare-permission", "Answer: a permission card waiting; the whole lesson is on the page");
  await step(p, "allow once", { op: "click", target: "allow", by: "drive" });
  await step(p, "the reply ends", { op: "wait_idle", timeout_ms: TURN_MS });
  await lineLook(p, "Soft field");
  await quit(p);
}

try {
  if (process.env.PAGES_ONLY === "work") {
    await workRecord();
    throw "only";
  }
  if (process.env.PAGES_ONLY === "answer") {
    await answerPages();
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
  await shoot(p, "session-permission", "Session: the tutor asks permission (Claude Code engine)");
  // The same card on the bare line, chosen in Settings as a learner chooses it, then the soft field again.
  await step(p, "settings", { op: "click", target: "settings", by: "drive" });
  await step(p, "your line: bare", { op: "click", target: "Bare line" });
  await step(p, "the session", { op: "click", target: "tab-2", by: "drive" });
  await settle(p, 500);
  await shoot(p, "session-permission-bare", "Session: the tutor asks permission, on the bare line");
  await step(p, "allow once", { op: "click", target: "allow", by: "drive" });
  await step(p, "the reply ends", { op: "wait_idle", timeout_ms: TURN_MS });
  // The tutor asks to change the plan, which its rules did not cover.
  await step(p, "focus the line for the plan", { op: "focus", target: "composer", by: "drive" });
  await step(p, "ask to change the plan", { op: "type", text: "PLANEDIT: move the lab report to Friday" });
  await step(p, "send the plan change", { op: "key", key: "Enter" });
  await settle(p, 3000);
  await shoot(p, "session-permission-plan-bare", "Session: the tutor asks to change the plan, on the bare line");
  await step(p, "refuse the plan change", { op: "click", target: "Refuse" });
  await step(p, "the plan reply ends", { op: "wait_idle", timeout_ms: TURN_MS });
  // The tutor asks to read a file outside the course.
  await step(p, "focus the line for outside", { op: "focus", target: "composer", by: "drive" });
  await step(p, "ask to read outside", { op: "type", text: "OUTSIDE: read my reading list" });
  await step(p, "send the outside read", { op: "key", key: "Enter" });
  await settle(p, 3000);
  await shoot(p, "session-permission-outside-bare", "Session: the tutor asks to read a file outside the course, on the bare line");
  await step(p, "refuse the outside read", { op: "click", target: "Refuse" });
  await step(p, "the outside reply ends", { op: "wait_idle", timeout_ms: TURN_MS });
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

  await answerPages();
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
