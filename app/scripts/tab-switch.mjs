// Switch tabs in the real window and say, frame by frame, whether anything
// moved after the first frame, and whether the menu bar was put in place again.
// Builds nothing and starts no model.
//
//   node scripts/tab-switch.mjs <app-binary> <out-dir>
//
// Works in a scratch course (scripts/scratch-course.sh) with a scratch window
// home. Each course's page is opened from Home's list and left for it again,
// twice. Then the Home tab shows a course's page, Settings has a tab beside it, and
// the window switches between them, to Home's list and back, and to a
// session's tab and back; then a key is changed in Settings, which the menu
// must follow; and the course's page, scrolled, must keep its place across a
// switch. Writes switch.json (each switch: what the first frame drew and
// every frame after it that differed; the menu as macOS holds it; how many
// times the menu bar was put in place) and a photo of each page.
//
// Exits 1 when a page moved after a switch's first frame (a line drawn
// somewhere else, or lines come or gone), when the menu bar was put in place
// again, when the menu did not follow the key, or when the page lost its
// place. Words that change in place, as a page read again says "2 minutes
// ago", are reported but are not a move.

import { spawn, execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, openSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { drive } from "./drive.mjs";

const APP = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const [binary, outArg] = process.argv.slice(2);
if (!binary || !outArg || !existsSync(binary)) {
  console.error("usage: node scripts/tab-switch.mjs <app-binary> <out-dir>");
  process.exit(64);
}
const out = resolve(outArg);
mkdirSync(out, { recursive: true });
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

const scratch = mkdtempSync(join(tmpdir(), "mg-switch-"));
execFileSync("sh", [join(APP, "scripts/scratch-course.sh"), scratch], { encoding: "utf8" });
const agent = join(scratch, "claude");
writeFileSync(agent, `#!/bin/sh\nexport STAND_IN_LOG='${join(scratch, "model.log")}'\nexec python3 '${join(APP, "crates/core/tests/stand_in_model.py")}' "$@"\n`, { mode: 0o755 });

const driveDir = join(scratch, "drive");
mkdirSync(driveDir, { recursive: true });
const env = { ...process.env, MAGNESIUM_APP_HOME: join(scratch, "window"), MAGNESIUM_APP_RUNTIME: join(scratch, "magnesium"), MAGNESIUM_APP_DRIVE: driveDir, MAGNESIUM_APP_AGENT: agent };
delete env.MAGNESIUM_APP_SELFTEST;
delete env.MAGNESIUM_APP_FOLDER;
const child = spawn(binary, [], { env, stdio: ["ignore", openSync(join(out, "window.log"), "w"), openSync(join(out, "window.err.log"), "w")] });
const op = (each, options) => drive(driveDir, each, options);

const report = { binary, steps: [], switches: [], problems: [] };
const save = () => writeFileSync(join(out, "switch.json"), JSON.stringify(report, null, 2));
async function step(label, each) {
  const answer = await op(each);
  report.steps.push({ label, answer: each.op === "switch" ? "(in switches)" : answer });
  if (!answer.ok) report.problems.push(`${label}: ${answer.error ?? "not ok"}`);
  save();
  return answer;
}
function shoot(stem) {
  try {
    execFileSync("sh", [join(APP, "scripts/capture.sh"), String(child.pid), join(out, `${stem}.png`)], { stdio: ["ignore", "ignore", "pipe"] });
  } catch (error) {
    report.problems.push(`photo ${stem}: ${String(error.stderr ?? error).trim()}`);
  }
}
async function change(label, target) {
  const answer = await op({ op: "switch", target, ms: 800 });
  if (!answer.ok) report.problems.push(`${label}: ${answer.error ?? "not ok"}`);
  report.switches.push({ label, target, ...answer });
  save();
  await sleep(400);
  return answer;
}

try {
  const start = Date.now();
  for (;;) {
    const answer = await op({ op: "ready" }, { timeoutMs: 5_000 });
    if (answer.ok && answer.ready) break;
    if (Date.now() - start > 60_000) throw new Error("the window never called ready()");
    await sleep(250);
  }
  await step("settle", { op: "wait_idle", timeout_ms: 60_000 });
  await step("light", { op: "scheme", scheme: "light" });
  // Each course's page opened from Home's list, and Home's list again from it: the first time a page is opened in
  // the window, and again.
  for (let n = 1; n <= 2; n++)
    for (const course of ["Intro Statistics", "Linear Algebra", "Organic Chemistry"]) {
      await change(`${n}: open ${course} from Home's list`, `workspace-${course}`);
      await change(`${n}: back to Home's list from ${course}`, "home");
    }
  await step("open the course's page", { op: "click", target: "workspace-Intro Statistics" });
  await sleep(1500);
  shoot("course-page");
  await step("open Settings", { op: "click", target: "settings", by: "drive" });
  await sleep(1000);
  shoot("settings");
  const menuBefore = await step("menu before the switches", { op: "menu" });

  for (let n = 1; n <= 4; n++) {
    await change(`${n}: to the course's page`, "home");
    await change(`${n}: to Settings`, "tab-2");
  }
  // Home's own list, then Settings and back to it.
  await change("to the Home tab", "home");
  await step("back to Home's list", { op: "click", target: "home", by: "drive" });
  await sleep(1500);
  for (let n = 1; n <= 2; n++) {
    await change(`${n}: to Settings, from Home's list`, "tab-2");
    await change(`${n}: to Home's list`, "home");
  }
  // A session beside them: it goes before Settings, so it is tab-2 and Settings tab-3.
  await step("open a session", { op: "click", target: "new-session", by: "drive" });
  await step("let it start", { op: "wait_idle", timeout_ms: 60_000 });
  await sleep(1000);
  for (let n = 1; n <= 2; n++) {
    await change(`${n}: to the Home tab, from a session`, "home");
    await change(`${n}: to the session`, "tab-2");
  }
  await change("to Settings, from a session", "tab-3");
  await change("to the session, from Settings", "tab-2");
  const menuAfter = await step("menu after the switches", { op: "menu" });
  const windowMenu = (menuAfter.menu ?? []).find((menu) => menu.title === "Window");
  report.window_menu = (windowMenu?.items ?? []).map((item) => (item.separator ? "—" : `${item.title}${item.accelerator ? ` (${item.accelerator})` : ""}${item.enabled === false ? " [off]" : ""}`));
  // Window as macOS holds it, with its own window list: the tabs added in place must come before it.
  report.window_menu_native = (menuAfter.native ?? []).find((menu) => menu.title === "Window")?.items ?? null;
  report.session_menu = (menuAfter.menu ?? []).find((menu) => menu.title === "Session")?.items?.map((item) => (item.separator ? "—" : `${item.title}${item.enabled === false ? " [off]" : ""}`));
  // A learner's own keys, changed in Settings, reach the menu in place too: taken off (the item is made again
  // without one), put back, and changed. Read from the bar as macOS holds it.
  const attachItem = async (label) => {
    const answer = await step(label, { op: "menu" });
    const file = (answer.native ?? []).find((menu) => menu.title === "File");
    return file?.items?.find((item) => String(item).startsWith("Attach")) ?? null;
  };
  await change("to Settings, for its keys", "tab-3");
  const canRead = (await op({ op: "menu" })).native !== undefined;
  report.keys = canRead ? { before: await attachItem("menu, keys as they were") } : { skipped: "this build cannot read the menu as macOS holds it" };
  if (canRead) {
  await step("choose Attach's key", { op: "click", target: "⌘U" });
  await step("take the key off", { op: "key", key: "Backspace" });
  await sleep(500);
  report.keys.off = await attachItem("menu, Attach with no key");
  await step("put it back", { op: "click", target: "back to ⌘U" });
  await sleep(500);
  report.keys.back = await attachItem("menu, Attach's key back");
  await step("choose Attach's key again", { op: "click", target: "⌘U" });
  await step("give it another", { op: "key", key: "y", meta: true, shift: true });
  await sleep(500);
  report.keys.other = await attachItem("menu, Attach on another key");
  await step("and back again", { op: "click", target: "back to ⌘U" });
  await sleep(500);
  const keysEnd = await step("menu, keys at the end", { op: "menu" });
  report.keys.end = (keysEnd.native ?? []).find((menu) => menu.title === "File")?.items?.find((item) => String(item).startsWith("Attach")) ?? null;
  if (report.keys.off !== "Attach a Photo or PDF…" || report.keys.back !== report.keys.before || report.keys.other === report.keys.before || report.keys.end !== report.keys.before)
    report.problems.push(`Attach's key did not follow Settings on the menu: ${JSON.stringify(report.keys)}`);
  }
  const menuEnd = await step("menu at the end", { op: "menu" });
  report.menu_replaced = { before: menuBefore.replaced, after: menuEnd.replaced };

  // A page keeps its place: the course's page, scrolled down in a short window, is where it was after a switch.
  await change("to the Home tab, for its place", "home");
  await step("the course's page again", { op: "click", target: "workspace-Intro Statistics" });
  await sleep(1500);
  await step("a short window", { op: "frame", height: 480 });
  await sleep(500);
  const left = await step("scroll the page down", { op: "scroll", by: 260 });
  await change("away to Settings", "tab-3");
  await change("back to the course's page", "home");
  const found = await step("where the page is", { op: "scroll", by: 0 });
  report.place = { left_at_from_foot: left.from_foot, found_at_from_foot: found.from_foot, kept: left.from_foot === found.from_foot };
  if (!report.place.kept) report.problems.push(`the course's page lost its place: left ${left.from_foot} px from its foot, found ${found.from_foot}`);
  if (menuEnd.replaced !== menuBefore.replaced) report.problems.push(`the menu bar was put in place ${menuEnd.replaced - menuBefore.replaced} more times during ${report.switches.length} switches and the key changes`);
  for (const each of report.switches) {
    if (each.answered_at === null) report.problems.push(`${each.label}: the window never answered the click`);
    const moved = (each.changes_after_first ?? []).filter((frame) => frame.moved);
    if (moved.length > 0) report.problems.push(`${each.label}: the page moved in ${moved.length} frames after the first`);
  }
} catch (error) {
  report.problems.push(String(error));
} finally {
  save();
  await op({ op: "quit" }, { timeoutMs: 10_000 });
  await Promise.race([new Promise((done) => child.on("exit", done)), sleep(10_000)]);
  if (child.exitCode === null) child.kill();
}

const summary = report.switches.map((each) => {
  const later = each.changes_after_first ?? [];
  const moved = later.filter((frame) => frame.moved).length;
  const answered = each.answered_at === undefined ? "" : each.answered_at === null ? ", never answered" : `, answered at ${each.answered_at} ms`;
  return `${each.label}${answered}: ${moved} frames moved, ${later.length - moved} changed words only`;
});
console.log(summary.join("\n"));
console.log(`menu bar put in place: ${JSON.stringify(report.menu_replaced ?? null)}`);
console.log(`the scrolled page's place: ${JSON.stringify(report.place ?? null)}`);
console.log(report.problems.length === 0 ? "nothing moved after a switch's first frame" : `${report.problems.length} problems; see switch.json`);
process.exit(report.problems.length === 0 ? 0 : 1);
