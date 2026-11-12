// What the learner set, and where they were, kept on this Mac in the
// window's own storage. None of it is learning state: the record keeps that.
//
// Settings bend the window to the learner: the face they read in, the face
// their own words are in, the size, the line length, the scheme and the ink.
// The memory is per course: the draft on the line, where the line was (in the
// task or in the conversation), how far the lesson was scrolled, and when the
// window last saw it.

export type Face = "charter" | "iowan" | "newyork" | "system" | "mono";
export type Voice = "mono" | "reading";
export type Size = 15 | 17 | 19 | 21;
export type Measure = "narrow" | "medium" | "wide";
export type Scheme = "system" | "light" | "dark";
export type Ink = "blue" | "green" | "violet" | "graphite";

export type Prefs = { face: Face; voice: Voice; size: Size; measure: Measure; scheme: Scheme; ink: Ink };

export const DEFAULTS: Prefs = { face: "charter", voice: "mono", size: 17, measure: "medium", scheme: "system", ink: "blue" };

export const FACES: [Face, string][] = [
  ["charter", "Charter"],
  ["iowan", "Iowan Old Style"],
  ["newyork", "New York"],
  ["system", "System"],
  ["mono", "Mono"],
];
export const VOICES: [Voice, string][] = [
  ["mono", "Mono"],
  ["reading", "Same as reading"],
];
export const SIZES: Size[] = [15, 17, 19, 21];
export const MEASURES: [Measure, string][] = [
  ["narrow", "Narrow"],
  ["medium", "Medium"],
  ["wide", "Wide"],
];
export const SCHEMES: [Scheme, string][] = [
  ["system", "Like the Mac"],
  ["light", "Light"],
  ["dark", "Dark"],
];
export const INKS: [Ink, string][] = [
  ["blue", "Blue-black"],
  ["green", "Green-black"],
  ["violet", "Violet"],
  ["graphite", "Graphite"],
];

const PREFS = "mg.prefs";
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

function pick<T>(value: unknown, allowed: readonly T[], fallback: T): T {
  return allowed.includes(value as T) ? (value as T) : fallback;
}

export function loadPrefs(): Prefs {
  const saved = readJson<Partial<Prefs>>(PREFS) ?? {};
  return {
    face: pick(saved.face, FACES.map(([f]) => f), DEFAULTS.face),
    voice: pick(saved.voice, VOICES.map(([v]) => v), DEFAULTS.voice),
    size: pick(saved.size, SIZES, DEFAULTS.size),
    measure: pick(saved.measure, MEASURES.map(([m]) => m), DEFAULTS.measure),
    scheme: pick(saved.scheme, SCHEMES.map(([s]) => s), DEFAULTS.scheme),
    ink: pick(saved.ink, INKS.map(([i]) => i), DEFAULTS.ink),
  };
}

/**
 * Put the settings on the page. The scheme is only written when the learner
 * chose one: "Like the Mac" leaves `data-scheme` alone, so a scheme set from
 * outside (the evidence harness) stands.
 */
export function applyPrefs(prefs: Prefs, schemeChanged = false): void {
  const root = document.documentElement;
  root.dataset.face = prefs.face;
  root.dataset.voice = prefs.voice;
  root.dataset.measure = prefs.measure;
  root.dataset.ink = prefs.ink;
  root.style.setProperty("--size", `${prefs.size}px`);
  if (prefs.scheme !== "system") root.dataset.scheme = prefs.scheme;
  else if (schemeChanged) delete root.dataset.scheme;
}

export function savePrefs(prefs: Prefs): void {
  writeJson(PREFS, prefs);
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
