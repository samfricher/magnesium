// Settings: a readable page in its own tab, with sections and real controls.
// Every setting the window has is here, and nothing is shown as a file: the
// learner's own choices are kept in the window's storage on this Mac; the
// engine, the payment and a paper's time are kept by the core.

import { useEffect, useMemo, useRef, useState } from "react";

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

function Choice<T extends string | number | boolean>(props: { name: string; options: [T, string][]; value: T; onPick: (value: T) => void; face?: (value: T) => string | undefined; disabled?: boolean }) {
  return (
    <span className="segments" role="radiogroup" aria-label={props.name}>
      {props.options.map(([value, label]) => (
        <button
          key={String(value)}
          role="radio"
          aria-checked={props.value === value}
          className={props.value === value ? "segment chosen" : "segment"}
          style={props.face?.(value) ? { fontFamily: props.face(value) } : undefined}
          disabled={props.disabled}
          onClick={() => props.onPick(value)}
        >
          {label}
        </button>
      ))}
    </span>
  );
}

function FontField(props: { label: string; value: string; known: [string, string][]; onPick: (value: string) => void; fallback: string; list: string[] }) {
  const isKnown = props.known.some(([value]) => value === props.value.toLowerCase());
  const [typed, setTyped] = useState(isKnown ? "" : props.value);
  const found = typed.trim() === "" ? null : installed(typed);
  return (
    <div className="setting">
      <p className="setting-name">{props.label}</p>
      <div className="setting-control">
        <Choice name={props.label} options={props.known} value={isKnown ? props.value.toLowerCase() : ""} onPick={(value) => (setTyped(""), props.onPick(value))} face={(value) => faceCss(value, props.fallback)} />
        <span className="font-other">
          <input
            className="text-field"
            list="mac-faces"
            aria-label={`${props.label}: another font on this Mac`}
            placeholder="Another font on this Mac"
            value={typed}
            spellCheck={false}
            style={typed && found ? { fontFamily: faceCss(typed, props.fallback) } : undefined}
            onChange={(event) => {
              const value = event.target.value;
              setTyped(value);
              if (value.trim() !== "" && installed(value)) props.onPick(value.trim());
            }}
          />
          {found === false && <span className="setting-note">not on this Mac</span>}
        </span>
      </div>
    </div>
  );
}

function ColourField(props: { label: string; value: string; dark: boolean; fallback: string; onPick: (value: string) => void; own?: boolean }) {
  return (
    <label className="colour">
      <input type="color" value={hexOf(props.value, props.dark, props.fallback)} onChange={(event) => props.onPick(event.target.value)} aria-label={props.label} />
      <span>{props.label}</span>
      {props.own && props.value !== "" && (
        <button className="plain" onClick={(event) => (event.preventDefault(), props.onPick(""))}>
          the window's own
        </button>
      )}
    </label>
  );
}

