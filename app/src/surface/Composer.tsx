// A line to write on. Where it sits says what it is:
//
// - "rest": under the next thing, before a lesson. Return starts it; words
//   typed first start it with those words.
// - "task": inside the open task, under its question. Return hands it in as
//   the answer (data-drive="answer").
// - "talk": the conversation's own line, after a `›`, always there. Whatever
//   is said on it is talk, which code never marks (data-drive="talk").
//
// While a task is open both are on the surface, the task's above the
// conversation's, like two lines of one document: ↓ at the end of the task's
// line goes down to the conversation's, ↑ at the start of the conversation's
// goes back up, and Esc steps out of the task. Each keeps its own words. A key
// pressed in one redraws that line and nothing else.
//
// The line takes files too: its quiet clip opens a picker for photos and
// PDFs, and a file dropped on the session or an image pasted in joins it.
// Each shows on the line as a small item that can be taken off, and goes with
// the next message. Under the task's line, a quiet row asks for help.

import { forwardRef, memo, useEffect, useImperativeHandle, useLayoutEffect, useRef, useState } from "react";

import { HELP, attachedWords } from "../lesson";
import { keepPlace } from "../prefs";

export type Place = "rest" | "task" | "talk";

export type Sending = { text: string; place: Place };

const CONFIDENCE = ["sure", "unsure", "guess"] as const;
type Confidence = (typeof CONFIDENCE)[number];

/** The symbols an answer can need, as the base window offered them. A `tex` one is set inside a formula. */
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
  { show: "∑", name: "sum", insert: "∑" },
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

/** Whether a key press is part of an input method still composing a character. */
const composing = (event: React.KeyboardEvent) => event.nativeEvent.isComposing || event.keyCode === 229;

/** Dollar signs and spaces alone are not something to send. */
const bare = (text: string) => /^[\s$]*$/.test(text);

/** A file's own name, without the folders it was kept in. */
export const fileName = (path: string) => path.split("/").pop() ?? path;

/** What a session can do with the line from outside it: a menu item, a drop, a pasted image. */
export type ComposerHandle = {
  focus: () => void;
  /** Open the picker for photos and PDFs. */
  pick: () => void;
  /** Files already handed in to the study folder, to go with the next message. */
  add: (saved: string[]) => void;
  toggleSymbols: () => void;
};

type Props = {
  place: Place;
  /** Where the line's draft is kept: the session's place. */
  placeKey: string | null;
  /** The words to start with, restored from where the learner left them. */
  initial: string;
  /** This line is where the learner is writing: it takes the focus. */
  active: boolean;
  /** The tutor is writing: Return waits for it, and the words go when it is done. */
  busy: boolean;
  /** A layer is over the surface, or the tab is not in front: the line does not take the focus. */
  paused: boolean;
  /** An open task exists that ↑ can go up into. */
  canAnswer: boolean;
  /** A task is open: a file sent now is the learner's work. */
  taskOpen: boolean;
  /** Words to show when the line is empty. */
  placeholder?: string;
  /** The quiet end of the line: the one key worth naming here, and what it does. */
  end?: { keys: string; what: string } | null;
  /** The key that stops the tutor's reply, named at the line's end while it writes. */
  stopKey?: string;
  onSend: (sending: Sending) => void;
  onMove: (place: "task" | "talk") => void;
  onNote: (text: string) => void;
  /** Hand files in to the study folder; resolves to their paths there. */
  onAttach: (paths: string[], working: boolean) => Promise<string[]>;
  /** Hand in a pasted image. */
  onPasteImage?: (name: string, bytes: Uint8Array, working: boolean) => Promise<string>;
  onPick: () => Promise<string[]>;
  onHelp?: (words: string) => void;
  onStop?: () => void;
  /** Return on an empty line: what the line's end names, when there is something to carry on. */
  onEmptyReturn?: () => void;
};

