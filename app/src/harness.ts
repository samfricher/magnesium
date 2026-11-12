// The evidence harness: a dev-only hook that lets the real window be driven,
// photographed and timed. It does nothing unless the window was started with
// MAGNESIUM_APP_DRIVE or MAGNESIUM_APP_PERF set, and nothing at all in the
// browser demo.
//
// Every concept calls `ready()` once its first screen has its real content
// painted (not a skeleton). Startup is measured from process start to the
// paint after that call.

let readyCalled = false;

/** Call once, when the first screen's real content is painted. Later calls do nothing. */
export function ready(): void {
  if (readyCalled) return;
  readyCalled = true;
}
