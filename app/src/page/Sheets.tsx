// Everything that is not today's page opens over it as a sheet from the
// right, one at a time, on a key, and closes with Esc: the commands, the
// courses, the open course's material, papers and record, the plan in full,
// and the learner's settings. None of it sits on screen while they work.

import { useEffect, useMemo, useRef, useState } from "react";

import { longDay, shortDay } from "../days";
import { courseName } from "../log";
import { FACES, SIZES, type Prefs } from "../prefs";
import { Line, Rich } from "../ui/Rich";
import type { About, Card, Engine, EngineChoice, Home, Materials, Paper, Plan, Progress, Row, Setup } from "../types";

export type SheetName = "commands" | "courses" | "course" | "plan" | "settings" | "paper";

export function Sheet(props: { label: string; wide?: boolean; onClose: () => void; children: React.ReactNode; role?: string }) {
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const back = document.activeElement as HTMLElement | null;
    const first = root.current?.querySelector<HTMLElement>("[data-autofocus], input, button, [tabindex='0']");
    first?.focus();
    return () => back?.focus?.();
  }, []);
  return (
    <div
      className={props.wide ? "sheet wide" : "sheet"}
      role="dialog"
      aria-modal="true"
      aria-label={props.label}
      data-sheet={props.role}
      ref={root}
      onKeyDown={(event) => {
        if (event.key === "Escape" && !event.defaultPrevented) {
          event.preventDefault();
          event.stopPropagation();
          props.onClose();
        }
      }}
    >
      <header className="sheet-head">
        <h2>{props.label}</h2>
        <button className="text-btn" onClick={props.onClose} title="Esc">
          Close
        </button>
      </header>
      <div className="sheet-body">{props.children}</div>
    </div>
  );
}

// ---------------------------------------------------------------- commands

export type Command = { label: string; key?: string; hint?: string; keywords?: string; run: () => void };

export function Commands(props: { commands: Command[]; onClose: () => void }) {
  const [query, setQuery] = useState("");
  const [at, setAt] = useState(0);
  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return props.commands;
    return props.commands.filter((c) => `${c.label} ${c.keywords ?? ""} ${c.hint ?? ""}`.toLowerCase().includes(q));
  }, [query, props.commands]);
  useEffect(() => setAt(0), [query]);
  const run = (command: Command | undefined) => {
    if (!command) return;
    props.onClose();
    // The sheet gives the focus back first; the command then takes it where it needs.
    window.setTimeout(command.run, 0);
  };
  return (
    <Sheet label="Commands" onClose={props.onClose} role="commands">
      <input
        className="filter"
        data-autofocus
        aria-label="Find a command"
        placeholder="Find a command"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown") {
            e.preventDefault();
            setAt((n) => Math.min(shown.length - 1, n + 1));
          } else if (e.key === "ArrowUp") {
            e.preventDefault();
            setAt((n) => Math.max(0, n - 1));
          } else if (e.key === "Enter") {
            e.preventDefault();
            run(shown[at]);
          }
        }}
        role="combobox"
        aria-expanded="true"
        aria-controls="command-list"
        aria-activedescendant={shown[at] ? `command-${at}` : undefined}
      />
      <ul className="list" id="command-list" role="listbox">
        {shown.map((command, i) => (
          <li
            key={command.label}
            id={`command-${i}`}
            role="option"
            aria-selected={i === at}
            className={i === at ? "item chosen" : "item"}
            onMouseEnter={() => setAt(i)}
            onClick={() => run(command)}
          >
            <span className="item-label">{command.label}</span>
            {command.hint && <span className="item-hint">{command.hint}</span>}
            {command.key && <kbd>{command.key}</kbd>}
          </li>
        ))}
        {shown.length === 0 && <li className="item empty">Nothing by that name.</li>}
      </ul>
    </Sheet>
  );
}

// ----------------------------------------------------------------- courses

