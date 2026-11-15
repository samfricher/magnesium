// A short menu over the lesson's line, opened by one of its words (Stuck, More).
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
};

export function LineMenu({ label, rows, opener, onClose }: Props) {
  const menu = useRef<HTMLDivElement>(null);
  // Over the line, unless the line sits too high on the page for the menu to fit above it.
  const [below, setBelow] = useState(false);
  const close = useRef(onClose);
  close.current = onClose;

  useLayoutEffect(() => {
    const el = menu.current;
    const line = el?.closest(".line");
    if (!el || !line) return;
    const top = document.querySelector(".tabs")?.getBoundingClientRect().bottom ?? 0;
    setBelow(line.getBoundingClientRect().top - top - 16 < el.offsetHeight);
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
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    const stops = [...(menu.current?.querySelectorAll<HTMLElement>("[role='menuitem']:not(:disabled)") ?? [])];
    const at = stops.indexOf(document.activeElement as HTMLElement);
    const next = stops[(at + (event.key === "ArrowDown" ? 1 : -1) + stops.length) % stops.length];
    if (next) {
      event.preventDefault();
      next.focus();
    }
  };

  return (
    <div className={`line-menu${below ? " below" : ""}`} ref={menu} role="menu" aria-label={label} onKeyDown={onKeyDown}>
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
