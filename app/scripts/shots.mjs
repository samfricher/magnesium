// Photograph the logbook with the demo's made-up data, in both schemes. Uses
// the Chrome already on this computer, with no window. Each address plays a
// short script in the demo (src/demo.ts) and marks the page ready when the
// state is on screen.
//
//   npm run shots            every state, into shots/
//   npm run shots -- s3      only the states whose name contains "s3"

import { mkdir, rm } from "node:fs/promises";
import { chromium } from "playwright-core";
import { createServer } from "vite";

const only = process.argv[2];

// [file stem, address, viewport, schemes]
const WIDE = { width: 1040, height: 720 };
const BOTH = ["dark", "light"];
const shots = [
  // The evidence contract's states.
  ["s1-cold", "s1-cold", WIDE, BOTH],
  ["s1-cold-first", "s1-cold-first", WIDE, BOTH],
  ["s2-start", "s2-start", WIDE, BOTH],
  ["s2-answer", "s2-answer", WIDE, BOTH],
  ["s3-before", "s3-before", WIDE, BOTH],
  ["s3-after", "s3-after", WIDE, BOTH],
  ["s4-offplan", "s4-offplan", WIDE, BOTH],
  ["s4-return", "s4-return", WIDE, BOTH],
  ["s5-week", "s5-week", WIDE, BOTH],
  ["s5-week-next", "s5-week-next", WIDE, BOTH],
  ["narrow-lesson", "narrow-lesson", { width: 720, height: 560 }, ["dark"]],
  ["settings", "settings", WIDE, BOTH],
  // More of the window.
  ["courses", "courses", WIDE, BOTH],
  ["course", "course", WIDE, BOTH],
  ["plan", "plan", WIDE, BOTH],
  ["commands", "commands", WIDE, ["dark"]],
  ["source", "source", WIDE, ["dark"]],
  ["stop", "stop", WIDE, ["dark"]],
  ["paper", "paper", WIDE, ["dark"]],
  ["setup-signed-out", "setup-signed-out", WIDE, ["dark"]],
  ["setup-no-claude", "setup-no-claude", WIDE, ["dark"]],
  ["setup-replace", "setup-replace", WIDE, ["dark"]],
  ["face-iowan", "face-iowan", WIDE, ["light"]],
  ["s2-answer-narrow-light", "s2-answer", { width: 720, height: 560 }, ["light"]],
].filter(([name]) => !only || name.includes(only));

const server = await createServer({ server: { port: 1431, strictPort: false }, logLevel: "error" });
await server.listen();
const base = server.resolvedUrls.local[0];
await mkdir("shots", { recursive: true });
if (!only) await rm("shots", { recursive: true, force: true }).then(() => mkdir("shots", { recursive: true }));

const browser = await chromium.launch({ channel: "chrome", headless: true });
const problems = [];
let count = 0;
for (const [name, address, viewport, schemes] of shots) {
  for (const scheme of schemes) {
    const page = await browser.newPage({ viewport, deviceScaleFactor: 2, colorScheme: scheme });
    page.on("pageerror", (error) => problems.push(`${name}: ${error.message}`));
    page.on("console", (message) => {
      if (message.type() === "error") problems.push(`${name}: ${message.text()}`);
    });
    await page.goto(`${base}#${address}`);
    try {
      await page.waitForSelector("html[data-shot='ready']", { timeout: 15_000 });
    } catch {
      problems.push(`${name}: the demo did not reach its state`);
    }
    // Fonts, KaTeX and the last paint.
    await page.evaluate(() => document.fonts.ready);
    await page.waitForTimeout(250);
    const suffix = name === "narrow-lesson" ? "-dark" : `-${scheme}`;
    await page.screenshot({ path: `shots/${name}${name.endsWith("-light") ? "" : suffix}.png` });
    count += 1;
    await page.close();
  }
}
await browser.close();
await server.close();

if (problems.length > 0) {
  console.error(`${problems.length} problem(s) on the page:\n` + problems.join("\n"));
  process.exit(1);
}
console.log(`photographed ${count} states into shots/`);
