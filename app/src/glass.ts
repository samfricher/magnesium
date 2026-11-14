// Frosted glass: the Mac's own blur behind the page, when the learner chooses
// it in Settings. The page only says when it wants it; the shell turns the
// window see-through and puts the glass under it (src-tauri/src/glass.rs),
// and `data-glass="on"` lets the styles paint the ground part-way clear.
//
// A learner who never chooses it never reaches the shell from here: the
// window stays as it opens, opaque. Glass is also left off on a Mac that
// cannot draw it (and in the browser demo), and while the Mac is set to
// reduce transparency or increase contrast. That setting is read again each
// time the window comes forward, as it is changed in System Settings.

import { useSyncExternalStore } from "react";

import { core } from "./bridge";
import type { GlassState } from "./types";

let wanted = false;
let state: GlassState | null = null;
/** What the shell was last asked for: "off", or the scheme the glass was drawn for. Null until it is asked. */
let asked: string | null = null;
let watching = false;
const listeners = new Set<() => void>();

/** The scheme the page shows when one is set on it; null when it follows the Mac. */
function pageDark(): boolean | null {
  const set = document.documentElement.dataset.scheme;
  return set === "dark" ? true : set === "light" ? false : null;
}

function sync(): void {
  const root = document.documentElement;
  const on = wanted && state !== null && state.can && !state.reduce;
  if (on) root.dataset.glass = "on";
  else delete root.dataset.glass;
  const dark = pageDark();
  const ask = on ? `on:${dark}` : "off";
  // Off is only said to a window that had glass.
  if (ask === asked || (ask === "off" && asked === null)) return;
  asked = ask;
  void core()
    .then((api) => api.setGlass(on, dark))
    .catch(() => {
      // The shell could not draw it: the page goes back to its solid ground rather than show a clear window.
      delete root.dataset.glass;
    });
}

async function readState(): Promise<void> {
  try {
    state = await (await core()).glassState();
  } catch {
    state = { can: false, reduce: false };
  }
  sync();
  for (const listener of listeners) listener();
}

/** Say whether the learner wants glass. Called with every change to their settings. */
export function wantGlass(want: boolean): void {
  wanted = want;
  if (want && !watching) {
    watching = true;
    // The scheme can be set on the page from outside the settings (the evidence harness), and the glass is tinted to match it.
    new MutationObserver(sync).observe(document.documentElement, { attributes: true, attributeFilter: ["data-scheme"] });
    window.addEventListener("focus", () => void readState());
    void readState();
    return;
  }
  sync();
}

/** What this Mac can do, for Settings: null until it is known. */
export function useGlassState(): GlassState | null {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      if (state === null) void readState();
      return () => listeners.delete(listener);
    },
    () => state,
  );
}
