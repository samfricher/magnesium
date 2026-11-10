// The first thing on screen: one course, where the learner stopped, and one
// card with one next action. Not here: statistics, a list of sessions, a
// graph, a streak, or any count above three. Everything else is behind ⌘K.

import { useEffect, useRef, useState } from "react";

import { composing, useNow } from "./Session";
import type { Home, Paper } from "./types";

function ago(days: number | null): string {
  if (days === null) return "Last time";
  if (days <= 0) return "Earlier today";
  if (days === 1) return "Yesterday";
  return `Last time, ${days} days ago`;
}

function daysLeft(days: number | null): string | null {
  if (days === null) return null;
  if (days < 0) return "past";
  if (days === 0) return "today";
  return days === 1 ? "1 day" : `${days} days`;
}

export function clockText(leftSeconds: number): string {
  const over = leftSeconds < 0;
  const s = Math.abs(leftSeconds);
  const mm = String(Math.floor(s / 60)).padStart(2, "0");
  const ss = String(s % 60).padStart(2, "0");
  return `${over ? "+" : ""}${mm}:${ss}`;
}

export function Welcome(props: { recent: string[]; error: string | null; onPick: () => void; onOpen: (path: string) => void }) {
  return (
    <section className="scr">
      <div className="top" data-tauri-drag-region>
        <span>magnesium</span>
      </div>
      <div className="col">
        <div className="card">
          <div className="meta">no folder open</div>
          <h2 className="q">Open a study folder</h2>
          <div className="sub">The folder with your course files. Your record stays in it.</div>
          <div className="row">
            <button className="btn pri" autoFocus onClick={props.onPick}>
              Open a folder &#8629;
            </button>
          </div>
          {props.error && <div className="note warn selectable">{props.error}</div>}
          {props.recent.length > 0 && (
            <div className="recent">
              {props.recent.slice(0, 5).map((path) => (
                <button key={path} onClick={() => props.onOpen(path)}>
                  {path.split("/").filter(Boolean).pop()}
                  <span>{path}</span>
                </button>
              ))}
            </div>
          )}
        </div>
      </div>
    </section>
  );
}