function KeyRow(props: { action: KeyAction; what: string; combo: string; taken: (combo: string) => KeyAction | null; onSet: (combo: string) => void }) {
  const [listening, setListening] = useState(false);
  const [said, setSaid] = useState<string | null>(null);
  const button = useRef<HTMLButtonElement>(null);
  return (
    <div className="setting key-row" data-key-action={props.action}>
      <p className="setting-name">{props.what}</p>
      <div className="setting-control">
        <button
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
        {said && <span className="setting-note">{said}</span>}
      </div>
    </div>
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
  useEffect(() => setMinutes(String(props.blockMinutes)), [props.blockMinutes]);
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
          <p className="where">Kept on this Mac. Each change shows at once.</p>
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
          <FontField label="The tutor's words" value={prefs.face} known={FACES} fallback={faceCss("charter")} list={macFaces} onPick={(face) => change({ face })} />
          <FontField label="Your own words" value={prefs.voice} known={VOICES} fallback={faceCss("system")} list={macFaces} onPick={(voice) => change({ voice })} />
          <div className="setting">
            <p className="setting-name">Size</p>
            <div className="setting-control">
              <input type="range" min={12} max={28} step={1} value={prefs.size} aria-label="Text size" onChange={(event) => change({ size: Number(event.target.value) })} />
              <span className="setting-value">{prefs.size}</span>
              {prefs.size !== DEFAULTS.size && (
                <button className="plain" onClick={() => change({ size: DEFAULTS.size })}>
                  usual size
                </button>
              )}
            </div>
          </div>
          <div className="setting">
            <p className="setting-name">Line width</p>
            <div className="setting-control">
              <Choice name="Line width" options={MEASURES} value={prefs.measure} onPick={(measure) => change({ measure })} />
              <input type="range" min={40} max={110} step={1} value={prefs.measure} aria-label="Line width in characters" onChange={(event) => change({ measure: Number(event.target.value) })} />
              <span className="setting-value">{prefs.measure} characters</span>
            </div>
          </div>
          <div className="setting">
            <p className="setting-name">Lines that name keys</p>
            <div className="setting-control">
              <Choice
                name="Lines that name keys"
                options={[
                  [true, "Shown"],
                  [false, "Hidden"],
                ]}
                value={prefs.hints}
                onPick={(hints) => change({ hints })}
              />
            </div>
          </div>
          <p className="sample" style={{ fontFamily: faceCss(prefs.face) }}>
            The line meets the y-axis at 1/V<sub>max</sub>.
            <span className="sample-you" style={{ fontFamily: prefs.voice === "reading" ? faceCss(prefs.face) : faceCss(prefs.voice, faceCss("system")) }}>
              {" "}
              Vmax = 4 µM/s
            </span>
          </p>
        </section>

        <section id="settings-appearance" aria-labelledby="h-appearance">
          <h2 className="section-head" id="h-appearance">
            Appearance
          </h2>
          <div className="setting">
            <p className="setting-name">Light or dark</p>
            <div className="setting-control">
              <Choice name="Light or dark" options={SCHEMES} value={prefs.scheme} onPick={(scheme) => change({ scheme })} />
            </div>
          </div>
          <div className="setting">
            <p className="setting-name">Ink</p>
            <div className="setting-control">
              <span className="swatches" role="radiogroup" aria-label="Ink">
                {Object.entries(INKS).map(([name, [light, dark]]) => (
                  <button
                    key={name}
                    role="radio"
                    aria-checked={prefs.ink === name && prefs.inkDark === name}
                    aria-label={name}
                    title={name}
                    className={prefs.ink === name && prefs.inkDark === name ? "swatch chosen" : "swatch"}
                    style={{ background: `linear-gradient(135deg, ${light} 50%, ${dark} 50%)` }}
                    onClick={() => change({ ink: name, inkDark: name })}
                  />
                ))}
              </span>
              <ColourField label="in light" value={prefs.ink} dark={false} fallback={INKS["blue-black"][0]} onPick={(ink) => change({ ink })} />
              <ColourField label="in dark" value={prefs.inkDark} dark fallback={INKS["blue-black"][1]} onPick={(inkDark) => change({ inkDark })} />
            </div>
          </div>
          <div className="setting">
            <p className="setting-name">Ground</p>
            <div className="setting-control">
              <ColourField label="in light" value={prefs.ground} dark={false} fallback="#fafaf8" own onPick={(ground) => change({ ground })} />
              <ColourField label="in dark" value={prefs.groundDark} dark fallback="#1c1d20" own onPick={(groundDark) => change({ groundDark })} />
            </div>
          </div>
          <div className="setting">
            <p className="setting-name">Text</p>
            <div className="setting-control">
              <ColourField label="in light" value={prefs.text} dark={false} fallback="#1e2023" own onPick={(text) => change({ text })} />
              <ColourField label="in dark" value={prefs.textDark} dark fallback="#e3e3df" own onPick={(textDark) => change({ textDark })} />
            </div>
          </div>
          <p className="setting-note wide">Quiet text, rules and the line an answer is written on are mixed from the text and the ground, so they stay readable whatever colours are chosen.</p>
        </section>

        <section id="settings-keys" aria-labelledby="h-keys">
          <h2 className="section-head" id="h-keys">
            Keys
          </h2>
          <p className="setting-note wide">Choose a key, then press the new combination. Delete leaves the command with no key; Esc keeps it as it was. The menu bar follows.</p>
          {KEY_ACTIONS.map(([action, what]) => (
            <KeyRow key={action} action={action} what={what} combo={prefs.keys[action]} taken={taken} onSet={(combo) => setKey(action, combo)} />
          ))}
          <div className="fixed-keys">
            <p>
              <span className="key-cap static">↩</span> start, or send · <span className="key-cap static">⇧↩</span> a new line
            </p>
            <p>
              <span className="key-cap static">esc</span> out of the task, to talk · <span className="key-cap static">↑</span> back into it
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
          <div className="setting">
            <p className="setting-name">Lessons run on</p>
            <div className="setting-control">
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
            </div>
          </div>
          <div className="setting">
            <p className="setting-name">Paid with</p>
            <div className="setting-control">
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
                  <button className="plain" onClick={props.onCheckAgain}>
                    check again
                  </button>
                </span>
              )}
            </div>
          </div>
          <div className="setting">
            <p className="setting-name">A paper's time</p>
            <div className="setting-control">
              <Choice
                name="A paper's time"
                options={[25, 50, 90].map((value) => [value, `${value} min`] as [number, string])}
                value={props.blockMinutes}
                onPick={(value) => props.onBlock(value)}
              />
              <input
                className="text-field number"
                type="number"
                min={5}
                max={240}
                aria-label="A paper's time in minutes"
                value={minutes}
                onChange={(event) => setMinutes(event.target.value)}
                onBlur={() => {
                  const value = Math.round(Number(minutes));
                  if (Number.isFinite(value) && value >= 5 && value <= 240) props.onBlock(value);
                  else setMinutes(String(props.blockMinutes));
                }}
              />
              <span className="setting-note">minutes, when a paper names none</span>
            </div>
          </div>
          {runtime && (
            <div className="setting">
              <p className="setting-name">The magnesium program</p>
              <div className="setting-control">
                <span className="setting-note">
                  {runtime.installed ? `Version ${runtime.installed.version.split("+")[0]}` : "Not found"}
                  {runtime.state === "kept" ? ", kept over this app's copy" : ""}
                </span>
                {runtime.state === "kept" && runtime.bundled && (
                  <button className="plain" onClick={props.onReplaceRuntime}>
                    use this app's {runtime.bundled.version.split("+")[0]}
                  </button>
                )}
              </div>
            </div>
          )}
          <p className="setting-note wide">Recording in a study folder is turned off and on by asking the tutor, in words, on the Magnesium engine.</p>
        </section>

        <section id="settings-about" aria-labelledby="h-about">
          <h2 className="section-head" id="h-about">
            About
          </h2>
          <p className="page-text" data-role="version">
            Magnesium {built}
          </p>
          <p className="setting-note wide">Your study folders stay on this Mac. The window makes no network request; lessons run on your own Claude Code.</p>
        </section>
      </div>
    </div>
  );
}
