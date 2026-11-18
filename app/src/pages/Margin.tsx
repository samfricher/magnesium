// The pieces Home and a course's page are set from. Both sit on the lesson's own column, and each part's name hangs in
// the margin left of it, as a lesson's › hangs left of the line, while that margin has room for it; on a narrower
// window the name moves onto a line of its own above the part.

import { Fragment, useId, useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from "react";

import { useHangs } from "../hangs";

/** The room a part's name takes in the margin: its 7rem and the 44px to the column, plus a little to the window's edge. */
const MARGIN_REM = 9.4;

/**
 * Whether the parts' names hang in the margin left of `column`. Measured rather than read from the window's width: the
 * column's left edge moves with the text size, the line width set in Settings and a document open beside the page.
 */
export function useMargin(column: RefObject<HTMLElement | null>, scroller: string, again: unknown[] = []): boolean {
  return useHangs(column, scroller, () => MARGIN_REM * (parseFloat(getComputedStyle(document.documentElement).fontSize) || 17), again);
}

/**
 * One part of the page: its name (and a sub-line under it) in the margin, its body in the column. A `group` is a day
 * of Your work: its name is smaller, and on a narrow window it leads its first row instead of standing above it.
 */
export function Part(props: { head?: ReactNode; sub?: ReactNode; group?: boolean; className?: string; label?: string; children?: ReactNode }) {
  const id = useId();
  const named = props.head !== undefined && props.head !== null;
  return (
    <section
      className={`mg-part${props.group ? " mg-group-part" : ""}${props.className ? ` ${props.className}` : ""}`}
      aria-labelledby={named && !props.label ? id : undefined}
      aria-label={props.label}
    >
      <div className="mg-head">
        {named && (
          <span id={id} className={props.group ? "mg-group" : "mg-name"}>
            {props.head}
          </span>
        )}
        {props.sub && <span className="mg-sub">{props.sub}</span>}
      </div>
      <div className="mg-body">{props.children}</div>
    </section>
  );
}

export function Chevron({ open }: { open: boolean }) {
  return (
    <svg className={`mg-chev${open ? " open" : ""}`} width="10" height="6" viewBox="0 0 10 6" aria-hidden="true">
      <path d="M1 1l4 4 4-4" fill="none" stroke="currentColor" strokeWidth="1.25" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

/** "Show all 15 sessions ⌄", opened in place; "Show fewer ⌃" once open. */
export function More({ open, more, less = "Show fewer", onToggle, drive }: { open: boolean; more: string; less?: string; onToggle: () => void; drive?: string }) {
  return (
    <button className="mg-act mg-more" data-drive={drive} aria-expanded={open} onClick={onToggle}>
      {open ? less : more}
      <Chevron open={open} />
    </button>
  );
}

/**
 * A row on the column's two edges: a name at the left, one fact at the right edge. A fact that does not fit beside
 * the name drops under it. A row that opens something is a button the arrow keys move between.
 */
export function Row(props: { name: ReactNode; end?: ReactNode; quiet?: boolean; onClick?: () => void; drive?: string; current?: boolean; className?: string; nameClass?: string }) {
  const body = (
    <>
      <span className={`mg-row-name${props.nameClass ? ` ${props.nameClass}` : ""}`}>{props.name}</span>
      {props.end !== undefined && props.end !== null && props.end !== "" && <span className="mg-row-end">{props.end}</span>}
    </>
  );
  const className = `mg-row${props.quiet ? " quiet" : ""}${props.className ? ` ${props.className}` : ""}`;
  return props.onClick ? (
    <button className={`${className} mg-hit`} data-drive={props.drive} data-row="" aria-current={props.current ? "true" : undefined} onClick={props.onClick}>
      {body}
    </button>
  ) : (
    <div className={className} role="listitem">
      {body}
    </div>
  );
}

/**
 * The cue under the step: the lead ("Intro Statistics, step 1 of 5.") with what the session starts with on the same
 * line when the two fit the column at the learner's face and size, else on a line of their own. "Last time" follows
 * only when they shared a line, so the cue is never more than two lines.
 */
export function Cue({ lead, said, last }: { lead: string; said: string | null; last: string | null }) {
  const probe = useRef<HTMLSpanElement>(null);
  const [fits, setFits] = useState(true);
  const joined = said ? `${lead} ${said}` : lead;
  useLayoutEffect(() => {
    const element = probe.current;
    const column = element?.parentElement;
    if (!element || !column) return;
    const measure = () => {
      if (column.clientWidth === 0) return;
      setFits(element.offsetWidth <= column.clientWidth);
    };
    measure();
    const watch = new ResizeObserver(measure);
    watch.observe(column);
    return () => watch.disconnect();
  }, [joined]);
  return (
    <div className="mg-cue-block">
      {/* The joined line, set unwrapped and out of sight, to measure. */}
      <span ref={probe} className="mg-cue mg-cue-probe" aria-hidden="true">
        {joined}
      </span>
      {fits || !said ? (
        <>
          <p className="mg-cue">{joined}</p>
          {last && <p className="mg-cue mg-last">{`Last time: ${last}.`}</p>}
        </>
      ) : (
        <>
          <p className="mg-cue">{lead}</p>
          <p className="mg-cue">{said}</p>
        </>
      )}
    </div>
  );
}

/** A skill the record names in capitals (POOLVAR, CLT-N): set a size down and spaced, so it does not shout. */
export const isCode = (name: string) => /^[A-Z0-9][A-Z0-9 _./-]*$/.test(name) && /[A-Z]/.test(name);

/** About three lines of the column: the codes and their commas a closed run of skills to repair shows. */
const RUN_CHARS = 168;

/**
 * The skills waiting for their repair as one running paragraph, each kept whole on its line. Past about three lines it
 * ends "and 9 more", which opens the rest in place.
 */
export function RepairRun({ names }: { names: string[] }) {
  const [open, setOpen] = useState(false);
  let fit = 0;
  for (let used = 0; fit < names.length && used + names[fit].length + 2 <= RUN_CHARS; fit++) used += names[fit].length + 2;
  fit = Math.max(1, fit);
  const shown = open ? names : names.slice(0, fit);
  const more = names.length - shown.length;
  return (
    <p className="mg-run">
      {shown.map((name, at) => (
        <Fragment key={name}>
          <span className={isCode(name) ? "mg-code" : "mg-run-name"}>{name}</span>
          {at < shown.length - 1 ? ", " : more > 0 ? " " : ""}
        </Fragment>
      ))}
      {more > 0 && <More open={false} more={`and ${more} more`} onToggle={() => setOpen(true)} drive="show-repair" />}
      {open && fit < names.length && (
        <>
          {" "}
          <More open more="" onToggle={() => setOpen(false)} drive="show-repair" />
        </>
      )}
    </p>
  );
}
