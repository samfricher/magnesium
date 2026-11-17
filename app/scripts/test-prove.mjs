// Which checks scripts/prove.sh picks for a change. A part left out that the change could affect is a pass that
// means nothing, so each kind of file is checked against the parts it must bring.
//
//   node scripts/test-prove.mjs

import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const prove = join(dirname(fileURLToPath(import.meta.url)), "prove.sh");
const parts = (files, ...args) =>
  execFileSync("sh", [prove, "--list", ...args], { encoding: "utf8", env: { ...process.env, PROVE_CHANGED: files.join("\n") } })
    .trim()
    .split("\n")
    .join(" ");

const WINDOW = "words pages build window";
const PAGES = `${WINDOW} unit`;
const ALL = `${WINDOW} rust runtime`;
const cases = [
  ["nothing differs", [], "words"],
  ["a page's styles", ["app/src/styles/surface.css"], PAGES],
  ["the page whose words the engine must know", ["app/src/surface/Session.tsx"], PAGES],
  ["a harness script", ["app/scripts/pages.mjs"], PAGES],
  ["the harness's notes", ["app/scripts/harness.md"], PAGES],
  ["the window's core", ["app/crates/core/src/claude.rs"], `${WINDOW} rust`],
  ["the tutor's prompt in the window's core", ["app/crates/core/prompts/tutor.md"], `${WINDOW} rust`],
  ["the window's shell", ["app/src-tauri/src/main.rs"], `${WINDOW} rust`],
  ["the window's packages", ["app/Cargo.lock"], `${WINDOW} rust`],
  ["the runtime", ["src/learner/practice.rs"], ALL],
  ["a runtime test", ["tests/it/sittings.rs"], ALL],
  ["the tutor's instructions, which the runtime carries", ["src/workspace/learning/workflows/papers.md"], ALL],
  ["the plugin's skill, which the runtime carries", ["claude-plugin/skills/learn/SKILL.md"], ALL],
  ["the user guide, which the runtime carries", ["docs/user-guide.md"], ALL],
  ["the runtime's packages", ["Cargo.lock"], ALL],
  ["a document at the root", ["CONTRIBUTING.md"], "words"],
  ["the checks' own tools", ["ci/check.py"], "words"],
  ["a page and a root document", ["README.md", "app/src/App.tsx"], PAGES],
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
if (all !== ALL) {
  failed += 1;
  console.error(`prove --all picks "${all}"`);
}

// The rule rests on what the sources read from outside their own folders. Hold it to them, so a new such read
// that the rule would not cover fails here and not as a check quietly left out.
const root = join(dirname(prove), "..", "..");
const rust = (folder) =>
  existsSync(join(root, folder))
    ? readdirSync(join(root, folder), { recursive: true })
        .filter((name) => name.endsWith(".rs"))
        .map((name) => join(folder, name))
    : [];
// 1. The runtime: every file its sources carry (include_str!) or its tests read from the repository must be one
//    whose change brings the runtime's checks.
for (const file of [...rust("src"), ...rust("tests"), "build.rs"]) {
  const text = readFileSync(join(root, file), "utf8");
  const read = [
    ...[...text.matchAll(/include_(?:str|bytes)!\(\s*"((?:\.\.\/)+[^"]+)"/g)].map((m) => normalize(join(dirname(file), m[1]))),
    ...[...text.matchAll(/repo_root\(\)\s*\.join\("([^"]+)"/g)].map((m) => m[1]),
  ];
  for (const path of new Set(read)) {
    if (path.startsWith("target") || path.startsWith("..")) continue;
    if (!parts([path]).endsWith("runtime")) {
      failed += 1;
      console.error(`${file} reads ${path}, and prove would not run the runtime's checks when that file changes`);
    }
  }
}
// 2. The window: only the core's unit tests (crates/core/src) may read the pages, since a page-only change runs
//    those and not the core's other tests or the shell's.
for (const file of [...rust("app/crates/core/tests"), ...rust("app/src-tauri")].filter((name) => !name.includes("/target/"))) {
  const text = readFileSync(join(root, file), "utf8");
  const pages = file.startsWith("app/src-tauri") ? /"\.\.\/(?:\.\.\/)?src[\/"]/ : /"\.\.\/\.\.\/src[\/"]/;
  if (pages.test(text) || /\.tsx\b/.test(text)) {
    failed += 1;
    console.error(`${file} reads the pages, and a page-only change runs only the core's unit tests: move the test there, or widen prove.sh`);
  }
}
if (failed > 0) process.exit(1);
console.log(`prove picks the right parts for ${cases.length + 1} kinds of change, and covers what the sources read`);
