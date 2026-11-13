// A workspace: one study folder's page, in the Home tab. Its sessions as a
// plain list (a name and when last opened) under a visible "New session",
// then what used to sit behind ⌘K: the folder's material, its papers, the
// record of the learner's marked answers, and the folder itself (take it off
// the list, upgrade it, set it up). File names are shown; paths are not.

import { useCallback, useEffect, useRef, useState } from "react";

import { docsOf, keepDocs, withClosed, withOpened, workspaceDocsKey, type Docs } from "../docs";
import type { DocHandle } from "../surface/DocView";
import { Line } from "../surface/Rich";
import { SidePanel } from "../surface/SidePanel";
import { sourceRef } from "../sources";
import type { Core, Home, LessonInfo, Mark, Materials, Paper } from "../types";
import { displayName, goalName, materialKind, materialTitle, namedByItsWords, sessionName, shortDay, whenOpened, withoutPaths } from "../words";

type Props = {
  api: Core;
  folder: string;
  /** The page is in front, with nothing over it. */
  focus: boolean;
  /** A session in this folder is running, so the folder cannot be taken off the list now. */
  running: boolean;
  onBack: () => void;
  onNewSession: (folder: string) => void;
  onOpenSession: (folder: string, session: string, title: string) => void;
  onHandedIn: (folder: string, words: { shown: string; sent: string }, title: string) => void;
  onForget: (folder: string) => void;
  onPaste: (folder: string) => void;
  /** The panel's width, in pixels, shared with the sessions'. */
  panelWidth: number;
  onPanelWidth: (width: number) => void;
  /** A document opened: a panel at the plan's width opens wider. */
  onShowPanel: () => void;
  /** One of the folder's sessions was named anew: a tab it is open in takes the name. */
  onSessionRenamed: (folder: string, session: string, title: string) => void;
  /** The documents changed: the menu follows. */
  onDocsChange: () => void;
  /** The page takes the panel's commands (expand, flip, close, zoom) while it is in front. */
  registerPanel: (run: ((command: string) => boolean) | null) => void;
  onNote: (text: string) => void;
  /** Bumped when the material changes from outside the page, as by a drop. */
  refresh: number;
};

/** A mark in a few words, for the record of past work. */
function markShort(mark: Mark): string {
  const help = mark.help === "hint" ? ", with a hint" : mark.help === "step" ? ", with a step shown" : mark.help === "solution" ? ", after the solution" : "";
  switch (mark.mark) {
    case "right":
      return help ? `right${help}` : mark.cold ? "right, on your own" : "right";
    case "partly":
      return `partly right${help}`;
    case "wrong":
      return "not right yet";
    case "declined":
      return "left for later";
    default:
      return mark.mark;
  }
}

