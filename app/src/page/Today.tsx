// Today's page: what was said today, in order, written in as it happens.
// The tutor's words run as plain text. A task it sets is an exercise, set
// apart by a rule; an answer to it sits inside the exercise with the record's
// mark in the margin. Anything else the learner writes is a line on the page.

import { memo } from "react";

import { clock } from "../days";
import { isHelp, type Status } from "../lesson";
import { openingLine, verdict, wordsOf } from "../log";
import { hasTask } from "../render";
import { Line, Rich } from "../ui/Rich";
import type { SourceRef } from "../sources";
import type { Intent, Mark } from "../types";

/** What the window knows of one exchange beyond its words. */
export type Meta = {
  /** How it was sent: an answer (written in the open task), talk (written on the page), or neither (the window's own words, a help button). */
  intent: Intent | null;
  /** The task that was open when it was sent. */
  task: string | null;
  /** The task the reply set, as the record holds it. */
  posed: string | null;
  /** What the plan change after the reply came to, in plain lines. */
  plan: string[] | null;
  /** The record's marks for it. */
  marks: Mark[];
  /** The record was read after the reply and held no mark for it. */
  unmarked: boolean;
};

export const blankMeta = (): Meta => ({ intent: null, task: null, posed: null, plan: null, marks: [], unmarked: false });

export type Item = {
  key: string;
  ms: number;
  folder: string;
  course: string;
  learner: string;
  tutor: string;
  /** The reply is whole. */
  final: boolean;
  interrupted: boolean;
  meta: Meta;
  /** A new sitting begins here: a new course, or the first words of the day. */
  sitting: boolean;
  status: Status;
  doing: string | null;
  error: string | null;
};

/** The reply's words before the task, and the task, when the record says which task it set. A task set in bold is drawn as one by the page itself. */
export function splitTask(reply: string, posed: string | null): { prose: string; task: string } | null {
  if (!posed || hasTask(reply)) return null;
  const at = (needle: string) => (needle.length >= 12 ? reply.lastIndexOf(needle) : -1);
  let index = at(posed);
  if (index < 0) index = at(posed.slice(0, 24));
  if (index < 0) {
    const cut = reply.trimEnd().lastIndexOf("\n\n");
    index = cut < 0 ? 0 : cut + 2;
  }
  // The task starts at the beginning of its sentence.
  return { prose: reply.slice(0, index).trimEnd(), task: reply.slice(index).trim() };
}

function MarkGut({ meta, answer }: { meta: Meta; answer: boolean }) {
  const mark = meta.marks[meta.marks.length - 1];
  if (!mark) {
    if (answer && meta.unmarked)
      return (
        <span className="mark none" title="The record holds no mark for this, so it counts for nothing and against nothing.">
          <span className="mark-word">no mark</span>
        </span>
      );
    return null;
  }
  const said = verdict(mark);
  return (
    <span className={said.right ? "mark right" : "mark"} title={said.title} data-role="mark">
      <span className="mark-word">{said.word}</span>
      {said.how && <span className="mark-how">{said.how}</span>}
    </span>
  );
}

/** The learner's words as they wrote them, with what the window added shown apart. */
function Said({ learner }: { learner: string }) {
  const words = wordsOf(learner);
  return (
    <>
      {words.text.trim() !== "" && (
        <div className="words">
          <Line text={words.text} />
        </div>
      )}
      {(words.confidence || words.photos > 0) && (
        <div className="aside">
          {[words.confidence, words.photos === 1 ? "a photo of working" : words.photos > 1 ? `${words.photos} photos of working` : null].filter(Boolean).join(", ")}
        </div>
      )}
    </>
  );
}

const isAnswerTo = (next: Item | undefined, posed: string | null, boxed: boolean): boolean => {
  if (!next) return false;
  if (wordsOf(next.learner).opening) return false;
  if (next.meta.intent === "talk") return false;
  if (next.meta.intent === "answer" || isHelp(next.learner)) return Boolean(posed || boxed);
  // Kept from before the window started: an answer the record marked.
  return next.meta.marks.length > 0 && Boolean(posed || boxed);
};

type ExchangeProps = {
  item: Item;
  /** This item's words were drawn inside the previous reply's exercise. */
  absorbed: boolean;
  /** The next item answers the task this reply set: drawn inside its exercise. */
  answer: Item | null;
  /** This reply's task is the one open now. */
  open: boolean;
  onSource?: (source: SourceRef, from?: HTMLElement) => void;
  onNote: (text: string) => void;
};

