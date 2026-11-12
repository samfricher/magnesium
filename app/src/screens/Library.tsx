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
import { Due, Fault, GoalText, Level, MarginNote, PaletteKey, TopBar, WeekLine, agoText, clockText, examWhen, skillsProven, useNow, useReturnForPrimary } from "../ui/bits";
import type { Activity, Card } from "../types";

/** A course's name as a title: "Demo-Course" reads "Demo Course". */
export function displayName(name: string): string {
  return name.replace(/[-_]+/g, " ").trim() || name;
}

function statusOf(card: Card): { text: string; tone: string } | null {
  if (card.sitting) return { text: "Paper running", tone: "plain" };
  switch (card.state) {
    // A course with nothing started says so once, in the line where the lesson would be.
    case "needs_upgrade":
      return { text: "Needs an upgrade", tone: "plain" };
    case "no_runtime":
    case "unreadable":
      return { text: "Could not be read", tone: "bad" };
    default:
      return null;
  }
}

/** What is so of the course, as plain words and not something to press: the exam and how near it is, or where the course stands. */
function ExamFact({ card }: { card: Card }) {
  const status = statusOf(card);
  if (status) {
    return (
      <span className={status.tone === "bad" ? "fact bad" : "fact"}>
        {status.tone === "bad" && <Icon name="bang" size={14} />}
        {status.text}
      </span>
    );
  }
  const exam = card.exam;
  if (!exam) return <span className="fact">No exam set</span>;
  return (
    <span className="fact">
      <Icon name="calendar" size={14} />
      <span className="fact-text">
        <Line text={exam.title} />
        {examWhen(exam)}
      </span>
    </span>
  );
}

/**
 * The course's proofs in a few words: "3 of 11 skills proven". Three dots mean
 * one skill's three proofs everywhere, so a course total is said, not dotted.
 */
