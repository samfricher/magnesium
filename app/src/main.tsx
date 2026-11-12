import { createRoot } from "react-dom/client";

import { App } from "./App";
import "./harness";
import { applyPrefs, loadPrefs } from "./prefs";
import "./styles/surface.css";
import "./styles/prose.css";

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

// The learner's face, size, scheme and ink are on the page before anything is drawn.
applyPrefs(loadPrefs());

// A focus ring is for the keyboard: the first key pressed shows it, the next click hides it.
document.documentElement.dataset.input = "pointer";
window.addEventListener("keydown", () => (document.documentElement.dataset.input = "key"), true);
window.addEventListener("pointerdown", () => (document.documentElement.dataset.input = "pointer"), true);

// Tab reaches every button and link. In WebKit, which draws the real window,
// Tab skips them unless the Mac's "keyboard navigation" setting is on, so each
// control with no tabindex of its own is given 0 as it appears.
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

// The text faces are the Mac's own, so nothing waits for them. The faces
// mathematics is set in are bundled; a goal with a formula in it is the
// first line on the surface, so the first paint waits for them, up to 150 ms.
const MATH = ["normal 400 1em KaTeX_Main", "italic 400 1em KaTeX_Math"];
async function mathReady(): Promise<void> {
  const loads = MATH.map((face) => document.fonts.load(face, "Aa0"));
  await Promise.race([Promise.allSettled(loads), new Promise((done) => window.setTimeout(done, 150))]);
}

void mathReady()
  .catch(() => {})
  .then(() => createRoot(document.getElementById("root")!).render(<App />));
