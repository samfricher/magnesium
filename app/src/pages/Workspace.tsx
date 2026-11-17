// A workspace: one study folder's page, in the Home tab. The plan's next item
// as the page's one main action, the earlier sessions, the papers, the
// material, where each skill stands in the record, and the folder itself
// (take it off the list, upgrade it, set it up). File names are shown; paths
// are not. While a timed paper runs there is no tutor and nothing to add, so
// the page is that paper alone until it is handed in or ended; the rest comes
// back as it was.

import { useCallback, useEffect, useId, useRef, useState } from "react";

import { agendaOf, againOf, earned, type Again, type Day } from "../agenda";
import { useHangs } from "../hangs";
import { docsOf, keepDocs, withClosed, withOpened, workspaceDocsKey, type Docs } from "../docs";
import { checksAlone, firstStep, namedSessions, nextOf, planDay } from "../next";
import { DISC_R, discFill, shareLeft } from "../paperTime";
import type { DocHandle } from "../surface/DocView";
import { Line } from "../surface/Rich";
import { SidePanel } from "../surface/SidePanel";
import { sourceRef } from "../sources";
import type { Core, Exam, Home, LessonInfo, Mark, Materials, Paper, PaperSitting, Plan, Progress, Row } from "../types";
import { displayName, goalName, materialKind, materialTitle, namedByItsWords, shortDay, whenOpened, withoutPaths } from "../words";

type Props = {
  api: Core;
  folder: string;
  /** The page is in front, with nothing over it. */
  focus: boolean;
  /** Home's tab, which holds the page, is in front. Behind another tab the page stays as it was, read again by `refresh`. */
  front: boolean;
  /** A session in this folder is running, so the folder cannot be taken off the list now. */
  running: boolean;
  onBack: () => void;
  onNewSession: (folder: string) => void;
  /** Start a session on the plan's next item, as Home's first row starts one. */
  onStartSession: (folder: string) => void;
  onOpenSession: (folder: string, session: string, title: string) => void;
  /** Just the quick checks due here, in a session of their own. */
  onChecks: (folder: string) => void;
  /** How many quick checks are due here, as the page read it. */
  onChecksDue?: (folder: string, due: number) => void;
  /** The key for Just the quick checks, as the learner set it. */
  checksKey?: string;
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
  /** Whether a timed paper runs in this folder, as the page shows it: a file dropped then is not added as material. */
  onPaper?: (folder: string, running: boolean) => void;
  onNote: (text: string) => void;
  /** Bumped when what the page shows may have changed from outside it: a drop, a lesson's marks, Home's tab back in front. */
  refresh: number;
  /** The page's first reading, made before the window opened it, or still being made when the window stopped waiting for it. */
  first?: Reading | Promise<Reading>;
};

/**
 * All the page shows of its folder, read at once. The page is drawn from one reading, so its parts never come in
 * one after another, each pushing the ones under it down.
 */
export type Reading = {
  home: Home | null;
  sessions: LessonInfo[];
  /** "none" when the material folder could not be read. */
  materials: Materials | "none";
  /** null when it could not be read, which the page treats as no plan. */
  plan: Plan | null;
  /** null when it could not be read. */
  record: { progress: Progress; marks: Mark[] } | null;
};

/** Reads a folder's page. Notes the visit, as opening the page does. */
export async function readWorkspace(api: Core, folder: string): Promise<Reading> {
  const [home, sessions, materials, plan, record] = await Promise.all([
    api.visitFolder(folder).catch(() => null),
    api.lessons(folder).catch(() => [] as LessonInfo[]),
    api.materialList(folder).catch(() => "none" as const),
    api.plan(folder).catch(() => null),
    Promise.all([api.progress(null, folder), api.marks(folder).catch(() => null)]).then(
      ([progress, marks]) => ({ progress, marks: marks ?? [] }),
      () => null,
    ),
  ]);
  return { home, sessions, materials, plan, record };
}

/**
 * How long the window keeps the page in front as it is while a folder's page it was asked to open is read. Past it,
 * the folder's page opens with its head alone and the rest comes in at once when read. On a made-up course the
 * reading took 50 to 100 ms.
 */
