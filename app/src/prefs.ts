// What the learner set, and where they were, kept on this Mac in the
// window's own storage. None of it is learning state: the record keeps that.
//
// Settings bend the window to the learner: the face they read in, the face
// their own words are in, the size, the line length, the scheme, the colours
// and every key. The Settings page is where they are changed; they are never
// shown as a file. The memory is per session: the draft on the line, where
// the line was (in the task or in the conversation), how far the lesson was
// scrolled, and when the window last saw it.

/** A face named by the window, or any installed face by its name. */
export type Face = string;
export type Scheme = "system" | "light" | "dark";

/** What a key does. Each can be bound to any key with ⌘, ⌃, ⌥ or ⇧. The names are the menu bar's commands. */
export type KeyAction =
  | "home"
  | "commands"
  | "new-session"
  | "open-workspace"
  | "close-session"
  | "attach"
  | "settings"
  | "plan"
  | "stop-reply"
  | "hint"
  | "lost"
  | "unknown"
  | "symbols"
  | "bigger"
  | "smaller"
  | "actual-size";

export type Prefs = {
  /** The face the material and the tutor are read in: a name below, or any face installed on this Mac. */
  face: Face;
  /** The face of the learner's own words: "system", "reading", "mono", or any installed face. */
  voice: Face;
  size: number;
  /** The longest line of the reading face, in characters. */
  measure: number;
  scheme: Scheme;
  /** The ink, for light and dark: a name below, or any colour CSS takes. */
  ink: string;
  inkDark: string;
  /** The ground and the text, for light and dark; empty is the window's own. */
  ground: string;
  groundDark: string;
  text: string;
  textDark: string;
  /** Whether the lines that name keys are shown. */
  hints: boolean;
  keys: Record<KeyAction, string>;
};

export const KEY_ACTIONS: [KeyAction, string][] = [
  ["home", "Home"],
  ["new-session", "A new session"],
  ["open-workspace", "Open a workspace"],
  ["close-session", "Close the session"],
  ["attach", "Attach a photo or PDF"],
  ["plan", "Show or hide the plan"],
  ["settings", "Settings"],
  ["commands", "The command menu"],
  ["hint", "A hint"],
  ["lost", "I'm lost"],
  ["unknown", "I don't know"],
  ["symbols", "Symbols"],
  ["stop-reply", "Stop the tutor's reply"],
  ["bigger", "Bigger text"],
  ["smaller", "Smaller text"],
  ["actual-size", "Text at its usual size"],
];

export const DEFAULTS: Prefs = {
  face: "charter",
  voice: "system",
  size: 17,
  measure: 66,
  scheme: "system",
  ink: "blue-black",
  inkDark: "blue-black",
  ground: "",
  groundDark: "",
  text: "",
  textDark: "",
  hints: true,
  keys: {
    home: "cmd+1",
    commands: "cmd+k",
    "new-session": "cmd+n",
    "open-workspace": "cmd+o",
    "close-session": "cmd+w",
    attach: "cmd+u",
    settings: "cmd+,",
    plan: "cmd+p",
    "stop-reply": "cmd+.",
    hint: "cmd+shift+h",
    lost: "cmd+shift+l",
    unknown: "cmd+shift+d",
    symbols: "cmd+shift+m",
    bigger: "cmd+=",
    smaller: "cmd+-",
    "actual-size": "cmd+0",
  },
};

export const FACES: [Face, string][] = [
  ["charter", "Charter"],
  ["iowan", "Iowan Old Style"],
  ["newyork", "New York"],
  ["system", "System"],
  ["mono", "Mono"],
];
export const VOICES: [Face, string][] = [
  ["system", "System"],
  ["reading", "Same as reading"],
  ["mono", "Mono"],
];
export const SIZES = [15, 17, 19, 21];
export const MEASURES: [number, string][] = [
  [56, "Narrow"],
  [66, "Medium"],
  [78, "Wide"],
];
export const SCHEMES: [Scheme, string][] = [
  ["system", "Like the Mac"],
  ["light", "Light"],
  ["dark", "Dark"],
];
/** The window's inks, light and dark. */
export const INKS: Record<string, [string, string]> = {
  "blue-black": ["#2f3f86", "#9aa9ee"],
  "green-black": ["#2e5b4a", "#86c2a6"],
  violet: ["#4e3c8c", "#bdaeef"],
  graphite: ["#3b3e44", "#c7cad0"],
};

