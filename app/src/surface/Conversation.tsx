// The lesson as it fills the surface: the tutor's words in the reading face,
// the learner's in the face they type in, and the record's lines (a mark, a
// change to the plan) small and in ink under what they are about. Every
// message of the learner's is drawn the same way: the window sends them all
// alike, and only a mark under one says the record took it as an answer.
// Nothing here reads a mark or a change from the tutor's words.

import { memo } from "react";

import { isHelp, opening, splitAttached, splitConfidence, splitQuote, type Card, type LessonTurn, type Live, type Notice } from "../lesson";
import { renderPlain } from "../render";
import type { SourceRef } from "../sources";
import type { Answer, Decision, Mark } from "../types";
import { firstWords, markWords, withoutPaths, type PlanDiff } from "../words";
import { Line, Rich } from "./Rich";

function Said({ text, className }: { text: string; className: string }) {
  return <div className={className} dangerouslySetInnerHTML={{ __html: renderPlain(text) }} />;
}

/** Files that went with a message, by their own names: never the folder they were kept in. */
function Sent({ files, working }: { files: string[]; working: boolean }) {
  if (files.length === 0) return null;
  return (
    <p className="sent-files" data-role="sent-files">
      {working ? "with your work: " : "with "}
      {files.map((file) => file.split("/").pop()).join(", ")}
    </p>
  );
}

function Marks({ marks }: { marks: Mark[] }) {
  return (
    <>
      {marks.map((mark) => (
        <p key={mark.id} className="record mark" data-mark={mark.mark}>
          {markWords(mark)}
        </p>
      ))}
    </>
  );
}

/** What changed, one row for each thing that changed: "now before Confidence intervals, on Sun 15 Nov". */
function byThing(changes: PlanDiff["changes"]): { what: string; detail: string }[] {
  const rows: { what: string; detail: string }[] = [];
  for (const each of changes) {
    const row = rows.find((other) => other.what === each.what);
    if (row) row.detail = `${row.detail}, ${each.detail.replace(/^now /, "")}`;
    else rows.push({ what: each.what, detail: each.detail });
  }
  return rows;
}

