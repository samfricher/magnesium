// What the learner set, and where they were, kept on this Mac in the
// window's own storage. None of it is learning state: the record keeps that.
//
// Settings bend the window to the learner: the face they read in, the face
// their own words are in, the size, the line length, the scheme and the ink.
// The memory is per course: the draft on the line, where the line was (in the
// task or in the conversation), how far the lesson was scrolled, and when the
// window last saw it.

/** A face named by the window, or any installed face by its name. */
export type Face = string;
export type Scheme = "system" | "light" | "dark";

/** What a key does. Each can be bound to any key with ⌘, ⌃, ⌥ or ⇧. */
export type KeyAction = "commands" | "courses" | "plan" | "material" | "work" | "settings" | "help" | "stop" | "new";

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
  ["commands", "everything (the command layer)"],
  ["courses", "courses"],
  ["plan", "the plan"],
  ["material", "material and papers"],
  ["work", "your work"],
  ["settings", "settings"],
  ["help", "help with the task"],
  ["stop", "stop the tutor's reply"],
  ["new", "a new lesson"],
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
  keys: { commands: "cmd+k", courses: "cmd+o", plan: "cmd+p", material: "cmd+e", work: "cmd+y", settings: "cmd+,", help: "cmd+/", stop: "cmd+.", new: "cmd+n" },
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
  for (const [action] of KEY_ACTIONS) {
    const combo = raw.keys?.[action];
    if (typeof combo === "string" && parseCombo(combo)) keys[action] = normalCombo(combo);
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
  const parts = text.toLowerCase().split("+").map((part) => NAMES[part.trim()] ?? part.trim());
  const key = parts.pop() ?? "";
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
  for (const [action] of KEY_ACTIONS) {
    const combo = parseCombo(keys[action]);
    if (!combo) continue;
    const want = (mod: string, held: boolean) => combo.mods.has(mod) === held;
    if (combo.key === key && want("cmd", event.metaKey) && want("ctrl", event.ctrlKey) && want("alt", event.altKey) && want("shift", event.shiftKey)) return action;
  }
  return null;
}

/** "cmd+k" as the Mac writes it, "⌘K". */
export function keyName(combo: string): string {
  const parsed = parseCombo(combo);
  if (!parsed) return combo;
  const marks: Record<string, string> = { cmd: "⌘", ctrl: "⌃", alt: "⌥", shift: "⇧" };
  return `${MODS.filter((mod) => parsed.mods.has(mod)).map((mod) => marks[mod]).join("")}${parsed.key.length === 1 ? parsed.key.toUpperCase() : parsed.key}`;
}

// ---------------------------------------------------------------------------
// Settings as text: what ⌘, opens to edit by hand, as Ghostty or Sublime do.

const FIELDS: [keyof Prefs, string][] = [
  ["face", "reading-font"],
  ["voice", "your-font"],
  ["size", "size"],
  ["measure", "line-length"],
  ["scheme", "scheme"],
  ["ink", "ink"],
  ["inkDark", "ink-dark"],
  ["ground", "ground"],
  ["groundDark", "ground-dark"],
  ["text", "text"],
  ["textDark", "text-dark"],
  ["hints", "key-hints"],
];

/** The settings as the text the learner edits. */
export function prefsToText(prefs: Prefs): string {
  const lines = [
    "# Magnesium's window, as you set it. One setting a line: name = value.",
    "# It is kept on this Mac and applied as you type. Esc closes it.",
    "#",
    "# reading-font and your-font: charter, iowan, newyork, system, mono, reading",
    "#   (your-font only), or any font on this Mac by its name.",
    "# size: 12 to 28. line-length: characters, 40 to 110.",
    "# scheme: system, light or dark. key-hints: on or off.",
    "# ink, ground and text, and their -dark twins: a colour (#2f3f86, rgb(...),",
    "#   a CSS name) or an ink: blue-black, green-black, violet, graphite.",
    "#   Leave ground and text empty for the window's own.",
    "",
  ];
  for (const [field, name] of FIELDS) {
    const value = prefs[field];
    lines.push(`${name} = ${typeof value === "boolean" ? (value ? "on" : "off") : String(value)}`);
  }
  lines.push("", "# Keys: key.<what> = a key with cmd, ctrl, alt or shift, such as cmd+shift+p.");
  for (const [action, what] of KEY_ACTIONS) lines.push(`key.${action} = ${prefs.keys[action]}`.padEnd(28) + `# ${what}`);
  return `${lines.join("\n")}\n`;
}

