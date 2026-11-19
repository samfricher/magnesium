import { memo, useEffect, useRef, useState } from "react";

import { isHelp, opening, splitAttached, splitConfidence, splitQuote, type Card, type LessonTurn, type Live, type Notice } from "../lesson";
import { useHangs } from "../hangs";
import { renderPlain } from "../render";
import type { SourceRef } from "../sources";
import type { Answer, Decision, Mark } from "../types";
import { firstWords, markWords, passDay, passDaySpoken, passWords, withoutPaths, type PlanDiff } from "../words";
import { Line, Rich } from "./Rich";

function Said({ text, className }: { text: string; className: string }) {
  return <div className={className} dangerouslySetInnerHTML={{ __html: renderPlain(text) }} />;
}

function Sent({ files, working }: { files: string[]; working: boolean }) {
  if (files.length === 0) return null;
  return (
    <p className="sent-files" data-role="sent-files">
      {working ? "with your work: " : "with "}
      {files.map((file) => file.split("/").pop()).join(", ")}
    </p>
  );
}

const arrived = new Set<string>();
const filled = new Set<string>();

export function markArrived(id: string): void {
  arrived.add(id);
}

function PassDays({ mark, day }: { mark: Mark; day: number }) {
  const [fresh] = useState(() => arrived.has(mark.id) && !filled.has(mark.id));
  const days = useRef<HTMLSpanElement | null>(null);
  const edge = useRef<HTMLElement | null>(null);
  useEffect(() => {
    filled.add(mark.id);
  }, [mark.id]);
  const hangs = useHangs(edge, ".surface", () => {
    const rings = days.current;
    return rings ? rings.offsetWidth + parseFloat(getComputedStyle(rings).getPropertyValue("--pass-gap") || "0") * parseFloat(getComputedStyle(document.documentElement).fontSize) : 0;
  });
  return (
    <span
      ref={(element) => {
        days.current = element;
        edge.current = element?.parentElement ?? null;
      }}
      className={hangs ? "pass-days" : "pass-days inline"}
      aria-hidden="true"
      data-role="pass-days"
      data-day={day}
    >
      {[1, 2, 3].map((at) => (
        <span key={at} className={at < day ? "pass-day earned" : at === day ? (fresh ? "pass-day earned arriving" : "pass-day earned") : "pass-day"} />
      ))}
    </span>
  );
}

function Marks({ marks }: { marks: Mark[] }) {
  return (
    <>
      {marks.map((mark) => {
        const day = passDay(mark);
        const spoken = passDaySpoken(mark);
        const pass = passWords(mark);
        return (
          <p key={mark.id} className="record mark" data-mark={mark.mark}>
            {day !== null && <PassDays mark={mark} day={day} />}
            {markWords(mark)}
            {spoken && <span className="visually-hidden"> {spoken}</span>}
            {pass && <span className="record-pass"> {pass}</span>}
            {pass && mark.receipt && (
              <span className="record-receipt" data-role="receipt">
                {" "}
                <Line text={mark.receipt} />
              </span>
            )}
          </p>
        );
      })}
    </>
  );
}

function Closing({ lines }: { lines: string[] }) {
  return (
    <div className="record closing" data-role="closing">
      {lines.map((line) => (
        <p key={line}>
          <Line text={line} />
        </p>
      ))}
    </div>
  );
}

function byThing(changes: PlanDiff["changes"]): { what: string; detail: string }[] {
  const rows: { what: string; detail: string }[] = [];
  for (const each of changes) {
    const row = rows.find((other) => other.what === each.what);
    if (row) row.detail = `${row.detail}, ${each.detail.replace(/^now /, "")}`;
    else rows.push({ what: each.what, detail: each.detail });
  }
  return rows;
}

