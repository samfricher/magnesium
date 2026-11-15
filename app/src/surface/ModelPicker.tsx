// The model and effort of the session in a tab, named on its line, and the
// agent's own list under the word. One control for the whole session: before
// it starts it sets what it starts on; once it runs, a change goes to Claude
// Code at once and holds from its next request to the model. Either way it
// becomes the pick new sessions start on.
//
// The list is the one Claude Code sent when the last lesson started, kept by
// the window: Magnesium keeps no list of models of its own and adds nothing to
// what the agent says about each one. What a running session is on is asked of
// Claude Code each time the list opens, not remembered by the page.

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import type { Core, ModelAt, ModelChoice, ModelOption } from "../types";

const EFFORT_WORDS: Record<string, string> = { low: "Low", medium: "Medium", high: "High", xhigh: "Extra high", max: "Max" };

/** An effort level as the window names it. A level the window does not know yet is shown as the agent names it. */
export const effortWord = (level: string) => EFFORT_WORDS[level] ?? level.charAt(0).toUpperCase() + level.slice(1);

const idOf = (row: ModelOption) => row.resolved ?? row.value;

/** The row a model shows as: a current one before the default row, the default row before an older one. */
export function rowFor(models: ModelOption[], model: string): ModelOption | null {
  const matches = models.filter((row) => idOf(row) === model || row.value === model);
  return matches.find((row) => row.current && row.value !== "default") ?? matches.find((row) => row.value === "default") ?? matches[0] ?? null;
}

const withEffort = (name: string, effort?: string | null) => (effort ? `${name} · ${effortWord(effort).toLowerCase()}` : name);

/** What the start line names: the model a session started now runs on, and its effort when one was picked. */
export function startWord(choice: ModelChoice): string {
  const fallback = choice.models.find((row) => row.value === "default");
  const fallbackName = fallback ? (rowFor(choice.models, idOf(fallback)) ?? fallback).name : "Default";
  if (!choice.pick) return fallbackName;
  // Not on the list now: this session runs on the agent's default, and the pick stays as the learner left it.
  if (choice.not_offered) return `${fallbackName}, for now`;
  const name = rowFor(choice.models, choice.pick.model)?.name ?? choice.pick.name ?? choice.pick.model;
  return withEffort(name, choice.pick.effort);
}

/** What the line names: what the session is on once it has one, else what it starts on. */
export function lineWord(choice: ModelChoice): string {
  const on = choice.on;
  if (!on) return startWord(choice);
  const name = rowFor(choice.models, on.model)?.name ?? (choice.pick?.model === on.model ? choice.pick.name : undefined) ?? on.model;
  return withEffort(name, on.effort);
}

type Props = {
  api: Core;
  /** The session's tab. */
  at: ModelAt;
  /** Which line the word sits on: the start line opens the list under it, the lesson's line over it. */
  place: "rest" | "lesson";
  /** The list closed; the keyboard goes back to the line. */
  onClose: () => void;
};

export type ModelPicker = {
  /** The word, for the line's tools. Null until the window has read the choice. */
  word: ReactNode;
  /** The list, for the line to place; null while it is shut. */
  list: ReactNode;
  open: boolean;
  /** Open or shut the list, as pressing the word does. */
  toggle: () => void;
};