export function Courses(props: {
  cards: Card[] | null;
  open: string | null;
  finding: boolean;
  notice: string | null;
  onOpen: (folder: string) => void;
  onPick: () => void;
  onNew: () => void;
  onFind: () => void;
  onForget: (folder: string) => void;
  onClose: () => void;
}) {
  const [forgetting, setForgetting] = useState<string | null>(null);
  return (
    <Sheet label="Courses" onClose={props.onClose} role="courses">
      {props.cards === null ? (
        <p className="quiet">Reading your courses.</p>
      ) : props.cards.length === 0 ? (
        <p className="quiet">No courses yet. A course is a folder with your material in it.</p>
      ) : (
        <ul className="list courses">
          {props.cards.map((card) => (
            <li key={card.folder} className={card.folder === props.open ? "item course-item open" : "item course-item"}>
              <button className="item-main" onClick={() => props.onOpen(card.folder)} data-drive={`course:${card.name}`}>
                <span className="item-label">{courseName(card.name)}</span>
                <span className="item-hint">
                  {card.goal ? <Line text={card.goal} /> : card.state === "not_set_up" ? "Not set up yet" : card.state === "empty" ? "Nothing done here yet" : null}
                </span>
                {card.exam?.date && (
                  <span className="item-hint">
                    {card.exam.title}, {shortDay(card.exam.date)}
                  </span>
                )}
              </button>
              {forgetting === card.folder ? (
                <span className="confirm">
                  <button className="text-btn" onClick={() => props.onForget(card.folder)}>
                    Take it off the list
                  </button>
                  <button className="text-btn" onClick={() => setForgetting(null)}>
                    Keep it
                  </button>
                </span>
              ) : (
                <button className="text-btn quiet" onClick={() => setForgetting(card.folder)} title="The folder itself stays as it is">
                  Remove
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
      <div className="actions">
        <button className="text-btn" onClick={props.onPick}>
          Open a folder
        </button>
        <button className="text-btn" onClick={props.onNew}>
          New folder
        </button>
        <button className="text-btn" onClick={props.onFind} disabled={props.finding}>
          {props.finding ? "Looking…" : "Find folders on this Mac"}
        </button>
      </div>
      {props.notice && <p className="quiet">{props.notice}</p>}
    </Sheet>
  );
}

// ------------------------------------------------------------------ course

/** A skill's record, said in words: what the record says, never a dot or a level. */
function recordLine(row: Row): string {
  const passes = row.dots.filter((d) => d === 2).length;
  const days = passes === 0 ? "not yet right from a blank page on a day of its own" : passes === 1 ? "right from a blank page on one day" : `right from a blank page on ${passes} separate days`;
  return row.note && row.note !== "–" ? `${days}; ${row.note}` : days;
}

export function Course(props: {
  home: Home;
  materials: Materials | null;
  materialLines: string[] | null;
  progress: Progress | null;
  adding: boolean;
  onAddFiles: () => void;
  onPaste: (text: string) => void;
  onSitPaper: (paper: Paper) => void;
  onUpgrade: () => void;
  onClose: () => void;
}) {
  const { home, progress } = props;
  const [pasting, setPasting] = useState<string | null>(null);
  const rows = progress ? [...progress.in_progress, ...progress.confirmed] : [];
  return (
    <Sheet label={courseName(home.name)} onClose={props.onClose} role="course">
      {home.message && <p className="quiet">{home.message}</p>}
      {home.state === "needs_upgrade" && (
        <p>
          <button className="btn" onClick={props.onUpgrade}>
            Upgrade this folder
          </button>
        </p>
      )}
      <section className="part" aria-label="Material">
        <h3>Material</h3>
        {props.materials && props.materials.files > 0 ? (
          <ul className="plain">
            {props.materials.names.map((name) => (
              <li key={name}>{name}</li>
            ))}
            {props.materials.files > props.materials.names.length && <li className="quiet">and {props.materials.files - props.materials.names.length} more</li>}
          </ul>
        ) : (
          <p className="quiet">Nothing added yet. Drop files on the window, or add them here.</p>
        )}
        {props.materials && props.materials.keys > 0 && (
          <p className="quiet">{props.materials.keys === 1 ? "One answer key is kept and never shown." : `${props.materials.keys} answer keys are kept and never shown.`}</p>
        )}
        <div className="actions">
          <button className="text-btn" onClick={props.onAddFiles} disabled={props.adding}>
            Add files
          </button>
          <button className="text-btn" onClick={() => setPasting("")}>
            Paste text
          </button>
        </div>
        {pasting !== null && (
          <div className="paste">
            <textarea data-autofocus aria-label="Text to add as material" value={pasting} onChange={(e) => setPasting(e.target.value)} rows={5} />
            <div className="actions">
              <button
                className="btn"
                disabled={pasting.trim() === "" || props.adding}
                onClick={() => {
                  props.onPaste(pasting);
                  setPasting(null);
                }}
              >
                Add as a text file
              </button>
              <button className="text-btn" onClick={() => setPasting(null)}>
                Cancel
              </button>
            </div>
          </div>
        )}
        {props.materialLines && props.materialLines.map((line) => <p key={line} className="quiet">{line}</p>)}
      </section>
      {home.papers.length > 0 && (
        <section className="part" aria-label="Papers">
          <h3>Practice papers</h3>
          <ul className="list">
            {home.papers.map((paper) => (
              <li key={paper.path} className="item">
                <span className="item-label">{paper.name}</span>
                <span className="item-hint">{paper.minutes ? `${paper.minutes} minutes` : null}{paper.sat ? ", sat" : ""}</span>
                <button className="text-btn" onClick={() => props.onSitPaper(paper)} disabled={Boolean(home.sitting)}>
                  Sit it
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}
      <section className="part" aria-label="The record">
        <h3>The record</h3>
        {!progress ? (
          <p className="quiet">Reading the record.</p>
        ) : progress.message ? (
          <p className="quiet">{progress.message}</p>
        ) : rows.length === 0 ? (
          <p className="quiet">Nothing marked yet.</p>
        ) : (
          <ul className="plain record">
            {rows.map((row) => (
              <li key={row.name}>
                <Line text={row.name} className="record-name" />
                <span className="quiet"> {recordLine(row)}</span>
              </li>
            ))}
          </ul>
        )}
        {progress && progress.untested.length > 0 && (
          <p className="quiet">
            Not answered yet: {progress.untested.slice(0, 8).map((row) => row.name).join(", ")}
            {progress.untested.length > 8 ? ", and more" : ""}.
          </p>
        )}
        {progress && !progress.level_unknown && progress.proofs.levelled && (
          <p className="quiet small">
            The runtime's own measure of this plan: level {progress.proofs.level} of 10. A skill counts as known after it is answered right, unaided, on three separate days.
          </p>
        )}
      </section>
    </Sheet>
  );
}

// -------------------------------------------------------------------- plan

export function PlanSheet(props: { plan: Plan | null; onClose: () => void }) {
  const { plan } = props;
  return (
    <Sheet label={plan?.title ?? "The plan"} onClose={props.onClose} role="plan" wide>
      {!plan || (!plan.title && plan.items.length === 0 && !plan.goal) ? (
        <p className="quiet">This course has no plan yet. Ask the tutor for one: "make me a plan for the midterm".</p>
      ) : (
        <>
          {plan.exam_date && <p className="plan-date">{longDay(plan.exam_date)}</p>}
          {plan.items.length > 0 && (
            <ol className="plan-items">
              {plan.items.map((item) => (
                <li key={item.id} className={item.status}>
                  <Line text={item.title} />
                  {item.note && <span className="quiet"> {item.note}</span>}
                </li>
              ))}
            </ol>
          )}
          {plan.goal && (
            <section className="part">
              <h3>Working on</h3>
              <p>
                <Line text={plan.goal} />
              </p>
              {plan.steps.length > 0 && (
                <ol className="plan-steps">
                  {plan.steps.map((step) => (
                    <li key={step.step} className={step.status}>
                      <Line text={step.step} />
                    </li>
                  ))}
                </ol>
              )}
            </section>
          )}
          <p className="quiet small">Say what to change in the lesson, on the page: "move the midterm to the 24th", "do the redos before Paper B". The tutor changes the plan and this follows.</p>
          {plan.text && (
            <details className="plan-file">
              <summary>The plan file{plan.path ? `, ${plan.path}` : ""}</summary>
              <Rich text={plan.text} final />
            </details>
          )}
        </>
      )}
    </Sheet>
  );
}

// ---------------------------------------------------------------- settings

export const KEYS: [string, string][] = [
  ["Return", "Send. On an empty first line, pick up the suggested line"],
  ["Shift-Return", "New line"],
  ["Esc", "Step out of the open task onto the page, or back in; close a sheet"],
  ["⌘.", "Stop the tutor's reply"],
  ["⌘E", "End the sitting, with a line for next time"],
  ["⌘N", "A new lesson in this course"],
  ["⌘O", "Courses"],
  ["⌘I", "This course: material, papers, the record"],
  ["⌘P", "The plan in full"],
  ["⌘K", "Every command"],
  ["⌘,", "Settings"],
  ["⌘⇧H  ⌘⇧L  ⌘⇧D", "Hint, I'm lost, I don't know"],
  ["⌥1  ⌥2  ⌥3", "Sure, unsure, guess"],
  ["⌘⇧A", "Add a photo of your working"],
  ["⌘⇧S", "Symbols"],
  ["⌘↑  ⌘↓", "Today, the next earlier day"],
  ["⌘+  ⌘−", "Text size"],
];

function Choice<T extends string | number>(props: { label: string; value: T; options: { id: T; name: string }[]; onChange: (value: T) => void; disabled?: string | null }) {
  return (
    <div className="choice" role="radiogroup" aria-label={props.label}>
      <span className="choice-label">{props.label}</span>
      <span className="choice-options">
        {props.options.map((option) => (
          <button
            key={String(option.id)}
            role="radio"
            aria-checked={props.value === option.id}
            className={props.value === option.id ? "text-btn chosen" : "text-btn"}
            disabled={Boolean(props.disabled)}
            onClick={() => props.onChange(option.id)}
            data-face={props.label === "Typeface" ? String(option.id) : undefined}
          >
            {option.name}
          </button>
        ))}
      </span>
      {props.disabled && <span className="quiet small">{props.disabled}</span>}
    </div>
  );
}

export function Settings(props: {
  prefs: Prefs;
  onPrefs: (prefs: Prefs) => void;
  engine: EngineChoice | null;
  inLesson: boolean;
  onEngine: (engine: Engine) => void;
  setup: Setup | null;
  onPayment: (payment: "plan" | "api_key") => void;
  onSignIn: () => void;
  signingIn: boolean;
  about: About | null;
  onClose: () => void;
}) {
  const { prefs, setup } = props;
  const account = setup?.account ?? null;
  return (
    <Sheet label="Settings" onClose={props.onClose} role="settings" wide>
      <section className="part">
        <h3>The page</h3>
        <Choice label="Typeface" value={prefs.face} options={FACES} onChange={(face) => props.onPrefs({ ...prefs, face })} />
        <Choice label="Size" value={prefs.size} options={SIZES.map((size) => ({ id: size, name: `${size}` }))} onChange={(size) => props.onPrefs({ ...prefs, size })} />
        <Choice
          label="Scheme"
          value={prefs.scheme}
          options={[
            { id: "system", name: "As the Mac" },
            { id: "light", name: "Light" },
            { id: "dark", name: "Dark" },
          ]}
          onChange={(scheme) => props.onPrefs({ ...prefs, scheme })}
        />
        <Choice
          label="Line length"
          value={prefs.width}
          options={[
            { id: "narrow", name: "Book" },
            { id: "wide", name: "Wide" },
          ]}
          onChange={(width) => props.onPrefs({ ...prefs, width })}
        />
        <p className="sample">
          <Line text={"A 95% interval for the mean is $\\bar{x} \\pm t^* s/\\sqrt{n}$, in the face and size you chose."} />
        </p>
      </section>
      <section className="part">
        <h3>Lessons</h3>
        {props.engine && (
          <Choice
            label="Run on"
            value={props.engine.chosen}
            options={[
              { id: "plugin", name: "Claude Code" },
              { id: "direct", name: "Magnesium" },
            ]}
            onChange={props.onEngine}
            disabled={props.inLesson ? "Changes after this lesson ends." : null}
          />
        )}
        {props.engine && props.engine.conversation && props.engine.conversation !== props.engine.chosen && (
          <p className="quiet small">The next lesson starts a new conversation. Your record carries on.</p>
        )}
        {account && (
          <Choice
            label="Paid with"
            value={setup?.payment ?? "plan"}
            options={[
              { id: "plan", name: "Your Claude plan" },
              { id: "api_key", name: "An API key" },
            ]}
            onChange={props.onPayment}
          />
        )}
        {account && (
          <p className="quiet small">
            {account.installed ? (account.signed_in ? `Claude Code is signed in${account.detail ? `: ${account.detail}` : ""}.` : "Claude Code is signed out.") : "Claude Code is not on this Mac."}
            {account.problem ? ` ${account.problem}` : ""}
            {account.installed && !account.signed_in && (
              <>
                {" "}
                <button className="text-btn" onClick={props.onSignIn} disabled={props.signingIn}>
                  {props.signingIn ? "Waiting for the browser…" : "Sign in"}
                </button>
              </>
            )}
          </p>
        )}
        {setup && <p className="quiet small">{setup.runtime.message ?? (setup.runtime.installed ? `The magnesium program, ${setup.runtime.installed.version.split("+")[0]}, at ${setup.runtime.installed.shown_path}.` : null)}</p>}
      </section>
      <section className="part">
        <h3>Keys</h3>
        <dl className="keys">
          {KEYS.map(([key, what]) => (
            <div key={key}>
              <dt>
                <kbd>{key}</kbd>
              </dt>
              <dd>{what}</dd>
            </div>
          ))}
        </dl>
      </section>
      {props.about && (
        <p className="quiet small">
          Magnesium {props.about.version}
          {props.about.commit ? ` (${props.about.commit.slice(0, 7)})` : ", built from source"}. Everything stays on this Mac.
        </p>
      )}
    </Sheet>
  );
}

// ------------------------------------------------------------------- paper

export function PaperSheet(props: { paper: Paper; blockMinutes: number; onStart: (minutes: number) => void; onClose: () => void }) {
  const [minutes, setMinutes] = useState(String(props.paper.minutes ?? props.blockMinutes));
  const value = Math.round(Number(minutes));
  const ok = Number.isFinite(value) && value >= 1 && value <= 600;
  return (
    <Sheet label={props.paper.name} onClose={props.onClose} role="paper">
      <p>The paper opens in your PDF viewer and the clock starts. Work on paper; while it runs, the tutor waits.</p>
      <label className="field-line">
        <span>Minutes</span>
        <input data-autofocus inputMode="numeric" value={minutes} onChange={(e) => setMinutes(e.target.value)} onKeyDown={(e) => e.key === "Enter" && ok && props.onStart(value)} />
      </label>
      <div className="actions">
        <button className="btn" disabled={!ok} onClick={() => props.onStart(value)}>
          Start the paper
        </button>
      </div>
    </Sheet>
  );
}

/** The record's answer for whether it can tell a course's state, in one quiet line. */
export function homeLine(home: Home | null): string | null {
  if (!home) return null;
  return home.message;
}
