import { createRoot } from "react-dom/client";

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

createRoot(document.getElementById("root")!).render(<App />);
