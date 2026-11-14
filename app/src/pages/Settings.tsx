// Settings: a readable page in its own tab, with sections and real controls.
// Every setting the window has is here, and nothing is shown as a file: the
// learner's own choices are kept in the window's storage on this Mac; the
// engine, the payment and a paper's time are kept by the core.

import { useEffect, useId, useMemo, useRef, useState, type ReactNode } from "react";

import { DEFAULTS, FACES, INKS, KEY_ACTIONS, MEASURES, SCHEMES, VOICES, comboOf, faceCss, keyName, type KeyAction, type Prefs } from "../prefs";
import type { About, Engine, EngineChoice, Payment, Setup } from "../types";

const ENGINE_NAMES: Record<Engine, string> = { plugin: "Claude Code", direct: "Magnesium" };

/** Faces every Mac has, offered by name beside the window's own; any other installed face can be typed. */
const MAC_FACES = [
  "Athelas",
  "Avenir Next",
  "Baskerville",
  "Big Caslon",
  "Charter",
  "Cochin",
  "Didot",
  "Futura",
  "Georgia",
  "Gill Sans",
  "Helvetica Neue",
  "Hoefler Text",
  "Iowan Old Style",
  "Menlo",
  "New York",
  "Optima",
  "Palatino",
  "Seravek",
  "SF Mono",
  "Superclarendon",
  "Times New Roman",
];

