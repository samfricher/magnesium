type Anchor = { el: HTMLElement; ms: string | null; at: number; y: number };

export type Spot = {
  follow: () => void;
  restore: (fromFoot: number) => void;
  settle: () => void;
  scrolled: () => void;
};

const END = 6;

const proseOf = (turn: Element) => turn.querySelector<HTMLElement>(":scope > .tutor > .prose");

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
  const ms = turn.dataset.ms ?? null;
  const held = (el: Element, at: number): Anchor => ({ el: el as HTMLElement, ms, at, y: el.getBoundingClientRect().top });
  if (turn.matches("article.turn")) {
    for (const part of Array.from(turn.children)) {
      if (part.getBoundingClientRect().bottom <= line) continue;
      const prose = part.classList.contains("tutor") ? proseOf(turn) : null;
      if (!prose) return held(part, -1);
      const blocks = Array.from(prose.children);
      const at = blocks.findIndex((block) => block.getBoundingClientRect().bottom > line);
      return at >= 0 ? held(blocks[at], at) : held(part, -1);
    }
  }
  return held(turn, -1);
}

function anchorNow(section: HTMLElement, anchor: Anchor): HTMLElement | null {
  if (anchor.el.isConnected) return anchor.el;
  if (anchor.ms === null || anchor.at < 0) return null;
  const turn = section.querySelector(`:scope > article.turn[data-ms="${anchor.ms}"]`);
  const found = turn ? proseOf(turn)?.children[anchor.at] : null;
  return found instanceof HTMLElement ? found : null;
}

export function readingSpot(view: () => HTMLElement | null, section: () => HTMLElement | null): Spot {
  let following = true;
  let seenTop = 0;
  let anchor: Anchor | null = null;

  const look = (shown: HTMLElement, page: HTMLElement) => {
    seenTop = shown.scrollTop;
    anchor = anchorAt(shown, page);
  };
  const apply = (shown: HTMLElement, page: HTMLElement) => {
    const scrolledBy = shown.scrollTop - seenTop;
    if (Math.abs(scrolledBy) > 1) {
      const atEnd = shown.scrollHeight - shown.clientHeight - shown.scrollTop < END;
      following = scrolledBy < 0 ? atEnd : following || atEnd;
    }
    if (following) shown.scrollTop = shown.scrollHeight;
    else if (anchor) {
      const el = anchorNow(page, anchor);
      if (el && el.getClientRects().length > 0) {
        const by = Math.round(el.getBoundingClientRect().top - (anchor.y - scrolledBy));
        if (by !== 0) shown.scrollTop += by;
      }
    }
    look(shown, page);
  };
  const both = (): [HTMLElement, HTMLElement] | null => {
    const shown = view();
    const page = section();
    return shown && page && shown.clientHeight > 0 ? [shown, page] : null;
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
      const found = both();
      if (found) apply(...found);
    },
    scrolled: () => {
      const found = both();
      if (found && Math.abs(found[0].scrollTop - seenTop) > 1) apply(...found);
    },
  };
}
