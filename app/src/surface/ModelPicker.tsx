// The model a new session starts on, named on its first line, and the
// agent's own list under it. The list is the one Claude Code sent when the
// last lesson started, kept by the window: Magnesium keeps no list of models
// of its own and adds nothing to what the agent says about each one.

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import type { Core, ModelChoice, ModelOption } from "../types";

const EFFORT_WORDS: Record<string, string> = { low: "Low", medium: "Medium", high: "High", xhigh: "Extra high", max: "Max" };

/** An effort level as the window names it. A level the window does not know yet is shown as the agent names it. */
export const effortWord = (level: string) => EFFORT_WORDS[level] ?? level.charAt(0).toUpperCase() + level.slice(1);

const idOf = (row: ModelOption) => row.resolved ?? row.value;

/** The row a model shows as: a current one before the default row, the default row before an older one. */
export function rowFor(models: ModelOption[], model: string): ModelOption | null {
  const matches = models.filter((row) => idOf(row) === model || row.value === model);
  return matches.find((row) => row.current && row.value !== "default") ?? matches.find((row) => row.value === "default") ?? matches[0] ?? null;
}

/** What the start line names: the model a session started now runs on, and its effort when one was picked. */
export function startWord(choice: ModelChoice): string {
  const fallback = choice.models.find((row) => row.value === "default");
  const fallbackName = fallback ? (rowFor(choice.models, idOf(fallback)) ?? fallback).name : "Default";
  if (!choice.pick) return fallbackName;
  // Not on the list now: this session runs on the agent's default, and the pick stays as the learner left it.
  if (choice.not_offered) return `${fallbackName}, for now`;
  const name = rowFor(choice.models, choice.pick.model)?.name ?? choice.pick.name ?? choice.pick.model;
  return choice.pick.effort ? `${name} · ${effortWord(choice.pick.effort).toLowerCase()}` : name;
}

type Props = {
  api: Core;
  /** The list closed; the keyboard goes back to the line. */
  onClose: () => void;
};

/**
 * The start line's model: a quiet word among the line's tools, and the list it opens under the line.
 * Returns the two parts for the line to place.
 */
