// Look at one state of the real window, in one command: build what changed,
// open the window on the made-up course, go to a place, do a few things there
// and photograph it. Lessons run through the runtime built from this checkout,
// never the installed one. Starts no model.
//
//   node scripts/look.mjs                                  Home
//   node scripts/look.mjs --at task                        a lesson with its first task open
//   node scripts/look.mjs --at task click:stuck            ... with Stuck's row open
//   node scripts/look.mjs --at task --base origin/main     ... and the same in the window built from main:
//                                                          <name>-before-*.png beside <name>-after-*.png
//
// It prints the photographs' paths, one a line. They go to shots/look/ unless
// --out names a folder; git ignores shots/.
//
//   --at <place>    home (the default), workspace, session (a new session, not begun), task (its first task
//                   open), marked (an answer marked, the next task open) or settings
//   --line <look>   the line a learner writes on: bare (the default), soft, ruled, or all for one photograph of
//                   each. A new window draws the soft field; the look is chosen in Settings before going anywhere.
//   --size <WxH>    the window's size, such as 720x560
//   --big           the largest text
//   --scheme <s>    light, dark or both (the default)
//   --name <stem>   the photographs' name (look)
//   --out <dir>     where they go
//   --base <ref>    photograph the same steps in the window as of that commit too, on the runtime as of that
//                   commit (scripts/window-build.sh --ref)
//   --bin <path>    a window already built, in place of building this checkout's
//   --keep          leave the window open afterwards and print its drive folder, to send it more ops with
//                   scripts/drive.mjs and photograph it with scripts/capture.sh <pid> <png>. Its copy of the
//                   program (target/debug/magnesium-window-run-*) and that copy's settings are then left too.
//
// Each further argument is a step, done in order once the place is reached:
//
//   click:<target>        a data-drive name, else the words on it (scripts/harness.md lists both)
//   focus:<target>
//   type:<text>
//   key:<Key>             with modifiers before it: key:meta+ArrowUp, key:alt+Enter
//   wait:<ms>
//   idle                  wait for the tutor's reply to end
//   shot:<stem>           photograph now, under this name; with no shot step, one photograph is taken at the end
//   text | session | ...  any op of scripts/harness.md by its name, its answer printed
//   '{"op": ...}'         any op as JSON, its answer printed
//
// A step that fails is said and the rest still run, so "before" can be photographed in a window that lacks what
// the change adds. The exit code is 1 when any step failed.

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

import { APP, openRun } from "./window.mjs";

const PLACES = ["home", "workspace", "session", "task", "marked", "settings"];
const LOOKS = { bare: "Bare line", soft: "Soft field", ruled: "Ruled line" };
const ANSWER = "The standard error is s/√n = 10/√25 = 2 minutes.";
const TURN_MS = 60_000;

function usage(problem) {
  if (problem) console.error(`look: ${problem}`);
  console.error("usage: node scripts/look.mjs [--at <place>] [--line bare|soft|ruled|all] [--size WxH] [--big] [--scheme light|dark|both]");
  console.error("                             [--name <stem>] [--out <dir>] [--base <ref>] [--bin <path>] [--keep] [step ...]");
  console.error("       the top of scripts/look.mjs says what each does");
  process.exit(64);
}

const options = { at: "home", line: "bare", scheme: "both", name: "look", out: join(APP, "shots", "look"), steps: [] };
const args = process.argv.slice(2);
for (let n = 0; n < args.length; n++) {
  const arg = args[n];
  const value = () => args[++n] ?? usage(`${arg} needs a value`);
  if (arg === "--at") options.at = value();
  else if (arg === "--line") options.line = value();
  else if (arg === "--size") options.size = value();
  else if (arg === "--big") options.big = true;
  else if (arg === "--scheme") options.scheme = value();
  else if (arg === "--name") options.name = value();
  else if (arg === "--out") options.out = resolve(value());
  else if (arg === "--base") options.base = value();
  else if (arg === "--bin") options.bin = resolve(value());
  else if (arg === "--keep") options.keep = true;
  else if (arg === "--help" || arg === "-h") usage();
  else if (arg.startsWith("--")) usage(`unknown option ${arg}`);
  else options.steps.push(arg);
}
if (!PLACES.includes(options.at)) usage(`--at is one of ${PLACES.join(", ")}`);
if (options.line !== "all" && !LOOKS[options.line]) usage("--line is bare, soft, ruled or all");
const schemes = { light: ["light"], dark: ["dark"], both: ["dark", "light"] }[options.scheme] ?? usage("--scheme is light, dark or both");
const size = options.size ? /^(\d+)x(\d+)$/.exec(options.size) ?? usage("--size is written 720x560") : null;
if (options.keep && (options.base || options.line === "all")) usage("--keep leaves one window open, so it goes with neither --base nor --line all");

/** A step as the op it sends, or {shot: stem}. */
function parse(text) {
  if (text.startsWith("{")) return { op: JSON.parse(text), say: true };
  const at = text.indexOf(":");
  const [word, rest] = at < 0 ? [text, ""] : [text.slice(0, at), text.slice(at + 1)];
  if (word === "shot") return { shot: rest || options.name };
  if (word === "click" || word === "focus") return { op: { op: word, target: rest } };
  if (word === "type") return { op: { op: "type", text: rest } };
  if (word === "wait") return { op: { op: "wait", ms: Number(rest) || 500 } };
  if (word === "idle") return { op: { op: "wait_idle", timeout_ms: TURN_MS, quiet_ms: 2500 } };
  if (word === "key") {
    const keys = rest.split("+");
    const op = { op: "key", key: keys.pop() };
    for (const held of keys) op[held === "cmd" ? "meta" : held] = true;
    return { op };
  }
  if (at < 0) return { op: { op: word }, say: true };
  usage(`no step is written ${text}`);
}
const steps = options.steps.map(parse);

