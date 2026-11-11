// One course. The page answers one question, "what do I do now?", with one
// card and one button. Under it, three quiet tiles lead to everything else
// about the course: the proofs, a timed paper, a new lesson.

import { useEffect, useRef, useState } from "react";

import { MaterialNote } from "../ui/Material";
import { Line } from "../ui/Rich";
import { Icon } from "../ui/icons";
import { PROOF_HINT, PaletteKey, Ring, TopBar, agoText, clockText, composing, daysLeftText, proofShare, urgency, useNow } from "../ui/bits";
import type { Home, Paper, Progress, Row } from "../types";
import { displayName } from "./Library";

/** How far one skill is, as a word the styles colour. */
function skillState(row: Row, group: "confirmed" | "in_progress" | "untested"): string {
  if (group === "confirmed") return "p3";
  if (group === "untested") return "p0";
  const earned = row.dots.filter((dot) => dot === 2).length;
  return `p${earned}${row.due ? " due" : ""}`;
}

/** Every skill of the course as one small mark: the whole proof at a glance. */
export function SkillMarks({ progress, limit }: { progress: Progress; limit: number }) {
  const marks = [
    ...progress.confirmed.map((row) => skillState(row, "confirmed")),
    ...progress.in_progress.map((row) => skillState(row, "in_progress")),
    ...progress.untested.map((row) => skillState(row, "untested")),
  ];
  const shown = marks.slice(0, limit);
  return (
    <span className="marks" aria-hidden="true">
      {shown.map((state, index) => (
        <i key={index} className={state} />
      ))}
      {marks.length > shown.length && <b>+{marks.length - shown.length}</b>}
    </span>
  );
}

