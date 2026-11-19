import { spawn, execFileSync } from "node:child_process";
import { copyFileSync, constants, mkdirSync, mkdtempSync, openSync, rmSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { drive } from "./drive.mjs";

export const APP = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

export function ownCopy(binary, { keep = false } = {}) {
  const name = `magnesium-window-run-${process.pid}-${Date.now().toString(36)}`;
  const copy = join(dirname(binary), name);
  try {
    copyFileSync(binary, copy, constants.COPYFILE_FICLONE);
  } catch {
    return binary;
  }
  if (keep) return copy;
  process.on("exit", () => {
    for (const path of [copy, join(homedir(), "Library", "WebKit", name), join(homedir(), "Library", "Caches", name)]) {
      rmSync(path, { recursive: true, force: true });
    }
  });
  for (const signal of ["SIGINT", "SIGTERM"]) process.on(signal, () => process.exit(130));
  return copy;
}

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
  const courseEnv = runtimeProgram ? { ...process.env, MAGNESIUM_TEST_BINARY: runtimeProgram } : process.env;
  execFileSync("sh", [join(APP, "scripts/scratch-course.sh"), scratch], { encoding: "utf8", env: courseEnv });
  const runtime = join(scratch, "magnesium");
  const agent = join(scratch, "claude");
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

  async function lineLook(w, look) {
    const bar = await w.op({ op: "click", target: "settings", by: "drive" });
    if (!bar.ok) await step(w, `settings for the ${look}`, { op: "click", target: "Settings" });
    await step(w, `your line: ${look}`, { op: "click", target: look });
    await step(w, `close Settings after the ${look}`, { op: "key", key: "w", meta: true });
    await settle(w, 300);
  }

  return { run, problem, save, scratch, runtime, agent, pluginAgent, photo, launch, step, shoot, quit, settle, lineLook };
}
