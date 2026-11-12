// The first screen: every study course the window knows, the most urgent
// first. One card is one course. A card shows what decides where to study
// next and nothing else: the exam and how near it is, how much is proved,
// and what is due. The most urgent card's button, and Enter, go straight
// into its lesson. A card is not a button: its one main action is a real
// button, stretched over the card, and the course page and the remove
// control are separate buttons beside it.

import { useEffect, useId, useRef, useState } from "react";

import { Line } from "../ui/Rich";
import { Icon } from "../ui/icons";
import { Due, Level, PaletteKey, ProofDots, Run, TopBar, Week, agoText, clockText, examWhen, proofShare, skillsProven, splitGoal, useNow } from "../ui/bits";
import type { Activity, Card } from "../types";

/** Today as a day on the calendar, in this Mac's time. */
function localDay(): string {
  const now = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
}

/** A course's name as a title: "Demo-Course" reads "Demo Course". */
export function displayName(name: string): string {
  return name.replace(/[-_]+/g, " ").trim() || name;
}

function statusOf(card: Card): { text: string; tone: string } | null {
  if (card.sitting) return { text: "Paper running", tone: "plain" };
  switch (card.state) {
    case "not_set_up":
      return { text: "New", tone: "plain" };
    case "empty":
      return { text: "Ready to start", tone: "plain" };
    case "needs_upgrade":
      return { text: "Needs an upgrade", tone: "plain" };
    case "no_runtime":
    case "unreadable":
      return { text: "Could not be read", tone: "bad" };
    default:
      return null;
  }
}

/** A neutral chip: the exam and how near it is, as "Midterm 1 in 6 days". */
function ExamChip({ card }: { card: Card }) {
  const status = statusOf(card);
  if (status) return <span className={`chip ${status.tone}`}>{status.text}</span>;
  const exam = card.exam;
  if (!exam) return <span className="chip">No exam set</span>;
  return (
    <span className="chip">
      <span className="chip-text">
        <Line text={exam.title} />
        {examWhen(exam)}
      </span>
    </span>
  );
}

/** Where the course stopped, as a reference over a title. */
function Goal({ goal }: { goal: string }) {
  const { ref, title } = splitGoal(goal);
  return (
    <>
      {ref && <span className="cc-ref">{ref}</span>}
      <span className="cc-title">
        <Line text={title} />
      </span>
    </>
  );
}

/** The course's proofs in a few words: one mark of a proven skill, and "3 of 11 skills proven". */
function ProofSummary({ card }: { card: Card }) {
  const said = skillsProven(card.proofs);
  if (!said || card.state !== "ready") return null;
  return (
    <span className="cc-proofs num">
      <ProofDots earned={card.proofs.proven > 0 ? 3 : 0} label="" />
      {said}
    </span>
  );
}

/**
 * One course. The first is the "next" card, the one with the lesson's button;
 * the others are rows of a ruled index. Either way the course's name is the
 * one real button, stretched over the whole of it: its action is the lesson
 * for the next card and the course page for a row. The remove control, and the
 * course page from the next card, are separate buttons beside it.
 */
