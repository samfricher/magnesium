// The lesson. The whole window is one column: what the tutor just said, and
// one box to answer in. The goal's steps and the clock are two small marks in
// the top strip. Not on screen: tool calls, save messages, token counts, the
// model's name, other sessions, a sidebar.
//
// Three things the page keeps true while the tutor writes:
//  - the task in hand stays on screen until the tutor sets a new one (a turn
//    that holds it is never folded away; if help replies push it up out of
//    view, a strip at the top of the column holds it);
//  - the task is never under the answer box or its fade;
//  - nothing the learner types goes anywhere while a dialog, a sheet or a
//    popover is open: the answer box is inert, and it takes the focus back
//    when the dialog closes.

import { useDeferredValue, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";

import { core } from "../bridge";
import { HELP, busy, isHint, opening, splitConfidence, tutorText, type Card, type SessionState } from "../lesson";
import { countTasks, hasMath, hasTask, renderPlain, sourceLinks } from "../render";
import { sourceRef, type SourceRef } from "../sources";
import { Line, Rich } from "../ui/Rich";
import { AddMenu, MaterialNote } from "../ui/Material";
import { SourcePanel } from "../ui/SourcePanel";
import { Icon } from "../ui/icons";
import { PaletteKey, TopBar, composing, splitGoal, useNow } from "../ui/bits";
import type { Answer, Decision, Question, Rail as RailData } from "../types";

const CONFIDENCE: [string, string][] = [
  ["sure", "Sure"],
  ["unsure", "Unsure"],
  ["guess", "Guess"],
];

/** How far from the top of the view a thing is brought to, and the room kept under the task, in pixels. */
const EDGE = 16;
const GAP = 32;

/** The learner's words as typed, with their mathematics drawn. */
function Said({ text, className }: { text: string; className: string }) {
  const html = useMemo(() => renderPlain(text), [text]);
  return <div className={className} dangerouslySetInnerHTML={{ __html: html }} />;
}

/**
 * What the learner said in one turn. The line the window sent to open the
 * lesson is not theirs, so it is one quiet line and not a bubble. How sure
 * they said they were is a small tag after their words, not part of them.
 */
function Turn({ text }: { text: string }) {
  const opener = opening(text);
  const said = useMemo(() => splitConfidence(text), [text]);
  const html = useMemo(() => renderPlain(said.text), [said.text]);
  if (!opener) {
    return (
      <div className="you selectable">
        <span className="you-body" dangerouslySetInnerHTML={{ __html: html }} />
        {said.confidence && (
          <span className="you-tag" title="How sure you said you were">
            {said.confidence}
          </span>
        )}
      </div>
    );
  }
  return (
    <div className="resumed">
      {opener.stoppedAt ? (
        <span>
          Picking up at <Line text={opener.stoppedAt} />
        </span>
      ) : (
        <span>Lesson started</span>
      )}
    </div>
  );
}

/** Whether the page is as narrow as a source can only open as a sheet over the lesson. */
function useNarrow(): boolean {
  const query = "(max-width: 900px)";
  const [narrow, setNarrow] = useState(() => window.matchMedia(query).matches);
  useEffect(() => {
    const media = window.matchMedia(query);
    const change = () => setNarrow(media.matches);
    media.addEventListener("change", change);
    return () => media.removeEventListener("change", change);
  }, []);
  return narrow;
}

/** The goal's steps as a row of marks, and the step in hand in words. A click shows them all. */
function Track(props: { rail: RailData | null; open: boolean; onOpen: (open: boolean) => void }) {
  const { rail, open } = props;
  const holder = useRef<HTMLDivElement>(null);
  const steps = rail?.steps ?? [];
  // The step in hand: the one the tutor marked, else the first not done.
  const marked = steps.findIndex((s) => s.status === "doing");
  const firstOpen = steps.findIndex((s) => s.status !== "done");
  const current = marked >= 0 ? marked : firstOpen;
  const label = current >= 0 ? steps[current].step : (rail?.goal ?? rail?.next_action ?? null);

  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => {
      if (!holder.current?.contains(event.target as Node)) props.onOpen(false);
    };
    const key = (event: KeyboardEvent) => {
      if (event.key === "Escape") props.onOpen(false);
    };
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", key);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", key);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  if (!label) return null;
  // Five unlabelled dashes mean nothing to a newcomer: say which step this is.
  const where = steps.length > 0 && current >= 0 ? `Step ${current + 1} of ${steps.length}` : null;
  const goal = rail?.goal ? splitGoal(rail.goal) : null;
  return (
    <div className="track-holder" ref={holder}>
      <button
        className={open ? "track on" : "track"}
        onClick={() => props.onOpen(!open)}
        title={where ? `${where}. Show the goal and its steps.` : "Show the goal"}
        aria-expanded={open}
        aria-haspopup="dialog"
        data-role="track"
      >
        {where && <span className="track-where">{where}</span>}
        {steps.length > 0 && (
          <span className="pips" aria-hidden="true">
            {steps.map((step, index) => (
              <i key={index} className={step.status === "done" ? "done" : index === current ? "cur" : undefined} />
            ))}
          </span>
        )}
        <Line text={label} className="track-label" />
        <Icon name="down" size={14} />
      </button>
      {open && (
        <div className="plan" role="dialog" aria-label="The goal and its steps">
          <span className="lesson-label">Goal</span>
          {rail?.goal && goal ? (
            <div className="plan-goal selectable">
              {goal.ref && <span className="plan-ref">{goal.ref}</span>}
              <p>
                <Line text={goal.title} />
              </p>
            </div>
          ) : (
            <p className="plan-goal dim">The tutor sets the goal in its first reply.</p>
          )}
          {steps.length > 0 ? (
            <ol>
              {steps.map((step, index) => (
                <li key={index} className={step.status === "done" ? "done" : index === current ? "cur" : undefined}>
                  <i>{step.status === "done" && <Icon name="check" size={11} />}</i>
                  <Line text={step.step} />
                </li>
              ))}
            </ol>
          ) : (
            rail?.next_action && (
              <ol>
                <li className="cur">
                  <i />
                  <Line text={rail.next_action} />
                </li>
              </ol>
            )
          )}
        </div>
      )}
    </div>
  );
}

/** The block's clock as plain text: the minutes left, or the minutes over. */
function Clock({ startedMs, blockMinutes }: { startedMs: number | null; blockMinutes: number }) {
  const now = useNow(15_000);
  const used = startedMs === null ? 0 : Math.max(0, Math.floor((now - startedMs) / 60_000));
  const left = blockMinutes - used;
  const over = left < 0;
  return (
    <span className={over ? "clock over" : "clock"} title={`Lesson length: ${blockMinutes} min. Change it in the menu (⌘K).`} data-role="clock">
      {over ? -left : left} {over ? "min over" : "min left"}
    </span>
  );
}

