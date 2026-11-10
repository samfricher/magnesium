// Progress counts one thing: an answer reproduced alone on a later day.
// Rows, map and levels draw the same proof three ways. Never counted:
// minutes, streaks, sessions, or answers the tutor wrote.

import { useState } from "react";

import type { Dots, Progress as ProgressData, Row } from "./types";

function DotsView({ dots }: { dots: Dots }) {
  return (
    <span className="dots" aria-label={`${dots.filter((d) => d === 2).length} of 3`}>
      {dots.map((dot, index) => (
        <i key={index} className={dot === 2 ? "f" : dot === 1 ? "h" : undefined} />
      ))}
    </span>
  );
}

function RowView({ row }: { row: Row }) {
  return (
    <div className="prow selectable">
      <DotsView dots={row.dots} />
      <span>{row.name}</span>
      <span className="pd">{row.what}</span>
      <span className="pn">{row.note}</span>
    </div>
  );
}

function Rows({ title, rows, fold }: { title: string; rows: Row[]; fold: number }) {
  const [all, setAll] = useState(false);
  if (rows.length === 0) return null;
  const shown = all ? rows : rows.slice(0, fold);
  return (
    <>
      <div className="sect">{title}</div>
      {shown.map((row, index) => (
        <RowView key={`${row.name}-${index}`} row={row} />
      ))}
      {rows.length > shown.length && (
        <button className="prow more" onClick={() => setAll(true)}>
          + {rows.length - shown.length} more {rows.length - shown.length === 1 ? "row" : "rows"}
        </button>
      )}
    </>
  );
}

/**
 * One chip per topic. A topic is as proved as its weakest row. The style says
 * how far: "s3" confirmed, "s2" and "s1" two and one passes, "sf" a gap found
 * and not yet passed, "s0" a topic nobody has tested.
 */
function chipsOf(data: ProgressData): { name: string; style: string }[] {
  const byName = new Map<string, number>();
  const add = (row: Row, passes: number) => {
    const had = byName.get(row.name);
    byName.set(row.name, had === undefined ? passes : Math.min(had, passes));
  };
  data.confirmed.forEach((row) => add(row, 3));
  data.in_progress.forEach((row) => add(row, row.dots.filter((d) => d === 2).length));
  const chips = Array.from(byName, ([name, passes]) => ({ name, style: passes === 0 ? "sf" : `s${passes}` }));
  data.untested.forEach((row) => {
    if (!byName.has(row.name)) chips.push({ name: row.name, style: "s0" });
  });
  return chips;
}

