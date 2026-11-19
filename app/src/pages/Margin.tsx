import { Fragment, useId, useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from "react";

import { useHangs } from "../hangs";

const MARGIN_REM = 9.4;

export function useMargin(column: RefObject<HTMLElement | null>, scroller: string, again: unknown[] = []): boolean {
  return useHangs(column, scroller, () => MARGIN_REM * (parseFloat(getComputedStyle(document.documentElement).fontSize) || 17), again);
}

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

export function More({ open, more, less = "Show fewer", onToggle, drive }: { open: boolean; more: string; less?: string; onToggle: () => void; drive?: string }) {
  return (
    <button className="mg-act mg-more" data-drive={drive} aria-expanded={open} onClick={onToggle}>
      {open ? less : more}
      <Chevron open={open} />
    </button>
  );
}

export function Row(props: { name: ReactNode; end?: ReactNode; quiet?: boolean; onClick?: () => void; drive?: string; current?: boolean; className?: string; nameClass?: string }) {
  const body = (
    <>
      <span className={`mg-row-name${props.nameClass ? ` ${props.nameClass}` : ""}`}>{props.name}</span>
      {props.end !== undefined && props.end !== null && props.end !== "" && <span className="mg-row-end">{props.end}</span>}
    </>
  );
  const className = `mg-row${props.quiet ? " quiet" : ""}${props.className ? ` ${props.className}` : ""}`;
  return props.onClick ? (
    <div role="listitem">
      <button className={`${className} mg-hit`} data-drive={props.drive} data-row="" aria-current={props.current ? "true" : undefined} onClick={props.onClick}>
        {body}
      </button>
    </div>
  ) : (
    <div className={className} role="listitem">
      {body}
    </div>
  );
}

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

export const isCode = (name: string) => /^[A-Z0-9][A-Z0-9 _./-]*$/.test(name) && /[A-Z]/.test(name);

const RUN_CHARS = 168;

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
          {at < shown.length - 1 ? ", " : ""}
        </Fragment>
      ))}
      {fit < names.length && (
        <>
          {" "}
          <More open={open} more={`and ${more} more`} onToggle={() => setOpen((was) => !was)} drive="show-repair" />
        </>
      )}
    </p>
  );
}
