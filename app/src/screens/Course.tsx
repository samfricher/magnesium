// One course. The page answers one question, "what do I do now?", with one
// card and one button. Under it, a ruled index of three rows leads to
// everything else about the course: the proofs, a timed paper, a new lesson.

import { useEffect, useRef, useState } from "react";

import { MaterialNote } from "../ui/Material";
import { Line } from "../ui/Rich";
import { Icon } from "../ui/icons";
import { Due, ExamSentence, GoalText, Level, PROOF_HINT, PaletteKey, ProofDots, TopBar, agoText, bucketsSentence, clockText, composing, proofsOf, sentence, skillBuckets, skillsProven, useNow, useReturnForPrimary } from "../ui/bits";
import type { Home, Paper, Progress } from "../types";
import { displayName } from "./Library";
import { todaySentence } from "./Summary";

/** What a proof is, for a course that has none yet. */
const PROOF_DEFINITION = "A proof is a right answer with no help, on its own day. Three prove a skill.";

/** What this window has been told about a folder's material, kept per folder: "skipped" or "added". Reads and writes never throw. */
const MATERIAL_KEY = "magnesium.material:";

function rememberedMaterial(folder: string): string | null {
  try {
    return window.localStorage.getItem(MATERIAL_KEY + folder);
  } catch {
    return null;
  }
}