/** Whether a face by this name is on this Mac: text set in it measures differently from the fallbacks. */
function installed(name: string): boolean {
  const face = name.trim().replace(/["\\;{}]/g, "");
  if (face === "") return false;
  const canvas = document.createElement("canvas").getContext("2d");
  if (!canvas) return true;
  const sample = "mmmmmmmmmmlliWW@#0Qq";
  return ["monospace", "serif", "sans-serif"].some((fallback) => {
    canvas.font = `40px ${fallback}`;
    const plain = canvas.measureText(sample).width;
    canvas.font = `40px "${face}", ${fallback}`;
    return canvas.measureText(sample).width !== plain;
  });
}

/** A colour as the colour well takes it, "#2f3f86", from a named ink or any CSS colour. */
function hexOf(value: string, dark: boolean, fallback: string): string {
  const named = INKS[value.toLowerCase()];
  const css = named ? named[dark ? 1 : 0] : value;
  if (/^#[0-9a-f]{6}$/i.test(css)) return css.toLowerCase();
  const probe = document.createElement("canvas").getContext("2d");
  if (!probe || css === "") return fallback;
  probe.fillStyle = fallback;
  probe.fillStyle = css;
  const got = String(probe.fillStyle);
  return /^#[0-9a-f]{6}$/i.test(got) ? got : fallback;
}

/**
 * Two to five choices, one of them chosen: native radio buttons (forms
 * guide), drawn as the window's quiet segments. One Tab stop; the arrow keys
 * move along them and choose, as radios do.
 */
function Choice<T extends string | number | boolean>(props: { name: string; options: [T, string][]; value: T; onPick: (value: T) => void; face?: (value: T) => string | undefined; disabled?: boolean }) {
  const group = useId();
  return (
    <span className="segments" role="radiogroup" aria-label={props.name}>
      {props.options.map(([value, label]) => (
        <label
          key={String(value)}
          className={`segment${props.value === value ? " chosen" : ""}${props.disabled ? " off" : ""}`}
          style={props.face?.(value) ? { fontFamily: props.face(value) } : undefined}
        >
          <input type="radio" className="visually-hidden" name={group} checked={props.value === value} disabled={props.disabled} onChange={() => props.onPick(value)} />
          <span>{label}</span>
        </label>
      ))}
    </span>
  );
}

/** One setting: its visible name names the group of controls that set it. */
function Setting(props: { name: string; id?: string; className?: string; children: ReactNode; action?: string }) {
  const made = useId();
  const id = props.id ?? made;
  return (
    <div className={`setting${props.className ? ` ${props.className}` : ""}`} role="group" aria-labelledby={id} data-key-action={props.action}>
      <p className="setting-name" id={id}>
        {props.name}
      </p>
      <div className="setting-control">{props.children}</div>
    </div>
  );
}

function FontField(props: { label: string; value: string; known: [string, string][]; onPick: (value: string) => void; fallback: string; list: string[] }) {
  const isKnown = props.known.some(([value]) => value === props.value.toLowerCase());
  const [typed, setTyped] = useState(isKnown ? "" : props.value);
  const found = typed.trim() === "" ? null : installed(typed);
  const note = useId();
  return (
    <Setting name={props.label}>
      <Choice name={props.label} options={props.known} value={isKnown ? props.value.toLowerCase() : ""} onPick={(value) => (setTyped(""), props.onPick(value))} face={(value) => faceCss(value, props.fallback)} />
        <span className="font-other">
          <input
            className="text-field"
            list="mac-faces"
            aria-label={`${props.label}: another font on this Mac`}
            aria-invalid={found === false}
            aria-describedby={found === false ? note : undefined}
            placeholder="Another font on this Mac"
            value={typed}
            spellCheck={false}
            autoCorrect="off"
            autoCapitalize="none"
            style={typed && found ? { fontFamily: faceCss(typed, props.fallback) } : undefined}
            onChange={(event) => {
              const value = event.target.value;
              setTyped(value);
              if (value.trim() !== "" && installed(value)) props.onPick(value.trim());
            }}
          />
          {found === false && (
            <span className="setting-note" id={note}>
              not on this Mac
            </span>
          )}
        </span>
    </Setting>
  );
}

/** The colours a well offers, by what it colours and for which scheme: each readable on the ground it is for. */
const PALETTES: Record<"ink" | "ground" | "text", [string[], string[]]> = {
  ink: [
    ["#2f3f86", "#2e5b4a", "#4e3c8c", "#3b3e44", "#1f4e8c", "#0f6b6b", "#7a3b2e", "#8a5a00", "#6b2d5c", "#1e2023"],
    ["#9aa9ee", "#86c2a6", "#bdaeef", "#c7cad0", "#8fb7f0", "#7fd0d0", "#e0a090", "#e6c070", "#e0a0d0", "#e3e3df"],
  ],
  ground: [
    ["#fafaf8", "#ffffff", "#f6f3ec", "#f2f2f2", "#eef1f4", "#f7f1e3", "#edf3ee", "#fdf8f0"],
    ["#1c1d20", "#000000", "#15171a", "#222326", "#1b1e24", "#201d1a", "#181c1a", "#26262a"],
  ],
  text: [
    ["#1e2023", "#000000", "#2b2b2b", "#3a3530", "#1d2733", "#2f2a24"],
    ["#e3e3df", "#ffffff", "#d8d8d2", "#ece6da", "#dfe6ee", "#cfcfcf"],
  ],
};

/**
 * A colour chosen in the page itself, not in the Mac's colour panel: a well
 * that opens a grid of swatches (radio buttons: one Tab stop, the arrow keys
 * choose) and a field for any colour as #rrggbb. Esc closes the grid.
 */
function ColourField(props: { label: string; value: string; dark: boolean; fallback: string; onPick: (value: string) => void; own?: boolean; kind: "ink" | "ground" | "text"; name: string; named?: boolean }) {
  const [open, setOpen] = useState(false);
  const hex = hexOf(props.value, props.dark, props.fallback);
  const [typed, setTyped] = useState(hex);
  useEffect(() => setTyped(hex), [hex]);
  const group = useId();
  const well = useRef<HTMLButtonElement>(null);
  const palette = PALETTES[props.kind][props.dark ? 1 : 0];
  const close = () => {
    setOpen(false);
    well.current?.focus();
  };
  return (
    <span className="colour-field">
      <button
        type="button"
        ref={well}
        className="colour-well"
        data-drive={`colour-${props.kind}-${props.dark ? "dark" : "light"}`}
        aria-expanded={open}
        aria-label={`${props.name} ${props.label}: ${hex}`}
        onClick={() => setOpen((shown) => !shown)}
      >
        <span className="swatch-face" style={{ background: hex }} />
        {/* Light and dark are named once, on the first row; the rows under it keep the same columns. */}
        <span className={props.named ? undefined : "unnamed"} aria-hidden={props.named ? undefined : true}>
          {props.label}
        </span>
      </button>
      {props.own && props.value !== "" && (
        <button type="button" className="plain" onClick={() => props.onPick("")}>
          the window's own
        </button>
      )}
      {open && (
        <span
          className="colour-picker"
          role="group"
          aria-label={`${props.name} ${props.label}`}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault();
              event.stopPropagation();
              close();
            }
          }}
        >
          <span className="swatches" role="radiogroup" aria-label={`${props.name} ${props.label}: colours`}>
            {palette.map((colour) => (
              <label key={colour} className={colour === hex ? "swatch chosen" : "swatch"} title={colour}>
                <input type="radio" className="visually-hidden" name={group} aria-label={colour} checked={colour === hex} onChange={() => props.onPick(colour)} />
                <span className="swatch-face" style={{ background: colour }} />
              </label>
            ))}
          </span>
          <input
            className="text-field hex"
            aria-label={`${props.name} ${props.label}: any colour, as #rrggbb`}
            aria-invalid={!/^#[0-9a-f]{6}$/i.test(typed.trim())}
            value={typed}
            spellCheck={false}
            autoCorrect="off"
            autoCapitalize="none"
            maxLength={7}
            onChange={(event) => {
              const value = event.target.value;
              setTyped(value);
              if (/^#[0-9a-f]{6}$/i.test(value.trim())) props.onPick(value.trim().toLowerCase());
            }}
          />
          <button type="button" className="plain" onClick={close}>
            Done
          </button>
        </span>
      )}
    </span>
  );
}

function KeyRow(props: { action: KeyAction; what: string; combo: string; taken: (combo: string) => KeyAction | null; onSet: (combo: string) => void }) {
  const [listening, setListening] = useState(false);
  const [said, setSaid] = useState<string | null>(null);
  const button = useRef<HTMLButtonElement>(null);
  return (
    <Setting name={props.what} className="key-row" action={props.action}>
        <button
          type="button"
          ref={button}
          className={listening ? "key-cap listening" : "key-cap"}
          aria-label={`${props.what}: ${listening ? "press the new keys" : keyName(props.combo) || "no key"}. Choose to change it.`}
          onClick={() => {
            setSaid(null);
            setListening((on) => !on);
          }}
          onBlur={() => setListening(false)}
          onKeyDown={(event) => {
            if (!listening || event.nativeEvent.isComposing || event.keyCode === 229) return;
            event.preventDefault();
            event.stopPropagation();
            if (event.key === "Escape") return setListening(false);
            if (event.key === "Backspace" || event.key === "Delete") {
              props.onSet("none");
              return setListening(false);
            }
            const combo = comboOf(event);
            if (!combo) {
              if (!["Meta", "Control", "Alt", "Shift"].includes(event.key)) setSaid("Hold ⌘, ⌃ or ⌥ with the key.");
              return;
            }
            const other = props.taken(combo);
            setSaid(other && other !== props.action ? `${keyName(combo)} was ${KEY_ACTIONS.find(([a]) => a === other)?.[1]}; that has no key now.` : null);
            props.onSet(combo);
            setListening(false);
          }}
        >
          {listening ? "Press the new keys…" : keyName(props.combo) || "none"}
        </button>
        {props.combo !== DEFAULTS.keys[props.action] && (
          <button className="plain" onClick={() => props.onSet(DEFAULTS.keys[props.action])}>
            back to {keyName(DEFAULTS.keys[props.action])}
          </button>
        )}
        <span className="setting-note" role="status">
          {said ?? (listening ? "Delete leaves it with no key; Esc keeps it as it was." : null)}
        </span>
    </Setting>
  );
}

type Props = {
  prefs: Prefs;
  change: (change: Partial<Prefs>) => void;
  setup: Setup | null;
  engine: EngineChoice | null;
  about: About | null;
  /** A lesson runs: its engine stays until it ends. */
  lessonOpen: boolean;
  blockMinutes: number;
  section: string | null;
  onEngine: (engine: Engine) => void;
  onPayment: (payment: Payment) => void;
  onBlock: (minutes: number) => void;
  onSignIn: () => void;
  onCheckAgain: () => void;
  onReplaceRuntime: () => void;
};

export function SettingsPage(props: Props) {
  const { prefs, change } = props;
  const page = useRef<HTMLDivElement>(null);
  const [minutes, setMinutes] = useState(String(props.blockMinutes));
  // The field holds a time of the learner's own; one of the choices beside it is not said twice.
  const custom = (value: number) => ![25, 50, 90].includes(value);
  useEffect(() => setMinutes(custom(props.blockMinutes) ? String(props.blockMinutes) : ""), [props.blockMinutes]);
  // A section named from outside (Help's "Keys and Commands", About) comes into view.
  useEffect(() => {
    if (!props.section) return;
    page.current?.querySelector(`#settings-${props.section}`)?.scrollIntoView({ block: "start" });
  }, [props.section]);
  const macFaces = useMemo(() => MAC_FACES.filter(installed), []);
  const taken = (combo: string) => (KEY_ACTIONS.find(([action]) => prefs.keys[action] === combo)?.[0] ?? null);
  const setKey = (action: KeyAction, combo: string) => {
    const keys = { ...prefs.keys, [action]: combo };
    // One key does one thing: the command that had it has none now.
    for (const [other] of KEY_ACTIONS) if (other !== action && keys[other] === combo && combo !== "none") keys[other] = "none";
    change({ keys });
  };
  const account = props.setup?.account ?? null;
  const runtime = props.setup?.runtime ?? null;
  const built = props.about ? `${props.about.version.split("+")[0]}${props.about.commit ? `, built from ${props.about.commit.slice(0, 7)}` : ", built from source"}` : "";

  return (
    <div className="page settings-page" data-page="settings" ref={page}>
      <div className="page-column">
        <header className="page-head">
          <h1 className="page-title">Settings</h1>
        </header>
        <datalist id="mac-faces">
          {macFaces.map((face) => (
            <option key={face} value={face} />
          ))}
        </datalist>

        <section id="settings-reading" aria-labelledby="h-reading">
          <h2 className="section-head" id="h-reading">
            Reading
          </h2>
          <FontField label="The tutor’s words" value={prefs.face} known={FACES} fallback={faceCss("charter")} list={macFaces} onPick={(face) => change({ face })} />
          <FontField label="Your own words" value={prefs.voice} known={VOICES} fallback={faceCss("system")} list={macFaces} onPick={(voice) => change({ voice })} />
          <Setting name="Size" id="setting-size">
            <input type="range" min={12} max={28} step={1} value={prefs.size} aria-labelledby="setting-size" onChange={(event) => change({ size: Number(event.target.value) })} />
            <span className="setting-value">{prefs.size}</span>
            {prefs.size !== DEFAULTS.size && (
              <button className="plain" onClick={() => change({ size: DEFAULTS.size })}>
                usual size
              </button>
            )}
          </Setting>
          <Setting name="Line width">
            <Choice name="Line width" options={MEASURES} value={prefs.measure} onPick={(measure) => change({ measure })} />
            <input type="range" min={40} max={110} step={1} value={prefs.measure} aria-label="Line width in characters" onChange={(event) => change({ measure: Number(event.target.value) })} />
            <span className="setting-value">{prefs.measure} characters</span>
          </Setting>
          <p className="sample" style={{ fontFamily: faceCss(prefs.face) }}>
            The line meets the y-axis at 1/V<sub>max</sub>.
            {/* The tutor's words, then the learner's, on a line of their own as in a lesson. */}
            <span className="sample-you" style={{ fontFamily: prefs.voice === "reading" ? faceCss(prefs.face) : faceCss(prefs.voice, faceCss("system")) }}>
              Vmax = 4 µM/s
            </span>
          </p>
          <Setting name="Lines that name keys">
            <Choice
              name="Lines that name keys"
              options={[
                [true, "Shown"],
                [false, "Hidden"],
              ]}
              value={prefs.hints}
              onPick={(hints) => change({ hints })}
            />
          </Setting>
          <Setting name="Your line">
            <Choice
              name="Your line"
              options={[
                ["bare", "Bare line"],
                ["soft", "Soft field"],
              ]}
              value={prefs.line}
              onPick={(line) => change({ line })}
            />
          </Setting>
        </section>

        <section id="settings-appearance" aria-labelledby="h-appearance">
          <h2 className="section-head" id="h-appearance">
            Appearance
          </h2>
          <Setting name="Light or dark">
            <Choice name="Light or dark" options={SCHEMES} value={prefs.scheme} onPick={(scheme) => change({ scheme })} />
          </Setting>
          <Setting name="Ink">
            <ColourField kind="ink" name="Ink" label="in light" value={prefs.ink} dark={false} named fallback={INKS["blue-black"][0]} onPick={(ink) => change({ ink })} />
            <ColourField kind="ink" name="Ink" label="in dark" value={prefs.inkDark} dark named fallback={INKS["blue-black"][1]} onPick={(inkDark) => change({ inkDark })} />
            <span className="swatches" role="radiogroup" aria-label="Ink">
              {Object.entries(INKS).map(([name, [light, dark]]) => {
                const chosen = prefs.ink === name && prefs.inkDark === name;
                return (
                  <label key={name} className={chosen ? "swatch chosen" : "swatch"} title={name}>
                    <input type="radio" className="visually-hidden" name="settings-ink" aria-label={name} checked={chosen} onChange={() => change({ ink: name, inkDark: name })} />
                    <span className="swatch-face" style={{ background: `linear-gradient(135deg, ${light} 50%, ${dark} 50%)` }} />
                  </label>
                );
              })}
            </span>
          </Setting>
          <Setting name="Ground">
            <ColourField kind="ground" name="Ground" label="in light" value={prefs.ground} dark={false} fallback="#fafaf8" own onPick={(ground) => change({ ground })} />
            <ColourField kind="ground" name="Ground" label="in dark" value={prefs.groundDark} dark fallback="#1c1d20" own onPick={(groundDark) => change({ groundDark })} />
          </Setting>
          <Setting name="Text">
            <ColourField kind="text" name="Text" label="in light" value={prefs.text} dark={false} fallback="#1e2023" own onPick={(text) => change({ text })} />
            <ColourField kind="text" name="Text" label="in dark" value={prefs.textDark} dark fallback="#e3e3df" own onPick={(textDark) => change({ textDark })} />
          </Setting>
        </section>

        <section id="settings-keys" aria-labelledby="h-keys">
          <h2 className="section-head" id="h-keys">
            Keys
          </h2>
          <p className="setting-note wide">Choose a key to change it.</p>
          {KEY_ACTIONS.map(([action, what]) => (
            <KeyRow key={action} action={action} what={what} combo={prefs.keys[action]} taken={taken} onSet={(combo) => setKey(action, combo)} />
          ))}
          <div className="fixed-keys">
            <p>
              <span className="key-cap static">esc</span> talk instead · <span className="key-cap static">↑</span> back to the task
            </p>
            <p>
              <span className="key-cap static">⌥1 ⌥2 ⌥3</span> sure, unsure, a guess, with an answer
            </p>
            <p>
              <span className="key-cap static">⌘1</span> to <span className="key-cap static">⌘9</span> a tab · <span className="key-cap static">⇧⌘[ ⇧⌘]</span> the tab before or after
            </p>
          </div>
        </section>

        <section id="settings-lessons" aria-labelledby="h-lessons">
          <h2 className="section-head" id="h-lessons">
            Lessons
          </h2>
          <Setting name="Lessons run on">
            {props.engine ? (
              <Choice name="Lessons run on" options={(["plugin", "direct"] as Engine[]).map((value) => [value, ENGINE_NAMES[value]] as [Engine, string])} value={props.engine.chosen} onPick={props.onEngine} disabled={props.lessonOpen} />
            ) : (
              <span className="setting-note">Open a workspace to choose.</span>
            )}
            <span className="setting-note">
              {props.lessonOpen
                ? "A running lesson keeps its engine; close it to change."
                : "Claude Code runs a lesson as in a terminal. Magnesium runs it in the window, with Claude Code as the model. The next session uses it."}
            </span>
          </Setting>
          <Setting name="Paid with">
            {props.setup && (
              <Choice
                name="Paid with"
                options={[
                  ["plan", "Your Claude plan"],
                  ["api_key", "An API key"],
                ]}
                value={props.setup.payment}
                onPick={props.onPayment}
              />
            )}
            {account && (
              <span className="setting-note">
                {account.problem ?? (account.signed_in ? `${account.name} is signed in.` : `${account.name} is not signed in.`)}
                {!account.signed_in && (
                  <button className="plain" onClick={props.onSignIn}>
                    Sign in
                  </button>
                )}
                {/* Checking again is for a check that failed. */}
                {(!account.signed_in || account.problem) && (
                  <button className="plain" onClick={props.onCheckAgain}>
                    Check again
                  </button>
                )}
              </span>
            )}
          </Setting>
          <Setting name="A paper’s time">
            <Choice
              name="A paper’s time"
              options={[25, 50, 90].map((value) => [value, String(value)] as [number, string])}
              value={props.blockMinutes}
              onPick={(value) => props.onBlock(value)}
            />
            <input
              className="text-field number"
              type="number"
              min={5}
              max={240}
              aria-label="A paper's time in minutes"
              placeholder="other"
              value={minutes}
              onChange={(event) => setMinutes(event.target.value)}
              onBlur={() => {
                const value = Math.round(Number(minutes));
                if (Number.isFinite(value) && value >= 5 && value <= 240) props.onBlock(value);
                else setMinutes(custom(props.blockMinutes) ? String(props.blockMinutes) : "");
              }}
            />
            <span className="setting-note">minutes, when a paper names none</span>
          </Setting>
          {/* Which program runs lessons is said under About; here only while there is a choice to make. */}
          {runtime?.state === "kept" && runtime.bundled && (
            <Setting name="The magnesium program">
              <span className="setting-note">{runtime.installed ? `${runtime.installed.version.split("+")[0]}, kept over this app's copy` : "Kept over this app's copy"}</span>
              <button className="plain" onClick={props.onReplaceRuntime}>
                use this app's {runtime.bundled.version.split("+")[0]}
              </button>
            </Setting>
          )}
        </section>

        <section id="settings-about" aria-labelledby="h-about">
          <h2 className="section-head" id="h-about">
            About
          </h2>
          <p className="setting-note wide" data-role="version">
            Magnesium {built}
            {runtime?.installed ? `. Lessons use the magnesium program ${runtime.installed.version.split("+")[0]}.` : runtime ? ". The magnesium program is not found." : ""}
          </p>
          <p className="setting-note wide">Your study folders stay on this Mac. The window makes no network request, and lessons use your own Claude Code.</p>
        </section>
      </div>
    </div>
  );
}
