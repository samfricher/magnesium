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

import { memo, useEffect, useLayoutEffect, useRef, useState } from "react";

import { core } from "../bridge";
import { keepPlace } from "../prefs";

export type Place = "rest" | "task" | "talk";

export type Sending = { text: string; place: Place };

const CONFIDENCE = ["sure", "unsure", "guess"] as const;
type Confidence = (typeof CONFIDENCE)[number];

/** Whether a key press is part of an input method still composing a character. */
const composing = (event: React.KeyboardEvent) => event.nativeEvent.isComposing || event.keyCode === 229;

/** Dollar signs and spaces alone are not something to send. */
const bare = (text: string) => /^[\s$]*$/.test(text);

type Props = {
  place: Place;
  /** The folder whose draft this is. */
  folder: string | null;
  /** The words to start with, restored from where the learner left them. */
  initial: string;
  /** This line is where the learner is writing: it takes the focus. */
  active: boolean;
  /** The tutor is writing: Return waits for it, and the words go when it is done. */
  busy: boolean;
  /** A layer is over the surface: the line does not take the focus. */
  paused: boolean;
  /** An open task exists that ↑ can go up into. */
  canAnswer: boolean;
  /** Words to show when the line is empty. */
  placeholder?: string;
  /** Photos of work dropped on the window, saved in the study folder, to go with the answer. */
  incoming?: string[];
  onSend: (sending: Sending) => void;
  onMove: (place: "task" | "talk") => void;
  onNote: (text: string) => void;
};

export const Composer = memo(function Composer(props: Props) {
  const [text, setText] = useState(props.initial);
  const [confidence, setConfidence] = useState<Confidence | null>(null);
  const [files, setFiles] = useState<string[]>([]);
  const [queued, setQueued] = useState(false);
  const box = useRef<HTMLTextAreaElement>(null);
  const length = useRef(props.initial.length);
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

  // The line where the learner is writing has the focus whenever nothing
  // else needs it: when it becomes theirs, when the tutor finishes, and when
  // a layer goes away.
  useEffect(() => {
    if (!props.active || props.paused) return;
    const el = box.current;
    if (!el) return;
    const active = document.activeElement;
    if (active && active !== document.body && active !== el && active.closest(".card")) return;
    el.focus({ preventScroll: true });
    el.setSelectionRange(el.value.length, el.value.length);
  }, [props.active, props.paused, props.busy]);

  useEffect(() => {
    if (props.incoming && props.incoming.length > 0) setFiles((old) => [...old, ...props.incoming!.filter((path) => !old.includes(path))]);
  }, [props.incoming]);

  const keep = (value: string) => {
    const { folder, place } = latest.current;
    if (!folder) return;
    keepPlace(folder, place === "task" ? { answerDraft: value } : { draft: value });
  };

  const change = (value: string) => {
    setText(value);
    keep(value);
    if (bare(value)) setConfidence(null);
  };

  const content = !bare(text) || files.length > 0;

  const send = (force = false) => {
    const { place, busy } = latest.current;
    if (!content && place !== "rest") return;
    if (busy && !force) {
      setQueued(true);
      return;
    }
    const parts = [text.trim()];
    if (confidence && !bare(text)) parts.push(`Confidence: ${confidence}`);
    if (files.length > 0) parts.push(`${files.length === 1 ? "Photo" : "Photos"} of my working: ${files.join(", ")}`);
    latest.current.onSend({ text: parts.filter((part) => part !== "").join("\n\n"), place });
    setText("");
    setConfidence(null);
    setFiles([]);
    setQueued(false);
    length.current = 0;
    keep("");
  };

  // Waited for: the words go once the tutor is done.
  useEffect(() => {
    if (queued && !props.busy) send(true);
  });

  // Photos chosen are handed in to the study folder, then named in the message.
  const handIn = async () => {
    try {
      const api = await core();
      const picked = await api.pickFiles();
      const saved: string[] = [];
      for (const path of picked) saved.push(await api.attach(path, "working"));
      if (saved.length > 0) setFiles((old) => [...old, ...saved]);
    } catch (error) {
      props.onNote(String(error));
    }
    box.current?.focus();
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
      return;
    }
    // Photos of your work, with the answer.
    if (event.metaKey && !event.shiftKey && event.key.toLowerCase() === "u" && place === "task") {
      event.preventDefault();
      void handIn();
    }
  };

  const { place } = props;
  const label = place === "task" ? "Your answer to the task" : place === "talk" ? "Say something to the tutor" : "Start, or say what to work on";
  return (
    <div className={`line ${place}${props.active ? " on" : ""}`} data-line={place}>
      <textarea
        ref={box}
        className="field"
        data-drive={place === "task" ? "answer" : "talk"}
        aria-label={label}
        rows={1}
        value={text}
        spellCheck={place !== "task"}
        placeholder={props.placeholder ?? ""}
        onChange={(event) => change(event.target.value)}
        onKeyDown={onKeyDown}
        onFocus={() => place !== "rest" && !latest.current.active && latest.current.onMove(place)}
      />
      {(confidence || files.length > 0 || queued) && (
        <p className="with">
          {confidence && <span className="said-sure">{confidence}</span>}
          {files.length > 0 && (
            <span className="photos">
              {files.length === 1 ? "1 photo" : `${files.length} photos`} of your work
              <button className="plain" onClick={() => setFiles([])} aria-label="Take the photos off">
                take off
              </button>
            </span>
          )}
          {queued && <span className="queued">goes when the tutor finishes</span>}
        </p>
      )}
      {place === "task" && text === "" && <span className="keyhint" aria-hidden="true">⌘/ help</span>}
    </div>
  );
});
