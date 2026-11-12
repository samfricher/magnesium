// A place to write: the answer space inside a task, or the line at the end of
// a page. It holds its own text, so a keystroke repaints this field and
// nothing else on the page; the words are kept as a draft a moment later.

import { forwardRef, memo, useCallback, useEffect, useImperativeHandle, useRef, useState } from "react";

import { renderPlain } from "../render";

export type WriterHandle = {
  focus: () => void;
  insert: (text: string) => void;
  clear: () => void;
  value: () => string;
  element: () => HTMLTextAreaElement | null;
};

type Props = {
  /** The field's name for the harness: "answer" or "talk". */
  drive: string;
  /** This is where the learner writes now: the harness's "composer". */
  composer?: boolean;
  placeholder: string;
  initial: string;
  /** Called with the words when Return is pressed. Return true when they were taken, and the field empties. */
  onSubmit: (text: string) => boolean | Promise<boolean>;
  onDraft: (text: string) => void;
  /** Keys this place handles itself before the field does: Esc, Tab, the help keys. Return true when handled. */
  onKey?: (event: React.KeyboardEvent<HTMLTextAreaElement>, text: string) => boolean;
  onFocus?: () => void;
  className?: string;
  label: string;
  /** Show a preview of the mathematics typed, under the field. */
  preview?: boolean;
  disabled?: boolean;
};

function fit(field: HTMLTextAreaElement) {
  field.style.height = "auto";
  field.style.height = `${field.scrollHeight}px`;
}

export const Writer = memo(
  forwardRef<WriterHandle, Props>(function Writer(props, ref) {
    const field = useRef<HTMLTextAreaElement>(null);
    const draftTimer = useRef<number | null>(null);
    const previewTimer = useRef<number | null>(null);
    const [math, setMath] = useState<string | null>(null);
    const lastLength = useRef(0);
    const latest = useRef(props);
    latest.current = props;

    const schedule = useCallback((text: string) => {
      if (draftTimer.current !== null) window.clearTimeout(draftTimer.current);
      draftTimer.current = window.setTimeout(() => latest.current.onDraft(text), 300);
      if (!latest.current.preview) return;
      if (previewTimer.current !== null) window.clearTimeout(previewTimer.current);
      // The preview is drawn when typing pauses, so a keystroke never waits for it.
      previewTimer.current = window.setTimeout(() => setMath(text.includes("$") || text.includes("\\(") ? text : null), 180);
    }, []);

    useImperativeHandle(ref, () => ({
      focus: () => {
        const el = field.current;
        if (!el) return;
        el.focus({ preventScroll: false });
        const end = el.value.length;
        el.setSelectionRange(end, end);
      },
      insert: (text: string) => {
        const el = field.current;
        if (!el) return;
        el.focus();
        if (!document.execCommand("insertText", false, text)) {
          const start = el.selectionStart ?? el.value.length;
          const end = el.selectionEnd ?? el.value.length;
          el.value = el.value.slice(0, start) + text + el.value.slice(end);
          el.setSelectionRange(start + text.length, start + text.length);
          el.dispatchEvent(new Event("input", { bubbles: true }));
        }
      },
      clear: () => {
        const el = field.current;
        if (!el) return;
        el.value = "";
        fit(el);
        setMath(null);
        latest.current.onDraft("");
      },
      value: () => field.current?.value ?? "",
      element: () => field.current,
    }));

    useEffect(() => {
      const el = field.current;
      if (!el) return;
      el.value = props.initial;
      fit(el);
      if (props.preview && props.initial.includes("$")) setMath(props.initial);
      // Only the first draft is put in; later changes are the learner's own typing.
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);

    useEffect(
      () => () => {
        if (draftTimer.current !== null) {
          window.clearTimeout(draftTimer.current);
          latest.current.onDraft(field.current?.value ?? "");
        }
        if (previewTimer.current !== null) window.clearTimeout(previewTimer.current);
      },
      [],
    );

    const onKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
      const el = event.currentTarget;
      if (latest.current.onKey?.(event, el.value)) return;
      if (event.key === "Enter" && !event.shiftKey && !event.altKey && !event.nativeEvent.isComposing) {
        event.preventDefault();
        const text = el.value;
        void Promise.resolve(latest.current.onSubmit(text)).then((taken) => {
          if (!taken || !field.current) return;
          field.current.value = "";
          fit(field.current);
          setMath(null);
          if (draftTimer.current !== null) window.clearTimeout(draftTimer.current);
          latest.current.onDraft("");
        });
      }
    };

    return (
      <div className={`writer ${props.className ?? ""}`} data-drive={props.composer ? "composer" : undefined}>
        <textarea
          ref={field}
          rows={1}
          spellCheck
          data-drive={props.drive}
          aria-label={props.label}
          placeholder={props.placeholder}
          disabled={props.disabled}
          onKeyDown={onKeyDown}
          onFocus={props.onFocus}
          onInput={(event) => {
            const el = event.currentTarget;
            // Growing needs no full measure; only a shorter text can shrink the field.
            if (el.value.length < lastLength.current) fit(el);
            else if (el.scrollHeight > el.clientHeight) el.style.height = `${el.scrollHeight}px`;
            lastLength.current = el.value.length;
            schedule(el.value);
          }}
        />
        {math !== null && <div className="math-preview" aria-hidden="true" dangerouslySetInnerHTML={{ __html: renderPlain(math) }} />}
      </div>
    );
  }),
);