export function Progress(props: {
  data: ProgressData | null;
  error: string | null;
  folderName: string;
  onView: (view: string) => void;
  onPalette: () => void;
}) {
  const { data } = props;
  if (!data) {
    return (
      <section className="scr">
        <div className="top" data-tauri-drag-region>
          <span>{props.folderName}</span>
        </div>
        {props.error && <div className="note warn selectable">{props.error}</div>}
      </section>
    );
  }
  const view = ["A", "B", "C"].includes(data.view) ? data.view : "A";
  const exam = data.exam;
  const confirmed = data.confirmed.length;
  const total = confirmed + data.in_progress.length + data.untested.length;
  const level = total === 0 ? 0 : Math.min(5, Math.floor((confirmed * 5) / total));
  const toNext = level >= 5 ? 0 : Math.ceil(((level + 1) * total) / 5) - confirmed;
  const within = total === 0 || level >= 5 ? 0 : (confirmed * 5) / total - level;
  const sure = data.scores.filter((s) => s.sure_total !== null && s.sure_right !== null);

  return (
    <section className="scr">
      <div className="top" data-tauri-drag-region>
        <span>
          {props.folderName}
          {exam && ` · ${exam.title}`}
          {exam?.days_left !== null && exam?.days_left !== undefined && exam.days_left >= 0 && ` · ${exam.days_left} days`}
        </span>
        <span className="grow" data-tauri-drag-region />
        <span className="sug">
          {[
            ["A", "Rows"],
            ["B", "Map"],
            ["C", "Levels"],
          ].map(([key, name]) => (
            <button key={key} className={view === key ? "on" : undefined} onClick={() => props.onView(key)}>
              {name}
            </button>
          ))}
        </span>
        <button className="kbd" title="Everything else lives here" onClick={props.onPalette}>
          &#8984;K
        </button>
      </div>
      <div className="progress">
        <div className="inner">
          {data.message ? (
            <div className="sum">{data.message}</div>
          ) : (
            <>
              <div className="sum">
                <b>{confirmed}</b> confirmed · <b>{data.in_progress.length}</b> in progress · <b>{data.untested.length}</b> not
                yet tested
              </div>
              {view === "A" && (
                <>
                  <Rows title="In progress" rows={data.in_progress} fold={12} />
                  <Rows title="Confirmed" rows={data.confirmed} fold={6} />
                  <Rows title="Not yet tested" rows={data.untested} fold={4} />
                </>
              )}
              {view === "B" && (
                <>
                  <div className="chips">
                    {chipsOf(data).map((chip) => (
                      <span key={chip.name} className={`chip ${chip.style}`}>
                        {chip.name}
                      </span>
                    ))}
                  </div>
                  <div className="legend">
                    dashed = not yet tested · outline = found, no pass yet · part filled = 1 or 2 of 3 · solid = confirmed
                  </div>
                  <div className="legend" style={{ marginTop: 6 }}>
                    The same proof as the rows, drawn over the course's topics.
                  </div>
                </>
              )}
              {view === "C" && (
                <div className="lvlwrap">
                  <div className="lvl">Level {level} of 5</div>
                  <div className="lbar">
                    {[0, 1, 2, 3, 4].map((index) => (
                      <i key={index}>
                        <b style={{ width: index < level ? "100%" : index === level ? `${Math.round(within * 100)}%` : "0%" }} />
                      </i>
                    ))}
                  </div>
                  <div>
                    {confirmed} {confirmed === 1 ? "row" : "rows"} confirmed.
                    {level < 5 && ` ${toNext} more ${toNext === 1 ? "reaches" : "reach"} level ${level + 1}.`}
                  </div>
                  <div className="legend" style={{ marginTop: 8 }}>
                    A level moves only when a row is confirmed. Nothing is earned for minutes or for showing up.
                  </div>
                </div>
              )}
              <div className="foot selectable">
                {sure.length > 0 && (
                  <div>
                    Sure but wrong:{" "}
                    {sure.map((score, index) => (
                      <span key={`${score.paper}${score.date}`}>
                        {index > 0 && ", then "}
                        <b>
                          {(score.sure_total ?? 0) - (score.sure_right ?? 0)} of {score.sure_total}
                        </b>{" "}
                        on {score.paper}
                      </span>
                    ))}
                  </div>
                )}
                {data.scores.length > 0 && <div>Papers: {data.scores.map((s) => `${s.paper} ${s.score}`).join(" · ")}</div>}
                {data.helped_count > 0 && (
                  <div className="dimt">
                    With help, not counted: {data.helped_count} {data.helped_count === 1 ? "answer" : "answers"}
                    {data.helped.length > 0 && ` (${data.helped.join(", ")})`}. They return as questions.
                  </div>
                )}
                {data.checks_unknown && (
                  <div className="dimt">
                    Quick checks from chat are not shown. This version of Magnesium keeps them where the window cannot
                    read them yet. The tutor still asks them as each sitting opens.
                  </div>
                )}
                {data.unreadable > 0 && (
                  <div className="dimt">
                    {data.unreadable} {data.unreadable === 1 ? "row" : "rows"} of the gap log could not be read and{" "}
                    {data.unreadable === 1 ? "is" : "are"} left out. Ask the tutor to fix your plan files.
                  </div>
                )}
              </div>
            </>
          )}
        </div>
      </div>
    </section>
  );
}
