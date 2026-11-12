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
import { HELP, busy, isHelp, isHint, opening, splitConfidence, tutorText, type Card, type SessionState } from "../lesson";
import { hasMath, hasTask, renderPlain, sourceLinks } from "../render";
import { marksFor, plainSaid, skillTex, useMarks } from "../marks";
import { sourceRef, type SourceRef } from "../sources";
import { Line, Rich } from "../ui/Rich";
import { MaterialNote } from "../ui/Material";
import { SourcePanel } from "../ui/SourcePanel";
import { Icon } from "../ui/icons";
import { PaletteKey, TopBar, composing, splitGoal, useNow } from "../ui/bits";
import type { Answer, Decision, Question, Rail as RailData } from "../types";

/** What "How sure?" is for, said where the short label is shown and on the control. */
const SURE_FOR = "Optional. Helps choose what to review.";

const CONFIDENCE: [string, string][] = [
  ["sure", "Sure"],
  ["unsure", "Unsure"],
  ["guess", "Guess"],
];

/** How far from the top of the view a thing is brought to, and the room kept under the task, in pixels. */
const EDGE = 16;
const GAP = 32;

/**
 * What the learner typed, with its mathematics drawn, inside the answer box: at
 * most three lines at the UI size, scrolling inside itself, with a fade at the
 * foot while there is more below.
 */
function Preview({ text }: { text: string }) {
  const el = useRef<HTMLDivElement>(null);
  const [more, setMore] = useState(false);
  const measure = () => {
    const node = el.current;
    if (node) setMore(node.scrollHeight - node.clientHeight - node.scrollTop > 2);
  };
  useLayoutEffect(measure, [text]);
  return (
    <div className="preview" ref={el} data-more={more ? "yes" : undefined} onScroll={measure} aria-label="Your formula, drawn">
      {hasMath(text) ? <Said text={text} className="preview-body" /> : <span className="preview-empty">Your formula is drawn here</span>}
    </div>
  );
}

/** The learner's words as typed, with their mathematics drawn. */
function Said({ text, className }: { text: string; className: string }) {
  const html = useMemo(() => renderPlain(text), [text]);
  return <div className={className} dangerouslySetInnerHTML={{ __html: html }} />;
}

/**
 * What the learner said in one turn. The line the window sent to open the
 * lesson is not theirs, so it is one quiet line and not a bubble. How sure
 * they said they were is a small tag after their words, not part of them.
 *
 * An examiner marks an answer in the margin, beside it. Where the page has a
 * margin, the note there says "Your answer" and, once the runtime's record has
 * a mark for it, a second line under that says the mark; the note is out of
 * the flow, so nothing moves when it arrives. A request for help ("Can I have a
 * hint?") is not an answer: its note says "You asked" and it is never marked.
 * In a narrow window, with no margin, the whole sentence stands under the
 * answer, in the flow, and the page makes up for the room it takes.
 */
function Turn({ text, since }: { text: string; since: number }) {
  const opener = opening(text);
  const said = useMemo(() => splitConfidence(text), [text]);
  const html = useMemo(() => renderPlain(said.text), [said.text]);
  const help = isHelp(text);
  const marks = help || opener ? [] : marksFor(since);
  const sentence = marks.map(plainSaid).join(" ");
  if (!opener) {
    return (
      <div className={help ? "you selectable asked" : "you selectable"}>
        {/* A note in the margin where the margin is wide; narrower, the indent and the bar say it. */}
        <span className="lesson-label you-label" aria-hidden="true" title={sentence === "" ? undefined : sentence}>
          <span className="you-who">{help ? "You asked" : "Your answer"}</span>
          {marks.map((mark, n) => (
            <span className={mark.right ? "you-mark right" : "you-mark"} key={n}>
              {mark.word}
            </span>
          ))}
        </span>
        <span className="you-body" dangerouslySetInnerHTML={{ __html: html }} />
        {said.confidence && (
          <span className="you-tag" title="How sure you said you were">
            {said.confidence}
          </span>
        )}
        {/* Said once, politely, when the record has its mark. The region is there before the words are. */}
        {!help && (
          <span className="sr" role="status" aria-live="polite" data-role="marks" data-state={sentence === "" ? undefined : "said"}>
            {sentence}
          </span>
        )}
        {/* In a narrow window the sentence is under the answer. Where there is a margin it is not drawn. */}
        {marks.length > 0 && (
          <div className="you-verdicts" aria-hidden="true">
            {marks.map((mark, n) => (
              <p className={mark.right ? "you-verdict right" : "you-verdict"} data-added={mark.added} key={n}>
                {mark.before}
                {mark.skill && <Line text={skillTex(mark.skill)} />}
                {mark.after}
              </p>
            ))}
          </div>
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

/**
 * A fault in the lesson (the usage limit, the tutor's program stopping, an
 * error), built as a fault is on every other screen: a red "!" at the left of
 * the first line, the title in the UI face 600, the explanation in the quiet
 * ink, and the one action (here the dismiss ×, in ink) at the right. The
 * runtime's words are one run of text, so the first sentence is the title and
 * the rest the explanation; nothing is added. After the merge with the shared
 * `Fault` in bits.tsx this is one line: `<Fault title={...} onDismiss={...}>`.
 */
function LessonFault({ text, onDismiss, alert }: { text: string; onDismiss?: () => void; alert?: boolean }) {
  const cut = /^(.+?[.!?])\s+(\S[\s\S]*)$/.exec(text.trim());
  const title = cut ? cut[1] : text.trim();
  const rest = cut ? cut[2] : "";
  return (
    <div className="banner bad fault selectable" role={alert ? "alert" : undefined}>
      <Icon name="bang" size={16} />
      <span className="fault-text">
        <b className="fault-title">{title}</b>
        {rest !== "" && <span className="fault-why">{rest}</span>}
      </span>
      {onDismiss && (
        <button className="icon-btn" aria-label="Dismiss" onClick={onDismiss}>
          <Icon name="close" size={14} />
        </button>
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

/** Whether the learner is using the keyboard (the window marks it as the focus rings do). */
const byKeyboard = () => document.documentElement.dataset.input === "key";

/** The goal's steps as a row of marks, and the step in hand in words. A click shows them all. */
function Track(props: { rail: RailData | null; open: boolean; onOpen: (open: boolean) => void; startedMs: number | null; blockMinutes: number }) {
  const { rail, open } = props;
  const left = useMinutesLeft(props.startedMs, props.blockMinutes);
  const time = `Lesson length ${props.blockMinutes} min, ${left < 0 ? `${-left} min over` : `${left} min left`}`;
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
      if (event.key !== "Escape") return;
      props.onOpen(false);
      // The step button opened it, and a keyboard learner is still on it: it keeps the focus.
      holder.current?.querySelector<HTMLButtonElement>("button.track")?.focus();
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
        title={`${where ? `${where}. ` : ""}${time}. ${steps.length > 0 ? "Show the goal and its steps." : "Show the goal."}`}
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
                  <i aria-hidden="true" />
                  <Line text={step.step} />
                  <span className="sr">{step.status === "done" ? ", done" : index === current ? ", in hand" : ""}</span>
                </li>
              ))}
            </ol>
          ) : (
            rail?.next_action && (
              <ol>
                <li className="cur">
                  <i aria-hidden="true" />
                  <Line text={rail.next_action} />
                </li>
              </ol>
            )
          )}
          <p className="plan-time">{time}</p>
        </div>
      )}
    </div>
  );
}

/** Minutes left of the lesson's length; below zero once it is over. */
function useMinutesLeft(startedMs: number | null, blockMinutes: number): number {
  const now = useNow(15_000);
  const used = startedMs === null ? 0 : Math.max(0, Math.floor((now - startedMs) / 60_000));
  return blockMinutes - used;
}

/** The last stretch of the lesson, in minutes: only then does the strip count the time. */
const CLOCK_SHOWN = 10;

/**
 * The block's clock as plain text. A clock all lesson long is pressure, so the
 * strip is silent about time until the last ten minutes; before then the time
 * is in the step button's popover and its tooltip.
 */
function Clock({ startedMs, blockMinutes }: { startedMs: number | null; blockMinutes: number }) {
  const left = useMinutesLeft(startedMs, blockMinutes);
  if (left > CLOCK_SHOWN) return null;
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
          <button className="btn" onClick={() => props.onDecide(card.id, "allow_always")}>
            Always allow this
          </button>
        )}
      </div>
    </div>
  );
}