function namesOf(rows: { what: string }[]): string {
  const names = rows.map((row) => row.what);
  return names.length <= 1 ? (names[0] ?? "") : `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

export function PlanChange({ change, brief, planKey }: { change: PlanDiff; brief?: boolean; planKey?: string | null }) {
  const shown = byThing(change.changes.filter((each) => !each.route));
  if (shown.length === 0) return null;
  if (brief) {
    return (
      <p className="record plan-brief" data-role="plan-change">
        <Line text={`The plan changed: ${namesOf(shown)}.`} />
      </p>
    );
  }
  return (
    <div className="record plan" data-role="plan-change">
      <p className="record-head">The plan changed</p>
      <dl>
        {shown.map((each, at) => (
          <div key={at}>
            <dt>
              <Line text={each.what} />
            </dt>
            <dd>
              <Line text={each.detail} />
            </dd>
          </div>
        ))}
      </dl>
      {planKey && <p className="record-keys">{planKey} the whole plan</p>}
    </div>
  );
}

function Learner({ turn, showQuestion }: { turn: LessonTurn; showQuestion: boolean }) {
  const text = turn.learner;
  if (text.trim() === "") return null;
  if (turn.said === "opening" || opening(text)) return null;
  const quoted = splitQuote(text);
  const attached = splitAttached(quoted.text);
  const { text: words, confidence } = splitConfidence(attached.text);
  const marks = turn.marks ?? [];
  const quiet = turn.pick || turn.said === "help" || (turn.said === undefined && isHelp(text));
  return (
    <>
      {showQuestion && marks.length > 0 && turn.question && (
        <p className="question">
          <Line text={firstWords(turn.question)} />
        </p>
      )}
      {quoted.quote && <p className="said-quote" dangerouslySetInnerHTML={{ __html: `“${renderPlain(quoted.quote)}”` }} />}
      {words.trim() !== "" && <Said text={words} className={`you talk${quiet ? (turn.pick ? " pick" : " help") : ""}`} />}
      {confidence && <span className="said-sure">{confidence.toLowerCase()}</span>}
      <Sent files={attached.files} working={attached.working} />
      <Marks marks={marks} />
    </>
  );
}

type TurnProps = {
  turn: LessonTurn;
  first: boolean;
  last: boolean;
  open: boolean;
  linksOpen: boolean;
  opensNow?: (path: string) => boolean;
  writing: boolean;
  divider: boolean;
  task?: string;
  showQuestion: boolean;
  settled?: boolean;
  planKey: string | null;
  onSource: (source: SourceRef, from?: HTMLElement) => void;
  onNote: (text: string) => void;
  waiting?: boolean;
};

export const Turn = memo(function Turn({ turn, first, last, open, linksOpen, opensNow, writing, divider, task, showQuestion, settled, planKey, onSource, onNote, waiting }: TurnProps) {
  const reply = turn.tutor;
  return (
    <article className={`turn${open ? " open" : ""}${first ? " first" : ""}${last ? " last" : ""}`} data-ms={turn.ms}>
      {divider && <hr className="sitting-rule" />}
      <Learner turn={turn} showQuestion={showQuestion} />
      {waiting && (
        <p className="record quiet" data-role="held">
          goes when the tutor finishes
        </p>
      )}
      {turn.step && (
        <h2 className="step-head" data-role="step-head">
          <span className="step-n" aria-hidden="true">
            {turn.step.n}
          </span>
          <span className="visually-hidden">{`Step ${turn.step.n} of ${turn.step.of}: `}</span>
          <Line text={turn.step.name} />
        </h2>
      )}
      {reply.trim() !== "" && (
        <div className="tutor">
          <Rich text={reply} final={!writing} stopped={turn.interrupted} linksOpen={linksOpen} opensNow={opensNow} onSource={onSource} onNote={onNote} task={task ?? turn.posed} caret={writing} />
        </div>
      )}
      {turn.interrupted && <p className="record quiet">Stopped.</p>}
      {turn.change && <PlanChange change={turn.change} brief={settled} planKey={planKey} />}
      {turn.closing && <Closing lines={turn.closing} />}
    </article>
  );
});

export function Working({ live }: { live: Live }) {
  if (live.status === "idle" || (live.status === "writing" && (live.streaming !== "" || live.done.length > 0))) return null;
  const words = live.status === "working" && live.doing ? live.doing : null;
  return (
    <p className={`working ${live.status}`} role="status" aria-label={words ?? "The tutor is writing"}>
      <span className="cursor" aria-hidden="true" />
      {words && <span className="doing">{words}</span>}
    </p>
  );
}

export function Notices({
  notices,
  onDismiss,
  onStartAgain,
  onSignIn,
  onPickModel,
}: {
  notices: Notice[];
  onDismiss: (text: string) => void;
  onStartAgain?: (text: string, atUsualEffort: boolean) => void;
  onSignIn?: () => void;
  onPickModel?: () => void;
}) {
  if (notices.length === 0) return null;
  return (
    <div className="notices">
      {notices.map((notice) =>
        (notice.kind === "record_unopened" || notice.kind === "record_unopened_at_effort") && onStartAgain ? (
          <p key={notice.text} className="record note" role="alert">
            {withoutPaths(notice.text)}
            <span className="note-acts">
              <button className="plain note-act" data-drive="start-again" onClick={() => onStartAgain(notice.text, notice.kind === "record_unopened_at_effort")}>
                {notice.kind === "record_unopened_at_effort" ? "Start again at usual effort" : "Start again"}
              </button>
              <button className="plain" onClick={() => onDismiss(notice.text)}>
                Carry on
              </button>
            </span>
          </p>
        ) : notice.kind === "sign_in" && onSignIn ? (
          <p key={notice.text} className="record note" role="alert">
            {withoutPaths(notice.text)}
            <span className="note-acts">
              <button className="plain note-act" data-drive="sign-in" onClick={onSignIn}>
                Sign in
              </button>
              <button className="plain" onClick={() => onDismiss(notice.text)}>
                Dismiss
              </button>
            </span>
          </p>
        ) : notice.kind === "model_unavailable" && onPickModel ? (
          <p key={notice.text} className="record note" role="alert">
            {notice.text}
            <span className="note-acts">
              <button className="plain note-act" data-drive="pick-model" onClick={onPickModel}>
                Pick a model
              </button>
              <button className="plain" onClick={() => onDismiss(notice.text)}>
                Dismiss
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

const TYPING_MS = 2000;
let lastKeyAt = -Infinity;
if (typeof window !== "undefined") window.addEventListener("keydown", () => (lastKeyAt = performance.now()), true);

function mayTakeKeys(): boolean {
  if (performance.now() - lastKeyAt < TYPING_MS) return false;
  const at = document.activeElement;
  if (!at || at === document.body) return true;
  if (!(at instanceof HTMLTextAreaElement) || at.dataset.drive !== "composer") return false;
  return at.value.trim() === "" && !at.closest(".line")?.querySelector(".attached .file");
}

function useFirstChoice() {
  const first = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (mayTakeKeys()) first.current?.focus();
  }, []);
  return first;
}

export function Cards(props: { cards: Card[]; onDecide: (id: string, decision: Decision) => void; onAnswer: (id: string, answers: Answer[]) => void }) {
  return (
    <>
      {props.cards.map((card) =>
        card.kind === "permission" ? (
          <PermissionCard key={card.id} card={card} onDecide={props.onDecide} />
        ) : (
          <QuestionCard key={card.id} card={card} onAnswer={props.onAnswer} />
        ),
      )}
    </>
  );
}

function PermissionCard({ card, onDecide }: { card: Extract<Card, { kind: "permission" }>; onDecide: (id: string, decision: Decision) => void }) {
  const first = useFirstChoice();
  const always = card.always ?? "Always allow";
  return (
    <div className="card" role="group" aria-label={card.title}>
      <p className="record-head">{card.title}</p>
      {card.detail && <p className="card-what">{withoutPaths(card.detail)}</p>}
      {card.exact && <p className="card-detail">{card.exact}</p>}
      <p className="card-acts">
        <button ref={first} className="act" data-drive="allow" onClick={() => onDecide(card.id, "allow_once")}>
          Allow once
        </button>
        <button className="act" onClick={() => onDecide(card.id, "refuse")}>
          Refuse
        </button>
      </p>
      {card.canAlways && (
        <p className="card-always">
          <button className="act" data-drive="allow-always" onClick={() => onDecide(card.id, "allow_always")}>
            or {always.charAt(0).toLowerCase() + always.slice(1)}
          </button>
        </p>
      )}
    </div>
  );
}

function QuestionCard({ card, onAnswer }: { card: Extract<Card, { kind: "question" }>; onAnswer: (id: string, answers: Answer[]) => void }) {
  const first = useFirstChoice();
  return (
    <div className="card" role="group" aria-label="The tutor asks">
      {card.questions.map((question, index) => (
        <div key={index} className="card-q">
          <p className="record-head">{question.header ?? "The tutor asks"}</p>
          <p className="card-question">{question.question}</p>
          <p className="card-acts">
            {question.options.map((option, n) => (
              <button
                key={option.label}
                ref={index === 0 && n === 0 ? first : undefined}
                className="act"
                title={option.description ?? undefined}
                onClick={() => onAnswer(card.id, card.questions.map((q, i) => ({ question: q.question, chosen: i === index ? [option.label] : [] })))}
              >
                {option.label}
              </button>
            ))}
          </p>
        </div>
      ))}
    </div>
  );
}
