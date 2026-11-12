// The learner's settings are a plain-text file, as an editor's are. The
// window applies it as it is typed, keeps it in its own storage on this Mac
// (never in a study folder), and reads it again on the next open. Any font
// installed on this Mac can be named; colours are any CSS colour; every key
// can be bound again.

export type Action =
  | "open"
  | "commands"
  | "folders"
  | "settings"
  | "symbols"
  | "photos"
  | "hint"
  | "lost"
  | "unknown"
  | "sure"
  | "unsure"
  | "guess"
  | "new-lesson"
  | "close"
  | "stop-reply"
  | "split"
  | "sidebar"
  | "lesson"
  | "bigger"
  | "smaller"
  | "reset-size";

export const ACTIONS: { id: Action; key: string; what: string }[] = [
  { id: "open", key: "cmd+p", what: "open any document" },
  { id: "commands", key: "cmd+k", what: "every command, and every setting" },
  { id: "folders", key: "cmd+o", what: "another study folder" },
  { id: "new-lesson", key: "cmd+n", what: "a new lesson" },
  { id: "lesson", key: "cmd+l", what: "back to the lesson" },
  { id: "close", key: "cmd+w", what: "close the tab; closing the lesson keeps where you stopped" },
  { id: "stop-reply", key: "cmd+.", what: "stop the reply" },
  { id: "hint", key: "cmd+shift+h", what: "ask for a hint on the task" },
  { id: "lost", key: "cmd+shift+l", what: "say you are lost" },
  { id: "unknown", key: "cmd+shift+d", what: "say you don't know" },
  { id: "sure", key: "alt+1", what: "mark your answer sure" },
  { id: "unsure", key: "alt+2", what: "mark your answer unsure" },
  { id: "guess", key: "alt+3", what: "mark your answer a guess" },
  { id: "photos", key: "cmd+shift+a", what: "photos or a PDF of your working" },
  { id: "symbols", key: "cmd+shift+m", what: "maths symbols" },
  { id: "split", key: "cmd+\\", what: "two panes, or one" },
  { id: "sidebar", key: "cmd+b", what: "the folder's files" },
  { id: "settings", key: "cmd+,", what: "this file" },
  { id: "bigger", key: "cmd+=", what: "larger text" },
  { id: "smaller", key: "cmd+-", what: "smaller text" },
  { id: "reset-size", key: "cmd+0", what: "text at its usual size" },
];

export const NAMED_INKS: Record<string, { light: string; dark: string }> = {
  blue: { light: "#2c5e9e", dark: "#8db3e2" },
  green: { light: "#2f6b4f", dark: "#93c4a1" },
  violet: { light: "#5b4a9b", dark: "#b6a3e8" },
  graphite: { light: "#1f2226", dark: "#dad8d3" },
};

export const FONT_CHOICES = ["Charter", "New York", "Iowan Old Style", "SF Pro", "Atkinson Hyperlegible", "SF Mono", "Avenir Next", "Palatino", "Menlo"];
const STACKS: Record<string, string> = {
  "new york": '"New York", ui-serif',
  "sf pro": "-apple-system, BlinkMacSystemFont",
  "sf mono": 'ui-monospace, "SF Mono"',
  "atkinson hyperlegible": '"Atkinson Hyperlegible Next"',
};
const TOKENS = ["ground", "shelf", "text", "quiet", "rule", "ink", "change"];

export type Settings = {
  font: string;
  ownFont: string;
  size: number;
  width: number;
  scheme: "system" | "light" | "dark";
  ink: string;
  hints: boolean;
  colours: { light: Record<string, string>; dark: Record<string, string> };
  keys: Record<Action, string>;
};

