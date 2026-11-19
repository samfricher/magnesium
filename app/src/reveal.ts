export type Drawn = () => boolean;

export function frontDrawn(): boolean {
  const root = document;
  if (root.querySelector(".session.in-front")) return root.querySelector(".session.in-front .now:not(.loading)") !== null;
  if (root.querySelector('[data-page="workspace"]')) return root.querySelector('[data-page="workspace"][data-read]') !== null;
  return true;
}

const QUIET_FRAMES = 2;
const KEY_FRAMES = 30;
export const GIVE_UP_MS = 1500;

export function whenDrawn(drawn: Drawn = frontDrawn, giveUpAt: number = GIVE_UP_MS): Promise<void> {
  return new Promise((done) => {
    let changed = 0;
    let frame = 0;
    let over = false;
    const note = () => (changed = frame);
    const watch = new MutationObserver(note);
    watch.observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true });
    document.addEventListener("focusin", note, true);
    window.addEventListener("focus", note);
    const finish = () => {
      if (over) return;
      over = true;
      window.clearTimeout(late);
      watch.disconnect();
      document.removeEventListener("focusin", note, true);
      window.removeEventListener("focus", note);
      done();
    };
    const late = window.setTimeout(finish, Math.max(0, giveUpAt - performance.now()));
    const tick = () => {
      if (over) return;
      frame += 1;
      const key = document.hasFocus() || frame > KEY_FRAMES;
      if (drawn() && key && frame - changed > QUIET_FRAMES) {
        requestAnimationFrame(finish);
        return;
      }
      if (performance.now() >= giveUpAt) return finish();
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
}
