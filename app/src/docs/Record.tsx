// The record: every answer the runtime marked, by day, newest first, and for
// each skill what the record holds of it, in words. It is a log of the
// learner's own work, read from the runtime, never a score. This is where the
// proofs, the papers' scores and when a skill comes back live, for a learner
// who looks for them.

import { Fragment } from "react";

import { dayName } from "../page";
import type { Mark, Progress, Row } from "../types";
import { Line } from "../ui/Rich";
import { markWords } from "./Lesson";

const ms = (at: string) => new Date(at.replace(" ", "T")).getTime();

/** What the record holds of a skill, as a sentence. */
function skillWords(row: Row, marks: Mark[]): string {
  // A day counts only when the record has a right answer on it, with no help, from a blank page.
  const backed = new Set(marks.filter((m) => m.label.trim().toLowerCase() === row.name.trim().toLowerCase() && m.mark === "right" && m.help === "none" && m.cold).map((m) => m.at.slice(0, 10))).size;
  const counted = Math.min(row.dots.filter((dot) => dot === 2).length, marks.length > 0 ? Math.max(backed, 0) : row.dots.filter((dot) => dot === 2).length);
  const days = counted === 0 ? "Not yet answered right alone" : `Answered right alone, from a blank page, on ${counted} ${counted === 1 ? "day" : "days"}`;
  const note = row.note && row.note !== "–" ? row.note.replace(/^due /, "asked again ").replace(/^next /, "asked again ") : "";
  return note ? `${days}; ${note}.` : `${days}.`;
}

export function RecordDoc({ marks, progress }: { marks: Mark[] | null; progress: Progress | null }) {
  const rows = [...(marks ?? [])].reverse();
  const byDay = new Map<string, Mark[]>();
  for (const mark of rows) {
    const day = mark.at.slice(0, 10);
    byDay.set(day, [...(byDay.get(day) ?? []), mark]);
  }
  const skills = progress ? [...progress.in_progress, ...progress.confirmed] : [];
  return (
    <article className="record" data-doc="record">
      <h1 className="doc-title">Record</h1>
      <p className="doc-path">Every answer the record marked, newest first.</p>
      {marks === null ? (
        <p className="quiet-para">This version of Magnesium keeps no list of marked answers.</p>
      ) : rows.length === 0 ? (
        <p className="quiet-para">No answer has been marked yet. Answers you give inside a task are marked; talk is not.</p>
      ) : (
        [...byDay.entries()].map(([day, list]) => (
          <section key={day} className="record-day">
            <h2>{dayName(ms(`${day} 12:00`))}</h2>
            <table className="record-table">
              <tbody>
                {list.map((mark) => (
                  <tr key={mark.id} data-mark={mark.mark}>
                    <td className="time">{mark.at.slice(11, 16)}</td>
                    <td>
                      <Line text={mark.label || "an answer"} />
                      {mark.paper && <span className="quiet"> · {mark.paper}</span>}
                    </td>
                    <td className="said">{markWords(mark)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </section>
        ))
      )}
      {skills.length > 0 && (
        <section className="record-skills">
          <h2>Skills</h2>
          <dl>
            {skills.map((row) => (
              <Fragment key={row.name}>
                <dt>
                  <Line text={row.name} />
                </dt>
                <dd>{skillWords(row, marks ?? [])}</dd>
              </Fragment>
            ))}
          </dl>
        </section>
      )}
      {progress && progress.scores.length > 0 && (
        <section className="record-skills">
          <h2>Papers</h2>
          <dl>
            {progress.scores.map((score) => (
              <Fragment key={`${score.paper}-${score.date}`}>
                <dt>{score.paper}</dt>
                <dd>
                  {score.score} on {dayName(ms(`${score.date} 12:00`))}
                </dd>
              </Fragment>
            ))}
          </dl>
        </section>
      )}
    </article>
  );
}
