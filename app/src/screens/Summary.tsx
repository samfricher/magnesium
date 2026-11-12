// The end of a sitting. One screen that closes it, so the learner stops with
// a clear end and a reason to come back.
//
// It says how many answers the tutor marked in this sitting, and says so
// plainly when none was. It leads with the proofs earned in this sitting,
// when there are any. A proof is an answer given alone on a later day, so
// most sittings earn one only through the checks and redos they open with.
// The window works the marks and the proofs out by comparing the runtime's
// record before and after the sitting. It never takes the tutor's word for
// one. Under them: today's counts, where the learner stopped, and today's
// place in the week.

import { Line } from "../ui/Rich";
import { Icon } from "../ui/icons";
import { Dots, TopBar, Week } from "../ui/bits";
import type { Activity, Row, Today } from "../types";

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/**
 * Today's counts as plain words, the ones that are not zero: "6 answers
 * marked today", "2 new skills", "1 proof", "3 due tomorrow".
 */
export function todayParts(today: Today | null): string[] {
  if (!today) return [];
  return [
    today.answered > 0 ? `${plural(today.answered, "answer", "answers")} marked today` : null,
    today.started > 0 ? plural(today.started, "new skill", "new skills") : null,
    today.proofs > 0 ? plural(today.proofs, "proof", "proofs") : null,
    today.due_tomorrow > 0 ? `${today.due_tomorrow} due tomorrow` : null,
  ].filter((part): part is string => part !== null);
}

export type Sitting = {
  minutes: number;
  /** Messages the learner sent, the opening line left out. */
  sent: number;
  /** Answers the tutor marked in this sitting, from the record. null when it cannot be told. */
  marked: number | null;
  /** Today's counts after the sitting, or null when the runtime cannot say. */
  today: Today | null;
  stepsDone: number;
  stepsTotal: number;
  /** The line the learner saved, which opens the next sitting. */
  stoppedAt: string | null;
  /** Whether today was already a study day before this sitting. */
  alreadyStudied: boolean;
  /** The skills that hold more proofs now than when the sitting began. */
  earned: { row: Row; proofs: number }[];
  /** How many skills can earn a proof tomorrow. Shown only without today's counts, which say what is due tomorrow. */
  tomorrow: number;
};

export function Summary(props: { sitting: Sitting; activity: Activity | null; courseName: string; onDone: () => void; onProgress: () => void }) {
  const { sitting, activity } = props;
  const proofs = sitting.earned.reduce((sum, each) => sum + each.proofs, 0);
  const facts = [
    `${sitting.minutes} min`,
    sitting.marked !== null && sitting.marked > 0 ? `${plural(sitting.marked, "answer", "answers")} marked` : null,
    sitting.stepsTotal > 0 ? `${sitting.stepsDone} of ${sitting.stepsTotal} steps` : null,
  ].filter(Boolean);
  const unmarked = sitting.sent > 0 && sitting.marked === 0;
  const today = todayParts(sitting.today);
  const tomorrow = sitting.today ? 0 : sitting.tomorrow;
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
          {unmarked && (
            <p className="marks-note" data-role="unmarked">
              The tutor marked none of your answers in this sitting, so it earned no proof.
            </p>
          )}

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

          {today.length > 0 && (
            <p className="today-line" data-role="today">
              {today.join(" · ")}
            </p>
          )}

          {(sitting.stoppedAt || tomorrow > 0) && (
            <div className="next-time">
              {sitting.stoppedAt && (
                <p className="selectable">
                  <b>Next time</b>
                  <Line text={sitting.stoppedAt} />
                </p>
              )}
              {tomorrow > 0 && (
                <p>
                  <b>Tomorrow</b>
                  {tomorrow} {tomorrow === 1 ? "skill" : "skills"} can earn a proof
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
            <button className="btn primary" autoFocus title="Done (Return)" onClick={props.onDone} data-role="summary-done">
              Done
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
