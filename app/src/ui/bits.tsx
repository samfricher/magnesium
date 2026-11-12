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

/**
 * A goal as its title, and under it, on its own quiet line, the problem it
 * came from: "Reading Vmax from a plot" and below it "PS3 Q3". The reference
 * is never joined to the title's last word, and never above it.
 */
export function GoalText({ goal }: { goal: string }) {
  const { ref, title } = splitGoal(goal);
  return (
    <>
      <Line text={title} />
      {ref && <span className="goal-ref">{ref}</span>}
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
      <Icon name={props.icon} size={props.size ?? 16} />
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

/**
 * "5 questions due today, on 2 skills": quick checks and paper redos are
 * questions, and what they prove are skills; the two nouns are said together
 * wherever either is. A screen that cannot tell the skills says the questions.
 */
export function questionsDue(n: number, skills?: number | null): string {
  const said = `${plural(n, "question", "questions")} due today`;
  return skills !== undefined && skills !== null && skills > 0 ? `${said}, on ${plural(skills, "skill", "skills")}` : said;
}

/** "2 skills, 5 questions": the same two counts, as they stand beside a heading. */
export function dueCounts(skills: number, questions: number | null): string {
  return questions !== null && questions > 0 ? `${plural(skills, "skill", "skills")}, ${plural(questions, "question", "questions")}` : plural(skills, "skill", "skills");
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

/**
 * The tally's ten strokes: how much of each is filled, and how much of that
 * was earned today. Level N fills strokes 1 to N; the stroke after them is
 * filled from its foot by the share earned towards level N+1; the rest are
 * outlines. A stroke at level 10 has nothing after it.
 */
export function levelStrokes(proofs: Proofs, gain: number): { a: number; g: number; full: boolean }[] {
  const { earned, total } = proofs;
  // Where the course stands, in strokes: the level, and the share of the way to the next. Levels are tenths of the proofs, as the runtime counts them: level n begins at the n-1th ninth.
  const at = (held: number): number => {
    if (total <= 0) return 0;
    const level = Math.min(TOP_LEVEL, 1 + Math.floor((Math.max(0, held) * (TOP_LEVEL - 1)) / total));
    if (level >= TOP_LEVEL) return TOP_LEVEL;
    const lo = Math.ceil(((level - 1) * total) / (TOP_LEVEL - 1));
    const hi = Math.ceil((level * total) / (TOP_LEVEL - 1));
    return level + (hi > lo ? Math.max(0, Math.min(1, (held - lo) / (hi - lo))) : 0);
  };
  let now = at(earned);
  if (proofs.level >= TOP_LEVEL) now = TOP_LEVEL;
  else if (proofs.to_next > 0) {
    // The runtime says how many proofs the next level is away, which is the width of this stroke.
    const lo = Math.ceil(((proofs.level - 1) * total) / (TOP_LEVEL - 1));
    const hi = earned + proofs.to_next;
    now = proofs.level + (hi > lo ? Math.max(0, Math.min(1, (earned - lo) / (hi - lo))) : 0);
  } else now = proofs.level;
  const before = gain > 0 ? Math.min(now, at(earned - gain)) : now;
  return Array.from({ length: TOP_LEVEL }, (_, k) => {
    const end = Math.max(0, Math.min(1, now - k));
    const a = Math.max(0, Math.min(1, before - k));
    return { a, g: end - a, full: end >= 1 };
  });
}

/** The tally alone: ten strokes. */
export function Tally({ proofs, size = "md", gain = 0, still = false }: { proofs: Proofs; size?: "sm" | "md" | "lg"; gain?: number; still?: boolean }) {
  return (
    <span className={`tally ${size}${gain > 0 && !still ? " grows" : ""}`} aria-hidden="true">
      {levelStrokes(proofs, gain).map((stroke, index) => (
        <i
          key={index}
          className={stroke.full ? "full" : undefined}
          style={{
            ["--a" as string]: stroke.a,
            ["--g" as string]: stroke.g,
            // Any progress at all is drawn at least as tall as the size's least (3px; for today's gain on Lesson done, 8px).
            ["--ma-on" as string]: stroke.a > 0 ? 1 : 0,
            ["--mg-on" as string]: stroke.g > 0.001 ? 1 : 0,
          }}
        />
      ))}
    </span>
  );
}

/**
 * The level in words, one sentence wherever the level is shown: "Level 5. 4
 * more proofs to reach level 6." On Lesson done, and only there, what was
 * earned today comes first: "1 proof today. Level 5. 3 more proofs to reach
 * level 6." Null when the course has no level.
 */
export function levelSaid(proofs: Proofs, gain = 0): { today: string | null; level: string; next: string | null } | null {
  if (!proofs.levelled || proofs.total === 0) return null;
  return {
    today: gain > 0 ? `${plural(gain, "proof", "proofs")} today.` : null,
    level: `Level ${proofs.level}.`,
    next: nextLevelSentence(proofs),
  };
}

/** The level sentence, as one run of words with the level itself in ink. The one component that says it. */
export function LevelSaid({ proofs, gain = 0 }: { proofs: Proofs; gain?: number }) {
  const said = levelSaid(proofs, gain);
  if (!said) return null;
  return (
    <span className="lvl-said num">
      {said.today && <span>{said.today} </span>}
      <b>{said.level}</b>
      {said.next && (
        <>
          {" "}
          <span className="next">{said.next}</span>
        </>
      )}
    </span>
  );
}

/**
 * The level: a tally of ten strokes, and the sentence that says it. With
 * `gain`, the part earned today is drawn brighter and grows once, and the
 * sentence begins with it.
 */
export function Level({
  proofs,
  size = "md",
  gain = 0,
  still = false,
  title,
}: {
  proofs: Proofs;
  size?: "sm" | "md" | "lg";
  /** How many proofs were earned today, to draw in the brighter tint and to say first. */
  gain?: number;
  /** Draw today's part at rest. Only the first sight of Lesson done grows it. */
  still?: boolean;
  title?: string;
}) {
  const said = levelSaid(proofs, gain);
  const name = said ? [said.today, said.level, said.next].filter(Boolean).join(" ") : `Level ${proofs.level}`;
  return (
    <span className={`lvl ${size}`} role="img" aria-label={name} title={title}>
      <Tally proofs={proofs} size={size} gain={gain} still={still} />
      <LevelSaid proofs={proofs} gain={gain} />
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

/** The element the window itself put focus on when a screen opened. */
let placed: Element | null = null;

/**
 * When a screen opens, focus goes to its one green button, or to its title
 * when it has none, so a keyboard starts somewhere sensible. In pointer mode
 * no ring shows (main.tsx sets data-input). It takes nothing from a control
 * the screen or the learner already focused, and it moves on from its own
 * placing when `ready` changes (a button that arrives once data has been read).
 */
function useScreenFocus(ready: unknown): void {
  useEffect(() => {
    const timer = window.setTimeout(() => {
      const active = document.activeElement;
      if (active && active !== document.body && active !== placed) return;
      if (document.querySelector(".palette-veil")) return;
      const screen = document.querySelector(".screen");
      const target = screen?.querySelector<HTMLElement>(".btn.primary:not(:disabled)") ?? screen?.querySelector<HTMLElement>("h1") ?? null;
      if (!target) return;
      if (target.tagName === "H1") target.tabIndex = -1;
      target.focus({ preventScroll: true });
      placed = target;
    }, 0);
    return () => window.clearTimeout(timer);
  }, [ready]);
}

/**
 * The keys a screen takes when nothing has focus. The window moves no focus by
 * itself on load, so a ring is never hidden and the first Tab lands on the
 * first control; until then Return does the screen's one green button, and
 * the keys that scroll a page (arrows, Space, Page Up and Down, Home, End)
 * scroll the screen's own scroll area, which is not the document and so would
 * not take them by itself.
 */
export function useReturnForPrimary(ready: unknown = true): void {
  useScreenFocus(ready);
  useEffect(() => {
    // A Return that began before this screen opened (the one that ended a lesson, say) is not for this screen's green button, and neither is one that is held down: the screen would be gone before it was seen.
    const opened = performance.now();
    const stale = (event: KeyboardEvent) => event.key === "Enter" && (event.repeat || event.timeStamp < opened + RETURN_GRACE_MS);
    const guard = (event: KeyboardEvent) => {
      if (!stale(event)) return;
      const target = event.target;
      // A button that has focus would be pressed by that Return; a text field keeps its own.
      if (target instanceof HTMLButtonElement || target === document.body) event.preventDefault();
    };
    window.addEventListener("keydown", guard, true);
    const onKey = (event: KeyboardEvent) => {
      if (stale(event)) return;
      if (event.defaultPrevented || event.isComposing || event.metaKey || event.ctrlKey || event.altKey) return;
      const focused = document.activeElement;
      const own = focused !== null && focused === placed;
      if (focused && focused !== document.body && !own) return;
      if (document.querySelector(".palette-veil")) return;
      // The control the window placed focus on does its own Return and Space; the keys that scroll still scroll.
      if (own && (event.key === "Enter" || event.key === " ")) return;
      if (event.key === "Enter") {
        if (event.shiftKey) return;
        const button = document.querySelector<HTMLButtonElement>(".screen .btn.primary:not(:disabled)");
        if (!button) return;
        event.preventDefault();
        button.click();
        return;
      }
      const area = document.querySelector<HTMLElement>(".screen > .scroll, .screen > .center");
      if (!area) return;
      const page = area.clientHeight * 0.9;
      const by: Record<string, number> = { ArrowDown: 48, ArrowUp: -48, PageDown: page, PageUp: -page, " ": event.shiftKey ? -page : page };
      if (event.key === "Home") area.scrollTo({ top: 0 });
      else if (event.key === "End") area.scrollTo({ top: area.scrollHeight });
      else if (event.key in by && !(event.shiftKey && event.key !== " ")) area.scrollBy({ top: by[event.key] });
      else return;
      event.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("keydown", guard, true);
    };
  }, []);
}

/** How long after a screen opens its Return is not taken (see useReturnForPrimary). */
const RETURN_GRACE_MS = 150;

/**
 * A fault, drawn one way on every screen: the red "!" at the left of the first
 * line, the title in the interface face at 600, what it means in the quiet
 * ink under it, and, when there is something to do, the one action below, a
 * primary button. Red is the mark and nothing else. `framed` sets it on a card
 * of its own, for a fault that stands alone on a page; inside a card that is
 * already there (a setup step, the next card) it has no frame. `detail` is
 * the title's own quiet addendum ("Magnesium  not installed").
 */
export function Fault(props: {
  title?: ReactNode;
  detail?: ReactNode;
  children?: ReactNode;
  action?: ReactNode;
  framed?: boolean;
  className?: string;
}) {
  return (
    <div className={`fault${props.framed === false ? "" : " framed"}${props.className ? ` ${props.className}` : ""}`}>
      <span className="fault-mark" aria-hidden="true">
        <Icon name="bang" size={16} />
      </span>
      <div className="fault-body selectable">
        <p className="fault-title">
          {props.title ?? "Something went wrong"}
          {props.detail ? <span className="fault-detail">{props.detail}</span> : null}
        </p>
        {props.children ? <p className="fault-what">{props.children}</p> : null}
        {props.action ? <div className="fault-act">{props.action}</div> : null}
      </div>
    </div>
  );
}

/**
 * The name of a section. One name, shown one of two ways by the width of the
 * window (screens.css): with 120px of room left of the column it hangs there
 * as the booklet's marginal note, Literata italic and quiet, level with the
 * section's first line (baseline to baseline); without it the same name stands
 * above its section as a heading, Literata at the card size and 600. A
 * heading or a button that the section already has can wear it directly with
 * the class `sec-name`; this component is the note for a section that has
 * none, and is hidden from a screen reader, which has the section's own label.
 */
export function MarginNote({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <span className={`sec-name${className ? ` ${className}` : ""}`} aria-hidden="true">
      {children}
    </span>
  );
}

/** A due thing: a small amber clock and a few words (a clock, not a dot, so it is never taken for a proof). With a number, questions are due: "5 questions due today, on 2 skills"; without, it is a skill's own row, and skills are "to prove today". `bare` is the clock alone, for a row that stands in a section which already says so. */
export function Due({ n, skills, bare }: { n?: number; skills?: number | null; bare?: boolean }) {
  const said = n === undefined ? "to prove today" : questionsDue(n, skills);
  if (bare) {
    return (
      <span className="due bare" role="img" aria-label={said} title={said}>
        <Icon name="clock" size={14} />
      </span>
    );
  }
  return (
    <span className="due">
      <Icon name="clock" size={14} />
      {said}
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
          <span key={index} className={`week-day${studied.has(dates[index]) ? " done" : ""}${isToday ? " today" : ""}${dates[index] > today ? " ahead" : ""}`}>
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
      <span className="h-label week-label">Days you studied this week</span>
      <Week days={activity.days} today={activity.today} />
      <Run days={activity.run} />
    </p>
  );
}

/** What a proof is, in one line, for a tooltip. */
export const PROOF_HINT = "A proof is one answer you gave alone, on its own day. Three proofs prove a skill.";

/** What the run is, in one line, for a tooltip. */
export const RUN_HINT = "Days on which you had an answer marked, in a row. One missed day a week is a rest day and does not end the run.";

