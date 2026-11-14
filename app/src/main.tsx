import { Component, type ReactNode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./App";
import { mark } from "./harness";
import { applyPrefs, loadPrefs } from "./prefs";
import "./styles/surface.css";
import "./styles/prose.css";
import "./styles/window.css";
import "./styles/side.css";

// Anything that goes wrong on the page is kept, so the self-check can report it.
declare global {
  interface Window {
    __mgErrors: string[];
  }
}
window.__mgErrors = [];
mark("script");
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

/** A page that fails says so in a line, and keeps what failed for the self-check, instead of going blank. */
class Kept extends Component<{ children: ReactNode }, { failed: string | null }> {
  state = { failed: null as string | null };
  static getDerivedStateFromError(error: unknown) {
    return { failed: String(error) };
  }
  componentDidCatch(error: unknown, info: { componentStack?: string | null }) {
    window.__mgErrors.push(`render: ${String(error)} ${(info.componentStack ?? "").split("\n").slice(0, 4).join(" ")}`);
  }
  render() {
    if (this.state.failed) return <p className="record note page-failed">The window met a problem it cannot draw past: {this.state.failed}. Quit and open it again.</p>;
    return this.props.children;
  }
}

// The text faces are the Mac's own and the faces mathematics is set in are
// bundled, so nothing waits for a face: the first paint is the real one.
createRoot(document.getElementById("root")!).render(
  <Kept>
    <App />
  </Kept>,
);