/** "Midterm 2, Hypothesis tests and Confidence intervals". */
function namesOf(rows: { what: string }[]): string {
  const names = rows.map((row) => row.what);
  return names.length <= 1 ? (names[0] ?? "") : `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

export function PlanChange({ change, brief, planKey }: { change: PlanDiff; brief?: boolean; planKey?: string | null }) {
  const shown = byThing(change.changes.filter((each) => !each.route));
  if (shown.length === 0) return null;
  // Once read, the change settles to one quiet line: what changed, by name; the plan says how.
  if (brief) {
    return (
      <p className="record plan-brief" data-role="plan-change">
        The plan changed: {namesOf(shown)}.
      </p>
    );
  }
  return (
    <div className="record plan" data-role="plan-change">
      <p className="record-head">The plan changed</p>
      <dl>
        {shown.map((each, at) => (
          <div key={at}>
            <dt>{each.what}</dt>
            <dd>{each.detail}</dd>
          </div>
        ))}
      </dl>
      {planKey && <p className="record-keys">{planKey} the whole plan</p>}
    </div>
  );
}

/** What the learner said: the passage it quoted, the words, the files that went with it, and the record's marks for it. */
function Learner({ turn, showQuestion }: { turn: LessonTurn; showQuestion: boolean }) {
  const text = turn.learner;
  if (text.trim() === "") return null;
  if (turn.said === "opening" || opening(text)) return null;
  const quoted = splitQuote(text);
  const attached = splitAttached(quoted.text);
  const { text: words, confidence } = splitConfidence(attached.text);
  const marks = turn.marks ?? [];
  // A choice picked on a card, and help asked for in the window's own words, are said the same way, quieter.
  const quiet = turn.pick || turn.said === "help" || (turn.said === undefined && isHelp(text));
  return (
    <>
      {/* A mark on an answer to a task that is not just above it says which task it was. */}
      {showQuestion && marks.length > 0 && turn.question && (
        <p className="question">
          <Line text={firstWords(turn.question)} />
        </p>
      )}
      {quoted.quote && <p className="said-quote">“{quoted.quote}”</p>}
      {words.trim() !== "" && <Said text={words} className={`you talk${quiet ? (turn.pick ? " pick" : " help") : ""}`} />}
      {confidence && <span className="said-sure">{confidence.toLowerCase()}</span>}
      <Sent files={attached.files} working={attached.working} />
      <Marks marks={marks} />
    </>
  );
}

type TurnProps = {
  turn: LessonTurn;
  /** This turn's reply holds the open task. */
  open: boolean;
  /** The line is in that task. */
  held: boolean;
  /** Links to a source stay shut until the task in this reply is answered. */
  linksOpen: boolean;
  /** A file that opens even while this reply's links are shut: a past paper. */
  opensNow?: (path: string) => boolean;
  /** The reply is still being written. */
  writing: boolean;
  /** A sitting began here, in an earlier part of the conversation. */
  divider: boolean;
  /** The open task's words, when this turn's reply posed it. */
  task?: string;
  /** Say above an answer which task it answered: the task is not just above it. */
  showQuestion: boolean;
  /** A later turn follows: what this one changed in the plan is one quiet line now. */
  settled?: boolean;
  /** The key that shows the plan, while it is not shown beside the lesson; null while it is. */
  planKey: string | null;
  onSource: (source: SourceRef, from?: HTMLElement) => void;
  onNote: (text: string) => void;
  onHold: () => void;
};

/** One exchange. Finished exchanges are drawn once and kept: a new word in the live reply redraws only the live one. */
export const Turn = memo(function Turn({ turn, open, held, linksOpen, opensNow, writing, divider, task, showQuestion, settled, planKey, onSource, onNote, onHold }: TurnProps) {
  const reply = turn.tutor;
  return (
    <article
      className={`turn${open ? " open" : ""}${held ? " held" : ""}`}
      data-ms={turn.ms}
      onClick={open && !held ? (event) => !(event.target as HTMLElement).closest("a, button, code.path") && onHold() : undefined}
    >
      {divider && <hr className="sitting-rule" />}
      <Learner turn={turn} showQuestion={showQuestion} />
      {reply.trim() !== "" && (
        <div className="tutor">
          <Rich text={reply} final={!writing} stopped={turn.interrupted} linksOpen={linksOpen} opensNow={opensNow} onSource={onSource} onNote={onNote} task={task ?? turn.posed} caret={writing} />
        </div>
      )}
      {turn.interrupted && <p className="record quiet">Stopped.</p>}
      {turn.change && <PlanChange change={turn.change} brief={settled} planKey={planKey} />}
    </article>
  );
});

export function Working({ live }: { live: Live }) {
  // While words come, the caret is at their end, inside the reply.
  if (live.status === "idle" || (live.status === "writing" && (live.streaming !== "" || live.done.length > 0))) return null;
  const words = live.status === "working" && live.doing ? live.doing : null;
  return (
    <p className={`working ${live.status}`} role="status" aria-label={words ?? "The tutor is writing"}>
      <span className="cursor" aria-hidden="true" />
      {words && <span className="doing">{words}</span>}
    </p>
  );
}

export function Notices({ notices, onDismiss, onStartAgain }: { notices: Notice[]; onDismiss: (text: string) => void; onStartAgain?: (text: string) => void }) {
  if (notices.length === 0) return null;
  return (
    <div className="notices">
      {notices.map((notice) =>
        notice.kind === "record_unopened" && onStartAgain ? (
          // A lesson that never opened the record: the way on is to start again, at the agent's own effort.
          <p key={notice.text} className="record note" role="alert">
            {withoutPaths(notice.text)}
            <span className="note-acts">
              <button className="plain note-act" data-drive="start-again" onClick={() => onStartAgain(notice.text)}>
                Start again at usual effort
              </button>
              <button className="plain" onClick={() => onDismiss(notice.text)}>
                Carry on
              </button>
            </span>
          </p>
        ) : (
          <p key={notice.text} className="record note" role={notice.kind === "usage_limit" || notice.kind === "sign_in" ? "alert" : "status"}>
            {withoutPaths(notice.text)}{" "}
            <button className="plain" onClick={() => onDismiss(notice.text)}>
              Dismiss
            </button>
          </p>
        ),
      )}
    </div>
  );
}

/** A request from the tutor that waits on the learner: a permission, or a question with choices. */
export function Cards(props: { cards: Card[]; onDecide: (id: string, decision: Decision) => void; onAnswer: (id: string, answers: Answer[]) => void }) {
  return (
    <>
      {props.cards.map((card) =>
        card.kind === "permission" ? (
          <div key={card.id} className="card" role="group" aria-label={card.title}>
            <p className="record-head">{card.title}</p>
            <p className="card-detail">{withoutPaths(card.detail)}</p>
            <p className="card-acts">
              <button className="act" data-drive="allow" onClick={() => props.onDecide(card.id, "allow_once")} autoFocus>
                Allow once
              </button>
              {card.canAlways && (
                <button className="act" onClick={() => props.onDecide(card.id, "allow_always")}>
                  Always allow
                </button>
              )}
              <button className="act" onClick={() => props.onDecide(card.id, "refuse")}>
                Refuse
              </button>
            </p>
          </div>
        ) : (
          <div key={card.id} className="card" role="group" aria-label="The tutor asks">
            {card.questions.map((question, index) => (
              <div key={index} className="card-q">
                <p className="record-head">{question.header ?? "The tutor asks"}</p>
                <p className="card-question">{question.question}</p>
                <p className="card-acts">
                  {question.options.map((option, n) => (
                    <button
                      key={option.label}
                      className="act"
                      autoFocus={index === 0 && n === 0}
                      title={option.description ?? undefined}
                      onClick={() => props.onAnswer(card.id, card.questions.map((q, i) => ({ question: q.question, chosen: i === index ? [option.label] : [] })))}
                    >
                      {option.label}
                    </button>
                  ))}
                </p>
              </div>
            ))}
          </div>
        ),
      )}
    </>
  );
}
