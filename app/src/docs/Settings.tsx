// Settings are a document: a plain-text file the window applies as it is
// typed, the way an editor treats its own settings. How lessons run and are
// paid for are the core's settings, and sit under the file as choices.

import { useEffect, useRef } from "react";

import type { About, Engine, EngineChoice, Payment, Setup } from "../types";

type Choice<T> = { id: T; name: string };

function Row<T extends string | number>(props: { label: string; choices: Choice<T>[]; value: T | null; onPick: (value: T) => void; disabled?: boolean; note?: string }) {
  return (
    <div className="setting" role="radiogroup" aria-label={props.label}>
      <span className="setting-label">{props.label}</span>
      <span className="setting-choices">
        {props.choices.map((choice) => (
          <button
            key={String(choice.id)}
            type="button"
            role="radio"
            aria-checked={props.value === choice.id}
            className={props.value === choice.id ? "chosen" : undefined}
            disabled={props.disabled}
            onClick={() => props.onPick(choice.id)}
          >
            {choice.name}
          </button>
        ))}
      </span>
      {props.note && <span className="setting-note">{props.note}</span>}
    </div>
  );
}

export type SettingsProps = {
  text: string;
  problems: string[];
  onText: (text: string) => void;
  active: boolean;
  engine: EngineChoice | null;
  lessonOpen: boolean;
  onEngine: (engine: Engine) => void;
  setup: Setup | null;
  onPayment: (payment: Payment) => void;
  onSignIn: () => void;
  blockMinutes: number | null;
  onBlock: (minutes: number) => void;
  about: About | null;
};

export function SettingsDoc(props: SettingsProps) {
  const area = useRef<HTMLTextAreaElement>(null);
  const timer = useRef<number | null>(null);

  // The file takes the keyboard when the document opens.
  useEffect(() => {
    if (props.active) area.current?.focus({ preventScroll: true });
  }, [props.active]);

  // A change made elsewhere (⌘K, ⌘=) shows in the file at once.
  useEffect(() => {
    const el = area.current;
    if (el && el.value !== props.text && document.activeElement !== el) el.value = props.text;
  }, [props.text]);

  const fit = (el: HTMLTextAreaElement) => {
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight + 2}px`;
  };
  useEffect(() => {
    if (area.current) fit(area.current);
  }, [props.text]);

  // ↑ ↓ and ← → move through the choices under the file.
  const onKeyDown = (event: React.KeyboardEvent) => {
    const button = (event.target as HTMLElement).closest<HTMLButtonElement>('[role="radio"]');
    if (!button) return;
    const row = button.closest('[role="radiogroup"]');
    const all = Array.from(document.querySelectorAll<HTMLElement>('.settings [role="radiogroup"]'));
    if (!row) return;
    const buttons = Array.from(row.querySelectorAll<HTMLButtonElement>('[role="radio"]:not(:disabled)'));
    const at = buttons.indexOf(button);
    if (event.key === "ArrowRight" || event.key === "ArrowLeft") {
      event.preventDefault();
      const next = buttons[(at + (event.key === "ArrowRight" ? 1 : -1) + buttons.length) % buttons.length];
      next?.click();
      next?.focus();
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const target = all[all.indexOf(row as HTMLElement) + (event.key === "ArrowDown" ? 1 : -1)];
      (target?.querySelector<HTMLButtonElement>('[aria-checked="true"]') ?? target?.querySelector<HTMLButtonElement>('[role="radio"]'))?.focus();
    }
  };

  const account = props.setup?.account ?? null;
  return (
    <article className="settings" data-doc="settings" onKeyDown={onKeyDown}>
      <h1 className="doc-title">Settings</h1>
      <p className="doc-path">Your settings file. The window follows each change as you type; ⌘K also reaches every value.</p>
      <textarea
        ref={area}
        className="settings-file"
        defaultValue={props.text}
        spellCheck={false}
        aria-label="Your settings file"
        data-drive="settings-file"
        onInput={(event) => {
          const el = event.currentTarget;
          fit(el);
          if (timer.current !== null) window.clearTimeout(timer.current);
          timer.current = window.setTimeout(() => props.onText(el.value), 200);
        }}
      />
      {props.problems.length > 0 && (
        <ul className="settings-problems">
          {props.problems.map((problem) => (
            <li key={problem}>{problem}</li>
          ))}
        </ul>
      )}
      <section>
        <h2>Lessons</h2>
        <Row
          label="Lessons run on"
          choices={[
            { id: "plugin" as Engine, name: "Claude Code" },
            { id: "direct" as Engine, name: "Magnesium" },
          ]}
          value={props.engine?.chosen ?? null}
          disabled={!props.engine || props.lessonOpen}
          onPick={props.onEngine}
          note={props.lessonOpen ? "Close the lesson to change this" : props.engine?.conversation && props.engine.conversation !== props.engine.chosen ? "The next lesson starts a new conversation" : undefined}
        />
        <Row
          label="Paid with"
          choices={[
            { id: "plan" as Payment, name: "Your Claude plan" },
            { id: "api_key" as Payment, name: "An API key" },
          ]}
          value={props.setup?.payment ?? null}
          disabled={!props.setup}
          onPick={props.onPayment}
          note={account?.problem ?? undefined}
        />
        <Row
          label="A paper's length"
          choices={[25, 50, 90].map((m) => ({ id: m, name: `${m} min` }))}
          value={props.blockMinutes}
          disabled={props.blockMinutes === null}
          onPick={props.onBlock}
          note="When the plan gives none"
        />
        <div className="setting">
          <span className="setting-label">Claude Code</span>
          <span className="setting-value">
            {!account ? "Checking…" : !account.installed ? "Not installed on this Mac" : account.signed_in ? `Signed in${account.detail ? `, ${account.detail}` : ""}` : "Signed out"}
            {account && account.installed && !account.signed_in && (
              <button type="button" className="text-button" onClick={props.onSignIn}>
                Sign in
              </button>
            )}
          </span>
        </div>
      </section>
      {props.about && (
        <p className="doc-path">
          Magnesium {props.about.version.split("+")[0]}
          {props.about.commit ? `, built from ${props.about.commit.slice(0, 12)}` : ""}.
        </p>
      )}
    </article>
  );
}