function CourseCard(props: {
  card: Card;
  hero: boolean;
  focus: boolean;
  /** The hero's lesson is starting. */
  starting: boolean;
  /** The card's main action: the lesson for the hero, else the course page. */
  onOpen: () => void;
  /** The course page, from the hero, whose main action is the lesson. */
  onCourse?: () => void;
  onForget: () => void;
  onMove: (by: number) => void;
}) {
  const { card } = props;
  const [asking, setAsking] = useState(false);
  const open = useRef<HTMLButtonElement>(null);
  const remove = useRef<HTMLButtonElement>(null);
  const keep = useRef<HTMLButtonElement>(null);
  const now = useNow(card.sitting ? 1000 : 60_000);
  const detail = useId();

  useEffect(() => {
    if (props.focus) open.current?.focus({ preventScroll: false });
  }, [props.focus]);
  // The question opens on "Keep". Closing it puts focus back on the control that opened it.
  useEffect(() => {
    if (asking) keep.current?.focus();
  }, [asking]);

  const proofs = card.proofs;
  const hasProofs = proofs.levelled && proofs.total > 0;
  const sitting = card.sitting;
  const leftSeconds = sitting ? Math.round((sitting.started_ms + sitting.minutes * 60_000 - now) / 1000) : 0;
  const name = displayName(card.name);
  const kind = props.hero ? "next-card library-next" : "ledger-row";

  if (asking) {
    const cancel = () => {
      setAsking(false);
      window.setTimeout(() => remove.current?.focus(), 0);
    };
    return (
      <div
        className={`${kind} asking`}
        role="group"
        aria-label={`Remove ${name}?`}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            event.stopPropagation();
            cancel();
          }
        }}
      >
        <h2>Remove {name}?</h2>
        <p className="cc-note">It leaves this list. Your files and your record stay on your Mac.</p>
        <div className="row">
          <button className="btn" ref={keep} onClick={cancel}>
            Keep
          </button>
          <button className="btn danger" onClick={props.onForget}>
            Remove
          </button>
        </div>
      </div>
    );
  }

  const label = props.starting ? "Starting…" : sitting ? "Back to the paper" : props.onCourse ? (card.goal ? "Continue" : "Start your first lesson") : "Open";
  const where = sitting ? (
    <>
      <span className="cc-ref">{sitting.name}</span>
      <span className="cc-title num">
        {clockText(leftSeconds)} {leftSeconds >= 0 ? "left" : "over"}
      </span>
    </>
  ) : card.goal ? (
    <Goal goal={card.goal} />
  ) : (
    <span className="cc-title dim">{card.state === "ready" || card.state === "empty" ? "Nothing started yet" : " "}</span>
  );
  const onKeyDown = (event: React.KeyboardEvent) => {
    if (event.target !== open.current) return;
    if (event.key === "ArrowRight" || event.key === "ArrowDown") {
      event.preventDefault();
      props.onMove(1);
    } else if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
      event.preventDefault();
      props.onMove(-1);
    }
  };
  const removeButton = (
    <button ref={remove} className="icon-btn cc-more" title="Remove from library" aria-label={`Remove ${name} from library`} onClick={() => setAsking(true)}>
      <Icon name="trash" size={16} />
    </button>
  );

  if (props.hero) {
    return (
      <div className={kind} data-folder={card.folder} onKeyDown={onKeyDown}>
        <div className="cc-body">
          <div className="cc-text">
            <div className="cc-name">
              <h2>{name}</h2>
              <ExamChip card={card} />
            </div>
            <p className="cc-where">
              {where}
              {card.days_ago !== null && <span className="cc-meta cc-ago">Last studied {agoText(card.days_ago)}</span>}
            </p>
            <ProofSummary card={card} />
          </div>
          {hasProofs && <Level value={proofs.level} size="lg" share={proofShare(proofs)} title={`${proofs.earned} of ${proofs.total} proofs`} />}
        </div>
        <div className="cc-foot">
          <button ref={open} className="btn primary cc-open" title={sitting ? "Back to the paper" : "Continue the lesson"} onClick={props.onOpen}>
            {label}
          </button>
          {props.onCourse && (
            <button className="btn ghost cc-side" data-role="course-page" title="This course's page: proofs, papers, a new lesson" onClick={props.onCourse}>
              Course page
            </button>
          )}
          {card.due > 0 && <Due n={card.due} />}
          <span className="grow" />
          {removeButton}
        </div>
      </div>
    );
  }

  return (
    <div className={kind} data-folder={card.folder} onKeyDown={onKeyDown}>
      <div className="lr-main">
        <div className="cc-name">
          <h2>
            <button ref={open} className="cc-open" aria-describedby={detail} onClick={props.onOpen}>
              {name}
            </button>
          </h2>
          <ExamChip card={card} />
        </div>
        <p className="cc-where" id={detail}>
          {where}
          {card.days_ago !== null && <span className="cc-meta cc-ago">Last studied {agoText(card.days_ago)}</span>}
        </p>
      </div>
      <div className="lr-proofs">
        <ProofSummary card={card} />
        {card.due > 0 && <Due n={card.due} />}
      </div>
      <div className="lr-level">
        {hasProofs && <Level value={proofs.level} size="sm" share={proofShare(proofs)} title={`${proofs.earned} of ${proofs.total} proofs`} />}
      </div>
      <div className="lr-remove">{removeButton}</div>
    </div>
  );
}