const Exchange = memo(function Exchange({ item, absorbed, answer, open, onSource, onNote }: ExchangeProps) {
  const words = wordsOf(item.learner);
  const writing = !item.final;
  const split = item.final ? splitTask(item.tutor, item.meta.posed) : null;
  const boxed = item.tutor !== "" && hasTask(item.tutor);
  const taskOf = item.meta.task;
  const linksOpen = !(open && (split || boxed));
  return (
    <>
      {item.sitting && (
        <div className="row sitting">
          <div className="gut">{clock(item.ms)}</div>
          <div className="txt">
            <span className="course">{item.course}</span>
          </div>
        </div>
      )}
      {!absorbed && words.opening && (
        <div className="row said opening" data-ms={item.ms}>
          <div className="gut">{item.sitting ? null : clock(item.ms)}</div>
          <div className="txt">{openingLine(words.text, words.opening.stoppedAt)}</div>
        </div>
      )}
      {!absorbed && !words.opening && item.learner.trim() !== "" && (item.meta.intent === "answer" || isHelp(item.learner)) && (
        // An answer to a task set further up the page: the task comes with it.
        <div className="row exercise answer detached" data-ms={item.ms}>
          <div className="gut">
            <MarkGut meta={item.meta} answer={item.meta.intent === "answer"} />
          </div>
          <div className="txt">
            {taskOf && (
              <div className="asked">
                <Line text={taskOf} />
              </div>
            )}
            <Said learner={item.learner} />
          </div>
        </div>
      )}
      {!absorbed && !words.opening && item.learner.trim() !== "" && item.meta.intent !== "answer" && !isHelp(item.learner) && (
        <div className={item.meta.intent === "talk" ? "row said talk" : "row said"} data-intent={item.meta.intent ?? undefined} data-ms={item.ms}>
          <div className="gut">
            {item.meta.marks.length > 0 ? <MarkGut meta={item.meta} answer={false} /> : clock(item.ms)}
          </div>
          <div className="txt">
            <Said learner={item.learner} />
          </div>
        </div>
      )}
      {(item.tutor !== "" || writing) && (
        <div className={writing ? "row reply writing" : "row reply"}>
          <div className="gut" />
          <div className={open && boxed ? "txt task-open" : "txt"}>
            {item.tutor === "" && item.status !== "idle" ? null : (
              <Rich text={split ? split.prose : item.tutor} final={item.final} stopped={item.interrupted} linksOpen={linksOpen} onSource={onSource} onNote={onNote} />
            )}
            {item.interrupted && <p className="stopped">Stopped here.</p>}
          </div>
        </div>
      )}
      {split && (
        <div className={open ? "row exercise posed open" : "row exercise posed"} data-role="exercise">
          <div className="gut" />
          <div className="txt">
            <Rich text={split.task} final linksOpen={!open} onSource={onSource} onNote={onNote} />
          </div>
        </div>
      )}
      {answer && (
        <div className={open ? "row exercise answer open" : "row exercise answer"} data-ms={answer.ms} data-intent={answer.meta.intent ?? undefined}>
          <div className="gut">
            <MarkGut meta={answer.meta} answer={answer.meta.intent === "answer"} />
          </div>
          <div className={isHelp(answer.learner) ? "txt help" : "txt"}>
            <Said learner={answer.learner} />
          </div>
        </div>
      )}
      {item.meta.plan && item.meta.plan.length > 0 && (
        <div className="row plan-change" data-role="plan-change">
          <div className="gut">{clock(item.ms)}</div>
          <div className="txt">
            <span className="plan-head">The plan changed.</span> {item.meta.plan.join(" ")}
          </div>
        </div>
      )}
      {item.error && (
        <div className="row note">
          <div className="gut" />
          <div className="txt">{item.error}</div>
        </div>
      )}
    </>
  );
});

/**
 * Today's entries in order. Where an answer follows the task it answers, it
 * is drawn inside that task's exercise; the reply to it then follows.
 */
export const Entries = memo(function Entries(props: {
  items: Item[];
  openTask: string | null;
  onSource?: (source: SourceRef, from?: HTMLElement) => void;
  onNote: (text: string) => void;
}) {
  const { items } = props;
  const out: React.ReactNode[] = [];
  let absorbed = false;
  items.forEach((item, i) => {
    const next = items[i + 1];
    const boxed = item.final && item.tutor !== "" && hasTask(item.tutor);
    // An answer is drawn inside the exercise only when its reply came at once.
    const answer = item.final && !next?.sitting && isAnswerTo(next, item.meta.posed, boxed) ? next : null;
    const isLast = i === items.length - 1;
    const open = isLast && props.openTask !== null && (item.meta.posed !== null || boxed);
    out.push(
      <Exchange
        key={item.key}
        item={item}
        absorbed={absorbed}
        answer={answer ?? null}
        open={open}
        onSource={props.onSource}
        onNote={props.onNote}
      />,
    );
    absorbed = answer !== null;
  });
  return <>{out}</>;
});
