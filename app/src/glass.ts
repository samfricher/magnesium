import { useSyncExternalStore } from "react";

import { core } from "./bridge";
import type { GlassMaterial, GlassState } from "./types";

let level = 0;
let state: GlassState | null = null;
let asked: string | null = null;
let watching = false;
const listeners = new Set<() => void>();

function pageDark(): boolean | null {
  const set = document.documentElement.dataset.scheme;
  return set === "dark" ? true : set === "light" ? false : null;
}

function sync(): void {
  const root = document.documentElement;
  const on = level !== 0 && state !== null && state.can && !state.reduce;
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
  if (ask === asked || (ask === "off" && asked === null)) return;
  asked = ask;
  void core()
    .then((api) => api.setGlass(material, dark))
    .catch(() => {
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

export function wantGlass(chosen: number): void {
  level = chosen;
  if (level !== 0 && !watching) {
    watching = true;
    new MutationObserver(sync).observe(document.documentElement, { attributes: true, attributeFilter: ["data-scheme"] });
    window.addEventListener("focus", () => void readState());
    void readState();
    return;
  }
  sync();
}

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