/**
 * The tutor's question, with its options. The field for the learner's own
 * words has the focus when it appears, so typing types: a digit typed there is
 * text (an answer often starts with one), and a single digit that numbers an
 * option is taken for the option only when Return follows it. The options are
 * numbered 1, 2, 3. An up arrow (or, with the field empty, a down arrow) moves
 * from the field into them. Inside them the keys do what a click does: a
 * number sends that option at once, as a click on it does; the arrows move
 * (and choose, in a group of one answer) and Return sends the one moved to. A
 * box that allows several is the exception, for it has no one answer to send:
 * a number ticks, and Return sends the ticked. A letter typed among the options
 * goes back to the field and is typed.
 */
function QuestionCard(props: {
  card: Extract<Card, { kind: "question" }>;
  onAnswer: (id: string, answers: Answer[]) => void;
  /** Where the card puts what Esc does to it, for the page to call when the focus is not in the card. */
  escape: React.MutableRefObject<(() => void) | null>;
}) {
  const { card } = props;
  const [chosen, setChosen] = useState<Record<number, string[]>>({});
  const [own, setOwn] = useState<Record<number, string>>({});
  const root = useRef<HTMLDivElement>(null);
  const single = card.questions.length === 1 && !card.questions[0].multi;

  useEffect(() => {
    root.current?.querySelector<HTMLInputElement>("input.field")?.focus();
  }, [card.id]);

  const answersFrom = (pick: Record<number, string[]>, words: Record<number, string>): Answer[] =>
    card.questions.map((q, index) => {
      const typed = (words[index] ?? "").trim();
      return { question: q.question, chosen: typed !== "" ? [...(pick[index] ?? []), typed] : (pick[index] ?? []) };
    });
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

  /** The learner's own words for a question. Typing takes back an option chosen, so the words are the answer. */
  const typeInto = (index: number, value: string) => {
    setOwn((old) => ({ ...old, [index]: value }));
    if (value !== "") setChosen((old) => (old[index]?.length ? { ...old, [index]: [] } : old));
  };

  /**
   * An option picked by a click or by its number inside the options: for a
   * single question it is also the answer, and sent at once. With several
   * questions it is that question's answer, and the answer goes once every
   * question has one; until then the focus moves on to the next. A box that
   * allows several only ticks it.
   */
  const pick = (index: number, q: Question, label: string) => {
    if (q.multi) return choose(index, q, label);
    choose(index, q, label);
    if (single || complete({ index, label })) props.onAnswer(card.id, answersNow({ index, label }));
    else field(index + 1)?.focus();
  };

  /**
   * Return in the field with one digit in it that numbers an option: the digit
   * was a choice, not words. The option is chosen and the field emptied; for a
   * question of one answer that is also the answer, and it is sent once every
   * question has one. Any other text stays the learner's own words (an answer
   * such as "1/v" starts with a digit).
   */
  const numbered = (index: number, q: Question, typed: string): boolean => {
    const n = /^[1-9]$/.test(typed) ? Number(typed) - 1 : -1;
    if (n < 0 || n >= q.options.length) return false;
    const label = q.options[n].label;
    const had = chosen[index] ?? [];
    const next = q.multi ? (had.includes(label) ? had.filter((l) => l !== label) : [...had, label]) : [label];
    const pick = { ...chosen, [index]: next };
    const words = { ...own, [index]: "" };
    setChosen(pick);
    setOwn(words);
    const answers = answersFrom(pick, words);
    // A box that allows several waits for Return on an empty field; one of one answer goes.
    if (!q.multi && answers.every((a) => a.chosen.length > 0)) props.onAnswer(card.id, answers);
    else if (!q.multi) field(index + 1)?.focus();
    return true;
  };

  /**
   * Esc on a card: the first takes back the choice made, and the focus goes to
   * the field for the learner's own words; with no choice, it goes to the field.
   * It never goes on to the page, so it cannot open a second card.
   */
  const back = (from?: number) => {
    if (Object.values(chosen).some((labels) => labels.length > 0)) setChosen({});
    const at = from ?? 0;
    const box = root.current?.querySelector<HTMLInputElement>(`.ask-q[data-q="${at}"] input.field`);
    if (box && document.activeElement !== box) box.focus();
  };
  props.escape.current = () => back();
  useEffect(() => {
    const slot = props.escape;
    return () => {
      slot.current = null;
    };
  }, [props.escape]);

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

    if (event.key === "Escape") {
      event.preventDefault();
      back(index);
      return;
    }

    if (target.tagName === "INPUT") {
      // From the field, up goes to the options (and so does down, with nothing typed): the one chosen, else the first.
      if ((event.key === "ArrowUp" || (event.key === "ArrowDown" && (own[index] ?? "") === "")) && list.length > 0) {
        event.preventDefault();
        (list.find((el) => el.getAttribute("aria-checked") === "true") ?? list[0]).focus();
      }
      // Anything else typed in the field is text: a digit too, since an answer often starts with one.
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
      pick(index, q, label);
      return;
    }

    // A number among the options does what a click on that option does.
    if (digit >= 0 && digit < q.options.length) {
      event.preventDefault();
      list[digit]?.focus();
      pick(index, q, q.options[digit].label);
      return;
    }

    // Any other character is the start of the learner's own words.
    if (event.key.length === 1) {
      event.preventDefault();
      const box = field(index);
      typeInto(index, `${own[index] ?? ""}${event.key}`);
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
                    title={q.multi ? `${n + 1} ticks it. Return sends the ticked ones.` : `${n + 1} sends it. Return sends the one moved to.`}
                    onClick={() => pick(index, q, option.label)}
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
              onChange={(e) => typeInto(index, e.target.value)}
              onKeyDown={(e) => {
                if (e.key !== "Enter" || composing(e)) return;
                if (numbered(index, q, own[index] ?? "")) e.preventDefault();
                else if (complete()) props.onAnswer(card.id, answersNow());
              }}
            />
          </div>
        );
      })}
      <div className="row">
        <span className="aside">Click an option, or press ↑ and then its number to send it. Or write it in your own words.</span>
        <button className="btn primary send" disabled={!complete()} title="Send the answer (Return)" onClick={() => props.onAnswer(card.id, answersNow())}>
          Send
        </button>
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

