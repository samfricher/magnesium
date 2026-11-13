// A layer: what a key summons over the surface, and Esc puts away. Every layer
// is this one thing, so one habit covers all of them: a line to type in, a
// list under it, and the keys at its foot.
//
// The line either filters the list ("filter"), or is said to the tutor when
// Return is pressed with words on it ("say", the plan's layer). A list with
// nothing to type (settings) has no line.
//
// A layer is a native modal dialog (dialog guide): the window behind it is
// inert, focus stays in it, Esc closes it, and focus goes back to where it
// was. The line is a combobox over a listbox (menu and dialog guides): the
// row ↑ and ↓ choose is the line's active option, so a screen reader says it
// while the focus stays on the line.

import { useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";

export type Choice = { label: string; chosen: boolean; pick: () => void; face?: string };

export type Row = {
  id: string;
  /** What the row shows. A string is drawn as it is. */
  label: ReactNode;
  /** The words the line filters on. */
  text: string;
  /** "thing": the learner's own material, in the reading face. "do": the window's verbs, in its own face. "note": words that do nothing. "head": a group's name. */
  kind?: "thing" | "do" | "file" | "note" | "head";
  /** A mark before the row: the state of a plan item. */
  mark?: string;
  /** Quiet words after the row: a key, a date, what changed. */
  hint?: ReactNode;
  /** The row was changed by the tutor in this sitting. */
  changed?: boolean;
  run?: () => void;
  /** ⌘↩ */
  alt?: () => void;
  /** ⌘⌫ */
  remove?: () => void;
  /** A setting's choices, changed with ← and →. */
  choices?: Choice[];
  drive?: string;
  /** Shown only once the line holds words that find it, as a setting's values are. */
  onlyFound?: boolean;
};

type Props = {
  name: string;
  mode: "filter" | "say" | "none";
  placeholder?: string;
  rows: Row[];
  /** Offered last when the line holds words: say them to the tutor. */
  fallback?: (text: string) => Row | null;
  /** In "say" mode: Return with words on the line. */
  onSay?: (text: string) => void;
  /** Rows can be chosen by their number, 1 to 9. */
  numbered?: boolean;
  keys: string;
  /** Something drawn above the list, such as the paste field or a source. */
  wide?: boolean;
  onClose: () => void;
  children?: ReactNode;
};

const runnable = (row: Row) => Boolean(row.run || row.choices);

function matches(row: Row, words: string[]): boolean {
  if (row.kind === "head") return false;
  const text = row.text.toLowerCase();
  return words.every((word) => text.includes(word));
}

/** Whether a key press belongs to an input method still composing: WebKit can report it with isComposing false, but keyCode 229. */
const composing = (event: React.KeyboardEvent) => event.nativeEvent.isComposing || event.keyCode === 229;

export function Layer(props: Props) {
  const [text, setText] = useState("");
  const [at, setAt] = useState(0);
  const field = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const box = useRef<HTMLDialogElement>(null);
  const id = useId();
  const listId = `${id}-rows`;
  const rowId = (index: number) => `${id}-row-${index}`;

  // Opened as a modal as it is drawn, and closed before it is taken away, so focus goes back to where it was.
  useLayoutEffect(() => {
    const dialog = box.current;
    if (dialog && !dialog.open) {
      try {
        dialog.showModal();
      } catch {
        dialog.setAttribute("open", "");
      }
    }
    return () => {
      if (dialog?.open) dialog.close();
    };
  }, []);

  const rows = useMemo(() => {
    if (props.mode !== "filter" || text.trim() === "") return props.rows.filter((row) => !row.onlyFound);
    const words = text.toLowerCase().split(/\s+/).filter(Boolean);
    const found = props.rows.filter((row) => matches(row, words));
    const extra = props.fallback?.(text.trim());
    return extra ? [...found, extra] : found;
  }, [props.rows, props.mode, props.fallback, text]);

  const choosable = rows.map((row, index) => (runnable(row) ? index : -1)).filter((index) => index >= 0);
  const selected = choosable.includes(at) ? at : (choosable[0] ?? -1);

  useEffect(() => {
    (field.current ?? list.current)?.focus({ preventScroll: true });
  }, []);

  useLayoutEffect(() => {
    list.current?.querySelector<HTMLElement>(`[data-at="${selected}"]`)?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  const move = (step: number) => {
    if (choosable.length === 0) return;
    const from = choosable.indexOf(selected);
    setAt(choosable[(from + step + choosable.length) % choosable.length]);
  };

  const onKeyDown = (event: React.KeyboardEvent) => {
    // Return and Esc that confirm or cancel an input method's composition are its own.
    if (composing(event)) return;
    const row = selected >= 0 ? rows[selected] : null;
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      props.onClose();
    } else if (event.key === "ArrowDown" || (event.key === "Tab" && !event.shiftKey)) {
      event.preventDefault();
      move(1);
    } else if (event.key === "ArrowUp" || (event.key === "Tab" && event.shiftKey)) {
      event.preventDefault();
      move(-1);
    } else if ((event.key === "ArrowLeft" || event.key === "ArrowRight") && row?.choices && text === "") {
      event.preventDefault();
      const choices = row.choices;
      const now = Math.max(0, choices.findIndex((choice) => choice.chosen));
      choices[(now + (event.key === "ArrowRight" ? 1 : -1) + choices.length) % choices.length].pick();
    } else if (event.key === "Enter") {
      event.preventDefault();
      if (props.mode === "say" && text.trim() !== "" && !event.metaKey) {
        props.onSay?.(text.trim());
        setText("");
        return;
      }
      if (!row) return;
      if (event.metaKey && row.alt) row.alt();
      else if (row.choices) {
        const now = Math.max(0, row.choices.findIndex((choice) => choice.chosen));
        row.choices[(now + 1) % row.choices.length].pick();
      } else row.run?.();
    } else if (event.key === "Backspace" && event.metaKey && row?.remove) {
      event.preventDefault();
      row.remove();
    } else if (props.numbered && /^[1-9]$/.test(event.key) && text === "" && !event.metaKey && !event.altKey) {
      const target = choosable[Number(event.key) - 1];
      if (target !== undefined) {
        event.preventDefault();
        rows[target].run?.();
      }
    }
  };

  /** A press outside the layer's box, on its backdrop, closes it. The pointer's place is tested: the dialog is also the target of a press on its own edge. */
  const onBackdrop = (event: React.MouseEvent<HTMLDialogElement>) => {
    if (event.target !== event.currentTarget) return;
    const r = event.currentTarget.getBoundingClientRect();
    if (event.clientX < r.left || event.clientX > r.right || event.clientY < r.top || event.clientY > r.bottom) props.onClose();
  };

  const active = selected >= 0 ? rowId(selected) : undefined;
  let number = 0;
  return (
    <>
      <div className="scrim" />
      <dialog
        ref={box}
        className={`layer${props.wide ? " wide" : ""}`}
        aria-label={props.name}
        data-layer={props.name}
        onKeyDown={onKeyDown}
        onMouseDown={onBackdrop}
        onCancel={(event) => {
          event.preventDefault();
          props.onClose();
        }}
      >
        {props.mode !== "none" && (
          <div className="layer-line">
            <input
              ref={field}
              data-drive="layer"
              role="combobox"
              aria-expanded="true"
              aria-controls={listId}
              aria-autocomplete="list"
              aria-activedescendant={active}
              aria-label={props.placeholder ?? props.name}
              placeholder={props.placeholder}
              value={text}
              spellCheck={props.mode === "say"}
              autoCorrect="off"
              autoCapitalize="none"
              onChange={(event) => {
                setText(event.target.value);
                setAt(0);
              }}
            />
          </div>
        )}
        {props.children}
        <div
          className="rows"
          id={listId}
          ref={list}
          tabIndex={props.mode === "none" ? 0 : -1}
          role="listbox"
          aria-label={props.name}
          aria-activedescendant={props.mode === "none" ? active : undefined}
        >
          {rows.map((row, index) => {
            if (runnable(row)) number += 1;
            const shownNumber = props.numbered && runnable(row) ? number : null;
            return (
              <div
                key={row.id}
                id={rowId(index)}
                data-at={index}
                data-drive={row.drive}
                role={runnable(row) ? "option" : "presentation"}
                aria-selected={index === selected}
                className={`row ${row.kind ?? "do"}${index === selected ? " on" : ""}${row.changed ? " changed" : ""}`}
                onMouseMove={() => runnable(row) && index !== selected && setAt(index)}
                onClick={() => (row.choices ? undefined : row.run?.())}
              >
                <span className="row-mark" aria-hidden="true">
                  {shownNumber ?? row.mark ?? ""}
                </span>
                <span className="row-label">{row.label}</span>
                {row.choices ? (
                  <span className="choices">
                    {row.choices.map((choice) => (
                      <button
                        key={choice.label}
                        className={choice.chosen ? "choice chosen" : "choice"}
                        style={choice.face ? { fontFamily: choice.face } : undefined}
                        tabIndex={-1}
                        onClick={(event) => {
                          event.stopPropagation();
                          setAt(index);
                          choice.pick();
                        }}
                      >
                        {choice.label}
                      </button>
                    ))}
                  </span>
                ) : (
                  row.hint !== undefined && <span className="row-hint">{row.hint}</span>
                )}
              </div>
            );
          })}
          {rows.length === 0 && <p className="row note empty">Nothing matches.</p>}
        </div>
        {props.keys !== "" && (
          <p className="layer-keys" aria-hidden="true">
            {props.keys}
          </p>
        )}
      </dialog>
    </>
  );
}
