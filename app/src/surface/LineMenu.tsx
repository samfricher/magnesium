import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { RefObject } from "react";

export type LineRow = {
  id: string;
  label: string;
  keys?: string;
  detail?: string;
  leads?: boolean;
  disabled?: boolean;
  group: string;
  run: () => void;
};

type Props = {
  label: string;
  rows: LineRow[];
  opener: RefObject<HTMLElement>;
  onClose: (back: boolean) => void;
  inline?: boolean;
};

export function LineMenu({ label, rows, opener, onClose, inline = false }: Props) {
  const menu = useRef<HTMLDivElement>(null);
  const [below, setBelow] = useState(false);
  const close = useRef(onClose);
  close.current = onClose;

  useLayoutEffect(() => {
    const el = menu.current;
    const line = el?.closest(".line");
    if (!el || !line) return;
    if (inline) {
      el.querySelector<HTMLElement>("[role='menuitem']:not(:disabled)")?.focus({ preventScroll: true });
      line.scrollIntoView({ block: "nearest" });
      return;
    }
    const top = document.querySelector(".tabs")?.getBoundingClientRect().bottom ?? 0;
    setBelow(line.classList.contains("first") || line.getBoundingClientRect().top - top - 16 < el.offsetHeight);
    el.querySelector<HTMLElement>("[role='menuitem']:not(:disabled)")?.focus();
  }, []);

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
