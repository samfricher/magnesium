// Home: what the window opens on. Its first row is what to study next, in the
// workspace studied in most recently, by the rule that workspace's own page
// uses (src/next.ts): the plan's item in hand, which Return starts or carries
// on. With no plan, one that cannot be read, or every item done, the first row
// carries on the most recent session, else starts a new one. Under it, every
// workspace in the library, most recently opened first, with when.
//
// A step this Mac needs before any lesson can run takes the first row's
// place. A choice that leaves lessons running (which copy of the program to
// keep) is a note under the first row instead, so it never takes Return.

import { useEffect, useRef, useState } from "react";

import { mark } from "../harness";
import { firstStep, namedSessions, nextOf, planDay, type Next } from "../next";
import { Line } from "../surface/Rich";
import type { Core, Setup, Workspace } from "../types";
import { displayName, sessionName, whenOpened } from "../words";

/**
 * Which of two programs' versions is later: 1 when `a` is, -1 when `b` is, 0 when the numbers are the same or
 * either cannot be read. "0.3.0+<commit>" is read as 0.3.0.
 */
export function later(a: string, b: string): number {
  const parts = (version: string) => (version.split("+")[0] ?? "").split(".").map((part) => Number.parseInt(part, 10));
  const [x, y] = [parts(a), parts(b)];
  if ([...x, ...y].some((n) => Number.isNaN(n))) return 0;
  for (let at = 0; at < Math.max(x.length, y.length); at++) {
    const [p, q] = [x[at] ?? 0, y[at] ?? 0];
    if (p !== q) return p > q ? 1 : -1;
  }
  return 0;
}

export type Step = { next: string; where: string | null; choices: { label: string; run: () => void }[]; status?: string | null };

/** A choice about this Mac that lessons do not wait on: what it says, and its two answers. */
export type Note = { text: string; choices: { label: string; run: () => void }[] };

/**
 * This app carries another copy of the program than the one installed, and the learner has not chosen. Lessons run
 * the installed copy meanwhile (src/setup.ts), so this is a note, not a step. The version numbers are in Settings,
 * under About: here the note says which is newer and what each choice does.
 */
export function programNote(setup: Setup | null, act: { replace: () => void; keep: () => void }): Note | null {
  const runtime = setup?.runtime;
  if (runtime?.state !== "different" || !runtime.installed || !runtime.bundled) return null;
  const keep = { label: "Keep the one on this Mac", run: act.keep };
  const newer = later(runtime.bundled.version, runtime.installed.version);
  if (newer > 0)
    return {
      text: "Lessons use the version of Magnesium already on this Mac. This app comes with a newer one, which updates each workspace’s record the first time it opens it, after keeping a copy.",
      choices: [{ label: "Use the newer version", run: act.replace }, keep],
    };
  if (newer < 0)
    return {
      text: "Lessons use the version of Magnesium already on this Mac, which is newer than the one this app comes with. Switching would put the older one in its place for every workspace.",
      choices: [keep, { label: "Use this app’s older version", run: act.replace }],
    };
  return {
    text: "Lessons use the version of Magnesium already on this Mac. This app comes with another build of the same version, the one it was made with.",
    choices: [{ label: "Use this app’s build", run: act.replace }, keep],
  };
}