/**
 * The symbols the course's answers most often need. Each is written as the
 * character itself, which the tutor reads as it is and which draws inside a
 * formula too; only the fraction has no character, so it is LaTeX, with the
 * caret between its first braces.
 */
const SYMBOLS: { show: string; name: string; insert: string; tex?: boolean; hint?: string }[] = [
  { show: "x̄", name: "x bar, the mean", insert: "x̄" },
  { show: "σ", name: "sigma", insert: "σ" },
  { show: "μ", name: "mu", insert: "μ" },
  { show: "√", name: "square root", insert: "√" },
  // Subscript and superscript are LaTeX: put inside dollar signs when the caret is not already in a formula, with the caret in the braces.
  { show: "x₂", name: "Subscript", insert: "_{|}", tex: true, hint: "Subscript" },
  { show: "x²", name: "Superscript", insert: "^{|}", tex: true, hint: "Superscript" },
  { show: "≤", name: "less than or equal to", insert: "≤" },
  { show: "≥", name: "greater than or equal to", insert: "≥" },
  { show: "±", name: "plus or minus", insert: "±" },
  { show: "×", name: "times", insert: "×" },
  { show: "÷", name: "divided by", insert: "÷" },
  { show: "∑", name: "sum", insert: "∑" },
  { show: "∞", name: "infinity", insert: "∞" },
  { show: "≠", name: "not equal to", insert: "≠" },
  { show: "≈", name: "approximately equal to", insert: "≈" },
  { show: "\\frac{a}{b}", name: "fraction", insert: "\\frac{|}{}", tex: true },
];

/** What the fraction button does, said where it is pointed at: it types LaTeX, which the preview draws. */
const FRACTION_HINT = "Fraction: types \\frac{top}{bottom}; the preview shows it drawn";

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

/**
 * How sure the learner is: optional, one choice of three, and about the answer.
 * It is sent with the answer, so it cannot be asked after Send without changing
 * what is sent; and it must not appear mid-sentence and change the box while the
 * learner types. So it is always in the box's foot, quiet, and says what it is
 * for. A radio group with one tab stop. The arrows move and choose; Space and
 * Return choose the one in hand and never clear it or move the focus; a click on
 * the chosen one clears it, the only way to. ⌥1, ⌥2 and ⌥3 choose from the answer box.
 */
