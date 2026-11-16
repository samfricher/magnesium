// A line to write on. Where it sits says what it is:
//
// - "rest": under the next thing, before a lesson. Return starts it; words
//   typed first start it with those words.
// - "lesson": the one line of a lesson. Every message goes the same way, and
//   the record decides whether it was an answer. Two ways of sending say
//   "this is not an answer" for sure: ⌥↩, and a message that quotes a passage
//   the learner had selected. Both go as talk, which code never marks.
//
// Both are the same line, drawn the same way: one row, the learner's words,
// then two quiet words at its end. More opens a short menu over the line.
// Stuck, in a lesson, opens its three rows above the words, in the line's own
// flow, so the question the learner is stuck on stays in view. Before a lesson
// there is nothing to be stuck on, and the line ends in More alone.
//
// The line takes files too: Attach opens a picker for photos and PDFs, and a
// file dropped on the session or an image pasted in joins it. Each shows on
// the line by its name, can be taken off, and goes with the next message.
//
// The session adds its own rows under More, and what a row opens sits beside
// the line.

import { forwardRef, memo, useEffect, useImperativeHandle, useLayoutEffect, useRef, useState } from "react";
import type { ReactNode } from "react";

import { HELP, attachedWords, quotable, withQuote } from "../lesson";
import { keepPlace, type KeyAction } from "../prefs";
import { LineMenu, type LineRow } from "./LineMenu";

export type Place = "rest" | "lesson";

/** `talk` is set when the learner said for sure it is not an answer: ⌥↩, or a quoted passage. */
export type Sending = { text: string; place: Place; talk: boolean };

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

/** What a session can do with the line from outside it: a menu item, a drop, a pasted image, a key typed on the page. */
export type ComposerHandle = {
  focus: () => void;
  /** Open the picker for photos and PDFs. */
  pick: () => void;
  /** Files already handed in to the study folder, to go with the next message. */
  add: (saved: string[]) => void;
  toggleSymbols: () => void;
  /** Words typed while the keyboard was elsewhere, and the passage selected when they were: the line takes both. */
  write: (chars: string, selected?: string) => void;
  /** Whether the line holds anything to send, or a passage to quote. */
  holds: () => boolean;
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
  /** A task is open: Stuck is offered, ⌥↩ means something, and a file sent now is the learner's work. */
  taskOpen: boolean;
  /** No task the page can see, but the tutor may have set one it did not save: a message may still be marked. */
  taskUnseen?: boolean;
  /** Words to show when the line is empty. */
  placeholder?: string;
  /** The quiet end of the line at rest: the one key worth naming there, and what it does. */
  end?: { keys: string; what: string } | null;
  /** What a session's row under More opens, placed beside the line. */
  toolBeside?: ReactNode;
  /** The session's rows under More, after the line's own. */
  more?: LineRow[];
  /** What a session's row opened is open: More reads as open while it is. */
  moreHeld?: boolean;
  /** The key bound to an action, as the window names it; empty when none is. */
  keyFor?: (action: KeyAction) => string;
  /** The key that stops the tutor's reply, named on the line while it writes. */
  stopKey?: string;
  onSend: (sending: Sending) => void;
  onNote: (text: string) => void;
  /** Hand files in to the study folder; resolves to their paths there. */
  onAttach: (paths: string[], working: boolean) => Promise<string[]>;
  /** Hand in a pasted image. */
  onPasteImage?: (name: string, bytes: Uint8Array, working: boolean) => Promise<string>;
  onPick: () => Promise<string[]>;
  onHelp?: (words: string) => void;
  onStop?: () => void;
  /** What stopping does now: "stop the reply", or "stop the tutor" while it waits on a card. */
  stopWords?: string;
  /** A card waits on the learner: the line can say how the keyboard reaches it. */
  cardWaits?: boolean;
  /** Return on an empty line: what the line's end names, when there is something to carry on. */
  onEmptyReturn?: () => void;
  /** Words were typed, a file joined or a tool was used: the learner is writing now. */
  onWrite?: () => void;
  /** Esc on the line: the learner is done writing for now. The words stay. */
  onPutAway?: () => void;
  /** The keyboard left the line, and it holds nothing. */
  onLeftEmpty?: () => void;
};

