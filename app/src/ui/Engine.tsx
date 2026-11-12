// What runs a lesson, chosen with one click. The learner sees two names,
// "Claude Code" and "Magnesium", and one plain line about the one picked.
// The pick is saved at once. A conversation stays on the engine it began on,
// so a new pick takes effect with the next lesson, and the learner record
// carries the progress across.

import type { Engine, EngineChoice, Payment } from "../types";
import { Choice } from "./bits";

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
  const changes = choice.conversation !== null && choice.conversation !== choice.chosen;
  return (
    <div className={locked ? "engine locked" : "engine"} data-role="engine">
      <div className="engine-pick">
        {!props.bare && <span className="h-label">Lessons run on</span>}
        <Choice
          label="Lessons run on"
          value={choice.chosen}
          disabled={locked}
          options={ENGINES.map((engine) => ({ value: engine, label: engineName(engine) }))}
          onChange={(engine) => {
            if (!locked && engine !== choice.chosen) props.onChoose(engine);
          }}
        />
        {locked && <span className="engine-why">End the lesson to change this.</span>}
      </div>
      {/* Both lines are laid out, one over the other, and the one not picked
          is hidden, so the space they take never changes and nothing below
          moves. The same holds for the line about a new conversation. */}
      <div className="engine-lines">
        {ENGINES.map((engine) => (
          <p key={engine} className={engine === choice.chosen ? "engine-line" : "engine-line off"} aria-hidden={engine !== choice.chosen || undefined}>
            {engineLine(engine, props.payment)}
          </p>
        ))}
      </div>
      <p className={changes && !locked ? "engine-line" : "engine-line off"} aria-hidden={!(changes && !locked) || undefined}>
        Your next lesson starts a new conversation. Your progress carries over.
      </p>
    </div>
  );
}
