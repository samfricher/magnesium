import { forwardRef, memo, useEffect, useImperativeHandle, useLayoutEffect, useRef, useState } from "react";
import type { ReactNode } from "react";

import { HELP, attachedWords, quotable, withQuote } from "../lesson";
import { keepPlace, type KeyAction } from "../prefs";
import { renderPlain } from "../render";
import { selecting } from "../selection";
import { LineMenu, type LineRow } from "./LineMenu";

export type Place = "rest" | "lesson";

export type Sending = { text: string; place: Place; talk: boolean };

const CONFIDENCE = ["sure", "unsure", "guess"] as const;
type Confidence = (typeof CONFIDENCE)[number];

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

const bare = (text: string) => /^[\s$]*$/.test(text);

export type Passage = { text: string; fromTask: boolean };

export const fileName = (path: string) => path.split("/").pop() ?? path;

export type ComposerHandle = {
  focus: () => void;
  pick: () => void;
  add: (saved: string[]) => void;
  toggleSymbols: () => void;
  write: (chars: string, passage?: Passage) => void;
  holds: () => boolean;
};

type Props = {
  place: Place;
  placeKey: string | null;
  initial: string;
  active: boolean;
  busy: boolean;
  paused: boolean;
  taskOpen: boolean;
  taskUnseen?: boolean;
  placeholder?: string;
  end?: { keys: string; what: string } | null;
  toolBeside?: ReactNode;
  more?: LineRow[];
  moreHeld?: boolean;
  keyFor?: (action: KeyAction) => string;
  stopKey?: string;
  onSend: (sending: Sending) => void;
  onNote: (text: string) => void;
  onAttach: (paths: string[], working: boolean) => Promise<string[]>;
  onPasteImage?: (name: string, bytes: Uint8Array, working: boolean) => Promise<string>;
  onPick: () => Promise<string[]>;
  onHelp?: (words: string) => void;
  onStop?: () => void;
  stopWords?: string;
  cardWaits?: boolean;
  onEmptyReturn?: () => void;
  onWrite?: (answering: boolean) => void;
  onPutAway?: () => void;
  onLeftEmpty?: () => void;
};

