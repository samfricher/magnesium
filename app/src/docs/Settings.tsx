// Settings are a document, opened in a tab with ⌘, the way an editor opens
// its own. Each line is one choice; ↑ ↓ move between lines and ← → choose.
// The learner's choices of face, size, width, scheme and ink are kept on
// this Mac; how lessons run and are paid for are the core's own settings.

import { useRef } from "react";

import { FONTS, INKS, MEASURES, SCHEMES, SIZE_MAX, SIZE_MIN, type Prefs } from "../prefs";
import type { About, EngineChoice, Engine, Payment, Setup } from "../types";

export const KEYS: [string, string][] = [
  ["Return", "Hand in your answer, or send what you wrote. On a stopped lesson, continue"],
  ["⇧Return", "A new line"],
  ["Esc", "Leave the task and say something else; close a list"],
  ["Tab, ⌘↑", "Back into the open task"],
  ["⌘⇧H  ⌘⇧L  ⌘⇧D", "Hint, I'm lost, I don't know"],
  ["⌥1  ⌥2  ⌥3", "Sure, unsure, guess, on your answer"],
  ["⌘⇧A", "Photos or a PDF of your working"],
  ["⌘⇧M", "Maths symbols"],
  ["⌘.", "Stop the reply"],
  ["⌘P", "Open any document"],
  ["⌘K", "Every command"],
  ["⌘O", "Another study folder"],
  ["⌘N", "New lesson"],
  ["⌘W", "Close the tab. Closing the lesson keeps where you stopped"],
  ["⌘1 to ⌘9, ⌃Tab", "Go to a tab"],
  ["⌘\\", "Two panes, or one; ⌘⌥← ⌘⌥→ move between them"],
  ["⌘B", "The folder's files"],
  ["⌘E, ⌘S", "Edit the plan as text, and save it"],
  ["⌘,", "Settings"],
  ["⌘=  ⌘−  ⌘0", "Text size"],
];

type Choice<T> = { id: T; name: string; style?: React.CSSProperties };

function Row<T extends string | number>(props: { label: string; choices: Choice<T>[]; value: T | null; onPick: (value: T) => void; disabled?: boolean; note?: string }) {
  return (
    <div className="setting" role="radiogroup" aria-label={props.label} data-setting={props.label}>
      <span className="setting-label">{props.label}</span>
      <span className="setting-choices">
        {props.choices.map((choice) => (
          <button
            key={String(choice.id)}
            type="button"
            role="radio"
            aria-checked={props.value === choice.id}
            tabIndex={props.value === choice.id || (props.value === null && choice === props.choices[0]) ? 0 : -1}
            className={props.value === choice.id ? "chosen" : undefined}
            style={choice.style}
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
  prefs: Prefs;
  setPrefs: (prefs: Prefs) => void;
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
  const { prefs, setPrefs } = props;
  const root = useRef<HTMLElement>(null);

  // ↑ ↓ between lines, ← → along a line, choosing as it goes.
  const onKeyDown = (event: React.KeyboardEvent) => {
    const button = (event.target as HTMLElement).closest<HTMLButtonElement>('[role="radio"]');
    if (!button || !root.current) return;
    const row = button.closest('[role="radiogroup"]');
    if (!row) return;
    const buttons = Array.from(row.querySelectorAll<HTMLButtonElement>('[role="radio"]:not(:disabled)'));
    const rows = Array.from(root.current.querySelectorAll('[role="radiogroup"]'));
    const at = buttons.indexOf(button);
    if (event.key === "ArrowRight" || event.key === "ArrowLeft") {
      event.preventDefault();
      const next = buttons[(at + (event.key === "ArrowRight" ? 1 : -1) + buttons.length) % buttons.length];
      next?.click();
      next?.focus();
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const r = rows.indexOf(row);
      const target = rows[r + (event.key === "ArrowDown" ? 1 : -1)];
      (target?.querySelector<HTMLButtonElement>('[aria-checked="true"]') ?? target?.querySelector<HTMLButtonElement>('[role="radio"]'))?.focus();
    }
  };

  const sizes: Choice<number>[] = [];
  for (let s = SIZE_MIN; s <= SIZE_MAX; s += 1) sizes.push({ id: s, name: String(s) });
  const account = props.setup?.account ?? null;
  return (
    <article className="settings" data-doc="settings" ref={root} onKeyDown={onKeyDown}>
      <h1 className="doc-title">Settings</h1>
      <p className="doc-path">Kept on this Mac. ↑ ↓ move between lines, ← → choose.</p>
      <section>
        <h2>Reading</h2>
        <Row
          label="Text"
          choices={FONTS.map((font) => ({ id: font.id, name: font.name, style: { fontFamily: font.stack } }))}
          value={prefs.font}
          onPick={(font) => setPrefs({ ...prefs, font })}
        />
        <Row label="Size" choices={sizes} value={prefs.size} onPick={(size) => setPrefs({ ...prefs, size })} />
        <Row label="Width" choices={MEASURES} value={prefs.measure} onPick={(measure) => setPrefs({ ...prefs, measure })} />
        <Row label="Scheme" choices={SCHEMES} value={prefs.scheme} onPick={(scheme) => setPrefs({ ...prefs, scheme })} />
        <Row
          label="Your ink"
          choices={INKS.map((ink) => ({ id: ink.id, name: ink.name }))}
          value={prefs.ink}
          onPick={(ink) => setPrefs({ ...prefs, ink })}
          note="The colour of what you write"
        />
      </section>
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
            {!account
              ? "Checking…"
              : !account.installed
                ? "Not installed on this Mac"
                : account.signed_in
                  ? `Signed in${account.detail ? `, ${account.detail}` : ""}`
                  : "Signed out"}
            {account && account.installed && !account.signed_in && (
              <button type="button" className="text-button" onClick={props.onSignIn}>
                Sign in
              </button>
            )}
          </span>
        </div>
      </section>
      <section>
        <h2>Keys</h2>
        <table className="keys">
          <tbody>
            {KEYS.map(([key, what]) => (
              <tr key={key}>
                <td>
                  <kbd>{key}</kbd>
                </td>
                <td>{what}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>
      {props.about && (
        <p className="doc-path">
          Magnesium {props.about.version}
          {props.about.commit ? `, built from ${props.about.commit.slice(0, 12)}` : ""}. Runtime{" "}
          {props.setup?.runtime.installed?.version ?? "unknown"}.
        </p>
      )}
    </article>
  );
}
