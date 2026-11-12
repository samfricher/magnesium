// Small parts every screen shares: the top strip, the proof dots, the level,
// the week of study days, the due token, and a few ways to say a time. Each
// concept is drawn here once and used everywhere.

import { useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type ReactNode } from "react";

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

/** A goal as its title, and after it the problem it came from in the quiet ink: "Reading Vmax from a plot, PS3 Q3". */
export function GoalText({ goal }: { goal: string }) {
  const { ref, title } = splitGoal(goal);
  return (
    <>
      <Line text={title} />
      {ref && <span className="goal-ref">, {ref}</span>}
    </>
  );
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

const TOP_LEVEL = 10;

/** The tally's ten strokes, how much of each is filled, and how much of that was earned today. */
export function levelStrokes(proofs: Proofs, gain: number): { a: number; g: number; full: boolean }[] {
  const { earned, total } = proofs;
  // Where the course stands, in strokes. Levels are tenths of the proofs, as the runtime counts them: level n begins at the n-1th ninth.
  const at = (held: number): number => {
    if (total <= 0) return 0;
    const level = Math.min(TOP_LEVEL, 1 + Math.floor((Math.max(0, held) * (TOP_LEVEL - 1)) / total));
    if (level >= TOP_LEVEL) return TOP_LEVEL;
    const lo = Math.ceil(((level - 1) * total) / (TOP_LEVEL - 1));
    const hi = Math.ceil((level * total) / (TOP_LEVEL - 1));
    return level - 1 + (hi > lo ? Math.max(0, Math.min(1, (held - lo) / (hi - lo))) : 0);
  };
  let now = at(earned);
  if (proofs.level >= TOP_LEVEL) now = TOP_LEVEL;
  else if (proofs.to_next > 0) {
    // The runtime says how many proofs the next level is away, which is the width of this stroke.
    const lo = Math.ceil(((proofs.level - 1) * total) / (TOP_LEVEL - 1));
    const hi = earned + proofs.to_next;
    now = proofs.level - 1 + (hi > lo ? Math.max(0, Math.min(1, (earned - lo) / (hi - lo))) : 0);
  }
  const before = gain > 0 ? Math.min(now, at(earned - gain)) : now;
  return Array.from({ length: TOP_LEVEL }, (_, k) => {
    const end = Math.max(0, Math.min(1, now - k));
    const a = Math.max(0, Math.min(1, before - k));
    return { a, g: end - a, full: end >= 1 };
  });
}

/** The tally alone: ten strokes. */
export function Tally({ proofs, size = "md", gain = 0 }: { proofs: Proofs; size?: "sm" | "md" | "lg"; gain?: number }) {
  return (
    <span className={`tally ${size}`} aria-hidden="true">
      {levelStrokes(proofs, gain).map((stroke, index) => (
        <i key={index} className={stroke.full ? "full" : undefined} style={{ ["--a" as string]: stroke.a, ["--g" as string]: stroke.g }} />
      ))}
    </span>
  );
}

/**
 * The level: a tally of ten strokes, and beside it "Level 5" and, where there
 * is room, how many proofs are left to the next. With `gain`, the part earned
 * today is drawn brighter and grows once.
 */
export function Level({
  proofs,
  size = "md",
  gain = 0,
  text = "next",
  title,
}: {
  proofs: Proofs;
  size?: "sm" | "md" | "lg";
  /** How many proofs were earned today, to draw in the brighter tint. */
  gain?: number;
  /** "next": the level and the next-level line; "level": the level only; "none": the tally alone. */
  text?: "next" | "level" | "none";
  title?: string;
}) {
  const next = nextLevelSentence(proofs);
  const name = `Level ${proofs.level} of ${TOP_LEVEL}${next ? `, ${next.replace(/\.$/, "")}` : ""}`;
  return (
    <span className={`lvl ${size}`} role="img" aria-label={name} title={title}>
      <Tally proofs={proofs} size={size} gain={gain} />
      {text !== "none" && (
        <span className="lvl-text">
          <b>Level {proofs.level}</b>
          {text === "next" && next && <span>{next}</span>}
        </span>
      )}
    </span>
  );
}

/** Move to the next or the previous of a group of radio buttons with the arrow keys. Returns true when the key was one. */
export function radioKey(event: ReactKeyboardEvent, at: number, count: number, move: (to: number) => void): boolean {
  let to = at;
  if (event.key === "ArrowRight" || event.key === "ArrowDown") to = (at + 1) % count;
  else if (event.key === "ArrowLeft" || event.key === "ArrowUp") to = (at - 1 + count) % count;
  else if (event.key === "Home") to = 0;
  else if (event.key === "End") to = count - 1;
  else return false;
  event.preventDefault();
  event.stopPropagation();
  move(to);
  return true;
}

/**
 * The one choice control: one of several, side by side. Arrow keys move and
 * choose, the group is one stop for Tab, and the choice made is filled in ink.
 */
export function Choice<T extends string>(props: {
  value: T | null;
  options: { value: T; label: ReactNode; title?: string }[];
  onChange: (value: T) => void;
  /** What the group is, for a screen reader. */
  label: string;
  size?: "md" | "sm";
  disabled?: boolean;
}) {
  const { options } = props;
  const buttons = useRef<(HTMLButtonElement | null)[]>([]);
  const chosen = options.findIndex((option) => option.value === props.value);
  return (
    <div
      className={`choose${props.size === "sm" ? " sm" : ""}${props.disabled ? " locked" : ""}`}
      role="radiogroup"
      aria-label={props.label}
      aria-disabled={props.disabled || undefined}
      onKeyDown={(event) => {
        if (props.disabled || options.length === 0) return;
        const at = chosen >= 0 ? chosen : 0;
        radioKey(event, at, options.length, (to) => {
          props.onChange(options[to].value);
          buttons.current[to]?.focus();
        });
      }}
    >
      {options.map((option, index) => (
        <button
          key={option.value}
          ref={(node) => {
            buttons.current[index] = node;
          }}
          type="button"
          role="radio"
          aria-checked={index === chosen}
          tabIndex={index === (chosen >= 0 ? chosen : 0) ? 0 : -1}
          title={option.title}
          disabled={props.disabled}
          onClick={() => {
            if (index !== chosen) props.onChange(option.value);
          }}
        >
          <span>{option.label}</span>
          <span className="c-ghost" aria-hidden="true">
            {option.label}
          </span>
        </button>
      ))}
    </div>
  );
}

/**
 * Enter does the screen's one green button when nothing has focus. The
 * window moves no focus by itself on load, so a ring is never hidden and the
 * first Tab lands on the first control; Return still goes straight on.
 */
export function useReturnForPrimary(): void {
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Enter" || event.defaultPrevented || event.isComposing || event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) return;
      const focused = document.activeElement;
      if (focused && focused !== document.body) return;
      if (document.querySelector(".palette-veil")) return;
      const button = document.querySelector<HTMLButtonElement>(".screen .btn.primary:not(:disabled)");
      if (!button) return;
      event.preventDefault();
      button.click();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
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

/** The week of study days the learner is in, and the run, as the end of a lesson shows them. */
export function ThisWeek({ activity }: { activity: Activity }) {
  return <Week days={activity.days} today={activity.today} />;
}

/** The study week as one quiet line: its name, the seven days, the run. */
export function WeekLine({ activity }: { activity: Activity }) {
  return (
    <p className="week-line">
      <span className="h-label">This week</span>
      <Week days={activity.days} today={activity.today} />
      <Run days={activity.run} />
    </p>
  );
}

/** What a proof is, in one line, for a tooltip. */
export const PROOF_HINT = "A proof is one answer you gave alone, on its own day. Three proofs prove a skill.";

/** What the run is, in one line, for a tooltip. */
export const RUN_HINT = "Days on which you had an answer marked, in a row. One missed day a week is a rest day and does not end the run.";

