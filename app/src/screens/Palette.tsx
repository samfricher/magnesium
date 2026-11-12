// Everything that is not the lesson lives here, behind one key, so none of it
// sits on screen while the learner works. It is a modal combobox: the input
// holds focus, the arrow keys move through the options, and closing it puts
// focus back where it was.

import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";

import { composing } from "../ui/bits";
import { Icon } from "../ui/icons";

export type Command = {
  label: string;
  hint?: string;
  icon?: string;
  /** Other words that find this item. */
  keywords?: string;
  /** Shown, but it cannot run now: the reason, in words. */
  disabled?: string;
  /** Instead of running, show these keys and what they do. */
  shortcuts?: [string, string][];
  run: () => void;
};

export function Palette(props: { commands: Command[]; footer?: string | null; onClose: () => void }) {
  const [query, setQuery] = useState("");
  const [at, setAt] = useState(0);
  const [keys, setKeys] = useState<Command | null>(null);
  const dialog = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLDivElement>(null);
  // Whatever had focus when the palette opened gets it back when it closes.
  const opener = useRef<Element | null>(document.activeElement);

  const shown = useMemo(() => {
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    return props.commands.filter((command) => {
      const text = `${command.label} ${command.hint ?? ""} ${command.keywords ?? ""}`.toLowerCase();
      return words.every((word) => text.includes(word));
    });
  }, [props.commands, query]);

  useEffect(() => setAt(0), [query]);
  useEffect(() => {
    list.current?.children[at]?.scrollIntoView({ block: "nearest" });
  }, [at]);

  // Focus goes to the input and stays there: a screen behind that takes
  // focus a moment later (an answer box, say) is given no keystrokes.
  useLayoutEffect(() => {
    input.current?.focus();
    const keep = (event: FocusEvent) => {
      if (dialog.current && event.target instanceof Node && !dialog.current.contains(event.target)) input.current?.focus();
    };
    document.addEventListener("focusin", keep);
    const before = opener.current;
    return () => {
      document.removeEventListener("focusin", keep);
      if (before instanceof HTMLElement && document.contains(before)) before.focus();
    };
  }, []);

  const run = (command: Command | undefined) => {
    if (!command || command.disabled) return;
    if (command.shortcuts) {
      setKeys(command);
      return;
    }
    props.onClose();
    command.run();
  };

  const active = shown[at] ? `palette-option-${at}` : undefined;

  return (
    <div
      className="palette-veil"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) props.onClose();
      }}
    >
      <div
        ref={dialog}
        className="palette"
        role="dialog"
        aria-modal="true"
        aria-label="Menu"
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            e.preventDefault();
            e.stopPropagation();
            if (keys) setKeys(null);
            else props.onClose();
          }
        }}
      >
        <div className="palette-in">
          <Icon name="search" size={17} />
          <input
            ref={input}
            role="combobox"
            aria-expanded={!keys}
            aria-controls="palette-list"
            aria-autocomplete="list"
            aria-activedescendant={keys ? undefined : active}
            placeholder="Type a command"
            aria-label="Type a command"
            spellCheck={false}
            value={query}
            onChange={(e) => {
              setKeys(null);
              setQuery(e.target.value);
            }}
            onKeyDown={(e) => {
              if (composing(e)) return;
              if (e.key === "ArrowDown") {
                e.preventDefault();
                setAt((n) => Math.min(n + 1, shown.length - 1));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setAt((n) => Math.max(n - 1, 0));
              } else if (e.key === "Enter") {
                e.preventDefault();
                if (!keys) run(shown[at]);
              }
            }}
          />
        </div>
        {keys?.shortcuts ? (
          <div className="palette-keys" id="palette-list">
            <p className="palette-keys-title">{keys.label}</p>
            {keys.shortcuts.map(([key, what]) => (
              <div className="palette-key" key={key}>
                <kbd>{key}</kbd>
                <span>{what}</span>
              </div>
            ))}
            <p className="palette-keys-back">Press Esc to go back to the list.</p>
          </div>
        ) : (
          <div className="palette-list" id="palette-list" role="listbox" aria-label="Commands" ref={list}>
            {shown.map((command, index) => (
              <div
                key={`${command.label}${command.hint ?? ""}`}
                id={`palette-option-${index}`}
                role="option"
                aria-selected={index === at}
                aria-disabled={command.disabled ? true : undefined}
                className={`${index === at ? "on" : ""}${command.disabled ? " off" : ""}`.trim() || undefined}
                onMouseEnter={() => setAt(index)}
                onClick={() => run(command)}
              >
                <Icon name={command.icon ?? "right"} size={16} />
                <span>{command.label}</span>
                <small>{command.disabled ?? command.hint}</small>
              </div>
            ))}
            {shown.length === 0 && <div className="palette-none">Nothing matches. Try: proofs, new lesson, paper.</div>}
          </div>
        )}
        {props.footer && <div className="palette-foot selectable">{props.footer}</div>}
      </div>
    </div>
  );
}
