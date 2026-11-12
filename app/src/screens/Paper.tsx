// A timed paper. The screen holds a countdown and nothing to read: the work
// happens on paper, away from the screen. The tutor and the key are both out
// of reach until the learner hands in.

import { useEffect, useRef, useState } from "react";

import { Icon } from "../ui/icons";
import { PaletteKey, TopBar, clockText, radioKey, useNow, useReturnForPrimary } from "../ui/bits";
import type { Home, Paper as PaperInfo } from "../types";
import { displayName } from "./Library";

export function Paper(props: {
  home: Home;
  /** The paper chosen on the course page, if one was. */
  chosen: PaperInfo | null;
  error: string | null;
  onStart: (paper: PaperInfo, minutes: number) => void;
  onHandIn: () => void;
  onEnd: () => void;
  onBack: () => void;
  onPalette: () => void;
}) {
  const { home } = props;
  useReturnForPrimary();
  const options = useRef<(HTMLButtonElement | null)[]>([]);
  const now = useNow(1000);
  const sitting = home.sitting;
  const [picked, setPicked] = useState<PaperInfo | null>(props.chosen ?? home.papers.find((p) => !p.sat) ?? home.papers[0] ?? null);
  const [minutes, setMinutes] = useState<string>(String(picked?.minutes ?? home.block_minutes));
  const [confirmEnd, setConfirmEnd] = useState(false);
  // Handing in marks the paper and ends the sitting, so it asks first. Keep working has focus, so Return never hands in by itself.
  const [confirmIn, setConfirmIn] = useState(false);
  const keep = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (confirmIn) keep.current?.focus();
  }, [confirmIn]);

  if (sitting) {
    const total = sitting.minutes * 60;
    const leftSeconds = Math.round((sitting.started_ms + total * 1000 - now) / 1000);
    const over = leftSeconds < 0;
    return (
      <section className="screen paper running" data-screen="paper">
        <TopBar
          left={
            <button className="crumb" onClick={props.onBack} title={`${displayName(home.name)} (Esc). The paper keeps running.`}>
              <Icon name="back" size={16} />
              {displayName(home.name)}
            </button>
          }
          right={<PaletteKey onClick={props.onPalette} />}
        />
        <div className="center top">
          <div className="paper-col">
            <h1 className="h-title">{sitting.name}</h1>
            <p className="paper-clock" role="timer" aria-label="Time left">
              <span className={over ? "count over" : "count"} data-role="countdown">
                {clockText(leftSeconds)}
              </span>
              <small>
                {over ? "over" : "left"} of {sitting.minutes} min
              </small>
            </p>
            <p className="paper-rule">
              {over ? (
                "Time is up. Hand in what you have."
              ) : (
                <>
                  Write on paper. Put <b>?</b> where you are stuck. After each answer write <b>S</b> for sure, <b>U</b> for unsure or <b>G</b> for a guess.
                </>
              )}
            </p>
            {confirmIn ? (
              <div
                className="paper-ask"
                role="group"
                aria-label={`Hand in ${sitting.name}?`}
                onKeyDown={(event) => {
                  if (event.key === "Escape") {
                    event.preventDefault();
                    event.stopPropagation();
                    setConfirmIn(false);
                  }
                }}
              >
                <p className="paper-ask-q">Hand in {sitting.name} now?</p>
                <div className="row center paper-actions">
                  <button
                    className="btn primary"
                    title="Hand in (Return)"
                    onClick={() => {
                      setConfirmIn(false);
                      props.onHandIn();
                    }}
                    data-role="paper-hand-in"
                  >
                    Hand in
                  </button>
                  <button className="btn" ref={keep} onClick={() => setConfirmIn(false)}>
                    Keep working
                  </button>
                </div>
              </div>
            ) : (
              <div className="row center paper-actions">
                <button className="btn primary" title="Hand in a photo or PDF (Return)" onClick={() => setConfirmIn(true)} data-role="paper-submit">
                  Hand in a photo or PDF
                </button>
                {confirmEnd ? (
                  <>
                    <button className="btn danger" onClick={props.onEnd}>
                      End the paper
                    </button>
                    <button className="btn" onClick={() => setConfirmEnd(false)}>
                      Keep going
                    </button>
                  </>
                ) : (
                  <button className="btn" data-role="paper-end" onClick={() => setConfirmEnd(true)}>
                    End without handing in
                  </button>
                )}
              </div>
            )}
            {confirmEnd && <p className="aside">Ending now marks nothing.</p>}
            <p className="aside">
              <Icon name="lock" size={13} /> The tutor is off and the key is locked until you hand in.
            </p>
            {props.error && <div className="banner bad selectable">{props.error}</div>}
          </div>
        </div>
      </section>
    );
  }

  const length = Math.round(Number(minutes));
  const valid = picked !== null && Number.isFinite(length) && length >= 1 && length <= 600;
  return (
    <section className="screen paper" data-screen="paper">
      <TopBar
        left={
          <button className="crumb" onClick={props.onBack} title={`${displayName(home.name)} (Esc)`}>
            <Icon name="back" size={16} />
            {displayName(home.name)}
          </button>
        }
        right={<PaletteKey onClick={props.onPalette} />}
      />
      <div className="center top">
        <div className="paper-col">
          {home.papers.length === 0 ? (
            <>
              <h1 className="h-title">No paper to sit yet</h1>
              <p className="paper-rule">The tutor builds papers for an exam. Start a lesson and name the exam, and its papers appear here.</p>
              <div className="row center">
                <button className="btn primary" title={`${displayName(home.name)} (Return)`} onClick={props.onBack}>
                  Back
                </button>
              </div>
            </>
          ) : (
            <>
              <h1 className="h-title">Timed paper</h1>
              <div
                className="paper-list"
                role="radiogroup"
                aria-label="Papers"
                onKeyDown={(event) => {
                  // Return on a row starts the clock, as the green button does.
                  if (event.key === "Enter" && valid && picked && !event.nativeEvent.isComposing) {
                    event.preventDefault();
                    props.onStart(picked, length);
                    return;
                  }
                  const at = Math.max(0, home.papers.findIndex((paper) => paper.path === picked?.path));
                  radioKey(event, at, home.papers.length, (to) => {
                    const paper = home.papers[to];
                    setPicked(paper);
                    setMinutes(String(paper.minutes ?? home.block_minutes));
                    options.current[to]?.focus();
                  });
                }}
              >
                {home.papers.map((paper, index) => (
                  <button
                    key={paper.path}
                    ref={(node) => {
                      options.current[index] = node;
                    }}
                    role="radio"
                    aria-checked={picked?.path === paper.path}
                    tabIndex={picked?.path === paper.path || (picked === null && index === 0) ? 0 : -1}
                    className={picked?.path === paper.path ? "on" : undefined}
                    onClick={() => {
                      setPicked(paper);
                      setMinutes(String(paper.minutes ?? home.block_minutes));
                    }}
                  >
                    <span>{paper.name}</span>
                    <small>{paper.sat ? "sat before" : paper.minutes ? `${paper.minutes} min` : ""}</small>
                  </button>
                ))}
              </div>
              <label className="minutes">
                <input
                  className="field"
                  inputMode="numeric"
                  aria-label="Minutes"
                  value={minutes}
                  onChange={(e) => setMinutes(e.target.value.replace(/[^0-9]/g, ""))}
                  onKeyDown={(event) => {
                    if (event.key === "Enter" && valid && picked && !event.nativeEvent.isComposing) {
                      event.preventDefault();
                      props.onStart(picked, length);
                    }
                  }}
                />
                minutes
              </label>
              <div className="row center">
                <button className="btn primary" disabled={!valid} title="Start the clock (Return)" onClick={() => picked && props.onStart(picked, length)}>
                  Start the clock
                </button>
                {!valid && <span className="aside">Enter a length from 1 to 600 minutes.</span>}
              </div>
              <p className="aside">The paper opens to print. The tutor stays off until you hand in.</p>
            </>
          )}
          {props.error && <div className="banner bad selectable">{props.error}</div>}
        </div>
      </div>
    </section>
  );
}
