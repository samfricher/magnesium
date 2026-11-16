// A short menu opened by one of the line's words. More floats over the line; Stuck opens as a row of its own
// in the line, above the words, so it does not cover the question the learner is stuck on.
// Its rows are data, so a tool joins the line by joining a list of rows.

import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { RefObject } from "react";

export type LineRow = {
  /** The row's harness name, and its React key. */
  id: string;
  label: string;
  /** The key that does the same from anywhere, as the window names it. */
  keys?: string;
  /** Said at the row's end instead of a key: what the row leads to, such as the model the session is on. */
  detail?: string;
  /** The row opens something further rather than acting. */
  leads?: boolean;
  disabled?: boolean;
  /** Rows of one group sit together; a rule parts one group from the next. */
  group: string;
  run: () => void;
};

type Props = {
  label: string;
  rows: LineRow[];
  /** The word that opened the menu: a press on it is its own, not a press elsewhere. */
  opener: RefObject<HTMLElement>;
  /** The menu closed; `back` when the keyboard should go back to the line. */
  onClose: (back: boolean) => void;
  /** Drawn as a row in the line's own flow, not floating over the page. */
  inline?: boolean;
};

export function LineMenu({ label, rows, opener, onClose, inline = false }: Props) {
  const menu = useRef<HTMLDivElement>(null);
  // Over the line, unless the line sits too high on the page for the menu to fit above it. Under a session's
  // first line, which has the next thing's name above it and the page's empty ground below.
  const [below, setBelow] = useState(false);
  const close = useRef(onClose);
  close.current = onClose;

  useLayoutEffect(() => {
    const el = menu.current;
    const line = el?.closest(".line");
    if (!el || !line) return;
    if (inline) {
      // The row made the line taller: the whole line stays in view, not only the row the keyboard goes to.
      el.querySelector<HTMLElement>("[role='menuitem']:not(:disabled)")?.focus({ preventScroll: true });
      line.scrollIntoView({ block: "nearest" });
      return;
    }
    const top = document.querySelector(".tabs")?.getBoundingClientRect().bottom ?? 0;
    setBelow(line.classList.contains("first") || line.getBoundingClientRect().top - top - 16 < el.offsetHeight);
    el.querySelector<HTMLElement>("[role='menuitem']:not(:disabled)")?.focus();
  }, []);

  // Esc closes it wherever the keyboard is; a press or the keyboard going elsewhere closes it too.
  useEffect(() => {
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      close.current(true);
    };
    const away = (event: Event) => {
      const target = event.target as Node;
      if (menu.current?.contains(target) || opener.current?.contains(target)) return;
      close.current(false);
    };
    document.addEventListener("keydown", escape, true);
    document.addEventListener("mousedown", away);
    document.addEventListener("focusin", away);
    return () => {
      document.removeEventListener("keydown", escape, true);
      document.removeEventListener("mousedown", away);
      document.removeEventListener("focusin", away);
    };
  }, [opener]);

  const onKeyDown = (event: React.KeyboardEvent) => {
    const forward = event.key === "ArrowDown" || (inline && event.key === "ArrowRight");
    const back = event.key === "ArrowUp" || (inline && event.key === "ArrowLeft");
    if (!forward && !back) return;
    const stops = [...(menu.current?.querySelectorAll<HTMLElement>("[role='menuitem']:not(:disabled)") ?? [])];
    const at = stops.indexOf(document.activeElement as HTMLElement);
    const next = stops[(at + (forward ? 1 : -1) + stops.length) % stops.length];
    if (next) {
      event.preventDefault();
      next.focus();
    }
  };

  return (
    <div
      className={`line-menu${inline ? " inline" : below ? " below" : ""}`}
      ref={menu}
      role="menu"
      aria-label={label}
      aria-orientation={inline ? "horizontal" : undefined}
      onKeyDown={onKeyDown}
    >
      {rows.map((row, index) => (
        <button
          key={row.id}
          type="button"
          role="menuitem"
          className={`line-menu-row${index > 0 && rows[index - 1].group !== row.group ? " parted" : ""}`}
          data-drive={row.id}
          aria-haspopup={row.leads ? "dialog" : undefined}
          disabled={row.disabled}
          onMouseDown={(event) => event.preventDefault()}
          onClick={row.run}
        >
          <span className="line-menu-label">{row.label}</span>
          {row.detail ? (
            <span className="line-menu-detail">
              {row.detail}
              {row.leads && <span aria-hidden="true"> ›</span>}
            </span>
          ) : (
            row.keys && <span className="line-menu-key">{row.keys}</span>
          )}
        </button>
      ))}
    </div>
  );
}
