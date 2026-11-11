// Small parts every screen shares: the top strip, the proof ring, the three
// proof dots, the week of study days, and a few ways to say a time.

import { useEffect, useState, type ReactNode } from "react";

import type { Activity, Dots as DotsData, Exam, Proofs } from "../types";
import { Icon } from "./icons";

/** True while an input method is composing. WebKit reports the keydown that confirms a word with keyCode 229 and isComposing false. */
export const composing = (event: { nativeEvent: KeyboardEvent; keyCode: number }) =>
  event.nativeEvent.isComposing || event.keyCode === 229;

/** The time now, refreshed on a slow tick. */
export function useNow(everyMs: number): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), everyMs);
    return () => window.clearInterval(timer);
  }, [everyMs]);
  return now;
}

export function clockText(leftSeconds: number): string {
  const over = leftSeconds < 0;
  const s = Math.abs(leftSeconds);
  const mm = String(Math.floor(s / 60)).padStart(2, "0");
  const ss = String(s % 60).padStart(2, "0");
  return `${over ? "+" : ""}${mm}:${ss}`;
}

/** "today", "tomorrow", "6 days", or null when the day is past or unknown. */
export function daysLeftText(days: number | null | undefined): string | null {
  if (days === null || days === undefined || days < 0) return null;
  if (days === 0) return "today";
  if (days === 1) return "tomorrow";
  return `${days} days`;
}

export function agoText(days: number | null): string {
  if (days === null) return "";
  if (days <= 0) return "today";
  if (days === 1) return "yesterday";
  return `${days} days ago`;
}

/** How near an exam is, as a word the styles colour: "now" within 3 days, "soon" within a week. */
export function urgency(exam: Exam | null): "now" | "soon" | "far" | "none" {
  const days = exam?.days_left;
  if (days === null || days === undefined || days < 0) return "none";
  return days <= 3 ? "now" : days <= 7 ? "soon" : "far";
}

/**
 * The strip at the top of every screen. It is also how the window is dragged,
 * and the traffic lights sit over its left end.
 */
export function TopBar(props: { left?: ReactNode; center?: ReactNode; right?: ReactNode; quiet?: boolean }) {
  return (
    <header className={props.quiet ? "bar quiet" : "bar"} data-tauri-drag-region>
      <div className="bar-side" data-tauri-drag-region>
        {props.left}
      </div>
      <div className="bar-center" data-tauri-drag-region>
        {props.center}
      </div>
      <div className="bar-side end" data-tauri-drag-region>
        {props.right}
      </div>
    </header>
  );
}

export function IconButton(props: {
  icon: string;
  label: string;
  onClick: () => void;
  disabled?: boolean;
  active?: boolean;
  size?: number;
}) {
  return (
    <button
      className={props.active ? "icon-btn on" : "icon-btn"}
      title={props.label}
      aria-label={props.label}
      disabled={props.disabled}
      onClick={props.onClick}
    >
      <Icon name={props.icon} size={props.size ?? 18} />
    </button>
  );
}

export function PaletteKey({ onClick }: { onClick: () => void }) {
  return (
    <button className="key" title="Everything else" aria-label="Open commands" onClick={onClick}>
      &#8984;K
    </button>
  );
}

/** A ring that fills with the share given, with anything in its middle. */
export function Ring(props: { share: number; size: number; stroke?: number; tone?: string; children?: ReactNode; label?: string }) {
  const stroke = props.stroke ?? Math.max(3, Math.round(props.size / 12));
  const radius = (props.size - stroke) / 2;
  const round = 2 * Math.PI * radius;
  const share = Math.max(0, Math.min(1, props.share));
  return (
    <span className={`ring ${props.tone ?? ""}`} style={{ width: props.size, height: props.size }} role="img" aria-label={props.label}>
      <svg width={props.size} height={props.size} viewBox={`0 0 ${props.size} ${props.size}`}>
        <circle className="ring-track" cx={props.size / 2} cy={props.size / 2} r={radius} strokeWidth={stroke} />
        <circle
          className="ring-fill"
          cx={props.size / 2}
          cy={props.size / 2}
          r={radius}
          strokeWidth={stroke}
          strokeDasharray={round}
          strokeDashoffset={round * (1 - share)}
          transform={`rotate(-90 ${props.size / 2} ${props.size / 2})`}
        />
      </svg>
      {props.children !== undefined && <span className="ring-mid">{props.children}</span>}
    </span>
  );
}

/** The share of a course's proofs that are earned. */
export function proofShare(proofs: Proofs): number {
  return proofs.total === 0 ? 0 : proofs.earned / proofs.total;
}

/** Three marks: a filled one is a proof that counts, a faded one was earned before a later miss. */
export function Dots({ dots }: { dots: DotsData }) {
  const earned = dots.filter((d) => d === 2).length;
  return (
    <span className="dots" role="img" aria-label={`${earned} of 3 proofs`}>
      {dots.map((dot, index) => (
        <i key={index} className={dot === 2 ? "full" : dot === 1 ? "past" : undefined} />
      ))}
    </span>
  );
}

const WEEKDAYS = ["M", "T", "W", "T", "F", "S", "S"];

function addDays(day: string, by: number): string {
  const date = new Date(`${day}T12:00:00`);
  date.setDate(date.getDate() + by);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

/** Monday to Sunday of this week, with the days studied filled in. */
export function Week({ activity, celebrate }: { activity: Activity; celebrate?: boolean }) {
  const today = new Date(`${activity.today}T12:00:00`);
  const sinceMonday = (today.getDay() + 6) % 7;
  const studied = new Set(activity.days);
  return (
    <span className="week" role="img" aria-label={`${activity.run} days running`} title={RUN_HINT}>
      {WEEKDAYS.map((letter, index) => {
        const date = addDays(activity.today, index - sinceMonday);
        const isToday = index === sinceMonday;
        const kind = studied.has(date) ? "done" : index > sinceMonday ? "ahead" : "missed";
        return (
          <span key={index} className={`week-day ${kind}${isToday ? " today" : ""}${isToday && celebrate ? " pop" : ""}`}>
            <i>{studied.has(date) && <Icon name="check" size={11} />}</i>
            <b>{letter}</b>
          </span>
        );
      })}
    </span>
  );
}

/** What a proof is, in one line, for a tooltip. */
export const PROOF_HINT = "A proof is one answer you gave alone, on its own day. Three proofs prove a skill.";

/** What the run is, in one line, for a tooltip. */
export const RUN_HINT = "Days on which you answered a question, in a row. One missed day a week is a rest day and does not end the run.";

/** Five weeks of study days, oldest first, as small squares. */
export function Weeks({ activity }: { activity: Activity }) {
  const today = new Date(`${activity.today}T12:00:00`);
  const sinceMonday = (today.getDay() + 6) % 7;
  const studied = new Set(activity.days);
  const cells: { date: string; done: boolean; ahead: boolean }[] = [];
  for (let index = -28; index < 7; index++) {
    const offset = index - sinceMonday;
    const date = addDays(activity.today, offset);
    cells.push({ date, done: studied.has(date), ahead: offset > 0 });
  }
  return (
    <span className="weeks" role="img" aria-label="Study days in the last five weeks">
      {cells.map((cell) => (
        <i key={cell.date} className={cell.ahead ? "ahead" : cell.done ? "done" : undefined} title={cell.ahead ? undefined : cell.date} />
      ))}
    </span>
  );
}
