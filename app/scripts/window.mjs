// A run of the real window on a made-up course: the course, windows opened on
// it, ops sent to them and photographs taken. Shared by pages.mjs (every page)
// and look.mjs (one state). Builds nothing and starts no model.
//
//   import { openRun } from "./window.mjs";
//   const { launch, step, shoot, quit } = openRun({ binary, out });
//
// It works in a scratch course made by scripts/scratch-course.sh, with a
// scratch window home, so none of your study folders, settings or agent
// sessions are touched. Lessons run on Magnesium with the stand-in model
// (crates/core/tests/stand_in_model.py).

import { spawn, execFileSync } from "node:child_process";
import { copyFileSync, constants, mkdirSync, mkdtempSync, openSync, rmSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { drive } from "./drive.mjs";

export const APP = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

/**
 * A copy of the window's program under a name no other run has, removed with what it kept when this process ends.
 *
 * A window that is not inside an app bundle keeps the page's settings (the line's look, the text size) by its
 * program's name, in ~/Library/WebKit/<name>. Every run of target/debug/magnesium-window, from every checkout,
 * shares that one store: two runs at once flip each other's settings, and a run starts on whatever the last left.
 */
export function ownCopy(binary, { keep = false } = {}) {
  const name = `magnesium-window-run-${process.pid}-${Date.now().toString(36)}`;
  const copy = join(dirname(binary), name);
  try {
    // A clone where the disk can make one, which takes no room.
    copyFileSync(binary, copy, constants.COPYFILE_FICLONE);
  } catch {
    return binary;
  }
  // A window left open afterwards still runs from the copy and still reads its settings.
  if (keep) return copy;
  process.on("exit", () => {
    for (const path of [copy, join(homedir(), "Library", "WebKit", name), join(homedir(), "Library", "Caches", name)]) {
      rmSync(path, { recursive: true, force: true });
    }
  });
  // A run that is stopped still clears up: the default for these signals ends the process without "exit".
  for (const signal of ["SIGINT", "SIGTERM"]) process.on(signal, () => process.exit(130));
  return copy;
}

/**
 * Start a run: make the scratch course and say where its photographs go. `binary` is the window's program and `out`
 * the folder for the photographs, run.json and each window's log. `stem` turns a photograph's or a window's name into
 * its file's name; `keep` is for a run that leaves a window open when it ends; `runtimeProgram` is the magnesium
 * program lessons run through, in place of the one scratch-course.sh would pick.
 */
export function openRun({ binary, out, stem = (name) => name, keep = false, runtimeProgram }) {
  mkdirSync(out, { recursive: true });
  const program = ownCopy(binary, { keep });
  const run = { started: new Date().toISOString(), binary, steps: [], problems: [], shots: [] };
  const problem = (what) => {
    run.problems.push(what);
    console.error(`${basename(process.argv[1] ?? "window", ".mjs")}: ${what}`);
  };
  const save = () => writeFileSync(join(out, "run.json"), JSON.stringify(run, null, 2));

  const scratch = mkdtempSync(join(tmpdir(), "mg-pages-"));
  run.scratch = scratch;
  // scratch-course.sh takes the runtime to use as the window's core tests do.
  const courseEnv = runtimeProgram ? { ...process.env, MAGNESIUM_TEST_BINARY: runtimeProgram } : process.env;
  execFileSync("sh", [join(APP, "scripts/scratch-course.sh"), scratch], { encoding: "utf8", env: courseEnv });
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
    const log = openSync(join(out, `window-${stem(name)}.log`), "w");
    const child = spawn(program, [], { env, stdio: ["ignore", log, log] });
    const exited = new Promise((done) => child.on("exit", (code) => done(code)));
    const window = { name, child, exited, driveDir, op: (op, options) => drive(driveDir, op, options) };
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

  /** Photograph the window in each scheme, as <stem>-dark.png and <stem>-light.png; `schemes` names fewer. */
  async function shoot(window, name, what, schemes = ["dark", "light"]) {
    const files = [];
    for (const scheme of schemes) {
      await window.op({ op: "scheme", scheme });
      await sleep(150);
      const file = join(out, `${stem(name)}-${scheme}.png`);
      try {
        execFileSync("sh", [join(APP, "scripts/capture.sh"), String(window.child.pid), file], { stdio: ["ignore", "ignore", "pipe"] });
        files.push(file);
      } catch (error) {
        problem(`photo ${stem(name)}-${scheme}: ${String(error.stderr ?? error).trim()}`);
      }
    }
    await window.op({ op: "scheme", scheme: "system" });
    run.shots.push({ stem: stem(name), what });
    save();
    return files;
  }

  async function quit(window) {
    await window.op({ op: "quit" }, { timeoutMs: 10_000 });
    const code = await Promise.race([window.exited, sleep(15_000).then(() => "timeout")]);
    if (code === "timeout") window.child.kill("SIGKILL");
  }

  const settle = (window, ms = 500) => window.op({ op: "wait", ms });

  /** The line's look, chosen in Settings as a learner chooses it: from the bar's Settings, or Settings' own tab once it has one. */
  async function lineLook(w, look) {
    const bar = await w.op({ op: "click", target: "settings", by: "drive" });
    if (!bar.ok) await step(w, `settings for the ${look}`, { op: "click", target: "Settings" });
    await step(w, `your line: ${look}`, { op: "click", target: look });
    // Settings closed again, so a session is always the second tab.
    await step(w, `close Settings after the ${look}`, { op: "key", key: "w", meta: true });
    await settle(w, 300);
  }

  return { run, problem, save, scratch, runtime, agent, pluginAgent, photo, launch, step, shoot, quit, settle, lineLook };
}