export const Composer = memo(
  forwardRef<ComposerHandle, Props>(function Composer(props, ref) {
    const [text, setText] = useState(props.initial);
    const [quote, setQuote] = useState<Passage | null>(null);
    const [confidence, setConfidence] = useState<Confidence | null>(null);
    const [files, setFiles] = useState<string[]>([]);
    const [queued, setQueued] = useState<null | { talk: boolean }>(null);
    const [symbols, setSymbols] = useState(false);
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
    const answering = () => held.current.quote === null || held.current.quote.fromTask;

    useLayoutEffect(() => {
      const el = box.current;
      if (!el) return;
      if (text.length < length.current || text === "") {
        const row = el.parentElement;
        if (row) row.style.minHeight = `${row.offsetHeight}px`;
        el.style.height = "auto";
        el.style.height = `${el.scrollHeight}px`;
        if (row) row.style.minHeight = "";
      } else if (el.scrollHeight > el.clientHeight) {
        el.style.height = `${el.scrollHeight}px`;
      }
      length.current = text.length;
    }, [text]);

    useEffect(() => {
      if (caretAt.current === null || !box.current) return;
      box.current.focus({ preventScroll: true });
      box.current.setSelectionRange(caretAt.current, caretAt.current);
      caretAt.current = null;
    });

    useEffect(() => {
      if (!props.active || props.paused) return;
      const el = box.current;
      if (!el) return;
      const active = document.activeElement;
      if (active === el) return;
      if (active && active !== document.body && active.closest(".card, .line-tools, .line-menu, .model-list, .symbols, .attached, .panel.side")) return;
      if (selecting()) return;
      el.focus({ preventScroll: true });
      el.setSelectionRange(el.value.length, el.value.length);
    }, [props.active, props.paused, props.busy]);

    const keep = (value: string) => {
      const { placeKey, place } = latest.current;
      if (!placeKey) return;
      keepPlace(placeKey, place === "rest" ? { draft: value } : { draft: value, answerDraft: "" });
    };

    const change = (value: string) => {
      setText(value);
      keep(value);
      if (bare(value)) setConfidence(null);
      if (latest.current.place === "lesson" && value !== "") latest.current.onWrite?.(answering());
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
      const asTalk = talk || quote !== null;
      const parts = [quote !== null ? withQuote(quote.text, text.trim()) : text.trim()];
      if (confidence && typed && !asTalk) parts.push(`Confidence: ${confidence}`);
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

    useEffect(() => {
      if (queued && !props.busy) send(queued.talk, true);
    });

    const add = (saved: string[]) => {
      if (saved.length === 0) return;
      setFiles((old) => [...old, ...saved.filter((path) => !old.includes(path))]);
      latest.current.onWrite?.(answering());
    };

    const working = () => latest.current.place === "lesson" && latest.current.taskOpen && held.current.quote === null;

    const pick = async () => {
      latest.current.onWrite?.(answering());
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
        latest.current.onWrite?.(answering());
        setSymbols((open) => !open);
      },
      write: (chars, passage) => {
        const words = passage ? quotable(passage.text) : "";
        if (passage && words !== "") {
          const quoted = { text: words, fromTask: passage.fromTask };
          setQuote(quoted);
          held.current = { ...held.current, quote: quoted };
        }
        const next = held.current.text + chars;
        caretAt.current = next.length;
        if (chars !== "") change(next);
        else box.current?.focus({ preventScroll: true });
        latest.current.onWrite?.(answering());
      },
      holds: () => !bare(held.current.text) || held.current.files.length > 0 || held.current.quote !== null,
    }));

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
        send(place === "lesson" && event.altKey && (taskOpen || taskUnseen === true));
        return;
      }
      if (event.key === "Tab" && event.shiftKey && !event.metaKey && !event.ctrlKey && !event.altKey && el.closest(".foot.asking")) {
        const choice = el.closest(".session")?.querySelector<HTMLElement>(".card .act");
        if (choice) {
          event.preventDefault();
          choice.focus();
          return;
        }
      }
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
      if (event.key === "Backspace" && plain && el.value === "" && quote !== null) {
        event.preventDefault();
        setQuote(null);
        return;
      }
      const digit = /^Digit([123])$/.exec(event.code);
      if (digit && place === "lesson" && taskOpen && quote === null && event.altKey && !event.metaKey && !event.ctrlKey && !bare(text)) {
        event.preventDefault();
        const picked = CONFIDENCE[Number(digit[1]) - 1];
        setConfidence((old) => (old === picked ? null : picked));
      }
    };

    const onBlur = (event: React.FocusEvent<HTMLTextAreaElement>) => {
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
    const noHelp = props.busy;
    const stuckRows: LineRow[] = [
      { id: "hint", label: "Hint", keys: key("hint"), group: "help", disabled: noHelp, run: () => help(HELP.hint) },
      { id: "lost", label: "I’m lost", keys: key("lost"), group: "help", disabled: noHelp, run: () => help(HELP.lost) },
      { id: "dont-know", label: "I don’t know", keys: key("unknown"), group: "help", disabled: noHelp, run: () => help(HELP.unknown) },
    ];
    const moreRows: LineRow[] = [
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
          <p className="quote" title="⌫ on an empty line takes it off" dangerouslySetInnerHTML={{ __html: `“${renderPlain(quote.text)}”` }} />
        )}
        <div className="field-row">
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
        {!rest && props.cardWaits && (
          <p className="with to-card" aria-hidden="true">
            <span className="keys">⇧⇥</span> answer the tutor
          </p>
        )}
        {symbols && (
          <p className="symbols" role="group" aria-label="Symbols">
            {SYMBOLS.map((symbol) => (
              <button key={symbol.name} className="symbol" title={symbol.name} aria-label={symbol.name} onMouseDown={keep_} onClick={() => insert(symbol)}>
                {symbol.show === "x̄" ? <span className="overline">x</span> : symbol.show}
              </button>
            ))}
          </p>
        )}
      </div>
    );
  }),
);
