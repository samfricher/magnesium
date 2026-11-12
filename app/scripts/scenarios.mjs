// Play the evidence contract's five scenarios in the real window, through
// the harness (src/harness.ts), and photograph each step. Builds nothing.
//
//   node scripts/scenarios.mjs <app-binary> <out-dir> [--live]
//
// <app-binary> is the window's program, such as
// target/release/bundle/macos/Magnesium.app/Contents/MacOS/magnesium-window.
// It works in a scratch course made by scripts/scratch-course.sh, under the
// system's temporary folder, with a scratch window home: none of your study
// folders, settings or agent sessions are touched. Lessons run on Magnesium
// (the direct engine) with the stand-in model (crates/core/tests/
// stand_in_model.py), which starts no model and reaches no network. With
// --live they run on the real `claude` on your PATH, on your subscription.
//
// It writes to <out-dir>:
//   <stem>-dark.png, <stem>-light.png  each step, in both schemes, with the
//                    contract's stems: s1-cold-first, s1-cold, s2-start,
//                    s2-answer, s3-before, s3-after, s4-offplan, s4-return,
//                    s5-week, s5-week-next
//   perf.json        startup of each opening, and input latency mid-lesson
//   attempts.json    the runtime's `attempts --json`, before and after
//   plan-before.json, plan-after.json   core.plan() around scenario 3
//   transcript.md    every message sent, with its intent, and every reply
//   run.json         what each step did, and any problem
// It needs only what a concept that follows the contract gives it: ready(),
// data-drive="start" and data-drive="composer", and every action reachable
// from there. It exits 1 when a step failed, after doing every step it can.

