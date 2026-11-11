// The end of a sitting. One screen that closes it, so the learner stops with
// a clear end and a reason to come back.
//
// It leads with the proofs earned in this sitting, when there are any. A
// proof is an answer given alone on a later day, so most sittings earn one
// only through the checks and redos they open with. The window works the
// proofs out by comparing the runtime's record before and after the sitting.
// It never takes the tutor's word for one. Under them: where the learner
// stopped, what can be proved tomorrow, and today's place in the week.

import { Line } from "../ui/Rich";
import { Icon } from "../ui/icons";
import { Dots, TopBar, Week } from "../ui/bits";
import type { Activity, Row } from "../types";

export type Sitting = {
  minutes: number;
  /** Messages the learner sent, the opening line left out. */
  answers: number;
  stepsDone: number;
  stepsTotal: number;
  /** The line the learner saved, which opens the next sitting. */
  stoppedAt: string | null;
  /** Whether today was already a study day before this sitting. */
  alreadyStudied: boolean;
  /** The skills that hold more proofs now than when the sitting began. */
  earned: { row: Row; proofs: number }[];
  /** How many skills can earn a proof tomorrow. */
  tomorrow: number;
};

export function Summary(props: { sitting: Sitting; activity: Activity | null; courseName: string; onDone: () => void; onProgress: () => void }) {
  const { sitting, activity } = props;
  const proofs = sitting.earned.reduce((sum, each) => sum + each.proofs, 0);
  const facts = [
    `${sitting.minutes} min`,
    `${sitting.answers} ${sitting.answers === 1 ? "answer" : "answers"}`,
    sitting.stepsTotal > 0 ? `${sitting.stepsDone} of ${sitting.stepsTotal} steps` : null,
  ].filter(Boolean);
  return (
    <section className="screen summary" data-screen="summary">
      <TopBar />
      <div className="center">
        <div className="summary-col">
          <span className="done-mark" aria-hidden="true">
            <Icon name="check" size={26} />
          </span>
          <h1>Sitting done</h1>
          <p className="summary-sub">
            {props.courseName} · {facts.join(" · ")}
          </p>

          {proofs > 0 && (
            <div className="earned" data-role="earned">
              <span className="earned-head">
                <b>+{proofs}</b> {proofs === 1 ? "proof" : "proofs"} earned
              </span>
              {sitting.earned.slice(0, 4).map(({ row }) => (
                <span className="earned-row" key={row.name}>
                  <Dots dots={row.dots} />
                  <Line text={row.name} />
                </span>
              ))}
              {sitting.earned.length > 4 && <span className="aside">and {sitting.earned.length - 4} more</span>}
            </div>
          )}

          {(sitting.stoppedAt || sitting.tomorrow > 0) && (
            <div className="next-time">
              {sitting.stoppedAt && (
                <p className="selectable">
                  <b>Next time</b>
                  <Line text={sitting.stoppedAt} />
                </p>
              )}
              {sitting.tomorrow > 0 && (
                <p>
                  <b>Tomorrow</b>
                  {sitting.tomorrow} {sitting.tomorrow === 1 ? "skill" : "skills"} can earn a proof
                </p>
              )}
            </div>
          )}

          {activity && (
            <div className="summary-week">
              <Week activity={activity} celebrate={!sitting.alreadyStudied && activity.studied_today} />
              {activity.run > 1 && (
                <span className="run-line">
                  <b>{activity.run}</b> days running
                </span>
              )}
            </div>
          )}

          <div className="row center">
            <button className="btn primary" autoFocus onClick={props.onDone} data-role="summary-done">
              Done <kbd>&#8629;</kbd>
            </button>
            <button className="btn ghost" onClick={props.onProgress}>
              See your proofs
            </button>
          </div>
        </div>
      </div>
    </section>
  );
}