export function Entrance(props: {
  home: Home;
  /** True while the card asks what to learn, for a new lesson in a folder that has one. */
  asking: boolean;
  error: string | null;
  onContinue: (skipChecks: boolean) => void;
  onNewLesson: (goal: string) => void;
  onAsk: (asking: boolean) => void;
  onPaper: (paper: Paper | null) => void;
  onRetry: () => void;
  onPalette: () => void;
}) {
  const { home } = props;
  const [goal, setGoal] = useState("");
  const now = useNow(1000);
  const input = useRef<HTMLInputElement>(null);
  const fresh = home.state === "empty" || home.state === "not_set_up";
  const asking = fresh || props.asking;

  useEffect(() => {
    if (asking) input.current?.focus();
  }, [asking]);

  const exam = home.exam;
  const left = exam ? daysLeft(exam.days_left) : null;
  const nextPaper = home.papers.find((paper) => !paper.sat) ?? null;
  const checks = Math.min(home.checks_due ?? 0, 3);
  const sitting = home.sitting;

  let card: JSX.Element;
  if (home.state === "no_runtime" || home.state === "unreadable") {
    card = (
      <>
        <div className="meta">something to fix first</div>
        <h2 className="q">{home.state === "no_runtime" ? "Magnesium is not installed" : "This folder could not be read"}</h2>
        <div className="sub selectable">{home.message}</div>
        <div className="row">
          <button className="btn pri" autoFocus onClick={props.onRetry}>
            Try again &#8629;
          </button>
        </div>
      </>
    );
  } else if (sitting) {
    const leftSeconds = Math.round((sitting.started_ms + sitting.minutes * 60_000 - now) / 1000);
    card = (
      <>
        <div className="meta">a paper is running</div>
        <h2 className="q">
          {sitting.name} · {clockText(leftSeconds)} {leftSeconds >= 0 ? "left" : "over"}
        </h2>
        <div className="sub">The tutor is off until you hand in.</div>
        <div className="row">
          <button className="btn pri" autoFocus onClick={() => props.onPaper(null)}>
            Back to the paper &#8629;
          </button>
        </div>
      </>
    );
  } else if (asking) {
    const start = () => goal.trim() !== "" && props.onNewLesson(goal.trim());
    card = (
      <>
        <div className="meta">{fresh ? "a new folder" : "a new lesson"}</div>
        <h2 className="q">What do you want to learn from this folder?</h2>
        <div className="sub">Name a topic, a file in the folder, or an exam and its date.</div>
        <input
          ref={input}
          className="in"
          type="text"
          placeholder="for example: the midterm on 17 Nov, chapters 4 to 6"
          autoComplete="off"
          value={goal}
          onChange={(e) => setGoal(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !composing(e)) start();
            if (e.key === "Escape" && !fresh) props.onAsk(false);
          }}
        />
        <div className="row">
          <button className="btn pri" disabled={goal.trim() === ""} onClick={start}>
            Start &#8629;
          </button>
          {!fresh && (
            <button className="btn ghost" onClick={() => props.onAsk(false)}>
              Back
            </button>
          )}
          {home.state === "not_set_up" && <span className="lock">Start sets this folder up for Magnesium</span>}
        </div>
      </>
    );
  } else {
    const due: string[] = [];
    if (checks > 0) due.push(`${checks} quick ${checks === 1 ? "check" : "checks"} first · about ${checks} min`);
    card = (
      <>
        <div className="meta">
          {due.length > 0 ? due.join(" · ") : home.last && home.checks_due !== null ? "nothing due" : "ready"}
        </div>
        <h2 className="q selectable">{home.last ? `Next: ${home.last.goal}` : `Start on ${exam?.title ?? home.name}`}</h2>
        {home.redos_due > 0 ? (
          <div className="sub">
            {home.redos_due} paper {home.redos_due === 1 ? "redo" : "redos"} due
            {home.first_redo ? `, first ${home.first_redo}` : ""}
          </div>
        ) : (
          <div className="sub">The tutor picks up from your record.</div>
        )}
        <div className="row">
          <button className="btn pri" autoFocus onClick={() => props.onContinue(false)}>
            Start &#8629;
          </button>
          {nextPaper && (
            <button className="btn ghost" onClick={() => props.onPaper(nextPaper)}>
              or sit {nextPaper.name}
              {nextPaper.minutes ? ` · ${nextPaper.minutes} min` : ""}
            </button>
          )}
          {checks > 0 && (
            <button className="btn ghost" onClick={() => props.onContinue(true)}>
              Not now
            </button>
          )}
        </div>
      </>
    );
  }

  return (
    <section className="scr">
      <div className="top" data-tauri-drag-region>
        <span>magnesium</span>
        <span className="grow" data-tauri-drag-region />
        <button className="kbd" title="Everything else lives here" onClick={props.onPalette}>
          &#8984;K
        </button>
      </div>
      <div className="col">
        <div className="meta">
          {home.name}
          {exam && ` · ${exam.title}`}
          {exam?.day && ` · ${exam.day}`}
          {left && (
            <>
              {" · "}
              <b>{left}</b>
            </>
          )}
        </div>
        {home.last && home.state === "ready" ? (
          <div className="last selectable">
            {home.last.stopped_at
              ? `${ago(home.last.days_ago)}, you stopped at: ${home.last.stopped_at}`
              : home.last.next_action
                ? `${ago(home.last.days_ago)}. Up next: ${home.last.next_action}`
                : `${ago(home.last.days_ago)}: ${home.last.goal}`}
          </div>
        ) : (
          <div className="last">&nbsp;</div>
        )}
        <div className="card">{card}</div>
        {props.error && <div className="note warn selectable">{props.error}</div>}
      </div>
    </section>
  );
}
