import type { RuntimeState, Setup } from "./types";

const LESSONS_RUN: readonly RuntimeState[] = ["ready", "kept", "different"];

export function setupReady(setup: Setup | null): boolean {
  if (!setup) return false;
  return LESSONS_RUN.includes(setup.runtime.state) && setup.account.installed && setup.account.signed_in;
}
