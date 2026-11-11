// Photograph every screen with the demo's made-up data. Uses the Chrome
// already on this computer, with no window.
//
//   npm run shots            all screens, into shots/
//   npm run shots -- paper   only the screens whose name contains "paper"

import { mkdir } from "node:fs/promises";
import { chromium } from "playwright-core";
import { createServer } from "vite";

const only = process.argv[2];
const shots = [
  ["setup-first-open", "welcome:first", 700],
  ["setup-replace", "welcome:replace", 700],
  ["setup-signed-out", "welcome:signed-out", 700],
  ["setup-no-claude", "welcome:no-claude", 700],
  ["setup-key-in-use", "welcome:key", 700],
  ["setup", "setup", 900],
  ["library", "library", 900],
  ["library-empty", "library:empty", 900],
  ["course", "course", 900],
  ["course-quiet", "course:quiet", 900],
  ["course-new-folder", "course:new", 900],
  ["course-new-lesson", "course:ask", 900],
  ["course-material", "course:material", 1400],
  ["course-drop", "course:drop", 900],
  ["course-paste", "course:paste", 900],
  ["course-upgrade", "course:upgrade", 900],
  ["course-broken", "course:broken", 900],
  ["session", "session", 1800],
  ["session-reply", "session:answer", 3600],
  ["session-source", "session:source", 5200],
  ["session-key", "session:key", 3600],
  ["session-material", "session:material", 3600],
  ["session-drop", "session:drop", 2400],
  ["session-math-answer", "session:math", 3600],
  ["session-typing-math", "session:typing", 2200],
  ["session-writing", "session:stream", 5400],
  ["session-lost", "session:lost", 2800],
  ["session-dont-know", "session:idk", 2400],
  ["session-hint", "session:hint", 2400],
  ["session-working", "session:slow", 2400],
  ["session-permission", "session:permission", 2400],
  ["session-diagram", "session:diagram", 5600],
  ["session-plan", "session:plan", 2000],
  ["session-stop", "session:stop", 2000],
  ["session-earlier", "session:earlier", 2000],
  ["session-limit", "session:limit", 2400],
  ["session-new-lesson", "session:fresh", 2800],
  ["summary", "summary", 5200],
  ["progress", "progress", 1100],
  ["paper-choose", "paper", 900],
  ["paper-running", "paper:running", 1100],
  ["palette", "palette", 900],
].filter(([name]) => !only || name.includes(only));

// Screens also photographed in the light scheme.
const light = ["library", "course", "course-new-folder", "course-material", "session-reply", "session-source", "progress", "summary"];

const server = await createServer({ server: { port: 1431, strictPort: false }, logLevel: "error" });
await server.listen();
const base = server.resolvedUrls.local[0];
await mkdir("shots", { recursive: true });

const browser = await chromium.launch({ channel: "chrome", headless: true });
const problems = [];
for (const scheme of ["dark", "light"]) {
  for (const [name, address, settle] of shots) {
    if (scheme === "light" && !light.includes(name)) continue;
    const page = await browser.newPage({ viewport: { width: 1040, height: 720 }, deviceScaleFactor: 2, colorScheme: scheme });
    page.on("pageerror", (error) => problems.push(`${name}: ${error.message}`));
    page.on("console", (message) => {
      if (message.type() === "error") problems.push(`${name}: ${message.text()}`);
    });
    await page.goto(`${base}#${address}`);
    await page.waitForTimeout(settle);
    await page.screenshot({ path: `shots/${name}${scheme === "light" ? "-light" : ""}.png` });
    await page.close();
  }
}
// The same lesson in a narrow window, and with a source open in one.
if (!only || "session-source-narrow".includes(only)) {
  const page = await browser.newPage({ viewport: { width: 720, height: 640 }, deviceScaleFactor: 2, colorScheme: "dark" });
  await page.goto(`${base}#session:source`);
  await page.waitForTimeout(5200);
  await page.screenshot({ path: "shots/session-source-narrow.png" });
  await page.close();
}
if (!only || "session-narrow".includes(only)) {
  const page = await browser.newPage({ viewport: { width: 720, height: 560 }, deviceScaleFactor: 2, colorScheme: "dark" });
  await page.goto(`${base}#session:answer`);
  await page.waitForTimeout(3600);
  await page.screenshot({ path: "shots/session-narrow.png" });
  await page.close();
}
await browser.close();
await server.close();

if (problems.length > 0) {
  console.error(`${problems.length} problem(s) on the page:\n` + problems.join("\n"));
  process.exit(1);
}
console.log(`photographed ${shots.length} screens into shots/`);
