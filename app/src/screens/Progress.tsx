// Progress counts one thing: an answer reproduced alone on a later day. Each
// one is a proof, three prove a skill, and the level moves only when a proof
// is earned. Never counted: minutes, sessions, or answers the tutor wrote.
// The study days are shown beside it, and no proof depends on them.

import { useState } from "react";

import { Line } from "../ui/Rich";
import { Icon } from "../ui/icons";
import { Choice, Due, ExamSentence, Fault, Level, Tally, PaletteKey, ProofDot, ProofDots, RUN_HINT, Run, TopBar, Week, dueCounts, plain, proofsOf, skillsProven, useReturnForPrimary } from "../ui/bits";
import type { Activity, Progress as ProgressData, Row } from "../types";
import { displayName } from "./Library";

/** The runtime's note for a miss that no lesson has repaired yet. It has no next redo until one does. */
const FIX_FIRST = "fix it first";

/** What a skill's note says of its last answer, worded as what it is: a slip is named a slip, a right answer a right answer, and a skill not tested yet has no last answer at all. */
function lastLine(row: Row, tone: string): string | null {
  if (!row.what || tone === "untested") return null;
  return /^right\b/i.test(row.what.trim()) ? `Last answer: ${plain(row.what)}` : `Last slip: ${plain(row.what)}`;
}

function RowView({ row, tone }: { row: Row; tone: string }) {
  const last = lastLine(row, tone);
  return (
    <div className={`skill ${tone}${row.due ? " is-due" : ""} selectable`}>
      <ProofDots earned={proofsOf(row.dots)} />
      <span className="skill-text">
        <Line text={row.name} className="skill-name" />
        {last && <Line text={last} className="skill-what" />}
      </span>
      {row.due ? (
        // The section this row stands in says "to prove today"; the row keeps the amber clock that says it too.
        <Due bare />
      ) : row.note === FIX_FIRST ? (
        <span className="skill-note">needs a fix first</span>
      ) : (
        row.note && row.note !== "–" && <span className="skill-note">{row.note}</span>
      )}
    </div>
  );
}

function Group({ title, count, rows, fold, tone, action }: { title: string; count: string; rows: Row[]; fold: number; tone: string; action?: JSX.Element | null }) {
  const [all, setAll] = useState(false);
  if (rows.length === 0) return null;
  const shown = all ? rows : rows.slice(0, fold);
  return (
    <section className="group">
      <h2 className="sec-name">{title}</h2>
      <div className="group-head">
        <span className="h-count">{count}</span>
        {action}
      </div>
      {shown.map((row, index) => (
        <RowView key={`${row.name}-${index}`} row={row} tone={tone} />
      ))}
      {rows.length > shown.length && (
        <button className="more" onClick={() => setAll(true)}>
          Show {rows.length - shown.length} more
        </button>
      )}
    </section>
  );
}

/** The runtime's empty-board message. It names a marked paper and a quick check, which a student has not met; the page says the path in plain words instead. */
const NOTHING_YET = /^Nothing is proved yet/;

const WEEK_NAMES = ["This week", "Last week", "2 weeks ago", "3 weeks ago", "4 weeks ago"];

/** A paper's score as a share, from "24/30". */
function scoreShare(score: string): number | null {
  const parts = /^\s*([\d.]+)\s*\/\s*([\d.]+)/.exec(score);
  if (!parts) return null;
  const total = Number(parts[2]);
  return total > 0 ? Math.min(1, Number(parts[1]) / total) : null;
}

/** "24/30" said as "24 of 30"; any other form as it was written. */
function scoreText(score: string): string {
  const parts = /^\s*([\d.]+)\s*\/\s*([\d.]+)\s*$/.exec(score);
  return parts ? `${parts[1]} of ${parts[2]}` : score;
}

/** "2026-11-08" as "Sun 8 Nov"; any other form as it was written. */
function dayText(date: string): string {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date)) return date;
  return new Date(`${date}T12:00:00`).toLocaleDateString("en-GB", { weekday: "short", day: "numeric", month: "short" }).replace(",", "");
}

