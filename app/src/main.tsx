import { createRoot } from "react-dom/client";

import "@fontsource-variable/literata/opsz.css";
import "@fontsource-variable/literata/opsz-italic.css";
import "./styles/fonts.css";

import { App } from "./App";
import "./harness";
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

// Tab reaches every button and link. In WebKit, which draws the real window,
// Tab skips buttons and links unless the Mac's "keyboard navigation" setting
// is on (Safari behaves the same), so a window with no text field on it was
// out of reach of the keyboard. Every control that has no tabindex of its
// own is given 0 as it appears; a control that sets its own (a group of
// radios, which is one stop) is left as it set it, and a disabled button
// is given none until it is enabled.
const REACHABLE = 'button, a[href], [role="button"], summary';
function reachable(element: Element) {
  if (element.hasAttribute("tabindex")) return;
  if (element instanceof HTMLButtonElement && element.disabled) return;
  element.setAttribute("tabindex", "0");
}
function reachAll(root: Element) {
  if (root.matches(REACHABLE)) reachable(root);
  root.querySelectorAll(REACHABLE).forEach(reachable);
}
reachAll(document.documentElement);
new MutationObserver((changes) => {
  for (const change of changes) {
    if (change.type === "attributes" && change.target instanceof Element) {
      if (change.target.matches(REACHABLE)) reachable(change.target);
    } else change.addedNodes.forEach((node) => node instanceof Element && reachAll(node));
  }
}).observe(document.documentElement, { childList: true, subtree: true, attributes: true, attributeFilter: ["disabled", "href", "role"] });

// The first paint uses the final faces. They are bundled with the window, so
// this takes a few milliseconds and makes no request; a face that cannot be
// had within 300 ms is not waited for, and the page draws with the fallback.
// Besides the two text faces, the few that mathematics is set in: a goal or a
// step with a formula in it is a different width until they are there.
const FACES = [
  ...['"Literata Variable"', '"Magnesium Sans"'].flatMap((face) => ["normal", "italic"].map((style) => `${style} 400 1em ${face}`)),
  "normal 600 1em \"Magnesium Sans\"",
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
