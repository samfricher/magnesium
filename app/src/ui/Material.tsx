// The small pieces of adding course material that more than one screen shows:
// what was added, the sheet for pasted text, the veil while files are dragged
// over the window, and the menu in the answer box.

import { useState } from "react";

import { Icon } from "./icons";

/** What adding material said: one plain line each. */
export function MaterialNote({ lines, onDismiss }: { lines: string[]; onDismiss: () => void }) {
  return (
    <div className="banner material selectable" data-role="material-note" role="status">
      <span>
        {lines.map((line, index) => (
          <span className="material-line" key={index}>
            {line}
          </span>
        ))}
      </span>
      <button className="icon-btn" aria-label="Dismiss" onClick={onDismiss}>
        <Icon name="close" size={14} />
      </button>
    </div>
  );
}

/** The sheet where text is pasted to be kept as a text file with the course material. */
export function PasteSheet(props: { initial: string; saving: boolean; onSave: (text: string) => void; onCancel: () => void }) {
  const [text, setText] = useState(props.initial);
  const ready = text.trim() !== "" && !props.saving;
  return (
    <div
      className="palette-veil"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) props.onCancel();
      }}
    >
      <div
        className="palette paste"
        role="dialog"
        aria-modal="true"
        aria-label="Paste text"
        data-role="paste-sheet"
        onKeyDown={(event) => {
          // Tab goes round the sheet and never out of it.
          if (event.key !== "Tab") return;
          const items = Array.from(event.currentTarget.querySelectorAll<HTMLElement>("textarea, button:not(:disabled)"));
          const at = items.indexOf(document.activeElement as HTMLElement);
          if (items.length === 0) return;
          const to = (at + (event.shiftKey ? -1 : 1) + items.length) % items.length;
          event.preventDefault();
          items[to]?.focus();
        }}
      >
        <h2 className="h-section">Paste text</h2>
        <textarea
          className="field paste-box"
          autoFocus
          spellCheck={false}
          placeholder="Notes, a page from a website, a question sheet"
          aria-label="Text to keep"
          value={text}
          onChange={(event) => setText(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.stopPropagation();
              props.onCancel();
            }
            if (event.key === "Enter" && (event.metaKey || event.ctrlKey) && ready) props.onSave(text);
          }}
        />
        <div className="row">
          <button className="btn primary" disabled={!ready} title="Save as a text file (Command-Return)" onClick={() => props.onSave(text)}>
            {props.saving ? "Saving…" : "Save as a text file"}
          </button>
          <button className="btn ghost" onClick={props.onCancel}>
            Cancel
          </button>
          <span className="aside">Kept in your course material, where the tutor can search it.</span>
        </div>
      </div>
    </div>
  );
}

/** Shown over the window while files are dragged onto it. */
export function DropVeil({ course }: { course: string }) {
  return (
    <div className="drop-veil" data-role="drop-veil" aria-hidden="true">
      <div>
        <Icon name="plus" size={26} />
        <b>Drop to add to {course}</b>
        <span>Slides, notes, papers. The tutor can search what it can read.</span>
      </div>
    </div>
  );
}

