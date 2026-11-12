// The workspace: which documents are open in which pane, and everything the
// window must give back exactly as it was when the folder is opened again.
// It is kept per folder on this Mac, in the page's local storage. None of it
// is learning state.

/**
 * A document, by id:
 * - "lesson": the folder's lesson, the one Continue resumes or a new one;
 * - "lesson:<session>": an earlier lesson, to read;
 * - "plan": the folder's plan file;
 * - "file:<path>": any file under Study/, from the study root;
 * - "record", "settings", "setup".
 */
export type DocId = string;

export type Pane = { tabs: DocId[]; active: DocId };

export type Layout = {
  panes: Pane[];
  /** Which pane has the keyboard. */
  focus: number;
  sidebar: boolean;
  /** Scroll position of each document, by id. */
  scroll: Record<DocId, number>;
  /** The page a PDF was left at, by id. */
  pages: Record<DocId, number>;
  /** Unsent words, by writing place: "answer", "talk". */
  drafts: Record<string, string>;
  /** Plan sections folded or opened by the learner, by heading. */
  folds: Record<string, boolean>;
};

export const freshLayout = (): Layout => ({
  panes: [{ tabs: ["lesson"], active: "lesson" }],
  focus: 0,
  sidebar: false,
  scroll: {},
  pages: {},
  drafts: {},
  folds: {},
});

const key = (folder: string) => `mg.ws:${folder}`;

export function readLayout(folder: string): Layout {
  try {
    const raw = localStorage.getItem(key(folder));
    if (!raw) return freshLayout();
    const got = JSON.parse(raw) as Partial<Layout>;
    const panes = (got.panes ?? []).filter((pane) => Array.isArray(pane.tabs) && pane.tabs.length > 0).slice(0, 2);
    if (panes.length === 0) return freshLayout();
    return {
      panes: panes.map((pane) => ({ tabs: pane.tabs, active: pane.tabs.includes(pane.active) ? pane.active : pane.tabs[0] })),
      focus: Math.min(got.focus ?? 0, panes.length - 1),
      sidebar: Boolean(got.sidebar),
      scroll: got.scroll ?? {},
      pages: got.pages ?? {},
      drafts: got.drafts ?? {},
      folds: got.folds ?? {},
    };
  } catch {
    return freshLayout();
  }
}

let pending: { folder: string; layout: Layout } | null = null;
let timer: number | null = null;

/** Keep the layout, a moment later: typing and scrolling never wait for storage. */
export function saveLayout(folder: string, layout: Layout): void {
  pending = { folder, layout };
  if (timer !== null) return;
  timer = window.setTimeout(flushLayout, 350);
}

export function flushLayout(): void {
  if (timer !== null) window.clearTimeout(timer);
  timer = null;
  if (!pending) return;
  try {
    localStorage.setItem(key(pending.folder), JSON.stringify(pending.layout));
  } catch {
    // Nothing is kept in a private window.
  }
  pending = null;
}

window.addEventListener("beforeunload", flushLayout);
window.addEventListener("pagehide", flushLayout);

/** Open a document in a pane: its tab if it has one, else a new tab after the active one. */
export function openIn(layout: Layout, doc: DocId, pane = layout.focus): Layout {
  const panes = layout.panes.map((p) => ({ ...p, tabs: [...p.tabs] }));
  const index = Math.min(pane, panes.length - 1);
  const target = panes[index];
  if (!target.tabs.includes(doc)) target.tabs.splice(target.tabs.indexOf(target.active) + 1, 0, doc);
  target.active = doc;
  return { ...layout, panes, focus: index };
}

/** Show a document in the second pane, making one if there is none. */
export function openBeside(layout: Layout, doc: DocId): Layout {
  if (layout.panes.length === 1) {
    const first = layout.panes[0];
    // A document already in the first pane moves across, so it is not open twice.
    const tabs = first.tabs.filter((tab) => tab !== doc);
    const kept = tabs.length > 0 ? { tabs, active: tabs.includes(first.active) ? first.active : tabs[0] } : first;
    return { ...layout, panes: [kept, { tabs: [doc], active: doc }] };
  }
  const panes = layout.panes.map((p) => ({ ...p, tabs: [...p.tabs] }));
  const other = panes[1];
  if (!other.tabs.includes(doc)) other.tabs.push(doc);
  other.active = doc;
  // Not open twice.
  if (panes[0].tabs.includes(doc) && panes[0].tabs.length > 1) {
    panes[0].tabs = panes[0].tabs.filter((tab) => tab !== doc);
    if (panes[0].active === doc) panes[0].active = panes[0].tabs[0];
  }
  return { ...layout, panes };
}

/** Close a tab. The last tab of the second pane closes the pane; the last tab of the only pane leaves the lesson open. */
export function closeTab(layout: Layout, doc: DocId, pane: number): Layout {
  const panes = layout.panes.map((p) => ({ ...p, tabs: [...p.tabs] }));
  const target = panes[pane];
  if (!target) return layout;
  const at = target.tabs.indexOf(doc);
  if (at === -1) return layout;
  target.tabs.splice(at, 1);
  if (target.tabs.length === 0) {
    if (panes.length > 1) {
      panes.splice(pane, 1);
      return { ...layout, panes, focus: 0 };
    }
    target.tabs = ["lesson"];
  }
  if (target.active === doc) target.active = target.tabs[Math.max(0, at - 1)] ?? target.tabs[0];
  return { ...layout, panes };
}

/** Join two panes into one, or split one into two with the document that fits beside it. */
export function toggleSplit(layout: Layout, beside: DocId): Layout {
  if (layout.panes.length > 1) {
    const [a, b] = layout.panes;
    const tabs = [...a.tabs, ...b.tabs.filter((tab) => !a.tabs.includes(tab))];
    return { ...layout, panes: [{ tabs, active: layout.focus === 1 ? b.active : a.active }], focus: 0 };
  }
  return { ...openBeside(layout, beside), focus: 1 };
}

/** Every document open anywhere. */
export const openDocs = (layout: Layout) => new Set(layout.panes.flatMap((pane) => pane.tabs));

/** Where a document is open, or null. */
export function findDoc(layout: Layout, doc: DocId): number | null {
  const at = layout.panes.findIndex((pane) => pane.tabs.includes(doc));
  return at === -1 ? null : at;
}

/** The name a file goes by: its own name, without the folders above it. */
export function fileName(path: string): string {
  const name = path.split("/").pop() ?? path;
  return name;
}

/** The kind of a file under Study/, by where it is and what it is. */
export function fileKind(path: string): "plan" | "paper" | "material" | "handed-in" | "log" | "other" {
  if (/^Study\/[^/]+\/plan\.md$/.test(path)) return "plan";
  if (/^Study\/[^/]+\/papers\/[^/]+\.pdf$/i.test(path)) return "paper";
  if (path.startsWith("Study/materials/")) return "material";
  if (path.startsWith("Study/handed-in/")) return "handed-in";
  if (/^Study\/[^/]+\/(gap-log\.md|graph\.yaml)$/.test(path)) return "log";
  return "other";
}