export const OPEN_WAIT_MS = 400;

/** "Midterm 1, Tue 17 Nov · 5 days", or null for a plan with no date ahead. */
function examLine(exam: Exam | null): string | null {
  if (!exam?.day || exam.days_left === null || exam.days_left < 0) return null;
  const left = exam.days_left === 0 ? "today" : exam.days_left === 1 ? "tomorrow" : `${exam.days_left} days`;
  return `${exam.title}, ${exam.day} · ${left}`;
}

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** What is due, in one plain line, or null when nothing is. */
function dueLine(home: Home): string | null {
  const parts = [home.checks_due ? plural(home.checks_due, "quick check", "quick checks") : null, home.redos_due ? plural(home.redos_due, "paper redo", "paper redos") : null].filter(
    (part): part is string => part !== null,
  );
  if (parts.length === 0) return null;
  const count = (home.checks_due ?? 0) + home.redos_due;
  return `${parts.join(" and ")} ${count === 1 ? "is" : "are"} due.`;
}

/** A skill's three days, filled for each the record counts now. A pass made before a later miss no longer counts, and is drawn empty. */
function Days({ row }: { row: Row }) {
  const said = `${earned(row)} of the 3 days it needs: right, on your own, from memory`;
  return (
    <span className="work-days" role="img" aria-label={said} title={said}>
      {row.dots.map((dot, at) => (
        <span key={at} className={dot === 2 ? "work-day earned" : "work-day"} />
      ))}
    </span>
  );
}

/**
 * Your work as an agenda: each day said once, beside the first skill it holds, in the margin left of the names while
 * the margin has room for it, else in a column of its own at the list's left. Each skill keeps its three days.
 */
function Agenda({ days }: { days: Day<{ row: Row; next: Again }>[] }) {
  const list = useRef<HTMLDivElement>(null);
  // Measured as a pass's rings beside its mark are: the margin left of the column depends on the window, the text
  // size, the line width set in Settings and a document open beside the page, and a day's words on its font.
  const hung = useHangs(
    list,
    ".page-scroll",
    (element) => {
      const label = element.querySelector<HTMLElement>(".agenda-day");
      const row = element.querySelector<HTMLElement>(".agenda-row");
      if (!label || !row) return 0;
      const widest = Math.max(label.offsetWidth, ...[...element.querySelectorAll<HTMLElement>(".agenda-day > span")].map((each) => each.offsetWidth));
      // 24px from the window's edge in all, as the column keeps at its narrowest; the hook keeps 8 of them.
      return widest + (parseFloat(getComputedStyle(row).columnGap) || 0) + 16;
    },
    [days],
  );
  return (
    <div ref={list} className={hung ? "agenda hung" : "agenda"} role="list" data-role="agenda">
      {days.map((day) =>
        day.exam !== null ? (
          <div key={day.key} role="listitem" className="agenda-row first agenda-exam">
            <span className="agenda-day">
              <span>{day.label}</span>
            </span>
            <span className="agenda-exam-name">
              <Line text={day.exam} />
            </span>
          </div>
        ) : (
          day.items.map(({ row }, at) => (
            <div key={row.name} role="listitem" className={`agenda-row${at === 0 ? " first" : ""}${day.proven ? " proven" : ""}`}>
              <span className={day.today ? "agenda-day today" : "agenda-day"}>{at === 0 && <span>{day.label}</span>}</span>
              <span className="row-name work-name">
                <Line text={row.name} />
              </span>
              <Days row={row} />
            </div>
          ))
        ),
      )}
    </div>
  );
}

/**
 * The time left as a disc beside its words: a wash of the ink over the share left, read from the desk without
 * subtracting. It is drawn again only when the words change, once a minute, and never moves in between.
 */
function TimeDisc({ share }: { share: number }) {
  const fill = discFill(share);
  return (
    <svg className="time-disc" viewBox="0 0 48 48" aria-hidden="true" data-role="time-disc" data-share={share.toFixed(3)}>
      {fill === "whole" ? <circle cx="24" cy="24" r={DISC_R} className="time-disc-left" /> : fill !== "none" && <path d={fill.path} className="time-disc-left" />}
      <circle cx="24" cy="24" r={DISC_R} className="time-disc-edge" />
    </svg>
  );
}

