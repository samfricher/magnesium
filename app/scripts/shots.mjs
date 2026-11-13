// Photograph the window with the demo's made-up data, in both schemes. Uses
// the Chrome already on this computer, with no window. Each address drives the
// demo to its state; the page says when it is there (data-shot="ready").
//
//   npm run shots            every state, into shots/
//   npm run shots -- s3      only the states whose name contains "s3"
//
// The names follow the evidence contract: <stem>-dark.png and <stem>-light.png
// at 1040 x 720, and narrow-lesson-dark.png at 720 x 560. Then the combined
// window's own screens: Home, a workspace, Settings, the plan beside the
// lesson shown and hidden, a file on the line, the tabs and ⌘K.
//
// These are photographs in Chrome. What only the Mac's WebKit draws (the
// colour wells, the range sliders, the menu bar) is checked in the real window.

import { mkdir } from "node:fs/promises";
import { chromium } from "playwright-core";
import { createServer } from "vite";

const only = process.argv[2];

// [stem, address, settle after ready (ms), viewport, schemes]
const WIDE = { width: 1040, height: 720 };
const BOTH = ["dark", "light"];
const shots = [
  // The five scenarios. The cold open is Home now.
  ["s1-cold", "s1-cold", 300],
  ["s1-cold-first", "s1-cold-first", 300],
  ["s2-start", "s2-start", 400],
  ["s2-answer", "s2-answer", 400],
  ["s3-before", "s3-before", 200],
  ["s3-after", "s3-after", 400],
  ["s4-offplan", "s4-offplan", 400],
  ["s4-return", "s4-return", 400],
  ["s5-week", "s5-week", 300],
  ["s5-week-next", "s5-week-next", 400],
  ["narrow-lesson", "narrow-lesson", 400, { width: 720, height: 560 }],
  // Home, Workspace, Session, Settings.
  ["workspace", "workspace", 400],
  ["new-session", "new-session", 400],
  ["new-session-start", "new-session-start", 400],
  ["session-earlier", "session-earlier", 400],
  ["settings", "settings", 300],
  ["settings-keys", "settings-keys", 300],
  ["settings-colour", "settings-colour", 300],
  ["material", "material", 600],
  // The plan beside the lesson: shown, and shown after the tutor changed it.
  ["panel", "panel", 400],
  ["panel-changed", "panel-changed", 500],
  ["panel-narrow", "panel", 400, { width: 720, height: 560 }],
  // The line: help under the answer, symbols, a file on the line.
  ["help-row", "help-row", 300],
  ["symbols", "symbols", 300],
  ["attach", "attach", 400],
  // Tabs, ⌘K and the rest.
  ["tabs", "tabs", 500],
  ["commands", "commands", 300],
  ["switch", "switch", 300],
  ["source", "source", 1200],
  ["working", "working", 700],
  ["permission", "permission", 500],
  ["paper", "paper", 300],
  ["upgrade", "upgrade", 300],
  ["setup-signed-out", "setup-signed-out", 300],
  ["setup-no-claude", "setup-no-claude", 300],
  ["setup-replace", "setup-replace", 300],
  ["setup-too-old", "setup-too-old", 300],
].filter(([name]) => !only || name.includes(only));

const server = await createServer({ server: { port: 1431, strictPort: false }, logLevel: "error" });
await server.listen();
const base = server.resolvedUrls.local[0];
await mkdir("shots", { recursive: true });

const browser = await chromium.launch({ channel: "chrome", headless: true });
const problems = [];
let count = 0;
for (const [name, address, settle, viewport = WIDE, schemes = BOTH] of shots) {
  for (const scheme of schemes) {
    const page = await browser.newPage({ viewport, deviceScaleFactor: 2, colorScheme: scheme });
    page.on("pageerror", (error) => problems.push(`${name}: ${error.message}`));
    page.on("console", (message) => {
      if (message.type() === "error") problems.push(`${name}: ${message.text()}`);
    });
    await page.goto(`${base}#${address}`);
    try {
      await page.waitForSelector("html[data-shot=ready]", { timeout: 15_000 });
    } catch {
      problems.push(`${name}: the demo never reached its state`);
    }
    await page.waitForTimeout(settle);
    await page.screenshot({ path: `shots/${name}-${scheme}.png` });
    await page.close();
    count += 1;
  }
}
await browser.close();
await server.close();

if (problems.length > 0) {
  console.error(`${problems.length} problem(s) on the page:\n` + problems.join("\n"));
  process.exit(1);
}
console.log(`photographed ${count} states into shots/`);