import { spawn, execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, openSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { drive } from "./drive.mjs";

const APP = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
const live = args.includes("--live");
const [binary, outArg] = args.filter((arg) => !arg.startsWith("--"));
if (!binary || !outArg) {
  console.error("usage: node scripts/scenarios.mjs <app-binary> <out-dir> [--live]");
  process.exit(64);
}
if (!existsSync(binary)) {
  console.error(`scenarios: no window program at ${binary}`);
  process.exit(66);
}
const out = resolve(outArg);
mkdirSync(out, { recursive: true });

// A reply from the real model takes far longer than the stand-in's.
const TURN_MS = live ? 300_000 : 60_000;
const OPEN_MS = 60_000;
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

// The words the learner types, from the brief's scenarios.
const ANSWER = "The standard error is s/√n = 10/√25 = 2 minutes.";
const CHANGE = "I skipped three days and the midterm moved, change the plan";
const OFF_PLAN = "By the way, why do we divide by n − 1 for the sample standard deviation?";
const BACK = "OK, back to the task.";

const run = { started: new Date().toISOString(), live, binary, steps: [], problems: [] };
const problem = (what) => {
  run.problems.push(what);
  console.error(`scenarios: ${what}`);
};
const save = () => writeFileSync(join(out, "run.json"), JSON.stringify(run, null, 2));

// --- The scratch course -------------------------------------------------------

const scratch = mkdtempSync(join(tmpdir(), "mg-scenarios-"));
run.scratch = scratch;
const course = execFileSync("sh", [join(APP, "scripts/scratch-course.sh"), scratch], { encoding: "utf8" }).trim();
const runtime = join(scratch, "magnesium");
const windowHome = join(scratch, "window");

let agent = null;
if (!live) {
  agent = join(scratch, "claude");
  writeFileSync(
    agent,
    `#!/bin/sh\nexport STAND_IN_LOG='${join(scratch, "model.log")}'\nexec python3 '${join(APP, "crates/core/tests/stand_in_model.py")}' "$@"\n`,
    { mode: 0o755 },
  );
}

function attempts() {
  try {
    return JSON.parse(execFileSync(runtime, ["attempts", "--json"], { cwd: course, encoding: "utf8" }));
  } catch (error) {
    return { error: String(error) };
  }
}
const attemptsBefore = attempts();

// --- The window ---------------------------------------------------------------

const launches = {};

/** Start the window. `home` is its scratch home; `folder` opens a study folder at once, or none for the library. */
async function launch(name, { home = windowHome, folder = null } = {}) {
  const driveDir = join(out, "drive", name);
  rmSync(driveDir, { recursive: true, force: true });
  mkdirSync(driveDir, { recursive: true });
  const perfFile = join(out, `perf-${name}.json`);
  rmSync(perfFile, { force: true });
  const env = {
    ...process.env,
    MAGNESIUM_APP_HOME: home,
    MAGNESIUM_APP_RUNTIME: runtime,
    MAGNESIUM_APP_DRIVE: driveDir,
    MAGNESIUM_APP_PERF: perfFile,
  };
  delete env.MAGNESIUM_APP_SELFTEST;
  if (folder) env.MAGNESIUM_APP_FOLDER = folder;
  else delete env.MAGNESIUM_APP_FOLDER;
  if (agent) env.MAGNESIUM_APP_AGENT = agent;
  else delete env.MAGNESIUM_APP_AGENT;
  const log = openSync(join(out, `window-${name}.log`), "w");
  const child = spawn(binary, [], { env, stdio: ["ignore", log, log] });
  const exited = new Promise((done) => child.on("exit", (code) => done(code)));
  const window = { name, child, exited, driveDir, perfFile, op: (op, options) => drive(driveDir, op, options) };
  launches[name] = window;
  // Ready: the first screen has its real content.
  const start = Date.now();
  for (;;) {
    const answer = await window.op({ op: "ready" }, { timeoutMs: 5_000 });
    if (answer.ok && answer.ready) break;
    if (Date.now() - start > OPEN_MS) {
      problem(`${name}: the window never called ready() within ${OPEN_MS / 1000} s`);
      break;
    }
    await sleep(250);
  }
  await step(window, `${name}: settle`, { op: "wait_idle", timeout_ms: OPEN_MS });
  return window;
}

async function step(window, label, op) {
  const answer = await window.op(op);
  run.steps.push({ label, op, answer: op.op === "transcript" || op.op === "text" ? "(kept apart)" : answer });
  if (!answer.ok) problem(`${label}: ${answer.error ?? "not ok"}`);
  save();
  return answer;
}

/** Photograph the window in both schemes, as <stem>-dark.png and <stem>-light.png. */
async function shoot(window, stem) {
  for (const scheme of ["dark", "light"]) {
    await window.op({ op: "scheme", scheme });
    await sleep(120);
    const file = join(out, `${stem}-${scheme}.png`);
    try {
      execFileSync("sh", [join(APP, "scripts/capture.sh"), String(window.child.pid), file], { stdio: ["ignore", "ignore", "pipe"] });
      run.steps.push({ label: `photo ${stem}-${scheme}` });
    } catch (error) {
      problem(`photo ${stem}-${scheme}: ${String(error.stderr ?? error).trim()}`);
    }
  }
  await window.op({ op: "scheme", scheme: "system" });
  save();
}

/** Type into the composer and send with Return, as the learner does. */
async function say(window, label, words) {
  await step(window, `${label}: focus the composer`, { op: "focus", target: "composer", by: "drive" });
  await step(window, `${label}: type`, { op: "type", text: words });
  await step(window, `${label}: Return`, { op: "key", key: "Enter" });
}

async function quit(window) {
  await window.op({ op: "quit" }, { timeoutMs: 10_000 });
  const code = await Promise.race([window.exited, sleep(15_000).then(() => "timeout")]);
  if (code === "timeout") {
    problem(`${window.name}: the window did not quit; killed`);
    window.child.kill("SIGKILL");
  }
}

const transcripts = [];
async function keepTranscript(window, heading) {
  const answer = await window.op({ op: "transcript" });
  transcripts.push({ heading, entries: answer.ok ? answer.transcript : [] });
}

async function readPlan(window, file) {
  const answer = await step(window, `plan into ${file}`, { op: "plan" });
  writeFileSync(join(out, file), JSON.stringify(answer.ok ? answer.plan : { error: answer.error }, null, 2));
}

const perf = { startup_ms: {}, input: null };
const keepStartup = (window) => {
  try {
    perf.startup_ms[window.name] = JSON.parse(readFileSync(window.perfFile, "utf8")).startup_ms ?? null;
  } catch {
    perf.startup_ms[window.name] = null;
    problem(`${window.name}: no perf file`);
  }
};

// --- The scenarios ------------------------------------------------------------

try {
  // The very first open on a new Mac: nothing set up yet.
  const firstHome = join(scratch, "window-first");
  mkdirSync(firstHome, { recursive: true });
  const first = await launch("first", { home: firstHome });
  await shoot(first, "s1-cold-first");
  keepStartup(first);
  await quit(first);

  // 1. Open the app cold, with three courses in the library.
  const main = await launch("main");
  keepStartup(main);
  await shoot(main, "s1-cold");

  // 2. Start a lesson: the single action the first screen offers, then
  // answer the tutor's first task.
  await step(main, "s2: start", { op: "click", target: "start", by: "drive" });
  await step(main, "s2: the first words", { op: "wait_idle", timeout_ms: TURN_MS, quiet_ms: 2500 });
  await shoot(main, "s2-start");
  await say(main, "s2: answer", ANSWER);
  await step(main, "s2: marked", { op: "wait_idle", timeout_ms: TURN_MS });
  await shoot(main, "s2-answer");
  const latency = await step(main, "perf: input latency", { op: "perf" });
  if (latency.ok) perf.input = { samples: latency.samples, median_ms: latency.median_ms, p95_ms: latency.p95_ms, max_ms: latency.max_ms };

  // 3. Mid-lesson, change the plan, and watch it change.
  await readPlan(main, "plan-before.json");
  await say(main, "s3: change the plan", CHANGE);
  await step(main, "s3: the reply begins", { op: "wait_reply", timeout_ms: TURN_MS });
  await shoot(main, "s3-before");
  await step(main, "s3: the reply ends", { op: "wait_idle", timeout_ms: TURN_MS });
  await shoot(main, "s3-after");
  await readPlan(main, "plan-after.json");

  // 4. Something off the plan, then back.
  await say(main, "s4: off the plan", OFF_PLAN);
  await step(main, "s4: answered", { op: "wait_idle", timeout_ms: TURN_MS });
  await shoot(main, "s4-offplan");
  await say(main, "s4: back", BACK);
  await step(main, "s4: the task again", { op: "wait_idle", timeout_ms: TURN_MS });
  await shoot(main, "s4-return");
  await keepTranscript(main, "Scenarios 2 to 4");
  await quit(main);

  // 5. A week away: every recorded time moves seven days back.
  execFileSync("sh", [join(APP, "scripts/scratch-course.sh"), scratch, "--days-ago", "7"], { stdio: "ignore" });
  const week = await launch("week");
  keepStartup(week);
  await shoot(week, "s5-week");
  await step(week, "s5: start", { op: "click", target: "start", by: "drive" });
  await step(week, "s5: back in a lesson", { op: "wait_idle", timeout_ms: TURN_MS, quiet_ms: 2500 });
  await shoot(week, "s5-week-next");
  await keepTranscript(week, "Scenario 5");
  await quit(week);
} catch (error) {
  problem(`stopped: ${error?.stack ?? error}`);
  for (const window of Object.values(launches)) if (window.child.exitCode === null) window.child.kill("SIGKILL");
}

// --- What was found ------------------------------------------------------------

writeFileSync(join(out, "perf.json"), JSON.stringify(perf, null, 2));
writeFileSync(join(out, "attempts.json"), JSON.stringify({ before: attemptsBefore, after: attempts() }, null, 2));

const lines = [`# Scenario transcript`, ``, `${live ? "The real Claude Code" : "The stand-in model"}, ${run.started}.`, ``];
for (const { heading, entries } of transcripts) {
  lines.push(`## ${heading}`, ``);
  for (const entry of entries) {
    if (entry.kind === "reply") {
      lines.push(`**Tutor**${entry.ok ? "" : " (ended badly)"}${entry.interrupted ? " (interrupted)" : ""}:`, ``, entry.text || "(nothing)", ``);
      if (entry.error) lines.push(`> error: ${entry.error}`, ``);
    } else if (entry.kind === "marked") {
      lines.push(`*The record marked it: ${entry.mark.mark}, help ${entry.mark.help}${entry.mark.cold ? ", cold" : ""}, "${entry.mark.label}".*`, ``);
    } else if (entry.kind === "plan_changed") {
      lines.push(`*The plan changed: ${entry.what.join(", ")}.*`, ``);
    } else if (entry.kind === "notice") {
      lines.push(`*Notice: ${entry.text}*`, ``);
    } else {
      const how = entry.kind === "send" ? `intent ${entry.intent ?? "none (the tutor decides)"}` : entry.kind === "new_lesson" ? "a new lesson" : "words the window wrote";
      lines.push(`**Learner** (${how}):`, ``, entry.text, ``);
    }
  }
}
writeFileSync(join(out, "transcript.md"), lines.join("\n"));
run.finished = new Date().toISOString();
save();
console.log(run.problems.length === 0 ? `scenarios: done, in ${out}` : `scenarios: ${run.problems.length} problem(s); see ${join(out, "run.json")}`);
process.exit(run.problems.length === 0 ? 0 : 1);