export function useModelPicker({ api, onClose }: Props): { word: ReactNode; list: ReactNode } {
  const [choice, setChoice] = useState<ModelChoice | null>(null);
  const [open, setOpen] = useState(false);
  const [older, setOlder] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const wordRef = useRef<HTMLButtonElement>(null);

  const read = useCallback(() => {
    api
      .models()
      .then(setChoice)
      .catch(() => setChoice(null));
  }, [api]);
  useEffect(read, [read]);

  const close = useCallback(
    (back: boolean) => {
      setOpen(false);
      setOlder(false);
      setProblem(null);
      if (back) onClose();
    },
    [onClose],
  );

  // The keyboard goes to the chosen row when the list opens, or its first.
  useLayoutEffect(() => {
    if (!open) return;
    const list = listRef.current;
    (list?.querySelector<HTMLElement>("[data-nav].on") ?? list?.querySelector<HTMLElement>("[data-nav]"))?.focus();
  }, [open, older]);

  // A press anywhere else closes it.
  useEffect(() => {
    if (!open) return;
    const away = (event: MouseEvent) => {
      const target = event.target as Node;
      if (listRef.current?.contains(target) || wordRef.current?.contains(target)) return;
      close(false);
    };
    document.addEventListener("mousedown", away);
    return () => document.removeEventListener("mousedown", away);
  }, [open, close]);

  if (!choice) return { word: null, list: null };

  const pickRow = choice.pick ? rowFor(choice.models, choice.pick.model) : null;
  const fallback = choice.models.find((row) => row.value === "default");
  const fallbackName = fallback ? (rowFor(choice.models, idOf(fallback)) ?? fallback).name : null;

  const pick = async (row: ModelOption) => {
    const kept = choice.pick?.effort && row.efforts?.includes(choice.pick.effort) ? choice.pick.effort : null;
    try {
      setChoice(await api.setPick(row.value, kept));
      setProblem(null);
    } catch (error) {
      setProblem(String(error).replace(/^Error: /, ""));
    }
  };
  const setEffort = async (level: string | null) => {
    if (!choice.pick) return;
    try {
      setChoice(await api.setPick(choice.pick.model, level));
      setProblem(null);
    } catch (error) {
      setProblem(String(error).replace(/^Error: /, ""));
    }
  };

  const onKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      close(true);
      return;
    }
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    const stops = [...(listRef.current?.querySelectorAll<HTMLElement>("[data-nav]") ?? [])];
    const at = stops.indexOf(document.activeElement as HTMLElement);
    const next = stops[Math.min(stops.length - 1, Math.max(0, at + (event.key === "ArrowDown" ? 1 : -1)))];
    if (next) {
      event.preventDefault();
      next.focus();
    }
  };

  const effortRow = (row: ModelOption) =>
    row === pickRow && row.efforts && row.efforts.length > 0 ? (
      <div className="model-effort" role="radiogroup" aria-label={`Effort for ${row.name}`}>
        <span className="model-effort-name">Effort</span>
        {[null, ...row.efforts].map((level) => {
          const chosen = (choice.pick?.effort ?? null) === level;
          return (
            <button
              key={level ?? "usual"}
              type="button"
              role="radio"
              aria-checked={chosen}
              className={`segment${chosen ? " chosen" : ""}`}
              data-drive="model-effort"
              data-level={level ?? "usual"}
              title={level ? undefined : "Claude Code’s own effort for this model"}
              onClick={() => void setEffort(level)}
            >
              {level ? effortWord(level) : "Usual"}
            </button>
          );
        })}
      </div>
    ) : null;

  const modelRow = (row: ModelOption) => (
    <div key={row.value}>
      <button
        type="button"
        className={`row model-row${row === pickRow ? " on" : ""}`}
        aria-pressed={row === pickRow}
        data-nav
        data-drive="model-row"
        data-value={row.value}
        onClick={() => void pick(row)}
      >
        <span className="row-mark" aria-hidden="true" />
        <span className="row-label">
          {row.name}
          {row.description && <span className="model-say">{row.description}</span>}
        </span>
      </button>
      {effortRow(row)}
    </div>
  );

  const current = choice.models.filter((row) => row.current);
  const earlier = choice.models.filter((row) => !row.current);
  const notes: string[] = [];
  if (choice.models.length === 0) {
    notes.push(
      choice.failed
        ? `Claude Code didn’t send its list of models when the last lesson started. ↩ starts on ${startWord(choice)}.`
        : `Claude Code sends its list of models when a lesson starts. ↩ starts this one on ${choice.pick ? startWord(choice) : "its default"}, and the next can be chosen here.`,
    );
  } else if (choice.not_offered && choice.pick) {
    notes.push(`${choice.pick.name ?? choice.pick.model} isn’t on Claude Code’s list now, so this session starts on ${fallbackName ?? "its default"}. Your pick stays until you choose another.`);
  }
  if (problem) notes.push(problem);

  const list = open ? (
    <div className="model-list" ref={listRef} role="dialog" aria-label="The model this session starts on" onKeyDown={onKeyDown}>
      {older ? (
        <>
          <button type="button" className="row model-back" data-nav data-drive="model-current" onClick={() => setOlder(false)}>
            <span className="row-mark" aria-hidden="true" />
            <span className="row-label">‹ Current models</span>
          </button>
          <div role="group" aria-label="Older models">
            {earlier.map(modelRow)}
          </div>
        </>
      ) : (
        <>
          {current.length > 0 && (
            <div role="group" aria-label="Models">
              {current.map(modelRow)}
            </div>
          )}
          {earlier.length > 0 && (
            <button type="button" className="row model-fold" data-nav data-drive="model-older" onClick={() => setOlder(true)}>
              <span className="row-mark" aria-hidden="true" />
              <span className="row-label">Older models</span>
              <span className="row-hint">{earlier.length} ›</span>
            </button>
          )}
        </>
      )}
      {notes.map((text) => (
        <p key={text} className="model-note" role={text === problem ? "alert" : undefined}>
          {text}
        </p>
      ))}
      {choice.models.length > 0 && !older && <p className="model-note">Claude Code’s list for your account. Your pick is kept for the next session.</p>}
    </div>
  ) : null;

  const word = (
    <button
      ref={wordRef}
      type="button"
      className={`clip${open ? " open" : ""}`}
      data-drive="model"
      aria-haspopup="dialog"
      aria-expanded={open}
      title="The model this session starts on"
      onMouseDown={(event) => event.preventDefault()}
      onClick={() => {
        if (open) close(true);
        else {
          read();
          setOpen(true);
        }
      }}
    >
      {startWord(choice)}
    </button>
  );

  return { word, list };
}
