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
  add: (text: string) => void;
  flush: () => void;
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
