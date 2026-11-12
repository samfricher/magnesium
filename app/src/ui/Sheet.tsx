// The one list the window ever lays over a document: open a document (⌘P),
// run a command (⌘K), open a study folder (⌘O), insert a symbol (⌘⇧M). Type
// to narrow it, ↑ ↓ to move, Return to choose, Esc to close.

import { useEffect, useMemo, useRef, useState } from "react";

import { renderInline } from "../render";

export type SheetItem = {
  id: string;
  label: string;
  /** Said after the label, quietly: a path, a key. */
  detail?: string;
  group?: string;
  keys?: string;
  /** Extra words it is found by. */
  words?: string;
  /** The label holds mathematics to draw. */
  math?: boolean;
  run: () => void;
  /** ⌘Return: open it beside the document. */
  beside?: () => void;
  /** ⌘⌫: take it off the list. */
  remove?: () => void;
};

/** How well a query matches a text: letters in order, closer and earlier is better. 0 is no match. */
export function score(query: string, text: string): number {
  const q = query.toLowerCase().replace(/\s+/g, "");
  if (q === "") return 1;
  const t = text.toLowerCase();
  const at = t.indexOf(query.toLowerCase().trim());
  if (at !== -1) return 1000 - at;
  let i = 0;
  let last = -1;
  let total = 0;
  for (const ch of q) {
    const found = t.indexOf(ch, i);
    if (found === -1) return 0;
    total += found === last + 1 ? 6 : 1;
    last = found;
    i = found + 1;
  }
  return total;
}

export function Sheet(props: { placeholder: string; items: SheetItem[]; onClose: () => void; empty?: string; label: string }) {
  const [query, setQuery] = useState("");
  const [at, setAt] = useState(0);
  const field = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLUListElement>(null);

  const shown = useMemo(() => {
    if (query.trim() === "") return props.items;
    return props.items
      .map((item) => ({ item, s: Math.max(score(query, item.label), score(query, `${item.words ?? ""} ${item.detail ?? ""}`) * 0.5) }))
      .filter((each) => each.s > 0)
      .sort((a, b) => b.s - a.s)
      .map((each) => each.item);
  }, [query, props.items]);

  useEffect(() => field.current?.focus(), []);
  useEffect(() => setAt(0), [query]);
  useEffect(() => {
    list.current?.querySelector(`[data-at="${at}"]`)?.scrollIntoView({ block: "nearest" });
  }, [at]);

  const choose = (item: SheetItem | undefined, beside: boolean) => {
    if (!item) return;
    props.onClose();
    if (beside && item.beside) item.beside();
    else item.run();
  };

  const onKeyDown = (event: React.KeyboardEvent) => {
    const command = event.metaKey || event.ctrlKey;
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      props.onClose();
    } else if (event.key === "ArrowDown" || (event.ctrlKey && event.key === "n")) {
      event.preventDefault();
      setAt((n) => Math.min(shown.length - 1, n + 1));
    } else if (event.key === "ArrowUp" || (event.ctrlKey && event.key === "p")) {
      event.preventDefault();
      setAt((n) => Math.max(0, n - 1));
    } else if (event.key === "Enter") {
      event.preventDefault();
      choose(shown[at], command);
    } else if (command && event.key === "Backspace" && shown[at]?.remove) {
      event.preventDefault();
      shown[at].remove?.();
    }
  };

  let lastGroup: string | undefined;
  return (
    <div className="sheet-veil" onMouseDown={(event) => event.target === event.currentTarget && props.onClose()}>
      <div className="sheet" role="dialog" aria-label={props.label} onKeyDown={onKeyDown}>
        <input
          ref={field}
          className="sheet-field"
          placeholder={props.placeholder}
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          aria-label={props.placeholder}
          aria-activedescendant={shown[at] ? `sheet-${at}` : undefined}
          spellCheck={false}
        />
        <ul className="sheet-list" ref={list} role="listbox">
          {shown.length === 0 && <li className="sheet-empty">{props.empty ?? "Nothing matches."}</li>}
          {shown.map((item, n) => {
            const header = query.trim() === "" && item.group && item.group !== lastGroup ? item.group : null;
            lastGroup = item.group;
            return (
              <li key={item.id} role="presentation">
                {header && <div className="sheet-group">{header}</div>}
                <div
                  id={`sheet-${n}`}
                  role="option"
                  aria-selected={n === at}
                  data-at={n}
                  className={n === at ? "sheet-item here" : "sheet-item"}
                  onMouseMove={() => setAt(n)}
                  onClick={(event) => choose(item, event.metaKey)}
                >
                  {item.math ? <span className="sheet-label" dangerouslySetInnerHTML={{ __html: renderInline(item.label) }} /> : <span className="sheet-label">{item.label}</span>}
                  {item.detail && <span className="sheet-detail">{item.detail}</span>}
                  {item.keys && <kbd>{item.keys}</kbd>}
                </div>
              </li>
            );
          })}
        </ul>
      </div>
    </div>
  );
}