/** Read the text back. A line that cannot be read is named in `problems` and left as it was. */
export function textToPrefs(text: string, base: Prefs): { prefs: Prefs; problems: string[] } {
  const raw: Partial<Prefs> & Record<string, unknown> = { ...base, keys: { ...base.keys } };
  const problems: string[] = [];
  text.split("\n").forEach((line, at) => {
    const bare = line.replace(/#.*$/, "").trim();
    if (bare === "") return;
    const found = /^([a-z.-]+)\s*=\s*(.*)$/i.exec(bare);
    if (!found) return void problems.push(`line ${at + 1} has no "name = value"`);
    const name = found[1].toLowerCase();
    const value = found[2].trim();
    if (name.startsWith("key.")) {
      const action = name.slice(4) as KeyAction;
      if (!KEY_ACTIONS.some(([known]) => known === action)) return void problems.push(`line ${at + 1}: no key is called ${name}`);
      if (!parseCombo(value)) return void problems.push(`line ${at + 1}: ${value} is not a key`);
      raw.keys![action] = value;
      return;
    }
    const field = FIELDS.find(([, known]) => known === name)?.[0];
    if (!field) return void problems.push(`line ${at + 1}: no setting is called ${name}`);
    if (field === "size" || field === "measure") {
      const number = Number(value);
      if (!Number.isFinite(number)) return void problems.push(`line ${at + 1}: ${name} wants a number`);
      raw[field] = number;
    } else if (field === "hints") raw.hints = !/^(off|no|false|0)$/i.test(value);
    else raw[field] = value as never;
  });
  return { prefs: cleanPrefs(raw), problems };
}

/** Where the learner was in one course. */
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
  /** When the window last saw this course, ms since the epoch. */
  seenMs: number;
};

type Places = { course: string | null; by: Record<string, Place> };

const blankPlace: Place = { draft: "", answerDraft: "", line: "talk", fromFoot: 0, inLesson: false, seenMs: 0 };

let places: Places | null = null;

function allPlaces(): Places {
  places ??= readJson<Places>(PLACE) ?? { course: null, by: {} };
  return places;
}

/** The course the window last had open. */
export function lastCourse(): string | null {
  return allPlaces().course;
}

export function placeOf(folder: string): Place {
  return { ...blankPlace, ...(allPlaces().by[folder] ?? {}) };
}

let timer: number | null = null;

/** Keep a change to where the learner is. Written a moment later, so typing stays cheap. */
export function keepPlace(folder: string, change: Partial<Place>): void {
  const all = allPlaces();
  all.course = folder;
  all.by[folder] = { ...placeOf(folder), ...change, seenMs: Date.now() };
  if (timer !== null) window.clearTimeout(timer);
  timer = window.setTimeout(flushPlaces, 300);
}

export function flushPlaces(): void {
  if (timer !== null) window.clearTimeout(timer);
  timer = null;
  if (places) writeJson(PLACE, places);
}

/**
 * The runtime starts a new sitting after three hours without a message. The
 * window reopens inside a lesson only within that time, and only when the
 * record agrees the lesson is from today.
 */
export const SITTING_GAP_MS = 3 * 60 * 60 * 1000;

if (typeof window !== "undefined") window.addEventListener("beforeunload", flushPlaces);