export function Course(props: {
  home: Home;
  progress: Progress | null;
  /** True while the card asks what to learn, for a new lesson in a folder that has one. */
  asking: boolean;
  error: string | null;
  /** One line that something finished, such as an upgrade. */
  notice: string | null;
  upgrading: boolean;
  starting: boolean;
  onUpgrade: () => void;
  onContinue: (skipChecks: boolean) => void;
  onNewLesson: (goal: string) => void;
  onAsk: (asking: boolean) => void;
  onPaper: (paper: Paper | null) => void;
  onProgress: () => void;
  onRetry: () => void;
  onLibrary: () => void;
  onPalette: () => void;
  /** What adding course material said, until it is dismissed. */
  material: string[] | null;
  adding: boolean;
  onDismissMaterial: () => void;
  onAddFiles: () => void;
  onPasteText: () => void;
}) {
  const { home, progress } = props;
  const [goal, setGoal] = useState("");
  // "Not now" to the upgrade, for this folder while it stays open.
  const [declined, setDeclined] = useState(false);
  // "Skip" on the first step of a new folder, for this folder while it stays open.
  const [skipped, setSkipped] = useState(false);
  useEffect(() => {
    setDeclined(false);
    setSkipped(false);
  }, [home.folder]);
  const now = useNow(home.sitting ? 1000 : 60_000);
  const input = useRef<HTMLInputElement>(null);
  const fresh = home.state === "empty" || home.state === "not_set_up";
  // A folder with nothing in it starts with its material: slides, notes, papers.
  // Once something is added, or the step is skipped, it asks what to learn.
  const gathering = fresh && !skipped && props.material === null && !props.asking;
  const asking = !gathering && (fresh || props.asking);

  useEffect(() => {
    if (asking) input.current?.focus();
  }, [asking]);

  const exam = home.exam;
  const left = daysLeftText(exam?.days_left);
  const nextPaper = home.papers.find((paper) => !paper.sat) ?? null;
  const checks = home.checks_due ?? 0;
  // One number for what is due, the same one the library shows.
  const due = home.redos_due + checks;
  const dueParts = [
    checks > 0 ? `${checks} quick ${checks === 1 ? "check" : "checks"} first` : null,
    home.redos_due > 0 ? `${home.redos_due} paper ${home.redos_due === 1 ? "question" : "questions"} to redo` : null,
  ].filter(Boolean);
  const sitting = home.sitting;
  const proofs = progress?.proofs ?? null;
  const usable = home.state === "ready" || home.state === "empty" || home.state === "not_set_up";

  let card: JSX.Element;
  if (home.state === "no_runtime" || home.state === "unreadable") {
    card = (
      <>
        <span className="eyebrow bad">
          <Icon name="warn" size={14} /> Something to fix first
        </span>
        <h2>{home.state === "no_runtime" ? "Magnesium is not installed" : "This folder could not be read"}</h2>
        <p className="selectable">{home.message}</p>
        <div className="row">
          <button className="btn primary" autoFocus onClick={props.onRetry}>
            Try again <kbd>&#8629;</kbd>
          </button>
        </div>
      </>
    );
  } else if (home.state === "needs_upgrade") {
    card = declined ? (
      <>
        <span className="eyebrow">Not upgraded</span>
        <h2>This folder stays as it was</h2>
        <p>A lesson cannot start here until the folder is upgraded. Nothing in it was changed.</p>
        <div className="row">
          <button className="btn" disabled={props.upgrading} onClick={props.onUpgrade}>
            Upgrade
          </button>
        </div>
      </>
    ) : (
      <>
        <span className="eyebrow">One step first</span>
        <h2>Upgrade this folder?</h2>
        <p className="selectable">{home.message}</p>
        <div className="row">
          <button className="btn primary" autoFocus disabled={props.upgrading} onClick={props.onUpgrade}>
            {props.upgrading ? "Upgrading…" : "Upgrade"} {!props.upgrading && <kbd>&#8629;</kbd>}
          </button>
          {!props.upgrading && (
            <button className="btn ghost" onClick={() => setDeclined(true)}>
              Not now
            </button>
          )}
        </div>
      </>
    );
  } else if (sitting) {
    const leftSeconds = Math.round((sitting.started_ms + sitting.minutes * 60_000 - now) / 1000);
    card = (
      <>
        <span className="eyebrow now">A paper is running</span>
        <h2>
          {sitting.name} · <span className="num">{clockText(leftSeconds)}</span> {leftSeconds >= 0 ? "left" : "over"}
        </h2>
        <p>The tutor is off until you hand in.</p>
        <div className="row">
          <button className="btn primary" autoFocus onClick={() => props.onPaper(null)}>
            Back to the paper <kbd>&#8629;</kbd>
          </button>
        </div>
      </>
    );
  } else if (gathering) {
    card = (
      <>
        <span className="eyebrow">First step</span>
        <h2>Add your course material</h2>
        <p>
          Slides, notes, past papers. Drop them anywhere in this window. The tutor can search what it can read, and cites it by page.
        </p>
        <div className="row">
          <button className="btn primary" autoFocus disabled={props.adding} onClick={props.onAddFiles} data-role="add-files">
            {props.adding ? "Adding…" : "Choose files"} {!props.adding && <kbd>&#8629;</kbd>}
          </button>
          <button className="btn" disabled={props.adding} onClick={props.onPasteText} data-role="paste-text">
            Paste text
          </button>
          <span className="grow" />
          <button className="btn ghost" disabled={props.adding} onClick={() => setSkipped(true)} data-role="skip-material">
            Skip for now
          </button>
        </div>
      </>
    );
  } else if (asking) {
    const start = () => goal.trim() !== "" && !props.starting && props.onNewLesson(goal.trim());
    card = (
      <>
        <span className="eyebrow">{fresh ? "First lesson" : "New lesson"}</span>
        <h2>What do you want to learn?</h2>
        <input
          ref={input}
          className="field big"
          type="text"
          placeholder="A topic, a file in this folder, or an exam and its date"
          autoComplete="off"
          spellCheck={false}
          value={goal}
          onChange={(e) => setGoal(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !composing(e)) start();
            if (e.key === "Escape" && !fresh) {
              e.stopPropagation();
              props.onAsk(false);
            }
          }}
        />
        <div className="row">
          <button className="btn primary" disabled={goal.trim() === "" || props.starting} onClick={start}>
            {props.starting ? "Starting…" : "Start"} {!props.starting && <kbd>&#8629;</kbd>}
          </button>
          {!fresh && (
            <button className="btn ghost" onClick={() => props.onAsk(false)}>
              Back
            </button>
          )}
          {home.state === "not_set_up" && <span className="aside">Start sets this folder up for Magnesium.</span>}
        </div>
      </>
    );
  } else {
    const last = home.last;
    card = (
      <>
        <span className="eyebrow">
          {last ? "Next" : "Ready"}
          {last && last.days_ago !== null && <i>last studied {agoText(last.days_ago)}</i>}
        </span>
        <h2 className="selectable">
          <Line text={last ? last.goal : `Start on ${exam?.title ?? displayName(home.name)}`} />
        </h2>
        {last?.stopped_at ? (
          <p className="selectable">
            <b>You stopped at</b> <Line text={last.stopped_at} />
          </p>
        ) : last?.next_action ? (
          <p className="selectable">
            <b>Up next</b> <Line text={last.next_action} />
          </p>
        ) : null}
        <div className="row">
          <button className="btn primary" autoFocus disabled={props.starting} onClick={() => props.onContinue(false)}>
            {props.starting ? "Starting…" : last ? "Continue" : "Start"} {!props.starting && <kbd>&#8629;</kbd>}
          </button>
          {due > 0 && (
            <span className="dues">
              <span className="due">
                <i />
                {due} due today
              </span>
              <span className="dues-what">{dueParts.join(", then ")}</span>
            </span>
          )}
        </div>
      </>
    );
  }

  const inProgress = progress?.in_progress.length ?? 0;

  return (
    <section className="screen course" data-screen="course">
      <TopBar
        left={
          <button className="crumb" onClick={props.onLibrary} title="All courses (Esc)">
            <Icon name="back" size={16} />
            Library
          </button>
        }
        right={<PaletteKey onClick={props.onPalette} />}
      />
      <div className="scroll">
        <div className="course-col">
          <header className="course-head">
            <div className="course-title">
              <h1>{displayName(home.name)}</h1>
              {exam ? (
                <div className={`exam-line ${urgency(exam)}`}>
                  <Icon name="flag" size={15} />
                  <Line text={exam.title} />
                  {exam.day && <span>{exam.day}</span>}
                  {left && <b>{left}</b>}
                </div>
              ) : (
                <div className="exam-line none">{usable && home.state === "ready" ? "No exam set" : " "}</div>
              )}
            </div>
            {proofs && proofs.total > 0 && (
              <button className="level" onClick={props.onProgress} title={PROOF_HINT}>
                <Ring share={proofShare(proofs)} size={64} label={`Level ${proofs.level}`}>
                  <b>{proofs.level}</b>
                </Ring>
                <span>
                  <b>Level {proofs.level}</b>
                  <small>
                    {proofs.earned} of {proofs.total} proofs
                  </small>
                </span>
              </button>
            )}
          </header>

          <div className="next-card" data-card="next">
            {card}
          </div>
          {props.notice && <div className="banner selectable">{props.notice}</div>}
          {props.error && <div className="banner bad selectable">{props.error}</div>}
          {props.material && <MaterialNote lines={props.material} onDismiss={props.onDismissMaterial} />}

          {usable && !sitting && (
            <div className="tiles">
              <button className="tile" onClick={props.onProgress} title={PROOF_HINT}>
                <span className="tile-head">
                  <Icon name="chart" size={16} />
                  Proofs
                </span>
                {progress && proofs && proofs.total > 0 ? (
                  <>
                    <SkillMarks progress={progress} limit={44} />
                    <span className="tile-foot">
                      {proofs.proven} of {proofs.skills} skills proven{inProgress > 0 ? ` · ${inProgress} in progress` : ""}
                    </span>
                  </>
                ) : (
                  <span className="tile-foot">Fills in after your first marked answer</span>
                )}
              </button>
              <button className="tile" onClick={() => props.onPaper(nextPaper)}>
                <span className="tile-head">
                  <Icon name="paper" size={16} />
                  Timed paper
                </span>
                {nextPaper ? (
                  <>
                    <span className="tile-main">{nextPaper.name}</span>
                    <span className="tile-foot">{nextPaper.minutes ? `${nextPaper.minutes} min` : "Sit it cold"}</span>
                  </>
                ) : (
                  <span className="tile-foot">{home.papers.length > 0 ? "All papers sat" : "None yet"}</span>
                )}
              </button>
              {!fresh && (
                <button className="tile" onClick={() => props.onAsk(true)}>
                  <span className="tile-head">
                    <Icon name="plus" size={16} />
                    New lesson
                  </span>
                  <span className="tile-foot">
                    Another topic <kbd>&#8984;N</kbd>
                  </span>
                </button>
              )}
            </div>
          )}
          {usable && !sitting && !gathering && (
            <div className="material-strip" data-role="material-strip">
              <Icon name="clip" size={16} />
              <span>
                <b>Course material</b>
                <small>Drop files here, or</small>
              </span>
              <span className="grow" />
              <button className="btn small" disabled={props.adding} onClick={props.onAddFiles} data-role="add-files">
                {props.adding ? "Adding…" : "Choose files"}
              </button>
              <button className="btn small" disabled={props.adding} onClick={props.onPasteText} data-role="paste-text">
                Paste text
              </button>
            </div>
          )}
        </div>
      </div>
    </section>
  );
}