function PermissionCard(props: {
  card: Extract<Card, { kind: "permission" }>;
  onDecide: (id: string, decision: Decision) => void;
}) {
  const { card } = props;
  // The card never takes the keyboard. A learner typing ahead would allow it
  // with a space or an Enter they meant for the answer box.
  return (
    <div className="ask" data-card="permission">
      <span className="lesson-label">The tutor asks to</span>
      <h3>{card.title}</h3>
      {card.detail && <code className="detail selectable">{card.detail}</code>}
      <div className="row">
        <button className="btn primary" onClick={() => props.onDecide(card.id, "allow_once")}>
          Allow once
        </button>
        <button className="btn" onClick={() => props.onDecide(card.id, "refuse")}>
          Refuse
        </button>
        {card.canAlways && (
          <button className="btn ghost" onClick={() => props.onDecide(card.id, "allow_always")}>
            Always allow this
          </button>
        )}
      </div>
    </div>
  );
}

/**
 * The tutor's question, with its options. The field for the learner's own
 * words has the focus when it appears, so typing types. The options are
 * numbered 1, 2, 3. With the field empty, a digit chooses that option and an
 * up arrow moves into them. Inside them the arrows move and choose, and Return
 * sends the one chosen. A letter typed there goes back to the field and is
 * typed. Nothing is sent by a key that only chooses.
 */
function QuestionCard(props: {
  card: Extract<Card, { kind: "question" }>;
  onAnswer: (id: string, answers: Answer[]) => void;
}) {
  const { card } = props;
  const [chosen, setChosen] = useState<Record<number, string[]>>({});
  const [own, setOwn] = useState<Record<number, string>>({});
  const root = useRef<HTMLDivElement>(null);
  const single = card.questions.length === 1 && !card.questions[0].multi;
  const most = Math.min(9, Math.max(1, ...card.questions.map((q) => q.options.length)));

  useEffect(() => {
    root.current?.querySelector<HTMLInputElement>("input.field")?.focus();
  }, [card.id]);

  const answersNow = (extra?: { index: number; label: string }): Answer[] =>
    card.questions.map((q, index) => {
      const typed = (own[index] ?? "").trim();
      const picked = extra && extra.index === index ? [extra.label] : (chosen[index] ?? []);
      return { question: q.question, chosen: typed !== "" ? [...picked, typed] : picked };
    });
  const complete = (extra?: { index: number; label: string }) => answersNow(extra).every((a) => a.chosen.length > 0);

  /** Highlight an option. Nothing is sent. A multiple-choice option toggles. */
  const choose = (index: number, q: Question, label: string) => {
    setChosen((old) => {
      const had = old[index] ?? [];
      const next = q.multi ? (had.includes(label) ? had.filter((l) => l !== label) : [...had, label]) : [label];
      return { ...old, [index]: next };
    });
  };

  /** A click on an option is a plain choice: for a single question it is also the answer. */
  const click = (index: number, q: Question, label: string) => {
    if (single) props.onAnswer(card.id, answersNow({ index, label }));
    else choose(index, q, label);
  };

  const options = (index: number) => Array.from(root.current?.querySelectorAll<HTMLButtonElement>(`.ask-q[data-q="${index}"] .option`) ?? []);
  const field = (index: number) => root.current?.querySelector<HTMLInputElement>(`.ask-q[data-q="${index}"] input.field`) ?? null;

  const onKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.metaKey || event.ctrlKey || event.altKey || composing(event)) return;
    const target = event.target as HTMLElement;
    const group = target.closest<HTMLElement>(".ask-q");
    if (!group) return;
    const index = Number(group.dataset.q);
    const q = card.questions[index];
    const list = options(index);
    const digit = /^[1-9]$/.test(event.key) ? Number(event.key) - 1 : -1;

    // Esc takes back a choice first. With none made it goes on to the page.
    if (event.key === "Escape") {
      if (Object.values(chosen).some((labels) => labels.length > 0)) {
        event.preventDefault();
        setChosen({});
        field(index)?.focus();
      }
      return;
    }

    if (target.tagName === "INPUT") {
      // From the field, up goes to the options: the one chosen, else the first.
      if (event.key === "ArrowUp" && list.length > 0) {
        event.preventDefault();
        (list.find((el) => el.getAttribute("aria-checked") === "true") ?? list[0]).focus();
      } else if (digit >= 0 && digit < q.options.length && (own[index] ?? "") === "") {
        // The field is empty, so a digit is a choice and not a character.
        event.preventDefault();
        choose(index, q, q.options[digit].label);
        list[digit]?.focus();
      }
      return;
    }

    const at = list.indexOf(target as HTMLButtonElement);
    if (at < 0 || !q) return;
    const move = (to: number) => {
      event.preventDefault();
      const next = (to + list.length) % list.length;
      list[next]?.focus();
      // In a group of one answer the arrow chooses, as in any radio group.
      if (!q.multi) choose(index, q, q.options[next].label);
    };
    if (event.key === "ArrowDown" || event.key === "ArrowRight") return move(at + 1);
    if (event.key === "ArrowUp" || event.key === "ArrowLeft") return move(at - 1);

    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      const label = q.options[at].label;
      if (q.multi) {
        // A box that allows several: Space ticks it, Return sends when every question has an answer.
        if (event.key === " ") choose(index, q, label);
        else if (complete()) props.onAnswer(card.id, answersNow());
        return;
      }
      if (event.key === " ") return choose(index, q, label);
      choose(index, q, label);
      if (single || complete({ index, label })) props.onAnswer(card.id, answersNow({ index, label }));
      else field(index + 1)?.focus();
      return;
    }

    if (digit >= 0 && digit < q.options.length) {
      event.preventDefault();
      choose(index, q, q.options[digit].label);
      list[digit]?.focus();
      return;
    }

    // Any other character is the start of the learner's own words.
    if (event.key.length === 1) {
      event.preventDefault();
      const box = field(index);
      setOwn((old) => ({ ...old, [index]: `${old[index] ?? ""}${event.key}` }));
      box?.focus();
      window.requestAnimationFrame(() => box?.setSelectionRange(box.value.length, box.value.length));
    }
  };

  return (
    <div className="ask" data-card="question" ref={root} onKeyDown={onKeyDown}>
      {card.questions.map((q, index) => {
        const picked = chosen[index] ?? [];
        // One tab stop for the group: the chosen option, else the first.
        const stop = Math.max(0, q.options.findIndex((o) => picked.includes(o.label)));
        return (
          <div className="ask-q" key={index} data-q={index}>
            {q.header && <span className="lesson-label">{q.header}</span>}
            <h3 id={`ask-${card.id}-${index}`}>
              <Line text={q.question} />
            </h3>
            <div className="options" role={q.multi ? "group" : "radiogroup"} aria-labelledby={`ask-${card.id}-${index}`}>
              {q.options.map((option, n) => {
                const on = picked.includes(option.label);
                return (
                  <button
                    key={option.label}
                    className={on ? "option on" : "option"}
                    role={q.multi ? "checkbox" : "radio"}
                    aria-checked={on}
                    tabIndex={n === stop ? 0 : -1}
                    aria-keyshortcuts={String(n + 1)}
                    title={`${n + 1} chooses it. Return sends it.`}
                    onClick={() => click(index, q, option.label)}
                    // A space on a button clicks it when the key comes up; here it only highlights.
                    onKeyUp={(e) => {
                      if (e.key === " ") e.preventDefault();
                    }}
                  >
                    <i>{n + 1}</i>
                    <span>
                      <Line text={option.label} />
                      {option.description && (
                        <small>
                          <Line text={option.description} />
                        </small>
                      )}
                    </span>
                  </button>
                );
              })}
            </div>
            <input
              className="field"
              placeholder="Or say it in your own words"
              aria-label="Or say it in your own words"
              value={own[index] ?? ""}
              onChange={(e) => setOwn((old) => ({ ...old, [index]: e.target.value }))}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !composing(e) && complete()) props.onAnswer(card.id, answersNow());
              }}
            />
          </div>
        );
      })}
      <div className="row">
        <button className="btn primary" disabled={!complete()} title="Send the answer (Return)" onClick={() => props.onAnswer(card.id, answersNow())}>
          Answer
        </button>
        <span className="aside">
          Choose with {Array.from({ length: most }, (_, n) => n + 1).join(" ")} or ↑ ↓ and Return, or write your own.
        </span>
      </div>
    </div>
  );
}

