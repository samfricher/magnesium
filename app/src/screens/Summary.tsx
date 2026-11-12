// The end of a lesson. One screen that closes it, so the learner stops with
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
import { Level, PaletteKey, ProofDots, Run, ThisWeek, TopBar, proofsOf, sentence, useReturnForPrimary } from "../ui/bits";
import type { Activity, Proofs, Row, Today } from "../types";

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/**
 * Today's counts as one sentence, the parts that are not zero: "6 answers
 * marked today, 2 new skills, 3 due tomorrow." Null when there is nothing to say.
 */
export function todaySentence(today: Today | null, options: { proofs?: boolean; tomorrow?: boolean } = {}): string | null {
  if (!today) return null;
  const { proofs = true, tomorrow = true } = options;
  const parts = [
    today.answered > 0 ? `${plural(today.answered, "answer", "answers")} marked today` : null,
    today.started > 0 ? plural(today.started, "new skill", "new skills") : null,
    proofs && today.proofs > 0 ? plural(today.proofs, "proof", "proofs") : null,
    tomorrow && today.due_tomorrow > 0 ? `${plural(today.due_tomorrow, "question", "questions")} due tomorrow` : null,
  ].filter((part): part is string => part !== null);
  return parts.length > 0 ? `${parts.join(", ")}.` : null;
}

const NUMBER_WORDS = ["No", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine", "Ten"];

/** "One proof earned", "Three proofs earned". */
function earnedHeadline(n: number): string {
  return `${NUMBER_WORDS[n] ?? n} ${n === 1 ? "proof" : "proofs"} earned`;
}

/** Where the course stands: "Level 5. 3 more proofs to reach level 6." Null when there is no level to show. */
function levelSentence(proofs: Proofs | null): string | null {
  if (!proofs || !proofs.levelled || proofs.total === 0) return null;
  const next = proofs.to_next > 0 ? ` ${proofs.to_next} more ${proofs.to_next === 1 ? "proof" : "proofs"} to reach level ${proofs.level + 1}.` : "";
  return `Level ${proofs.level}.${next}`;
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

export function Summary(props: {
  sitting: Sitting;
  activity: Activity | null;
  /** The course's proofs after the sitting, as the Proofs screen reads them. */
  proofs: Proofs | null;
  courseName: string;
  onDone: () => void;
  onProgress: () => void;
  onPalette: () => void;
}) {
  const { sitting, activity } = props;
  useReturnForPrimary();
  // The proofs earned in this lesson.
  const proofs = sitting.earned.reduce((sum, each) => sum + each.proofs, 0);
  // The mark is a check only when something moved: a proof, or an answer marked.
  const moved = proofs > 0 || (sitting.marked ?? 0) > 0;
  const unmarked = sitting.sent > 0 && sitting.marked === 0;
  const level = levelSentence(props.proofs);
  // With nothing marked, say when the next chance comes. Otherwise today's counts say it.
  const comes = unmarked
    ? sitting.tomorrow > 0
      ? `${plural(sitting.tomorrow, "skill comes", "skills come")} back tomorrow.`
      : sitting.today && sitting.today.due_tomorrow > 0
        ? `${plural(sitting.today.due_tomorrow, "question", "questions")} due tomorrow.`
        : null
    : null;
  const today = todaySentence(sitting.today, { proofs: proofs === 0, tomorrow: comes === null });
  const marked = !sitting.today && sitting.marked !== null && sitting.marked > 0 ? `${plural(sitting.marked, "answer", "answers")} marked.` : null;
  const steps = sitting.stepsTotal > 0 ? `${sitting.stepsDone} of ${sitting.stepsTotal} steps done.` : null;
  const tomorrow = sitting.today ? 0 : sitting.tomorrow;
  const nothing = !today && !marked && !steps && !sitting.stoppedAt && tomorrow === 0;
  return (
    <section className="screen summary" data-screen="summary">
      <TopBar center={<span className="bar-title">{props.courseName}</span>} right={<PaletteKey onClick={props.onPalette} />} />
      <div className="center">
        <div className="summary-col">
          <span className={moved ? "done-mark" : "done-mark quiet"} aria-hidden="true">
            <Icon name={moved ? "check" : "dash"} size={22} />
          </span>
          <h1 className="h-title" data-role={unmarked ? "unmarked" : undefined}>
            {proofs > 0 ? earnedHeadline(proofs) : unmarked ? "No proof this time" : "Lesson done"}
          </h1>
          {unmarked && (
            <p className="summary-why" data-role="why">
              The tutor didn&apos;t mark an answer in this lesson, so no proof yet. A proof is an answer the tutor marks right, given without help, on its own day.
              {comes ? ` ${comes}` : ""}
            </p>
          )}

          {proofs > 0 && (
            <div className="earned" data-role="earned">
              {sitting.earned.slice(0, 4).map(({ row, proofs: gained }) => (
                <span className="earned-row" key={row.name}>
                  <ProofDots earned={proofsOf(row.dots)} today={gained} flare size={20} />
                  <Line text={row.name} className="earned-name" />
                </span>
              ))}
              {sitting.earned.length > 4 && <span className="aside">and {sitting.earned.length - 4} more</span>}
            </div>
          )}

          {level && props.proofs && (
            <p className="level-line" data-role="level">
              <Level
                proofs={props.proofs}
                size="lg"
                text="none"
                gain={proofs}
                title={`${props.proofs.earned} of ${props.proofs.total} proofs`}
              />
              <span>{level}</span>
            </p>
          )}

          {!nothing && (
            <div className="summary-facts">
              {today && (
                <p className="today-line" data-role="today">
                  {today}
                </p>
              )}
              {marked && <p>{marked}</p>}
              {steps && <p>{steps}</p>}
              {tomorrow > 0 && <p>{plural(tomorrow, "skill", "skills")} can earn a proof tomorrow.</p>}
              {sitting.stoppedAt && (
                <p className="selectable next-time">
                  Next time: <Line text={sentence(sitting.stoppedAt)} />
                </p>
              )}
            </div>
          )}

          {activity && (
            <div className="summary-week">
              <ThisWeek activity={activity} />
              <Run days={activity.run > 1 ? activity.run : 0} />
            </div>
          )}

          <div className="row center">
            <button className="btn primary" title="Done (Return)" onClick={props.onDone} data-role="summary-done">
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