function Sure(props: { value: string | null; onChange: (value: string | null, fromKey: boolean) => void }) {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const at = CONFIDENCE.findIndex(([word]) => word === props.value);
  const stop = at >= 0 ? at : 0;
  const key = (event: React.KeyboardEvent, from: number) => {
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    if (event.key === " " || event.key === "Enter") {
      // A key chooses. Left to itself the button would click, and a click on the chosen one clears it.
      event.preventDefault();
      event.stopPropagation();
      props.onChange(CONFIDENCE[from][0], true);
      return;
    }
    const step = event.key === "ArrowRight" || event.key === "ArrowDown" ? 1 : event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 0;
    if (step === 0) return;
    event.preventDefault();
    const next = (from + step + CONFIDENCE.length) % CONFIDENCE.length;
    props.onChange(CONFIDENCE[next][0], true);
    refs.current[next]?.focus();
  };
  return (
    <span className="sure">
      <span className="sure-label" aria-hidden="true" title={SURE_FOR}>
        <span className="sure-long">How sure? (optional, helps choose what to review)</span>
        <span className="sure-short">How sure?</span>
      </span>
      <span
        className="seg"
        role="radiogroup"
        aria-label="How sure are you? Optional."
        title={`${SURE_FOR} Return sends without it.`}
      >
        {CONFIDENCE.map(([word, label], n) => (
          <button
            key={word}
            ref={(el) => {
              refs.current[n] = el;
            }}
            type="button"
            role="radio"
            aria-checked={props.value === word}
            aria-keyshortcuts={`Alt+${n + 1}`}
            tabIndex={n === stop ? 0 : -1}
            data-label={label}
            title={`${label} (⌥${n + 1}). Click it again to clear.`}
            onKeyDown={(e) => key(e, n)}
            onKeyUp={(e) => {
              // The key was taken on the way down; the button must not click on the way up.
              if (e.key === " ") e.preventDefault();
            }}
            onClick={() => props.onChange(props.value === word ? null : word, false)}
          >
            {label}
          </button>
        ))}
      </span>
    </span>
  );
}

/**
 * The one button for what the learner adds: a photo of their work, or course
 * material. It opens a short menu. Return, Space or the arrows open it and
 * move in it, Esc closes it and gives the focus back, and Tab leaves it.
 */