function ProofSummary({ card }: { card: Card }) {
  const said = skillsProven(card.proofs);
  if (!said || card.state !== "ready") return null;
  return <span className="cc-proofs num">{said}</span>;
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
  /** This Mac can run a lesson. When it cannot, the fix is the screen's one green button, and the card's is outlined. */
  ready: boolean;
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
  const confirm = useRef<HTMLButtonElement>(null);
  const now = useNow(card.sitting ? 1000 : 60_000);
  const detail = useId();

  useEffect(() => {
    if (props.focus) open.current?.focus({ preventScroll: false });
  }, [props.focus]);
  // A confirmation is one pattern everywhere: the action first (ink-filled, as it removes something), then the outlined way out. Return does the action, Esc the way out, and focus starts on the action. Closing it puts focus back on the control that opened it.
  useEffect(() => {
    if (asking) confirm.current?.focus();
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
        <h2 className="h-section">Remove {name}?</h2>
        <p className="cc-note">It leaves this list. Your files and your record stay on your Mac.</p>
        <div className="row">
          <button className="btn danger" ref={confirm} onClick={props.onForget}>
            Remove
          </button>
          <button className="btn" onClick={cancel}>
            Keep
          </button>
        </div>
      </div>
    );
  }

  const label = props.starting ? "Starting…" : sitting ? "Back to the paper" : props.onCourse ? (card.goal ? "Continue" : "Start your first lesson") : "Open";
  const where = sitting ? (
    <span className="cc-title num">
      {sitting.name}, {clockText(leftSeconds)} {leftSeconds >= 0 ? "left" : "over"}
    </span>
  ) : card.goal ? (
    <span className="cc-title">
      <GoalText goal={card.goal} />
    </span>
  ) : (
    <span className="cc-title dim">{card.state === "ready" || card.state === "empty" || card.state === "not_set_up" ? "Nothing started yet" : " "}</span>
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
              <h2 className="h-section">{name}</h2>
              <ExamFact card={card} />
            </div>
            <p className="cc-where">{where}</p>
            <p className="cc-metas">
              {card.days_ago !== null && <span className="cc-meta">Last studied {agoText(card.days_ago)}</span>}
              <ProofSummary card={card} />
              {card.due > 0 && <Due n={card.due} />}
            </p>
          </div>
        </div>
        <div className="cc-foot">
          <button ref={open} className={props.ready ? "btn primary cc-open" : "btn cc-open"} title={sitting ? "Back to the paper (Return)" : `${label} (Return)`} onClick={props.onOpen}>
            {label}
          </button>
          {props.onCourse && (
            <button className="btn cc-side" data-role="course-page" title="This course's page: proofs, papers, a new lesson" onClick={props.onCourse}>
              Course page
            </button>
          )}
          {hasProofs && <Level proofs={proofs} size="lg" title={`${proofs.earned} of ${proofs.total} proofs`} />}
          {removeButton}
        </div>
      </div>
    );
  }

  return (
    <div className={kind} data-folder={card.folder} onKeyDown={onKeyDown}>
      <div className="lr-main">
        <div className="cc-name">
          <h2 className="h-section">
            <button ref={open} className="cc-open" aria-describedby={detail} onClick={props.onOpen}>
              {name}
            </button>
          </h2>
          <ExamFact card={card} />
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
        {hasProofs && <Level proofs={proofs} size="sm" title={`${proofs.earned} of ${proofs.total} proofs`} />}
      </div>
      <div className="lr-remove">
        {removeButton}
        <span className="row-chev" aria-hidden="true">
          <Icon name="right" size={14} />
        </span>
      </div>
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
  // The window moves no focus by itself: a card is focused only once an arrow key has moved to it.
  const [moved, setMoved] = useState(false);
  const count = cards?.length ?? 0;
  useReturnForPrimary(cards !== null);

  useEffect(() => {
    if (focus >= count && count > 0) setFocus(count - 1);
  }, [count, focus]);

  // The three ways to add a course are one row in one style: outlined and 32px under the list; with no course yet the first is the screen's green button and all three are 40px, so they stand on one line. One button, one icon, in either place.
  const first = props.cards !== null && props.cards.length === 0;
  const size = first ? "btn" : "btn small";
  const add = (
    <div className={first ? "shelf-add big" : "shelf-add"}>
      <button className={first ? "btn primary" : size} title="Open a course folder (⌘O)" onClick={props.onPick}>
        <Icon name="folder" size={16} />
        Open a course folder…
      </button>
      <button className={size} onClick={props.onNew}>
        <Icon name="plus" size={16} />
        Make a new course…
      </button>
      <button className={size} disabled={props.finding} onClick={props.onFind} title="Looks through your home folder for courses that Magnesium was set up in from a terminal">
        <Icon name="search" size={16} />
        {props.finding ? "Looking…" : "Find course folders on this Mac"}
      </button>
    </div>
  );

  let body: JSX.Element;
  if (cards === null) {
    const holders = (props.folders.length > 0 ? props.folders : [""]).slice(0, 6).map((path, index) => {
      const name = displayName(path.split("/").filter(Boolean).pop() ?? "");
      // A placeholder takes the room a real card or row takes, so nothing below it moves when the courses are read.
      return index === 0 ? (
        <div key={path || index} className="next-card library-next ghosted" aria-hidden="true">
          <div className="cc-body">
            <div className="cc-text">
              <div className="cc-name">
                <h2 className="h-section">{name}&nbsp;</h2>
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
              <h2 className="h-section">{name}&nbsp;</h2>
            </div>
            <p className="cc-where">&nbsp;</p>
          </div>
        </div>
      );
    });
    // The notes are said only when there is a second course to tell the next one from, here as when the courses have been read.
    body = (
      <>
        <div className="sec">
          {holders.length > 1 && <MarginNote>Next</MarginNote>}
          {holders[0]}
        </div>
        {holders.length > 1 && (
          <div className="sec courses">
            <MarginNote>Courses</MarginNote>
            {holders.slice(1)}
          </div>
        )}
      </>
    );
  } else if (cards.length === 0) {
    body = (
      <div className="empty">
        <h2 className="h-section">Add your first course</h2>
        <p>A course is a folder with your notes, slides and past papers.</p>
        {add}
      </div>
    );
  } else {
    const rendered = cards.map((card, index) => {
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
          focus={moved && index === focus}
          starting={index === 0 && props.starting}
          ready={props.ready}
          onOpen={open}
          onCourse={straight ? () => props.onOpen(card.folder) : undefined}
          onForget={() => props.onForget(card.folder)}
          onMove={(by) => {
            setMoved(true);
            setFocus(Math.max(0, Math.min(count - 1, index + by)));
          }}
        />
      );
    });
    body = (
      <>
        <section className="sec" aria-label={rendered.length > 1 ? "Next" : "Course"}>
          {rendered.length > 1 && <MarginNote>Next</MarginNote>}
          {rendered[0]}
        </section>
        {rendered.length > 1 && (
          <section className="sec courses" aria-label="Courses">
            <MarginNote>Courses</MarginNote>
            {rendered.slice(1)}
          </section>
        )}
        {add}
      </>
    );
  }

  return (
    <section className="screen library" data-screen="library">
      <TopBar
        right={
          <>
            <button className="menu-key" title="Check this Mac is ready for lessons" onClick={props.onSetup}>
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
            <h1 className="h-title">Library</h1>
          </header>
          {!props.ready && (
            <Fault
              title="Something on this Mac needs fixing"
              action={
                <button className="btn primary" onClick={props.onSetup}>
                  Fix it
                </button>
              }
            >
              A lesson cannot start until it is fixed.
            </Fault>
          )}
          {body}
          {cards !== null && count > 0 && (
            // Until the week is known its room is kept, so nothing moves when it arrives.
            <div className="shelf-foot sec">
              <MarginNote>This week</MarginNote>
              {props.activity ? <WeekLine activity={props.activity} /> : <span className="week-line" aria-hidden="true" />}
            </div>
          )}
          {props.notice && <div className="banner">{props.notice}</div>}
          {props.error && <Fault>{props.error}</Fault>}
        </div>
      </div>
    </section>
  );
}