export const Composer = memo(
  forwardRef<ComposerHandle, Props>(function Composer(props, ref) {
    const [text, setText] = useState(props.initial);
    const [confidence, setConfidence] = useState<Confidence | null>(null);
    const [files, setFiles] = useState<string[]>([]);
    const [queued, setQueued] = useState(false);
    const [symbols, setSymbols] = useState(false);
    const box = useRef<HTMLTextAreaElement>(null);
    const length = useRef(props.initial.length);
    const caretAt = useRef<number | null>(null);
    const latest = useRef(props);
    latest.current = props;

    // The line grows with what is typed. It is measured only when it may have
    // to grow, or after text was taken away, so most keys cost one read.
    useLayoutEffect(() => {
      const el = box.current;
      if (!el) return;
      if (text.length < length.current || text === "") {
        el.style.height = "auto";
        el.style.height = `${el.scrollHeight}px`;
      } else if (el.scrollHeight > el.clientHeight) {
        el.style.height = `${el.scrollHeight}px`;
      }
      length.current = text.length;
    }, [text]);

    // A symbol put in moves the caret to where it should stand.
    useEffect(() => {
      if (caretAt.current === null || !box.current) return;
      box.current.setSelectionRange(caretAt.current, caretAt.current);
      caretAt.current = null;
      box.current.focus();
    });

    // The line where the learner is writing has the focus whenever nothing
    // else needs it: when it becomes theirs, when the tutor finishes, and when
    // a layer goes away.
    useEffect(() => {
      if (!props.active || props.paused) return;
      const el = box.current;
      if (!el) return;
      const active = document.activeElement;
      if (active && active !== document.body && active !== el && active.closest(".card, .help-row, .symbols, .attached")) return;
      el.focus({ preventScroll: true });
      el.setSelectionRange(el.value.length, el.value.length);
    }, [props.active, props.paused, props.busy]);

    const keep = (value: string) => {
      const { placeKey, place } = latest.current;
      if (!placeKey) return;
      keepPlace(placeKey, place === "rest" ? { draft: value } : { answerDraft: value });
    };

    const change = (value: string) => {
      setText(value);
      keep(value);
      if (bare(value)) setConfidence(null);
    };

    const content = !bare(text) || files.length > 0;

    const send = (force = false) => {
      const { place, busy, taskOpen } = latest.current;
      if (!content) {
        if (latest.current.onEmptyReturn) latest.current.onEmptyReturn();
        else if (place === "rest") latest.current.onSend({ text: "", place });
        return;
      }
      if (busy && !force) {
        setQueued(true);
        return;
      }
      const parts = [text.trim()];
      if (confidence && !bare(text)) parts.push(`Confidence: ${confidence}`);
      // A file sent with an answer, or while a task is open, is the learner's
      // work. Otherwise it goes as a file for the tutor to read, and the tutor
      // is told it may be either.
      if (files.length > 0) parts.push(attachedWords(files, place === "task" || taskOpen));
      latest.current.onSend({ text: parts.filter((part) => part !== "").join("\n\n"), place });
      setText("");
      setConfidence(null);
      setFiles([]);
      setQueued(false);
      setSymbols(false);
      length.current = 0;
      keep("");
    };

    // Waited for: the words go once the tutor is done.
    useEffect(() => {
      if (queued && !props.busy) send(true);
    });

    const add = (saved: string[]) => {
      if (saved.length > 0) setFiles((old) => [...old, ...saved.filter((path) => !old.includes(path))]);
    };

    // Photos and PDFs chosen are handed in to the study folder, then named in the message.
    const pick = async () => {
      try {
        const picked = await latest.current.onPick();
        const working = latest.current.place === "task" || latest.current.taskOpen;
        if (picked.length > 0) add(await latest.current.onAttach(picked, working));
      } catch (error) {
        latest.current.onNote(String(error));
      }
      box.current?.focus();
    };

    useImperativeHandle(ref, () => ({
      focus: () => box.current?.focus(),
      pick: () => void pick(),
      add,
      toggleSymbols: () => setSymbols((open) => !open),
    }));

    // An image pasted in joins the line as a file; pasted words stay a normal paste.
    const onPaste = (event: React.ClipboardEvent<HTMLTextAreaElement>) => {
      const images = Array.from(event.clipboardData.files).filter((file) => file.type.startsWith("image/") || file.type === "application/pdf");
      if (images.length === 0 || !latest.current.onPasteImage) return;
      if (event.clipboardData.getData("text/plain").trim() === "") event.preventDefault();
      const working = latest.current.place === "task" || latest.current.taskOpen;
      void (async () => {
        for (const file of images) {
          try {
            const bytes = new Uint8Array(await file.arrayBuffer());
            const name = file.name && file.name !== "image.png" ? file.name : `Pasted ${new Date().toTimeString().slice(0, 5).replace(":", ".")}.${file.type.split("/")[1] ?? "png"}`;
            add([await latest.current.onPasteImage!(name, bytes, working)]);
          } catch (error) {
            latest.current.onNote(String(error));
          }
        }
      })();
    };

    const insert = (symbol: (typeof SYMBOLS)[number]) => {
      const el = box.current;
      if (!el) return;
      const from = el.selectionStart;
      const to = el.selectionEnd;
      if (!symbol.tex) {
        caretAt.current = from + symbol.insert.length;
        change(`${text.slice(0, from)}${symbol.insert}${text.slice(to)}`);
        return;
      }
      const mark = symbol.insert.indexOf("|");
      const picked = text.slice(from, to);
      const snippet = `${symbol.insert.slice(0, mark)}${picked}${symbol.insert.slice(mark + 1)}`;
      const wrap = inMath(text.slice(0, from)) ? "" : "$";
      caretAt.current = from + wrap.length + (picked === "" ? mark : snippet.length - 1);
      change(`${text.slice(0, from)}${wrap}${snippet}${wrap}${text.slice(to)}`);
    };

    const onKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (composing(event)) return;
      const el = event.currentTarget;
      const { place, canAnswer } = latest.current;
      const plain = !event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey;
      if (event.key === "Enter" && !event.shiftKey && !event.metaKey && !event.ctrlKey && !event.altKey) {
        event.preventDefault();
        send();
        return;
      }
      // Esc does what the line's end says: in a task it steps out to talk,
      // in one press, whatever is on the line (which stays) and whether the
      // symbols are open (they close). While the tutor writes, Esc stops the
      // reply instead, and the window handles it.
      if (event.key === "Escape" && plain && !latest.current.busy && (place === "task" || symbols)) {
        event.preventDefault();
        event.stopPropagation();
        setSymbols(false);
        if (place === "task") latest.current.onMove("talk");
        return;
      }
      // Down to the conversation's line: ↓ at the end of the task's.
      if (place === "task" && plain && event.key === "ArrowDown" && el.selectionStart === el.value.length && el.selectionEnd === el.value.length) {
        event.preventDefault();
        latest.current.onMove("talk");
        return;
      }
      // Up into the open task: ↑ at the start of the conversation's line.
      if (place === "talk" && canAnswer && plain && event.key === "ArrowUp" && el.selectionStart === 0 && el.selectionEnd === 0) {
        event.preventDefault();
        latest.current.onMove("task");
        return;
      }
      // How sure, said with the answer: ⌥1, ⌥2, ⌥3. The key's code is read, for ⌥1 types a character of its own on a Mac.
      const digit = /^Digit([123])$/.exec(event.code);
      if (digit && place === "task" && event.altKey && !event.metaKey && !event.ctrlKey && !bare(text)) {
        event.preventDefault();
        const picked = CONFIDENCE[Number(digit[1]) - 1];
        setConfidence((old) => (old === picked ? null : picked));
      }
    };

    const { place } = props;
    const label = place === "task" ? "Your answer to the task" : place === "talk" ? "Say something to the tutor" : "Start, or say what to work on";
    const help = (words: string) => latest.current.onHelp?.(words);
    return (
      <div className={`line ${place}${props.active ? " on" : ""}${files.length > 0 ? " with-files" : ""}`} data-line={place}>
        <div className="field-row">
        <textarea
          ref={box}
          className="field"
          // One field for both, so it is the "composer"; its other role is one
          // element away: "talk" (its quiet end) under a task, "answer" (the open task) while talking.
          data-drive="composer"
          aria-label={label}
          rows={1}
          value={text}
          spellCheck={place !== "task"}
          placeholder={props.placeholder ?? ""}
          onChange={(event) => change(event.target.value)}
          onKeyDown={onKeyDown}
          onPaste={onPaste}
          onFocus={() => place !== "rest" && !latest.current.active && latest.current.onMove(place)}
        />
        <span className="line-tools">
          {/* The clip: photos and PDFs, with the next message. */}
          <button
            type="button"
            className="clip"
            data-drive="attach"
            aria-label="Attach a photo or PDF"
            title="Attach a photo or PDF"
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => void pick()}
          >
            <svg viewBox="0 0 16 16" width="15" height="15" aria-hidden="true">
              <path
                d="M10.6 4.2 5.3 9.5a1.6 1.6 0 0 0 2.3 2.3l5.6-5.6a2.9 2.9 0 0 0-4.1-4.1L3.4 7.8a4.1 4.1 0 0 0 5.8 5.8l4.6-4.6"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.25"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            </svg>
          </button>
          {/* The one line's quiet end. Under a task it is also the step out to talk
              (data-drive="talk"), so a harness or a click can say "talk" here. */}
          {props.busy && props.onStop ? (
            <button className="line-stop" onMouseDown={(event) => event.preventDefault()} onClick={() => latest.current.onStop?.()}>
              <span className="keys">{props.stopKey ?? "esc"}</span> stop the reply
            </button>
          ) : place === "task" ? (
            <button
              className="line-end"
              data-drive="talk"
              tabIndex={-1}
              onFocus={() => latest.current.onMove("talk")}
              onMouseDown={(event) => {
                event.preventDefault();
                latest.current.onMove("talk");
              }}
            >
              <span className="keys">esc</span> talk instead
            </button>
          ) : (
            props.end && (
              <span className="line-end" aria-hidden="true">
                <span className="keys">{props.end.keys}</span> {props.end.what}
              </span>
            )
          )}
        </span>
        </div>
        {/* Always there, so a file added by a drop or a paste is read out as it joins the line. */}
        <p className="attached" role="group" aria-label="Files that go with the next message" aria-live="polite">
          {files.map((file) => (
            <span key={file} className="file">
              {fileName(file)}
              <button type="button" className="take-off" aria-label={`Take ${fileName(file)} off`} title="Take it off" onClick={() => setFiles((old) => old.filter((each) => each !== file))}>
                ×
              </button>
            </span>
          ))}
        </p>
        {(confidence || queued) && (
          <p className="with">
            {confidence && <span className="said-sure">{confidence}</span>}
            {queued && <span className="queued">goes when the tutor finishes</span>}
          </p>
        )}
        {place === "task" && props.onHelp && (
          <p className="help-row" role="group" aria-label="Help with the task">
            <button className="quiet-act" data-drive="hint" disabled={props.busy} onMouseDown={(e) => e.preventDefault()} onClick={() => help(HELP.hint)}>
              Hint
            </button>
            <button className="quiet-act" data-drive="lost" disabled={props.busy} onMouseDown={(e) => e.preventDefault()} onClick={() => help(HELP.lost)}>
              I'm lost
            </button>
            <button className="quiet-act" data-drive="dont-know" disabled={props.busy} onMouseDown={(e) => e.preventDefault()} onClick={() => help(HELP.unknown)}>
              I don't know
            </button>
            <span className="help-gap" aria-hidden="true" />
            <button className="quiet-act" data-drive="photo" onMouseDown={(e) => e.preventDefault()} onClick={() => void pick()}>
              Photo
            </button>
            <button className="quiet-act" data-drive="symbols" aria-expanded={symbols} onMouseDown={(e) => e.preventDefault()} onClick={() => setSymbols((open) => !open)}>
              Symbols
            </button>
          </p>
        )}
        {symbols && (
          <p className="symbols" role="group" aria-label="Symbols">
            {SYMBOLS.map((symbol) => (
              <button key={symbol.name} className="symbol" title={symbol.name} aria-label={symbol.name} onMouseDown={(e) => e.preventDefault()} onClick={() => insert(symbol)}>
                {symbol.show}
              </button>
            ))}
          </p>
        )}
      </div>
    );
  }),
);
