import { Component, type ReactNode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./App";
import { core } from "./bridge";
import { mark } from "./harness";
import { applyPrefs, loadPrefs } from "./prefs";
import "./styles/surface.css";
import "./styles/prose.css";
import "./styles/window.css";
import "./styles/side.css";

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

applyPrefs(loadPrefs());

document.documentElement.dataset.input = "pointer";
window.addEventListener("keydown", () => (document.documentElement.dataset.input = "key"), true);
window.addEventListener("pointerdown", () => (document.documentElement.dataset.input = "pointer"), true);

document.addEventListener("selectionchange", () => {
  const selection = document.getSelection();
  const whole = selection !== null && !selection.isCollapsed;
  if (whole !== "whole" in document.documentElement.dataset) document.documentElement.toggleAttribute("data-whole", whole);
});

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

class Kept extends Component<{ children: ReactNode }, { failed: string | null }> {
  state = { failed: null as string | null };
  static getDerivedStateFromError(error: unknown) {
    return { failed: String(error) };
  }
  componentDidCatch(error: unknown, info: { componentStack?: string | null }) {
    window.__mgErrors.push(`render: ${String(error)} ${(info.componentStack ?? "").split("\n").slice(0, 4).join(" ")}`);
    void core().then((api) => api.showWindow()).catch(() => {});
  }
  render() {
    if (this.state.failed) return <p className="record note page-failed">The window met a problem it cannot draw past: {this.state.failed}. Quit and open it again.</p>;
    return this.props.children;
  }
}

createRoot(document.getElementById("root")!).render(
  <Kept>
    <App />
  </Kept>,
);
