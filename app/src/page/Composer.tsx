// A line the learner writes on. Where it stands says what it is: the line
// inside the open exercise takes answers to that task; the line on the page
// takes talk, which the record never marks. Esc moves between the two.
// Nothing on either is labelled "your answer".
//
// Each keeps its own text, so a keystroke redraws that line and nothing else.

import { memo, useEffect, useImperativeHandle, useMemo, useRef, useState, forwardRef } from "react";

import { HELP } from "../lesson";
import { hasMath, renderPlain } from "../render";

export type Place = "task" | "page";

const SURE: [string, string][] = [
  ["sure", "Sure"],
  ["unsure", "Unsure"],
  ["guess", "Guess"],
];

/** The characters an answer most often needs, and a fraction, which only LaTeX can write. */
const SYMBOLS: { show: string; name: string; insert: string; tex?: boolean }[] = [
  { show: "x̄", name: "x bar", insert: "x̄" },
  { show: "σ", name: "sigma", insert: "σ" },
  { show: "μ", name: "mu", insert: "μ" },
  { show: "√", name: "square root", insert: "√" },
  { show: "x₂", name: "subscript", insert: "_{|}", tex: true },
  { show: "x²", name: "superscript", insert: "^{|}", tex: true },
  { show: "≤", name: "less than or equal to", insert: "≤" },
  { show: "≥", name: "greater than or equal to", insert: "≥" },
  { show: "±", name: "plus or minus", insert: "±" },
  { show: "×", name: "times", insert: "×" },
  { show: "÷", name: "divided by", insert: "÷" },
  { show: "≠", name: "not equal to", insert: "≠" },
  { show: "≈", name: "approximately", insert: "≈" },
  { show: "∞", name: "infinity", insert: "∞" },
  { show: "a⁄b", name: "fraction", insert: "\\frac{|}{}", tex: true },
];

/** Whether the caret, after `before`, is inside a formula begun with a dollar sign. */
function inMath(before: string): boolean {
  let display = false;
  let inline = false;
  for (let i = 0; i < before.length; i++) {
    const c = before[i];
    if (c === "\\") i += 1;
    else if (c === "$" && before[i + 1] === "$" && !inline) {
      display = !display;
      i += 1;
    } else if (c === "$" && !display) inline = !inline;
  }
  return display || inline;
}

const composing = (event: React.KeyboardEvent) => event.nativeEvent.isComposing || event.keyCode === 229;

export type ComposerHandle = { focus: () => void; setText: (text: string) => void; text: () => string };

type Props = {
  /** "task": the line inside the open exercise. "page": the line on the page. */
  place: Place;
  /** The other line is there, so Esc can move to it. */
  canMove: boolean;
  onMove: () => void;
  onFocus: () => void;
  /** The tutor is writing: a message waits until it is done. */
  busy: boolean;
  /** Writing is shut, and why. */
  shut: string | null;
  placeholder: string;
  /** Photo and symbols: offered in a lesson. */
  tools: boolean;
  /** A quiet line under the box: what Esc does, or how to start. */
  hint: string | null;
  draft: string;
  onDraft: (text: string) => void;
  onSend: (text: string, place: Place) => void;
  /** Return on an empty line, when the page offers something to start. */
  onEmptyReturn?: () => void;
  onHelp: (words: string) => void;
  onPhoto: () => Promise<string[]>;
  onNote: (text: string) => void;
};

