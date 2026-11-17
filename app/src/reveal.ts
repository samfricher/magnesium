// When the first screen is drawn whole, so the shell can let the window be
// seen (src-tauri/src/reveal.rs) and the harness can time it.
//
// "Drawn whole" is the page in front showing its real content, read from the
// page itself, and then nothing on the page changing, not even where the
// focus is, for two frames, in the key window: Home focuses its first row (its rule and ↩) a
// frame after its rows appear, and a session lays out its kept turns after
// they are read. The shell shows the window anyway after a while, so a page
// that never settles is still seen.

/** The page in front has its real content: a predicate over the page as it is drawn. */
export type Drawn = () => boolean;

/** Content the page in front shows once read, by the kind of page. */
export function frontDrawn(): boolean {
  // Sessions stay on the page behind other tabs; the one in front is marked. Any other page is drawn only in front.
  const root = document;
  // A session: its kept turns, or what comes next, are on the page (not still loading).
  if (root.querySelector(".session.in-front")) return root.querySelector(".session.in-front .now:not(.loading)") !== null;
  // A folder's page, once its plan, sessions and record are read.
  if (root.querySelector('[data-page="workspace"]')) return root.querySelector('[data-page="workspace"][data-read]') !== null;
  // Home, once its column shows (App waits for that before asking).
  return true;
}

const QUIET_FRAMES = 2;
const KEY_FRAMES = 30;

/** Resolves at the frame after the page has had its real content and been still for two frames. */
export function whenDrawn(drawn: Drawn = frontDrawn): Promise<void> {
  return new Promise((done) => {
    let changed = 0;
    let frame = 0;
    const note = () => (changed = frame);
    const watch = new MutationObserver(note);
    watch.observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true });
    document.addEventListener("focusin", note, true);
    window.addEventListener("focus", note);
    const tick = () => {
      frame += 1;
      // The focused row draws its rule only once the window is the key window, which the shell asks for as it puts
      // the window on screen; a window opened behind others (`open -g`) never becomes it, so not past half a second.
      const key = document.hasFocus() || frame > KEY_FRAMES;
      if (drawn() && key && frame - changed > QUIET_FRAMES) {
        watch.disconnect();
        document.removeEventListener("focusin", note, true);
        window.removeEventListener("focus", note);
        // This frame's paint reaches the window before the next frame begins.
        requestAnimationFrame(() => done());
        return;
      }
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
}