function Attach(props: { onPhoto: () => void; onMaterial: () => void }) {
  const [open, setOpen] = useState(false);
  const holder = useRef<HTMLSpanElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const items = useRef<(HTMLButtonElement | null)[]>([]);
  const first = useRef<0 | 1>(0);

  useEffect(() => {
    if (!open) return;
    items.current[first.current === 1 ? items.current.length - 1 : 0]?.focus();
    const close = (event: MouseEvent) => {
      if (!holder.current?.contains(event.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [open]);

  const choose = (run: () => void) => {
    setOpen(false);
    // The menu is gone: the focus goes back to its button for a keyboard learner, to the answer box for a mouse.
    if (byKeyboard()) button.current?.focus();
    else document.querySelector<HTMLTextAreaElement>(".composer textarea")?.focus();
    run();
  };
  const onKeyDown = (event: React.KeyboardEvent) => {
    if (event.metaKey || event.ctrlKey || event.altKey || composing(event)) return;
    if (!open) {
      if (event.target === button.current && (event.key === "ArrowDown" || event.key === "ArrowUp")) {
        event.preventDefault();
        first.current = event.key === "ArrowUp" ? 1 : 0;
        setOpen(true);
      }
      return;
    }
    const list = items.current.filter((el): el is HTMLButtonElement => el !== null);
    const at = list.indexOf(document.activeElement as HTMLButtonElement);
    if (event.key === "Escape") {
      // Takes the key: the lesson's own Esc must not also run.
      event.preventDefault();
      event.stopPropagation();
      setOpen(false);
      button.current?.focus();
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      list[(Math.max(at, 0) + (event.key === "ArrowDown" ? 1 : -1) + list.length) % list.length]?.focus();
    } else if (event.key === "Home" || event.key === "End") {
      event.preventDefault();
      list[event.key === "Home" ? 0 : list.length - 1]?.focus();
    } else if (event.key === "Tab") {
      setOpen(false);
    }
  };
  return (
    <span className="add-menu" ref={holder} onKeyDown={onKeyDown}>
      <button
        ref={button}
        type="button"
        className={open ? "icon-btn on" : "icon-btn"}
        title="Attach a photo of your work or course material"
        aria-label="Attach a photo of your work or course material"
        aria-haspopup="menu"
        aria-expanded={open}
        data-role="add-material"
        onClick={() => {
          first.current = 0;
          setOpen(!open);
        }}
      >
        <Icon name="clip" size={16} />
      </button>
      {open && (
        <span className="menu" role="menu" aria-label="Attach">
          <button
            type="button"
            role="menuitem"
            ref={(el) => {
              items.current[0] = el;
            }}
            onClick={() => choose(props.onPhoto)}
          >
            Photo of your work…
          </button>
          <button
            type="button"
            role="menuitem"
            ref={(el) => {
              items.current[1] = el;
            }}
            onClick={() => choose(props.onMaterial)}
          >
            Course material…
          </button>
        </span>
      )}
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
    // Measuring takes the box to its natural height for a moment, which gives
    // the lesson above it more room; WebKit then pulls the lesson's view up to
    // fit, and the view would not come back. The view is put where it was.
    const stage = document.querySelector<HTMLElement>(".stage");
    const kept = stage?.scrollTop ?? 0;
    el.style.height = "auto";
    // The page's CSS holds the whole box (text and preview) to a share of the window; the text scrolls inside it.
    el.style.height = `${el.scrollHeight}px`;
    if (stage && stage.scrollTop !== kept) stage.scrollTop = kept;
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
    // A keyboard learner who opened the steps from their button is still on it, and keeps it.
    const onOpener = byKeyboard() && active instanceof HTMLButtonElement;
    const back = wasPaused.current && !(active instanceof HTMLInputElement || active instanceof HTMLTextAreaElement) && !onOpener;
    wasPaused.current = false;
    if (free || back) box.current?.focus();
  }, [props.paused, props.disabled]);

  // Dollar signs and spaces alone are not an answer.
  const content = !bare(text) || files.length > 0;
  // How sure goes with words; a photo alone has nothing to be sure of.
  const typed = !bare(text);

  const send = () => {
    if (props.paused || !content) return;
    if (props.disabled) {
      setQueued(true);
      return;
    }
    const parts = [text.trim()];
    if (confidence && typed) parts.push(`Confidence: ${confidence}`);
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
    box.current?.focus();
  };

  /**
   * Write a symbol at the caret. A character goes in as it is, wherever the
   * caret is. The fraction is LaTeX: inside a formula already begun it goes in
   * as it is; outside one it goes between dollar signs, with the caret between
   * its first braces, and a selection becomes its numerator.
   */
  const insert = (symbol: (typeof SYMBOLS)[number]) => {
    const el = box.current;
    if (!el) return;
    if (!symbol.tex) {
      const at = el.selectionEnd;
      caretAt.current = at + symbol.insert.length;
      setText(`${text.slice(0, at)}${symbol.insert}${text.slice(at)}`);
      return;
    }
    const tex = symbol.insert;
    const mark = tex.indexOf("|");
    const picked = text.slice(el.selectionStart, el.selectionEnd);
    const from = el.selectionStart;
    const to = el.selectionEnd;
    const before = text.slice(0, from);
    const snippet = `${tex.slice(0, mark)}${picked}${tex.slice(mark + 1)}`;
    // A subscript or a superscript straight after a formula just closed belongs to that formula: it takes the closing dollar sign back, so two formulas never touch.
    if (symbol.hint && picked === "" && !inMath(before) && before.endsWith("$") && !before.endsWith("$$")) {
      const base = before.slice(0, -1);
      caretAt.current = base.length + mark;
      setText(`${base}${snippet}$${text.slice(to)}`);
      return;
    }
    const wrap = inMath(before) ? "" : "$";
    // The caret goes in the first braces, or, for a selection taken as the numerator, into the denominator.
    const caret = picked === "" ? mark : snippet.length - 1;
    caretAt.current = from + wrap.length + caret;
    setText(`${before}${wrap}${snippet}${wrap}${text.slice(to)}`);
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
          return;
        }
        // ⌥1, ⌥2, ⌥3 say how sure, from the answer box.
        // The key's code is read, for ⌥1 types a character of its own on a Mac.
        const pick = /^Digit([123])$/.exec(e.code);
        if (pick && e.altKey && !e.metaKey && !e.ctrlKey && !e.shiftKey && !composing(e)) {
          e.preventDefault();
          setConfidence(CONFIDENCE[Number(pick[1]) - 1][0]);
        }
      }}
    >
      <div className="box">
        <textarea
          ref={box}
          rows={1}
          placeholder="Your answer. ⇧Return starts a new line."
          aria-label="Your answer"
          value={text}
          autoFocus
          spellCheck={false}
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
          <Preview text={preview} />
        )}
        {files.length > 0 && (
          <div className="files">
            {files.map((file) => (
              <span className="file" key={file}>
                <Icon name="camera" size={14} />
                {file.split("/").pop()}
                <button aria-label="Remove" onClick={() => setFiles((old) => old.filter((f) => f !== file))}>
                  <Icon name="close" size={14} />
                </button>
              </span>
            ))}
          </div>
        )}
        <div className="tools">
          <Attach onPhoto={addPhoto} onMaterial={props.onAddFiles} />
          <button
            className={symbols ? "icon-btn on" : "icon-btn"}
            title="Symbols for mathematics"
            aria-label="Symbols for mathematics"
            aria-expanded={symbols}
            aria-controls="symbol-row"
            onClick={() => {
              if (symbols) {
                // Closed from its own button: a keyboard learner stays on it.
                if (byKeyboard()) setSymbols(false);
                else closeSymbols();
                return;
              }
              setSymbols(true);
              // Opened from the keyboard, the focus stays on the button, and Tab goes into the row; a click puts the caret back in the box.
              if (!byKeyboard()) box.current?.focus();
            }}
          >
            <Icon name="sigma" size={16} />
          </button>
          {symbols && (
            <div className="symbols" id="symbol-row" role="toolbar" aria-label="Symbols for mathematics">
              {SYMBOLS.map((symbol) => (
                <button
                  key={symbol.name}
                  type="button"
                  className="symbol"
                  title={symbol.hint ?? (symbol.tex ? FRACTION_HINT : symbol.name)}
                  aria-label={symbol.hint ?? (symbol.tex ? FRACTION_HINT : symbol.name)}
                  // The caret stays in the answer box: a click on a symbol never takes it.
                  onMouseDown={(e) => e.preventDefault()}
                  onClick={() => insert(symbol)}
                >
                  {symbol.tex && !symbol.hint ? <Line text={`$${symbol.show}$`} /> : symbol.show}
                </button>
              ))}
            </div>
          )}
          <span className="grow" />
          {queued && (
            <span className="queued" role="status">
              Sends when the tutor finishes
            </span>
          )}
          {/* While an answer waits for the tutor, its note has the row, whole; the question returns after. */}
          {!queued && (
            <Sure
              value={confidence}
              onChange={(value, fromKey) => {
                setConfidence(value);
                if (!fromKey) box.current?.focus();
              }}
            />
          )}
          <button
            className="btn primary send"
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
          title={props.disabled ? why : "Hint (⌘⇧H). An answer given with a hint won't count as a proof today."}
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
  /** The tutor's turn has begun, so there is something to stop. */
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
        Stop
      </button>
    </div>
  );
}

