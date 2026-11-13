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
// visits, menu.json (the menu bar, which a photograph of the window does
// not show) and run.json (each step, and any problem). It exits 1 when a step
// failed, after doing every step it can.

import { spawn, execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, openSync, readFileSync, writeFileSync } from "node:fs";
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

async function launch(name, home, withAgent = agent) {
  const driveDir = join(scratch, "drive", name);
  mkdirSync(driveDir, { recursive: true });
  const env = { ...process.env, MAGNESIUM_APP_HOME: home, MAGNESIUM_APP_RUNTIME: runtime, MAGNESIUM_APP_DRIVE: driveDir, MAGNESIUM_APP_AGENT: withAgent };
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

try {
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
  // The open task's line is the composer itself.
  await step(w, "focus the answer", { op: "focus", target: "composer", by: "drive" });
  await step(w, "type the answer", { op: "type", text: ANSWER });
  await step(w, "send", { op: "key", key: "Enter" });
  await step(w, "marked", { op: "wait_idle", timeout_ms: TURN_MS });
  await shoot(w, "session-marked", "Session: an answer marked by the record, the next task open");
  await step(w, "symbols", { op: "click", target: "symbols", by: "drive" });
  await settle(w, 300);
  await shoot(w, "session-symbols", "Session: the symbol row under the answer line");
  await step(w, "close the symbols", { op: "click", target: "symbols", by: "drive" });

  // The plan beside the lesson, then changed by the tutor.
  await step(w, "show the plan", { op: "click", target: "plan-toggle", by: "drive" });
  await settle(w, 600);
  await shoot(w, "session-plan", "Session with the plan panel shown");
  await step(w, "focus talk", { op: "focus", target: "talk", by: "drive" });
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

  // Home again, with a session to carry on, and three tabs open.
  // The Home tab still shows the workspace's page: ‹ Home goes back to Home itself.
  await step(w, "home", { op: "click", target: "tab-1", by: "drive" });
  await settle(w, 300);
  await step(w, "‹ Home", { op: "click", target: "‹ Home" });
  await settle(w, 600);
  await shoot(w, "home-recent", "Home with a session to carry on; three tabs open");
  await step(w, "workspace again", { op: "click", target: "workspace-Intro Statistics", by: "drive" });
  await settle(w, 800);
  await step(w, "your work", { op: "click", target: "Your work" });
  await settle(w, 800);
  await step(w, "to the foot", { op: "scroll", to: "bottom" });
  await settle(w, 300);
  await shoot(w, "workspace-work", "Workspace with a session, Your work open, at its foot");
  await step(w, "back to the top", { op: "scroll", to: "top" });
  await step(w, "open the past paper", { op: "click", target: "Midterm 2023" });
  await settle(w, 1800);
  await shoot(w, "workspace-document", "Workspace: a past paper opened from its material");
  await closeDocument(w, "Midterm 2023");

  const menu = await step(w, "the menu bar", { op: "menu" });
  writeFileSync(join(out, "menu.json"), JSON.stringify(menu.menu ?? menu, null, 2));
  const errors = await step(w, "errors", { op: "errors" });
  if (errors.ok && errors.errors.length > 0) problem(`errors on the page: ${errors.errors.join(" | ")}`);
  await quit(w);

  // Lessons on the Claude Code engine, whose stand-in asks permission and can hit a usage limit.
  const pluginHome = join(scratch, "window-plugin");
  mkdirSync(pluginHome, { recursive: true });
  const kept = JSON.parse(readFileSync(join(scratch, "window", "state.json"), "utf8"));
  writeFileSync(join(pluginHome, "state.json"), JSON.stringify({ recent: kept.recent, folders: {}, engine: "plugin" }, null, 2));
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
  await step(p, "allow once", { op: "click", target: "Allow once" });
  await step(p, "the reply ends", { op: "wait_idle", timeout_ms: TURN_MS });
  await step(p, "focus the line again", { op: "focus", target: "composer", by: "drive" });
  await step(p, "hit the usage limit", { op: "type", text: "LIMIT" });
  await step(p, "send it", { op: "key", key: "Enter" });
  await step(p, "the error", { op: "wait_idle", timeout_ms: TURN_MS });
  await settle(p, 500);
  await shoot(p, "session-error", "Session: the reply stopped by an error (a usage limit)");
  await quit(p);
} catch (error) {
  problem(`stopped: ${error?.stack ?? error}`);
}

run.finished = new Date().toISOString();
save();
console.log(run.problems.length === 0 ? `pages: ${run.shots.length} pages in ${out}` : `pages: ${run.problems.length} problem(s); see ${join(out, "run.json")}`);
process.exit(run.problems.length === 0 ? 0 : 1);
