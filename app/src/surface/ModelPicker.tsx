// The model and effort of the session in a tab, named in More's Model row on
// its line, and the agent's own list. One control for the whole session: before
// it starts it sets what it starts on; once it runs, a change goes to Claude
// Code at once and holds from its next request to the model. Either way it
// becomes the pick new sessions start on.
//
// The list is the one Claude Code last sent, kept by the window: Magnesium
// keeps no list of models of its own and adds nothing to what the agent says
// about each one. The window asks for it as it opens; a tab that finds none
// kept asks again, and so does opening the list. What a running session is on
// is asked of Claude Code each time the list opens, not remembered by the page.

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

// No-break spaces: a model and its effort are one name, never split across lines ("Opus 4.6 /· Low").
const withEffort = (name: string, effort?: string | null) => (effort ? `${name}\u00a0·\u00a0${effortWord(effort)}` : name);

/** What the start line names: the model a session started now runs on, and its effort when one was picked. */
export function startWord(choice: ModelChoice): string {
  const fallback = choice.models.find((row) => row.value === "default");
  const fallbackName = fallback ? (rowFor(choice.models, idOf(fallback)) ?? fallback).name : "Default model";
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
  /** The list closed; the keyboard goes back to the line. */
  onClose: () => void;
};

export type ModelPicker = {
  /** What the session is on, or starts on, in words: More's Model row names it. Null until the window has read the choice. */
  name: string | null;
  /** The list, for the line to place; null while it is shut. */
  list: ReactNode;
  open: boolean;
  /** Open or shut the list, as More's Model row does. */
  toggle: () => void;
};

/** The model of the session in a tab: its name for More's Model row, and the list that row opens. */
export function useModelPicker({ api, at, onClose }: Props): ModelPicker {
  const [choice, setChoice] = useState<ModelChoice | null>(null);
  const [open, setOpen] = useState(false);
  const [older, setOlder] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const [changing, setChanging] = useState(false);
  // Over the lesson's line, the list opens up unless the line sits too high for it; its height is what the room allows.
  const [room, setRoom] = useState<{ below: boolean; height: number; scrolls: boolean } | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const where = useRef(at);
  where.current = at;

  // No list kept: Claude Code is asked for it now, with no lesson started and no message sent.
  const [asking, setAsking] = useState(false);
  const asked = useRef(false);
  const read = useCallback(() => {
    api
      .sessionModels(where.current)
      .then((now) => {
        setChoice(now);
        if (now.models.length > 0 || asked.current) return;
        asked.current = true;
        setAsking(true);
        api
          .askModels()
          .then(() => api.sessionModels(where.current))
          .then(setChoice)
          .catch(() => {})
          .finally(() => setAsking(false));
      })
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
      // Opening the list asks again for one that did not come.
      if (!asking) asked.current = false;
      read();
      setOpen(true);
    }
  }, [open, close, read, asking]);

  // The keyboard goes to the chosen row when the list opens, or its first.
  useLayoutEffect(() => {
    if (!open) return;
    const list = listRef.current;
    (list?.querySelector<HTMLElement>("[data-nav].on") ?? list?.querySelector<HTMLElement>("[data-nav]"))?.focus();
  }, [open, older, choice === null, choice?.models.length]);

  useLayoutEffect(() => {
    const list = listRef.current;
    const line = list?.parentElement;
    if (!open || !list || !line) return setRoom(null);
    const box = line.getBoundingClientRect();
    const top = document.querySelector(".tabs")?.getBoundingClientRect().bottom ?? 0;
    // Clear of the step's name under the tabs by about a line.
    const above = box.top - top - 40;
    const below = window.innerHeight - box.bottom - 16;
    const wanted = Math.min(list.scrollHeight, window.innerHeight * 0.62);
    // Under a session's first line, as the line's menus open there.
    const down = line.classList.contains("first") || (above < wanted && below > above);
    // All the room on its side of the line: a list cut short at a set share of a short window hid its last rows with
    // nothing to say it scrolls (WebKit's scrollbar shows only while scrolling).
    const height = Math.max(160, down ? below : above);
    setRoom({ below: down, height, scrolls: list.scrollHeight > height + 1 });
  }, [open, older, choice]);

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
      if (listRef.current?.contains(target)) return;
      close(false);
    };
    document.addEventListener("mousedown", away);
    document.addEventListener("focusin", away);
    return () => {
      document.removeEventListener("mousedown", away);
      document.removeEventListener("focusin", away);
    };
  }, [open, close]);

  if (!choice) return { name: null, list: null, open, toggle };

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
      // Said in the list's own names first; Claude Code's reason, which may carry a model's id, after it.
      // The name asked for is kept on one line: "Opus 4.6 · Low" read wrong broken at any of its spaces.
      const asked = withEffort(rowFor(choice.models, model)?.name ?? model, effort).replace(/ /g, "\u00a0");
      const why = String(error)
        .replace(/^Error: /, "")
        .replace(/^Claude Code did not change the (model|effort):\s*/, "")
        .replace(/\.$/, "")
        // A model's id ("claude-opus-4-6") is not broken across lines at its hyphens.
        .replace(/-/g, "\u2011");
      setProblem(`Claude Code didn’t move this session to ${asked}.${why ? ` It said: ${why}.` : ""}`);
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
    const now = on ? `This session is on ${lineWord(choice)}.` : `↩ starts on ${choice.pick ? startWord(choice) : "its default"}.`;
    notes.push(asking || !choice.failed ? `Asking Claude Code for its models… ${now}` : `Claude Code didn’t send its list of models. ${now} Open this again to ask once more.`);
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
      className={`model-list${room?.below ? " below" : ""}${room?.scrolls ? " scrolls" : ""}`}
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
            <span className="row-mark" aria-hidden="true">
              ‹
            </span>
            <span className="row-label">Current models</span>
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

  return { name: lineWord(choice), list, open, toggle };
}