export const Composer = memo(
  forwardRef<ComposerHandle, Props>(function Composer(props, ref) {
    const [text, setText] = useState(props.initial);
    const [quote, setQuote] = useState<string | null>(null);
    const [confidence, setConfidence] = useState<Confidence | null>(null);
    const [files, setFiles] = useState<string[]>([]);
    const [queued, setQueued] = useState<null | { talk: boolean }>(null);
    const [symbols, setSymbols] = useState(false);
    // Stuck opens a row above the words; More, a short menu over the line.
    const [menu, setMenu] = useState<"stuck" | "more" | null>(null);
    const stuckRef = useRef<HTMLButtonElement>(null);
    const moreRef = useRef<HTMLButtonElement>(null);
    const box = useRef<HTMLTextAreaElement>(null);
    const length = useRef(props.initial.length);
    const caretAt = useRef<number | null>(null);
    const latest = useRef(props);
    latest.current = props;
    const held = useRef({ text, quote, files });
    held.current = { text, quote, files };

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

    // A symbol put in, or words typed from the page, move the caret to where it should stand.
    useEffect(() => {
      if (caretAt.current === null || !box.current) return;
      box.current.focus({ preventScroll: true });
      box.current.setSelectionRange(caretAt.current, caretAt.current);
      caretAt.current = null;
    });

    // The line where the learner is writing has the focus whenever nothing
    // else needs it: when it becomes theirs, when the tutor finishes, and when
    // a layer goes away.
    useEffect(() => {
      if (!props.active || props.paused) return;
      const el = box.current;
      if (!el) return;
      const active = document.activeElement;
      // Not from a card, the line's own controls or menus, the model list, the symbols, a file on the line, or a document being read beside the lesson.
      if (active && active !== document.body && active !== el && active.closest(".card, .line-tools, .line-menu, .model-list, .symbols, .attached, .panel.side")) return;
      el.focus({ preventScroll: true });
      el.setSelectionRange(el.value.length, el.value.length);
    }, [props.active, props.paused, props.busy]);

    const keep = (value: string) => {
      const { placeKey, place } = latest.current;
      if (!placeKey) return;
      // One line now: words an older window kept on the task's own line are this line's draft once anything is typed.
      keepPlace(placeKey, place === "rest" ? { draft: value } : { draft: value, answerDraft: "" });
    };

    const change = (value: string) => {
      setText(value);
      keep(value);
      if (bare(value)) setConfidence(null);
      if (latest.current.place === "lesson" && value !== "") latest.current.onWrite?.();
    };

    const typed = !bare(text);
    const content = typed || files.length > 0;

    const send = (talk: boolean, force = false) => {
      const { place, busy, taskOpen } = latest.current;
      if (!content) {
        if (latest.current.onEmptyReturn) latest.current.onEmptyReturn();
        else if (place === "rest") latest.current.onSend({ text: "", place, talk: false });
        return;
      }
      if (busy && !force) {
        setQueued({ talk });
        return;
      }
      // A quoted passage is about the passage: never an answer.
      const asTalk = talk || quote !== null;
      const parts = [quote !== null ? withQuote(quote, text.trim()) : text.trim()];
      if (confidence && typed && !asTalk) parts.push(`Confidence: ${confidence}`);
      // A file sent while a task is open is the learner's work, unless they said the message is not an answer.
      // Otherwise it goes as a file for the tutor to read, and the tutor is told it may be either.
      if (files.length > 0) parts.push(attachedWords(files, place === "lesson" && taskOpen && !asTalk));
      latest.current.onSend({ text: parts.filter((part) => part !== "").join("\n\n"), place, talk: asTalk });
      setText("");
      setQuote(null);
      setConfidence(null);
      setFiles([]);
      setQueued(null);
      setSymbols(false);
      setMenu(null);
      length.current = 0;
      keep("");
    };

    // Waited for: the words go once the tutor is done.
    useEffect(() => {
      if (queued && !props.busy) send(queued.talk, true);
    });

    const add = (saved: string[]) => {
      if (saved.length === 0) return;
      setFiles((old) => [...old, ...saved.filter((path) => !old.includes(path))]);
      latest.current.onWrite?.();
    };

    const working = () => latest.current.place === "lesson" && latest.current.taskOpen && held.current.quote === null;

    // Photos and PDFs chosen are handed in to the study folder, then named in the message.
    const pick = async () => {
      latest.current.onWrite?.();
      try {
        const picked = await latest.current.onPick();
        if (picked.length > 0) add(await latest.current.onAttach(picked, working()));
      } catch (error) {
        latest.current.onNote(String(error));
      }
      box.current?.focus({ preventScroll: true });
    };

    useImperativeHandle(ref, () => ({
      focus: () => box.current?.focus({ preventScroll: true }),
      pick: () => void pick(),
      add,
      toggleSymbols: () => {
        latest.current.onWrite?.();
        setSymbols((open) => !open);
      },
      write: (chars, selected) => {
        const passage = selected ? quotable(selected) : "";
        if (passage !== "") setQuote(passage);
        const next = held.current.text + chars;
        caretAt.current = next.length;
        if (chars !== "") change(next);
        else box.current?.focus({ preventScroll: true });
        latest.current.onWrite?.();
      },
      holds: () => !bare(held.current.text) || held.current.files.length > 0 || held.current.quote !== null,
    }));

    // An image pasted in joins the line as a file; pasted words stay a normal paste.
    const onPaste = (event: React.ClipboardEvent<HTMLTextAreaElement>) => {
      const images = Array.from(event.clipboardData.files).filter((file) => file.type.startsWith("image/") || file.type === "application/pdf");
      if (images.length === 0 || !latest.current.onPasteImage) return;
      if (event.clipboardData.getData("text/plain").trim() === "") event.preventDefault();
      void (async () => {
        for (const file of images) {
          try {
            const bytes = new Uint8Array(await file.arrayBuffer());
            const name = file.name && file.name !== "image.png" ? file.name : `Pasted ${new Date().toTimeString().slice(0, 5).replace(":", ".")}.${file.type.split("/")[1] ?? "png"}`;
            add([await latest.current.onPasteImage!(name, bytes, working())]);
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
      const { place, taskOpen, taskUnseen } = latest.current;
      const plain = !event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey;
      if (event.key === "Enter" && !event.shiftKey && !event.metaKey && !event.ctrlKey) {
        event.preventDefault();
        // ⌥↩ sends the words as talk: not an answer, whatever they say. It means something only while a message
        // could be marked: a task is open, or the tutor may have set one the page cannot see.
        send(place === "lesson" && event.altKey && (taskOpen || taskUnseen === true));
        return;
      }
      // While a card waits above the line, ⇧⇥ goes to its first choice. In the page's order the nearest choice is the
      // card's last, the standing grant, which is the one a learner should never land on by going back.
      if (event.key === "Tab" && event.shiftKey && !event.metaKey && !event.ctrlKey && !event.altKey && el.closest(".foot.asking")) {
        const choice = el.closest(".session")?.querySelector<HTMLElement>(".card .act");
        if (choice) {
          event.preventDefault();
          choice.focus();
          return;
        }
      }
      // Esc closes what is open on the line, and then puts the line away with its words kept.
      // While the tutor writes, Esc stops the reply instead, and the window handles it.
      if (event.key === "Escape" && plain && !latest.current.busy && (symbols || menu || place === "lesson")) {
        event.preventDefault();
        event.stopPropagation();
        if (symbols || menu) {
          setSymbols(false);
          setMenu(null);
          return;
        }
        el.blur();
        latest.current.onPutAway?.();
        return;
      }
      // ⌫ on an empty line takes the quoted passage off.
      if (event.key === "Backspace" && plain && el.value === "" && quote !== null) {
        event.preventDefault();
        setQuote(null);
        return;
      }
      // How sure, said with the answer: ⌥1, ⌥2, ⌥3. The key's code is read, for ⌥1 types a character of its own on a Mac.
      const digit = /^Digit([123])$/.exec(event.code);
      if (digit && place === "lesson" && taskOpen && quote === null && event.altKey && !event.metaKey && !event.ctrlKey && !bare(text)) {
        event.preventDefault();
        const picked = CONFIDENCE[Number(digit[1]) - 1];
        setConfidence((old) => (old === picked ? null : picked));
      }
    };

    const onBlur = (event: React.FocusEvent<HTMLTextAreaElement>) => {
      // The keyboard went to one of the line's own controls, or to the Mac (a picker): the line is still in use.
      const to = event.relatedTarget as HTMLElement | null;
      if (!to || to.closest(".line")) return;
      if (!(!bare(held.current.text) || held.current.files.length > 0 || held.current.quote !== null)) latest.current.onLeftEmpty?.();
    };

    const { place } = props;
    const help = (words: string) => {
      setMenu(null);
      latest.current.onHelp?.(words);
    };
    const keep_ = (event: React.MouseEvent) => event.preventDefault();

    const key = (action: KeyAction) => props.keyFor?.(action) || undefined;
    const closeMenu = (back: boolean) => {
      setMenu(null);
      if (back) box.current?.focus({ preventScroll: true });
    };
    const rest = place === "rest";
    // Help is about the lesson's work: its rows wait while the tutor writes.
    const noHelp = props.busy;
    const stuckRows: LineRow[] = [
      { id: "hint", label: "Hint", keys: key("hint"), group: "help", disabled: noHelp, run: () => help(HELP.hint) },
      { id: "lost", label: "I’m lost", keys: key("lost"), group: "help", disabled: noHelp, run: () => help(HELP.lost) },
      { id: "dont-know", label: "I don’t know", keys: key("unknown"), group: "help", disabled: noHelp, run: () => help(HELP.unknown) },
    ];
    const moreRows: LineRow[] = [
      // Said for sure to be no answer: only while a message could be marked, words are typed, and no passage is quoted (which is never an answer).
      ...((props.taskOpen || props.taskUnseen) && typed && quote === null
        ? [{ id: "send-talk", label: "Send without a mark", keys: "⌥↩", group: "send", run: () => send(true) }]
        : []),
      { id: "attach", label: "Attach a photo or PDF", keys: key("attach"), group: "line", run: () => (setMenu(null), void pick()) },
      { id: "symbols", label: symbols ? "Hide symbols" : "Symbols", keys: key("symbols"), group: "line", run: () => (closeMenu(true), setSymbols((open) => !open)) },
      ...(props.more ?? []).map((row) => ({ ...row, run: () => (setMenu(null), row.run()) })),
    ];
    const word = (which: "stuck" | "more", label: string, ref: React.RefObject<HTMLButtonElement>, held = false) => (
      <button
        key={which}
        ref={ref}
        type="button"
        className={`quiet-act${menu === which || held ? " open" : ""}`}
        data-drive={which}
        aria-haspopup="menu"
        aria-expanded={menu === which}
        onMouseDown={keep_}
        onClick={() => setMenu((open) => (open === which ? null : which))}
      >
        {label}
      </button>
    );
    const tools: React.ReactNode[] = [];
    if (props.busy && props.onStop)
      tools.push(
        <button key="stop" type="button" className="quiet-act" data-drive="stop" onMouseDown={keep_} onClick={() => latest.current.onStop?.()}>
          <span className="keys">{props.stopKey ?? "esc"}</span> {props.stopWords ?? "stop the reply"}
        </button>,
      );
    // Before a lesson, with words on the line, its end says what Return does with them.
    if (rest && text.trim() !== "" && props.end)
      tools.push(
        <span key="end" className="line-end" aria-hidden="true">
          <span className="keys">{props.end.keys}</span> {props.end.what}
        </span>,
      );
    if (props.onHelp && !rest) tools.push(word("stuck", "Stuck", stuckRef));
    tools.push(word("more", "More", moreRef, props.moreHeld));

    return (
      <div className={`line lesson${rest ? " first" : ""}${props.active ? " on" : ""}${typed ? " typed" : ""}`} data-line={place}>
        {menu === "stuck" && <LineMenu label="Stuck" rows={stuckRows} opener={stuckRef} onClose={closeMenu} inline />}
        {quote !== null && (
          <p className="quote" title="⌫ on an empty line takes it off">
            “{quote}”
          </p>
        )}
        <div className="field-row">
          {/* Always there, so a file added by a drop or a paste is read out as it joins the line. */}
          <span className="attached" role="group" aria-label="Files that go with the next message" aria-live="polite">
            {files.map((file) => (
              <span key={file} className="file">
                {fileName(file)}
                <button type="button" className="take-off" aria-label={`Take ${fileName(file)} off`} title="Take it off" onClick={() => setFiles((old) => old.filter((each) => each !== file))}>
                  ×
                </button>
              </span>
            ))}
          </span>
          <textarea
            ref={box}
            className="field"
            data-drive="composer"
            aria-label={rest ? "Start, or say what to work on" : quote !== null ? "Say something about the passage" : props.taskOpen ? "Your answer, or anything else to say" : "Say something to the tutor"}
            rows={1}
            value={text}
            spellCheck
            placeholder={props.placeholder ?? ""}
            onChange={(event) => change(event.target.value)}
            onKeyDown={onKeyDown}
            onPaste={onPaste}
            onBlur={onBlur}
          />
          <span className="line-tools" role="group" aria-label="The line's tools">
            {tools}
          </span>
        </div>
        {menu === "more" && <LineMenu label="More" rows={moreRows} opener={moreRef} onClose={closeMenu} />}
        {props.toolBeside}
        {(confidence || queued) && (
          <p className="with">
            {confidence && <span className="said-sure">{confidence}</span>}
            {queued && <span className="queued">goes when the tutor finishes</span>}
          </p>
        )}
        {/* How the keyboard reaches a card that arrived while the learner was typing. Under the words, so the line
            keeps its width at 720 by 560; shown only while a card waits and the keyboard is on this line (surface.css,
            by the foot's "asking"). It is in the page only while a card waits: the Mac's WebKit kept it drawn after
            the foot lost "asking", until something else made it work the styles out again. */}
        {!rest && props.cardWaits && (
          <p className="with to-card" aria-hidden="true">
            <span className="keys">⇧⇥</span> answer the tutor
          </p>
        )}
        {symbols && (
          <p className="symbols" role="group" aria-label="Symbols">
            {SYMBOLS.map((symbol) => (
              <button key={symbol.name} className="symbol" title={symbol.name} aria-label={symbol.name} onMouseDown={keep_} onClick={() => insert(symbol)}>
                {/* The system face sets a combining bar off its x: the button draws the bar itself. */}
                {symbol.show === "x̄" ? <span className="overline">x</span> : symbol.show}
              </button>
            ))}
          </p>
        )}
      </div>
    );
  }),
);