export function Progress(props: {
  data: ProgressData | null;
  activity: Activity | null;
  error: string | null;
  folderName: string;
  backLabel: string;
  onTarget: (target: string) => void;
  /** Continue the lesson, which opens with what is due. Absent while a lesson or a paper is running. */
  onStart?: () => void;
  /** The quick checks and paper redos due today, or null when the window cannot tell. */
  questionsDue: number | null;
  onBack: () => void;
  onPalette: () => void;
}) {
  const { data, activity } = props;
  // Return does Continue, the one green button, when nothing has focus; the arrow, Space and Page keys scroll the page.
  useReturnForPrimary(data !== null);
  const bar = (
    <TopBar
      left={
        <button className="crumb" onClick={props.onBack} title={`${props.backLabel} (Esc)`}>
          <Icon name="back" size={16} />
          {props.backLabel}
        </button>
      }
      right={<PaletteKey onClick={props.onPalette} />}
    />
  );
  if (!data) {
    return (
      <section className="screen progress" data-screen="progress">
        {bar}
        <div className="scroll">
          <div className="board">{props.error && <Fault>{props.error}</Fault>}</div>
        </div>
      </section>
    );
  }
  const exam = data.exam;
  const proofs = data.proofs;
  const due = data.in_progress.filter((row) => row.due);
  const later = data.in_progress.filter((row) => !row.due);
  const sure = data.scores.filter((s) => s.sure_total !== null && s.sure_right !== null);
  const skills = skillsProven(proofs);
  const levelShown = !data.level_unknown && proofs.levelled && proofs.total > 0;
  const waiting = [...due, ...later].some((row) => row.note === FIX_FIRST);

  return (
    <section className="screen progress" data-screen="progress">
      {bar}
      <div className="scroll">
        <div className="board">
          <header className="board-head">
            <div className="board-title">
              <h1 className="h-title">Proofs</h1>
              <p className="board-course">{displayName(props.folderName)}</p>
              <p className="board-what">Answers you got right on your own, each on its own day.</p>
              {data.targets.length > 1 ? (
                <div className="board-targets">
                  <Choice
                    label="Exam"
                    value={exam?.target ?? null}
                    options={data.targets.map((target) => ({ value: target.target, label: <Line text={target.title} /> }))}
                    onChange={props.onTarget}
                  />
                </div>
              ) : (
                exam && (
                  <p className="exam-line">
                    <ExamSentence exam={exam} />
                  </p>
                )
              )}
            </div>
          </header>

          {skills && (
            // The level and the next step lead; the counts are the detail under it, said of the one exam they are counted for, so the two reconcile.
            <div className="board-level">
              {levelShown && <Level proofs={proofs} size="lg" title={`${proofs.earned} of ${proofs.total} proofs`} />}
              <p className="board-sum num">
                For {exam ? <Line text={exam.title} /> : "this course"}:{" "}
                {proofs.levelled && proofs.total > 0 ? `${proofs.earned} of ${proofs.total} proofs earned, ${skills}.` : `${skills}. A course with no plan has no level.`}
              </p>
              {levelShown && <p className="board-link">Three proofs prove a skill. Each proof moves your level.</p>}
            </div>
          )}

          {data.message ? (
            <div className="board-empty">
              {NOTHING_YET.test(data.message) ? (
                <>
                  <p>Nothing is proved yet.</p>
                  <p>Answer a task in a lesson without a hint. When the tutor marks it right, that is your first proof.</p>
                  {props.onStart && (
                    <div className="row">
                      <button className="btn primary" title="Start a lesson (Return)" onClick={props.onStart}>
                        Start a lesson
                      </button>
                    </div>
                  )}
                </>
              ) : (
                data.message
              )}
            </div>
          ) : (
            <>
            <section className="foot-section legend-sec">
              <h2 className="sec-name">Marks</h2>
              <ul className="legend">
                <li>
                  <ProofDot state="on" />
                  <span>A proof: one right answer you gave alone, on its own day. Three prove a skill.</span>
                </li>
                <li>
                  <ProofDot state="off" />
                  <span>A proof still to earn. A skill that was restarted shows empty dots and says so.</span>
                </li>
                <li>
                  <span className="due" aria-hidden="true">
                    <Icon name="clock" size={14} />
                  </span>
                  <span>To prove today: you can earn this skill&apos;s next proof today.</span>
                </li>
                {waiting && (
                  <li>
                    <b className="legend-word">needs a fix first</b>
                    <span>A slip you have not gone over yet. Go over it with help in a lesson, then answer it again on another day, which can earn its next proof.</span>
                  </li>
                )}
                {levelShown && (
                  <li>
                    <Tally proofs={proofs} size="sm" />
                    <span>
                      {proofs.level >= 10
                        ? "The level is ten strokes, and all ten are full."
                        : `The level is ten strokes. Level ${proofs.level} fills ${proofs.level} of them, and the next fills as you earn the proofs for level ${proofs.level + 1}.`}
                    </span>
                  </li>
                )}
              </ul>
            </section>
              <Group
                title="To prove today"
                count={dueCounts(due.length, props.questionsDue)}
                rows={due}
                fold={12}
                tone="open"
                action={
                  props.onStart ? (
                    <button className="btn primary" title="Continue (Return)" onClick={props.onStart}>
                      Continue
                    </button>
                  ) : null
                }
              />
              <Group title="In progress" count={dueCounts(later.length, null)} rows={later} fold={10} tone="open" />
              <Group title="Proven" count={dueCounts(data.confirmed.length, null)} rows={data.confirmed} fold={6} tone="proven" />
              <Group title="Not tested yet" count={dueCounts(data.untested.length, null)} rows={data.untested} fold={6} tone="untested" />

              <footer className="board-foot selectable">
                {data.scores.length > 0 && (
                  <section className="foot-section">
                    <h2 className="sec-name">Papers</h2>
                    <ol className="scores">
                      {data.scores.map((score) => {
                        const share = scoreShare(score.score);
                        return (
                          <li className="score" key={`${score.paper}${score.date}`}>
                            <span className="score-name">{score.paper}</span>
                            <span className="score-date">{dayText(score.date)}</span>
                            <b className="score-num num">{scoreText(score.score)}</b>
                            {share !== null && (
                              <span className="score-rule" aria-hidden="true">
                                <i style={{ width: `${Math.max(2, Math.round(share * 100))}%` }} />
                              </span>
                            )}
                          </li>
                        );
                      })}
                    </ol>
                  </section>
                )}
                {sure.length > 0 && (
                  <section className="foot-section">
                    <h2 className="sec-name">Sure but wrong</h2>
                    <p>
                      {sure
                        .map((score) => `${(score.sure_total ?? 0) - (score.sure_right ?? 0)} of ${score.sure_total} on ${score.paper}`)
                        .join(", then ")}
                      .
                    </p>
                  </section>
                )}
                {data.helped_count > 0 && (
                  <section className="foot-section">
                    <h2 className="sec-name">With help, not counted</h2>
                    <p>
                      {data.helped_count === 1 ? "1 answer" : `${data.helped_count} answers`}
                      {data.helped.length > 0 ? `: ${data.helped.join(", ")}` : ""}. {data.helped_count === 1 ? "It returns" : "They return"} as{" "}
                      {data.helped_count === 1 ? "a question" : "questions"}.
                    </p>
                  </section>
                )}
                {activity && (
                  <section className="foot-section" title={RUN_HINT}>
                    <h2 className="sec-name">Days you studied</h2>
                    <p className="foot-run">
                      <Run days={activity.run} invite />
                    </p>
                    <div className="weeks">
                      {WEEK_NAMES.map((name, back) => (
                        <div className="weeks-row" key={name}>
                          <span className="weeks-name">{name}</span>
                          <Week days={activity.days} today={activity.today} back={back} letters={back === 0} />
                        </div>
                      ))}
                    </div>
                    <p className="foot-key">A filled square is a day with an answer marked, a grey one a day without, an outline a day still to come. The rule under a square is today.</p>
                  </section>
                )}
                {(data.level_unknown || data.checks_unknown || data.unreadable > 0) && (
                  <div className="foot-notes">
                    {data.level_unknown && (
                      <p>
                        No level is shown. This version of Magnesium cannot tell the window how far each skill is proved, or it has not read this
                        plan&apos;s gap log into your record yet. Update Magnesium, or mark one more answer with the tutor.
                      </p>
                    )}
                    {data.checks_unknown && (
                      <p>
                        Quick checks are not shown. This version of Magnesium keeps them where the window cannot read them yet. The tutor still
                        asks them as each lesson opens.
                      </p>
                    )}
                    {data.unreadable > 0 && (
                      <p>
                        {data.unreadable} {data.unreadable === 1 ? "row" : "rows"} of the gap log could not be read and{" "}
                        {data.unreadable === 1 ? "is" : "are"} left out. Ask the tutor to fix your plan files.
                      </p>
                    )}
                  </div>
                )}
              </footer>
            </>
          )}
          {props.error && <Fault>{props.error}</Fault>}
        </div>
      </div>
    </section>
  );
}
