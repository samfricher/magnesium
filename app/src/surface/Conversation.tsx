// The lesson as it fills the surface: the tutor's words in the reading face,
// the learner's in the face they type in, an answer inside the task it
// answers, and the record's lines (a mark, a change to the plan) small and in
// ink under what they are about. Nothing here reads a mark or a change from
// the tutor's words.

import { memo } from "react";

import { isHelp, opening, splitAttached, splitConfidence, type Card, type LessonTurn, type Live, type Notice } from "../lesson";
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

export function PlanChange({ change, brief }: { change: PlanDiff; brief?: boolean }) {
  const shown = change.changes.filter((each) => !each.route);
  if (shown.length === 0) return null;
  // Once read, the change settles to one quiet line.
  if (brief) {
    return (
      <p className="record plan-brief" data-role="plan-change">
        The plan changed: {shown.map((each) => `${each.what}, ${each.detail}`).join("; ")}.
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
      <p className="record-keys">⌘P the whole plan</p>
    </div>
  );
}

/** What the learner said, drawn by how it was sent. */
function Learner({ turn, showQuestion }: { turn: LessonTurn; showQuestion: boolean }) {
  const text = turn.learner;
  if (text.trim() === "") return null;
  if (turn.said === "opening" || opening(text)) return null;
  const attached = splitAttached(text);
  const { text: words, confidence } = splitConfidence(attached.text);
  const marks = turn.marks ?? [];
  const sent = <Sent files={attached.files} working={attached.working} />;
  if (turn.pick) return <Said text={words} className="you pick" />;
  // Help asked for is the window's own words, said for the learner: one quiet line, not an answer.
  if (turn.said === "help" || (turn.said === undefined && isHelp(text))) {
    return (
      <>
        <Said text={words} className="you talk help" />
        {sent}
        <Marks marks={marks} />
      </>
    );
  }
  if (turn.said === "answer") {
    return (
      <div className="answer">
        {showQuestion && turn.question && (
          <p className="question">
            <Line text={firstWords(turn.question)} />
          </p>
        )}
        {words.trim() !== "" && <Said text={words} className="you" />}
        {sent}
        {confidence && <span className="said-sure">{confidence.toLowerCase()}</span>}
        <Marks marks={marks} />
        {marks.length === 0 && turn.unmarked && (
          <p className="record quiet unmarked" title={turn.unmarked}>
            no mark
          </p>
        )}
      </div>
    );
  }
  return (
    <>
      {words.trim() !== "" && <Said text={words} className="you talk" />}
      {sent}
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
  onSource: (source: SourceRef, from?: HTMLElement) => void;
  onNote: (text: string) => void;
  onHold: () => void;
};

/** One exchange. Finished exchanges are drawn once and kept: a new word in the live reply redraws only the live one. */
export const Turn = memo(function Turn({ turn, open, held, linksOpen, writing, divider, task, showQuestion, settled, onSource, onNote, onHold }: TurnProps) {
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
          <Rich text={reply} final={!writing} stopped={turn.interrupted} linksOpen={linksOpen} onSource={onSource} onNote={onNote} task={task ?? turn.posed} caret={writing} />
        </div>
      )}
      {turn.interrupted && <p className="record quiet">Stopped.</p>}
      {turn.change && <PlanChange change={turn.change} brief={settled} />}
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

export function Notices({ notices, onDismiss }: { notices: Notice[]; onDismiss: (text: string) => void }) {
  if (notices.length === 0) return null;
  return (
    <div className="notices">
      {notices.map((notice) => (
        <p key={notice.text} className="record note" role={notice.kind === "usage_limit" || notice.kind === "sign_in" ? "alert" : "status"}>
          {withoutPaths(notice.text)}{" "}
          <button className="plain" onClick={() => onDismiss(notice.text)}>
            dismiss
          </button>
        </p>
      ))}
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
