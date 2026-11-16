// Whether this Mac can run a lesson now. Kept apart from the session's
// component so that scripts/test-setup.mjs can check it with node alone.

import type { RuntimeState, Setup } from "./types";

/**
 * The program states a lesson runs in. A lesson runs the `magnesium` found on
 * this Mac (runtime::program in the core), so "different" (this app carries
 * another copy, and the learner has not chosen yet) runs the installed one, as
 * "kept" does. The core refuses an installed copy that is too old as
 * "too_old" before this is asked.
 */
const LESSONS_RUN: readonly RuntimeState[] = ["ready", "kept", "different"];

/** Lessons start only when this Mac is set up: the program is usable and Claude Code is signed in. */
export function setupReady(setup: Setup | null): boolean {
  if (!setup) return false;
  return LESSONS_RUN.includes(setup.runtime.state) && setup.account.installed && setup.account.signed_in;
}
