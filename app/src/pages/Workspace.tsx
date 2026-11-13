// A workspace: one study folder's page, in the Home tab. Its sessions as a
// plain list (a name and when last opened) under a visible "New session",
// then what used to sit behind ⌘K: the folder's material, its papers, the
// record of the learner's marked answers, and the folder itself (take it off
// the list, upgrade it, set it up). File names are shown; paths are not.

import { useCallback, useEffect, useState } from "react";

import { Line } from "../surface/Rich";
import type { Core, Home, LessonInfo, Mark, Materials, Paper } from "../types";
import { displayName, sessionName, shortDay, whenOpened, withoutPaths } from "../words";

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
    <div className="page workspace-page" data-page="workspace">
      <div className="page-column">
        <button className="back" data-drive="home" onClick={props.onBack}>
          ‹ Home
        </button>
        <header className="page-head">
          <h1 className="page-title">{name}</h1>
          {home?.message && home.state !== "ready" && <p className="where">{withoutPaths(home.message)}</p>}
        </header>

        <section aria-label="Sessions">
          <button className="new-session" data-drive="new-session" autoFocus={props.focus} disabled={!ready || sitting !== null} onClick={() => props.onNewSession(folder)}>
            <span className="plus" aria-hidden="true">
              +
            </span>{" "}
            New session
          </button>
          {sessions === null ? null : sessions.length === 0 ? (
            <p className="page-note">No sessions yet.</p>
          ) : (
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
                <button className="quiet-act" onClick={() => void api.openFile(sitting.path!, folder).catch((problem) => props.onNote(String(problem)))}>
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
                <button key={file} role="listitem" className="home-row file-row" onClick={() => void api.openFile(`Study/materials/${file}`, folder).catch((problem) => props.onNote(String(problem)))}>
                  <span className="row-name file-name">{file}</span>
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
                <div key={paper.path} role="listitem" className="home-row static">
                  <span className="row-name">{paper.name}</span>
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

        <section aria-label="This folder">
          <p className="list-head">This folder</p>
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
            <button className="quiet-act" disabled={props.running} title={props.running ? "Close its running session first" : undefined} onClick={() => props.onForget(folder)}>
              Take it off the list
            </button>
          </p>
          <p className="page-note">Taking it off the list leaves the folder and its sessions as they are.</p>
        </section>
      </div>
    </div>
  );
}
