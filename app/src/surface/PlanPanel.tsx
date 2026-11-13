// The plan beside the lesson, from the logbook concept's agenda (#69): the
// exam's date, the plan's items in order as a task list with the one in hand
// marked, and this session's steps under them. What moved in this sitting says
// so on its row, read from the plan before and after, never from the tutor's
// words. It shows no file, no path and no Markdown: the plan is changed by
// talking to the tutor. Hidden, the lesson is the surface alone.

import { memo } from "react";

import type { Plan } from "../types";
import { shortDay, type PlanDiff } from "../words";
import { Line } from "./Rich";

const MOVED_WORDS: Record<string, string> = { up: "moved up", down: "moved down", new: "new", day: "day moved" };

export const PlanPanel = memo(function PlanPanel({ plan, changed, id }: { plan: Plan | null; changed: PlanDiff | null; id?: string }) {
  // The item in hand: the one the plan marks doing, else the first not done.
  const current = plan ? (plan.items.find((item) => item.status === "doing")?.id ?? plan.items.find((item) => item.status !== "done")?.id) : undefined;
  const steps = plan?.goal && plan.steps.length > 0 ? plan.steps : [];
  const empty = plan !== null && plan.items.length === 0 && !plan.exam_date && steps.length === 0;
  return (
    // A sidebar beside the lesson, not a dialog: no focus is moved or trapped, and it holds nothing to operate.
    <aside className="panel" id={id} aria-label="The plan" data-role="plan-panel">
      {plan === null ? (
        <p className="panel-note">Reading the plan…</p>
      ) : (
        <>
          {(plan.title || plan.exam_date) && (
            <header className="panel-head">
              <span className="panel-title">
                <Line text={plan.title ?? "The plan"} />
              </span>
              {plan.exam_date && <span className="panel-date">{shortDay(plan.exam_date)}</span>}
            </header>
          )}
          {changed?.examFrom && changed.examFrom !== plan.exam_date && (
            <p className="panel-was changed" data-role="plan-moved">
              moved from {shortDay(changed.examFrom)}
            </p>
          )}
          {plan.items.length > 0 && (
            <ol className="panel-items" aria-label="What to work on">
              {plan.items.map((item) => {
                const moved = changed?.moved[item.id];
                const here = item.id === current;
                return (
                  <li
                    key={item.id}
                    className={`${item.status}${here ? " current" : ""}${moved ? " changed" : ""}`}
                    aria-current={here ? "step" : undefined}
                    data-drive={`plan-${item.id}`}
                  >
                    <span className="panel-mark" aria-hidden="true">
                      {item.status === "done" ? "✓" : here ? "●" : "○"}
                    </span>
                    <span className="panel-item">
                      <Line text={item.title} />
                      {(moved || item.day) && (
                        <span className="panel-day">
                          {[moved ? MOVED_WORDS[moved] : null, item.day ? shortDay(item.day) : null].filter(Boolean).join(", ")}
                        </span>
                      )}
                    </span>
                  </li>
                );
              })}
            </ol>
          )}
          {steps.length > 0 && (
            <>
              <p className="panel-sub">
                <Line text={plan.goal!} />
              </p>
              <ol className="panel-items steps" aria-label="This session's steps">
                {steps.map((step, at) => (
                  <li key={at} className={`${step.status}${step.status === "doing" ? " current" : ""}`} aria-current={step.status === "doing" ? "step" : undefined}>
                    <span className="panel-mark" aria-hidden="true">
                      {step.status === "done" ? "✓" : step.status === "doing" ? "●" : "○"}
                    </span>
                    <span className="panel-item">
                      <Line text={step.step} />
                    </span>
                  </li>
                ))}
              </ol>
            </>
          )}
          {empty && <p className="panel-note">No plan yet. Tell the tutor what this course is for, and it writes one.</p>}
        </>
      )}
    </aside>
  );
});
