// Which checks scripts/prove.sh picks for a change. A part left out that the change could affect is a pass that
// means nothing, so each kind of file is checked against the parts it must bring.
//
//   node scripts/test-prove.mjs

import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const prove = join(dirname(fileURLToPath(import.meta.url)), "prove.sh");
const parts = (files, ...args) =>
  execFileSync("sh", [prove, "--list", ...args], { encoding: "utf8", env: { ...process.env, PROVE_CHANGED: files.join("\n") } })
    .trim()
    .split("\n")
    .join(" ");

const WINDOW = "words pages build window";
const cases = [
  ["nothing differs", [], "words"],
  ["a page's styles", ["app/src/styles/surface.css"], WINDOW],
  ["a harness script", ["app/scripts/pages.mjs"], WINDOW],
  ["the harness's notes", ["app/scripts/harness.md"], WINDOW],
  ["the window's core", ["app/crates/core/src/claude.rs"], `${WINDOW} rust`],
  ["the window's shell", ["app/src-tauri/src/main.rs"], `${WINDOW} rust`],
  ["the window's packages", ["app/Cargo.lock"], `${WINDOW} rust`],
  ["the runtime", ["src/learner/practice.rs"], `${WINDOW} rust runtime`],
  ["a runtime test", ["tests/it/sittings.rs"], `${WINDOW} rust runtime`],
  ["the tutor's instructions, which the runtime carries", ["src/workspace/learning/workflows/papers.md"], `${WINDOW} rust runtime`],
  ["the plugin's skill", ["claude-plugin/skills/learn/SKILL.md"], `${WINDOW} rust runtime`],
  ["the runtime's packages", ["Cargo.lock"], `${WINDOW} rust runtime`],
  ["a document at the root", ["CONTRIBUTING.md"], "words"],
  ["the user guide", ["docs/user-guide.md"], "words"],
  ["the checks' own tools", ["ci/check.py"], "words"],
  ["a page and a document", ["docs/user-guide.md", "app/src/App.tsx"], WINDOW],
];
let failed = 0;
for (const [what, files, want] of cases) {
  const got = parts(files);
  if (got !== want) {
    failed += 1;
    console.error(`prove picks "${got}" for ${what} (${files.join(", ") || "no file"}), not "${want}"`);
  }
}
const all = parts([], "--all");
if (all !== `${WINDOW} rust runtime`) {
  failed += 1;
  console.error(`prove --all picks "${all}"`);
}
if (failed > 0) process.exit(1);
console.log(`prove picks the right parts for ${cases.length + 1} kinds of change`);
