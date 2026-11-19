import { createContext, useCallback, useContext, useEffect, useLayoutEffect, useRef, type ReactNode } from "react";

const Claim = createContext<() => void>(() => {});
export const useClaimKeyboard = () => useContext(Claim);

type Props = {
  front: boolean;
  labelledBy: string;
  children: ReactNode;
};

export function TabPage({ front, labelledBy, children }: Props) {
  const box = useRef<HTMLDivElement>(null);
  const kept = useRef<HTMLElement | null>(null);
  const leftOn = useRef<HTMLElement | null>(null);
  const claimed = useRef(false);
  const claim = useCallback(() => {
    claimed.current = true;
  }, []);

  useLayoutEffect(() => {
    const page = box.current;
    if (!page) return;
    if (!front) {
      leftOn.current = kept.current;
      claimed.current = false;
      const at = document.activeElement;
      if (at instanceof HTMLElement && page.contains(at)) at.blur();
    }
    page.toggleAttribute("inert", !front);
  }, [front]);

  useEffect(() => {
    if (!front) return;
    const at = leftOn.current;
    leftOn.current = null;
    const own = claimed.current;
    claimed.current = false;
    if (!own && at && at.isConnected && box.current?.contains(at) && document.activeElement !== at) at.focus({ preventScroll: true });
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
      <Claim.Provider value={claim}>{children}</Claim.Provider>
    </div>
  );
}
