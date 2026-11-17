// The page of a tab that is not a session: Home (or a workspace's page in its place) and Settings. It stays on the
// page behind the tab in front, as a session does, so coming back to it shows it as it was left: drawn, scrolled
// and read, with nothing read again before it is drawn. When these pages were taken off and made again on each
// switch, a workspace's page drew half of itself and then moved down as its reading came in.

import { useEffect, useLayoutEffect, useRef, type ReactNode } from "react";

type Props = {
  front: boolean;
  labelledBy: string;
  children: ReactNode;
};

export function TabPage({ front, labelledBy, children }: Props) {
  const box = useRef<HTMLDivElement>(null);
  // The last thing in the page that had the keyboard. A click on another tab takes the keyboard to the tab bar
  // first, so what had it when the page went behind is not known then.
  const kept = useRef<HTMLElement | null>(null);
  // What `kept` was as the page went behind: the page's own effects may focus something else as it comes back (Home
  // puts the keyboard on its first row), which `kept` then follows.
  const leftOn = useRef<HTMLElement | null>(null);

  // Behind, the page is out of reach of the keyboard and of a screen reader. Set before any effect runs, so a page
  // brought to the front can take the focus.
  useLayoutEffect(() => {
    const page = box.current;
    if (!page) return;
    if (!front) {
      leftOn.current = kept.current;
      // Hidden, a focused row would still draw what its :focus shows (visibility: visible); it lets go.
      const at = document.activeElement;
      if (at instanceof HTMLElement && page.contains(at)) at.blur();
    }
    page.toggleAttribute("inert", !front);
  }, [front]);

  // Back in front, the keyboard goes back where it was, after the page's own effects have run.
  useEffect(() => {
    if (!front) return;
    const at = leftOn.current;
    leftOn.current = null;
    if (at && at.isConnected && box.current?.contains(at) && document.activeElement !== at) at.focus({ preventScroll: true });
  }, [front]);

  return (
    <div
      ref={box}
      className={`view${front ? " in-front" : ""}`}
      role="tabpanel"
      aria-labelledby={labelledBy}
      onFocus={(event) => {
        if (event.target instanceof HTMLElement) kept.current = event.target;
      }}
    >
      {children}
    </div>
  );
}
