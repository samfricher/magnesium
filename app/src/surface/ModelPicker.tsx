import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import type { Core, ModelAt, ModelChoice, ModelOption } from "../types";

const EFFORT_WORDS: Record<string, string> = { low: "Low", medium: "Medium", high: "High", xhigh: "Extra high", max: "Max" };

export const effortWord = (level: string) => EFFORT_WORDS[level] ?? level.charAt(0).toUpperCase() + level.slice(1);

const idOf = (row: ModelOption) => row.resolved ?? row.value;

export function rowFor(models: ModelOption[], model: string): ModelOption | null {
  const matches = models.filter((row) => idOf(row) === model || row.value === model);
  return matches.find((row) => row.current && row.value !== "default") ?? matches.find((row) => row.value === "default") ?? matches[0] ?? null;
}

const withEffort = (name: string, effort?: string | null) => (effort ? `${name}\u00a0·\u00a0${effortWord(effort)}` : name);

export function startWord(choice: ModelChoice): string {
  const fallback = choice.models.find((row) => row.value === "default");
  const fallbackName = fallback ? (rowFor(choice.models, idOf(fallback)) ?? fallback).name : "Default model";
  if (!choice.pick) return fallbackName;
  if (choice.not_offered) return `${fallbackName}, for now`;
  const name = rowFor(choice.models, choice.pick.model)?.name ?? choice.pick.name ?? choice.pick.model;
  return withEffort(name, choice.pick.effort);
}

export function lineWord(choice: ModelChoice): string {
  const on = choice.on;
  if (!on) return startWord(choice);
  const name = rowFor(choice.models, on.model)?.name ?? (choice.pick?.model === on.model ? choice.pick.name : undefined) ?? on.model;
  return withEffort(name, on.effort);
}

type Why = "unavailable" | "effort" | "no_answer" | "other";
type Refused = { asked: string | null; effort: string | null; why: Why };

function refusal(error: string): Why {
  const said = error.replace(/^Error: /, "");
  if (/(did not answer in time|stopped before it answered|did not say which model)/.test(said)) return "no_answer";
  if (/^Claude Code did not change the effort|^Claude Code runs this session at /.test(said)) return "effort";
  if (/^Claude Code did not change the model|^That model cannot be picked/.test(said)) return "unavailable";
  return "other";
}

function refusedWords(problem: Refused, now: string): string {
  const keep = (name: string) => name.replace(/ /g, " ");
  const asked = problem.asked ? keep(withEffort(problem.asked, problem.why === "effort" ? null : problem.effort)) : "That model";
  switch (problem.why) {
    case "unavailable":
      return `${asked} isn’t available to you. Pick another, or stay on ${keep(now)}.`;
    case "effort":
      return `${asked} doesn’t take ${effortWord(problem.effort ?? "").toLowerCase()} effort here. This session is on ${keep(now)}.`;
    case "no_answer":
      return `Claude Code didn’t answer, so this session stays on ${keep(now)}. Try again.`;
    default:
      return `This session couldn’t move to ${asked}. It stays on ${keep(now)}.`;
  }
}

type Props = {
  api: Core;
  at: ModelAt;
  onClose: () => void;
};

export type ModelPicker = {
  name: string | null;
  list: ReactNode;
  open: boolean;
  toggle: () => void;
};

export function useModelPicker({ api, at, onClose }: Props): ModelPicker {
  const [choice, setChoice] = useState<ModelChoice | null>(null);
  const [open, setOpen] = useState(false);
  const [older, setOlder] = useState(false);
  const [problem, setProblem] = useState<Refused | null>(null);
  const [changing, setChanging] = useState(false);
  const [room, setRoom] = useState<{ below: boolean; height: number; scrolls: boolean } | null>(null);
  const [atEnd, setAtEnd] = useState(false);
  const listRef = useRef<HTMLDivElement>(null);
  const where = useRef(at);
  where.current = at;

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
      if (!asking) asked.current = false;
      read();
      setOpen(true);
    }
  }, [open, close, read, asking]);

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
    const above = box.top - top - 40;
    const below = window.innerHeight - box.bottom - 16;
    const wanted = Math.min(list.scrollHeight, window.innerHeight * 0.62);
    const down = line.classList.contains("first") || (above < wanted && below > above);
    const height = Math.max(160, down ? below : above);
    setRoom({ below: down, height, scrolls: list.scrollHeight > height + 1 });
    setAtEnd(false);
  }, [open, older, choice]);

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
      setProblem({ asked: rowFor(choice.models, model)?.name ?? null, effort, why: refusal(String(error)) });
      read();
    } finally {
      setChanging(false);
    }
  };
  const pick = (row: ModelOption) => {
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
    const name = choice.pick?.model === on.model ? choice.pick.name : null;
    notes.push(name ? `This session is on ${name}, which isn’t on Claude Code’s list now. Pick another to go on.` : "This session is on a model that isn’t on Claude Code’s list now. Pick another to go on.");
  } else if (!on && choice.not_offered && choice.pick) {
    notes.push(`${choice.pick.name ?? choice.pick.model} isn’t on Claude Code’s list now, so this session starts on ${fallbackName ?? "its default"}. Your pick stays until you choose another.`);
  }
  const refused = problem ? refusedWords(problem, lineWord(choice)) : null;
  if (refused) notes.push(refused);
  const footing = !on
    ? "Claude Code’s list for your account. Your pick is kept for the next session."
    : on.live
      ? "Claude Code’s list for your account. A change holds from the tutor’s next reply, and new sessions start on it."
      : "Claude Code’s list for your account. This session goes on with your pick when you next write, and new sessions start on it.";

  const label = on ? "The model this session runs on" : "The model this session starts on";
  const list = open ? (
    <div
      className={`model-list${room?.below ? " below" : ""}${room?.scrolls && !atEnd ? " scrolls" : ""}`}
      ref={listRef}
      onScroll={(e) => {
        const l = e.currentTarget;
        setAtEnd(l.scrollTop + l.clientHeight >= l.scrollHeight - 1);
      }}
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
        <p key={text} className="model-note" role={text === refused ? "alert" : undefined}>
          {text}
        </p>
      ))}
      {choice.models.length > 0 && !older && <p className="model-note">{footing}</p>}
    </div>
  ) : null;

  return { name: lineWord(choice), list, open, toggle };
}
