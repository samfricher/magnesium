// The first screen: every study folder the window knows, the most urgent
// first. One card is one course. A card shows what decides where to study
// next and nothing else: the exam and how near it is, how much is proved,
// and what is due. The most urgent card's button, and Enter, go straight
// into its lesson. Under the title, one quiet choice: what runs lessons.

import { type ReactNode, useEffect, useRef, useState } from "react";

import { Line } from "../ui/Rich";
import { Icon } from "../ui/icons";
import { IconButton, PROOF_HINT, PaletteKey, Ring, TopBar, Week, agoText, clockText, daysLeftText, proofShare, skillCounts, urgency, useNow } from "../ui/bits";
import type { Activity, Card } from "../types";

/** A folder's name as a title: "Demo-Course" reads "Demo Course". */
export function displayName(name: string): string {
  return name.replace(/[-_]+/g, " ").trim() || name;
}

function statusOf(card: Card): { text: string; tone: string } | null {
  if (card.sitting) return { text: "Paper running", tone: "now" };
  switch (card.state) {
    case "not_set_up":
      return { text: "New", tone: "plain" };
    case "empty":
      return { text: "Ready to start", tone: "plain" };
    case "needs_upgrade":
      return { text: "Needs an upgrade", tone: "soon" };
    case "no_runtime":
    case "unreadable":
      return { text: "Could not be read", tone: "bad" };
    default:
      return null;
  }
}

function ExamChip({ card }: { card: Card }) {
  const status = statusOf(card);
  if (status) return <span className={`chip ${status.tone}`}>{status.text}</span>;
  const exam = card.exam;
  if (!exam) return <span className="chip plain">No exam set</span>;
  const left = daysLeftText(exam.days_left);
  return (
    <span className={`chip ${urgency(exam)}`}>
      <Line text={exam.title} />
      {left && <b>{left}</b>}
    </span>
  );
}

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
  const ref = useRef<HTMLDivElement>(null);
  const now = useNow(card.sitting ? 1000 : 60_000);

  useEffect(() => {
    if (props.focus) ref.current?.focus();
  }, [props.focus]);

  const proofs = card.proofs;
  const hasProofs = proofs.levelled && proofs.total > 0;
  const counts = skillCounts(proofs);
  const sitting = card.sitting;
  const leftSeconds = sitting ? Math.round((sitting.started_ms + sitting.minutes * 60_000 - now) / 1000) : 0;

  if (asking) {
    return (
      <div className={props.hero ? "course-card hero asking" : "course-card asking"}>
        <h2>Remove {displayName(card.name)}?</h2>
        <p className="cc-note">It leaves this list. The folder and your record stay on your Mac.</p>
        <div className="row">
          <button className="btn danger" autoFocus onClick={props.onForget}>
            Remove
          </button>
          <button className="btn ghost" onClick={() => setAsking(false)}>
            Keep
          </button>
        </div>
      </div>
    );
  }

  return (
    <div
      ref={ref}
      className={props.hero ? "course-card hero" : "course-card"}
      role="button"
      tabIndex={0}
      data-folder={card.folder}
      onClick={props.onOpen}
      onKeyDown={(event) => {
        if (event.target !== event.currentTarget) return;
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          props.onOpen();
        } else if (event.key === "ArrowRight" || event.key === "ArrowDown") {
          event.preventDefault();
          props.onMove(1);
        } else if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
          event.preventDefault();
          props.onMove(-1);
        }
      }}
    >
      <div className="cc-top">
        <ExamChip card={card} />
        <button
          className="icon-btn cc-more"
          title="Remove from the library"
          aria-label={`Remove ${card.name} from the library`}
          onClick={(event) => {
            event.stopPropagation();
            setAsking(true);
          }}
        >
          <Icon name="more" size={18} />
        </button>
      </div>
      <div className="cc-body">
        <div className="cc-text">
          <h2>{displayName(card.name)}</h2>
          {sitting ? (
            <p className="cc-goal">
              {sitting.name} · {clockText(leftSeconds)} {leftSeconds >= 0 ? "left" : "over"}
            </p>
          ) : card.goal ? (
            <p className="cc-goal">
              <Line text={card.goal} />
            </p>
          ) : (
            <p className="cc-goal dim">{card.state === "ready" || card.state === "empty" ? "Nothing started yet" : " "}</p>
          )}
        </div>
        {hasProofs && (
          <span title={PROOF_HINT}>
            <Ring share={proofShare(proofs)} size={props.hero ? 84 : 52} label={`Level ${proofs.level}, ${proofs.earned} of ${proofs.total} proofs`}>
              <b>{proofs.level}</b>
              {props.hero && <small>level</small>}
            </Ring>
          </span>
        )}
      </div>
      <div className="cc-foot">
        {props.hero && (
          <span className="btn primary as-label">
            {props.starting ? "Starting…" : sitting ? "Back to the paper" : props.onCourse ? (card.goal ? "Continue" : "Start") : "Open"}
            {!props.starting && <kbd>&#8629;</kbd>}
          </span>
        )}
        {props.onCourse && (
          <button
            className="btn ghost small"
            data-role="course-page"
            title="This course's page: proofs, papers, a new lesson"
            onClick={(event) => {
              event.stopPropagation();
              props.onCourse?.();
            }}
          >
            Course page
          </button>
        )}
        {card.due > 0 && (
          <span className="due">
            <i />
            {card.due} due today
          </span>
        )}
        {counts && <span className="cc-meta">{counts}</span>}
        {hasProofs && (
          <span className="cc-meta">
            {props.hero ? `${proofs.earned} of ${proofs.total} proofs` : `Level ${proofs.level}`}
          </span>
        )}
        <span className="grow" />
        {card.days_ago !== null && <span className="cc-meta">{agoText(card.days_ago)}</span>}
      </div>
    </div>
  );
}

