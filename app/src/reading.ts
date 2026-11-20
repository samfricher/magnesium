type Anchor = { el: HTMLElement; ms: string | null; at: number; y: number };

export type Spot = {
  follow: () => void;
  restore: (fromFoot: number) => void;
  settle: () => void;
  scrolled: () => boolean;
};

const END = 6;
const MOVED = 0.5;

const proseOf = (turn: Element) => turn.querySelector<HTMLElement>(":scope > .tutor > .prose");

function anchorAt(view: HTMLElement, section: HTMLElement): Anchor | null {
  const line = view.getBoundingClientRect().top;
  const folded = section.classList.contains("folded");
  for (const part of Array.from(section.children) as HTMLElement[]) {
    const turn = part.matches("article.turn");
    if ((folded && turn) || part.matches(".foot.raised") || part.getClientRects().length === 0 || part.getBoundingClientRect().bottom <= line) continue;
    const ms = turn ? (part.dataset.ms ?? null) : null;
    const held = (el: Element, at: number): Anchor => ({ el: el as HTMLElement, ms, at, y: el.getBoundingClientRect().top });
    if (ms === null) return held(part, -1);
    for (const piece of Array.from(part.children)) {
      if (piece.getBoundingClientRect().bottom <= line) continue;
      const prose = piece.classList.contains("tutor") ? proseOf(part) : null;
      if (!prose) return held(piece, -1);
      const blocks = Array.from(prose.children);
      const at = blocks.findIndex((block) => block.getBoundingClientRect().bottom > line);
      return at >= 0 ? held(blocks[at], at) : held(piece, -1);
    }
    return held(part, -1);
  }
  return null;
}

function anchorNow(section: HTMLElement, anchor: Anchor): HTMLElement | null {
  if (anchor.ms !== null && section.classList.contains("folded")) return null;
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
    if (Math.abs(scrolledBy) > MOVED) {
      const fromFoot = shown.scrollHeight - shown.clientHeight - shown.scrollTop;
      following = scrolledBy < 0 ? fromFoot <= MOVED : following || fromFoot < END;
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
      if (!found || Math.abs(found[0].scrollTop - seenTop) <= MOVED) return false;
      apply(...found);
      return true;
    },
  };
}