export const DEFAULT_TEXT = `# Your settings. Change a line and the window follows as you type.
# Kept in the window's own storage on this Mac, never in a study folder.
# A line is "name = value"; a line starting with # is a note.

font = Charter          # any font on this Mac, by name
own font = Charter      # the face of your own words
size = 17               # 14 to 22
width = 66              # characters to a line
scheme = system         # system, light or dark
ink = blue              # blue, green, violet, graphite, or #2c5e9e
hints = on              # which key does what, in the status line

# Colours, for one scheme or both. Any CSS colour, such as:
# dark ground = #101214
# light text = #111111
# (ground, shelf, text, quiet, rule, ink, change)

# Keys. Write a key as cmd+shift+h, ctrl+alt+x, or none.
${ACTIONS.map((action) => `key ${action.id} = ${action.key}`.padEnd(30) + `# ${action.what}`).join("\n")}
`;

const STORE = "mg.settings";

export function readText(): string {
  try {
    return localStorage.getItem(STORE) ?? DEFAULT_TEXT;
  } catch {
    return DEFAULT_TEXT;
  }
}

export function saveText(text: string): void {
  try {
    localStorage.setItem(STORE, text);
  } catch {
    // Held for this session only.
  }
}

/** Read the file. Lines that cannot be read are returned, each with why, and change nothing. */
export function parse(text: string): { settings: Settings; problems: string[] } {
  const settings: Settings = {
    font: "Charter",
    ownFont: "Charter",
    size: 17,
    width: 66,
    scheme: "system",
    ink: "blue",
    hints: true,
    colours: { light: {}, dark: {} },
    keys: Object.fromEntries(ACTIONS.map((a) => [a.id, a.key])) as Record<Action, string>,
  };
  const problems: string[] = [];
  text.split("\n").forEach((raw, n) => {
    const line = raw.replace(/\s+#\s.*$/, "").trim();
    if (line === "" || line.startsWith("#")) return;
    const eq = line.indexOf("=");
    if (eq === -1) {
      problems.push(`Line ${n + 1} has no "=".`);
      return;
    }
    const name = line.slice(0, eq).trim().toLowerCase().replace(/\s+/g, " ");
    const value = line.slice(eq + 1).trim();
    const number = Number(value);
    if (name === "font") settings.font = value;
    else if (name === "own font") settings.ownFont = value;
    else if (name === "size" && number >= 10 && number <= 32) settings.size = Math.round(number);
    else if (name === "width" && number >= 40 && number <= 120) settings.width = Math.round(number);
    else if (name === "scheme" && ["system", "light", "dark"].includes(value)) settings.scheme = value as Settings["scheme"];
    else if (name === "ink" && value !== "") settings.ink = value;
    else if (name === "hints" && ["on", "off"].includes(value)) settings.hints = value === "on";
    else if (name.startsWith("key ") && ACTIONS.some((a) => a.id === name.slice(4))) settings.keys[name.slice(4) as Action] = value.toLowerCase();
    else if (/^(light|dark) [a-z]+$/.test(name) && TOKENS.includes(name.split(" ")[1])) {
      const [scheme, token] = name.split(" ") as ["light" | "dark", string];
      settings.colours[scheme][token] = value;
    } else problems.push(`Line ${n + 1}: "${name}" is not a setting this window knows, or its value is out of range.`);
  });
  return { settings, problems };
}

/** Write one setting back into the file, in place when it is there, else at the end. */
export function setLine(text: string, name: string, value: string): string {
  const lines = text.split("\n");
  const at = lines.findIndex((line) => {
    const eq = line.indexOf("=");
    return eq !== -1 && !line.trim().startsWith("#") && line.slice(0, eq).trim().toLowerCase() === name;
  });
  if (at === -1) return `${text.replace(/\n*$/, "\n")}${name} = ${value}\n`;
  const comment = /\s+#\s.*$/.exec(lines[at])?.[0] ?? "";
  lines[at] = `${`${name} = ${value}`.padEnd(Math.max(name.length + value.length + 3, lines[at].length - comment.length))}${comment}`.trimEnd();
  return lines.join("\n");
}

const stack = (font: string, fallback: string) => {
  const known = STACKS[font.trim().toLowerCase()];
  return `${known ?? `"${font.replace(/"/g, "")}"`}, ${fallback}`;
};

/** Put the settings on the page: faces, size, width, scheme, ink, colours, hints. */
export function apply(settings: Settings): void {
  const root = document.documentElement;
  root.style.setProperty("--doc-font", stack(settings.font, 'Charter, "Iowan Old Style", Georgia, serif'));
  root.style.setProperty("--own-font", stack(settings.ownFont, "var(--doc-font)"));
  root.style.setProperty("--doc-size", `${settings.size}px`);
  root.style.setProperty("--measure", `${settings.width}ch`);
  // The column's width in pixels, so the tab names can start where the text does.
  root.style.setProperty("--col-px", `${Math.round(settings.width * settings.size * 0.56)}px`);
  if (settings.scheme !== "system") root.dataset.scheme = settings.scheme;
  else if (root.dataset.schemeFrom === "settings") delete root.dataset.scheme;
  root.dataset.schemeFrom = settings.scheme === "system" ? "" : "settings";
  root.dataset.hints = settings.hints ? "on" : "off";
  const named = NAMED_INKS[settings.ink.toLowerCase()];
  const ink = named ?? { light: settings.ink, dark: settings.ink };
  const block = (scheme: "light" | "dark") =>
    [
      `--ink: ${ink[scheme]}; --ink-soft: color-mix(in srgb, ${ink[scheme]} 13%, transparent);`,
      ...Object.entries(settings.colours[scheme]).map(([token, value]) => `--${token}: ${value};`),
    ].join(" ");
  const css = `:root { ${block("light")} }
@media (prefers-color-scheme: dark) { :root:not([data-scheme="light"]) { ${block("dark")} } }
:root[data-scheme="dark"] { ${block("dark")} }`;
  let style = document.getElementById("mg-settings") as HTMLStyleElement | null;
  if (!style) {
    style = document.createElement("style");
    style.id = "mg-settings";
    document.head.appendChild(style);
  }
  if (style.textContent !== css) style.textContent = css;
}

/** A key event as the file writes keys: "cmd+shift+h", "alt+1". */
export function comboOf(event: KeyboardEvent): string {
  const code = event.code;
  let key = event.key.toLowerCase();
  if (/^Key[A-Z]$/.test(code)) key = code.slice(3).toLowerCase();
  else if (/^Digit\d$/.test(code)) key = code.slice(5);
  else if (code === "Comma") key = ",";
  else if (code === "Period") key = ".";
  else if (code === "Backslash") key = "\\";
  else if (code === "Equal") key = "=";
  else if (code === "Minus") key = "-";
  const parts = [event.metaKey ? "cmd" : null, event.ctrlKey ? "ctrl" : null, event.altKey ? "alt" : null, event.shiftKey ? "shift" : null, key];
  return parts.filter(Boolean).join("+");
}

/** The action a key event is bound to, if any. "+" is read as "=", so cmd++ makes the text larger too. */
export function actionOf(settings: Settings, event: KeyboardEvent): Action | null {
  const combo = comboOf(event);
  for (const action of ACTIONS) {
    const bound = settings.keys[action.id];
    if (!bound || bound === "none") continue;
    if (bound === combo || (action.id === "bigger" && combo === "cmd+shift+=")) return action.id;
  }
  return null;
}
