// Glass: the Mac's own blur behind the page, when the learner chooses it in
// Settings. Their choice is one number from -10 to 10 (prefs.glass): 0 is the
// solid window, below it is frosted glass and above it Liquid Glass, each
// stronger the further from 0. The page only says which material it wants;
// the shell turns the window see-through and puts that material under it
// (src-tauri/src/glass.rs). How strong it is belongs to the page alone:
// `data-glass` names the material and `--glass` is the strength from 0 to 1,
// and the styles paint the ground that much clearer.
//
// A learner who never chooses it never reaches the shell from here: the
// window stays as it opens, opaque. Glass is also left off on a Mac that
// cannot draw it (and in the browser demo), and while the Mac is set to
// reduce transparency or increase contrast. That setting is read again each
// time the window comes forward, as it is changed in System Settings.

import { useSyncExternalStore } from "react";

import { core } from "./bridge";
import type { GlassMaterial, GlassState } from "./types";

let level = 0;
let state: GlassState | null = null;
/** What the shell was last asked for: "off", or the material and the scheme it was drawn for. Null until it is asked. */
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
  const on = level !== 0 && state !== null && state.can && !state.reduce;
  // A Mac without Liquid Glass draws the frosted material for either side of the scale.
  const material: GlassMaterial | null = !on ? null : level < 0 || !state?.liquid ? "frosted" : "liquid";
  if (material) {
    root.dataset.glass = material;
    root.style.setProperty("--glass", String(Math.abs(level) / 10));
  } else {
    delete root.dataset.glass;
    root.style.removeProperty("--glass");
  }
  const dark = pageDark();
  const ask = material ? `${material}:${dark}` : "off";
  // Off is only said to a window that had glass, and a change of strength alone is not said at all.
  if (ask === asked || (ask === "off" && asked === null)) return;
  asked = ask;
  void core()
    .then((api) => api.setGlass(material, dark))
    .catch(() => {
      // The shell could not draw it: the page goes back to its solid ground rather than show a clear window.
      delete root.dataset.glass;
    });
}

async function readState(): Promise<void> {
  try {
    state = await (await core()).glassState();
  } catch {
    state = { can: false, liquid: false, reduce: false };
  }
  sync();
  for (const listener of listeners) listener();
}

/** Say what the learner chose, from -10 to 10. Called with every change to their settings. */
export function wantGlass(chosen: number): void {
  level = chosen;
  if (level !== 0 && !watching) {
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
