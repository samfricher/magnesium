import { useCallback, useEffect, useId, useRef, useState } from "react";

import { agendaOf, againOf, earned } from "../agenda";
import { docsOf, keepDocs, withClosed, withOpened, workspaceDocsKey, type Docs } from "../docs";
import { checksAlone, dueWords, latestSkill, namedSessions, nextOf, openedDay, startsWithWords, planDay } from "../next";
import { DISC_R, discFill, shareLeft } from "../paperTime";
import type { DocHandle } from "../surface/DocView";
import { Line } from "../surface/Rich";
import { SidePanel } from "../surface/SidePanel";
import { sourceRef } from "../sources";
import type { Core, Exam, Home, LessonInfo, Mark, Materials, Paper, PaperSitting, Plan, Progress, Row } from "../types";
import { displayName, goalName, longDay, materialKind, materialTitle, namedByItsWords, shortDay, withoutPaths } from "../words";
import { Cue, More, Part, RepairRun, Row as PageRow, isCode, useMargin } from "./Margin";

type Props = {
  api: Core;
  folder: string;
  focus: boolean;
  front: boolean;
  running: boolean;
  onBack: () => void;
  onNewSession: (folder: string) => void;
  onStartSession: (folder: string) => void;
  onOpenSession: (folder: string, session: string, title: string) => void;
  onChecks: (folder: string) => void;
  onChecksDue?: (folder: string, due: number) => void;
  checksKey?: string;
  onHandedIn: (folder: string, words: { shown: string; sent: string }, title: string) => void;
  onForget: (folder: string) => void;
  onPaste: (folder: string) => void;
  panelWidth: number;
  onPanelWidth: (width: number) => void;
  onShowPanel: () => void;
  onSessionRenamed: (folder: string, session: string, title: string) => void;
  onDocsChange: () => void;
  registerPanel: (run: ((command: string) => boolean) | null) => void;
  onPaper?: (folder: string, running: boolean) => void;
  onNote: (text: string) => void;
  refresh: number;
  first?: Reading | Promise<Reading>;
};

export type Reading = {
  home: Home | null;
  sessions: LessonInfo[];
  materials: Materials | "none";
  plan: Plan | null;
  record: { progress: Progress; marks: Mark[] } | null;
};