/** The task in the strip: its label, and its first lines as running text. */
type Pin = { label: string; html: string };

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
  return { label, html: parts.join(" ") };
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
  // Return was pressed down in the End card: the lesson ends when the key comes up, so the key's own press never reaches the screen that follows.
  const endArmed = useRef(false);
  const now = useNow(20_000);
  const cardEscape = useRef<(() => void) | null>(null);
  // What the runtime's record says of each answer sent here: read before it is sent, and again once the reply is whole.
  const { arm, version: marked } = useMarks(live?.since ?? null, live !== null && !isBusy);
  const sendAnswer = (words: string) => {
    // A request for help is not an answer: the record has no mark to wait for.
    if (!(Object.values(HELP) as string[]).includes(words.trim())) arm();
    props.onSend(words);
  };

  // A source a reply cited opens beside the lesson, but only after the learner
  // has answered that reply's task, and it closes when the tutor poses the
  // next one. Until then the link reads as plain text. A reply that sets no
  // task has nothing to answer, so its links are open.
  const [source, setSource] = useState<{ ref: SourceRef; posed: number } | null>(null);
  const sheet = source !== null && narrow;
  // Anything that takes the keyboard from the answer box.
  const covered = (props.overlay ?? false) || planOpen || stopping || sheet;

  // The End card is a confirmation: its field has the focus, so Return does
  // its main action (End lesson) and Esc means "Keep going".
  const beginStop = () => {
    setPlanOpen(false);
    setLine(props.rail?.next_action ?? "");
    setStopping(true);
    window.setTimeout(() => stopInput.current?.focus(), 0);
  };

  // Esc closes the innermost thing first: the symbol row, a popover, a source
  // (each takes the key and says so). With a card on screen (a question, or a
  // request for permission) it never opens the End sheet: a question takes back
  // its choice, then moves the focus to its field. Only with nothing open does
  // it open the End sheet, with its field in hand: Return there ends the lesson
  // (the sheet is a confirmation, and Return does its main action) and Esc
  // keeps going. It does this the same way whether or not the answer box holds
  // a draft: the draft is kept. It never ends a lesson on Esc.
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented || event.isComposing) return;
      if (covered || document.querySelector(".add-menu .menu")) return;
      event.preventDefault();
      // A source beside the lesson, or over it, is shut before anything else is.
      if (source) closeSource(true);
      // A card on screen takes the key: it never opens the End sheet beside it.
      else if (state.cards.length > 0) cardEscape.current?.();
      else beginStop();
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  });

  // ⌘. stops the reply (App sends it to the program). The working line shows
  // "Stopping" in the same frame, not when the program has answered.
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey) || event.key !== "." || !isBusyRef.current) return;
      setInterrupting(true);
    };
    window.addEventListener("keydown", key, true);
    return () => window.removeEventListener("keydown", key, true);
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

  // Which replies set a task. The page does not number them: the tutor's own
  // label ("Task", "Check 1 of 2") and the rule in the margin say which is live.
  const setsTask = useMemo(() => state.earlier.map((turn) => hasTask(turn.tutor)), [state.earlier]);
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
  // What opened the source, so closing it gives the focus back there.
  const opener = useRef<{ el: HTMLElement | null; href: string | null } | null>(null);
  const openSource = (ref: SourceRef, from?: HTMLElement | null) => {
    opener.current = { el: from ?? null, href: from?.getAttribute("href") ?? null };
    setSource({ ref, posed });
  };
  /** Shut the source. When the learner shut it, the focus goes back to the link or chip that opened it, else to the answer box. */
  const closeSource = (restore: boolean) => {
    setSource(null);
    if (!restore) return;
    const from = opener.current;
    window.requestAnimationFrame(() => {
      let el = from?.el && from.el.isConnected ? from.el : null;
      // The reply was drawn again and the link with it: find the same link by where it points.
      if (!el && from?.href) el = Array.from(document.querySelectorAll<HTMLElement>("a.source")).find((a) => a.getAttribute("href") === from.href) ?? null;
      if (!el) el = document.querySelector<HTMLElement>('[data-role="source-chip"]');
      (el ?? document.querySelector<HTMLTextAreaElement>(".composer textarea"))?.focus();
    });
  };
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
  const exchange = (turn: { learner: string; tutor: string; interrupted: boolean; ms: number }, index: number, opts: { writing: boolean }) => {
    const hint = isHint(turn.learner) && turn.tutor !== "";
    return (
      <>
        {turn.learner && <Turn text={turn.learner} since={turn.ms} />}
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
  // Every move of the view is a plain write of scrollTop, never a smooth scroll
  // or scrollIntoView: WebKit (the real window) times those differently from
  // Chrome. The position written is remembered, so a scroll event that goes
  // above it is the learner's, and the page's own writes never read as theirs.
  const mine = useRef(0);
  // How tall the page was at the last look. A page that gets shorter (a card
  // answered and gone) pulls the view up with it: that is not the learner
  // scrolling up, so it must not stop the view following the reply.
  const tall = useRef(0);
  const setTop = (top: number) => {
    const s = stage.current;
    if (!s) return;
    s.scrollTop = top;
    mine.current = s.scrollTop;
  };

  // A card arriving takes the view to the end by a short glide, not in one frame.
  const gliding = useRef(false);
  const glide = () => {
    const s = stage.current;
    if (!s) return;
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return setTop(s.scrollHeight);
    const from = s.scrollTop;
    const began = performance.now();
    gliding.current = true;
    const step = (at: number) => {
      const el = stage.current;
      // The learner took the view, or the page went: the glide is over.
      if (!el || !follow.current) {
        gliding.current = false;
        return;
      }
      const t = Math.min(1, (at - began) / 200);
      const eased = 1 - (1 - t) ** 3;
      // The end is read each frame: the box under the card is leaving as the card arrives.
      const max = Math.max(0, el.scrollHeight - el.clientHeight);
      setTop(from + (max - from) * eased);
      if (t < 1) window.requestAnimationFrame(step);
      else gliding.current = false;
    };
    window.requestAnimationFrame(step);
  };

  const place = (kind: "start" | "grow" | "settle") => {
    const s = stage.current;
    const now = liveRef.current;
    if (!s || !now || gliding.current) return;
    const max = Math.max(0, s.scrollHeight - s.clientHeight);
    if (kind === "settle") {
      const task = openTask();
      const want = task ? holding(task) : null;
      if (want !== null) setTop(Math.max(s.scrollTop, want));
      return;
    }
    setTop(max);
  };

  /**
   * Bring the task in hand clear of the answer box when the box takes the
   * focus. It only ever moves the view down: if the learner has read on to the
   * reply, focusing the box must not pull them back up to the task.
   */
  const reveal = () => {
    const s = stage.current;
    const task = openTask();
    if (!s || !task || isBusyRef.current) return;
    const want = holding(task);
    if (want !== null && want > s.scrollTop) setTop(want);
  };

  const measure = () => {
    const s = stage.current;
    if (!s) return;
    if (s.scrollHeight < tall.current) mine.current = Math.min(mine.current, s.scrollTop);
    tall.current = s.scrollHeight;
    const task = openTask();
    let next: Pin | null = null;
    // The strip holds the task whenever it is out of view: scrolled up past the top, or (in a short window) below the foot.
    if (task) {
      const at = task.getBoundingClientRect();
      const view = s.getBoundingClientRect();
      if (at.bottom < view.top + EDGE || at.top > view.bottom - EDGE) next = summarize(task);
    }
    setPin((old) => (old === next || (old && next && old.html === next.html && old.label === next.label) ? old : next));
    const rest = s.scrollHeight - s.clientHeight - s.scrollTop;
    setBelow(rest > 24);
    // Back at the end, the view follows the reply again.
    if (rest < 2) follow.current = true;
    if (main.current) main.current.dataset.scrolled = s.scrollTop > 0 ? "yes" : "no";
  };

  const showTask = () => {
    const task = openTask();
    if (!task || !stage.current) return;
    follow.current = false;
    setTop(Math.max(0, contentTop(task) - EDGE));
  };
  const toEnd = () => {
    const s = stage.current;
    if (!s) return;
    follow.current = true;
    setTop(s.scrollHeight);
  };

  // A mark arrives in a window with no margin: its sentence takes a line under
  // the answer, and everything below it would move down that line. The view
  // moves down with it, so what is on screen stays where it is. (Where there is
  // a margin the mark is out of the flow, takes no room, and there is nothing to do.)
  useLayoutEffect(() => {
    const s = stage.current;
    if (!s || marked === 0) return;
    const el = Array.from(s.querySelectorAll<HTMLElement>(`.you-verdict[data-added="${marked}"]`)).find((node) => node.offsetHeight > 0);
    if (!el) return;
    // The page was this tall at its last look, before the sentence was in it.
    const room = s.scrollHeight - tall.current;
    if (room > 0 && contentTop(el) < s.scrollTop + s.clientHeight) setTop(s.scrollTop + room);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [marked]);

  // A new exchange shows its end.
  useLayoutEffect(() => {
    follow.current = true;
    place("start");
    measure();
  }, [live?.since]);

  // A card arrives (a question, a request for permission): the view glides to it.
  const cardCount = useRef(state.cards.length);
  useLayoutEffect(() => {
    const before = cardCount.current;
    cardCount.current = state.cards.length;
    if (state.cards.length > before && follow.current) glide();
  }, [state.cards.length]);

  // The reply settles: the task is brought clear of the answer box.
  useLayoutEffect(() => {
    if (!isBusy && follow.current) place("settle");
    measure();
    // Fonts and drawn formulas can still change the page's height a frame or two
    // later, in WebKit more than in Chrome: settle again once they have.
    if (isBusy) return;
    let frames = 0;
    let id = window.requestAnimationFrame(function again() {
      if (follow.current) place("settle");
      measure();
      if (++frames < 3) id = window.requestAnimationFrame(again);
    });
    return () => window.cancelAnimationFrame(id);
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
    // Tall content arrives after the words: a diagram drawn in place of its
    // placeholder, an image once it is read. WebKit then takes the view to the
    // very top in the same frame, and the scroll event that follows reads as
    // the learner scrolling up, so the view would stop following the reply. The
    // view is put back at once, before that event, with plain writes of scrollTop.
    const tall = (node: Node) => node instanceof Element && (node.matches(".diagram, img, table, .table") || node.querySelector(".diagram, img, table") !== null);
    const arrived = new MutationObserver((records) => {
      if (!follow.current || !records.some((r) => Array.from(r.addedNodes).some(tall))) return;
      place(isBusyRef.current ? "grow" : "settle");
      measure();
    });
    arrived.observe(column, { childList: true, subtree: true });
    const loaded = () => {
      if (!follow.current) return;
      place(isBusyRef.current ? "grow" : "settle");
      measure();
    };
    column.addEventListener("load", loaded, true);
    return () => {
      watch.disconnect();
      arrived.disconnect();
      column.removeEventListener("load", loaded, true);
    };
  }, []);

  // Opened, the earlier turns go above the exchange in hand, as in any
  // conversation. The view stays on that exchange, with the end of the turn
  // before it showing, so it is plain that there is more to scroll up to.
  useLayoutEffect(() => {
    if (!showEarlier) return;
    const el = liveRef.current;
    if (el && stage.current) setTop(Math.max(0, contentTop(el) - 72));
  }, [showEarlier]);

  // Only a learner reading back holds the view where it is: the view moving up
  // from where the page last put it (wheel, finger, keys or scroll bar), or a
  // press on the scroll bar. A click on a card is not that, and nor is the
  // stray tick of a trackpad.
  const held = () => {
    follow.current = false;
  };
  const scrolled = () => {
    const s = stage.current;
    // A view the page itself pulled up is not the learner's: the window can
    // shrink or grow under it for a moment (the answer box resizing as it is
    // typed in), and the browser then clamps the view to its end. Only a view
    // that moved up and is not at the end is theirs.
    if (s && s.scrollTop < mine.current - 2 && s.scrollTop < s.scrollHeight - s.clientHeight - 2) held();
    if (s) mine.current = s.scrollTop;
    measure();
  };

  /**
   * Reading without leaving the answer box: from a text field, Page Up and
   * Page Down move the transcript a page, ⌥↑ and ⌥↓ a few lines. Plain writes
   * of scrollTop, like every other move of the view; a move up is the
   * learner's, so the view stops following the reply.
   */
  const scrollFromField = (event: React.KeyboardEvent) => {
    const s = stage.current;
    const target = event.target;
    if (!s || event.defaultPrevented || composing(event) || event.metaKey || event.ctrlKey || event.shiftKey) return;
    if (!(target instanceof HTMLTextAreaElement || target instanceof HTMLInputElement)) return;
    let by = 0;
    if (!event.altKey && event.key === "PageDown") by = s.clientHeight * 0.9;
    else if (!event.altKey && event.key === "PageUp") by = -s.clientHeight * 0.9;
    else if (event.altKey && event.key === "ArrowDown") by = 90;
    else if (event.altKey && event.key === "ArrowUp") by = -90;
    if (by === 0) return;
    event.preventDefault();
    s.scrollTop += by;
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
      onKeyDown={scrollFromField}
      {...(props.overlay ? ({ inert: "" } as object) : {})}
    >
      <TopBar
        left={
          <button className="crumb" onClick={() => beginStop()} disabled={stopping} title="End lesson (Esc)" data-role="end">
            <Icon name="close" size={16} />
            End lesson
          </button>
        }
        center={<Track rail={props.rail} open={planOpen} onOpen={setPlanOpen} startedMs={state.startedMs} blockMinutes={props.blockMinutes} />}
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
              <div className="column">
                <button className="task-pin" onClick={showTask} title="Show the whole task" data-role="task-pin">
                  <span className="task-pin-label">{pin.label}</span>
                  <span className="task-pin-text" dangerouslySetInnerHTML={{ __html: pin.html }} />
                </button>
              </div>
            </div>
          )}
          <div
            className="scroll stage"
            ref={stage}
            role="region"
            aria-label="The lesson"
            tabIndex={-1}
            onScroll={scrolled}
            onPointerDown={(e) => e.target === stage.current && held()}
          >
            <div className="column">
              {earlier > 0 && (
                <button className="earlier-toggle" onClick={() => setShowEarlier(!showEarlier)} aria-expanded={showEarlier} data-role="earlier">
                  <Icon name="history" size={14} />
                  {showEarlier ? "Hide the earlier part" : `Earlier in this lesson (${earlier})`}
                </button>
              )}
              {showEarlier && (
                <div className="earlier answered">
                  {state.earlier.slice(0, earlier).map((turn, index) => (
                    <div className="exchange" key={index}>
                      {turn.learner && <Turn text={turn.learner} since={turn.ms} />}
                      <div className="tutor">
                        <Rich text={turn.tutor} final stopped={turn.interrupted} onNote={props.onNote} linksOpen={!setsTask[index] || answered(index)} onSource={openSource} />
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
                {live && exchange({ learner: live.learner, tutor: text, interrupted: live.interrupted, ms: live.since }, state.earlier.length, { writing: isBusy })}
                {!live && <p className="hint">Say what you want to work on.</p>}
                {live?.interrupted && !isBusy && <div className="stopped">Stopped by you</div>}
                {live?.error && !isBusy && <LessonFault text={live.error} alert />}
              </div>,
              ]}
              {/* One card at a time: while the End sheet stands, a card waits, whole, behind it. */}
              {state.cards.map((card) => (
                <div key={card.id} hidden={stopping}>
                  {card.kind === "permission" ? (
                    <PermissionCard card={card} onDecide={decide} />
                  ) : (
                    <QuestionCard card={card} onAnswer={answer} escape={cardEscape} />
                  )}
                </div>
              ))}
              {state.notices.map((notice) => (
                <LessonFault key={notice.text} text={notice.text} onDismiss={() => props.onDismiss(notice.text)} />
              ))}
            </div>
          </div>
          {/* The end sheet, the nudge and a note about material stand above the dock, not in it: the answer box is on the foot of the window and nothing moves it. */}
          {(stopping || (blockOver && !nudged && !isBusy && state.cards.length === 0) || props.material) && (
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
                  <h3>End this lesson?</h3>
                  <label className="sheet-note" htmlFor="stop-line">
                    Your next lesson starts from here (you can edit it)
                  </label>
                  <input
                    id="stop-line"
                    ref={stopInput}
                    className="field"
                    value={line}
                    placeholder="One line: where you are, and what comes next"
                    onChange={(e) => setLine(e.target.value)}
                    // Return takes the key on the way down and ends the lesson on the way up, so
                    // no keydown of it reaches the screen that follows.
                    onKeyDown={(e) => {
                      if (e.key !== "Enter" || composing(e)) return;
                      e.preventDefault();
                      e.stopPropagation();
                      if (!e.repeat) endArmed.current = true;
                    }}
                    onKeyUp={(e) => {
                      if (e.key !== "Enter" || !endArmed.current) return;
                      endArmed.current = false;
                      stop(line);
                    }}
                  />
                  <div className="row">
                    <button
                      className="btn primary"
                      title="End the lesson and keep this line (Return)"
                      onClick={() => stop(line)}
                      onKeyDown={(e) => {
                        // A button clicks on Return's way down: here it waits for the way up.
                        if (e.key !== "Enter" || composing(e)) return;
                        e.preventDefault();
                        e.stopPropagation();
                        if (!e.repeat) endArmed.current = true;
                      }}
                      onKeyUp={(e) => {
                        if (e.key !== "Enter" || !endArmed.current) return;
                        endArmed.current = false;
                        stop(line);
                      }}
                    >
                      End lesson
                    </button>
                    <button className="btn" onClick={() => setStopping(false)}>
                      Keep going
                    </button>
                  </div>
                </div>
              )}
              {!stopping && blockOver && !nudged && !isBusy && state.cards.length === 0 && (
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
                {showPassages && <span className="lesson-label passages-label">Sources the tutor used</span>}
                {showPassages && (
                  <div className="passages" data-role="passages">
                    {passages.slice(0, 4).map((passage) => (
                      <button
                        key={passage.href}
                        className={source?.ref.path === sourceRef(passage.href)?.path ? "passage on" : "passage"}
                        data-role="source-chip"
                        onClick={(e) => {
                          const ref = sourceRef(passage.href);
                          if (ref) openSource(ref, e.currentTarget);
                        }}
                      >
                        <Icon name="paper" size={14} />
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
                    onSend={sendAnswer}
                    onNote={props.onNote}
                    onAddFiles={props.onAddFiles}
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
        {source && <SourcePanel source={source.ref} sheet={narrow} onClose={() => closeSource(true)} onNote={props.onNote} />}
      </div>
    </section>
  );
}