/** The one step this Mac needs before a lesson can run, or null when it needs none. */
export function setupStep(
  setup: Setup | null,
  signingIn: boolean,
  act: { replace: () => void; checkAgain: () => void; openUrl: (url: string) => void; signIn: () => void },
): Step | null {
  if (!setup) return null;
  const { runtime, account } = setup;
  if (runtime.state === "too_old" || runtime.state === "missing" || runtime.state === "failed") {
    const guide = "https://github.com/samfricher/magnesium/blob/main/docs/install.md";
    return {
      next: runtime.state === "too_old" ? "The magnesium program on this Mac is too old for this window" : "The magnesium program could not be set up",
      where: runtime.state === "too_old" ? "Lessons need a newer magnesium program." : "Lessons need the magnesium program, which this window could not put in place.",
      choices: [
        ...(runtime.bundled && runtime.state === "too_old" ? [{ label: "Replace it with this app’s copy", run: act.replace }] : []),
        ...(!runtime.bundled ? [{ label: "Open the install guide", run: () => act.openUrl(guide) }] : []),
        { label: "Check again", run: act.checkAgain },
      ],
    };
  }
  if (!account.installed) {
    return {
      next: `Install ${account.name}`,
      where: `Magnesium teaches through ${account.name}, which this window cannot find on this Mac. It needs a paid Claude plan or an Anthropic Console account.`,
      choices: [
        ...(account.install_page ? [{ label: "Open Anthropic’s install page", run: () => act.openUrl(account.install_page!) }] : []),
        { label: "Check again", run: act.checkAgain },
      ],
    };
  }
  if (!account.signed_in) {
    return {
      next: `Sign in to ${account.name}`,
      where: signingIn ? "Waiting for the sign-in to finish in your browser." : (account.problem ?? `${account.name}’s own sign-in opens Anthropic’s page in your browser. This window never sees your password.`),
      choices: [
        { label: "Sign in", run: act.signIn },
        { label: "Check again", run: act.checkAgain },
      ],
    };
  }
  return null;
}

type Props = {
  api: Core | null;
  workspaces: Workspace[] | null;
  /** The step this Mac needs first, if any. */
  step: Step | null;
  /** A choice about this Mac that lessons do not wait on, if any. */
  note: Note | null;
  /** Home is in front, with nothing over it: the first row takes the focus. */
  focus: boolean;
  notes: string[];
  onContinue: (folder: string, session: string, title: string) => void;
  onStartIn: (folder: string) => void;
  onWorkspace: (folder: string) => void;
  onPickFolder: () => void;
  onNewFolder: () => void;
  onFindFolders: () => void;
  /** Home's column is drawn with its first row: the first screen is ready. */
  onShown?: () => void;
};

/** The most recent session in the library, and its workspace. */
export function mostRecent(workspaces: Workspace[]): Workspace | null {
  let best: Workspace | null = null;
  for (const each of workspaces) if (each.latest && (!best || each.latest.opened_ms > best.latest!.opened_ms)) best = each;
  return best;
}

/**
 * What Home last read of a workspace: its Next, its last goal (the name of a session an earlier window kept), what
 * the session Return opens starts with (src/next.ts, firstStep), and when it was read.
 */
type Read = { next: Next | null; goal: string | null; first: string | null; at: number };

/**
 * How long Home waits for the lead workspace's plan before it draws the first row without it. The plan is read
 * through `magnesium digest`, which the core gives ten seconds; past this, Home draws the row it drew before the plan
 * led (the last session, else New session), keeps it for this visit, and keeps the plan read for the next one.
 */
export const PLAN_WAIT_MS = 400;

// What Home read last, by folder, so that coming back to Home draws the first row at once while it reads again.
const known = new Map<string, Read>();

async function readNext(api: Core, folder: string): Promise<Read> {
  const at = Date.now();
  const [plan, sessions, home] = await Promise.all([api.plan(folder).catch(() => null), api.lessons(folder).catch(() => []), api.home(folder).catch(() => null)]);
  const goal = sessions.some((info) => !info.title) ? (home?.last?.goal ?? null) : null;
  return { next: nextOf(plan?.items ?? [], namedSessions(sessions, goal)), goal, first: firstStep(home), at };
}

function NoteBlock({ note }: { note: Note }) {
  return (
    <div className="home-note" role="group" aria-label="Which Magnesium lessons use">
      <p className="page-note">{note.text}</p>
      <p className="page-acts">
        {note.choices.map((choice) => (
          <button key={choice.label} className="quiet-act" onClick={choice.run}>
            {choice.label}
          </button>
        ))}
      </p>
    </div>
  );
}