function rememberMaterial(folder: string, what: "skipped" | "added"): void {
  try {
    window.localStorage.setItem(MATERIAL_KEY + folder, what);
  } catch {
    // Without storage the step is simply asked again next time.
  }
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
  // "Skip for now" on the first step of a new folder. It sticks for that folder: the step is not asked again.
  const [skipped, setSkipped] = useState(() => rememberedMaterial(home.folder) !== null);
  useEffect(() => {
    setDeclined(false);
    setSkipped(rememberedMaterial(home.folder) !== null);
  }, [home.folder]);
  // Material added through the window is remembered too, for when the folder is empty again of anything the window can see.
  useEffect(() => {
    if (props.material !== null) rememberMaterial(home.folder, "added");
  }, [props.material, home.folder]);
  useReturnForPrimary();
  const now = useNow(home.sitting ? 1000 : 60_000);
  const input = useRef<HTMLInputElement>(null);
  const fresh = home.state === "empty" || home.state === "not_set_up";
  // A course that is known to have material (papers, an exam, a lesson begun) never shows the step.
  const hasMaterial = home.papers.length > 0 || home.exam !== null || home.last !== null;
  // A folder with nothing in it starts with its material: slides, notes, papers.
  // Once something is added, or the step is skipped, it asks what to learn.
  const gathering = fresh && !hasMaterial && !skipped && props.material === null && !props.asking;
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
        <h2 className="h-section fault">
          <Icon name="warn" size={20} />
          {home.state === "no_runtime" ? "Magnesium is not installed" : "This course could not be read"}
        </h2>
        <p className="selectable">{home.message}</p>
        <div className="row">
          <button className="btn primary" title="Try again (Return)" onClick={props.onRetry}>
            Try again
          </button>
        </div>
      </>
    );
  } else if (home.state === "needs_upgrade") {
    card = declined ? (
      <>
        <h2 className="h-section">This course stays as it was</h2>
        <p>A lesson cannot start here until the course is upgraded. Nothing in it was changed.</p>
        <div className="row">
          <button className="btn" disabled={props.upgrading} onClick={props.onUpgrade}>
            Upgrade
          </button>
        </div>
      </>
    ) : (
      <>
        <h2 className="h-section">Upgrade this course?</h2>
        <p className="selectable">{home.message}</p>
        <div className="row">
          <button className="btn primary" disabled={props.upgrading} title="Upgrade (Return)" onClick={props.onUpgrade}>
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
        <h2 className="h-section">
          {sitting.name}, <span className="num">{clockText(leftSeconds)}</span> {leftSeconds >= 0 ? "left" : "over"}
        </h2>
        <p>A paper is running. The tutor is off until you hand in.</p>
        <div className="row">
          <button className="btn primary" title="Back to the paper (Return)" onClick={() => props.onPaper(null)}>
            Back to the paper
          </button>
        </div>
      </>
    );
  } else if (gathering) {
    card = (
      <>
        <h2 className="h-section">Add your course material</h2>
        <p>
          Slides, notes, past papers. Drop them anywhere in this window. The tutor can search what it can read, and cites it by page.
        </p>
        <div className="row">
          <button className="btn" disabled={props.adding} title="Choose files" onClick={props.onAddFiles} data-role="add-files">
            {props.adding ? "Adding…" : "Choose files"}
          </button>
          <button className="btn" disabled={props.adding} onClick={props.onPasteText} data-role="paste-text">
            Paste text
          </button>
          <button
            className="btn ghost"
            disabled={props.adding}
            onClick={() => {
              rememberMaterial(home.folder, "skipped");
              setSkipped(true);
            }}
            data-role="skip-material"
          >
            Skip for now
          </button>
        </div>
      </>
    );
  } else if (asking) {
    const start = () => goal.trim() !== "" && !props.starting && props.onNewLesson(goal.trim());
    card = (
      <>
        <h2 className="h-section">What do you want to learn?</h2>
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
            {props.starting ? "Starting…" : home.state === "not_set_up" ? "Start your first lesson" : "Start"}
          </button>
          {!fresh && (
            <button className="btn ghost" onClick={() => props.onAsk(false)}>
              Back
            </button>
          )}
        </div>
      </>
    );
  } else {
    const last = home.last;
    const stopped = last?.stopped_at ? sentence(last.stopped_at) : null;
    const upNext = last?.next_action ? sentence(last.next_action) : null;
    card = (
      <>
        <h2 className="h-section selectable">
          {last ? <GoalText goal={last.goal} /> : <Line text={`Start on ${exam?.title ?? displayName(home.name)}`} />}
        </h2>
        {last && last.days_ago !== null && <p className="next-meta">Last studied {agoText(last.days_ago)}</p>}
        {stopped ? (
          <p className="selectable">You stopped at <Line text={stopped} /></p>
        ) : upNext ? (
          <p className="selectable">Up next: <Line text={upNext} /></p>
        ) : null}
        <div className="row">
          <button className="btn primary" disabled={props.starting} title={last ? "Continue (Return)" : "Start (Return)"} onClick={() => props.onContinue(false)}>
            {props.starting ? "Starting…" : last ? "Continue" : "Start your first lesson"}
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
  const skills = proofs && !proofs.levelled ? skillsProven(proofs) : null;
  const buckets = progress && !progress.message ? bucketsSentence(skillBuckets(progress)) : null;
  // The skills in play: those with a proof to earn, the ones due today first.
  const inPlay = progress ? [...progress.in_progress.filter((row) => row.due), ...progress.in_progress.filter((row) => !row.due)] : [];

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
              <h1 className="h-title">{displayName(home.name)}</h1>
              <p className="exam-line">
                {exam ? <ExamSentence exam={exam} /> : usable && home.state === "ready" ? "No exam set." : " "}
              </p>
            </div>
            {skills && (
              <button className="level" onClick={props.onProgress} title={PROOF_HINT}>
                <span className="lvl-text">{skills}</span>
              </button>
            )}
            {proofs && proofs.levelled && proofs.total > 0 && (
              <button className="level" onClick={props.onProgress} title={PROOF_HINT}>
                <Level proofs={proofs} size="md" />
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
            <div className="index">
              <button className="index-row" onClick={props.onProgress} title={PROOF_HINT}>
                <span className="index-name">Proofs</span>
                <span className="index-body">
                  {progress && proofs && proofs.total > 0 ? (
                    <span className="index-what num">{buckets ?? skillsProven(proofs)}</span>
                  ) : (
                    <span className="index-what">{PROOF_DEFINITION}</span>
                  )}
                  {inPlay.length > 0 && (
                    <span className="index-marks" aria-hidden="true">
                      {inPlay.slice(0, 8).map((row, at) => (
                        <ProofDots key={`${row.name}${at}`} earned={proofsOf(row.dots)} label="" />
                      ))}
                      {inPlay.length > 8 && <span className="index-more">+{inPlay.length - 8}</span>}
                    </span>
                  )}
                </span>
                <Icon name="right" size={16} />
              </button>
              <button className="index-row" onClick={() => props.onPaper(nextPaper)}>
                <span className="index-name">Timed paper</span>
                <span className="index-body">
                  <span className="index-what">
                    {nextPaper
                      ? `${nextPaper.name}${nextPaper.minutes ? `, ${nextPaper.minutes} minutes` : ""}. A practice exam from your past papers.`
                      : home.papers.length > 0
                        ? "All papers sat. A timed practice exam from your past papers."
                        : "None yet: the tutor writes one when you ask."}
                  </span>
                </span>
                <Icon name="right" size={16} />
              </button>
              {!fresh && (
                <button className="index-row" title="Another topic (⌘N)" onClick={() => props.onAsk(true)}>
                  <span className="index-name">New lesson</span>
                  <span className="index-body">
                    <span className="index-what">Another topic: pick what to learn next.</span>
                  </span>
                  <Icon name="right" size={16} />
                </button>
              )}
            </div>
          )}
          {usable && !sitting && !gathering && (
            <div className="material-strip" data-role="material-strip">
              <Icon name="clip" size={16} />
              <span>Drop files here, or choose them</span>
              <button className="btn" disabled={props.adding} onClick={props.onAddFiles} data-role="add-files">
                {props.adding ? "Adding…" : "Choose files"}
              </button>
              <button className="btn" disabled={props.adding} onClick={props.onPasteText} data-role="paste-text">
                Paste text
              </button>
            </div>
          )}
        </div>
      </div>
    </section>
  );
}
