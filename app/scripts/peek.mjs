// A look at one demo state for working on the page: prints what a script
// asks of it, and can photograph the page scrolled down.
//   node scripts/peek.mjs <address> [scrollTop] [out.png] [js]
import { chromium } from "playwright-core";
import { createServer } from "vite";
const [address = "s1-cold", scroll = "0", out, js] = process.argv.slice(2);
const server = await createServer({ server: { port: 1432, strictPort: false }, logLevel: "error" });
await server.listen();
const browser = await chromium.launch({ channel: "chrome", headless: true });
const page = await browser.newPage({ viewport: { width: 1040, height: 720 }, deviceScaleFactor: 1, colorScheme: "dark" });
page.on("pageerror", (e) => console.log("pageerror", e.message));
page.on("console", (m) => m.type() === "error" && console.log("console", m.text()));
await page.goto(`${server.resolvedUrls.local[0]}#${address}`);
await page.waitForSelector("html[data-shot='ready']", { timeout: 15000 }).catch(() => console.log("not ready"));
await page.evaluate((top) => (document.querySelector(".scroller").scrollTop = Number(top)), scroll);
await page.waitForTimeout(300);
if (js) console.log(JSON.stringify(await page.evaluate(js), null, 1));
if (out) await page.screenshot({ path: out });
await browser.close();
await server.close();
