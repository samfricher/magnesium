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

import { useEffect, useState } from "react";

import { Line } from "../ui/Rich";
import { Icon } from "../ui/icons";
import { Level, PaletteKey, ProofDots, Run, ThisWeek, TopBar, proofsOf, sentence, useReturnForPrimary } from "../ui/bits";
import type { Activity, Proofs, Row, Today } from "../types";

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/**
 * Today's counts as one sentence, the parts that are not zero: "6 answers
 * marked today, 2 new skills, 3 due tomorrow." Null when there is nothing to say.
 */
export function todaySentence(today: Today | null, options: { proofs?: boolean; tomorrow?: boolean; split?: boolean } = {}): string | null {
  if (!today) return null;
  const { proofs = true, tomorrow = true, split = false } = options;
  const done = [
    today.answered > 0 ? `${plural(today.answered, "answer", "answers")} marked today` : null,
    today.started > 0 ? plural(today.started, "new skill", "new skills") : null,
    proofs && today.proofs > 0 ? plural(today.proofs, "proof", "proofs") : null,
  ].filter((part): part is string => part !== null);
  const next = tomorrow && today.due_tomorrow > 0 ? `${plural(today.due_tomorrow, "question", "questions")} due tomorrow` : null;
  // Joined, one sentence; split, what was done and what comes are two short sentences.
  const parts = split ? [done.join(", "), next ?? ""] : [[...done, ...(next ? [next] : [])].join(", ")];
  const said = parts.filter((part) => part !== "").map((part) => `${part.replace(/^./, (c) => c.toUpperCase())}.`);
  return said.length > 0 ? said.join(" ") : null;
}

const NUMBER_WORDS = ["No", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine", "Ten"];

/** "One proof earned", "Three proofs earned". */
function earnedHeadline(n: number): string {
  return `${NUMBER_WORDS[n] ?? n} ${n === 1 ? "proof" : "proofs"} earned`;
}

/**
 * What the lesson did to the level, and what is left: "Level 5. 1 proof
 * today. 3 more to reach level 6." The change comes before the remainder, so
 * the sentence says both. Null when there is no level to show.
 */
function levelSentence(proofs: Proofs | null, gained: number): string | null {
  if (!proofs || !proofs.levelled || proofs.total === 0) return null;
  const change = gained > 0 ? ` ${plural(gained, "proof", "proofs")} today.` : "";
  const next =
    proofs.to_next > 0
      ? gained > 0
        ? ` ${proofs.to_next} more to reach level ${proofs.level + 1}.`
        : ` ${plural(proofs.to_next, "more proof", "more proofs")} to reach level ${proofs.level + 1}.`
      : "";
  return `Level ${proofs.level}.${change}${next}`;
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

/** The sittings whose end has been seen once. The proof flare and the tally's growth play the first time only; a return to the screen shows them at rest. */
const seen = new WeakSet<object>();

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
  const [first] = useState(() => !seen.has(sitting));
  useEffect(() => {
    seen.add(sitting);
  }, [sitting]);
  // The proofs earned in this lesson.
  const proofs = sitting.earned.reduce((sum, each) => sum + each.proofs, 0);
  const unmarked = sitting.sent > 0 && sitting.marked === 0;
  const level = levelSentence(props.proofs, proofs);
  // With nothing marked, say when the next chance comes. Otherwise today's counts say it.
  const comes = unmarked
    ? sitting.tomorrow > 0
      ? `${plural(sitting.tomorrow, "skill comes", "skills come")} back tomorrow.`
      : sitting.today && sitting.today.due_tomorrow > 0
        ? `${plural(sitting.today.due_tomorrow, "question", "questions")} due tomorrow.`
        : null
    : null;
  const today = todaySentence(sitting.today, { proofs: proofs === 0, tomorrow: comes === null });
  // Answers were marked, and not every one is a proof: say why, once.
  const fewer = !unmarked && Math.max(sitting.marked ?? 0, sitting.today?.answered ?? 0) > proofs;
  const marked = !sitting.today && sitting.marked !== null && sitting.marked > 0 ? `${plural(sitting.marked, "answer", "answers")} marked.` : null;
  // The count of steps done and the line to pick up from must agree: when the saved line names a step other than the one after the last done, only the line is said.
  const named = sitting.stoppedAt ? /\bstep (\d+)\b/i.exec(sitting.stoppedAt) : null;
  const agrees = !named || Number(named[1]) === sitting.stepsDone + 1;
  const steps = sitting.stepsTotal > 0 && agrees ? `${sitting.stepsDone} of ${sitting.stepsTotal} steps done.` : null;
  const tomorrow = sitting.today ? 0 : sitting.tomorrow;
  const nothing = !today && !marked && !steps && !sitting.stoppedAt && tomorrow === 0;
  return (
    <section className="screen summary" data-screen="summary">
      <TopBar
        left={
          <button className="crumb" onClick={props.onDone} title={`${props.courseName} (Esc)`}>
            <Icon name="back" size={16} />
            {props.courseName}
          </button>
        }
        right={<PaletteKey onClick={props.onPalette} />}
      />
      <div className="center top">
        <div className="summary-col">
          <h1 className="h-title" data-role={unmarked ? "unmarked" : undefined}>
            {proofs > 0 ? earnedHeadline(proofs) : unmarked ? "Lesson saved" : "Lesson done"}
          </h1>
          {unmarked && (
            <p className="summary-why" data-role="why">
              The tutor didn&apos;t mark an answer in this lesson, so there is no proof yet. A proof is an answer the tutor marks right, given without help, on its own day.
              {comes ? ` ${comes}` : ""}
            </p>
          )}

          {proofs > 0 && (
            <div className="earned" data-role="earned">
              {sitting.earned.slice(0, 4).map(({ row, proofs: gained }) => (
                <span className="earned-row" key={row.name}>
                  <ProofDots earned={proofsOf(row.dots)} today={gained} flare={first} size={20} />
                  <Line text={row.name} className="earned-name" />
                </span>
              ))}
              {sitting.earned.length > 4 && <span className="aside">and {sitting.earned.length - 4} more</span>}
            </div>
          )}

          {fewer && <p className="summary-why quiet">A proof needs a right answer without help, on a new day for that skill.</p>}

          {level && props.proofs && (
            <p className="level-line" data-role="level">
              <Level
                proofs={props.proofs}
                size="lg"
                text="none"
                gain={proofs}
                still={!first}
                title={`${props.proofs.earned} of ${props.proofs.total} proofs`}
              />
              <span className="num">{level}</span>
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
            <button className="btn" onClick={props.onProgress}>
              See your proofs
            </button>
          </div>
        </div>
      </div>
    </section>
  );
}
