// What the learner set, and where they left the window. Both are kept on
// this Mac, in the page's own storage, and nowhere else. Storage can be
// missing or refuse a write; the window then simply starts as it would the
// first time.

export type Face = "charter" | "iowan" | "newyork" | "sans" | "mono";
export type Scheme = "system" | "light" | "dark";
export type Prefs = { face: Face; size: number; scheme: Scheme; width: "narrow" | "wide" };

export const FACES: { id: Face; name: string }[] = [
  { id: "charter", name: "Charter" },
  { id: "iowan", name: "Iowan Old Style" },
  { id: "newyork", name: "New York" },
  { id: "sans", name: "San Francisco" },
  { id: "mono", name: "SF Mono" },
];
export const SIZES = [15, 17, 19, 21];
export const DEFAULTS: Prefs = { face: "charter", size: 17, scheme: "system", width: "narrow" };

const PREFS = "mg.prefs";
const LEFT = "mg.left";

function read<T>(key: string): Partial<T> | null {
  try {
    const text = window.localStorage.getItem(key);
    return text ? (JSON.parse(text) as Partial<T>) : null;
  } catch {
    return null;
  }
}

function write(key: string, value: unknown): void {
  try {
    window.localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Kept for this run only.
  }
}

export function loadPrefs(): Prefs {
  const saved = read<Prefs>(PREFS) ?? {};
  return {
    face: FACES.some((f) => f.id === saved.face) ? (saved.face as Face) : DEFAULTS.face,
    size: SIZES.includes(saved.size as number) ? (saved.size as number) : DEFAULTS.size,
    scheme: saved.scheme === "light" || saved.scheme === "dark" ? saved.scheme : "system",
    width: saved.width === "wide" ? "wide" : "narrow",
  };
}

/** Put the learner's choices on the page. A scheme set from outside (the harness) is left alone while the learner's is "system". */
export function applyPrefs(prefs: Prefs, save = true): void {
  const root = document.documentElement;
  root.dataset.face = prefs.face;
  root.dataset.width = prefs.width;
  root.style.setProperty("--size", `${prefs.size}px`);
  if (prefs.scheme === "system") {
    if (root.dataset.schemeBy === "learner") {
      delete root.dataset.scheme;
      delete root.dataset.schemeBy;
    }
  } else {
    root.dataset.scheme = prefs.scheme;
    root.dataset.schemeBy = "learner";
  }
  if (save) write(PREFS, prefs);
}

/** Where the learner left the window: the folder, the scroll, the half-written line, and whether the cursor was in the open task. */
export type Left = {
  folder: string | null;
  scroll: number;
  draft: string;
  place: "task" | "page";
  day: string;
  /** Plan changes noted today, kept so the agenda still says what moved after a restart. */
  moved: Record<string, string>;
  examWas: string | null;
};

export function loadLeft(day: string): Left {
  const saved = read<Left>(LEFT) ?? {};
  const sameDay = saved.day === day;
  return {
    folder: typeof saved.folder === "string" ? saved.folder : null,
    scroll: sameDay && typeof saved.scroll === "number" ? saved.scroll : 0,
    draft: typeof saved.draft === "string" ? saved.draft : "",
    place: saved.place === "page" ? "page" : "task",
    day,
    moved: sameDay && saved.moved && typeof saved.moved === "object" ? (saved.moved as Record<string, string>) : {},
    examWas: sameDay && typeof saved.examWas === "string" ? saved.examWas : null,
  };
}

let pending: number | null = null;
let latest: Left | null = null;

/** Keep where the learner is, a moment after it changes, and at once when the window is hidden. */
export function keepLeft(left: Left): void {
  latest = left;
  if (pending !== null) return;
  pending = window.setTimeout(() => {
    pending = null;
    if (latest) write(LEFT, latest);
  }, 400);
}

export function flushLeft(): void {
  if (pending !== null) window.clearTimeout(pending);
  pending = null;
  if (latest) write(LEFT, latest);
}