const MINUTE = 60_000;
const clock = (ms: number) => new Date(ms).toTimeString().slice(0, 5);
const dayOf = (ms: number) => new Date(ms).toDateString();
const isoDay = (ms: number) => {
  const date = new Date(ms);
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
};

/** How long ago a paper's time was up, said coarsely: minutes, then hours, then days. */
function agoWords(ms: number): string {
  const minutes = Math.floor(ms / MINUTE);
  if (minutes < 1) return "just now";
  if (minutes < 60) return plural(minutes, "minute", "minutes") + " ago";
  if (minutes < 24 * 60) return plural(Math.floor(minutes / 60), "hour", "hours") + " ago";
  return plural(Math.floor(minutes / (24 * 60)), "day", "days") + " ago";
}

/** The time now, read again only when the paper's count of whole minutes left would change, so nothing moves in between. */
function useMinuteOf(end: number): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    // The count changes as end - now crosses a whole minute; the timer is set just past that moment.
    const left = end - now;
    const wait = (((left % MINUTE) + MINUTE) % MINUTE || MINUTE) + 50;
    const timer = window.setTimeout(() => setNow(Date.now()), wait);
    return () => window.clearTimeout(timer);
  }, [end, now]);
  return now;
}

type Running = {
  sitting: PaperSitting;
  busy: boolean;
  focus: boolean;
  onHandIn: () => void;
  onOpen: (() => void) | null;
  onEnd: () => Promise<void>;
};

/** A timed paper while it runs: the page's lead, with its end, its actions and the paper's two marks. */
function RunningPaper({ sitting, busy, focus, onHandIn, onOpen, onEnd }: Running) {
  const end = sitting.started_ms + sitting.minutes * MINUTE;
  const now = useMinuteOf(end);
  const [confirming, setConfirming] = useState(false);
  const said = useId();
  const endButton = useRef<HTMLButtonElement>(null);
  const keepButton = useRef<HTMLButtonElement>(null);
  const over = now >= end;
  const left = Math.ceil((end - now) / MINUTE);
  // A paper started on an earlier day says the day, so a window opened the next morning does not read as today.
  const today = dayOf(now);
  const when = (ms: number) => (dayOf(ms) === today ? clock(ms) : `${shortDay(isoDay(ms))}, ${clock(ms)}`);

  useEffect(() => {
    if (confirming) keepButton.current?.focus();
  }, [confirming]);
  const keepGoing = () => {
    setConfirming(false);
    // Back on the action that asked, so a second Tab or Return is where it was.
    window.requestAnimationFrame(() => endButton.current?.focus());
  };

  return (
    <section
      aria-label="The paper running"
      className="running-paper"
      onKeyDown={(event) => {
        // Esc puts the paper back as it was before it leaves the page.
        if (confirming && event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
          keepGoing();
        }
      }}
    >
      <p className="list-head">A paper is running</p>
      {/* The time leads, at a size read from the desk where the paper is sat. Its words change only with the
          minute, and nothing else in the block moves. */}
      <div className="paper-head">
        <TimeDisc share={over ? 0 : shareLeft(left, sitting.minutes)} />
        <p className="paper-time" data-drive="sitting-left">
          {over ? "Time is up" : `${plural(left, "minute", "minutes")} left`}
        </p>
        <h2 className="paper-name" title={sitting.name}>
          {sitting.name}
        </h2>
        {/* Past its time the paper's length says nothing new, and leaving it out keeps Hand in on a short window
            at the largest text. */}
        <p className="paper-facts">
          <span>{over ? `Up at ${when(end)}, ${agoWords(now - end)}` : `Until ${when(end)}`}</span>
          <span>started {when(sitting.started_ms)}</span>
          {!over && <span>{plural(sitting.minutes, "minute", "minutes")}</span>}
        </p>
      </div>
      {confirming ? (
        <div className="paper-confirm" role="group" aria-label="End without handing in" aria-describedby={said}>
          <p className="paper-confirm-text" id={said}>
            The paper ends: nothing is handed in or marked, your record stays as it was, and you can sit it again from the start.
          </p>
          <p className="page-acts">
            <button ref={keepButton} className="quiet-act strong" data-drive="sitting-keep" disabled={busy} onClick={keepGoing}>
              Keep going
            </button>
            <button className="quiet-act" data-drive="sitting-end-now" disabled={busy} onClick={() => void onEnd()}>
              End the paper
            </button>
          </p>
        </div>
      ) : (
        <>
          {/* Past its time, handing in is what is left to do, so it stands as a step of its own. */}
          {over && (
            <button className="paper-hand-in" data-drive="sitting-hand-in" autoFocus={focus} disabled={busy} onClick={onHandIn}>
              Hand in your work…
            </button>
          )}
          <p className="page-acts">
            {!over && (
              <button className="quiet-act strong" data-drive="sitting-hand-in" autoFocus={focus} disabled={busy} onClick={onHandIn}>
                Hand in your work…
              </button>
            )}
            {onOpen && (
              <button className="quiet-act" data-drive="sitting-open" onClick={onOpen}>
                Open the paper again
              </button>
            )}
            <button ref={endButton} className="quiet-act" data-drive="sitting-end" disabled={busy} onClick={() => setConfirming(true)}>
              End without handing in
            </button>
          </p>
        </>
      )}
      <p className="paper-rule">No tutor until you hand in.</p>
      <dl className="paper-marks">
        <dt>?</dt>
        <dd>where you are stuck, instead of looking it up</dd>
        <dt>S U G</dt>
        <dd>after each answer: sure, unsure or a guess</dd>
      </dl>
    </section>
  );
}