/**
 * What the learner has typed and not sent. It lives above the lesson screen,
 * so leaving the lesson (the menu, Proofs) and coming back finds it as it was.
 * It belongs to one exchange of one lesson, and is dropped when that lesson ends.
 */
type Draft = { key: string; text: string; confidence: string | null; files: string[] };
let kept: Draft | null = null;

function forgetDraft() {
  kept = null;
}

/** The symbols the course's answers most often need, and the LaTeX each one writes. `|` is where the caret goes. */
const SYMBOLS: { show: string; name: string; tex: string }[] = [
  { show: "\\bar{x}", name: "x bar, the mean", tex: "\\bar{x}" },
  { show: "\\sigma", name: "sigma", tex: "\\sigma" },
  { show: "\\mu", name: "mu", tex: "\\mu" },
  { show: "\\sqrt{x}", name: "square root", tex: "\\sqrt{|}" },
  { show: "x^2", name: "squared", tex: "^2" },
  { show: "x^3", name: "cubed", tex: "^3" },
  { show: "\\le", name: "less than or equal to", tex: "\\le" },
  { show: "\\ge", name: "greater than or equal to", tex: "\\ge" },
  { show: "\\pm", name: "plus or minus", tex: "\\pm" },
  { show: "\\times", name: "times", tex: "\\times" },
  { show: "\\div", name: "divided by", tex: "\\div" },
  { show: "\\sum", name: "sum", tex: "\\sum" },
  { show: "\\infty", name: "infinity", tex: "\\infty" },
  { show: "\\ne", name: "not equal to", tex: "\\ne" },
  { show: "\\approx", name: "approximately equal to", tex: "\\approx" },
  { show: "\\frac{a}{b}", name: "fraction", tex: "\\frac{|}{}" },
];

/** Whether the caret, after `before`, is inside a formula already begun with a dollar sign. */
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

/** The learner's text holds nothing to send: empty, or only dollar signs and spaces. */
const bare = (text: string) => /^[\s$]*$/.test(text);

/** How sure the learner is: optional, one choice of three. A radio group with one tab stop; the arrows move and choose. */
function Sure(props: { value: string | null; onChange: (value: string | null, fromKey: boolean) => void }) {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const at = CONFIDENCE.findIndex(([word]) => word === props.value);
  const stop = at >= 0 ? at : 0;
  const key = (event: React.KeyboardEvent, from: number) => {
    const step = event.key === "ArrowRight" || event.key === "ArrowDown" ? 1 : event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 0;
    if (step === 0 || event.metaKey || event.ctrlKey || event.altKey) return;
    event.preventDefault();
    const next = (from + step + CONFIDENCE.length) % CONFIDENCE.length;
    props.onChange(CONFIDENCE[next][0], true);
    refs.current[next]?.focus();
  };
  return (
    <span className="sure">
      <span className="sure-label" aria-hidden="true">
        How sure?
      </span>
      <span
        className="seg"
        role="radiogroup"
        aria-label="How sure are you? Optional."
        title="Optional. Helps the tutor choose what to review. Return sends without it."
      >
        {CONFIDENCE.map(([word, label], n) => (
          <button
            key={word}
            ref={(el) => {
              refs.current[n] = el;
            }}
            role="radio"
            aria-checked={props.value === word}
            tabIndex={n === stop ? 0 : -1}
            data-label={label}
            onKeyDown={(e) => key(e, n)}
            onClick={() => props.onChange(props.value === word ? null : word, false)}
          >
            {label}
          </button>
        ))}
      </span>
    </span>
  );
}