export const Composer = memo(
  forwardRef<ComposerHandle, Props>(function Composer(props, ref) {
    const [text, setText] = useState(props.draft);
    const [sure, setSure] = useState<string | null>(null);
    const [files, setFiles] = useState<string[]>([]);
    const [symbols, setSymbols] = useState(false);
    const [queued, setQueued] = useState(false);
    const box = useRef<HTMLTextAreaElement>(null);
    const caretAt = useRef<number | null>(null);
    const inTask = props.place === "task";

    useImperativeHandle(ref, () => ({
      focus: () => box.current?.focus(),
      setText: (value: string) => setText(value),
      text: () => box.current?.value ?? "",
    }));

    useEffect(() => {
      props.onDraft(text);
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [text]);

    // A symbol put in moves the caret to where it should stand.
    useEffect(() => {
      if (caretAt.current === null || !box.current) return;
      box.current.setSelectionRange(caretAt.current, caretAt.current);
      caretAt.current = null;
      box.current.focus();
    });

    const typed = text.trim().replace(/\$/g, "") !== "";
    const content = typed || files.length > 0;

    const send = () => {
      if (props.shut) return;
      if (!content) {
        props.onEmptyReturn?.();
        return;
      }
      if (props.busy) {
        setQueued(true);
        return;
      }
      const parts = [text.trim()];
      if (sure && typed && inTask) parts.push(`Confidence: ${sure}`);
      if (files.length > 0) parts.push(`${files.length === 1 ? "Photo" : "Photos"} of my working: ${files.join(", ")}`);
      props.onSend(parts.filter((part) => part !== "").join("\n\n"), props.place);
      setText("");
      setSure(null);
      setFiles([]);
      setQueued(false);
      setSymbols(false);
    };

    // A message written while the tutor was writing goes as soon as it is done.
    useEffect(() => {
      if (queued && !props.busy && !props.shut) send();
    });

    const insert = (symbol: (typeof SYMBOLS)[number]) => {
      const el = box.current;
      if (!el) return;
      const from = el.selectionStart;
      const to = el.selectionEnd;
      if (!symbol.tex) {
        caretAt.current = from + symbol.insert.length;
        setText(`${text.slice(0, from)}${symbol.insert}${text.slice(to)}`);
        return;
      }
      const mark = symbol.insert.indexOf("|");
      const picked = text.slice(from, to);
      const snippet = `${symbol.insert.slice(0, mark)}${picked}${symbol.insert.slice(mark + 1)}`;
      const wrap = inMath(text.slice(0, from)) ? "" : "$";
      caretAt.current = from + wrap.length + (picked === "" ? mark : snippet.length - 1);
      setText(`${text.slice(0, from)}${wrap}${snippet}${wrap}${text.slice(to)}`);
    };

    const photo = async () => {
      try {
        const saved = await props.onPhoto();
        if (saved.length > 0) setFiles((old) => [...old, ...saved]);
      } catch (error) {
        props.onNote(String(error));
      }
      box.current?.focus();
    };

    const preview = useMemo(() => (text.includes("$") && hasMath(text) ? renderPlain(text) : null), [text]);

    const onKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (composing(event)) return;
      if (event.key === "Enter" && !event.shiftKey && !event.altKey && !event.metaKey && !event.ctrlKey) {
        event.preventDefault();
        send();
        return;
      }
      if (event.key === "Escape") {
        if (symbols) {
          event.preventDefault();
          setSymbols(false);
          return;
        }
        if (props.canMove) {
          event.preventDefault();
          props.onMove();
        }
        return;
      }
      if (event.metaKey && event.shiftKey && !event.altKey && !event.ctrlKey && props.tools) {
        if (event.code === "KeyA") {
          event.preventDefault();
          void photo();
          return;
        }
        if (event.code === "KeyS") {
          event.preventDefault();
          setSymbols((open) => !open);
          return;
        }
      }
      const pick = /^Digit([123])$/.exec(event.code);
      if (pick && inTask && event.altKey && !event.metaKey && !event.ctrlKey && !event.shiftKey) {
        event.preventDefault();
        const word = SURE[Number(pick[1]) - 1][0];
        setSure((old) => (old === word ? null : word));
      }
    };

    return (
      <div className={inTask ? "composer in-task" : "composer"} data-place={props.place}>
        <div className="field" data-value={`${text} `}>
          <textarea
            ref={box}
            data-drive={inTask ? "answer" : "talk"}
            rows={1}
            value={text}
            spellCheck
            disabled={props.shut !== null}
            aria-label={inTask ? "Answer to the open task" : "Write to the tutor"}
            placeholder={props.shut ?? props.placeholder}
            onChange={(event) => setText(event.target.value)}
            onKeyDown={onKeyDown}
            onFocus={props.onFocus}
          />
        </div>
        {preview && <div className="preview" aria-hidden="true" dangerouslySetInnerHTML={{ __html: preview }} />}
        {files.length > 0 && (
          <div className="attached">
            {files.map((file) => (
              <span key={file} className="file">
                {file.split("/").pop()}
                <button className="text-btn" aria-label={`Take off ${file.split("/").pop()}`} onClick={() => setFiles((old) => old.filter((f) => f !== file))}>
                  ×
                </button>
              </span>
            ))}
          </div>
        )}
        {symbols && (
          <div className="symbols" role="toolbar" aria-label="Symbols">
            {SYMBOLS.map((symbol) => (
              <button key={symbol.name} className="symbol" title={symbol.name} aria-label={symbol.name} onMouseDown={(e) => e.preventDefault()} onClick={() => insert(symbol)}>
                {symbol.show}
              </button>
            ))}
          </div>
        )}
        <div className="foot">
          {inTask && !props.shut && (
            <span className="help">
              <button className="text-btn" disabled={props.busy} onClick={() => props.onHelp(HELP.hint)} title="⌘⇧H">
                Hint
              </button>
              <button className="text-btn" disabled={props.busy} onClick={() => props.onHelp(HELP.lost)} title="⌘⇧L">
                I'm lost
              </button>
              <button className="text-btn" disabled={props.busy} onClick={() => props.onHelp(HELP.unknown)} title="⌘⇧D">
                I don't know
              </button>
              {typed && (
                <span className="sure" role="radiogroup" aria-label="How sure">
                  {SURE.map(([word, label], i) => (
                    <button
                      key={word}
                      role="radio"
                      aria-checked={sure === word}
                      className={sure === word ? "text-btn chosen" : "text-btn"}
                      title={`⌥${i + 1}`}
                      onClick={() => setSure((old) => (old === word ? null : word))}
                    >
                      {label}
                    </button>
                  ))}
                </span>
              )}
            </span>
          )}
          {!props.shut && props.tools && (
            <span className="tools">
              <button className="text-btn" onClick={() => void photo()} title="⌘⇧A">
                Photo
              </button>
              <button className="text-btn" onClick={() => setSymbols((open) => !open)} title="⌘⇧S" aria-expanded={symbols}>
                Symbols
              </button>
            </span>
          )}
          {queued ? <span className="hint">Sends when the tutor finishes</span> : props.hint && <span className="hint">{props.hint}</span>}
        </div>
      </div>
    );
  }),
);
