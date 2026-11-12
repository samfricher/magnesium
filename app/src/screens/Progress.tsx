// Progress counts one thing: an answer reproduced alone on a later day. Each
// one is a proof, three prove a skill, and the level moves only when a proof
// is earned. Never counted: minutes, sessions, or answers the tutor wrote.
// The study days are shown beside it, and no proof depends on them.

import { useState } from "react";

import { Line } from "../ui/Rich";
import { Icon } from "../ui/icons";
import { Due, Level, PaletteKey, ProofDot, ProofDots, RUN_HINT, Run, TopBar, Week, ExamSentence, plain, proofShare, proofsOf, skillCounts } from "../ui/bits";
import type { Activity, Progress as ProgressData, Row } from "../types";
import { displayName } from "./Library";

function RowView({ row, tone }: { row: Row; tone: string }) {
  return (
    <div className={`skill ${tone}${row.due ? " is-due" : ""} selectable`}>
      <ProofDots earned={proofsOf(row.dots)} />
      <span className="skill-text">
        <Line text={row.name} className="skill-name" />
        {row.what && <Line text={plain(row.what)} className="skill-what" />}
      </span>
      {row.due ? <Due /> : row.note && row.note !== "–" && <span className="skill-note">{row.note}</span>}
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
        {title} <span>({rows.length})</span>
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

const WEEK_NAMES = ["This week", "Last week", "2 weeks ago", "3 weeks ago", "4 weeks ago"];

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
  const skills = skillCounts(proofs);
  const levelShown = !data.level_unknown && proofs.levelled;

  return (
    <section className="screen progress" data-screen="progress">
      {bar}
      <div className="scroll">
        <div className="board">
          <header className="board-head">
            {levelShown && <Level value={proofs.level} size="md" share={proofShare(proofs)} title={`${proofs.earned} of ${proofs.total} proofs`} />}
            <div className="board-title">
              <h1>{displayName(props.folderName)}</h1>
              {data.targets.length > 1 ? (
                <div className="targets" role="group" aria-label="Exam">
                  {data.targets.map((target) => (
                    <button key={target.target} className={target.target === exam?.target ? "on" : undefined} aria-pressed={target.target === exam?.target} onClick={() => props.onTarget(target.target)}>
                      <Line text={target.title} />
                    </button>
                  ))}
                </div>
              ) : (
                exam && (
                  <p className="exam-line">
                    <ExamSentence exam={exam} />
                  </p>
                )
              )}
              {skills && <p className="board-sum">{skills}. A course with no plan has no level.</p>}
              {proofs.levelled && proofs.total > 0 && (
                <>
                  <p className="board-sum num">
                    {proofs.earned} of {proofs.total} proofs. {proofs.proven} of {proofs.skills} skills proven.
                  </p>
                  {proofs.to_next > 0 && (
                    <p className="board-next num">
                      {proofs.to_next} more {proofs.to_next === 1 ? "proof" : "proofs"} to reach level {proofs.level + 1}.
                    </p>
                  )}
                </>
              )}
            </div>
          </header>

          {data.message ? (
            <div className="board-empty">{data.message}</div>
          ) : (
            <>
              <ul className="legend" aria-label="What the marks mean">
                <li>
                  <ProofDot state="on" />
                  <span>A proof: one right answer you gave alone, on its own day. Three prove a skill.</span>
                </li>
                <li>
                  <ProofDot state="off" />
                  <span>A proof still to earn. A skill that was restarted shows empty dots and says so.</span>
                </li>
                <li>
                  <Due />
                  <span>You can earn this skill&apos;s next proof today.</span>
                </li>
                {levelShown && (
                  <li>
                    <span className="legend-text">The level&apos;s ring fills as you earn proofs.</span>
                  </li>
                )}
              </ul>
              <Group
                title="Prove today"
                rows={due}
                fold={12}
                tone="open"
                action={
                  props.onStart ? (
                    <button className="btn primary" onClick={props.onStart}>
                      Start
                    </button>
                  ) : null
                }
              />
              <Group title="In progress" rows={later} fold={10} tone="open" />
              <Group title="Proven" rows={data.confirmed} fold={6} tone="proven" />
              <Group title="Not tested yet" rows={data.untested} fold={6} tone="untested" />

              <footer className="board-foot selectable">
                {data.scores.length > 0 && (
                  <section className="foot-section">
                    <h3>
                      Papers <span>({data.scores.length})</span>
                    </h3>
                    <div className="scores">
                      {data.scores.map((score) => {
                        const share = scoreShare(score.score);
                        return (
                          <div className="score" key={`${score.paper}${score.date}`}>
                            <span className="score-bar">
                              <i style={{ height: `${Math.round((share ?? 0) * 100)}%` }} />
                            </span>
                            <b className="num">{score.score}</b>
                            <span>{score.paper}</span>
                          </div>
                        );
                      })}
                    </div>
                  </section>
                )}
                {sure.length > 0 && (
                  <section className="foot-section">
                    <h3>Sure but wrong</h3>
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
                    <h3>
                      With help, not counted <span>({data.helped_count})</span>
                    </h3>
                    <p>
                      {data.helped_count} {data.helped_count === 1 ? "answer" : "answers"}
                      {data.helped.length > 0 && ` (${data.helped.join(", ")})`}. They return as questions.
                    </p>
                  </section>
                )}
                {activity && (
                  <section className="foot-section" title={RUN_HINT}>
                    <h3>
                      Days you studied <Run days={activity.run} invite />
                    </h3>
                    <div className="weeks">
                      {WEEK_NAMES.map((name, back) => (
                        <div className="weeks-row" key={name}>
                          <span className="weeks-name">{name}</span>
                          <Week days={activity.days} today={activity.today} back={back} letters={back === 0} />
                        </div>
                      ))}
                    </div>
                    <p className="foot-key">A filled circle is a day with an answer marked. The ringed circle is today.</p>
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
                        asks them as each sitting opens.
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
          {props.error && <div className="banner bad selectable">{props.error}</div>}
        </div>
      </div>
    </section>
  );
}
