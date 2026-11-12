// One course. The page answers one question, "what do I do now?", with one
// card and one button. Under it, a ruled index of three rows leads to
// everything else about the course: the proofs, a timed paper, a new lesson.

import { useEffect, useRef, useState } from "react";

import { MaterialNote } from "../ui/Material";
import { Line } from "../ui/Rich";
import { Icon } from "../ui/icons";
import { Due, ExamSentence, GoalText, Level, PROOF_HINT, PaletteKey, ProofDots, TopBar, agoText, bucketsSentence, clockText, composing, proofsOf, sentence, skillBuckets, skillsProven, useNow, useReturnForPrimary } from "../ui/bits";
import type { Home, Materials, Paper, Progress } from "../types";
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
  /** The files already in the course's material folder, or null while they are being read or cannot be. */
  materials: Materials | null;
}) {
  const { home, progress } = props;
  const [goal, setGoal] = useState("");
  // "Not now" to the upgrade, for this folder while it stays open.
  const [declined, setDeclined] = useState(false);
  useEffect(() => setDeclined(false), [home.folder]);
  useReturnForPrimary();
  const now = useNow(home.sitting ? 1000 : 60_000);
  const input = useRef<HTMLInputElement>(null);
  const fresh = home.state === "empty" || home.state === "not_set_up";
  // A new course leads with what to learn. Adding material is a quiet row under it, never a step to get past.
  const asking = fresh || props.asking;

  useEffect(() => {
    if (asking) input.current?.focus();
  }, [asking]);

  /** A file's name was pressed while the card asks what to learn: it asks for a lesson from that file. */
  const teachFrom = (name: string) => {
    setGoal(`Teach me from ${name}`);
    input.current?.focus();
  };

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
  // The skills those questions are for, when the runtime has said which skills are due.
  const dueSkills = progress && !progress.message ? progress.in_progress.filter((row) => row.due).length : null;
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
  } else if (asking) {
    const start = () => goal.trim() !== "" && !props.starting && props.onNewLesson(goal.trim());
    card = (
      <>
        <h2 className="h-section">What do you want to learn?</h2>
        <input
          ref={input}
          className="field"
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
              <Due n={due} skills={dueSkills} />
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
  // Up to five skills in play, in the order the Proofs page lists them: due today first, then the others.
  const inPlay =
    progress && !progress.message
      ? [...progress.in_progress.filter((row) => row.due), ...progress.in_progress.filter((row) => !row.due)].map((row) => ({
          name: row.name,
          earned: proofsOf(row.dots),
        }))
      : [];
  const allSkills = proofs?.skills ?? 0;

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
                    <span className="index-skills">
                      {inPlay.slice(0, 5).map((row, at) => (
                        <span className="index-skill" key={`${row.name}${at}`}>
                          <ProofDots earned={row.earned} />
                          <Line text={row.name} className="index-skill-name" />
                        </span>
                      ))}
                    </span>
                  )}
                  {allSkills > 0 && <span className="index-all num">{allSkills > 1 ? `All ${allSkills} skills` : "Open Proofs"}</span>}
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
          {usable && !sitting && (
            <div className="material-strip" data-role="material-strip">
              <Icon name="clip" size={16} />
              {props.materials && props.materials.files + props.materials.keys > 0 ? (
                <MaterialHeld listed={props.materials} onPick={asking ? teachFrom : null} />
              ) : (
                <span>Add course material: drop files here, or choose them</span>
              )}
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

/**
 * What the course's material folder holds, in a line: how many files, the
 * newest by name, and how many answer keys, which are counted and never
 * named. While the card asks what to learn, a name asks for a lesson from it.
 */
function MaterialHeld({ listed, onPick }: { listed: Materials; onPick: ((name: string) => void) | null }) {
  const names = listed.names.slice(0, 2);
  const more = listed.files - names.length;
  const name = (each: string) =>
    onPick ? (
      <button key={each} className="file-name" title={`Teach me from ${each}`} onClick={() => onPick(each)} data-role="material-file">
        {each}
      </button>
    ) : (
      <span key={each} className="file-name">
        {each}
      </span>
    );
  const keys = listed.keys === 1 ? "1 answer key" : `${listed.keys} answer keys`;
  return (
    <span className="material-held" data-role="material-held">
      {listed.files > 0 && (
        <>
          {listed.files} {listed.files === 1 ? "file" : "files"}:{" "}
          {names.length === 2 && more <= 0 ? (
            <>
              {name(names[0])} and {name(names[1])}
            </>
          ) : names.length === 2 ? (
            <>
              {name(names[0])}, {name(names[1])} and {more} more
            </>
          ) : (
            names.map(name)
          )}
          {listed.keys > 0 && <>, and {keys}</>}
        </>
      )}
      {listed.files === 0 && <>{keys}, kept closed</>}
    </span>
  );
}
