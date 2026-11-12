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
import type { Activity, Core, Proofs, Row, Today } from "../types";

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/**
 * Today's counts as two short sentences, the parts that are not zero: "6
 * answers marked today, 2 new skills. 3 questions due tomorrow." Null when
 * there is nothing to say.
 */
export function todaySentence(
  today: Today | null,
  options: { proofs?: boolean; tomorrow?: boolean; split?: boolean; lesson?: number } = {},
): string | null {
  if (!today) return null;
  // What was done, then what is due: two short sentences, the same on the course page and on Lesson done.
  const { proofs = true, tomorrow = true, split = true, lesson } = options;
  // On Lesson done the lesson's own marks are listed above, so the day's count is said only when it is a different one, and says what it counts: "Today: 5 answers marked, across lessons."
  const across = lesson !== undefined && today.answered > 0 && today.answered !== lesson;
  const done = [
    today.answered > 0 && lesson === undefined ? `${plural(today.answered, "answer", "answers")} marked today` : null,
    today.started > 0 ? plural(today.started, "new skill", "new skills") : null,
    proofs && today.proofs > 0 ? plural(today.proofs, "proof", "proofs") : null,
  ].filter((part): part is string => part !== null);
  const next = tomorrow && today.due_tomorrow > 0 ? `${plural(today.due_tomorrow, "question", "questions")} due tomorrow` : null;
  // Joined, one sentence; split, what was done and what comes are two short sentences.
  const parts = split ? [done.join(", "), next ?? ""] : [[...done, ...(next ? [next] : [])].join(", ")];
  const said = parts.filter((part) => part !== "").map((part) => `${part.replace(/^./, (c) => c.toUpperCase())}.`);
  if (across) said.unshift(`Today: ${plural(today.answered, "answer", "answers")} marked, across lessons.`);
  return said.length > 0 ? said.join(" ") : null;
}