export function WorkspacePage(props: Props) {
  const { api, folder } = props;
  const [home, setHome] = useState<Home | null>(null);
  const [sessions, setSessions] = useState<LessonInfo[] | null>(null);
  const [materials, setMaterials] = useState<Materials | null | "none">(null);
  const [marks, setMarks] = useState<Mark[] | null | "unread">("unread");
  const [busy, setBusy] = useState(false);

  // Material opens beside the page, as a document opens beside a lesson, and stays open with the page.
  const docsKey = workspaceDocsKey(folder);
  const [docs, setDocsState] = useState<Docs>(() => docsOf(docsKey));
  const [goTo, setGoTo] = useState(0);
  const [picking, setPicking] = useState(false);
  const docView = useRef<DocHandle | null>(null);
  const column = useRef<HTMLDivElement>(null);
  const pageRef = useRef<HTMLDivElement>(null);
  const docsNow = useRef(docs);
  docsNow.current = docs;
  const setDocs = useCallback(
    (next: Docs) => {
      setDocsState(next);
      docsNow.current = next;
      keepDocs(docsKey, next);
      props.onDocsChange();
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [docsKey],
  );
  const openMaterial = (path: string) => {
    if (!sourceRef(path)) {
      props.onNote("PDFs, notes and pictures open beside the page; this kind of file stays in the folder.");
      return;
    }
    setDocs(withOpened(docsNow.current, path, null));
    setGoTo((n) => n + 1);
    props.onShowPanel();
  };
  const panelRun = useRef<(command: string) => boolean>(() => false);
  panelRun.current = (command) => {
    const docs = docsNow.current;
    if (command === "open-document") {
      // The panel first: in front of the page if it fills it.
      props.onShowPanel();
      if (docs.expanded && docs.lessonFront) setDocs({ ...docs, lessonFront: false });
      setPicking(true);
      return true;
    }
    if (docs.open.length === 0) return false;
    switch (command) {
      case "escape": {
        // Out of the document, back to the page; from a document that fills the tab, back to the page.
        const inPanel = (document.activeElement as HTMLElement | null)?.closest(".panel.side");
        if (!inPanel && !(docs.expanded && !docs.lessonFront)) return false;
        if (docs.expanded && !docs.lessonFront) setDocs({ ...docs, lessonFront: true });
        (document.activeElement as HTMLElement | null)?.blur();
        return true;
      }
      case "expand-panel":
        setDocs({ ...docs, expanded: !docs.expanded || docs.lessonFront, lessonFront: false });
        return true;
      case "flip-panel":
        if (docs.expanded) setDocs({ ...docs, lessonFront: !docs.lessonFront });
        else if (column.current?.contains(document.activeElement) === false) column.current.querySelector<HTMLElement>("button")?.focus();
        else docView.current?.focus();
        return true;
      case "close-document":
        if (!docs.shown) return false;
        setDocs(withClosed(docs, docs.shown));
        return true;
      case "zoom-in-document":
      case "zoom-out-document":
      case "zoom-fit-document":
        docView.current?.zoom(command === "zoom-in-document" ? 1 : command === "zoom-out-document" ? -1 : 0);
        return true;
      case "share-document":
        // The panel's own control: the sheet points from it.
        pageRef.current?.querySelector<HTMLElement>('[data-drive="share-document"]')?.click();
        return true;
    }
    return false;
  };
  const { registerPanel } = props;
  useEffect(() => {
    registerPanel((command) => panelRun.current(command));
    return () => registerPanel(null);
  }, [registerPanel]);
  // The panel shows while material is open beside the page, or while its list is open to pick from.
  const withDocs = docs.open.length > 0 || picking;

  const read = useCallback(async () => {
    const [opened, listed, held] = await Promise.all([
      api.visitFolder(folder).catch(() => null),
      api.lessons(folder).catch(() => [] as LessonInfo[]),
      api.materialList(folder).catch(() => "none" as const),
    ]);
    setHome(opened);
    setSessions(listed);
    setMaterials(held);
  }, [api, folder]);

  useEffect(() => {
    void read();
  }, [read, props.refresh]);

  // A session still named by the words it was started with ("Continue") takes the goal the tutor set for it, as
  // a new one does. The record says which goal that is for the folder's current session only; older sessions
  // keep the names they have.
  const { onSessionRenamed } = props;
  const lastGoal = home?.last?.goal ?? null;
  useEffect(() => {
    const info = sessions?.find((each) => each.current);
    if (!info || !namedByItsWords(info)) return;
    const name = goalName(lastGoal, info.opening);
    if (!name || name === info.title) return;
    void api.renameSession(folder, info.session, name).then(
      () => {
        onSessionRenamed(folder, info.session, name);
        void read();
      },
      () => {},
    );
  }, [api, folder, sessions, lastGoal, onSessionRenamed, read]);

  const readMarks = () => {
    if (marks !== "unread") return;
    void api.marks(folder).then(
      (found) => setMarks(found),
      () => setMarks(null),
    );
  };

  const act = async (work: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await work();
    } catch (problem) {
      props.onNote(String(problem).replace(/^Error: /, ""));
    } finally {
      setBusy(false);
      void read();
    }
  };

  const addFiles = () =>
    act(async () => {
      const picked = await api.pickMaterial();
      if (picked.length === 0) return;
      const added = await api.addMaterial(picked, folder);
      added.lines.forEach(props.onNote);
    });

  const sit = (paper: Paper) =>
    act(async () => {
      await api.paperStart(paper, paper.minutes ?? home?.block_minutes ?? 50, folder);
    });

  const handIn = () =>
    act(async () => {
      const files = await api.pickFiles();
      if (files.length === 0) return;
      const name = home?.sitting?.name ?? "The paper";
      const words = await api.paperEnd(files, folder);
      if (words) props.onHandedIn(folder, words, name);
    });

  const endPaper = () => act(() => api.paperEnd([], folder));
  const upgrade = () =>
    act(async () => {
      setHome(await api.upgradeFolder(folder));
      props.onNote("This folder is upgraded. Magnesium kept a copy of its record from before.");
    });
  const setUp = () => act(async () => setHome(await api.setUp(folder)));

  const name = home ? displayName(home.name) : displayName(folder.split("/").pop() ?? "");
  // A kind beside each file only says something when the files are not all of one kind.
  const mixed = materials !== null && materials !== "none" && new Set(materials.names.map(materialKind)).size > 1;
  const sitting = home?.sitting ?? null;
  const ready = home !== null && ["ready", "empty", "not_set_up"].includes(home.state);
  const bySkill = new Map<string, Mark[]>();
  if (Array.isArray(marks)) {
    for (const mark of [...marks].reverse()) {
      if (mark.paper) continue;
      const label = mark.label.trim() || "Without a skill named";
      bySkill.set(label, [...(bySkill.get(label) ?? []), mark]);
    }
  }

  return (
    <div
      ref={pageRef}
      className={`page workspace-page${withDocs ? " with-docs" : ""}${withDocs && docs.expanded ? " expanded" : ""}${withDocs && docs.expanded && docs.lessonFront ? " lesson-front" : ""}`}
      data-page="workspace"
      style={withDocs ? ({ "--side-w": `${props.panelWidth}px` } as React.CSSProperties) : undefined}
    >
      <div className="page-scroll">
      <div className="page-column" ref={column}>
        <button className="back" data-drive="home" onClick={props.onBack}>
          ‹ Home
        </button>
        <header className="page-head">
          <h1 className="page-title">{name}</h1>
          {home?.message && home.state !== "ready" && <p className="where">{withoutPaths(home.message)}</p>}
        </header>

        <section aria-label="Sessions">
          <button className="new-session" data-drive="new-session" autoFocus={props.focus} disabled={!ready || sitting !== null} onClick={() => props.onNewSession(folder)}>
            New session
          </button>
          {sessions === null || sessions.length === 0 ? null : (
            <div className="list" role="list">
              {sessions.map((info) => {
                const title = sessionName(info, info.current ? (home?.last?.goal ?? null) : null);
                return (
                  <button key={info.session} role="listitem" className="home-row" onClick={() => props.onOpenSession(folder, info.session, title)}>
                    <span className="row-name">
                      <Line text={title} />
                    </span>
                    <span className="row-when">{whenOpened(info.opened_ms)}</span>
                  </button>
                );
              })}
            </div>
          )}
        </section>

        {sitting && (
          <section aria-label="The paper running">
            <p className="list-head">A paper is running</p>
            <p className="page-text">
              {sitting.name}, started at {new Date(sitting.started_ms).toTimeString().slice(0, 5)} for {sitting.minutes} minutes. Write ? where you are stuck, and S, U or G after each
              answer for sure, unsure or a guess. The tutor is out of reach until it is handed in.
            </p>
            <p className="page-acts">
              <button className="quiet-act strong" disabled={busy} onClick={() => void handIn()}>
                Hand in your work…
              </button>
              {sitting.path && (
                <button
                  className="quiet-act"
                  onClick={() =>
                    // The paper opens beside the page, as the material does; a kind the window does not show goes to the Mac.
                    sourceRef(sitting.path!) ? openMaterial(sitting.path!) : void api.openFile(sitting.path!, folder).catch((problem) => props.onNote(String(problem)))
                  }
                >
                  Open the paper again
                </button>
              )}
              <button className="quiet-act" disabled={busy} onClick={() => void endPaper()}>
                End without handing in
              </button>
            </p>
          </section>
        )}

        <section aria-label="Material">
          <p className="list-head">Material</p>
          {materials === null ? null : materials === "none" ? (
            <p className="page-note">The material folder could not be read.</p>
          ) : materials.files === 0 ? (
            <p className="page-note">Nothing here yet. Add notes, slides or past papers, or drop them on this page.</p>
          ) : (
            <div className="list" role="list">
              {materials.names.map((file) => (
                <button key={file} role="listitem" className="home-row file-row" aria-current={docs.shown === `Study/materials/${file}` ? "true" : undefined} onClick={() => openMaterial(`Study/materials/${file}`)}>
                  <span className="row-name">{materialTitle(file)}</span>
                  {mixed && <span className="row-when">{materialKind(file)}</span>}
                </button>
              ))}
              {materials.files > materials.names.length && <p className="page-note">and {materials.files - materials.names.length} more</p>}
            </div>
          )}
          {materials !== null && materials !== "none" && materials.keys > 0 && (
            <p className="page-note">{materials.keys === 1 ? "One answer key is" : `${materials.keys} answer keys are`} kept out of sight.</p>
          )}
          <p className="page-acts">
            <button className="quiet-act" data-drive="add-material" disabled={!ready || sitting !== null || busy} onClick={() => void addFiles()}>
              Add files…
            </button>
            <button className="quiet-act" disabled={!ready || sitting !== null} onClick={() => props.onPaste(folder)}>
              Paste text…
            </button>
          </p>
        </section>

        {home && home.papers.length > 0 && !sitting && (
          <section aria-label="Papers">
            <p className="list-head">Papers</p>
            <div className="list" role="list">
              {home.papers.map((paper) => (
                <div key={paper.path} role="listitem" className="home-row static paper-row">
                  {/* Its name opens it to read, beside the page; sitting it is its own action. */}
                  <button
                    type="button"
                    className="row-name paper-open"
                    data-drive={`read-${paper.name}`}
                    aria-current={docs.shown === paper.path ? "true" : undefined}
                    title="Read it beside the page"
                    onClick={() => openMaterial(paper.path)}
                  >
                    {paper.name}
                  </button>
                  <span className="row-when">{paper.sat ? "sat before" : paper.minutes ? `${paper.minutes} minutes` : ""}</span>
                  <button className="quiet-act" data-drive={`paper-${paper.name}`} disabled={busy} onClick={() => void sit(paper)}>
                    Sit it
                  </button>
                </div>
              ))}
            </div>
          </section>
        )}

        <details className="your-work" onToggle={(event) => (event.currentTarget.open ? readMarks() : undefined)}>
          <summary className="list-head">Your work</summary>
          {marks === "unread" ? (
            <p className="page-note">Reading the record…</p>
          ) : marks === null ? (
            <p className="page-note">This runtime keeps no record of marked answers the window can read.</p>
          ) : bySkill.size === 0 ? (
            <p className="page-note">Nothing marked yet. Each answer the record marks is kept here.</p>
          ) : (
            <div className="list" role="list">
              {[...bySkill].map(([label, kept]) => (
                <div key={label} role="listitem" className="home-row static">
                  <span className="row-name">
                    <Line text={label} />
                  </span>
                  <span className="row-when">
                    {kept
                      .slice(0, 2)
                      .map((mark) => `${shortDay(mark.at)}, ${markShort(mark)}`)
                      .join("; ")}
                  </span>
                </div>
              ))}
            </div>
          )}
        </details>

        <section aria-label="This folder" className="folder-acts">
          <p className="page-acts">
            {home?.state === "not_set_up" && (
              <button className="quiet-act" disabled={busy} onClick={() => void setUp()}>
                Set it up for Magnesium
              </button>
            )}
            {home?.state === "needs_upgrade" && (
              <button className="quiet-act strong" disabled={busy} onClick={() => void upgrade()}>
                Upgrade it
              </button>
            )}
            {(home?.state === "no_runtime" || home?.state === "unreadable") && (
              <button className="quiet-act" onClick={() => void read()}>
                Check again
              </button>
            )}
            <button className="quiet-act" disabled={props.running} title={props.running ? "Close its running session first" : "The folder and its sessions stay as they are"} onClick={() => props.onForget(folder)}>
              Take this folder off the list
            </button>
          </p>
          {/* Said only when the action cannot be taken now; otherwise what it leaves alone is its title. */}
          {props.running && <p className="page-note">Close its running session first.</p>}
        </section>
      </div>
      </div>
      {withDocs && (
        <SidePanel
          plan={null}
          docs={docs}
          folder={folder}
          goTo={goTo}
          width={props.panelWidth}
          expanded={docs.expanded}
          onDocs={setDocs}
          onWidth={props.onPanelWidth}
          onExpand={(expanded) => setDocs({ ...docsNow.current, expanded, lessonFront: false })}
          onNote={props.onNote}
          handle={(found) => (docView.current = found)}
          picking={picking}
          onPicking={setPicking}
        />
      )}
    </div>
  );
}