const FACE_CSS: Record<string, string> = {
  charter: 'Charter, "Iowan Old Style", Georgia, serif',
  iowan: '"Iowan Old Style", Charter, Georgia, serif',
  newyork: 'ui-serif, "New York", "Iowan Old Style", Charter, Georgia, serif',
  system: '-apple-system, BlinkMacSystemFont, "SF Pro Text", "Helvetica Neue", sans-serif',
  mono: 'ui-monospace, "SF Mono", SFMono-Regular, Menlo, monospace',
};

/** The CSS for a face: one of the window's names, or an installed face by its name, with fallbacks. */
export function faceCss(face: string, fallback = FACE_CSS.charter): string {
  const named = FACE_CSS[face.trim().toLowerCase()];
  if (named) return named;
  const name = face.trim().replace(/["\;{}]/g, "");
  return name === "" ? fallback : `"${name}", ${fallback}`;
}

const PREFS = "mg.prefs.2";
/** Keys an earlier window kept under other names. */
const OLD_KEYS: Record<string, KeyAction> = { stop: "stop-reply", new: "new-session", courses: "open-workspace" };
const PLACE = "mg.place";

function readJson<T>(key: string): T | null {
  try {
    const raw = window.localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : null;
  } catch {
    return null;
  }
}

function writeJson(key: string, value: unknown): void {
  try {
    window.localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // A window that cannot keep it still works; it opens as new next time.
  }
}

/** Keep only what makes sense, so a bad value never breaks the page. */
export function cleanPrefs(raw: Partial<Prefs>): Prefs {
  const str = (value: unknown, fallback: string) => (typeof value === "string" ? value.trim() : fallback);
  const num = (value: unknown, low: number, high: number, fallback: number) =>
    typeof value === "number" && Number.isFinite(value) ? Math.min(high, Math.max(low, Math.round(value))) : fallback;
  const keys = { ...DEFAULTS.keys };
  const given = (raw.keys ?? {}) as Record<string, unknown>;
  for (const [old, action] of Object.entries(OLD_KEYS)) {
    const combo = given[old];
    if (typeof combo === "string" && given[action] === undefined && parseCombo(combo)) keys[action] = normalCombo(combo);
  }
  for (const [action] of KEY_ACTIONS) {
    const combo = given[action];
    if (combo === "none") keys[action] = "none";
    else if (typeof combo === "string" && parseCombo(combo)) keys[action] = normalCombo(combo);
  }
  return {
    face: str(raw.face, DEFAULTS.face) || DEFAULTS.face,
    voice: str(raw.voice, DEFAULTS.voice) || DEFAULTS.voice,
    size: num(raw.size, 12, 28, DEFAULTS.size),
    measure: num(raw.measure, 40, 110, DEFAULTS.measure),
    scheme: (["system", "light", "dark"] as Scheme[]).includes(raw.scheme as Scheme) ? (raw.scheme as Scheme) : DEFAULTS.scheme,
    ink: str(raw.ink, DEFAULTS.ink) || DEFAULTS.ink,
    inkDark: str(raw.inkDark, DEFAULTS.inkDark) || DEFAULTS.inkDark,
    ground: str(raw.ground, ""),
    groundDark: str(raw.groundDark, ""),
    text: str(raw.text, ""),
    textDark: str(raw.textDark, ""),
    hints: typeof raw.hints === "boolean" ? raw.hints : DEFAULTS.hints,
    keys,
  };
}

export function loadPrefs(): Prefs {
  return cleanPrefs(readJson<Partial<Prefs>>(PREFS) ?? {});
}

export function savePrefs(prefs: Prefs): void {
  writeJson(PREFS, prefs);
}

/** A colour the page can use: one of the window's inks by name, or any CSS colour. */
function colour(value: string, dark: boolean): string | null {
  const named = INKS[value.toLowerCase()];
  if (named) return named[dark ? 1 : 0];
  if (value === "" || !CSS.supports("color", value)) return null;
  return value;
}

/**
 * Put the settings on the page. The scheme is only written when the learner
 * chose one: "Like the Mac" leaves `data-scheme` alone, so a scheme set from
 * outside (the evidence harness) stands.
 */
export function applyPrefs(prefs: Prefs, schemeChanged = false): void {
  const root = document.documentElement;
  const set = (name: string, value: string | null) => (value ? root.style.setProperty(name, value) : root.style.removeProperty(name));
  set("--read", faceCss(prefs.face));
  const voice = prefs.voice.trim().toLowerCase();
  set("--you", voice === "reading" ? "var(--read)" : faceCss(prefs.voice, FACE_CSS.system));
  set("--size", `${prefs.size}px`);
  set("--measure", `${(prefs.measure * 0.47).toFixed(2)}em`);
  set("--ink-l", colour(prefs.ink, false));
  set("--ink-d", colour(prefs.inkDark || prefs.ink, true));
  set("--ground-l", colour(prefs.ground, false));
  set("--ground-d", colour(prefs.groundDark, true));
  set("--text-l", colour(prefs.text, false));
  set("--text-d", colour(prefs.textDark, true));
  root.dataset.hints = prefs.hints ? "on" : "off";
  if (prefs.scheme !== "system") root.dataset.scheme = prefs.scheme;
  else if (schemeChanged) delete root.dataset.scheme;
}

// ---------------------------------------------------------------------------
// Keys

const MODS = ["cmd", "ctrl", "alt", "shift"] as const;
const NAMES: Record<string, string> = { "⌘": "cmd", command: "cmd", meta: "cmd", control: "ctrl", "⌃": "ctrl", option: "alt", "⌥": "alt", "⇧": "shift" };

/** "cmd+k" as its parts, or null when it names no key. */
export function parseCombo(text: string): { mods: Set<string>; key: string } | null {
  if (text.trim().toLowerCase() === "none") return null;
  const lower = text.toLowerCase();
  // A key that is itself "+" comes last, after a "+".
  const plus = lower.endsWith("++");
  const parts = (plus ? lower.slice(0, -2) : lower).split("+").filter((part, at, all) => part !== "" || at === all.length - 1).map((part) => NAMES[part.trim()] ?? part.trim());
  const key = plus ? "+" : (parts.pop() ?? "");
  if (key === "" || parts.some((part) => !(MODS as readonly string[]).includes(part))) return null;
  return { mods: new Set(parts), key };
}

function normalCombo(text: string): string {
  const parsed = parseCombo(text)!;
  return [...MODS.filter((mod) => parsed.mods.has(mod)), parsed.key].join("+");
}

/** The action a key press is bound to, if any. */
export function actionOf(event: KeyboardEvent, keys: Record<KeyAction, string>): KeyAction | null {
  const key = event.key.toLowerCase();
  // ⌥ and ⇧ change the character a key types; its code names the key itself.
  const code = /^Key([A-Z])$/.exec(event.code)?.[1]?.toLowerCase() ?? /^Digit(\d)$/.exec(event.code)?.[1] ?? null;
  for (const [action] of KEY_ACTIONS) {
    const combo = parseCombo(keys[action]);
    if (!combo) continue;
    const want = (mod: string, held: boolean) => combo.mods.has(mod) === held;
    if ((combo.key === key || combo.key === code) && want("cmd", event.metaKey) && want("ctrl", event.ctrlKey) && want("alt", event.altKey) && want("shift", event.shiftKey)) return action;
  }
  return null;
}

/** The key combination a key press makes, "cmd+shift+h", or null for a modifier alone or a plain key. */
export function comboOf(event: { key: string; code: string; metaKey: boolean; ctrlKey: boolean; altKey: boolean; shiftKey: boolean }): string | null {
  const key = event.key.toLowerCase();
  if (["meta", "control", "alt", "shift", "capslock", "fn", "dead", "unidentified"].includes(key)) return null;
  const mods = [event.metaKey && "cmd", event.ctrlKey && "ctrl", event.altKey && "alt", event.shiftKey && "shift"].filter(Boolean) as string[];
  const fkey = /^f\d{1,2}$/.test(key);
  // A plain key types; a combination needs ⌘, ⌃ or ⌥ (⇧ alone types a capital).
  if (!fkey && !mods.some((mod) => mod !== "shift")) return null;
  // With ⌥ a Mac types another character: the key's own name is read from its code.
  const fromCode = /^Key([A-Z])$/.exec(event.code)?.[1]?.toLowerCase() ?? /^Digit(\d)$/.exec(event.code)?.[1];
  const name = event.altKey && fromCode ? fromCode : key === " " ? "space" : key;
  return [...mods, name].join("+");
}

/** "cmd+k" as the Mac writes it, "⌘K". */
export function keyName(combo: string): string {
  const parsed = parseCombo(combo);
  if (!parsed) return combo === "none" ? "" : combo;
  const marks: Record<string, string> = { cmd: "⌘", ctrl: "⌃", alt: "⌥", shift: "⇧" };
  // In the Mac's own order: ⌃ ⌥ ⇧ ⌘.
  const order = ["ctrl", "alt", "shift", "cmd"];
  const names: Record<string, string> = { arrowup: "↑", arrowdown: "↓", arrowleft: "←", arrowright: "→", enter: "↩", escape: "esc", backspace: "⌫", space: "Space", tab: "⇥" };
  const key = names[parsed.key] ?? (parsed.key.length === 1 ? parsed.key.toUpperCase() : parsed.key.toUpperCase());
  return `${order.filter((mod) => parsed.mods.has(mod)).map((mod) => marks[mod]).join("")}${key}`;
}

/** Where the learner was in one session. */
export type Place = {
  /** The words on the conversation's line, not yet sent. */
  draft: string;
  /** The words on the open task's line, not yet handed in. */
  answerDraft: string;
  /** Where the line was: in the open task, or in the conversation. */
  line: "task" | "talk";
  /** How far the lesson was scrolled from its foot, in pixels. 0 is at the foot. */
  fromFoot: number;
  /** Whether a lesson was on the surface when the window last saw it. */
  inLesson: boolean;
  /** The open task's words then, so a reopened lesson has it as a task again. */
  task: string | null;
  /** When the window last saw this course, ms since the epoch. */
  seenMs: number;
};

type Places = { by: Record<string, Place> };

const blankPlace: Place = { draft: "", answerDraft: "", line: "talk", fromFoot: 0, inLesson: false, task: null, seenMs: 0 };

let places: Places | null = null;

function allPlaces(): Places {
  places ??= { by: readJson<Places>(PLACE)?.by ?? {} };
  return places;
}

/** The key a session's place is kept under: its folder and its conversation, or the tab of a session not begun. */
export const placeKey = (folder: string, session: string | null, tab: string) => `${folder}#${session ?? tab}`;

export function placeOf(key: string): Place {
  return { ...blankPlace, ...(allPlaces().by[key] ?? {}) };
}

/** A session just begun has its conversation's id now: its place moves to it. */
export function movePlace(from: string, to: string): void {
  const all = allPlaces();
  if (!all.by[from] || all.by[to]) return;
  all.by[to] = all.by[from];
  delete all.by[from];
  flushPlaces();
}

let timer: number | null = null;

/** Keep a change to where the learner is. Written a moment later, so typing stays cheap. */
export function keepPlace(key: string, change: Partial<Place>): void {
  const all = allPlaces();
  all.by[key] = { ...placeOf(key), ...change, seenMs: Date.now() };
  // Kept for the last 200 sessions only.
  const keys = Object.keys(all.by);
  if (keys.length > 200) {
    keys.sort((a, b) => all.by[a].seenMs - all.by[b].seenMs);
    for (const old of keys.slice(0, keys.length - 200)) delete all.by[old];
  }
  if (timer !== null) window.clearTimeout(timer);
  timer = window.setTimeout(flushPlaces, 300);
}

export function flushPlaces(): void {
  if (timer !== null) window.clearTimeout(timer);
  timer = null;
  if (places) writeJson(PLACE, places);
}

// ---------------------------------------------------------------------------
// What the plan's reading said changed after a turn, so a reopened session
// shows its plan card again. It is the window's own reading of the plan,
// before and after, never the tutor's words; kept for the last 300 turns.

const CHANGES = "mg.changes";

/** The key of a turn's change: its session, the learner's words and the start of the reply. */
const changeKey = (session: string, learner: string, tutor: string) => `${session}\u241f${learner.trim()}\u241f${tutor.replace(/\s+/g, " ").trim().slice(0, 80)}`;

export function keepChange(session: string, learner: string, tutor: string, change: unknown): void {
  const all = readJson<Record<string, { at: number; change: unknown }>>(CHANGES) ?? {};
  all[changeKey(session, learner, tutor)] = { at: Date.now(), change };
  const keys = Object.keys(all);
  if (keys.length > 300) for (const old of keys.sort((a, b) => all[a].at - all[b].at).slice(0, keys.length - 300)) delete all[old];
  writeJson(CHANGES, all);
}

export function changeOf(session: string, learner: string, tutor: string): unknown {
  return readJson<Record<string, { at: number; change: unknown }>>(CHANGES)?.[changeKey(session, learner, tutor)]?.change ?? null;
}

/**
 * The runtime starts a new sitting after three hours without a message. The
 * window reopens on the session it was left on only within that time;
 * after it, it opens on Home, with the tabs as they were.
 */
export const SITTING_GAP_MS = 3 * 60 * 60 * 1000;

if (typeof window !== "undefined") window.addEventListener("beforeunload", flushPlaces);
