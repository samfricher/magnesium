// The panel beside a session (or a workspace's page): the plan, and the
// documents open in this session, one in front. A switcher at its top names
// each; a document closes from it. The panel is dragged wider by its left
// edge, hidden with the tab bar's switch, or expanded to fill the session,
// and a key flips between the lesson and the document.

import { memo, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";

import { core } from "../bridge";
import { docTitle, fitPanelWidth, PANEL_MIN, withClosed, withOpened, withPlace, type Docs, type OpenDoc } from "../docs";
import { sourceRef } from "../sources";
import { materialKind, materialTitle } from "../words";
import { DocView, type DocHandle } from "./DocView";
import { Layer, type Row } from "./Layer";


type Props = {
  /** The plan's own view, first in the switcher; null where there is no plan (a workspace's page). */
  plan: ReactNode | null;
  docs: Docs;
  folder: string;
  /** Bumped when a link asks for a document's page, so it goes there. */
  goTo: number;
  width: number;
  expanded: boolean;
  /** The panel is the thing in front: it fills the session, or the learner flipped to it. */
  onDocs: (docs: Docs) => void;
  onWidth: (width: number) => void;
  onExpand: (expanded: boolean) => void;
  onNote: (note: string) => void;
  /** The panel's document handle, for the window's commands (zoom, focus). */
  handle?: (handle: DocHandle | null) => void;
  /** The list of the folder's material to open from is showing (its control, or File > Open Document…). */
  picking: boolean;
  onPicking: (picking: boolean) => void;
  id?: string;
};

export const SidePanel = memo(function SidePanel(props: Props) {
  const { docs, onDocs } = props;
  const shown: OpenDoc | null = docs.open.find((doc) => doc.path === docs.shown) ?? (props.plan === null ? (docs.open[0] ?? null) : null);
  const [kind, setKind] = useState<{ pages: number | null; zoom: number | null; refused: boolean }>({ pages: null, zoom: null, refused: false });
  const view = useRef<DocHandle>(null);
  const box = useRef<HTMLElement>(null);

  useEffect(() => {
    props.handle?.(shown ? view.current : null);
  });

  const onKind = useCallback((found: { pages: number | null; zoom: number | null; refused: boolean }) => setKind(found), []);
  // Where a document was left is written on the newest documents, and only while it is still open:
  // a save that lands after a switch or a close does not bring the old state back.
  const docsNow = useRef(docs);
  docsNow.current = docs;
  const placeOf = useCallback(
    (path: string) => (change: Partial<OpenDoc>) => {
      const latest = docsNow.current;
      if (latest.open.some((doc) => doc.path === path)) onDocs(withPlace(latest, path, change));
    },
    [onDocs],
  );
  const onPlace = useMemo(() => (shown ? placeOf(shown.path) : () => {}), [shown?.path, placeOf]);

  // The left edge is dragged to make the panel wider or narrower; ← and → do the same from the keyboard.
  const drag = useRef<{ x: number; width: number } | null>(null);
  const limit = (width: number) => fitPanelWidth(width, box.current?.parentElement?.clientWidth ?? window.innerWidth);
  const onGripDown = (event: React.PointerEvent<HTMLDivElement>) => {
    if (props.expanded || event.button !== 0) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = { x: event.clientX, width: props.width };
    document.documentElement.dataset.dragging = "panel";
  };
  const onGripMove = (event: React.PointerEvent<HTMLDivElement>) => {
    if (!drag.current) return;
    props.onWidth(limit(drag.current.width + (drag.current.x - event.clientX)));
  };
  const onGripUp = () => {
    drag.current = null;
    delete document.documentElement.dataset.dragging;
  };
  const onGripKey = (event: React.KeyboardEvent) => {
    const by = event.key === "ArrowLeft" ? 24 : event.key === "ArrowRight" ? -24 : 0;
    if (by === 0) return;
    event.preventDefault();
    props.onWidth(limit(props.width + by));
  };

  const pick = (path: string | null) => onDocs({ ...docs, shown: path });

  // The folder's material, to open beside the lesson: by title, never a path; an answer key is never listed.
  // What can be opened: the folder's past papers, then its material, each by its title; an answer key is never
  // listed (the runtime leaves keys out of both), and the core refuses one besides.
  type Openable = { path: string; title: string; kind: string };
  const [openable, setOpenable] = useState<Openable[] | null>(null);
  useEffect(() => {
    if (!props.picking) return;
    let alive = true;
    setOpenable(null);
    void core()
      .then(async (api) => {
        const [home, material] = await Promise.all([api.home(props.folder).catch(() => null), api.materialList(props.folder).catch(() => null)]);
        const papers: Openable[] = (home?.papers ?? []).filter((paper) => sourceRef(paper.path)).map((paper) => ({ path: paper.path, title: paper.name, kind: "paper" }));
        const notes: Openable[] = (material?.names ?? [])
          .map((name) => `Study/materials/${name}`)
          .filter((path) => sourceRef(path) !== null && !papers.some((paper) => paper.path === path))
          .map((path) => ({ path, title: materialTitle(path.slice("Study/materials/".length)), kind: materialKind(path) }));
        if (alive) setOpenable([...papers, ...notes]);
      })
      .catch(() => alive && setOpenable([]));
    return () => {
      alive = false;
    };
  }, [props.picking, props.folder]);
  const openRows: Row[] = (openable ?? []).map((item) => {
    const open = docs.open.some((doc) => doc.path === item.path);
    return {
      id: item.path,
      kind: "thing",
      label: item.title,
      text: `${item.title} ${item.kind}`,
      hint: open ? "open" : item.kind,
      drive: `open-${item.title}`,
      run: () => {
        props.onPicking(false);
        onDocs(withOpened(docs, item.path, null));
        window.setTimeout(() => view.current?.focus(), 120);
      },
    };
  });
  const close = (path: string) => onDocs(withClosed(docs, path));
  const isPdf = shown !== null && kind.pages !== null;

  return (
    <>
    {props.picking && (
      <Layer
        name="Open a document"
        mode="filter"
        placeholder={openable === null ? "Reading the folder…" : openable.length === 0 ? "Nothing to open in this folder yet" : "A paper or a note"}
        rows={openRows}
        keys=""
        onClose={() => props.onPicking(false)}
      />
    )}
    <aside
      ref={box}
      className={`panel side${props.expanded ? " expanded" : ""}`}
      id={props.id}
      aria-label="Beside the lesson"
      data-role="side-panel"
      style={props.expanded ? undefined : { width: `${props.width}px` }}
    >
      {!props.expanded && (
        <div
          className="side-grip"
          role="separator"
          aria-orientation="vertical"
          aria-label="Panel width"
          aria-valuenow={props.width}
          aria-valuemin={PANEL_MIN}
          tabIndex={0}
          data-drive="panel-grip"
          onPointerDown={onGripDown}
          onPointerMove={onGripMove}
          onPointerUp={onGripUp}
          onPointerCancel={onGripUp}
          onKeyDown={onGripKey}
          onDoubleClick={() => props.onWidth(limit(PANEL_MIN + 28))}
        />
      )}
      <header className="side-head">
        <div className="side-switch" role="tablist" aria-label="Open in this session">
          {props.plan !== null && (
            <button type="button" role="tab" aria-selected={shown === null} className={`side-tab${shown === null ? " here" : ""}`} data-drive="side-plan" onClick={() => pick(null)}>
              Plan
            </button>
          )}
          {docs.open.map((doc) => {
            const here = doc.path === shown?.path;
            const title = docTitle(doc.path);
            return (
              <span key={doc.path} className={`side-tab doc${here ? " here" : ""}`}>
                <button type="button" role="tab" aria-selected={here} className="side-tab-name" title={title} data-drive={`doc-${title}`} onClick={() => pick(doc.path)}>
                  {title}
                </button>
                <button type="button" className="side-close" aria-label={`Close ${title}`} title="Close" data-drive={`close-doc-${title}`} onClick={() => close(doc.path)}>
                  ×
                </button>
              </span>
            );
          })}
          <button type="button" className="side-open" aria-label="Open a document beside the lesson" title="Open a document" data-drive="open-document" onClick={() => props.onPicking(true)}>
            Open…
          </button>
        </div>
        <div className="side-tools">
          {isPdf && (
            <span className="side-zoom" role="group" aria-label="Zoom">
              <button type="button" className="bar-act" aria-label="Zoom out" title="Zoom out" data-drive="zoom-out" onClick={() => view.current?.zoom(-1)}>
                −
              </button>
              <button type="button" className="bar-act zoom-now" title={shown?.zoom === "fit" ? "The text as wide as the panel" : "Back to the text as wide as the panel"} aria-label="Fit the text to the panel" data-drive="zoom-fit" onClick={() => view.current?.zoom(0)}>
                {`${Math.round((kind.zoom ?? 1) * 100)}%`}
              </button>
              <button type="button" className="bar-act" aria-label="Zoom in" title="Zoom in" data-drive="zoom-in" onClick={() => view.current?.zoom(1)}>
                +
              </button>
            </span>
          )}
          <button
            type="button"
            className="bar-act"
            data-drive="panel-expand"
            aria-pressed={props.expanded}
            title={props.expanded ? "Back beside the lesson" : "Fill the session"}
            onClick={() => props.onExpand(!props.expanded)}
          >
            {props.expanded ? "Beside" : "Expand"}
          </button>
        </div>
      </header>
      <div className="side-body">
        {shown ? (
          <DocView key={shown.path} ref={view} doc={shown} folder={props.folder} goTo={props.goTo} onPlace={onPlace} onNote={props.onNote} onKind={onKind} />
        ) : (
          props.plan
        )}
      </div>
    </aside>
    </>
  );
});