function Composer(props: {
  /** The tutor is writing: Return waits for it. */
  disabled: boolean;
  /** A dialog, a sheet or a popover is open: nothing is sent from here and the box does not take the focus. */
  paused: boolean;
  /** Names the exchange the draft belongs to. */
  draftKey: string;
  onSend: (text: string) => void;
  onNote: (text: string) => void;
  onAddFiles: () => void;
  onPasteText: () => void;
  /** The working line, shown at the end of the row of help buttons while the tutor writes. */
  working?: React.ReactNode;
}) {
  const saved = kept && kept.key === props.draftKey ? kept : null;
  const [text, setText] = useState(saved?.text ?? "");
  const [confidence, setConfidence] = useState<string | null>(saved?.confidence ?? null);
  const [files, setFiles] = useState<string[]>(saved?.files ?? []);
  const [symbols, setSymbols] = useState(false);
  // Return was pressed while the tutor was writing: the answer goes when it finishes.
  const [queued, setQueued] = useState(false);
  const box = useRef<HTMLTextAreaElement>(null);
  const preview = useDeferredValue(text);
  // Where the caret goes once the text has been set, so a key pressed at once lands where the symbol left it.
  const caretAt = useRef<number | null>(null);
  const wasPaused = useRef(props.paused);
  // The box always has the focus, so its ring is for a learner who Tabbed
  // into it, never for a click or for the focus the page gave it.
  const [ringed, setRinged] = useState(false);
  const tabbed = useRef(false);
  useEffect(() => {
    const down = (event: KeyboardEvent) => {
      tabbed.current = event.key === "Tab";
    };
    const point = () => {
      tabbed.current = false;
      setRinged(false);
    };
    window.addEventListener("keydown", down, true);
    window.addEventListener("pointerdown", point, true);
    return () => {
      window.removeEventListener("keydown", down, true);
      window.removeEventListener("pointerdown", point, true);
    };
  }, []);

  // A draft left behind when the lesson was left is back, with the caret at its end.
  useLayoutEffect(() => {
    const el = box.current;
    if (el && el.value !== "") el.setSelectionRange(el.value.length, el.value.length);
  }, []);

  // What is typed is kept above the screen, so leaving the lesson does not lose it.
  useEffect(() => {
    kept = text === "" && confidence === null && files.length === 0 ? null : { key: props.draftKey, text, confidence, files };
  }, [text, confidence, files, props.draftKey]);

  // The box grows with what is typed, up to about eight lines.
  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 200)}px`;
    if (caretAt.current !== null) {
      el.focus();
      el.setSelectionRange(caretAt.current, caretAt.current);
      caretAt.current = null;
    }
  }, [text]);

  // The box has the focus when there is nothing else that has it: on arrival,
  // when the tutor finishes, and when a dialog closes. It never takes it from
  // a field the learner is in, and never while a dialog is open.
  useEffect(() => {
    if (props.paused) {
      wasPaused.current = true;
      return;
    }
    const active = document.activeElement;
    const free = !active || active === document.body;
    const back = wasPaused.current && !(active instanceof HTMLInputElement || active instanceof HTMLTextAreaElement);
    wasPaused.current = false;
    if (free || back) box.current?.focus();
  }, [props.paused, props.disabled]);

  // Dollar signs and spaces alone are not an answer.
  const content = !bare(text) || files.length > 0;

  const send = () => {
    if (props.paused || !content) return;
    if (props.disabled) {
      setQueued(true);
      return;
    }
    const parts = [text.trim()];
    if (confidence) parts.push(`Confidence: ${confidence}`);
    if (files.length > 0) parts.push(`${files.length === 1 ? "Photo" : "Photos"} of my working: ${files.join(", ")}`);
    props.onSend(parts.filter((part) => part !== "").join("\n\n"));
    setText("");
    setConfidence(null);
    setFiles([]);
    setQueued(false);
    setSymbols(false);
  };

  // Waited for: the answer goes once the tutor is done, and no dialog is open.
  useEffect(() => {
    if (queued && !props.disabled && !props.paused) send();
  });

  /** Ask for help in plain words, and keep the focus in the answer box. */
  const ask = (words: string) => {
    if (props.disabled || props.paused) return;
    props.onSend(words);
    box.current?.focus();
  };

  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (!event.metaKey || !event.shiftKey || event.altKey || event.ctrlKey || props.paused) return;
      const words = event.code === "KeyH" ? HELP.hint : event.code === "KeyD" ? HELP.unknown : event.code === "KeyL" ? HELP.lost : null;
      if (!words) return;
      event.preventDefault();
      ask(words);
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  });

  const addPhoto = async () => {
    const api = await core();
    try {
      for (const path of await api.pickFiles()) {
        const saved = await api.attach(path, "working");
        setFiles((old) => [...old, saved]);
      }
    } catch (error) {
      props.onNote(String(error));
    }
  };

  /**
   * Write a symbol's LaTeX at the caret. Inside a formula already begun it goes
   * in as it is; outside one it goes between dollar signs. A selection is
   * wrapped by a symbol that takes an argument, and left as it is by one that
   * does not.
   */
  const insert = (tex: string) => {
    const el = box.current;
    if (!el) return;
    const mark = tex.indexOf("|");
    const picked = mark >= 0 ? text.slice(el.selectionStart, el.selectionEnd) : "";
    const from = mark >= 0 ? el.selectionStart : el.selectionEnd;
    const to = mark >= 0 ? el.selectionEnd : el.selectionEnd;
    const before = text.slice(0, from);
    const snippet = mark >= 0 ? `${tex.slice(0, mark)}${picked}${tex.slice(mark + 1)}` : tex;
    const inside = inMath(before);
    const open = inside ? "" : "$";
    const close = inside ? "" : "$";
    // The caret goes where the argument is, or, for a fraction taking a selection, into its denominator.
    const caret = mark >= 0 && picked === "" ? mark : picked !== "" && snippet.endsWith("{}") ? snippet.length - 1 : snippet.length;
    caretAt.current = from + open.length + caret;
    setText(`${before}${open}${snippet}${close}${text.slice(to)}`);
  };

  const closeSymbols = () => {
    setSymbols(false);
    box.current?.focus();
  };

  const why = "Available when the tutor finishes";
  return (
    <div
      className="composer"
      onKeyDown={(e) => {
        // Esc takes back the symbol row first, before anything else.
        if (e.key === "Escape" && symbols && !composing(e)) {
          e.preventDefault();
          closeSymbols();
        }
      }}
    >
      {symbols && (
        <div className="symbols" id="symbol-row" role="toolbar" aria-label="Symbols for mathematics">
          {SYMBOLS.map((symbol) => (
            <button
              key={symbol.name}
              type="button"
              className="symbol"
              title={symbol.name}
              aria-label={symbol.name}
              // The caret stays in the answer box: a click on a symbol never takes it.
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => insert(symbol.tex)}
            >
              <Line text={`$${symbol.show}$`} />
            </button>
          ))}
        </div>
      )}
      <div className="box" data-ring={ringed ? "key" : undefined}>
        <textarea
          ref={box}
          rows={1}
          placeholder="Your answer. Shift+Return starts a new line."
          aria-label="Your answer"
          value={text}
          autoFocus
          spellCheck={false}
          onFocus={() => setRinged(tabbed.current)}
          onBlur={() => setRinged(false)}
          onChange={(e) => {
            setText(e.target.value);
            setQueued(false);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey && !composing(e)) {
              e.preventDefault();
              send();
            }
          }}
        />
        {text.includes("$") && (
          // Once a formula is begun, its room is kept, so the box does not
          // change height as the formula is drawn.
          <div className="preview" aria-label="Your formula, drawn">
            {hasMath(preview) ? <Said text={preview} className="preview-body" /> : <span className="preview-empty">Your formula is drawn here</span>}
          </div>
        )}
        {files.length > 0 && (
          <div className="files">
            {files.map((file) => (
              <span className="file" key={file}>
                <Icon name="camera" size={13} />
                {file.split("/").pop()}
                <button aria-label="Remove" onClick={() => setFiles((old) => old.filter((f) => f !== file))}>
                  <Icon name="close" size={12} />
                </button>
              </span>
            ))}
          </div>
        )}
        <div className="tools">
          <button className="icon-btn" title="Add a photo of your working" aria-label="Add a photo of your working" onClick={addPhoto}>
            <Icon name="camera" size={17} />
          </button>
          <AddMenu disabled={false} onFiles={props.onAddFiles} onPaste={props.onPasteText} />
          <button
            className={symbols ? "icon-btn on" : "icon-btn"}
            title="Symbols for mathematics"
            aria-label="Symbols for mathematics"
            aria-expanded={symbols}
            aria-controls="symbol-row"
            onClick={() => {
              if (symbols) return closeSymbols();
              setSymbols(true);
              box.current?.focus();
            }}
          >
            <Icon name="sigma" size={17} />
          </button>
          <span className="grow" />
          {queued && (
            <span className="queued" role="status">
              Sends when the tutor finishes
            </span>
          )}
          <Sure
            value={confidence}
            onChange={(value, fromKey) => {
              setConfidence(value);
              if (!fromKey) box.current?.focus();
            }}
          />
          <button
            className="btn primary small send"
            disabled={!content}
            onClick={send}
            title={props.disabled ? "Sends when the tutor finishes" : "Send (Return)"}
          >
            Send
          </button>
        </div>
      </div>
      <div className={props.working ? "outs busy" : "outs"}>
        {/* Plain words, as the other two send. Quiet until pointed at. */}
        <button
          className="btn ghost small"
          disabled={props.disabled}
          title={props.disabled ? why : "Ask for a hint (⌘⇧H)"}
          aria-keyshortcuts="Meta+Shift+H"
          onClick={() => ask(HELP.hint)}
        >
          Hint
        </button>
        <button
          className="btn ghost small"
          disabled={props.disabled}
          title={props.disabled ? why : "Say you don't know (⌘⇧D)"}
          aria-keyshortcuts="Meta+Shift+D"
          onClick={() => ask(HELP.unknown)}
        >
          I don&apos;t know
        </button>
        <button
          className="btn ghost small"
          disabled={props.disabled}
          title={props.disabled ? why : "Say you are lost (⌘⇧L)"}
          aria-keyshortcuts="Meta+Shift+L"
          onClick={() => ask(HELP.lost)}
        >
          I&apos;m lost
        </button>
        {props.working}
      </div>
    </div>
  );
}

/** The line the lesson shows while the tutor works, as a plain sentence. */
export function workingLabel(seconds: number, status: string, doing: string | null): string {
  const minutes = seconds >= 60 ? `, ${Math.floor(seconds / 60)} min` : "";
  // A named step shows at once, from the first second of the turn.
  if (status === "working" && doing) return `${doing.charAt(0).toUpperCase()}${doing.slice(1)}${minutes}`;
  return seconds < 60 ? (status === "working" && seconds >= 8 ? "Working" : "") : `Working${minutes}`;
}

function Thinking(props: {
  since: number;
  status: string;
  doing: string | null;
  quiet: boolean;
  /** Stop was pressed: the line says so at once, before the program answers. */
  stopping: boolean;
  onInterrupt: () => void;
}) {
  const now = useNow(5_000);
  const seconds = Math.max(0, Math.floor((now - props.since) / 1000));
  // A plan can take half an hour to build. One quiet line says the window is
  // alive and names the work in hand when it can, such as "Writing paper 2".
  // It is one line that changes, not a log.
  const label = props.stopping ? "Stopping" : workingLabel(seconds, props.status, props.doing) || (props.quiet ? "" : "Thinking");
  return (
    <div className="thinking" data-role="thinking" data-stopping={props.stopping ? "yes" : undefined}>
      {label !== "" && (
        <>
          <span className="thinking-label" role="status">
            {label}
          </span>
          <span className="working-line" aria-hidden="true" />
        </>
      )}
      <button
        className="btn ghost small stop"
        onClick={props.onInterrupt}
        disabled={props.stopping}
        aria-pressed={props.stopping}
        title="Stop this reply (⌘.)"
      >
        <Icon name="stop" size={13} fill /> Stop
      </button>
    </div>
  );
}

/** The task in the strip: its label, and its first lines as running text. */
type Pin = { label: string; html: string; n: string };

function summarize(task: HTMLElement): Pin {
  const box = task.cloneNode(true) as HTMLElement;
  const blocks = box.matches("p") ? [box] : Array.from(box.children);
  const label = (blocks[0]?.querySelector("strong:first-child")?.textContent ?? "Task").trim();
  const parts: string[] = [];
  for (const block of blocks) {
    if (block.querySelector(".katex-display") || block.matches(".katex-display")) continue;
    block.querySelector("strong:first-child")?.remove();
    const html = block.innerHTML.trim();
    if (html !== "") parts.push(html);
    if (parts.length === 2) break;
  }
  return { label, html: parts.join(" "), n: task.dataset.n ?? "" };
}

export function Session(props: {
  state: SessionState;
  rail: RailData | null;
  blockMinutes: number;
  /** A dialog over the window, such as the command palette: the lesson is inert behind it. */
  overlay?: boolean;
  onSend: (text: string) => void;
  onInterrupt: () => void;
  onDecide: (id: string, decision: Decision) => void;
  onAnswer: (id: string, answers: Answer[]) => void;
  onStop: (line: string) => void;
  onNote: (text: string) => void;
  onDismiss: (text: string) => void;
  onPalette: () => void;
  /** What adding course material said, until it is dismissed. */
  material: string[] | null;
  onDismissMaterial: () => void;
  onAddFiles: () => void;
  onPasteText: () => void;
}) {
  const { state } = props;
  const live = state.live;
  const isBusy = busy(state);
  const isBusyRef = useRef(isBusy);
  isBusyRef.current = isBusy;
  const narrow = useNarrow();
  const [showEarlier, setShowEarlier] = useState(false);
  const [stopping, setStopping] = useState(false);
  // Stop was pressed (or ⌘. was): the lesson says so at once, then the program answers.
  const [interrupting, setInterrupting] = useState(false);
  const [planOpen, setPlanOpen] = useState(false);
  const [line, setLine] = useState("");
  // The block's end is said once. "Keep going" means it.
  const [nudged, setNudged] = useState(false);
  const liveRef = useRef<HTMLDivElement>(null);
  const stage = useRef<HTMLDivElement>(null);
  const stopInput = useRef<HTMLInputElement>(null);
  const keepGoing = useRef<HTMLButtonElement>(null);
  const now = useNow(20_000);

  // A source a reply cited opens beside the lesson, but only after the learner
  // has answered that reply's task, and it closes when the tutor poses the
  // next one. Until then the link reads as plain text. A reply that sets no
  // task has nothing to answer, so its links are open.
  const [source, setSource] = useState<{ ref: SourceRef; posed: number } | null>(null);
  const sheet = source !== null && narrow;
  // Anything that takes the keyboard from the answer box.
  const covered = (props.overlay ?? false) || planOpen || stopping || sheet;

  const beginStop = (keep = false) => {
    setPlanOpen(false);
    setLine(props.rail?.next_action ?? "");
    setStopping(true);
    window.setTimeout(() => (keep ? keepGoing.current : stopInput.current)?.focus(), 0);
  };

  // Esc closes the innermost thing first: the symbol row, a choice made on a
  // card, a popover, a source (each takes the key and says so). Only with
  // nothing open does it open the End sheet, with "Keep going" in hand, so a
  // second Esc or a Return goes back to the lesson. It does this the same way
  // whether or not the answer box holds a draft: the draft is kept. It never
  // ends a lesson by itself.
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented || event.isComposing) return;
      if (covered || document.querySelector(".add-menu .menu")) return;
      event.preventDefault();
      // A source beside the lesson, or over it, is shut before anything else is.
      if (source) setSource(null);
      else beginStop(true);
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  });

  // ⌘. stops the reply (App sends it to the program). The working line shows
  // "Stopping" in the same frame, not when the program has answered.
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key === "." && isBusyRef.current) setInterrupting(true);
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, []);
  // A new reply, or the end of this one, ends "Stopping". If the program never
  // answers, Stop comes back after a few seconds rather than staying dead.
  useEffect(() => {
    if (!interrupting) return;
    if (!isBusy) return setInterrupting(false);
    const timer = window.setTimeout(() => setInterrupting(false), 4000);
    return () => window.clearTimeout(timer);
  }, [interrupting, isBusy, live?.since]);
  useEffect(() => setInterrupting(false), [live?.since]);

  const stopReply = () => {
    setInterrupting(true);
    props.onInterrupt();
    // The button goes dead; the answer box keeps the keyboard.
    document.querySelector<HTMLTextAreaElement>(".composer textarea")?.focus();
  };

  // The text of the reply may lag behind the stream, but never across a change
  // of turn: the next reply does not begin with the last one's words on screen.
  const raw = live ? tutorText(live) : "";
  const lagged = useDeferredValue(useMemo(() => ({ since: live?.since, raw }), [live?.since, raw]));
  const text = lagged.since === live?.since ? lagged.raw : raw;

  // The tasks are numbered in the order the lesson set them, a reply with two checks taking two numbers.
  const counts = useMemo(() => state.earlier.map((turn) => countTasks(turn.tutor)), [state.earlier]);
  const setsTask = useMemo(() => counts.map((count) => count > 0), [counts]);
  const taskStarts = useMemo(() => {
    let next = 1;
    return counts.map((count) => {
      const start = next;
      next += count;
      return start;
    });
  }, [counts]);
  const liveStart = 1 + counts.reduce((all, count) => all + count, 0);
  const liveSetsTask = text !== "" && hasTask(text);
  const posed = setsTask.filter(Boolean).length + (liveSetsTask ? 1 : 0);

  // The task in hand is the last one the tutor set. The turn that holds it is
  // never folded: the turns before it are, and it stays with the help that
  // follows it until the tutor sets a new task. The lesson's own opening is
  // not an answer to anything, so nothing is carried into it.
  const lastTask = setsTask.lastIndexOf(true);
  const from = lastTask >= 0 && !opening(live?.learner ?? "") ? lastTask : state.earlier.length;
  const carried = state.earlier.slice(from);
  const earlier = from;

  /** Whether the learner has said something real since the reply `index` of the earlier turns. */
  const answered = (index: number) => {
    const next = state.earlier[index + 1]?.learner ?? live?.learner ?? "";
    return next.trim() !== "" && !opening(next);
  };
  useEffect(() => {
    if (source && posed > source.posed) setSource(null);
  }, [posed, source]);
  const openSource = (ref: SourceRef) => setSource({ ref, posed });
  // The passages of the reply just answered, in reach while the next one is written.
  const last = state.earlier.length - 1;
  const passages = useMemo(
    () => (last >= 0 && setsTask[last] ? sourceLinks(state.earlier[last].tutor) : []),
    [last, setsTask, state.earlier],
  );
  // They stand in the band above the answer box, whose room is always kept, so
  // showing them or taking them away (once the next reply sets a task) moves nothing.
  const showPassages = passages.length > 0 && answered(last) && !liveSetsTask;
  const blockOver = state.startedMs !== null && now - state.startedMs >= props.blockMinutes * 60_000;

  // The first hint asked on a task says what a hint costs, once.
  const firstHints = useMemo(() => {
    const firsts = new Set<number>();
    let asked = false;
    state.earlier.forEach((turn, index) => {
      if (isHint(turn.learner) && !asked) {
        firsts.add(index);
        asked = true;
      }
      if (setsTask[index]) asked = false;
    });
    if (live && isHint(live.learner) && !asked) firsts.add(state.earlier.length);
    return firsts;
  }, [state.earlier, setsTask, live]);

  /** One exchange of the lesson: what was said, the reply, and what a hint costs. */
  const exchange = (turn: { learner: string; tutor: string; interrupted: boolean }, index: number, opts: { writing: boolean }) => {
    const hint = isHint(turn.learner) && turn.tutor !== "";
    return (
      <>
        {turn.learner && <Turn text={turn.learner} />}
        {hint && <div className="lesson-label help-label">Hint</div>}
        {turn.tutor !== "" && (
          <div className={opts.writing ? "tutor writing" : "tutor"} data-role={index === state.earlier.length ? "tutor" : undefined}>
            <Rich
              text={turn.tutor}
              final={!opts.writing}
              stopped={turn.interrupted && !opts.writing}
              onNote={props.onNote}
              linksOpen={index < state.earlier.length ? !setsTask[index] || answered(index) : !opts.writing && !liveSetsTask}
              onSource={openSource}
              taskFrom={taskStarts[index] ?? liveStart}
            />
          </div>
        )}
        {hint && firstHints.has(index) && <p className="help-note">With a hint, this answer won&apos;t count as a proof today. It comes back later, to answer without help.</p>}
      </>
    );
  };

  // ---- where the view is, and what it holds in view ----

  const follow = useRef(true);
  const main = useRef<HTMLDivElement>(null);
  const [pin, setPin] = useState<Pin | null>(null);
  const [below, setBelow] = useState(false);

  /** The last task box of the turn that holds the task in hand. */
  const openTask = (): HTMLElement | null => {
    const found = stage.current?.querySelectorAll<HTMLElement>(".exchange.holds-open div.task, .exchange.holds-open :not(div.task) > p.task");
    return found && found.length > 0 ? found[found.length - 1] : null;
  };
  const contentTop = (el: HTMLElement) => {
    const s = stage.current!;
    return el.getBoundingClientRect().top - s.getBoundingClientRect().top + s.scrollTop;
  };

  /** The scroll position that holds the whole of `task` in view, with room kept under it. Null when it already is. */
  const holding = (task: HTMLElement): number | null => {
    const s = stage.current;
    if (!s) return null;
    const top = contentTop(task);
    const bottom = top + task.offsetHeight;
    const max = Math.max(0, s.scrollHeight - s.clientHeight);
    // A task taller than the view shows its start.
    const want = Math.min(max, Math.max(0, Math.min(bottom + GAP - s.clientHeight, top - EDGE)));
    const visible = top >= s.scrollTop + EDGE && bottom + GAP <= s.scrollTop + s.clientHeight;
    return visible ? null : want;
  };

  /**
   * Where the view should be. While a reply is written the view follows its
   * end, unless the learner has scrolled up. When it settles, the whole task
   * in hand is above the answer box with room to spare, so the box's fade
   * never covers it, and the last line does not move: the room under the text
   * is the room the settling would have made.
   */
  const place = (kind: "start" | "grow" | "settle") => {
    const s = stage.current;
    const now = liveRef.current;
    if (!s || !now) return;
    const max = Math.max(0, s.scrollHeight - s.clientHeight);
    if (kind === "settle") {
      const task = openTask();
      const want = task ? holding(task) : null;
      if (want !== null) s.scrollTop = Math.max(s.scrollTop, want);
      return;
    }
    s.scrollTop = max;
  };

  /** Bring the task in hand into view above the answer box, wherever the view is. */
  const reveal = () => {
    const s = stage.current;
    const task = openTask();
    if (!s || !task) return;
    const want = holding(task);
    if (want !== null) s.scrollTop = want;
  };

  const measure = () => {
    const s = stage.current;
    if (!s) return;
    const task = openTask();
    let next: Pin | null = null;
    if (task && task.getBoundingClientRect().bottom < s.getBoundingClientRect().top + EDGE) next = summarize(task);
    setPin((old) => (old === next || (old && next && old.html === next.html && old.label === next.label && old.n === next.n) ? old : next));
    const rest = s.scrollHeight - s.clientHeight - s.scrollTop;
    setBelow(rest > 24);
    // Back at the end, the view follows the reply again.
    if (rest < 2) follow.current = true;
    if (main.current) main.current.dataset.scrolled = s.scrollTop > 0 ? "yes" : "no";
  };

  const smooth = () => (window.matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth");
  const showTask = () => {
    const task = openTask();
    if (!task || !stage.current) return;
    follow.current = false;
    stage.current.scrollTo({ top: Math.max(0, contentTop(task) - EDGE), behavior: smooth() });
  };
  const toEnd = () => {
    const s = stage.current;
    if (!s) return;
    follow.current = true;
    s.scrollTo({ top: s.scrollHeight, behavior: smooth() });
  };

  // A new exchange shows its end.
  useLayoutEffect(() => {
    follow.current = true;
    place("start");
    measure();
  }, [live?.since]);

  // The reply settles: the task is brought clear of the answer box.
  useLayoutEffect(() => {
    if (!isBusy && follow.current) place("settle");
    measure();
  }, [isBusy]);

  useLayoutEffect(() => {
    measure();
  });

  // The page grows as the reply is written, a diagram is drawn or the window
  // is resized. The view follows the end through all of it.
  useEffect(() => {
    const s = stage.current;
    const column = s?.firstElementChild;
    if (!s || !column) return;
    const watch = new ResizeObserver(() => {
      if (follow.current) place(isBusyRef.current ? "grow" : "settle");
      measure();
    });
    watch.observe(s);
    watch.observe(column);
    return () => watch.disconnect();
  }, []);

  // Opened, the earlier turns go above the exchange in hand, as in any
  // conversation. The view stays on that exchange, with the end of the turn
  // before it showing, so it is plain that there is more to scroll up to.
  useLayoutEffect(() => {
    if (!showEarlier) return;
    const el = liveRef.current;
    el?.scrollIntoView({ block: "start" });
    el?.closest(".scroll")?.scrollBy(0, -72);
  }, [showEarlier]);

  // Only a learner reading back holds the view where it is: the wheel going
  // up, a finger, or the scroll bar. A click on a card is not that.
  const held = () => {
    follow.current = false;
  };

  /** Answering a card, or deciding on one, sends the learner back to the end, where the reply will be. */
  const answer = (id: string, answers: Answer[]) => {
    follow.current = true;
    props.onAnswer(id, answers);
  };
  const decide = (id: string, decision: Decision) => {
    follow.current = true;
    props.onDecide(id, decision);
  };
  const stop = (text: string) => {
    forgetDraft();
    props.onStop(text);
  };

  return (
    <section
      className={source && !narrow ? "screen session has-source" : "screen session"}
      data-screen="session"
      data-stopping={interrupting ? "yes" : undefined}
      {...(props.overlay ? ({ inert: "" } as object) : {})}
    >
      <TopBar
        left={
          <button className="crumb" onClick={() => beginStop()} disabled={stopping} title="End this lesson" data-role="end">
            <Icon name="close" size={15} />
            End lesson
          </button>
        }
        center={<Track rail={props.rail} open={planOpen} onOpen={setPlanOpen} />}
        right={
          <>
            <Clock startedMs={state.startedMs} blockMinutes={props.blockMinutes} />
            <PaletteKey onClick={props.onPalette} />
          </>
        }
      />
      <div className={source ? "session-body with-source" : "session-body"}>
        <div className="session-main" ref={main} data-scrolled="no" {...(sheet ? ({ inert: "" } as object) : {})}>
          {pin && (
            <div className="task-pin-holder">
              <button className="task-pin" onClick={showTask} title="Show the whole task" data-role="task-pin" data-n={pin.n}>
                <span className="task-pin-label">{pin.label}</span>
                <span className="task-pin-text" dangerouslySetInnerHTML={{ __html: pin.html }} />
              </button>
            </div>
          )}
          <div
            className="scroll stage"
            ref={stage}
            onScroll={measure}
            onWheel={(e) => e.deltaY < 0 && held()}
            onTouchMove={held}
            onPointerDown={(e) => e.target === stage.current && held()}
          >
            <div className="column">
              {earlier > 0 && (
                <button className="earlier-toggle" onClick={() => setShowEarlier(!showEarlier)} data-role="earlier">
                  <Icon name="history" size={14} />
                  {showEarlier ? "Hide" : "Show"} {earlier} earlier {earlier === 1 ? "turn" : "turns"}
                </button>
              )}
              {showEarlier && (
                <div className="earlier answered">
                  {state.earlier.slice(0, earlier).map((turn, index) => (
                    <div className="exchange" key={index}>
                      {turn.learner && <Turn text={turn.learner} />}
                      <div className="tutor">
                        <Rich text={turn.tutor} final stopped={turn.interrupted} onNote={props.onNote} linksOpen={!setsTask[index] || answered(index)} onSource={openSource} taskFrom={taskStarts[index]} />
                      </div>
                    </div>
                  ))}
                </div>
              )}
              {/* One list, keyed by turn: when the live reply settles and the learner
                  answers, its element stays where it is and becomes an earlier turn,
                  so nothing already on screen moves when the next reply starts. */}
              {[
                ...carried.map((turn, n) => {
                const index = from + n;
                // The turn that holds the task in hand is live until the tutor sets another.
                const open = n === 0 && !liveSetsTask;
                return (
                  <div className={`exchange carried${open ? " holds-open" : " answered"}`} key={index}>
                    {exchange(turn, index, { writing: false })}
                  </div>
                );
              }),
              <div className={liveSetsTask ? "exchange now holds-open" : "exchange now"} ref={liveRef} key={state.earlier.length}>
                {live && exchange({ learner: live.learner, tutor: text, interrupted: live.interrupted }, state.earlier.length, { writing: isBusy })}
                {!live && <p className="hint">Say what you want to work on.</p>}
                {live?.interrupted && !isBusy && <div className="stopped">Stopped by you</div>}
                {live?.error && !isBusy && <div className="banner bad selectable">{live.error}</div>}
              </div>,
              ]}
              {state.cards.map((card) =>
                card.kind === "permission" ? (
                  <PermissionCard key={card.id} card={card} onDecide={decide} />
                ) : (
                  <QuestionCard key={card.id} card={card} onAnswer={answer} />
                ),
              )}
              {state.notices.map((notice) => (
                <div className="banner warn selectable" key={notice.text}>
                  <span>{notice.text}</span>
                  <button className="icon-btn" aria-label="Dismiss" onClick={() => props.onDismiss(notice.text)}>
                    <Icon name="close" size={14} />
                  </button>
                </div>
              ))}
            </div>
          </div>
          {/* The end sheet, the nudge and a note about material stand above the dock, not in it: the answer box is on the foot of the window and nothing moves it. */}
          {(stopping || (blockOver && !nudged && !isBusy) || props.material) && (
            <div className="dock-notes">
              <div className="column">
              {stopping && (
                <div
                  className="sheet"
                  role="dialog"
                  aria-label="End this lesson"
                  data-role="stop-sheet"
                  onKeyDown={(e) => {
                    if (e.key === "Escape" && !composing(e)) {
                      e.preventDefault();
                      e.stopPropagation();
                      setStopping(false);
                    }
                  }}
                >
                  <h3>Where did you stop?</h3>
                  <input
                    ref={stopInput}
                    className="field"
                    value={line}
                    aria-label="Where did you stop?"
                    placeholder="One line: where you are, and what comes next"
                    onChange={(e) => setLine(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" && !composing(e)) stop(line);
                    }}
                  />
                  <div className="row">
                    <button className="btn primary" title="Save and end the lesson (Return)" onClick={() => stop(line)}>
                      Save and end the lesson
                    </button>
                    <button className="btn ghost" ref={keepGoing} onClick={() => setStopping(false)}>
                      Keep going
                    </button>
                    <span className="aside">Your next lesson opens on this line.</span>
                  </div>
                </div>
              )}
              {!stopping && blockOver && !nudged && !isBusy && (
                <div className="banner nudge">
                  <span>
                    <b>{props.blockMinutes} minutes done.</b> A good place to stop?
                  </span>
                  <span className="grow" />
                  <button className="btn small" onClick={() => beginStop()}>
                    End here
                  </button>
                  <button className="btn ghost small" onClick={() => setNudged(true)}>
                    Keep going
                  </button>
                </div>
              )}
              {!stopping && props.material && <MaterialNote lines={props.material} onDismiss={props.onDismissMaterial} />}
              </div>
            </div>
          )}
          <div className="dock" key={state.cards.length === 0 ? "answer" : "card"}>
            {/* A band whose room is always kept, between the transcript and the answer box. It holds the passages the last reply cited and the cue that there is more below, so neither covers the text or moves anything. */}
            <div className="band">
              <div className="column band-row">
                {showPassages && (
                  <div className="passages" data-role="passages">
                    <span className="lesson-label passages-label">Passages from the last reply</span>
                    {passages.slice(0, 4).map((passage) => (
                      <button
                        key={passage.href}
                        className={source?.ref.path === sourceRef(passage.href)?.path ? "passage on" : "passage"}
                        data-role="source-chip"
                        onClick={() => {
                          const ref = sourceRef(passage.href);
                          if (ref) openSource(ref);
                        }}
                      >
                        <Icon name="paper" size={13} />
                        <span className="passage-text">{passage.text}</span>
                      </button>
                    ))}
                  </div>
                )}
                {below && !stopping && !props.material && !(blockOver && !nudged && !isBusy) && (
                  <button className="more-below" onClick={toEnd}>
                    <Icon name="down" size={14} />
                    More below
                  </button>
                )}
              </div>
            </div>
            <div className="column">
              {state.cards.length === 0 && (
                // Kept while the End sheet is up, so what was typed is still there after "Keep going".
                <div hidden={stopping} onFocus={reveal} {...(covered ? ({ inert: "" } as object) : {})}>
                  <Composer
                    disabled={isBusy}
                    paused={covered}
                    draftKey={`${state.earlier.length}:${live?.since ?? 0}`}
                    onSend={props.onSend}
                    onNote={props.onNote}
                    onAddFiles={props.onAddFiles}
                    onPasteText={props.onPasteText}
                    working={
                      live && isBusy ? (
                        <Thinking
                          since={live.since}
                          status={live.status}
                          doing={live.doing}
                          quiet={text !== "" && live.status !== "working"}
                          stopping={interrupting}
                          onInterrupt={stopReply}
                        />
                      ) : null
                    }
                  />
                </div>
              )}
            </div>
          </div>
        </div>
        {source && <SourcePanel source={source.ref} sheet={narrow} onClose={() => setSource(null)} onNote={props.onNote} />}
      </div>
    </section>
  );
}
