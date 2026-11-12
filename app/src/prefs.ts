// The learner's own settings: the face and size the documents are set in,
// their width, the scheme, and the ink their own writing is in. Kept on this
// Mac only, in the page's local storage, and applied before the first paint.

export type FontId = "charter" | "newyork" | "iowan" | "sans" | "atkinson" | "mono";
export type Ink = "blue" | "green" | "violet" | "graphite";
export type SchemeChoice = "system" | "dark" | "light";
export type Measure = 58 | 66 | 80;

export type Prefs = { font: FontId; size: number; measure: Measure; scheme: SchemeChoice; ink: Ink };

export const FONTS: { id: FontId; name: string; stack: string }[] = [
  { id: "charter", name: "Charter", stack: 'Charter, "Bitstream Charter", "Iowan Old Style", Georgia, serif' },
  { id: "newyork", name: "New York", stack: '"New York", ui-serif, Georgia, serif' },
  { id: "iowan", name: "Iowan Old Style", stack: '"Iowan Old Style", Palatino, Georgia, serif' },
  { id: "sans", name: "SF Pro", stack: '-apple-system, BlinkMacSystemFont, "Helvetica Neue", sans-serif' },
  { id: "atkinson", name: "Atkinson Hyperlegible", stack: '"Atkinson Hyperlegible Next", -apple-system, sans-serif' },
  { id: "mono", name: "SF Mono", stack: 'ui-monospace, "SF Mono", Menlo, monospace' },
];

export const INKS: { id: Ink; name: string }[] = [
  { id: "blue", name: "Blue-black" },
  { id: "green", name: "Green-black" },
  { id: "violet", name: "Violet" },
  { id: "graphite", name: "Graphite" },
];

export const MEASURES: { id: Measure; name: string }[] = [
  { id: 58, name: "Narrow" },
  { id: 66, name: "Medium" },
  { id: 80, name: "Wide" },
];

export const SCHEMES: { id: SchemeChoice; name: string }[] = [
  { id: "system", name: "Same as macOS" },
  { id: "light", name: "Light" },
  { id: "dark", name: "Dark" },
];

export const SIZE_MIN = 14;
export const SIZE_MAX = 22;
const DEFAULTS: Prefs = { font: "charter", size: 17, measure: 66, scheme: "system", ink: "blue" };
const KEY = "mg.prefs";

export function readPrefs(): Prefs {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return { ...DEFAULTS };
    const got = JSON.parse(raw) as Partial<Prefs>;
    return {
      font: FONTS.some((f) => f.id === got.font) ? (got.font as FontId) : DEFAULTS.font,
      size: typeof got.size === "number" ? Math.min(SIZE_MAX, Math.max(SIZE_MIN, Math.round(got.size))) : DEFAULTS.size,
      measure: MEASURES.some((m) => m.id === got.measure) ? (got.measure as Measure) : DEFAULTS.measure,
      scheme: SCHEMES.some((s) => s.id === got.scheme) ? (got.scheme as SchemeChoice) : DEFAULTS.scheme,
      ink: INKS.some((i) => i.id === got.ink) ? (got.ink as Ink) : DEFAULTS.ink,
    };
  } catch {
    return { ...DEFAULTS };
  }
}

export function savePrefs(prefs: Prefs): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(prefs));
  } catch {
    // A private window keeps nothing; the settings hold for this session.
  }
}

/** Put the settings on the page. A scheme the learner chose wins; "system" leaves it to macOS, or to the harness. */
export function applyPrefs(prefs: Prefs, previous?: Prefs): void {
  const root = document.documentElement;
  const font = FONTS.find((f) => f.id === prefs.font) ?? FONTS[0];
  root.style.setProperty("--doc-font", font.stack);
  root.style.setProperty("--doc-size", `${prefs.size}px`);
  root.style.setProperty("--measure", `${prefs.measure}ch`);
  root.dataset.ink = prefs.ink;
  root.dataset.font = prefs.font;
  if (prefs.scheme !== "system") root.dataset.scheme = prefs.scheme;
  else if (previous && previous.scheme !== "system") delete root.dataset.scheme;
}
