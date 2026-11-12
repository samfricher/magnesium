import { createRoot } from "react-dom/client";

import "@fontsource-variable/literata/opsz.css";
import "@fontsource-variable/literata/opsz-italic.css";
import "@fontsource-variable/atkinson-hyperlegible-next/wght.css";
import "@fontsource-variable/atkinson-hyperlegible-next/wght-italic.css";

import { App } from "./App";
import "./styles/base.css";
import "./styles/prose.css";
import "./styles/screens.css";

// Anything that goes wrong on the page is kept, so the self-check can report it.
declare global {
  interface Window {
    __mgErrors: string[];
  }
}
window.__mgErrors = [];
window.addEventListener("error", (event) => window.__mgErrors.push(`error: ${event.message}`));
window.addEventListener("unhandledrejection", (event) => window.__mgErrors.push(`rejection: ${String(event.reason)}`));
document.addEventListener("securitypolicyviolation", (event) =>
  window.__mgErrors.push(`blocked by the content policy: ${event.violatedDirective} ${event.blockedURI}`),
);

// A focus ring is for the keyboard. The page starts in "pointer" mode, so a
// control the window focuses by itself on load shows no ring; the first key
// pressed switches it on, and the next click or touch switches it off again.
document.documentElement.dataset.input = "pointer";
window.addEventListener("keydown", () => (document.documentElement.dataset.input = "key"), true);
window.addEventListener("pointerdown", () => (document.documentElement.dataset.input = "pointer"), true);

// The first paint uses the final faces. They are bundled with the window, so
// this takes a few milliseconds and makes no request; a face that cannot be
// had within 300 ms is not waited for, and the page draws with the fallback.
// Besides the two text faces, the few that mathematics is set in: a goal or a
// step with a formula in it is a different width until they are there.
const FACES = [
  ...['"Literata Variable"', '"Atkinson Hyperlegible Next Variable"'].flatMap((face) => ["normal", "italic"].map((style) => `${style} 400 1em ${face}`)),
  "normal 400 1em KaTeX_Main",
  "italic 400 1em KaTeX_Main",
  "normal 700 1em KaTeX_Main",
  "italic 400 1em KaTeX_Math",
  "normal 400 1em KaTeX_Size1",
  "normal 400 1em KaTeX_Size2",
  "normal 400 1em KaTeX_AMS",
];
const SAMPLE = "Aa0 µ–α";
async function facesReady(): Promise<void> {
  const loads = FACES.map((face) => document.fonts.load(face, SAMPLE));
  await Promise.race([Promise.allSettled(loads), new Promise((done) => window.setTimeout(done, 300))]);
}

void facesReady()
  .catch(() => {})
  .then(() => createRoot(document.getElementById("root")!).render(<App />));
