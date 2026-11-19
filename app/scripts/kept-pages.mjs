import { spawn, execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, openSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { drive } from "./drive.mjs";

const APP = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const [binary, outArg] = process.argv.slice(2);
if (!binary || !outArg || !existsSync(binary)) {
  console.error("usage: node scripts/kept-pages.mjs <app-binary> <out-dir>");
  process.exit(64);
}
const out = resolve(outArg);
mkdirSync(out, { recursive: true });
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));
const TURN_MS = 60_000;
const ANSWER = "(9·4 + 9·9)/18 = 6.5";

const scratch = mkdtempSync(join(tmpdir(), "mg-kept-"));
execFileSync("sh", [join(APP, "scripts/scratch-course.sh"), scratch], { encoding: "utf8" });
execFileSync("python3", [join(APP, "scripts/scratch-record.py"), scratch], { encoding: "utf8" });
const course = join(scratch, "Intro-Statistics");
const sittings = join(course, ".magnesium", "sittings.sqlite");
const FOUR_HOURS_AGO = "import sqlite3,sys,time; c=sqlite3.connect(sys.argv[1]); c.execute('UPDATE meta SET last_learner_ms=?', (int((time.time()-4*3600)*1000),)); c.commit()";
execFileSync("python3", ["-I", "-c", FOUR_HOURS_AGO, sittings]);
const hold = [
  "import sqlite3, sys, time",
  "end = time.time() + 40",
  "while time.time() < end:",
  "    try:",
  "        c = sqlite3.connect(sys.argv[1]); c.execute('UPDATE meta SET last_learner_ms=?', (int((time.time() - 4 * 3600) * 1000),)); c.commit(); c.close()",
  "    except sqlite3.Error:",
  "        pass",
  "    time.sleep(0.1)",
].join("\n");
const listed = JSON.parse(readFileSync(join(scratch, "window", "state.json"), "utf8"));
const home = join(scratch, "window-kept");
mkdirSync(home, { recursive: true });
const stateFile = join(home, "state.json");
writeFileSync(stateFile, JSON.stringify({ ...listed, folders: {} }, null, 2));
const agent = join(scratch, "claude");
writeFileSync(agent, `#!/bin/sh\nexport STAND_IN_LOG='${join(scratch, "model.log")}'\nexport STAND_IN_SLOW_CHECKS=1\nexec python3 '${join(APP, "crates/core/tests/stand_in_model.py")}' "$@"\n`, { mode: 0o755 });

const driveDir = join(scratch, "drive");
mkdirSync(driveDir, { recursive: true });
const env = { ...process.env, MAGNESIUM_APP_HOME: home, MAGNESIUM_APP_RUNTIME: join(scratch, "magnesium"), MAGNESIUM_APP_DRIVE: driveDir, MAGNESIUM_APP_AGENT: agent };
delete env.MAGNESIUM_APP_SELFTEST;
delete env.MAGNESIUM_APP_FOLDER;
const child = spawn(binary, [], { env, stdio: ["ignore", openSync(join(out, "window.log"), "w"), openSync(join(out, "window.err.log"), "w")] });
const op = (each) => drive(driveDir, each);

const report = { binary, steps: [], problems: [] };
const save = () => writeFileSync(join(out, "kept.json"), JSON.stringify(report, null, 2));
async function step(label, each) {
  const answer = await op(each);
  report.steps.push({ label, op: each, answer: each.op === "text" ? "(kept apart)" : answer });
  if (!answer.ok) report.problems.push(`${label}: ${answer.error ?? "not ok"}`);
  save();
  return answer;
}
const opened = () => {
  const state = JSON.parse(readFileSync(stateFile, "utf8"));
  const of = (name) => Object.entries(state.folders ?? {}).find(([key]) => key.endsWith(`/${name}`))?.[1]?.opened_ms ?? null;
  return { linear: of("Linear-Algebra"), intro: of("Intro-Statistics"), recent: (state.recent ?? []).map((key) => key.split("/").pop()) };
};

