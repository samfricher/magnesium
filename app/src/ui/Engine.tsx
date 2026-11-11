// What runs a lesson, chosen with one click. The learner sees two names,
// "Claude Code" and "Magnesium", and one plain line about the one picked.
// The pick is saved at once. A conversation stays on the engine it began on,
// so a new pick takes effect with the next lesson, and the learner record
// carries the progress across.

import type { KeyboardEvent } from "react";

import type { Engine, EngineChoice, Payment } from "../types";

const ENGINES: Engine[] = ["plugin", "direct"];

/** The name the learner sees for an engine. */
export function engineName(engine: Engine): string {
  return engine === "direct" ? "Magnesium" : "Claude Code";
}

/** One plain line about an engine. */
function engineLine(engine: Engine, payment: Payment | null): string {
  const paid = payment === "api_key" ? "Both are paid the same way." : "Both use your Claude plan.";
  return engine === "direct"
    ? `New: Magnesium runs the lesson itself, so replies start sooner. ${paid}`
    : `Claude Code runs the lesson with Magnesium's plugin, as in a terminal. ${paid}`;
}

export function EngineControl(props: {
  choice: EngineChoice;
  payment: Payment | null;
  /** A lesson is open, so the pick cannot change now. */
  locked: boolean;
  onChoose: (engine: Engine) => void;
  /** Leave out the label, where the screen gives one. */
  bare?: boolean;
}) {
  const { choice, locked } = props;
  const pick = (engine: Engine) => {
    if (!locked && engine !== choice.chosen) props.onChoose(engine);
  };
  // Arrow keys move the pick, as in any group of radio buttons.
  const onKey = (event: KeyboardEvent) => {
    const by = event.key === "ArrowRight" || event.key === "ArrowDown" ? 1 : event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 0;
    if (by === 0 || locked) return;
    event.preventDefault();
    event.stopPropagation();
    const next = ENGINES[(ENGINES.indexOf(choice.chosen) + by + ENGINES.length) % ENGINES.length];
    pick(next);
    const group = event.currentTarget as HTMLElement;
    window.setTimeout(() => group.querySelector<HTMLButtonElement>('[aria-checked="true"]')?.focus(), 0);
  };
  const changes = choice.conversation !== null && choice.conversation !== choice.chosen;
  return (
    <div className={locked ? "engine locked" : "engine"} data-role="engine">
      <div className="engine-pick">
        {!props.bare && <span className="engine-label">Lessons run on</span>}
        <div className="segmented" role="radiogroup" aria-label="Lessons run on" aria-disabled={locked || undefined} onKeyDown={onKey}>
          {ENGINES.map((engine) => (
            <button
              key={engine}
              role="radio"
              aria-checked={choice.chosen === engine}
              tabIndex={choice.chosen === engine ? 0 : -1}
              className={choice.chosen === engine ? "on" : undefined}
              disabled={locked}
              onClick={() => pick(engine)}
            >
              {engineName(engine)}
            </button>
          ))}
        </div>
        {locked && <span className="engine-why">End the lesson to change this.</span>}
      </div>
      <p className="engine-line">{engineLine(choice.chosen, props.payment)}</p>
      {changes && !locked && <p className="engine-line">Your next lesson starts a new conversation. Your progress carries over.</p>}
    </div>
  );
}
