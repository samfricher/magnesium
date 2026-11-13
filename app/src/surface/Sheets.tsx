// A larger layer: text being pasted in as course material. It goes with Esc.
// (A source the tutor cited, and the course's material, open beside the
// lesson or the workspace's page now, in the panel: SidePanel.)

import { useEffect, useRef, useState } from "react";

export function PasteSheet(props: { initial: string; saving: boolean; onSave: (text: string) => void; onClose: () => void }) {
  const [text, setText] = useState(props.initial);
  const field = useRef<HTMLTextAreaElement>(null);
  useEffect(() => field.current?.focus(), []);
  return (
    <>
      <div className="scrim" onMouseDown={props.onClose} />
      <div className="layer sheet paste" role="dialog" aria-modal="true" aria-label="Paste text as course material">
        <p className="sheet-name">Text to keep with the course material</p>
        <textarea
          ref={field}
          className="paste-field"
          value={text}
          placeholder="Paste here"
          onChange={(event) => setText(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault();
              event.stopPropagation();
              props.onClose();
            } else if (event.key === "Enter" && event.metaKey && text.trim() !== "" && !props.saving) {
              event.preventDefault();
              props.onSave(text);
            }
          }}
        />
        <p className="layer-keys">{props.saving ? "Saving…" : "⌘↩ keep it   esc close"}</p>
      </div>
    </>
  );
}
