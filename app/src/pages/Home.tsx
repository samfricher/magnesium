// Home: what the window opens on. A plain list: first the most recent
// session, named by its goal, with its workspace, focused so that Return
// continues it; then every workspace in the library, most recently opened
// first, with when. No levels, counts, due items or countdowns.
//
// Before this Mac can run a lesson, the one step it needs is said first, in
// the same quiet words the surface used before (#67).

import { useEffect, useRef, useState } from "react";

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

/** The one step this Mac needs before a lesson, or null when it needs none. */
export function setupStep(
  setup: Setup | null,
  signingIn: boolean,
  act: { replace: () => void; keep: () => void; checkAgain: () => void; openUrl: (url: string) => void; signIn: () => void },
): Step | null {
  if (!setup) return null;
  const { runtime, account } = setup;
  // The version numbers are in Settings, under About: here the step says which is newer and what each choice does.
  if (runtime.state === "different" && runtime.installed && runtime.bundled) {
    const use = { label: "Use this app’s copy", run: act.replace };
    const keep = { label: "Keep the one on this Mac", run: act.keep };
    const newer = later(runtime.bundled.version, runtime.installed.version);
    if (newer > 0)
      return {
        next: "This app has a newer magnesium program",
        where: "Lessons use the one already on this Mac until you replace it. The newer one updates each workspace’s record the first time it opens it, after keeping a copy.",
        choices: [use, keep],
      };
    if (newer < 0)
      return {
        next: "This Mac has a newer magnesium program than this app",
        where: "Lessons can go on using it. This app’s copy is older, and would replace it for every workspace.",
        choices: [keep, use],
      };
    return {
      next: "Another copy of the magnesium program is on this Mac",
      where: "Lessons use one copy for every workspace. This app’s is the one it was built with.",
      choices: [use, keep],
    };
  }
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
  /** Home is in front, with nothing over it: the first row takes the focus. */
  focus: boolean;
  notes: string[];
  onContinue: (folder: string, session: string, title: string) => void;
  onStartIn: (folder: string) => void;
  onWorkspace: (folder: string) => void;
  onPickFolder: () => void;
  onNewFolder: () => void;
  onFindFolders: () => void;
};

/** The most recent session in the library, and its workspace. */
export function mostRecent(workspaces: Workspace[]): Workspace | null {
  let best: Workspace | null = null;
  for (const each of workspaces) if (each.latest && (!best || each.latest.opened_ms > best.latest!.opened_ms)) best = each;
  return best;
}

export function HomePage(props: Props) {
  const list = useRef<HTMLDivElement>(null);
  const workspaces = props.workspaces ?? [];
  const recent = mostRecent(workspaces);
  const first = recent ?? workspaces[0] ?? null;
  // A session kept by an earlier window has no name of its own: its folder's last goal stands for it.
  const [goalOf, setGoalOf] = useState<Record<string, string>>({});
  useEffect(() => {
    if (!recent?.latest || recent.latest.title || goalOf[recent.folder] !== undefined || !props.api) return;
    void props.api.home(recent.folder).then(
      (home) => setGoalOf((old) => ({ ...old, [recent.folder]: home.last?.goal ?? "" })),
      () => setGoalOf((old) => ({ ...old, [recent.folder]: "" })),
    );
  }, [recent, goalOf, props.api]);

  const rows = () => Array.from(list.current?.querySelectorAll<HTMLButtonElement>("button.home-row") ?? []);
  useEffect(() => {
    if (props.focus && !props.step) rows()[0]?.focus({ preventScroll: true });
  }, [props.focus, props.step, props.workspaces]);

  const onKeyDown = (event: React.KeyboardEvent) => {
    const step = event.key === "ArrowDown" ? 1 : event.key === "ArrowUp" ? -1 : 0;
    if (step === 0 || event.metaKey || event.altKey) return;
    const all = rows();
    const at = all.indexOf(document.activeElement as HTMLButtonElement);
    if (at === -1) return;
    event.preventDefault();
    all[Math.min(all.length - 1, Math.max(0, at + step))]?.focus();
  };

  const name = recent?.latest ? sessionName(recent.latest, goalOf[recent.folder] || null) : null;

  return (
    <div className="page home-page" data-page="home">
      <div className="page-column" ref={list} onKeyDown={onKeyDown}>
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
            </section>
          )
        ) : (
          <>
            {first && !props.step && (
              <section className="home-first" aria-label="Carry on">
                {recent && <p className="list-head">Carry on</p>}
                <button
                  className="home-row first"
                  data-drive="start"
                  onClick={() => (recent?.latest ? props.onContinue(recent.folder, recent.latest.session, name ?? "") : props.onStartIn(first.folder))}
                >
                  <span className="row-name next-name">{recent?.latest ? <Line text={name ?? ""} /> : "New session"}</span>
                  <span className="row-when">
                    {displayName(first.name)}
                    {recent?.latest ? `, ${whenOpened(recent.latest.opened_ms)}` : ""}
                  </span>
                  <span className="row-key" aria-hidden="true">
                    ↩
                  </span>
                </button>
              </section>
            )}
            <section className="home-list" aria-label="Workspaces">
              {workspaces.map((workspace) => (
                <button key={workspace.folder} className="home-row" data-drive={`workspace-${displayName(workspace.name)}`} onClick={() => props.onWorkspace(workspace.folder)}>
                  <span className="row-name">{displayName(workspace.name)}</span>
                  {/* The session carried on says when; its workspace's row does not say it again. */}
                  <span className="row-when">{recent?.folder === workspace.folder && recent.latest && !props.step ? "" : whenOpened(workspace.opened_ms)}</span>
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
