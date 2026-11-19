import { memo } from "react";

import type { Plan } from "../types";
import { shortDay, type PlanDiff } from "../words";
import { Line } from "./Rich";

const MOVED_WORDS: Record<string, string> = { up: "moved up", down: "moved down", new: "new", day: "day moved" };

function Steps({ steps }: { steps: Plan["steps"] }) {
  return (
    <ol className="panel-items steps" aria-label="This session's steps">
      {steps.map((step, at) => (
        <li key={at} className={`${step.status}${step.status === "doing" ? " current" : ""}`} aria-current={step.status === "doing" ? "step" : undefined}>
          <span className="panel-mark" aria-hidden="true">
            {step.status === "done" ? "✓" : ""}
          </span>
          <span className="panel-item">
            <Line text={step.step} />
          </span>
        </li>
      ))}
    </ol>
  );
}

export const PlanPanel = memo(function PlanPanel({ plan, changed }: { plan: Plan | null; changed: PlanDiff | null }) {
  const current = plan ? (plan.items.find((item) => item.status === "doing")?.id ?? plan.items.find((item) => item.status !== "done")?.id) : undefined;
  const steps = plan?.goal && plan.steps.length > 0 ? plan.steps : [];
  const empty = plan !== null && plan.items.length === 0 && !plan.exam_date && steps.length === 0;
  return (
    <div className="plan-view" aria-label="The plan" data-role="plan-panel">
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
                    <span className={here && item.status !== "done" ? "panel-mark here" : "panel-mark"} aria-hidden="true">
                      {item.status === "done" ? "✓" : here ? "●" : ""}
                    </span>
                    <span className="panel-item">
                      <Line text={item.title} />
                      {(moved || item.day) && (
                        <span className="panel-day">
                          {[moved ? MOVED_WORDS[moved] : null, item.day ? shortDay(item.day) : null].filter(Boolean).join(", ")}
                        </span>
                      )}
                      {here && steps.length > 0 && <Steps steps={steps} />}
                    </span>
                  </li>
                );
              })}
            </ol>
          )}
          {steps.length > 0 && (plan.items.length === 0 || current === undefined) && <Steps steps={steps} />}
          {empty && <p className="panel-note">No plan yet. Tell the tutor what this space is for, and it writes one.</p>}
        </>
      )}
    </div>
  );
});
