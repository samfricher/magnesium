export type Face = string;
export type Scheme = "system" | "light" | "dark";

export type KeyAction =
  | "home"
  | "commands"
  | "new-tab"
  | "new-session"
  | "just-checks"
  | "open-workspace"
  | "close-session"
  | "attach"
  | "settings"
  | "plan"
  | "stop-reply"
  | "hint"
  | "lost"
  | "unknown"
  | "look-back"
  | "symbols"
  | "model"
  | "bigger"
  | "smaller"
  | "actual-size"
  | "expand-panel"
  | "flip-panel"
  | "close-document"
  | "open-document"
  | "zoom-in-document"
  | "zoom-out-document"
  | "zoom-fit-document"
  | "share-document";

export type Prefs = {
  face: Face;
  voice: Face;
  size: number;
  measure: number;
  scheme: Scheme;
  ink: string;
  inkDark: string;
  ground: string;
  groundDark: string;
  text: string;
  textDark: string;
  hints: boolean;
  line: LineLook;
  glass: number;
  keys: Record<KeyAction, string>;
};

export type LineLook = "bare" | "ruled" | "soft";
export const LINE_LOOKS: LineLook[] = ["bare", "ruled", "soft"];

export const KEY_ACTIONS: [KeyAction, string][] = [
  ["home", "Home"],
  ["new-tab", "New tab"],
  ["new-session", "New session"],
  ["just-checks", "Just the quick checks"],
  ["open-workspace", "Open a folder"],
  ["close-session", "Close session"],
  ["attach", "Attach a photo or PDF"],
  ["plan", "Show or hide the plan"],
  ["expand-panel", "Expand panel"],
  ["flip-panel", "Switch to the document"],
  ["open-document", "Open document"],
  ["close-document", "Close document"],
  ["zoom-in-document", "Zoom in on the document"],
  ["zoom-out-document", "Zoom out of the document"],
  ["zoom-fit-document", "Fit the document to the panel"],
  ["share-document", "Share document"],
  ["settings", "Settings"],
  ["commands", "Command menu"],
  ["hint", "Hint"],
  ["lost", "I’m lost"],
  ["unknown", "I don’t know"],
  ["look-back", "Show or hide the lesson"],
  ["symbols", "Symbols"],
  ["model", "Model and effort"],
  ["stop-reply", "Stop the reply"],
  ["bigger", "Bigger text"],
  ["smaller", "Smaller text"],
  ["actual-size", "Actual size"],
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
  line: "soft",
  glass: 0,
  keys: {
    home: "cmd+1",
    commands: "cmd+k",
    "new-tab": "cmd+t",
    "new-session": "cmd+n",
    "just-checks": "cmd+shift+n",
    "open-workspace": "cmd+o",
    "close-session": "cmd+w",
    attach: "cmd+u",
    settings: "cmd+,",
    plan: "cmd+p",
    "stop-reply": "cmd+.",
    hint: "cmd+shift+h",
    lost: "cmd+shift+l",
    unknown: "cmd+shift+d",
    "look-back": "cmd+arrowup",
    symbols: "cmd+shift+m",
    model: "cmd+shift+j",
    bigger: "cmd+=",
    smaller: "cmd+-",
    "actual-size": "cmd+0",
    "expand-panel": "cmd+shift+e",
    "flip-panel": "cmd+\\",
    "close-document": "cmd+shift+w",
    "open-document": "cmd+shift+o",
    "zoom-in-document": "cmd+alt+=",
    "zoom-out-document": "cmd+alt+-",
    "zoom-fit-document": "none",
    "share-document": "none",
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

export function faceCss(face: string, fallback = FACE_CSS.charter): string {
  const named = FACE_CSS[face.trim().toLowerCase()];
  if (named) return named;
  const name = face.trim().replace(/["\;{}]/g, "");
  return name === "" ? fallback : `"${name}", ${fallback}`;
}

const PREFS = "mg.prefs.2";
const OLD_KEYS: Record<string, KeyAction> = { stop: "stop-reply", new: "new-session", courses: "open-workspace" };
const PLACE = "mg.place";
const PANEL = "mg.panel";

export function panelWidthKept(): number {
  const kept = readJson<{ width?: number }>(PANEL)?.width;
  const plan = Math.round(15.5 * (parseFloat(getComputedStyle(document.documentElement).fontSize) || 17));
  return typeof kept === "number" && kept >= 200 && kept <= 4000 ? kept : plan;
}

export function keepPanelWidth(width: number): void {
  writeJson(PANEL, { width });
}

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
  }
}

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
    line: LINE_LOOKS.includes(raw.line as LineLook) ? (raw.line as LineLook) : DEFAULTS.line,
    glass: num(raw.glass, -10, 10, DEFAULTS.glass),
    keys,
  };
}

export function loadPrefs(): Prefs {
  return cleanPrefs(readJson<Partial<Prefs>>(PREFS) ?? {});
}

export function savePrefs(prefs: Prefs): void {
  writeJson(PREFS, prefs);
}

function colour(value: string, dark: boolean): string | null {
  const named = INKS[value.toLowerCase()];
  if (named) return named[dark ? 1 : 0];
  if (value === "" || !CSS.supports("color", value)) return null;
  return value;
}

