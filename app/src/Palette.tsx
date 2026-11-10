// Everything that is not the lesson lives here, behind one key, so none of it
// sits on screen while the learner works.

import { useEffect, useMemo, useRef, useState } from "react";

import { composing } from "./Session";

export type Command = { label: string; hint?: string; run: () => void };

export function Palette(props: { commands: Command[]; onClose: () => void }) {
  const [query, setQuery] = useState("");
  const [at, setAt] = useState(0);
  const list = useRef<HTMLDivElement>(null);

  const shown = useMemo(() => {
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    return props.commands.filter((command) => {
      const text = `${command.label} ${command.hint ?? ""}`.toLowerCase();
      return words.every((word) => text.includes(word));
    });
  }, [props.commands, query]);

  useEffect(() => setAt(0), [query]);
  useEffect(() => {
    list.current?.children[at]?.scrollIntoView({ block: "nearest" });
  }, [at]);

  const run = (command: Command | undefined) => {
    if (!command) return;
    props.onClose();
    command.run();
  };

  return (
    <div
      className="pal"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) props.onClose();
      }}
    >
      <div className="palbox" role="dialog" aria-label="Commands">
        <input
          className="palin"
          autoFocus
          placeholder="Type a command…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
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
              run(shown[at]);
            } else if (e.key === "Escape") {
              e.preventDefault();
              props.onClose();
            }
          }}
        />
        <div className="pallist" ref={list}>
          {shown.map((command, index) => (
            <button
              key={`${command.label}${command.hint ?? ""}`}
              className={index === at ? "on" : undefined}
              onMouseEnter={() => setAt(index)}
              onClick={() => run(command)}
            >
              {command.label}
              {command.hint && <span>{command.hint}</span>}
            </button>
          ))}
          {shown.length === 0 && <div className="none">Nothing matches.</div>}
        </div>
      </div>
    </div>
  );
}