export function Library(props: {
  /** null while the courses are being read. */
  cards: Card[] | null;
  /** The courses' paths, known at once, to draw placeholders from. */
  folders: string[];
  activity: Activity | null;
  error: string | null;
  notice: string | null;
  finding: boolean;
  ready: boolean;
  /** A lesson is starting from the main card. */
  starting: boolean;
  /** Open a course and continue its lesson at once. */
  onContinue: (path: string) => void;
  /** Open a course's page. */
  onOpen: (path: string) => void;
  onPick: () => void;
  onNew: () => void;
  onFind: () => void;
  onForget: (path: string) => void;
  onSetup: () => void;
  onPalette: () => void;
}) {
  const { cards } = props;
  const [focus, setFocus] = useState(0);
  const count = cards?.length ?? 0;

  useEffect(() => {
    if (focus >= count && count > 0) setFocus(count - 1);
  }, [count, focus]);

  const add = (
    <div className="shelf-add">
      <button className="btn" title="Open a course (⌘O)" onClick={props.onPick}>
        <Icon name="folder" size={16} />
        Open a course
      </button>
      <button className="btn ghost" onClick={props.onNew}>
        <Icon name="plus" size={16} />
        New course
      </button>
      <button className="btn ghost" disabled={props.finding} onClick={props.onFind} title="Looks through your home folder for courses that Magnesium was set up in from a terminal">
        <Icon name="search" size={16} />
        {props.finding ? "Looking…" : "Find courses on this Mac"}
      </button>
    </div>
  );

  let body: JSX.Element;
  if (cards === null) {
    body = (
      <div className="cards">
        {(props.folders.length > 0 ? props.folders : [""]).slice(0, 6).map((path, index) => {
          const name = displayName(path.split("/").filter(Boolean).pop() ?? "");
          // A placeholder takes the room a real card or row takes, so nothing below it moves when the courses are read.
          return index === 0 ? (
            <div key={path || index} className="next-card library-next ghosted" aria-hidden="true">
              <div className="cc-body">
                <div className="cc-text">
                  <div className="cc-name">
                    <h2>{name}&nbsp;</h2>
                  </div>
                  <p className="cc-where">&nbsp;</p>
                </div>
              </div>
              <div className="cc-foot">&nbsp;</div>
            </div>
          ) : (
            <div key={path || index} className="ledger-row ghosted" aria-hidden="true">
              <div className="lr-main">
                <div className="cc-name">
                  <h2>{name}&nbsp;</h2>
                </div>
                <p className="cc-where">&nbsp;</p>
              </div>
            </div>
          );
        })}
      </div>
    );
  } else if (cards.length === 0) {
    body = (
      <div className="empty">
        <h2>Add your first course</h2>
        <p>A course is a folder with your notes, slides and past papers.</p>
        {add}
      </div>
    );
  } else {
    body = (
      <>
        <div className="cards">
          {cards.map((card, index) => {
            // The main card goes straight into its lesson when it can. A
            // course that is new, needs an upgrade or has a paper running
            // opens its page, which says what comes first.
            const straight = index === 0 && props.ready && card.state === "ready" && !card.sitting;
            const open = () => {
              if (props.starting) return;
              if (straight) props.onContinue(card.folder);
              else props.onOpen(card.folder);
            };
            return (
              <CourseCard
                key={card.folder}
                card={card}
                hero={index === 0}
                focus={index === focus}
                starting={index === 0 && props.starting}
                onOpen={open}
                onCourse={straight ? () => props.onOpen(card.folder) : undefined}
                onForget={() => props.onForget(card.folder)}
                onMove={(by) => setFocus(Math.max(0, Math.min(count - 1, index + by)))}
              />
            );
          })}
        </div>
        {add}
      </>
    );
  }

  return (
    <section className="screen library" data-screen="library">
      <TopBar
        right={
          <>
            <button className="menu-key" title="This Mac's setup: Claude Code, Magnesium, how lessons run" onClick={props.onSetup}>
              <Icon name="gear" size={16} />
              Setup
            </button>
            <PaletteKey onClick={props.onPalette} />
          </>
        }
      />
      <div className="scroll">
        <div className="shelf">
          <header className="shelf-head">
            <h1>Library</h1>
            {props.activity && count > 0 ? (
              <div className="shelf-week">
                <Week days={props.activity.days} today={props.activity.today} />
                <p className="shelf-caption">
                  Days you studied this week
                  <Run days={props.activity.run} />
                </p>
              </div>
            ) : (
              // Until the week is known its room is kept, so the title and the courses do not move when it arrives.
              (cards === null || count > 0) && (
                <div className="shelf-week" aria-hidden="true" style={{ visibility: "hidden" }}>
                  <Week days={[]} today={localDay()} />
                  <p className="shelf-caption">
                    Days you studied this week
                    <span className="run-line">&nbsp;</span>
                  </p>
                </div>
              )
            )}
          </header>
          {!props.ready && (
            <button className="banner warn" onClick={props.onSetup}>
              <Icon name="warn" size={16} />
              Something on this Mac needs fixing before a lesson can start.
              <span className="grow" />
              <b>Fix it</b>
            </button>
          )}
          {body}
          {props.notice && <div className="banner">{props.notice}</div>}
          {props.error && <div className="banner bad selectable">{props.error}</div>}
        </div>
      </div>
    </section>
  );
}