export function applyPrefs(prefs: Prefs, schemeChanged = false): void {
  const root = document.documentElement;
  const set = (name: string, value: string | null) => (value ? root.style.setProperty(name, value) : root.style.removeProperty(name));
  set("--read", faceCss(prefs.face));
  const voice = prefs.voice.trim().toLowerCase();
  set("--you", voice === "reading" ? "var(--read)" : faceCss(prefs.voice, FACE_CSS.system));
  set("--size", `${prefs.size}px`);
  set("--measure", `${(prefs.measure * 0.47).toFixed(2)}em`);
  set("--measure-rem", `${(prefs.measure * 0.47).toFixed(2)}rem`);
  set("--ink-l", colour(prefs.ink, false));
  set("--ink-d", colour(prefs.inkDark || prefs.ink, true));
  set("--ground-l", colour(prefs.ground, false));
  set("--ground-d", colour(prefs.groundDark, true));
  set("--text-l", colour(prefs.text, false));
  set("--text-d", colour(prefs.textDark, true));
  root.dataset.hints = prefs.hints ? "on" : "off";
  root.dataset.field = prefs.line;
  if (prefs.scheme !== "system") root.dataset.scheme = prefs.scheme;
  else if (schemeChanged) delete root.dataset.scheme;
}

const MODS = ["cmd", "ctrl", "alt", "shift"] as const;
const NAMES: Record<string, string> = { "⌘": "cmd", command: "cmd", meta: "cmd", control: "ctrl", "⌃": "ctrl", option: "alt", "⌥": "alt", "⇧": "shift" };

export function parseCombo(text: string): { mods: Set<string>; key: string } | null {
  if (text.trim().toLowerCase() === "none") return null;
  const lower = text.toLowerCase();
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

export function actionOf(event: KeyboardEvent, keys: Record<KeyAction, string>): KeyAction | null {
  const key = event.key.toLowerCase();
  const code = /^Key([A-Z])$/.exec(event.code)?.[1]?.toLowerCase() ?? /^Digit(\d)$/.exec(event.code)?.[1] ?? null;
  for (const [action] of KEY_ACTIONS) {
    const combo = parseCombo(keys[action]);
    if (!combo) continue;
    const want = (mod: string, held: boolean) => combo.mods.has(mod) === held;
    if ((combo.key === key || combo.key === code) && want("cmd", event.metaKey) && want("ctrl", event.ctrlKey) && want("alt", event.altKey) && want("shift", event.shiftKey)) return action;
  }
  return null;
}

export function comboOf(event: { key: string; code: string; metaKey: boolean; ctrlKey: boolean; altKey: boolean; shiftKey: boolean }): string | null {
  const key = event.key.toLowerCase();
  if (["meta", "control", "alt", "shift", "capslock", "fn", "dead", "unidentified"].includes(key)) return null;
  const mods = [event.metaKey && "cmd", event.ctrlKey && "ctrl", event.altKey && "alt", event.shiftKey && "shift"].filter(Boolean) as string[];
  const fkey = /^f\d{1,2}$/.test(key);
  if (!fkey && !mods.some((mod) => mod !== "shift")) return null;
  const fromCode = /^Key([A-Z])$/.exec(event.code)?.[1]?.toLowerCase() ?? /^Digit(\d)$/.exec(event.code)?.[1];
  const name = event.altKey && fromCode ? fromCode : key === " " ? "space" : key;
  return [...mods, name].join("+");
}

export function keyName(combo: string): string {
  const parsed = parseCombo(combo);
  if (!parsed) return combo === "none" ? "" : combo;
  const marks: Record<string, string> = { cmd: "⌘", ctrl: "⌃", alt: "⌥", shift: "⇧" };
  const order = ["ctrl", "alt", "shift", "cmd"];
  const names: Record<string, string> = { arrowup: "↑", arrowdown: "↓", arrowleft: "←", arrowright: "→", enter: "↩", escape: "esc", backspace: "⌫", space: "Space", tab: "⇥" };
  const key = names[parsed.key] ?? (parsed.key.length === 1 ? parsed.key.toUpperCase() : parsed.key.toUpperCase());
  return `${order.filter((mod) => parsed.mods.has(mod)).map((mod) => marks[mod]).join("")}${key}`;
}

export type Place = {
  draft: string;
  answerDraft: string;
  line: "task" | "talk";
  fromFoot: number;
  inLesson: boolean;
  seenMs: number;
};

type Places = { by: Record<string, Place> };

const blankPlace: Place = { draft: "", answerDraft: "", line: "talk", fromFoot: 0, inLesson: false, seenMs: 0 };

let places: Places | null = null;

function allPlaces(): Places {
  places ??= { by: readJson<Places>(PLACE)?.by ?? {} };
  return places;
}

export const placeKey = (folder: string, session: string | null, tab: string) => `${folder}#${session ?? tab}`;

export function placeOf(key: string): Place {
  return { ...blankPlace, ...(allPlaces().by[key] ?? {}) };
}

export function movePlace(from: string, to: string): void {
  const all = allPlaces();
  if (!all.by[from] || all.by[to]) return;
  all.by[to] = all.by[from];
  delete all.by[from];
  flushPlaces();
}

let timer: number | null = null;

export function keepPlace(key: string, change: Partial<Place>): void {
  const all = allPlaces();
  all.by[key] = { ...placeOf(key), ...change, seenMs: Date.now() };
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

const CHANGES = "mg.changes";

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

export const SITTING_GAP_MS = 3 * 60 * 60 * 1000;

if (typeof window !== "undefined") window.addEventListener("beforeunload", flushPlaces);
