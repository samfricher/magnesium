// The plan, as a short agenda in the margin of today's page: the exam's date
// and what comes next, in order. It has no days on its items and no colour
// for being late: being off the plan is not a state the agenda can show.
// What moved today says so in a few words, from the plan before and after.

import { memo } from "react";

import { shortDay } from "../days";
import { Line } from "../ui/Rich";
import type { Plan } from "../types";

/** The items still to do, in the plan's order. */
export function ahead(plan: Plan) {
  return plan.items.filter((item) => item.status !== "done");
}

export const Agenda = memo(function Agenda(props: { plan: Plan | null; moved: Record<string, string>; examWas: string | null; onOpen: () => void }) {
  const { plan } = props;
  if (!plan || (!plan.title && plan.items.length === 0)) return null;
  const items = ahead(plan).slice(0, 7);
  const more = ahead(plan).length - items.length;
  return (
    <nav className="agenda" aria-label="The plan">
      <button className="agenda-head" onClick={props.onOpen} title="The plan in full (⌘P)">
        <span className="agenda-title">{plan.title ?? "The plan"}</span>
        {plan.exam_date && <span className="agenda-date">{shortDay(plan.exam_date)}</span>}
      </button>
      {props.examWas && props.examWas !== plan.exam_date && <p className="agenda-was">Moved from {shortDay(props.examWas)}</p>}
      <ol className="agenda-items">
        {items.map((item) => (
          <li key={item.id} className={item.status === "doing" ? "doing" : undefined}>
            <Line text={item.title} />
            {props.moved[item.id] && <span className="agenda-moved">{props.moved[item.id]}</span>}
          </li>
        ))}
      </ol>
      {more > 0 && <p className="agenda-more">and {more} more</p>}
    </nav>
  );
});

/** The agenda in one line, for a window too narrow for the margin. */
export function agendaLine(plan: Plan | null): string | null {
  if (!plan) return null;
  const next = ahead(plan)[0];
  const head = [plan.title, plan.exam_date ? shortDay(plan.exam_date) : null].filter(Boolean).join(", ");
  if (!head && !next) return null;
  return next ? `${head}${head ? ". " : ""}Next: ${next.title}` : head;
}

/**
 * What a turn changed, said in plain lines, from the plan before and after.
 * The tutor's words are never read for it.
 */
export function planChanges(before: Plan | null, after: Plan): { lines: string[]; moved: Record<string, string> } {
  const lines: string[] = [];
  const moved: Record<string, string> = {};
  if (!before) return { lines, moved };
  const title = after.title ?? "The exam";
  if (before.exam_date !== after.exam_date) {
    if (after.exam_date && before.exam_date) lines.push(`${title} is on ${shortDay(after.exam_date)}, was ${shortDay(before.exam_date)}.`);
    else if (after.exam_date) lines.push(`${title} is on ${shortDay(after.exam_date)}.`);
    else lines.push(`${title} has no date now.`);
  }
  const was = before.items.map((item) => item.id);
  const now = after.items.map((item) => item.id);
  const added = after.items.filter((item) => !was.includes(item.id));
  const dropped = before.items.filter((item) => !now.includes(item.id));
  const common = now.filter((id) => was.includes(id));
  const oldOrder = was.filter((id) => common.includes(id));
  const shifted = common.filter((id, i) => oldOrder[i] !== id);
  const titleOf = (id: string) => after.items.find((item) => item.id === id)?.title ?? id;
  if (shifted.length === 2) {
    const [first, second] = shifted;
    lines.push(`${titleOf(first)} now comes before ${titleOf(second)}.`);
    moved[first] = "moved up";
    moved[second] = "moved down";
  } else if (shifted.length > 0) {
    for (const id of shifted) {
      const up = common.indexOf(id) < oldOrder.indexOf(id);
      moved[id] = up ? "moved up" : "moved down";
    }
    const ups = shifted.filter((id) => moved[id] === "moved up").map(titleOf);
    if (ups.length > 0) lines.push(`Moved up: ${ups.join(", ")}.`);
  }
  for (const item of added) {
    moved[item.id] = "new";
    lines.push(`Added: ${item.title}.`);
  }
  if (dropped.length > 0) lines.push(`Taken off: ${dropped.map((item) => item.title).join(", ")}.`);
  if (before.goal !== after.goal && after.goal) lines.push(`Working on: ${after.goal}.`);
  const steps = (plan: Plan) => plan.steps.map((s) => s.step).join("\n");
  if (steps(before) !== steps(after)) lines.push("The steps for this goal changed.");
  if (lines.length === 0 && before.text !== after.text) lines.push("The plan's notes changed.");
  return { lines, moved };
}