const NUMBER_WORDS = ["No", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine", "Ten"];

/** "One proof earned", "Three proofs earned". */
function earnedHeadline(n: number): string {
  return `${NUMBER_WORDS[n] ?? n} ${n === 1 ? "proof" : "proofs"} earned`;
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
  /** The skills answered wrong in this lesson, from the marks the lesson recorded, and the day the record brings each back. Empty when the record has none. */
  slipped: { name: string; next_day: string }[];
  /** The answers the lesson showed a mark for, in the order they were marked. Empty when the record has none. */
  marks: Reviewed[];
};

/** What the lesson says of a mark, in its own words. */
const VERDICT_SAID: Record<Reviewed["verdict"], string> = { right: "Marked right", "right with help": "Right, with help", "not yet": "Not right yet" };

/** One marked answer of the lesson, said by the skill it was on: "right", "right with help" or "not yet". */
export type Reviewed = { name: string; verdict: "right" | "right with help" | "not yet" };

/**
 * The marks the lesson recorded, as the lesson showed them (a lesson answer
 * marked right or wrong, and nothing else), each named by the skill the record
 * calls it: the same name Proofs gives it. The record's times are local, "2026-11-12
 * 09:41", to the minute.
 */
export async function reviewIn(api: Core, rows: Row[], startedMs: number | null): Promise<Reviewed[]> {
  if (startedMs === null) return [];
  let marks: Awaited<ReturnType<Core["marks"]>> = null;
  try {
    marks = await api.marks();
  } catch {
    return [];
  }
  if (!marks) return [];
  const pad = (n: number) => String(n).padStart(2, "0");
  const began = new Date(startedMs);
  const since = `${began.getFullYear()}-${pad(began.getMonth() + 1)}-${pad(began.getDate())} ${pad(began.getHours())}:${pad(began.getMinutes())}`;
  const key = (text: string) => text.trim().toLowerCase();
  const out: Reviewed[] = [];
  for (const mark of marks) {
    if (mark.paper !== null || mark.at < since || mark.label.trim() === "") continue;
    const verdict = mark.mark === "wrong" ? "not yet" : mark.mark === "right" ? (mark.help === "none" ? "right" : "right with help") : null;
    if (verdict === null) continue;
    out.push({ name: rows.find((each) => key(each.name) === key(mark.label))?.name ?? mark.label.trim(), verdict });
  }
  return out;
}

/** "tomorrow", "in 3 days", "today", or "on Mon 16 Nov" when the day now is not known. */
function backWhen(day: string, today: string | null): string {
  if (today) {
    const gap = Math.round((new Date(`${day}T12:00:00`).getTime() - new Date(`${today}T12:00:00`).getTime()) / 86_400_000);
    if (gap <= 0) return "today";
    if (gap === 1) return "tomorrow";
    return `in ${gap} days`;
  }
  return `on ${new Date(`${day}T12:00:00`).toLocaleDateString("en-GB", { weekday: "short", day: "numeric", month: "short" }).replace(",", "")}`;
}

/** What slipped in this lesson, grouped by the day it comes back: [["tomorrow", ["slope and $K_m$"]], ...], soonest first. */
function slippedBy(slipped: Sitting["slipped"], today: string | null): [string, string[]][] {
  const days = [...new Set(slipped.map((each) => each.next_day))].sort();
  return days.map((day) => [backWhen(day, today), slipped.filter((each) => each.next_day === day).map((each) => each.name)]);
}

/** "a", "a and b", "a, b and c". */
function listed(names: string[]): string {
  return names.length < 2 ? (names[0] ?? "") : `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

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
  const level = props.proofs !== null && props.proofs.levelled && props.proofs.total > 0;
  // With nothing marked, what comes next leads, and in the words of the other screens: questions due, else skills to prove. Otherwise today's counts say it.
  const due = sitting.today?.due_tomorrow ?? 0;
  const comes = unmarked
    ? due > 0
      ? `${plural(due, "question", "questions")} ${due === 1 ? "is" : "are"} due tomorrow.`
      : sitting.tomorrow > 0
        ? `${plural(sitting.tomorrow, "skill", "skills")} to prove tomorrow.`
        : null
    : null;
  // The lesson's own count of marks: the record's list when it has one, else the count the window worked out.
  const lessonMarks = sitting.marks.length > 0 ? sitting.marks.length : (sitting.marked ?? 0);
  const today = todaySentence(sitting.today, { proofs: proofs === 0, tomorrow: comes === null, lesson: lessonMarks });
  // Answers were marked, and not every one is a proof: say why, once.
  const fewer = !unmarked && Math.max(sitting.marked ?? 0, sitting.today?.answered ?? 0) > proofs;
  const marked = !sitting.today && sitting.marked !== null && sitting.marked > 0 ? `${plural(sitting.marked, "answer", "answers")} marked.` : null;
  // The count of steps done and the line to pick up from must agree: when the saved line names a step other than the one after the last done, only the line is said.
  const named = sitting.stoppedAt ? /\bstep (\d+)\b/i.exec(sitting.stoppedAt) : null;
  const agrees = !named || Number(named[1]) === sitting.stepsDone + 1;
  const steps = sitting.stepsTotal > 0 && agrees ? `${sitting.stepsDone} of ${sitting.stepsTotal} steps done.` : null;
  // What moved in a lesson with nothing marked, when the data has it: "You reached step 3 of 5: what the y-intercept gives."
  const reached = (() => {
    if (!unmarked || sitting.stepsTotal <= 0) return null;
    if (named && sitting.stoppedAt && Number(named[1]) <= sitting.stepsTotal) {
      const rest = sitting.stoppedAt.slice(named.index + named[0].length).replace(/^\s*[:,.\u2013-]\s*/, "").trim();
      return { text: `You reached step ${named[1]} of ${sitting.stepsTotal}${rest ? `: ${sentence(rest)}` : "."}`, saysStop: true };
    }
    return agrees && sitting.stepsDone > 0 ? { text: `You finished ${sitting.stepsDone} of ${sitting.stepsTotal} steps.`, saysStop: false } : null;
  })();
  const tomorrow = sitting.today ? 0 : sitting.tomorrow;
  const slips = slippedBy(sitting.slipped, sitting.today?.day ?? activity?.today ?? null);
  const showSteps = steps !== null && reached === null;
  const showStop = sitting.stoppedAt !== null && !reached?.saysStop;
  const nothing = !today && !marked && !showSteps && !showStop && tomorrow === 0 && slips.length === 0;
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
            <div className="summary-why" data-role="why">
              {reached && (
                <p className="summary-lead">
                  <Line text={reached.text} />
                </p>
              )}
              <p className="summary-because">Nothing was marked in this lesson, so no proof yet.</p>
              {comes && <p className="summary-because due-next">{comes}</p>}
            </div>
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
            <div className="level-line" data-role="level">
              <Level proofs={props.proofs} size="lg" gain={proofs} still={!first} title={`${props.proofs.earned} of ${props.proofs.total} proofs`} />
            </div>
          )}

          {sitting.marks.length > 0 && (
            <section className="sec marked" data-role="marked" aria-labelledby="marked-today">
              <h2 className="sec-name" id="marked-today">
                Marked in this lesson
              </h2>
              <ul className="marked-list">
                {sitting.marks.slice(0, 6).map((mark, index) => (
                  <li key={index}>
                    <Line text={mark.name} className="marked-skill" />
                    <span className="marked-verdict">{VERDICT_SAID[mark.verdict]}</span>
                  </li>
                ))}
                {sitting.marks.length > 6 && <li className="marked-more">and {sitting.marks.length - 6} more</li>}
              </ul>
            </section>
          )}

          {!nothing && (
            <div className="summary-facts">
              {today && (
                <p className="today-line" data-role="today">
                  {today}
                </p>
              )}
              {slips.map(([when, names]) => (
                <p className="selectable slipped" data-role="slipped" key={when}>
                  Back {when}:{" "}
                  <Line text={listed(names)} />.
                </p>
              ))}
              {marked && <p>{marked}</p>}
              {showSteps && <p>{steps}</p>}
              {tomorrow > 0 && <p>{plural(tomorrow, "skill", "skills")} to prove tomorrow.</p>}
              {showStop && sitting.stoppedAt && (
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