try {
  const start = Date.now();
  for (;;) {
    const answer = await drive(driveDir, { op: "ready" }, { timeoutMs: 5_000 });
    if (answer.ok && answer.ready) break;
    if (Date.now() - start > 60_000) throw new Error("the window never called ready()");
    await sleep(250);
  }
  await step("settle", { op: "wait_idle", timeout_ms: 60_000 });
  const shown = await step("Home's words", { op: "text" });
  report.question_shown = String(shown.text ?? "").includes("pooled variance");
  if (!report.question_shown) report.problems.push("Home does not show the check's question; the answer steps below test nothing");

  await step("the keyboard on Linear Algebra's row", { op: "focus", target: "workspace-Linear Algebra", by: "drive" });
  await step("open Settings", { op: "click", target: "settings", by: "drive" });
  await sleep(800);
  const back = await step("back to Home, and 1.5 s on", { op: "switch", target: "home", by: "drive", ms: 1500 });
  report.keyboard = back.focused;
  if (!String(back.focused ?? "").includes("workspace-Linear Algebra")) report.problems.push(`the keyboard left Linear Algebra's row after Home came back: ${back.focused}`);

  if (report.question_shown) {
    await step("write on Home's answer line", { op: "focus", target: "home-answer", by: "drive" });
    await step("the answer", { op: "type", text: ANSWER });
    const holding = spawn("python3", ["-I", "-c", hold, sittings], { stdio: "ignore" });
    await step("Return starts the checks", { op: "key", key: "Enter" });
    await step("the tutor takes it", { op: "wait_idle", timeout_ms: TURN_MS, quiet_ms: 2500 });
    await sleep(3000);
    holding.kill();
    const said = await step("what was sent", { op: "transcript" });
    report.answer_sent = (said.transcript ?? []).some((entry) => entry.kind === "send" && String(entry.text).includes("6.5"));
    await step("back to Home", { op: "click", target: "home", by: "drive" });
    await sleep(1500);
    const again = await op({ op: "text" });
    report.question_again = String(again.text ?? "").includes("pooled variance");
    const line = await op({ op: "focus", target: "home-answer", by: "drive" });
    report.home_line = line.ok ? line.focused : "(no answer line: no question on Home now)";
    if (report.answer_sent && String(line.focused ?? "").includes("6.5")) report.problems.push(`Home's answer line still holds the sent answer: ${line.focused}`);
    if (!report.answer_sent || !report.question_again) report.problems.push(`the answer case did not arise (sent: ${report.answer_sent}, question again on Home: ${report.question_again}); it tested nothing`);
  }

  await step("Home's list", { op: "click", target: "home", by: "drive" });
  await sleep(800);
  await step("Linear Algebra's page in Home's tab", { op: "click", target: "workspace-Linear Algebra", by: "drive" });
  await sleep(2000);
  await step("to the Intro Statistics session", { op: "click", target: "tab-2", by: "drive" });
  await sleep(1000);
  report.before_turn = opened();
  await step("focus the line", { op: "focus", target: "composer", by: "drive" });
  await step("a message", { op: "type", text: "go on" });
  await step("send it", { op: "key", key: "Enter" });
  await step("the reply", { op: "wait_idle", timeout_ms: TURN_MS, quiet_ms: 2500 });
  await sleep(3000);
  report.after_turn = opened();
  if (report.before_turn.linear !== report.after_turn.linear)
    report.problems.push(`a turn in Intro Statistics noted a visit to Linear Algebra, kept in Home's tab: opened ${report.before_turn.linear} → ${report.after_turn.linear}`);
  await step("back to Home's tab", { op: "click", target: "home", by: "drive" });
  await sleep(2000);
  report.after_return = opened();
  if (report.after_return.linear === report.after_turn.linear) report.problems.push("coming back to Linear Algebra's page in Home's tab did not note a visit");

  await step("Home's list again", { op: "click", target: "home", by: "drive" });
  await sleep(2000);
  const plain = await op({ op: "text" });
  report.question_before_leaving = String(plain.text ?? "").includes("pooled variance");
  await step("the keyboard on Home's first row", { op: "focus", target: "start", by: "drive" });
  await step("to the session", { op: "click", target: "tab-2", by: "drive" });
  await sleep(800);
  const holdingAgain = spawn("python3", ["-I", "-c", hold, sittings], { stdio: "ignore" });
  await step("focus the line", { op: "focus", target: "composer", by: "drive" });
  await step("another message", { op: "type", text: "and then?" });
  await step("send it", { op: "key", key: "Enter" });
  await step("the reply", { op: "wait_idle", timeout_ms: TURN_MS, quiet_ms: 2500 });
  await sleep(3000);
  holdingAgain.kill();
  const returned = await step("back to Home, and 1.5 s on", { op: "switch", target: "home", by: "drive", ms: 1500 });
  const set = await op({ op: "text" });
  report.question_on_return = String(set.text ?? "").includes("pooled variance");
  report.keyboard_on_new_question = returned.focused;
  if (report.question_before_leaving || !report.question_on_return)
    report.problems.push(`the new-question case did not arise (a question as Home went behind: ${report.question_before_leaving}, on its return: ${report.question_on_return}); it tested nothing`);
  else if (!String(returned.focused ?? "").includes("home-answer")) report.problems.push(`a question set while Home was behind did not take the keyboard: ${returned.focused}`);

  await step("to the session again", { op: "click", target: "tab-2", by: "drive" });
  await step("focus the line", { op: "focus", target: "composer", by: "drive" });
  await step("a message that keeps the sitting", { op: "type", text: "ok" });
  await step("send it", { op: "key", key: "Enter" });
  await step("its reply", { op: "wait_idle", timeout_ms: TURN_MS, quiet_ms: 2500 });
  await sleep(3000);
  await step("focus the line", { op: "focus", target: "composer", by: "drive" });
  await step("a slow message", { op: "type", text: "VERYSLOW: and after that?" });
  const holdingThird = spawn("python3", ["-I", "-c", hold, sittings], { stdio: "ignore" });
  await step("send it", { op: "key", key: "Enter" });
  await step("to Home while it is written", { op: "click", target: "home", by: "drive" });
  await sleep(500);
  const before = await op({ op: "text" });
  report.question_drawn_in_front = String(before.text ?? "").includes("pooled variance");
  await step("the keyboard on Home's first row", { op: "focus", target: "start", by: "drive" });
  await step("the slow reply ends, Home in front", { op: "wait_idle", timeout_ms: TURN_MS, quiet_ms: 2500 });
  await sleep(3000);
  const still = await op({ op: "text" });
  report.question_drawn_after_read = String(still.text ?? "").includes("pooled variance");
  await step("to Settings", { op: "click", target: "tab-3", by: "drive" });
  await sleep(800);
  const fromSettings = await step("back to Home, and 1.5 s on", { op: "switch", target: "home", by: "drive", ms: 1500 });
  holdingThird.kill();
  const drawnNow = await op({ op: "text" });
  report.question_drawn_on_return = String(drawnNow.text ?? "").includes("pooled variance");
  report.keyboard_on_undrawn_question = fromSettings.focused;
  if (report.question_drawn_in_front || report.question_drawn_after_read || !report.question_drawn_on_return)
    report.problems.push(
      `the undrawn-question case did not arise (drawn in front: ${report.question_drawn_in_front}, after Home's read: ${report.question_drawn_after_read}, on return: ${report.question_drawn_on_return}); it tested nothing`,
    );
  else if (!String(fromSettings.focused ?? "").includes("home-answer")) report.problems.push(`a question Home read in front but drew only on return did not take the keyboard: ${fromSettings.focused}`);
} catch (error) {
  report.problems.push(String(error));
} finally {
  save();
  await op({ op: "quit" }, { timeoutMs: 10_000 });
  await Promise.race([new Promise((done) => child.on("exit", done)), sleep(10_000)]);
  if (child.exitCode === null) child.kill();
}

console.log(`keyboard after Home came back: ${report.keyboard}`);
console.log(`answer sent: ${report.answer_sent}; question on Home again: ${report.question_again}; Home's line after: ${report.home_line}`);
console.log(`keyboard on a question set while Home was behind: ${report.keyboard_on_new_question}`);
console.log(`keyboard on a question read in front, drawn on return: ${report.keyboard_on_undrawn_question}`);
console.log(`Linear Algebra opened: before the turn ${report.before_turn?.linear}, after ${report.after_turn?.linear}, after coming back ${report.after_return?.linear}`);
console.log(report.problems.length === 0 ? "the kept pages kept what they should" : `${report.problems.length} problems:\n- ${report.problems.join("\n- ")}`);
process.exit(report.problems.length === 0 ? 0 : 1);
