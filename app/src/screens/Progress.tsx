// Progress counts one thing: an answer reproduced alone on a later day. Each
// one is a proof, three prove a skill, and the level moves only when a proof
// is earned. Never counted: minutes, sessions, or answers the tutor wrote.
// The study days are shown beside it, and no proof depends on them.

import { useState } from "react";

import { Line } from "../ui/Rich";
import { Icon } from "../ui/icons";
import { Dots, PaletteKey, RUN_HINT, Ring, TopBar, Weeks, daysLeftText, proofShare, urgency } from "../ui/bits";
import type { Activity, Progress as ProgressData, Row } from "../types";
import { displayName } from "./Library";

function RowView({ row, tone }: { row: Row; tone: string }) {
  return (
    <div className={`skill ${tone}${row.due ? " is-due" : ""} selectable`}>
      <Dots dots={row.dots} />
      <span className="skill-text">
        <Line text={row.name} className="skill-name" />
        {row.what && <Line text={row.what} className="skill-what" />}
      </span>
      {row.note && row.note !== "–" && <span className="skill-note">{row.note}</span>}
    </div>
  );
}

function Group({ title, rows, fold, tone, action }: { title: string; rows: Row[]; fold: number; tone: string; action?: JSX.Element | null }) {
  const [all, setAll] = useState(false);
  if (rows.length === 0) return null;
  const shown = all ? rows : rows.slice(0, fold);
  return (
    <section className="group">
      <h3>
        {title}
        <span>{rows.length}</span>
        {action}
      </h3>
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

/** A paper's score as a share, from "24/30". */
function scoreShare(score: string): number | null {
  const parts = /^\s*([\d.]+)\s*\/\s*([\d.]+)/.exec(score);
  if (!parts) return null;
  const total = Number(parts[2]);
  return total > 0 ? Math.min(1, Number(parts[1]) / total) : null;
}

export function Progress(props: {
  data: ProgressData | null;
  activity: Activity | null;
  error: string | null;
  folderName: string;
  backLabel: string;
  onTarget: (target: string) => void;
  /** Start a sitting, which opens with what is due. Absent while a lesson or a paper is running. */
  onStart?: () => void;
  onBack: () => void;
  onPalette: () => void;
}) {
  const { data, activity } = props;
  const bar = (
    <TopBar
      left={
        <button className="crumb" onClick={props.onBack} title="Back (Esc)">
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
          <div className="board">{props.error && <div className="banner bad selectable">{props.error}</div>}</div>
        </div>
      </section>
    );
  }
  const exam = data.exam;
  const proofs = data.proofs;
  const due = data.in_progress.filter((row) => row.due);
  const later = data.in_progress.filter((row) => !row.due);
  const sure = data.scores.filter((s) => s.sure_total !== null && s.sure_right !== null);
  const left = daysLeftText(exam?.days_left);

  return (
    <section className="screen progress" data-screen="progress">
      {bar}
      <div className="scroll">
        <div className="board">
          <header className="board-head">
            {!data.level_unknown && (
              <Ring share={proofShare(proofs)} size={104} stroke={8} label={`Level ${proofs.level}`}>
                <b>{proofs.level}</b>
                <small>level</small>
              </Ring>
            )}
            <div className="board-title">
              <h1>{displayName(props.folderName)}</h1>
              {data.targets.length > 1 ? (
                <div className="targets">
                  {data.targets.map((target) => (
                    <button key={target.target} className={target.target === exam?.target ? "on" : undefined} onClick={() => props.onTarget(target.target)}>
                      <Line text={target.title} />
                    </button>
                  ))}
                </div>
              ) : (
                exam && (
                  <div className={`exam-line ${urgency(exam)}`}>
                    <Icon name="flag" size={15} />
                    <Line text={exam.title} />
                    {left && <b>{left}</b>}
                  </div>
                )
              )}
              {proofs.total > 0 && (
                <p className="board-sum">
                  <b>{proofs.earned}</b> of {proofs.total} proofs · <b>{proofs.proven}</b> of {proofs.skills} skills proven
                  {proofs.to_next > 0 && (
                    <span>
                      {" "}
                      · {proofs.to_next} more for level {proofs.level + 1}
                    </span>
                  )}
                </p>
              )}
            </div>
            {activity && (
              <div className="board-days" title={RUN_HINT}>
                <Weeks activity={activity} />
                <span className="run-line">
                  {activity.run > 0 ? (
                    <>
                      <b>{activity.run}</b> {activity.run === 1 ? "day" : "days"} running
                    </>
                  ) : (
                    "One answer starts a run"
                  )}
                </span>
              </div>
            )}
          </header>

          {data.message ? (
            <div className="board-empty">{data.message}</div>
          ) : (
            <>
              <p className="legend">
                <Dots dots={[2, 2, 0]} />
                One dot is one answer you gave alone, on its own day. Three prove the skill.
              </p>
              <Group
                title="Prove today"
                rows={due}
                fold={12}
                tone="open"
                action={
                  props.onStart ? (
                    <button className="btn small primary" onClick={props.onStart}>
                      Start
                    </button>
                  ) : null
                }
              />
              <Group title="In progress" rows={later} fold={10} tone="open" />
              <Group title="Proven" rows={data.confirmed} fold={6} tone="proven" />
              <Group title="Not tested yet" rows={data.untested} fold={6} tone="untested" />

              {(data.scores.length > 0 || data.helped_count > 0 || data.checks_unknown || data.level_unknown || data.unreadable > 0) && (
                <footer className="board-foot selectable">
                  {data.scores.length > 0 && (
                    <div className="papers">
                      <h3>Papers</h3>
                      <div className="scores">
                        {data.scores.map((score) => {
                          const share = scoreShare(score.score);
                          return (
                            <div className="score" key={`${score.paper}${score.date}`}>
                              <span className="score-bar">
                                <i style={{ height: `${Math.round((share ?? 0) * 100)}%` }} />
                              </span>
                              <b>{score.score}</b>
                              <span>{score.paper}</span>
                            </div>
                          );
                        })}
                      </div>
                    </div>
                  )}
                  <div className="foot-notes">
                    {sure.length > 0 && (
                      <p>
                        <b>Sure but wrong</b>{" "}
                        {sure.map((score, index) => (
                          <span key={`${score.paper}${score.date}`}>
                            {index > 0 && ", then "}
                            {(score.sure_total ?? 0) - (score.sure_right ?? 0)} of {score.sure_total} on {score.paper}
                          </span>
                        ))}
                      </p>
                    )}
                    {data.helped_count > 0 && (
                      <p>
                        <b>With help, not counted</b> {data.helped_count} {data.helped_count === 1 ? "answer" : "answers"}
                        {data.helped.length > 0 && ` (${data.helped.join(", ")})`}. They return as questions.
                      </p>
                    )}
                    {data.level_unknown && (
                      <p>
                        <b>No level</b> is shown. This version of Magnesium cannot tell the window how far each skill is proved, or it has
                        not read this plan&apos;s gap log into your record yet. Update Magnesium, or mark one more answer with the tutor.
                      </p>
                    )}
                    {data.checks_unknown && (
                      <p>
                        <b>Quick checks</b> are not shown. This version of Magnesium keeps them where the window cannot read them yet. The tutor
                        still asks them as each sitting opens.
                      </p>
                    )}
                    {data.unreadable > 0 && (
                      <p>
                        {data.unreadable} {data.unreadable === 1 ? "row" : "rows"} of the gap log could not be read and{" "}
                        {data.unreadable === 1 ? "is" : "are"} left out. Ask the tutor to fix your plan files.
                      </p>
                    )}
                  </div>
                </footer>
              )}
            </>
          )}
          {props.error && <div className="banner bad selectable">{props.error}</div>}
        </div>
      </div>
    </section>
  );
}