export function HomePage(props: Props) {
  const list = useRef<HTMLDivElement>(null);
  const workspaces = props.workspaces ?? [];
  const recent = mostRecent(workspaces);
  // The workspace studied in most recently; in a library with no session yet, the one opened most recently.
  const lead = recent ?? workspaces[0] ?? null;
  // `here`: read on this visit, rather than kept from an earlier one.
  const [read, setRead] = useState<{ folder: string; read: Read; here: boolean } | null>(() => {
    const kept = lead ? known.get(lead.folder) : undefined;
    return lead && kept ? { folder: lead.folder, read: kept, here: false } : null;
  });
  // The lead workspace whose plan took longer than PLAN_WAIT_MS: for this visit its first row is drawn without it.
  const [gaveUp, setGaveUp] = useState<string | null>(null);
  const gaveUpOn = useRef<string | null>(null);
  useEffect(() => {
    if (!lead || !props.api) return;
    let live = true;
    const folder = lead.folder;
    const limit = window.setTimeout(() => {
      if (!live || known.has(folder)) return;
      gaveUpOn.current = folder;
      setGaveUp(folder);
    }, PLAN_WAIT_MS);
    void readNext(props.api, folder).then((fresh) => {
      known.set(folder, fresh);
      // A row already drawn without the plan stays as it is, rather than changing under the learner.
      if (live && gaveUpOn.current !== folder) setRead({ folder, read: fresh, here: true });
    });
    return () => {
      live = false;
      window.clearTimeout(limit);
    };
  }, [lead?.folder, props.api, props.workspaces]);
  const fallback: Read | null = lead && gaveUp === lead.folder ? { next: null, goal: null, first: null, at: 0 } : null;
  const now = (lead && read?.folder === lead.folder ? read.read : null) ?? fallback;
  const next = now?.next ?? null;

  // What the session Return opens starts with. A read kept from an earlier visit is not trusted for it once a lesson
  // in that folder has moved since (its first message may have begun the sitting, and its checks been answered). The
  // line is drawn only with the row's first drawing on this visit: a later read may take it away, never put it there.
  const trusted = read !== null && read.read === now && (read.here || !lead?.latest || lead.latest.last_ms <= read.read.at);
  const firstNow = trusted ? (now?.first ?? null) : null;
  const firstDrawn = useRef<{ folder: string; line: boolean } | null>(null);

  const rows = () => Array.from(list.current?.querySelectorAll<HTMLButtonElement>("button.home-row") ?? []);
  useEffect(() => {
    if (props.focus && !props.step) rows()[0]?.focus({ preventScroll: true });
  }, [props.focus, props.step, props.workspaces, now]);

  const onKeyDown = (event: React.KeyboardEvent) => {
    const step = event.key === "ArrowDown" ? 1 : event.key === "ArrowUp" ? -1 : 0;
    if (step === 0 || event.metaKey || event.altKey) return;
    const all = rows();
    const at = all.indexOf(document.activeElement as HTMLButtonElement);
    if (at === -1) return;
    event.preventDefault();
    all[Math.min(all.length - 1, Math.max(0, at + step))]?.focus();
  };

  const name = recent?.latest ? sessionName(recent.latest, now?.goal ?? null) : null;
  // Until the lead workspace's plan is read the first row is not known: the page waits for it, the first time only and
  // for PLAN_WAIT_MS at most, rather than drawing one row and putting another in its place.
  const waiting = lead !== null && now === null && !props.step;
  if (lead && !waiting && props.workspaces !== null && firstDrawn.current?.folder !== lead.folder) firstDrawn.current = { folder: lead.folder, line: firstNow !== null };
  const first = lead && firstDrawn.current?.folder === lead.folder && firstDrawn.current.line ? firstNow : null;
  const firstLine = first ? <span className="next-first">{first}</span> : null;
  const note = props.note ? <NoteBlock note={props.note} /> : null;
  // For the harness: when Home mounted, and when its column showed.
  mark("home");
  const { onShown } = props;
  useEffect(() => {
    if (waiting || props.workspaces === null) return;
    mark("home-shown");
    onShown?.();
  }, [waiting, props.workspaces, onShown]);

  return (
    <div className="page home-page" data-page="home">
      <div className="page-column" ref={list} onKeyDown={onKeyDown} style={waiting ? { visibility: "hidden" } : undefined}>
        {props.step && (
          <section className="step" aria-label="This Mac">
            <h1 className="page-next">{props.step.next}</h1>
            {props.step.where && <p className="where">{props.step.where}</p>}
            <div className="choices-rest" role="group">
              {props.step.choices.map((choice, at) => (
                <button key={choice.label} className="choice-rest" autoFocus={at === 0 && props.focus} onClick={choice.run}>
                  {choice.label}
                </button>
              ))}
            </div>
            {note}
          </section>
        )}
        {props.workspaces === null ? null : workspaces.length === 0 ? (
          !props.step && (
            <section className="step">
              <h1 className="page-next">Open the folder your course is in</h1>
              <p className="where">Your notes, slides and past papers can be in it already, or be added later.</p>
              <div className="choices-rest" role="group">
                <button className="choice-rest home-row-choice" data-drive="open-folder" autoFocus={props.focus} onClick={props.onPickFolder}>
                  Open a folder…
                </button>
                <button className="choice-rest" onClick={props.onNewFolder}>
                  Make a new folder…
                </button>
                <button className="choice-rest" onClick={props.onFindFolders}>
                  Find folders on this Mac
                </button>
              </div>
              {note}
            </section>
          )
        ) : (
          <>
            {lead && !props.step && next && (
              <section className="home-first next" aria-label="Next">
                <p className="list-head">Next</p>
                <button
                  className="home-row first next-row"
                  data-drive="start"
                  onClick={() => (next.carried ? props.onContinue(lead.folder, next.carried.info.session, next.carried.title) : props.onStartIn(lead.folder))}
                >
                  <span className="next-what">
                    <span className="row-name next-name">
                      <Line text={next.item.title} />
                    </span>
                    {firstLine}
                    {next.item.note && (
                      <span className="next-note">
                        <Line text={next.item.note} />
                      </span>
                    )}
                  </span>
                  <span className="row-when">{[displayName(lead.name), planDay(next.item.day)].filter(Boolean).join(" · ")}</span>
                  <span className="row-key" aria-hidden="true">
                    ↩
                  </span>
                </button>
                {note}
              </section>
            )}
            {lead && !props.step && !next && (
              <section className="home-first" aria-label="Carry on">
                {recent && <p className="list-head">Carry on</p>}
                <button
                  className="home-row first"
                  data-drive="start"
                  onClick={() => (recent?.latest ? props.onContinue(recent.folder, recent.latest.session, name ?? "") : props.onStartIn(lead.folder))}
                >
                  <span className="next-what">
                    <span className="row-name next-name">{recent?.latest ? <Line text={name ?? ""} /> : "New session"}</span>
                    {firstLine}
                  </span>
                  <span className="row-when">
                    {displayName(lead.name)}
                    {recent?.latest ? `, ${whenOpened(recent.latest.opened_ms)}` : ""}
                  </span>
                  <span className="row-key" aria-hidden="true">
                    ↩
                  </span>
                </button>
                {note}
              </section>
            )}
            <section className="home-list" aria-label="Workspaces">
              {workspaces.map((workspace) => (
                <button key={workspace.folder} className="home-row" data-drive={`workspace-${displayName(workspace.name)}`} onClick={() => props.onWorkspace(workspace.folder)}>
                  <span className="row-name">{displayName(workspace.name)}</span>
                  {/* The session carried on says when; its workspace's row does not say it again. */}
                  <span className="row-when">{recent?.folder === workspace.folder && recent.latest && !props.step && !next ? "" : whenOpened(workspace.opened_ms)}</span>
                </button>
              ))}
            </section>
          </>
        )}
        {workspaces.length > 0 && (
          <p className="page-acts">
            <button className="quiet-act" data-drive="open-folder" onClick={props.onPickFolder}>
              Open a folder…
            </button>
            <button className="quiet-act" onClick={props.onNewFolder}>
              Make a new folder…
            </button>
            <button className="quiet-act" onClick={props.onFindFolders}>
              Find folders on this Mac
            </button>
          </p>
        )}
        {props.notes.length > 0 && (
          <div className="notices">
            {props.notes.map((text) => (
              <p key={text} className="record note">
                {text}
              </p>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