export function WorkspacePage(props: Props) {
  const { api, folder } = props;
  // null until the first reading is in. Until then the page draws its head alone.
  const [reading, setReading] = useState<Reading | null>(() => (props.first && !(props.first instanceof Promise) ? props.first : null));
  const home = reading?.home ?? null;
  const sessions = reading?.sessions ?? null;
  const materials = reading?.materials ?? null;
  const plan = reading?.plan;
  const record = reading?.record;
  const [materialOpen, setMaterialOpen] = useState(false);
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
      case "close-document": {
        if (!docs.shown) return false;
        const hadKeys = (document.activeElement as HTMLElement | null)?.closest(".panel.side") !== null;
        const left = withClosed(docs, docs.shown);
        setDocs(left);
        // The keyboard goes to the next document, or back to the page when none is left.
        if (hadKeys) window.setTimeout(() => (left.shown ? docView.current?.focus() : column.current?.querySelector<HTMLElement>("button")?.focus()), 120);
        return true;
      }
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
  // Back in front with the keyboard outside the page, it goes to what leads the page, as when the page was first
  // drawn. The tab's page then gives it back to whatever in the page had it last, if anything did.
  const { front } = props;
  const wasFront = useRef(front);
  useEffect(() => {
    const back = front && !wasFront.current;
    wasFront.current = front;
    if (back && props.focus && !pageRef.current?.contains(document.activeElement))
      column.current?.querySelector<HTMLElement>(".next-row, section button:not(:disabled)")?.focus({ preventScroll: true });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [front]);
  // The panel shows while material is open beside the page, or while its list is open to pick from.
  const withDocs = docs.open.length > 0 || picking;

  // Each reading is numbered: one that comes back after a later one began is not drawn over it.
  const readings = useRef(0);
  const take = useCallback(async (pending: Promise<Reading>) => {
    const at = ++readings.current;
    const fresh = await pending;
    if (at === readings.current) setReading(fresh);
  }, []);
  const read = useCallback(() => take(readWorkspace(api, folder)), [take, api, folder]);

  // The window hands the page its first reading: read again only when something changes after it.
  const handed = useRef(props.first);
  useEffect(() => {
    const first = handed.current;
    handed.current = undefined;
    if (first instanceof Promise) void take(first);
    else if (!first) void read();
  }, [take, read, props.refresh]);
  const { onChecksDue } = props;
  useEffect(() => {
    if (home) onChecksDue?.(folder, checksAlone(home));
  }, [home, folder, onChecksDue]);

  // A session still named by the words it was started with ("Continue") takes its own lesson's goal, as a new
  // one does: the goal the record binds to that conversation, never the folder's last goal, which a lesson in a
  // terminal may have set.
  const { onSessionRenamed } = props;
  useEffect(() => {
    const renames = (sessions ?? [])
      .filter((info) => namedByItsWords(info))
      .map((info) => ({ info, name: goalName(info.goal, info.opening) }))
      .filter((each): each is { info: LessonInfo; name: string } => each.name !== null && each.name !== each.info.title);
    if (renames.length === 0) return;
    void Promise.all(renames.map(({ info, name }) => api.renameSession(folder, info.session, name).then(() => onSessionRenamed(folder, info.session, name)))).then(
      () => void read(),
      () => {},
    );
  }, [api, folder, sessions, onSessionRenamed, read]);

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

  const endPaper = async () => {
    setBusy(true);
    try {
      await api.paperEnd([], folder);
    } catch (problem) {
      props.onNote(String(problem).replace(/^Error: /, ""));
    } finally {
      setBusy(false);
      await read();
    }
    // The block the keyboard was in is gone: the keyboard goes to what leads the page now.
    window.requestAnimationFrame(() => column.current?.querySelector<HTMLElement>(".next-row, section button:not(:disabled)")?.focus());
  };
  const withHome = (fresh: Home) => setReading((old) => old && { ...old, home: fresh });
  const upgrade = () =>
    act(async () => {
      withHome(await api.upgradeFolder(folder));
      props.onNote("This folder is upgraded. Magnesium kept a copy of its record from before.");
    });
  const setUp = () => act(async () => withHome(await api.setUp(folder)));

  const name = home ? displayName(home.name) : displayName(folder.split("/").pop() ?? "");
  // A kind beside each file only says something when the files are not all of one kind.
  const mixed = materials !== null && materials !== "none" && new Set(materials.names.map(materialKind)).size > 1;
  const sitting = home?.sitting ?? null;
  const ready = home !== null && ["ready", "empty", "not_set_up"].includes(home.state);
  const exam = examLine(home?.exam ?? null);
  const held = props.running ? "Close its running session first" : null;
  const { onPaper } = props;
  const paperRunning = sitting !== null;
  useEffect(() => {
    onPaper?.(folder, paperRunning);
    return () => onPaper?.(folder, false);
  }, [onPaper, folder, paperRunning]);

  // The plan's item in hand, by the rule Home's first row uses too (src/next.ts).
  const items = plan?.items ?? [];
  const named = namedSessions(sessions ?? [], home?.last?.goal ?? null);
  const next = nextOf(items, named);
  const current = next?.item ?? null;
  const carried = next?.carried ?? null;
  const earlier = named.filter((each) => each !== carried);
  // The next thing shows once the folder can be studied in and the plan has been read; a running paper takes its place.
  const showNext = home !== null && (home.state === "ready" || home.state === "empty") && plan !== undefined && sitting === null;
  // What the session Next opens starts with, from the record's reading, said on its row. When the next message would
  // not begin a sitting (one carries on, or the opener is off), what is due is said under the row instead, with no
  // promise of when it comes.
  const first = firstStep(home);
  const due = home && !first ? dueLine(home) : null;
  // The quick checks due can be taken alone, in a session of their own.
  const alone = checksAlone(home);
  const checksDue = alone > 0;

  const listed = materials !== null && materials !== "none" && materials.files > 0 ? materials : null;

  // Each skill the learner has answered on, with the runtime's count of its days, in the order it can count again:
  // today, then by its day, then those with no day, then the proven; the newest answer first within each.
  const marksById = new Map(record ? record.marks.map((mark) => [mark.id, mark]) : []);
  const work =
    record
      ? [...record.progress.in_progress, ...record.progress.confirmed, ...record.progress.untested]
          .map((row) => ({ row, last: row.last_answer ? (marksById.get(row.last_answer) ?? null) : null, next: againOf(row) }))
          .filter(({ row, last }) => last !== null || earned(row) > 0)
          .sort((a, b) => a.next.rank - b.next.rank || a.next.day.localeCompare(b.next.day) || (b.last?.at ?? "").localeCompare(a.last?.at ?? ""))
      : [];

  return (
    <div
      ref={pageRef}
      className={`page workspace-page${withDocs ? " with-docs" : ""}${withDocs && docs.expanded ? " expanded" : ""}${withDocs && docs.expanded && docs.lessonFront ? " lesson-front" : ""}`}
      data-page="workspace"
      data-read={reading ? "" : undefined}
      style={withDocs ? ({ "--side-w": `${props.panelWidth}px` } as React.CSSProperties) : undefined}
    >
      <div className="page-scroll">
      <div className="page-column" ref={column}>
        <button className="back" data-drive="home" onClick={props.onBack}>
          ‹ Home
        </button>
        <header className="page-head">
          <h1 className="page-title">{name}</h1>
          {exam && <p className="page-exam">{exam}</p>}
          {home?.message && home.state !== "ready" && <p className="where">{withoutPaths(home.message)}</p>}
          {/* What the folder needs before study, under the words that ask for it, drawn as Home draws a step's choices. */}
          {home && ["not_set_up", "needs_upgrade", "no_runtime", "unreadable"].includes(home.state) && (
            <div className="choices-rest" role="group">
              {home.state === "not_set_up" && (
                <button className="choice-rest" autoFocus={props.focus} disabled={busy} onClick={() => void setUp()}>
                  Set it up for Magnesium
                </button>
              )}
              {home.state === "needs_upgrade" && (
                <button className="choice-rest" autoFocus={props.focus} disabled={busy} onClick={() => void upgrade()}>
                  Upgrade it
                </button>
              )}
              {(home.state === "no_runtime" || home.state === "unreadable") && (
                <button className="choice-rest" autoFocus={props.focus} onClick={() => void read()}>
                  Check again
                </button>
              )}
            </div>
          )}
        </header>

        {sitting && (
          <RunningPaper
            key={`${sitting.name}@${sitting.started_ms}`}
            sitting={sitting}
            busy={busy}
            focus={props.focus}
            onHandIn={() => void handIn()}
            onOpen={
              sitting.path
                ? () =>
                    // The paper opens beside the page, as the material does; a kind the window does not show goes to the Mac.
                    sourceRef(sitting.path!) ? openMaterial(sitting.path!) : void api.openFile(sitting.path!, folder).catch((problem) => props.onNote(String(problem)))
                : null
            }
            onEnd={endPaper}
          />
        )}

        {showNext && (
          <section aria-label="Next" className="next">
            <p className="list-head">Next</p>
            {current ? (
              <button
                className="home-row first next-row"
                data-drive="next"
                autoFocus={props.focus}
                onClick={() => (carried ? props.onOpenSession(folder, carried.info.session, carried.title) : props.onStartSession(folder))}
              >
                <span className="next-what">
                  <span className="row-name next-name">
                    <Line text={current.title} />
                  </span>
                  {first && <span className="next-first">{first}</span>}
                  {current.note && (
                    <span className="next-note">
                      <Line text={current.note} />
                    </span>
                  )}
                </span>
                <span className="row-when">{planDay(current.day)}</span>
                <span className="row-key" aria-hidden="true">
                  ↩
                </span>
              </button>
            ) : (
              <>
                {items.length > 0 && <p className="page-note">Everything on the plan is done.</p>}
                <button className="home-row first next-row" data-drive="new-session" autoFocus={props.focus} onClick={() => props.onNewSession(folder)}>
                  <span className="next-what">
                    <span className="row-name next-name">New session</span>
                    {first && <span className="next-first">{first}</span>}
                  </span>
                  <span className="row-when" />
                  <span className="row-key" aria-hidden="true">
                    ↩
                  </span>
                </button>
              </>
            )}
            {due && <p className="page-note next-due">{due}</p>}
            {(current || checksDue) && (
              <p className="page-acts next-acts">
                {checksDue && (
                  <button className="quiet-act" data-drive="just-checks" title={props.checksKey ? `Just the quick checks, ${props.checksKey}` : undefined} onClick={() => props.onChecks(folder)}>
                    {alone === 1 ? "Just the quick check" : "Just the quick checks"}
                  </button>
                )}
                {current && (
                  <button className="quiet-act" data-drive="new-session" onClick={() => props.onNewSession(folder)}>
                    Something else…
                  </button>
                )}
              </p>
            )}
          </section>
        )}

        {reading && !sitting && <section aria-label="Your work" className="your-work">
          <p className="list-head">Your work</p>
          {!record ? (
            <p className="page-note">The record could not be read.</p>
          ) : work.length === 0 ? (
            <p className="page-note">
              {/* The core's own words for an empty record speak of papers and checks only; a lesson's answers count too. Its other words (a gap log edited by hand, an upgrade) are said as they are. */}
              {record.progress.message && !record.progress.message.startsWith("Nothing is proved yet")
                ? withoutPaths(record.progress.message)
                : "Nothing marked yet. Each skill you answer on is kept here."}
            </p>
          ) : (
            <>
              <Agenda days={agendaOf(work, home?.exam ?? null)} />
              <p className="page-note work-key">Each ring is a day you got it right on your own, from memory. Three prove a skill.</p>
            </>
          )}
        </section>}

        {earlier.length > 0 && !sitting && (
          <section aria-label="Earlier sessions">
            <p className="list-head">Earlier</p>
            <div className="list" role="list">
              {earlier.map(({ info, title }) => (
                <button key={info.session} role="listitem" className="home-row" onClick={() => props.onOpenSession(folder, info.session, title)}>
                  <span className="row-name">
                    <Line text={title} />
                  </span>
                  <span className="row-when">{whenOpened(info.opened_ms)}</span>
                </button>
              ))}
            </div>
          </section>
        )}

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
                  <button className="quiet-act" data-drive={`paper-${paper.name}`} disabled={busy || !ready} onClick={() => void sit(paper)}>
                    Sit it
                  </button>
                </div>
              ))}
            </div>
          </section>
        )}

        {reading && !sitting && <section aria-label="Material">
          {listed ? (
            <button className="list-head fold" data-drive="material" aria-expanded={materialOpen} onClick={() => setMaterialOpen((open) => !open)}>
              Material <span className="fold-count">{plural(listed.files, "file", "files")}</span>
            </button>
          ) : (
            <p className="list-head">Material</p>
          )}
          {materials === "none" && <p className="page-note">The material folder could not be read.</p>}
          {materials !== null && materials !== "none" && materials.files === 0 && <p className="page-note">Nothing here yet. Add notes, slides or past papers, or drop them on this page.</p>}
          {listed && materialOpen && (
            <div className="list" role="list">
              {listed.names.map((file) => (
                <button key={file} role="listitem" className="home-row file-row" aria-current={docs.shown === `Study/materials/${file}` ? "true" : undefined} onClick={() => openMaterial(`Study/materials/${file}`)}>
                  <span className="row-name">{materialTitle(file)}</span>
                  {mixed && <span className="row-when">{materialKind(file)}</span>}
                </button>
              ))}
              {listed.files > listed.names.length && <p className="page-note">and {listed.files - listed.names.length} more, in the folder</p>}
            </div>
          )}
          {(materialOpen || !listed) && materials !== null && materials !== "none" && (
            <>
              {materials.keys > 0 && <p className="page-note">{materials.keys === 1 ? "One answer key is" : `${materials.keys} answer keys are`} kept out of sight.</p>}
              <p className="page-acts">
                <button className="quiet-act" data-drive="add-material" disabled={!ready || busy} onClick={() => void addFiles()}>
                  Add files…
                </button>
                <button className="quiet-act" disabled={!ready} onClick={() => props.onPaste(folder)}>
                  Paste text…
                </button>
              </p>
            </>
          )}
        </section>}

        {/* Off the list, a running paper's clock would go on with nothing to show it or hand it in: the action waits
            for the paper, and is not drawn while it runs. */}
        {reading && !sitting && <section aria-label="This folder" className="folder-acts">
          <p className="page-acts">
            <button className="quiet-act" disabled={held !== null} title={held ?? "The folder and its sessions stay as they are"} onClick={() => props.onForget(folder)}>
              Take this folder off the list
            </button>
          </p>
          {/* Said only when the action cannot be taken now; otherwise what it leaves alone is its title. */}
          {held && <p className="page-note">{held}.</p>}
        </section>}
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
