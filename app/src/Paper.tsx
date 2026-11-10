// A timed paper. The screen holds a countdown and nothing to read: the work
// happens on paper, away from the screen. The tutor and the key are both out
// of reach until the learner hands in.

import { useState } from "react";

import { clockText } from "./Entrance";
import { useNow } from "./Session";
import type { Home, Paper as PaperInfo } from "./types";

export function Paper(props: {
  home: Home;
  /** The paper chosen on the entrance, if one was. */
  chosen: PaperInfo | null;
  error: string | null;
  onStart: (paper: PaperInfo, minutes: number) => void;
  onHandIn: () => void;
  onEnd: () => void;
  onBack: () => void;
}) {
  const { home } = props;
  const now = useNow(1000);
  const sitting = home.sitting;
  const [picked, setPicked] = useState<PaperInfo | null>(props.chosen ?? home.papers.find((p) => !p.sat) ?? home.papers[0] ?? null);
  const [minutes, setMinutes] = useState<string>(String(picked?.minutes ?? home.block_minutes));
  const [confirmEnd, setConfirmEnd] = useState(false);

  if (sitting) {
    const leftSeconds = Math.round((sitting.started_ms + sitting.minutes * 60_000 - now) / 1000);
    return (
      <section className="scr">
        <div className="top" data-tauri-drag-region />
        <div className="paper">
          <div className="pcol">
            <div className="meta">
              {home.name} · {sitting.name} · {sitting.minutes} min
            </div>
            <div className={leftSeconds < 0 ? "count over" : "count"}>{clockText(leftSeconds)}</div>
            <div>Write on paper. Put ? where you are stuck. After each answer write S, U or G.</div>
            <div className="lock" style={{ marginTop: 8 }}>
              {leftSeconds < 0
                ? "Time is up. Hand in what you have."
                : "The tutor is off and the key is locked until you hand in."}
            </div>
            <div className="row center">
              <button className="btn pri" autoFocus onClick={props.onHandIn}>
                Hand in · photo or PDF
              </button>
            </div>
            <div className="row center">
              {confirmEnd ? (
                <>
                  <span className="lock">End without handing in? Nothing is marked.</span>
                  <button className="btn sm" onClick={props.onEnd}>
                    End the sitting
                  </button>
                  <button className="btn ghost sm" onClick={() => setConfirmEnd(false)}>
                    Keep going
                  </button>
                </>
              ) : (
                <button className="btn ghost sm" onClick={() => setConfirmEnd(true)}>
                  End without handing in
                </button>
              )}
            </div>
            {props.error && <div className="note warn selectable">{props.error}</div>}
          </div>
        </div>
      </section>
    );
  }

  const length = Math.round(Number(minutes));
  const valid = picked !== null && Number.isFinite(length) && length >= 1 && length <= 600;
  return (
    <section className="scr">
      <div className="top" data-tauri-drag-region />
      <div className="paper">
        <div className="pcol">
          <div className="meta">
            {home.name}
            {home.exam && ` · ${home.exam.title}`}
          </div>
          {home.papers.length === 0 ? (
            <>
              <h2 className="q">No paper to sit yet</h2>
              <div className="sub">
                The tutor builds papers for an exam plan. Start a lesson and name the exam, and the papers appear here.
              </div>
              <div className="row center">
                <button className="btn pri" autoFocus onClick={props.onBack}>
                  Back &#8629;
                </button>
              </div>
            </>
          ) : (
            <>
              <h2 className="q">Sit a paper</h2>
              <div className="plist">
                {home.papers.map((paper) => (
                  <button
                    key={paper.path}
                    className={picked?.path === paper.path ? "on" : undefined}
                    onClick={() => {
                      setPicked(paper);
                      setMinutes(String(paper.minutes ?? home.block_minutes));
                    }}
                  >
                    {paper.name}
                    <span>{paper.sat ? "sat before" : paper.minutes ? `${paper.minutes} min` : ""}</span>
                  </button>
                ))}
              </div>
              <div className="row center">
                <input
                  className="in"
                  style={{ width: 84, textAlign: "center" }}
                  inputMode="numeric"
                  aria-label="Minutes"
                  value={minutes}
                  onChange={(e) => setMinutes(e.target.value.replace(/[^0-9]/g, ""))}
                />
                <span className="lock">minutes</span>
              </div>
              <div className="row center">
                <button className="btn pri" disabled={!valid} onClick={() => picked && props.onStart(picked, length)}>
                  Start the clock &#8629;
                </button>
                <button className="btn ghost" onClick={props.onBack}>
                  Back
                </button>
              </div>
              <div className="lock" style={{ marginTop: 10 }}>
                The paper opens to print. Starting hides the tutor until you hand in.
              </div>
            </>
          )}
          {props.error && <div className="note warn selectable">{props.error}</div>}
        </div>
      </div>
    </section>
  );
}
