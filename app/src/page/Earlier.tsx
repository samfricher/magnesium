// The earlier pages: each day the learner worked, under its date, newest
// first. A page says what was asked, what they wrote and what the record
// marked; the tutor's whole words are one keystroke away. Days with no work
// are not there at all.

import { memo, useState } from "react";

import { clock, longDay } from "../days";
import { openingLine, verdict, wordsOf, type Page, type Sitting } from "../log";
import { isHelp } from "../lesson";
import { Line, Rich } from "../ui/Rich";

function MarkGut({ marks }: { marks: Page["sittings"][number]["entries"][number]["marks"] }) {
  if (marks.length === 0) return null;
  const said = verdict(marks[marks.length - 1]);
  return (
    <span className={said.right ? "mark right" : "mark"} title={said.title}>
      <span className="mark-word">{said.word}</span>
      {said.how && <span className="mark-how">{said.how}</span>}
    </span>
  );
}

function SittingRows({ sitting, whole }: { sitting: Sitting; whole: boolean }) {
  const span = sitting.end - sitting.start > 60_000 ? `${clock(sitting.start)} to ${clock(sitting.end)}` : clock(sitting.start);
  return (
    <>
      <div className="row sitting">
        <div className="gut">{clock(sitting.start)}</div>
        <div className="txt">
          <span className="course">{sitting.course}</span>
          {span !== clock(sitting.start) && <span className="span"> until {clock(sitting.end)}</span>}
        </div>
      </div>
      {sitting.papers.length > 0 && (
        <div className="row note">
          <div className="gut" />
          <div className="txt">{sitting.papers[0].paper}, handed in and marked</div>
        </div>
      )}
      {sitting.entries.map((entry) => {
        const words = wordsOf(entry.learner);
        const asked = entry.marks.find((m) => m.question)?.question ?? null;
        const answered = entry.marks.length > 0;
        if (words.opening) {
          return (
            <div className="row said opening" key={entry.ms}>
              <div className="gut" />
              <div className="txt">
                {openingLine(words.text, words.opening.stoppedAt)}
                {whole && entry.tutor && (
                  <div className="reply">
                    <Rich text={entry.tutor} final />
                  </div>
                )}
              </div>
            </div>
          );
        }
        return (
          <div className={answered ? "row said answered" : isHelp(entry.learner) ? "row said help" : "row said"} key={entry.ms}>
            <div className="gut">
              <MarkGut marks={entry.marks} />
            </div>
            <div className="txt">
              {asked && <div className="asked"><Line text={asked} /></div>}
              {words.text.trim() !== "" && <div className="words"><Line text={words.text} /></div>}
              {words.photos > 0 && <div className="aside">{words.photos === 1 ? "a photo of working" : `${words.photos} photos of working`}</div>}
              {whole && entry.tutor && (
                <div className="reply">
                  <Rich text={entry.tutor} final />
                </div>
              )}
            </div>
          </div>
        );
      })}
    </>
  );
}

export const EarlierPage = memo(function EarlierPage({ page }: { page: Page }) {
  const [whole, setWhole] = useState(false);
  return (
    <section className="day earlier" aria-label={longDay(page.day)} data-day={page.day}>
      <div className="row dateline">
        <div className="gut" />
        <h2 className="txt">
          <button className="date-btn" aria-expanded={whole} onClick={() => setWhole((open) => !open)} title={whole ? "Show the short page" : "Show everything said that day"}>
            {longDay(page.day)}
          </button>
        </h2>
      </div>
      {page.sittings.map((sitting) => (
        <SittingRows key={`${sitting.folder}-${sitting.start}`} sitting={sitting} whole={whole} />
      ))}
    </section>
  );
});