export function Library(props: {
  /** null while the folders are being read. */
  cards: Card[] | null;
  /** The folders' paths, known at once, to draw placeholders from. */
  folders: string[];
  activity: Activity | null;
  error: string | null;
  notice: string | null;
  finding: boolean;
  ready: boolean;
  /** A lesson is starting from the main card. */
  starting: boolean;
  /** The choice of what runs lessons, or null when the core cannot say. */
  engine: ReactNode;
  /** Open a folder and continue its lesson at once. */
  onContinue: (path: string) => void;
  /** Open a folder's page. */
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
      <button className="btn" onClick={props.onPick}>
        <Icon name="folder" size={16} />
        Open a folder
        <kbd>&#8984;O</kbd>
      </button>
      <button className="btn ghost" onClick={props.onNew}>
        <Icon name="plus" size={16} />
        New folder
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
        {(props.folders.length > 0 ? props.folders : [""]).slice(0, 6).map((path, index) => (
          <div key={path || index} className={index === 0 ? "course-card hero ghosted" : "course-card ghosted"}>
            <div className="cc-top">
              <span className="chip plain">&nbsp;</span>
            </div>
            <div className="cc-body">
              <div className="cc-text">
                <h2>{displayName(path.split("/").filter(Boolean).pop() ?? "")}&nbsp;</h2>
                <p className="cc-goal">&nbsp;</p>
              </div>
            </div>
            <div className="cc-foot">&nbsp;</div>
          </div>
        ))}
      </div>
    );
  } else if (cards.length === 0) {
    body = (
      <div className="empty">
        <span className="mark" aria-hidden="true">
          <i />
          <i />
          <i />
        </span>
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
            <IconButton icon="gear" label="Magnesium and Claude Code on this Mac" onClick={props.onSetup} />
            <PaletteKey onClick={props.onPalette} />
          </>
        }
      />
      <div className="scroll">
        <div className="shelf">
          <header className="shelf-head">
            <div className="shelf-title">
              <h1>Library</h1>
              {props.engine}
            </div>
            {props.activity && count > 0 && (
              <div className="shelf-week">
                {props.activity.run > 1 && (
                  <span className="run-line">
                    <b>{props.activity.run}</b> days running
                  </span>
                )}
                <Week activity={props.activity} />
              </div>
            )}
          </header>
          {!props.ready && (
            <button className="banner warn" onClick={props.onSetup}>
              <Icon name="warn" size={16} />
              Something on this Mac needs a look before a lesson can start.
              <span className="grow" />
              <b>Fix</b>
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