/** The model of the session in a tab: a quiet word among the line's tools, and the list it opens. */
export function useModelPicker({ api, at, place, onClose }: Props): ModelPicker {
  const [choice, setChoice] = useState<ModelChoice | null>(null);
  const [open, setOpen] = useState(false);
  const [older, setOlder] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const [changing, setChanging] = useState(false);
  // Over the lesson's line, the list opens up unless the line sits too high for it; its height is what the room allows.
  const [room, setRoom] = useState<{ below: boolean; height: number } | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const wordRef = useRef<HTMLButtonElement>(null);
  const where = useRef(at);
  where.current = at;

  const read = useCallback(() => {
    api
      .sessionModels(where.current)
      .then(setChoice)
      .catch(() => setChoice(null));
  }, [api]);
  // Read again when the tab's session gets its id, or starts or stops running here.
  useEffect(read, [read, at.folder, at.session, at.live]);

  const close = useCallback(
    (back: boolean) => {
      setOpen(false);
      setOlder(false);
      setProblem(null);
      if (back) onClose();
    },
    [onClose],
  );

  const toggle = useCallback(() => {
    if (open) close(true);
    else {
      read();
      setOpen(true);
    }
  }, [open, close, read]);

  // The keyboard goes to the chosen row when the list opens, or its first.
  useLayoutEffect(() => {
    if (!open) return;
    const list = listRef.current;
    (list?.querySelector<HTMLElement>("[data-nav].on") ?? list?.querySelector<HTMLElement>("[data-nav]"))?.focus();
  }, [open, older, choice === null]);

  useLayoutEffect(() => {
    const list = listRef.current;
    const line = list?.parentElement;
    if (!open || place !== "lesson" || !list || !line) return setRoom(null);
    const box = line.getBoundingClientRect();
    const top = document.querySelector(".tabs")?.getBoundingClientRect().bottom ?? 0;
    const above = box.top - top - 16;
    const below = window.innerHeight - box.bottom - 16;
    const wanted = Math.min(list.scrollHeight, window.innerHeight * 0.62);
    const down = above < wanted && below > above;
    setRoom({ below: down, height: Math.max(160, Math.min(window.innerHeight * 0.62, down ? below : above)) });
  }, [open, older, place, choice]);

  // Esc closes it wherever the keyboard is. A list with no rows yet (before
  // the first lesson, or when the agent sent none) has nothing to move the
  // keyboard to, so it stays on the line or the word, outside the list.
  useEffect(() => {
    if (!open) return;
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      close(true);
    };
    document.addEventListener("keydown", escape, true);
    return () => document.removeEventListener("keydown", escape, true);
  }, [open, close]);

  // A press anywhere else closes it, and so does the keyboard moving elsewhere (Tab, or the line taking the focus):
  // left open over the lesson's line, it would sit over the words being typed.
  useEffect(() => {
    if (!open) return;
    const away = (event: Event) => {
      const target = event.target as Node;
      if (listRef.current?.contains(target) || wordRef.current?.contains(target)) return;
      close(false);
    };
    document.addEventListener("mousedown", away);
    document.addEventListener("focusin", away);
    return () => {
      document.removeEventListener("mousedown", away);
      document.removeEventListener("focusin", away);
    };
  }, [open, close]);

  if (!choice) return { word: null, list: null, open, toggle };

  const on = choice.on ?? null;
  const chosenModel = on?.model ?? choice.pick?.model ?? null;
  const chosenEffort = on ? (on.effort ?? null) : (choice.pick?.effort ?? null);
  const chosenRow = chosenModel ? rowFor(choice.models, chosenModel) : null;
  const fallback = choice.models.find((row) => row.value === "default");
  const fallbackName = fallback ? (rowFor(choice.models, idOf(fallback)) ?? fallback).name : null;

  const change = async (model: string, effort: string | null) => {
    if (changing) return;
    setChanging(true);
    try {
      setChoice(await api.setSessionModel(where.current, model, effort));
      setProblem(null);
    } catch (error) {
      setProblem(String(error).replace(/^Error: /, ""));
      // What the session is on after a refusal is Claude Code's to say.
      read();
    } finally {
      setChanging(false);
    }
  };
  const pick = (row: ModelOption) => {
    // An effort carries over to a model that takes it.
    const kept = chosenEffort && row.efforts?.includes(chosenEffort) ? chosenEffort : null;
    void change(row.value, kept);
  };
  const setEffort = (level: string | null) => {
    if (chosenModel) void change(chosenModel, level);
  };

  const onKeyDown = (event: React.KeyboardEvent) => {
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
    row === chosenRow && row.efforts && row.efforts.length > 0 ? (
      <div className="model-effort" role="radiogroup" aria-label={`Effort for ${row.name}`}>
        <span className="model-effort-name">Effort</span>
        {[null, ...row.efforts].map((level) => {
          const chosen = chosenEffort === level;
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
              onClick={() => setEffort(level)}
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
        className={`row model-row${row === chosenRow ? " on" : ""}`}
        aria-pressed={row === chosenRow}
        data-nav
        data-drive="model-row"
        data-value={row.value}
        onClick={() => pick(row)}
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
      on
        ? `Claude Code sends its list of models when a lesson starts. This session is on ${lineWord(choice)}.`
        : choice.failed
          ? `Claude Code didn’t send its list of models when the last lesson started. ↩ starts on ${startWord(choice)}.`
          : `Claude Code sends its list of models when a lesson starts. ↩ starts this one on ${choice.pick ? startWord(choice) : "its default"}, and the next can be chosen here.`,
    );
  } else if (on && !chosenRow) {
    notes.push(`This session is on ${on.model}, which isn’t on Claude Code’s list now. Pick another to go on with that.`);
  } else if (!on && choice.not_offered && choice.pick) {
    notes.push(`${choice.pick.name ?? choice.pick.model} isn’t on Claude Code’s list now, so this session starts on ${fallbackName ?? "its default"}. Your pick stays until you choose another.`);
  }
  if (problem) notes.push(problem);
  const footing = !on
    ? "Claude Code’s list for your account. Your pick is kept for the next session."
    : on.live
      ? "Claude Code’s list for your account. A change holds from the tutor’s next reply, and new sessions start on it."
      : "Claude Code’s list for your account. This session goes on with your pick when you next write, and new sessions start on it.";

  const label = on ? "The model this session runs on" : "The model this session starts on";
  const list = open ? (
    <div
      className={`model-list ${place}${room?.below ? " below" : ""}`}
      ref={listRef}
      style={room ? { maxHeight: room.height } : undefined}
      role="dialog"
      aria-label={label}
      aria-busy={changing}
      onKeyDown={onKeyDown}
    >
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
      {choice.models.length > 0 && !older && <p className="model-note">{footing}</p>}
    </div>
  ) : null;

  const word = (
    <button
      key="model"
      ref={wordRef}
      type="button"
      className={`${place === "rest" ? "clip" : "quiet-act"} model-word${open ? " open" : ""}`}
      data-drive="model"
      aria-haspopup="dialog"
      aria-expanded={open}
      title={label}
      onMouseDown={(event) => event.preventDefault()}
      onClick={toggle}
    >
      {lineWord(choice)}
    </button>
  );

  return { word, list, open, toggle };
}
