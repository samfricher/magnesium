// One course. The page answers one question, "what do I do now?", with one
// card and one button. Under it, three quiet tiles lead to everything else
// about the course: the proofs, a timed paper, a new lesson.

import { useEffect, useRef, useState } from "react";

import { MaterialNote } from "../ui/Material";
import { Line } from "../ui/Rich";
import { Icon } from "../ui/icons";
import { Due, ExamSentence, Level, PROOF_HINT, PaletteKey, TopBar, agoText, clockText, composing, proofShare, sentence, skillCounts, splitGoal, useNow } from "../ui/bits";
import type { Home, Paper, Progress } from "../types";
import { displayName } from "./Library";
import { todaySentence } from "./Summary";

/** What a proof is, for a course that has none yet. */
const PROOF_DEFINITION = "A proof is a right answer with no help, on its own day. Three prove a skill.";

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
  const nextPaper = home.papers.find((paper) => !paper.sat) ?? null;
  const checks = home.checks_due ?? 0;
  // One number for what is due, the same one the library shows.
  const due = home.redos_due + checks;
  const dueParts = [
    checks > 0 ? `${checks} quick ${checks === 1 ? "check" : "checks"} first` : null,
    home.redos_due > 0 ? `${home.redos_due} paper ${home.redos_due === 1 ? "question" : "questions"} to redo` : null,
  ].filter(Boolean);
  const dueSentence = dueParts.length > 0 ? `${dueParts.join(", then ")}.`.replace(/^./, (c) => c.toUpperCase()) : null;
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
        <h2>{home.state === "no_runtime" ? "Magnesium is not installed" : "This course could not be read"}</h2>
        <p className="selectable">{home.message}</p>
        <div className="row">
          <button className="btn primary" autoFocus title="Try again (Return)" onClick={props.onRetry}>
            Try again
          </button>
        </div>
      </>
    );
  } else if (home.state === "needs_upgrade") {
    card = declined ? (
      <>
        <h2>This course stays as it was</h2>
        <p>A lesson cannot start here until the course is upgraded. Nothing in it was changed.</p>
        <div className="row">
          <button className="btn" disabled={props.upgrading} onClick={props.onUpgrade}>
            Upgrade
          </button>
        </div>
      </>
    ) : (
      <>
        <h2>Upgrade this course?</h2>
        <p className="selectable">{home.message}</p>
        <div className="row">
          <button className="btn primary" autoFocus disabled={props.upgrading} title="Upgrade (Return)" onClick={props.onUpgrade}>
            {props.upgrading ? "Upgrading…" : "Upgrade"}
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
        <p className="next-ref">A paper is running</p>
        <h2>
          {sitting.name}, <span className="num">{clockText(leftSeconds)}</span> {leftSeconds >= 0 ? "left" : "over"}
        </h2>
        <p>The tutor is off until you hand in.</p>
        <div className="row">
          <button className="btn primary" autoFocus title="Back to the paper (Return)" onClick={() => props.onPaper(null)}>
            Back to the paper
          </button>
        </div>
      </>
    );
  } else if (gathering) {
    card = (
      <>
        <h2>Add your course material</h2>
        <p>
          Slides, notes, past papers. Drop them anywhere in this window. The tutor can search what it can read, and cites it by page.
        </p>
        <div className="row">
          <button className="btn primary" autoFocus disabled={props.adding} title="Choose files (Return)" onClick={props.onAddFiles} data-role="add-files">
            {props.adding ? "Adding…" : "Choose files"}
          </button>
          <button className="btn" disabled={props.adding} onClick={props.onPasteText} data-role="paste-text">
            Paste text
          </button>
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
        <h2>What do you want to learn?</h2>
        <input
          ref={input}
          className="field big"
          type="text"
          placeholder="A topic, a file in this course, or an exam and its date"
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
          <button className="btn primary" disabled={goal.trim() === "" || props.starting} title="Start (Return)" onClick={start}>
            {props.starting ? "Starting…" : "Start"}
          </button>
          {!fresh && (
            <button className="btn ghost" onClick={() => props.onAsk(false)}>
              Back
            </button>
          )}
          {home.state === "not_set_up" && <span className="aside">Start sets this course up for Magnesium.</span>}
        </div>
      </>
    );
  } else {
    const last = home.last;
    const goal = last ? splitGoal(last.goal) : null;
    const stopped = last?.stopped_at ? sentence(last.stopped_at) : null;
    const upNext = last?.next_action ? sentence(last.next_action) : null;
    card = (
      <>
        {last && (
          <p className="next-ref">
            <span>{goal?.ref ?? ""}</span>
            {last.days_ago !== null && <span>Last studied {agoText(last.days_ago)}</span>}
          </p>
        )}
        <h2 className="selectable">
          {goal ? <Line text={goal.title} /> : <Line text={`Start on ${exam?.title ?? displayName(home.name)}`} />}
        </h2>
        {stopped ? (
          <p className="selectable">You stopped at <Line text={stopped} /></p>
        ) : upNext ? (
          <p className="selectable">Up next: <Line text={upNext} /></p>
        ) : null}
        <div className="row">
          <button className="btn primary" autoFocus disabled={props.starting} title={last ? "Continue (Return)" : "Start (Return)"} onClick={() => props.onContinue(false)}>
            {props.starting ? "Starting…" : last ? "Continue" : "Start"}
          </button>
          {due > 0 && (
            <span className="dues">
              <Due n={due} />
              {dueSentence && <span className="dues-what">{dueSentence}</span>}
            </span>
          )}
        </div>
      </>
    );
  }

  const today = todaySentence(progress?.today ?? null);
  const skills = proofs ? skillCounts(proofs) : null;

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
              <p className="exam-line">
                {exam ? <ExamSentence exam={exam} /> : usable && home.state === "ready" ? "No exam set." : " "}
              </p>
            </div>
            {skills && (
              <button className="level" onClick={props.onProgress} title={PROOF_HINT}>
                <span>{skills}</span>
              </button>
            )}
            {proofs && proofs.levelled && proofs.total > 0 && (
              <button className="level" onClick={props.onProgress} title={PROOF_HINT}>
                <Level value={proofs.level} size="md" share={proofShare(proofs)} />
                <span className="num">
                  {proofs.earned} of {proofs.total} proofs
                </span>
              </button>
            )}
          </header>

          <div className="next-card" data-card="next">
            {card}
          </div>
          {usable && !sitting && today && (
            <p className="today-line" data-role="today">
              {today}
            </p>
          )}
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
                    <span className="tile-main">
                      {proofs.proven} of {proofs.skills} skills proven
                    </span>
                    <span className="tile-foot">{progress.in_progress.length} in progress.</span>
                  </>
                ) : (
                  <span className="tile-foot">{PROOF_DEFINITION}</span>
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
                    <span className="tile-foot">
                      A timed practice exam from your past papers{nextPaper.minutes ? `, ${nextPaper.minutes} minutes` : ""}.
                    </span>
                  </>
                ) : (
                  <span className="tile-foot">
                    {home.papers.length > 0
                      ? "All papers sat. A timed practice exam from your past papers."
                      : "None yet: the tutor writes one when you ask."}
                  </span>
                )}
              </button>
              {!fresh && (
                <button className="tile" title="Another topic (⌘N)" onClick={() => props.onAsk(true)}>
                  <span className="tile-head">
                    <Icon name="plus" size={16} />
                    New lesson
                  </span>
                  <span className="tile-main">Another topic</span>
                  <span className="tile-foot">Pick what to learn next.</span>
                </button>
              )}
            </div>
          )}
          {usable && !sitting && !gathering && (
            <div className="material-strip" data-role="material-strip">
              <Icon name="clip" size={16} />
              <span>Drop files here, or choose them</span>
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
