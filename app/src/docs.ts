// The documents open beside a session, or beside a workspace's page: which
// files, the one in front, and where each was left (its page and zoom, or how
// far down a note was read). They belong to the session, so a session opened
// again shows them as they were. Kept in the window's own storage on this Mac,
// beside each session's draft and scroll; the learner's record is not touched.

import { sourceRef } from "./sources";
import { materialTitle } from "./words";

/** How large a PDF's pages are drawn: its text as wide as the panel, or a share of the page's own size. */
export type Zoom = "fit" | number;

export type OpenDoc = {
  /** From the study folder, as the core reads it: "Study/materials/week5.pdf". */
  path: string;
  /** The page in view, from 1. Notes and pictures have one. */
  page: number;
  zoom: Zoom;
  /** How far down a note was read, 0 to 1. */
  scroll: number;
};

/**
 * What a session's panel holds: the plan first (sessions only), then the
 * documents, and the one shown (null: the plan). Expanded, the panel fills
 * the session; the learner can flip to the lesson and back.
 */
export type Docs = { open: OpenDoc[]; shown: string | null; expanded: boolean; lessonFront: boolean };

/** The panel's narrowest, and its widest as a share of the window. */
export const PANEL_MIN = 300;
export const PANEL_MAX_SHARE = 0.72;

/** The width a panel is drawn at: the one the learner chose, fitted to the room there is now. */
export function fitPanelWidth(width: number, room: number = typeof window === "undefined" ? 1200 : window.innerWidth): number {
  return Math.round(Math.min(Math.max(PANEL_MIN, width), Math.max(PANEL_MIN, room * PANEL_MAX_SHARE)));
}

export const ZOOMS = [0.5, 0.67, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3];

/** The next zoom step in or out from where a PDF is, "fit" being its text as wide as the panel (`fitScale`). */
export function stepZoom(zoom: Zoom, fitScale: number, by: 1 | -1): number {
  const now = zoom === "fit" ? fitScale : zoom;
  const next = by > 0 ? ZOOMS.find((step) => step > now + 0.001) : [...ZOOMS].reverse().find((step) => step < now - 0.001);
  return next ?? now;
}

/** A document's title: its file's name, with no folder and no extension. Never a path. */
export const docTitle = (path: string) => materialTitle(path.split("/").pop() ?? path);

const STORE = "mg.docs";
type Stored = { by: Record<string, Docs & { seenMs: number }> };

let all: Stored | null = null;
let timer: number | null = null;

function load(): Stored {
  if (all) return all;
  try {
    const raw = window.localStorage.getItem(STORE);
    const parsed = raw ? (JSON.parse(raw) as Stored) : null;
    all = parsed && typeof parsed.by === "object" && parsed.by !== null ? parsed : { by: {} };
  } catch {
    all = { by: {} };
  }
  return all;
}

function flush(): void {
  if (timer !== null) window.clearTimeout(timer);
  timer = null;
  try {
    if (all) window.localStorage.setItem(STORE, JSON.stringify(all));
  } catch {
    // A window that cannot keep them still shows them; they open closed next time.
  }
}

if (typeof window !== "undefined") window.addEventListener("pagehide", flush);

/** Keep only what makes sense, so a bad value never breaks the panel. */
function clean(raw: unknown): Docs {
  const given = (raw ?? {}) as Partial<Docs>;
  const open: OpenDoc[] = [];
  for (const doc of Array.isArray(given.open) ? given.open : []) {
    if (!doc || typeof doc.path !== "string" || !sourceRef(doc.path) || open.some((each) => each.path === doc.path)) continue;
    const zoom = doc.zoom === "fit" || (typeof doc.zoom === "number" && doc.zoom >= ZOOMS[0] && doc.zoom <= ZOOMS[ZOOMS.length - 1]) ? doc.zoom : "fit";
    open.push({
      path: doc.path,
      page: typeof doc.page === "number" && doc.page >= 1 ? Math.floor(doc.page) : 1,
      zoom,
      scroll: typeof doc.scroll === "number" && doc.scroll >= 0 && doc.scroll <= 1 ? doc.scroll : 0,
    });
  }
  const shown = typeof given.shown === "string" && open.some((doc) => doc.path === given.shown) ? given.shown : null;
  return { open, shown, expanded: given.expanded === true, lessonFront: given.lessonFront === true };
}

/** The key documents are kept under: a session's folder and conversation (or its tab, before it has one), or a workspace's page. */
export const sessionDocsKey = (folder: string, session: string | null, tab: string) => `session:${folder}#${session ?? tab}`;
export const workspaceDocsKey = (folder: string) => `workspace:${folder}`;

export function docsOf(key: string): Docs {
  return clean(load().by[key]);
}

export function keepDocs(key: string, docs: Docs): void {
  const store = load();
  if (docs.open.length === 0 && !docs.expanded) delete store.by[key];
  else store.by[key] = { ...docs, seenMs: Date.now() };
  // Kept for the last 200 sessions and pages only.
  const keys = Object.keys(store.by);
  if (keys.length > 200) {
    keys.sort((a, b) => store.by[a].seenMs - store.by[b].seenMs);
    for (const old of keys.slice(0, keys.length - 200)) delete store.by[old];
  }
  if (timer !== null) window.clearTimeout(timer);
  timer = window.setTimeout(flush, 300);
}

/** A session just begun has its conversation's id now: its documents move to it. */
export function moveDocs(from: string, to: string): void {
  const store = load();
  if (!store.by[from] || store.by[to]) return;
  store.by[to] = store.by[from];
  delete store.by[from];
  flush();
}

/** Open a document, or bring it to the front at the page asked for. */
export function withOpened(docs: Docs, path: string, page: number | null): Docs {
  const found = docs.open.find((doc) => doc.path === path);
  const open = found
    ? docs.open.map((doc) => (doc.path === path && page !== null ? { ...doc, page } : doc))
    : [...docs.open, { path, page: page ?? 1, zoom: "fit" as Zoom, scroll: 0 }];
  return { ...docs, open, shown: path, lessonFront: false };
}

/** Close a document; the one beside it comes to the front, or the plan. */
export function withClosed(docs: Docs, path: string): Docs {
  const at = docs.open.findIndex((doc) => doc.path === path);
  if (at === -1) return docs;
  const open = docs.open.filter((doc) => doc.path !== path);
  const shown = docs.shown === path ? (open[Math.min(at, open.length - 1)]?.path ?? null) : docs.shown;
  // With nothing left open, a panel that filled the page or session is beside it again next time.
  return open.length === 0 ? { ...docs, open, shown, expanded: false, lessonFront: false } : { ...docs, open, shown };
}

/** Where a document was left, as it moves. */
export function withPlace(docs: Docs, path: string, change: Partial<OpenDoc>): Docs {
  return { ...docs, open: docs.open.map((doc) => (doc.path === path ? { ...doc, ...change } : doc)) };
}
