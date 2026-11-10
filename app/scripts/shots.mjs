// Photograph every screen with the demo's made-up data, to compare with the
// approved mock. Uses the Chrome already on this computer, with no window.
//
//   npm run shots            all screens, into shots/
//   npm run shots -- paper   only the screens whose name contains "paper"

import { mkdir } from "node:fs/promises";
import { chromium } from "playwright-core";
import { createServer } from "vite";

const only = process.argv[2];
const shots = [
  ["welcome", "welcome", 600],
  ["entrance", "entrance", 600],
  ["entrance-quiet", "entrance:quiet", 600],
  ["entrance-new-folder", "entrance:new", 600],
  ["entrance-new-lesson", "entrance:ask", 600],
  ["entrance-broken", "entrance:broken", 600],
  ["session", "session", 1500],
  ["session-reply", "session:answer", 3200],
  ["session-writing", "session:stream", 5200],
  ["session-lost", "session:lost", 2600],
  ["session-dont-know", "session:idk", 2200],
  ["session-permission", "session:permission", 2200],
  ["session-diagram", "session:diagram", 5200],
  ["session-stop", "session:stop", 1800],
  ["session-earlier", "session:earlier", 1800],
  ["session-limit", "session:limit", 2200],
  ["session-new-lesson", "session:fresh", 2600],
  ["progress-rows", "progress:A", 900],
  ["progress-map", "progress:B", 900],
  ["progress-levels", "progress:C", 900],
  ["paper-choose", "paper", 700],
  ["paper-running", "paper:running", 900],
  ["palette", "palette", 700],
].filter(([name]) => !only || name.includes(only));

const server = await createServer({ server: { port: 1431, strictPort: false }, logLevel: "error" });
await server.listen();
const base = server.resolvedUrls.local[0];
await mkdir("shots", { recursive: true });

const browser = await chromium.launch({ channel: "chrome", headless: true });
const problems = [];
for (const scheme of ["dark", "light"]) {
  for (const [name, address, settle] of shots) {
    if (scheme === "light" && !["entrance", "session-reply", "progress-rows"].includes(name)) continue;
    const page = await browser.newPage({ viewport: { width: 1040, height: 720 }, colorScheme: scheme });
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
// The same lesson in a narrow window.
if (!only || "session-narrow".includes(only)) {
  const page = await browser.newPage({ viewport: { width: 620, height: 720 }, colorScheme: "dark" });
  await page.goto(`${base}#session:answer`);
  await page.waitForTimeout(3200);
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
