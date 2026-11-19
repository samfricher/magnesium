import { useLayoutEffect, useState, type RefObject } from "react";

export function useHangs(edge: RefObject<HTMLElement | null>, scroller: string, need: (edge: HTMLElement) => number, again: unknown[] = []): boolean {
  const [hangs, setHangs] = useState(true);
  useLayoutEffect(() => {
    const element = edge.current;
    const page = element?.closest<HTMLElement>(scroller) ?? null;
    if (!element || !page) return;
    const measure = () => {
      if (page.clientWidth === 0) return;
      const room = element.getBoundingClientRect().left - page.getBoundingClientRect().left;
      setHangs(room >= need(element) + 8);
    };
    measure();
    const watch = new ResizeObserver(measure);
    watch.observe(page);
    watch.observe(element);
    return () => watch.disconnect();
  }, again);
  return hangs;
}
