import { createRoot } from "react-dom/client";

import { App } from "./App";
import { applyPrefs, loadPrefs } from "./prefs";
import "./harness";
import "./styles/page.css";
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

// The page's faces are the Mac's own, so nothing waits for a font: the first
// paint is the real page. The learner's face, size and scheme go on before it.
applyPrefs(loadPrefs(), false);
createRoot(document.getElementById("root")!).render(<App />);
