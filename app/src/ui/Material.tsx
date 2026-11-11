// The small pieces of adding course material that more than one screen shows:
// what was added, the sheet for pasted text, the veil while files are dragged
// over the window, and the menu in the answer box.

import { useEffect, useRef, useState } from "react";

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
      <div className="palette paste" role="dialog" aria-label="Paste text" data-role="paste-sheet">
        <span className="eyebrow">Paste text</span>
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
          <button className="btn primary" disabled={!ready} onClick={() => props.onSave(text)}>
            {props.saving ? "Saving…" : "Save as a text file"} {!props.saving && <kbd>&#8984;&#8629;</kbd>}
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

/** The paperclip in the answer box: pick files, or paste text. */
export function AddMenu(props: { disabled: boolean; onFiles: () => void; onPaste: () => void }) {
  const [open, setOpen] = useState(false);
  const holder = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => {
      if (!holder.current?.contains(event.target as Node)) setOpen(false);
    };
    const key = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", key);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", key);
    };
  }, [open]);
  return (
    <span className="add-menu" ref={holder}>
      <button
        className={open ? "icon-btn on" : "icon-btn"}
        title="Add course material"
        aria-label="Add course material"
        aria-expanded={open}
        disabled={props.disabled}
        data-role="add-material"
        onClick={() => setOpen(!open)}
      >
        <Icon name="clip" size={17} />
      </button>
      {open && (
        <span className="menu" role="menu">
          <button
            role="menuitem"
            onClick={() => {
              setOpen(false);
              props.onFiles();
            }}
          >
            Choose files…
          </button>
          <button
            role="menuitem"
            onClick={() => {
              setOpen(false);
              props.onPaste();
            }}
          >
            Paste text…
          </button>
        </span>
      )}
    </span>
  );
}
