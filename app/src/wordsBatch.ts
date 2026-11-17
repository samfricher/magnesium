// A reply's words come in many small pieces, often several between two
// frames, and each one put on the page redraws the whole reply: its Markdown,
// formulas and sanitising. This gathers them and hands them on at most once a
// frame. A frame never comes for a window that is not drawn (covered, or on
// another desktop), so a short timer stands in for it.

/** Run something at the next frame, or after some ms; each gives back a way to call it off. */
export type Clock = {
  frame: (run: () => void) => () => void;
  after: (ms: number, run: () => void) => () => void;
};

export const pageClock: Clock = {
  frame: (run) => {
    const id = requestAnimationFrame(() => run());
    return () => cancelAnimationFrame(id);
  },
  after: (ms, run) => {
    const id = window.setTimeout(run, ms);
    return () => window.clearTimeout(id);
  },
};

export type WordsBatch = {
  /** Words arrived: they are handed on at the next frame, with any others that come before it. */
  add: (text: string) => void;
  /** Hand on what is held now: before anything else that happened after those words. */
  flush: () => void;
  /** Let go of what is held, handing on nothing. */
  drop: () => void;
};

export function wordsBatch(deliver: (text: string) => void, clock: Clock, fallbackMs = 100): WordsBatch {
  let held = "";
  let stops: (() => void)[] = [];
  const stop = () => {
    for (const each of stops) each();
    stops = [];
  };
  const flush = () => {
    stop();
    if (held === "") return;
    const text = held;
    held = "";
    deliver(text);
  };
  return {
    add: (text) => {
      held += text;
      if (stops.length === 0) stops = [clock.frame(flush), clock.after(fallbackMs, flush)];
    },
    flush,
    drop: () => {
      stop();
      held = "";
    },
  };
}