export async function readWorkspace(api: Core, folder: string, visit = false): Promise<Reading> {
  const [home, sessions, materials, plan, record] = await Promise.all([
    (visit ? api.visitFolder(folder) : api.home(folder)).catch(() => null),
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

export const OPEN_WAIT_MS = 400;

function examWords(exam: Exam | null): string | null {
  if (!exam || exam.days_left === null || exam.days_left < 0) return null;
  const day = exam.date ? longDay(exam.date) : exam.day;
  if (!day) return null;
  const left = exam.days_left === 0 ? "today" : exam.days_left === 1 ? "tomorrow" : `in ${exam.days_left} days`;
  return `${exam.title}, ${day}, ${left}.`;
}

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

const SESSIONS_SHOWN = 6;

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

function agoWords(ms: number): string {
  const minutes = Math.floor(ms / MINUTE);
  if (minutes < 1) return "just now";
  if (minutes < 60) return plural(minutes, "minute", "minutes") + " ago";
  if (minutes < 24 * 60) return plural(Math.floor(minutes / 60), "hour", "hours") + " ago";
  return plural(Math.floor(minutes / (24 * 60)), "day", "days") + " ago";
}

function useMinuteOf(end: number): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
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

function RunningPaper({ sitting, busy, focus, onHandIn, onOpen, onEnd }: Running) {
  const end = sitting.started_ms + sitting.minutes * MINUTE;
  const now = useMinuteOf(end);
  const [confirming, setConfirming] = useState(false);
  const said = useId();
  const endButton = useRef<HTMLButtonElement>(null);
  const keepButton = useRef<HTMLButtonElement>(null);
  const over = now >= end;
  const left = Math.ceil((end - now) / MINUTE);
  const today = dayOf(now);
  const when = (ms: number) => (dayOf(ms) === today ? clock(ms) : `${shortDay(isoDay(ms))}, ${clock(ms)}`);

  useEffect(() => {
    if (confirming) keepButton.current?.focus();
  }, [confirming]);
  const keepGoing = () => {
    setConfirming(false);
    window.requestAnimationFrame(() => endButton.current?.focus());
  };

  return (
    <section
      aria-label="The paper running"
      className="running-paper"
      onKeyDown={(event) => {
        if (confirming && event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
          keepGoing();
        }
      }}
    >
      <p className="list-head">A paper is running</p>
      <div className="paper-head">
        <TimeDisc share={over ? 0 : shareLeft(left, sitting.minutes)} />
        <p className="paper-time" data-drive="sitting-left">
          {over ? "Time is up" : `${plural(left, "minute", "minutes")} left`}
        </p>
        <h2 className="paper-name" title={sitting.name}>
          {sitting.name}
        </h2>
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
  const [reading, setReading] = useState<Reading | null>(() => (props.first && !(props.first instanceof Promise) ? props.first : null));
  const home = reading?.home ?? null;
  const sessions = reading?.sessions ?? null;
  const materials = reading?.materials ?? null;
  const plan = reading?.plan;
  const record = reading?.record;
  const [materialOpen, setMaterialOpen] = useState(false);
  const [busy, setBusy] = useState(false);

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
      props.onShowPanel();
      if (docs.expanded && docs.lessonFront) setDocs({ ...docs, lessonFront: false });
      setPicking(true);
      return true;
    }
    if (docs.open.length === 0) return false;
    switch (command) {
      case "escape": {
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
        if (hadKeys) window.setTimeout(() => (left.shown ? docView.current?.focus() : column.current?.querySelector<HTMLElement>("button")?.focus()), 120);
        return true;
      }
      case "zoom-in-document":
      case "zoom-out-document":
      case "zoom-fit-document":
        docView.current?.zoom(command === "zoom-in-document" ? 1 : command === "zoom-out-document" ? -1 : 0);
        return true;
      case "share-document":
        pageRef.current?.querySelector<HTMLElement>('[data-drive="share-document"]')?.click();
        return true;
    }
    return false;
  };
  const { registerPanel, front } = props;
  useEffect(() => {
    if (!front) return;
    registerPanel((command) => panelRun.current(command));
    return () => registerPanel(null);
  }, [registerPanel, front]);
  const wasFront = useRef(front);
  useEffect(() => {
    const back = front && !wasFront.current;
    wasFront.current = front;
    if (back) visitDue.current = true;
    if (back && props.focus && !pageRef.current?.contains(document.activeElement))
      column.current?.querySelector<HTMLElement>(".next-row, section button:not(:disabled)")?.focus({ preventScroll: true });
  }, [front]);
  const withDocs = docs.open.length > 0 || picking;

  const begun = useRef(0);
  const drawn = useRef(0);
  const take = useCallback(async (pending: Promise<Reading>) => {
    const at = ++begun.current;
    const fresh = await pending;
    if (at < drawn.current) return;
    drawn.current = at;
    setReading(fresh);
  }, []);
  const visitDue = useRef(props.front && props.first === undefined);
  const read = useCallback(() => {
    const visit = visitDue.current;
    visitDue.current = false;
    return take(readWorkspace(api, folder, visit));
  }, [take, api, folder]);

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
    window.requestAnimationFrame(() => column.current?.querySelector<HTMLElement>(".next-row, section button:not(:disabled)")?.focus());
  };
  const withHome = (fresh: Home) => setReading((old) => old && { ...old, home: fresh });
  const upgrade = () =>
    act(async () => {
      withHome(await api.upgradeFolder(folder));
      props.onNote("This space is upgraded. Magnesium kept a copy of its record from before.");
    });
  const setUp = () => act(async () => withHome(await api.setUp(folder)));

  const name = home ? displayName(home.name) : displayName(folder.split("/").pop() ?? "");
  const mixed = materials !== null && materials !== "none" && new Set(materials.names.map(materialKind)).size > 1;
  const sitting = home?.sitting ?? null;
  const ready = home !== null && ["ready", "empty", "not_set_up"].includes(home.state);
  const exam = examWords(home?.exam ?? null);
  const held = props.running ? "Close its running session first" : null;
  const { onPaper } = props;
  const paperRunning = sitting !== null;
  useEffect(() => {
    onPaper?.(folder, paperRunning);
    return () => onPaper?.(folder, false);
  }, [onPaper, folder, paperRunning]);
  const hung = useMargin(column, ".page-scroll", [withDocs, reading !== null]);
  const [allSessions, setAllSessions] = useState(false);

  const items = plan?.items ?? [];
  const named = namedSessions(sessions ?? [], home?.last?.goal ?? null);
  const next = nextOf(items, named);
  const current = next?.item ?? null;
  const carried = next?.carried ?? null;
  const showNext = home !== null && (home.state === "ready" || home.state === "empty") && plan !== undefined && sitting === null;
  const said = startsWithWords(home) ?? dueWords(home);
  const alone = checksAlone(home);
  const checksDue = alone > 0;
  const startNext = () => (carried ? props.onOpenSession(folder, carried.info.session, carried.title) : props.onStartSession(folder));
  const step = current ? items.indexOf(current) + 1 : 0;
  const planned = current ? planDay(current.day) : "";

  const listed = materials !== null && materials !== "none" && materials.files > 0 ? materials : null;

  const marksById = new Map(record ? record.marks.map((mark) => [mark.id, mark]) : []);
  const rows = record ? [...record.progress.in_progress, ...record.progress.confirmed, ...record.progress.untested] : [];
  const work = rows
    .map((row) => ({ row, last: row.last_answer ? (marksById.get(row.last_answer) ?? null) : null, next: againOf(row) }))
    .filter(({ row, last }) => last !== null || earned(row) > 0)
    .sort((a, b) => a.next.rank - b.next.rank || a.next.day.localeCompare(b.next.day) || (b.last?.at ?? "").localeCompare(a.last?.at ?? ""));
  const last = record ? latestSkill(rows, record.marks) : null;
  const days = work.length > 0 ? agendaOf(work, home?.exam ?? null) : [];
  const repairs = days.some((day) => day.label === "To repair");
  const skillName = (name: string) => (isCode(name) ? <span className="mg-code">{name}</span> : <Line text={name} />);
  const workRow = (row: Row, quiet = false) => (
    <div key={row.name} role="listitem" className={`mg-skill${quiet ? " quiet" : ""}`}>
      <span className="mg-row-name">{skillName(row.name)}</span>
      <Days row={row} />
    </div>
  );
  const shownSessions = allSessions ? named : named.slice(0, SESSIONS_SHOWN);
  const capital = (text: string) => text.charAt(0).toUpperCase() + text.slice(1);

  return (
    <div
      ref={pageRef}
      className={`page workspace-page${withDocs ? " with-docs" : ""}${withDocs && docs.expanded ? " expanded" : ""}${withDocs && docs.expanded && docs.lessonFront ? " lesson-front" : ""}`}
      data-page="workspace"
      data-read={reading ? "" : undefined}
      style={withDocs ? ({ "--side-w": `${props.panelWidth}px` } as React.CSSProperties) : undefined}
    >
      <div className="page-scroll">
      <div className={`page-column mg${hung ? "" : " stacked"}`} ref={column}>
        <header className="mg-course-head">
          <button className="back" data-drive="home" onClick={props.onBack}>
            ‹ Home
          </button>
          <h1 className="page-title mg-title">{name}</h1>
          {exam && <p className="mg-cue mg-exam">{exam}</p>}
          {home?.message && home.state !== "ready" && <p className="mg-where">{withoutPaths(home.message)}</p>}
          {home && ["not_set_up", "needs_upgrade", "no_runtime", "unreadable"].includes(home.state) && (
            <p className="mg-controls" role="group">
              {home.state === "not_set_up" && (
                <button className="mg-start" autoFocus={props.focus} disabled={busy} onClick={() => void setUp()}>
                  Set it up for Magnesium <span className="keys">↩</span>
                </button>
              )}
              {home.state === "needs_upgrade" && (
                <button className="mg-start" autoFocus={props.focus} disabled={busy} onClick={() => void upgrade()}>
                  Upgrade it <span className="keys">↩</span>
                </button>
              )}
              {(home.state === "no_runtime" || home.state === "unreadable") && (
                <button className="mg-start" autoFocus={props.focus} onClick={() => void read()}>
                  Check again <span className="keys">↩</span>
                </button>
              )}
            </p>
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
                    sourceRef(sitting.path!) ? openMaterial(sitting.path!) : void api.openFile(sitting.path!, folder).catch((problem) => props.onNote(String(problem)))
                : null
            }
            onEnd={endPaper}
          />
        )}

        {showNext && (
          <Part head="Next" sub={planned && planned !== "today" ? capital(planned) : undefined} className="mg-now mg-next">
            {current ? (
              <button className="mg-step next-row" data-drive="next" autoFocus={props.focus} onClick={startNext}>
                <Line text={current.title} />
              </button>
            ) : (
              <button className="mg-step next-row" data-drive="new-session" autoFocus={props.focus} onClick={() => props.onNewSession(folder)}>
                New session
              </button>
            )}
            {current?.note && (
              <p className="mg-note">
                <Line text={current.note} />
              </p>
            )}
            {(current || items.length > 0 || said) && (
              <Cue lead={current ? `Step ${step} of ${items.length}.` : items.length > 0 ? "Everything on the plan is done." : ""} said={said} last={current ? last : null} />
            )}
            <p className="mg-controls">
              <button className="mg-start" tabIndex={-1} onClick={current ? startNext : () => props.onNewSession(folder)}>
                {carried ? "Carry on" : "Start"} <span className="keys">↩</span>
              </button>
              {checksDue && (
                <button className="mg-act" data-drive="just-checks" title={props.checksKey ? `Just the quick checks, ${props.checksKey}` : undefined} onClick={() => props.onChecks(folder)}>
                  {alone === 1 ? "Just the quick check" : "Just the quick checks"}
                </button>
              )}
              {current && (
                <button className="mg-act" data-drive="new-session" onClick={() => props.onNewSession(folder)}>
                  Something else…
                </button>
              )}
            </p>
          </Part>
        )}

        {reading && !sitting && (
          <Part head="Sessions" className="mg-sessions">
            {named.length === 0 ? (
              <p className="mg-cue">No sessions yet.</p>
            ) : (
              <div role="list">
                {shownSessions.map(({ info, title }) => (
                  <PageRow key={info.session} name={<Line text={title} />} end={openedDay(info.opened_ms)} onClick={() => props.onOpenSession(folder, info.session, title)} />
                ))}
              </div>
            )}
            {named.length > SESSIONS_SHOWN && (
              <p className="mg-controls mg-more-line">
                <More open={allSessions} more={`Show all ${named.length} sessions`} onToggle={() => setAllSessions((open) => !open)} drive="show-sessions" />
              </p>
            )}
          </Part>
        )}

        {reading && !sitting && (
          <>
            <Part head="Your work" className="mg-work">
              {!record ? (
                <p className="mg-cue">The record could not be read.</p>
              ) : work.length === 0 ? (
                <p className="mg-cue">
                  {record.progress.message && !record.progress.message.startsWith("Nothing is proved yet")
                    ? withoutPaths(record.progress.message)
                    : "Nothing marked yet."}
                </p>
              ) : (
                <p className="mg-legend">
                  Three separate days right from memory prove a skill.
                  {repairs ? " A skill to repair counts again once it is repaired." : ""}
                </p>
              )}
            </Part>
            {days.map((day) =>
              day.exam !== null ? (
                <Part key={day.key} head={day.label} group className="mg-day mg-exam-day">
                  <p className="mg-exam-name">
                    <Line text={day.exam} />
                  </p>
                </Part>
              ) : day.label === "To repair" ? (
                <Part key={day.key} head={day.label} group className="mg-repair">
                  <RepairRun names={day.items.map(({ row }) => row.name)} />
                </Part>
              ) : (
                <Part key={day.key} head={day.label} group className={day.proven ? "mg-proven" : "mg-day"}>
                  <div role="list">{day.items.map(({ row }) => workRow(row, day.proven))}</div>
                </Part>
              ),
            )}
          </>
        )}

        {home && home.papers.length > 0 && !sitting && (
          <Part head="Papers" className="mg-papers">
            <div role="list">
              {home.papers.map((paper) => (
                <div key={paper.path} role="listitem" className="mg-row mg-paper">
                  <span className="mg-row-name">
                    <button
                      type="button"
                      className="mg-name-act"
                      data-drive={`read-${paper.name}`}
                      aria-current={docs.shown === paper.path ? "true" : undefined}
                      title="Read it beside the page"
                      onClick={() => openMaterial(paper.path)}
                    >
                      {paper.name}
                    </button>
                    {(paper.sat || paper.minutes) && <span className="mg-inline">{paper.sat ? "sat before" : `${paper.minutes} min`}</span>}
                  </span>
                  <span className="mg-row-end">
                    <button className="mg-act" data-drive={`paper-${paper.name}`} disabled={busy || !ready} onClick={() => void sit(paper)}>
                      Sit it
                    </button>
                  </span>
                </div>
              ))}
            </div>
          </Part>
        )}

        {reading && !sitting && (
          <Part head="Material" className="mg-material">
            {materials === "none" && <p className="mg-cue">The material folder could not be read.</p>}
            {materials !== null && materials !== "none" && materials.files === 0 && <p className="mg-cue">Nothing here yet. Add anything you want to learn from to this page.</p>}
            {listed && (
              <p className="mg-cue">
                {plural(listed.files, "file", "files")}
                {materials !== null && materials !== "none" && materials.keys > 0 ? `, and ${materials.keys === 1 ? "one answer key" : `${materials.keys} answer keys`} kept out of sight` : ""}.{" "}
                <More open={materialOpen} more={listed.files === 1 ? "Show it" : "Show them"} less={listed.files === 1 ? "Hide it" : "Hide them"} onToggle={() => setMaterialOpen((open) => !open)} drive="material" />
              </p>
            )}
            {listed && materialOpen && (
              <div className="mg-files" role="list">
                {listed.names.map((file) => (
                  <PageRow
                    key={file}
                    className="file-row"
                    nameClass="row-name"
                    current={docs.shown === `Study/materials/${file}`}
                    name={materialTitle(file)}
                    end={mixed ? materialKind(file) : undefined}
                    onClick={() => openMaterial(`Study/materials/${file}`)}
                  />
                ))}
              </div>
            )}
            {listed && materialOpen && listed.files > listed.names.length && <p className="mg-cue">and {listed.files - listed.names.length} more, in the folder</p>}
            {materials !== null && materials !== "none" && (
              <p className="mg-controls mg-material-acts">
                <button className="mg-act" data-drive="add-material" disabled={!ready || busy} onClick={() => void addFiles()}>
                  Add files…
                </button>
                <button className="mg-act" disabled={!ready} onClick={() => props.onPaste(folder)}>
                  Paste text…
                </button>
              </p>
            )}
          </Part>
        )}

        {reading && !sitting && (
          <section aria-label="This space" className="mg-forget">
            <p className="mg-controls">
              <button className="mg-act" disabled={held !== null} title={held ?? "Its folder and sessions stay as they are"} onClick={() => props.onForget(folder)}>
                Take this space off the list
              </button>
            </p>
            {held && <p className="mg-cue">{held}.</p>}
          </section>
        )}
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
