type Anchor = { el: HTMLElement; ms: string | null; at: number; y: number };

export type Spot = {
  follow: () => void;
  restore: (fromFoot: number) => void;
  settle: () => void;
  scrolled: () => void;
};

const END = 6;

function blocksOf(turn: Element): Element[] {
  return Array.from(turn.children).flatMap((child) => (child.classList.contains("tutor") ? Array.from(child.querySelector(":scope > .prose")?.children ?? []) : [child]));
}

function anchorAt(view: HTMLElement, section: HTMLElement): Anchor | null {
  const line = view.getBoundingClientRect().top;
  const turns = section.querySelectorAll<HTMLElement>(":scope > article.turn");
  let low = 0;
  let high = turns.length - 1;
  let first = -1;
  while (low <= high) {
    const mid = (low + high) >> 1;
    if (turns[mid].getBoundingClientRect().bottom > line) {
      first = mid;
      high = mid - 1;
    } else low = mid + 1;
  }
  const turn = first >= 0 ? turns[first] : section.querySelector<HTMLElement>(":scope > .foot");
  if (!turn || turn.getClientRects().length === 0) return null;
  const blocks = turn.matches("article.turn") ? blocksOf(turn) : [];
  const at = blocks.findIndex((block) => block.getBoundingClientRect().bottom > line);
  const el = at >= 0 ? (blocks[at] as HTMLElement) : turn;
  return { el, ms: turn.dataset.ms ?? null, at, y: el.getBoundingClientRect().top };
}

function anchorNow(section: HTMLElement, anchor: Anchor): HTMLElement | null {
  if (anchor.el.isConnected) return anchor.el;
  if (anchor.ms === null) return null;
  const turn = section.querySelector<HTMLElement>(`:scope > article.turn[data-ms="${anchor.ms}"]`);
  if (!turn || anchor.at < 0) return turn;
  const found = blocksOf(turn)[anchor.at];
  return found instanceof HTMLElement ? found : null;
}

export function readingSpot(view: () => HTMLElement | null, section: () => HTMLElement | null): Spot {
  let following = true;
  let seenTop = 0;
  let anchor: Anchor | null = null;

  const look = (shown: HTMLElement, page: HTMLElement) => {
    seenTop = shown.scrollTop;
    anchor = following ? null : anchorAt(shown, page);
  };
  const moved = (shown: HTMLElement) => {
    const fromFoot = shown.scrollHeight - shown.clientHeight - shown.scrollTop;
    following = shown.scrollTop < seenTop ? fromFoot < END : following || fromFoot < END;
  };

  return {
    follow: () => {
      following = true;
    },
    restore: (fromFoot) => {
      const shown = view();
      const page = section();
      if (!shown || !page) return;
      shown.scrollTop = shown.scrollHeight - shown.clientHeight - fromFoot;
      following = fromFoot < END;
      look(shown, page);
    },
    settle: () => {
      const shown = view();
      const page = section();
      if (!shown || !page || shown.clientHeight === 0) return;
      if (Math.abs(shown.scrollTop - seenTop) > 1) moved(shown);
      else if (following) shown.scrollTop = shown.scrollHeight;
      else if (anchor) {
        const el = anchorNow(page, anchor);
        if (el && el.getClientRects().length > 0) {
          const by = Math.round(el.getBoundingClientRect().top - anchor.y);
          if (by !== 0) shown.scrollTop += by;
        }
      }
      look(shown, page);
    },
    scrolled: () => {
      const shown = view();
      const page = section();
      if (!shown || !page || Math.abs(shown.scrollTop - seenTop) <= 1) return;
      moved(shown);
      look(shown, page);
    },
  };
}
