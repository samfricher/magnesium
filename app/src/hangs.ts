// Whether something drawn in the margin left of the column has room there: a pass's rings beside its mark, a day in
// Your work. The room is measured rather than read from the window's width, because the column's left edge moves with
// the window, the text size, the line width set in Settings and a panel open beside the page.

import { useLayoutEffect, useState, type RefObject } from "react";

/**
 * True while the space between `scroller`'s left edge and `edge`'s left edge holds `need(edge)` pixels and a little
 * more, so nothing hung there touches the window's edge. Measured before paint, and again whenever either resizes.
 */
export function useHangs(edge: RefObject<HTMLElement | null>, scroller: string, need: (edge: HTMLElement) => number, again: unknown[] = []): boolean {
  const [hangs, setHangs] = useState(true);
  useLayoutEffect(() => {
    const element = edge.current;
    const page = element?.closest<HTMLElement>(scroller) ?? null;
    if (!element || !page) return;
    const measure = () => {
      // A view not drawn (a tab behind another, display none) has nothing to measure; it is measured once drawn.
      if (page.clientWidth === 0) return;
      const room = element.getBoundingClientRect().left - page.getBoundingClientRect().left;
      setHangs(room >= need(element) + 8);
    };
    measure();
    const watch = new ResizeObserver(measure);
    watch.observe(page);
    watch.observe(element);
    return () => watch.disconnect();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, again);
  return hangs;
}