/** The window's program, or with `runtime` the magnesium program lessons run through: this checkout's, or as of `ref`. */
function built(ref, runtime = false) {
  const args = [join(APP, "scripts/window-build.sh"), ...(runtime ? ["--runtime"] : []), ...(ref ? ["--ref", ref] : [])];
  try {
    return execFileSync("sh", args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim().split("\n").pop();
  } catch (error) {
    console.error(String(error.stderr ?? error).trim().split("\n").slice(-25).join("\n"));
    process.exit(1);
  }
}

const photographs = [];
const failed = [];

/** One window: the look chosen, the place reached, the steps done and photographed. `tag` goes into each name. */
async function look({ binary, runtimeProgram }, tag, lineName) {
  const stem = (name) => [name, tag, options.line === "all" ? lineName : null].filter(Boolean).join("-");
  const { run, scratch, launch, step, shoot, quit, settle, lineLook } = openRun({ binary, out: options.out, stem, keep: options.keep, runtimeProgram });
  const w = await launch("look", join(scratch, "window"));
  const photograph = async (name) => photographs.push(...(await shoot(w, name, `${options.at}${tag ? `, ${tag}` : ""}`, schemes)));

  if (size) await step(w, options.size, { op: "frame", width: Number(size[1]), height: Number(size[2]) });
  if (options.big) for (let n = 0; n < 11; n++) await w.op({ op: "menu_click", path: ["View", "Bigger Text"] });
  // A new window draws the soft field, so only another look is chosen; and only where a line is drawn.
  if (lineName !== "soft" && !["home", "workspace"].includes(options.at)) {
    await lineLook(w, LOOKS[lineName]);
    await step(w, "back to the first tab", { op: "click", target: "tab-1", by: "drive" });
  }
  const to = async (label, op, ms = 600) => {
    await step(w, label, op);
    await settle(w, ms);
  };
  if (options.at === "settings") {
    const bar = await w.op({ op: "click", target: "settings", by: "drive" });
    if (!bar.ok) await step(w, "Settings", { op: "click", target: "Settings" });
    await settle(w, 600);
  } else if (options.at !== "home") {
    await to("open the workspace", { op: "click", target: "workspace-Intro Statistics", by: "drive" }, 800);
    if (options.at !== "workspace") {
      await to("a new session", { op: "key", key: "n", meta: true }, 800);
      if (options.at !== "session") {
        await step(w, "focus the line", { op: "focus", target: "composer", by: "drive" });
        await step(w, "start", { op: "key", key: "Enter" });
        await step(w, "the first task", { op: "wait_idle", timeout_ms: TURN_MS, quiet_ms: 2500 });
        if (options.at === "marked") {
          await step(w, "focus the answer", { op: "focus", target: "composer", by: "drive" });
          await step(w, "type the answer", { op: "type", text: ANSWER });
          await step(w, "send", { op: "key", key: "Enter" });
          await step(w, "marked", { op: "wait_idle", timeout_ms: TURN_MS });
        }
      }
    }
  }

  let shot = false;
  for (const [n, each] of steps.entries()) {
    if (each.shot) {
      await photograph(each.shot);
      shot = true;
      continue;
    }
    const answer = await step(w, options.steps[n], each.op);
    if (each.say) console.log(JSON.stringify(answer, null, 2));
    if (["click", "key", "type", "focus"].includes(each.op.op)) await settle(w, 300);
  }
  if (!shot) await photograph(options.name);
  failed.push(...run.problems.map((what) => (tag ? `${tag}: ${what}` : what)));

  if (options.keep) {
    writeFileSync(join(options.out, "kept.json"), JSON.stringify({ drive: w.driveDir, pid: w.child.pid }, null, 2));
    console.log(`window left open: drive ${w.driveDir}, pid ${w.child.pid}`);
    console.log(`  node scripts/drive.mjs ${w.driveDir} '{"op":"text"}'      sh scripts/capture.sh ${w.child.pid} <png>      ... '{"op":"quit"}' when done`);
    w.child.unref();
    return;
  }
  await quit(w);
}

mkdirSync(options.out, { recursive: true });
// Each window with the runtime of its own commit: a lesson's replies and the record come from it, so a change to
// the runtime shows only if "before" runs on the runtime as it was.
console.error("look: building what changed…");
const after = { binary: options.bin ?? built(), runtimeProgram: built(null, true) };
if (!existsSync(after.binary)) usage(`no window program at ${after.binary}`);
console.error(`look: the window ${after.binary}\n      on the runtime ${after.runtimeProgram}`);
let before = null;
if (options.base) {
  console.error(`look: the window and the runtime as of ${options.base}…`);
  before = { binary: built(options.base), runtimeProgram: built(options.base, true) };
  console.error(`look: before, the window ${before.binary}\n      on the runtime ${before.runtimeProgram}`);
}
for (const lineName of options.line === "all" ? Object.keys(LOOKS) : [options.line]) {
  if (before) await look(before, "before", lineName);
  await look(after, before ? "after" : "", lineName);
}
for (const file of photographs) console.log(file);
if (failed.length > 0) {
  console.error(`look: ${failed.length} step(s) failed:`);
  for (const what of failed) console.error(`  ${what}`);
}
// With --keep the window outlives this process, which would otherwise wait on it.
process.exit(failed.length > 0 ? 1 : 0);
