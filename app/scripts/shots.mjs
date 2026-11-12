// Photograph the workspace with the demo's made-up data, in both schemes.
// Uses the Chrome already on this computer, with no window.
//
//   npm run shots            every state, into shots/
//   npm run shots -- s3      only the states whose name contains "s3"
//
// The evidence contract's states come first, at 1040 by 720 in the dark and
// the light scheme; `narrow-lesson` is a lesson at 720 by 560, in both.
// The rest show the other documents and setup states.

import { mkdir } from "node:fs/promises";
import { chromium } from "playwright-core";
import { createServer } from "vite";

const only = process.argv[2];

// [file stem, address, milliseconds to settle, schemes, size]
const BOTH = ["dark", "light"];
const DARK = ["dark"];
const WIDE = { width: 1040, height: 720 };
const shots = [
  ["s1-cold", "s1-cold", 900, BOTH],
  ["s1-cold-first", "s1-cold-first", 900, BOTH],
  ["s2-start", "s2-start", 1600, BOTH],
  ["s2-answer", "s2-answer", 2600, BOTH],
  ["s3-before", "s3-before", 3000, BOTH],
  ["s3-after", "s3-after", 3900, BOTH],
  ["s4-offplan", "s4-offplan", 4800, BOTH],
  ["s4-return", "s4-return", 5600, BOTH],
  ["s5-week", "s5-week", 900, BOTH],
  ["s5-week-next", "s5-week-next", 1600, BOTH],
  ["narrow-lesson", "s2-answer", 2600, BOTH, { width: 720, height: 560 }],
  ["settings", "settings", 900, BOTH],
  ["open", "open", 900, BOTH],
  ["commands", "commands", 900, DARK],
  ["folders", "folders", 900, DARK],
  ["sidebar", "sidebar", 900, DARK],
  ["material", "material", 1200, BOTH],
  ["paper", "paper", 1500, DARK],
  ["paper-running", "paper-running", 1500, DARK],
  ["record", "record", 1100, BOTH],
  ["earlier-lesson", "earlier", 1100, DARK],
  ["plan", "plan", 900, DARK],
  ["plan-edit", "plan-edit", 1100, DARK],
  ["new-lesson", "new-lesson", 900, BOTH],
  ["hint", "hint", 2400, DARK],
  ["setup-signed-out", "setup-signed-out", 900, DARK],
  ["setup-no-claude", "setup-no-claude", 900, DARK],
  ["setup-replace", "setup-replace", 900, DARK],
  ["upgrade", "upgrade", 900, DARK],
  ["drop", "drop", 1100, DARK],
].filter(([name]) => !only || name.includes(only));

const server = await createServer({ server: { port: 1431, strictPort: false }, logLevel: "error" });
await server.listen();
const base = server.resolvedUrls.local[0];
await mkdir("shots", { recursive: true });

const browser = await chromium.launch({ channel: "chrome", headless: true });
const problems = [];
let taken = 0;
for (const [name, address, settle, schemes, size] of shots) {
  for (const scheme of schemes) {
    // Each photograph is a fresh page with nothing kept from the one before.
    const context = await browser.newContext({ viewport: size ?? WIDE, deviceScaleFactor: 2, colorScheme: scheme });
    const page = await context.newPage();
    page.on("pageerror", (error) => problems.push(`${name}: ${error.message}`));
    page.on("console", (message) => {
      if (message.type() === "error") problems.push(`${name}: ${message.text()}`);
    });
    await page.goto(`${base}#${address}`);
    await page.waitForTimeout(settle);
    await page.screenshot({ path: `shots/${name}-${scheme}.png` });
    taken += 1;
    await context.close();
  }
}
await browser.close();
await server.close();

if (problems.length > 0) {
  console.error(`${problems.length} problem(s) on the page:\n` + problems.join("\n"));
  process.exit(1);
}
console.log(`photographed ${taken} pictures of ${shots.length} states into shots/`);
