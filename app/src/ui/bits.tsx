// Small parts every screen shares: the top strip, the proof dots, the level,
// the week of study days, the due token, and a few ways to say a time. Each
// concept is drawn here once and used everywhere.

import { useEffect, useState, type ReactNode } from "react";

import type { Activity, Dots as DotsData, Exam, Progress, Proofs } from "../types";
import { Icon } from "./icons";
import { Line } from "./Rich";

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

/** The words after an exam's title in a chip: " in 6 days", " is tomorrow", " is today", or "". */
export function examWhen(exam: Exam): string {
  const days = exam.days_left;
  if (days === null || days === undefined || days < 0) return "";
  if (days === 0) return " is today";
  if (days === 1) return " is tomorrow";
  return ` in ${days} days`;
}

/** The exam as one sentence: "Midterm 1 is on Tue 17 Nov, in 6 days." The title may hold mathematics. */
export function ExamSentence({ exam }: { exam: Exam }) {
  const days = exam.days_left;
  const when = days === null || days === undefined || days < 0 ? null : days === 0 ? "today" : days === 1 ? "tomorrow" : `in ${days} days`;
  const on = exam.day ? `on ${exam.day}` : null;
  const tail = on && when ? `${on}, ${when}` : (on ?? when);
  return (
    <>
      <Line text={exam.title} />
      {tail ? ` is ${tail}.` : ""}
    </>
  );
}

/** A goal such as "PS3 Q3 · reading Vmax" as a short reference and a title. */
export function splitGoal(goal: string): { ref: string | null; title: string } {
  const parts = /^(.{1,28}?)\s+[\u00b7\u2022]\s+(.+)$/.exec(goal.trim());
  const ref = parts ? parts[1] : null;
  const rest = parts ? parts[2] : goal.trim();
  const title = rest.replace(/^[a-z]/, (letter) => letter.toUpperCase()).replace(/\s+[\u00b7\u2022]\s+/g, ", ");
  return { ref, title };
}

/** A line of text with no middle dots in it, and a full stop at its end. */
export function sentence(text: string): string {
  const plain = text.trim().replace(/\s+[\u00b7\u2022]\s+/g, ", ");
  return /[.!?]$/.test(plain) ? plain : `${plain}.`;
}

/** Say a list as one run of words: "a, b and c". */
export function plain(text: string): string {
  return text.replace(/\s+[\u00b7\u2022]\s+/g, ", ");
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

/** The menu's key, with its name: "Menu ⌘K". The same on every screen. */
export function PaletteKey({ onClick }: { onClick: () => void }) {
  return (
    <button className="menu-key" title="Menu: library, proofs, papers, settings (⌘K)" aria-label="Open the menu (Command K)" onClick={onClick}>
      Menu
      <kbd aria-hidden="true">&#8984;K</kbd>
    </button>
  );
}

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** "3 of 11 skills proven": what the library can say of a course, and the lead of what the other screens say. */
export function skillsProven(proofs: Proofs): string | null {
  if (proofs.skills === 0) return null;
  return `${proofs.proven} of ${proofs.skills} ${proofs.skills === 1 ? "skill" : "skills"} proven`;
}

/** The skills of a course in the four places they can be, counted the one way on every screen. */
export type SkillBuckets = { proven: number; today: number; later: number; untested: number };

export function skillBuckets(progress: Progress): SkillBuckets {
  const today = progress.in_progress.filter((row) => row.due).length;
  return {
    proven: progress.proofs.proven,
    today,
    later: progress.in_progress.length - today,
    untested: progress.untested.length,
  };
}

/** "3 proven, 2 to prove today, 4 in progress, 2 not tested yet." Null when there is nothing to count. */
export function bucketsSentence(buckets: SkillBuckets): string | null {
  const parts = [
    buckets.proven > 0 ? `${buckets.proven} proven` : null,
    buckets.today > 0 ? `${buckets.today} to prove today` : null,
    buckets.later > 0 ? `${buckets.later} in progress` : null,
    buckets.untested > 0 ? `${buckets.untested} not tested yet` : null,
  ].filter((part): part is string => part !== null);
  return parts.length > 0 ? `${parts.join(", ")}.` : null;
}

/** "5 questions due today": quick checks and paper redos, the one noun for them on every screen. */
export function questionsDue(n: number): string {
  return `${plural(n, "question", "questions")} due today`;
}

/** The next level, in words: "4 more proofs to reach level 6." Null at the top, or with no level. */
export function nextLevelSentence(proofs: Proofs): string | null {
  if (!proofs.levelled || proofs.total === 0 || proofs.to_next <= 0) return null;
  return `${plural(proofs.to_next, "more proof", "more proofs")} to reach level ${proofs.level + 1}.`;
}

/** The share of a course's proofs that are earned. */
export function proofShare(proofs: Proofs): number {
  return proofs.total === 0 ? 0 : proofs.earned / proofs.total;
}

/** How many proofs a skill holds that count: its filled dots. */
export function proofsOf(dots: DotsData): number {
  return dots.filter((dot) => dot === 2).length;
}

/**
 * The proof mark: three dots. A filled dot is a proof that counts, a ring is
 * one still to earn. A proof earned today has a soft halo; with `flare` the
 * new dot burns white for an instant and settles into green. There is no
 * other state: a skill that was restarted shows rings and says so in words.
 */
export function ProofDots({
  earned,
  today = 0,
  flare = false,
  size = 12,
  label,
}: {
  earned: number;
  today?: number;
  flare?: boolean;
  /** 12 everywhere a skill appears; 20 for the reward at the end of a lesson. */
  size?: 12 | 20;
  /** What the mark says to a screen reader, when "2 of 3 proofs" is not enough. "" hides it, where words beside it say the same. */
  label?: string;
}) {
  const total = Math.max(0, Math.min(3, earned));
  const fresh = Math.max(0, Math.min(total, today));
  return (
    <span
      className={size === 20 ? "dots lg" : "dots"}
      role={label === "" ? undefined : "img"}
      aria-hidden={label === "" ? true : undefined}
      aria-label={label === "" ? undefined : (label ?? `${total} of 3 proofs${fresh > 0 ? `, ${fresh} earned today` : ""}`)}
    >
      {[0, 1, 2].map((index) => (
        <i key={index} className={index < total - fresh ? "on" : index < total ? (flare ? "on new flare" : "on new") : undefined} />
      ))}
    </span>
  );
}

/** One dot, for the legend. */
export function ProofDot({ state }: { state: "on" | "off" | "new" }) {
  return (
    <span className="dots" aria-hidden="true">
      <i className={state === "off" ? undefined : state === "new" ? "on new" : "on"} />
    </span>
  );
}

const LEVEL_SIZE = { sm: 56, md: 80, lg: 104 } as const;

/** The level: a thin ring that fills with the share of proofs earned, the number in the middle. */
export function Level({
  value,
  size = "md",
  share,
  gain = 0,
  title,
}: {
  value: number;
  size?: "sm" | "md" | "lg";
  share?: number;
  /** The share of the course's proofs earned today. Drawn in green over the rest, which turns to ink. */
  gain?: number;
  title?: string;
}) {
  const box = LEVEL_SIZE[size];
  const stroke = 2.5;
  const radius = (box - stroke) / 2;
  const round = 2 * Math.PI * radius;
  const filled = Math.max(0, Math.min(1, share ?? 0));
  const today = Math.max(0, Math.min(filled, gain));
  const before = filled - today;
  const centre = box / 2;
  return (
    <span className={`level-ring ${size}`} style={{ width: box, height: box }} role="img" aria-label={`Level ${value}`} title={title}>
      <svg width={box} height={box} viewBox={`0 0 ${box} ${box}`} aria-hidden="true">
        <circle className="level-track" cx={centre} cy={centre} r={radius} />
        {before > 0 && (
          <circle
            className={today > 0 ? "level-fill before" : "level-fill"}
            cx={centre}
            cy={centre}
            r={radius}
            strokeWidth={stroke}
            strokeDasharray={round}
            strokeDashoffset={round * (1 - before)}
            transform={`rotate(-90 ${centre} ${centre})`}
          />
        )}
        {today > 0 && (
          <circle
            className="level-fill gain"
            cx={centre}
            cy={centre}
            r={radius}
            strokeWidth={stroke}
            style={{ strokeDasharray: `${round * today} ${round}`, ["--round" as string]: `${round}px` }}
            transform={`rotate(${-90 + 360 * before} ${centre} ${centre})`}
          />
        )}
      </svg>
      <span className="level-mid">
        <b className="num">{value}</b>
        <small>Level</small>
      </span>
    </span>
  );
}

/** A due thing: an amber point and a few words. With a number, "5 questions due today"; without, "due today". */
export function Due({ n }: { n?: number }) {
  return (
    <span className="due">
      <i />
      {n === undefined ? "due today" : questionsDue(n)}
    </span>
  );
}

/** "4 days running", or the invitation to start a run. */
export function Run({ days, invite }: { days: number; invite?: boolean }) {
  if (days <= 0) return invite ? <span className="run-line" title={RUN_HINT}>One marked answer starts a run</span> : null;
  return (
    <span className="run-line num" title={RUN_HINT}>
      {days} {days === 1 ? "day" : "days"} running
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

/**
 * Seven days, Monday to Sunday, `back` weeks before the week of `today`. A day
 * studied is filled, today is ringed, any other day is an empty ring.
 */
export function Week({ days, today, back = 0, letters = true }: { days: string[]; today: string; back?: number; letters?: boolean }) {
  const sinceMonday = (new Date(`${today}T12:00:00`).getDay() + 6) % 7;
  const studied = new Set(days);
  const dates = WEEKDAYS.map((_, index) => addDays(today, index - sinceMonday - back * 7));
  const count = dates.filter((date) => studied.has(date)).length;
  return (
    <span className="week" role="img" aria-label={`${count} of 7 days studied`}>
      {WEEKDAYS.map((letter, index) => {
        const isToday = dates[index] === today;
        return (
          <span key={index} className={`week-day${studied.has(dates[index]) ? " done" : ""}${isToday ? " today" : ""}`}>
            <i />
            {letters && <b>{letter}</b>}
          </span>
        );
      })}
    </span>
  );
}

/** The week of study days the learner is in, and the run, as the library and the end of a sitting show them. */
export function ThisWeek({ activity }: { activity: Activity }) {
  return <Week days={activity.days} today={activity.today} />;
}

/** What a proof is, in one line, for a tooltip. */
export const PROOF_HINT = "A proof is one answer you gave alone, on its own day. Three proofs prove a skill.";

/** What the run is, in one line, for a tooltip. */
export const RUN_HINT = "Days on which you had an answer marked, in a row. One missed day a week is a rest day and does not end the run.";

