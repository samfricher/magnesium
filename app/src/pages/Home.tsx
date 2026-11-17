// Home: what the window opens on, headed by the day. Its first row is what to
// study next, in the workspace studied in most recently, by the rule that
// workspace's own page uses (src/next.ts): the plan's item in hand, which
// Return starts or carries on. With no plan, one that cannot be read, or every
// item done, the first row carries on the most recent session, else starts a
// new one. When the next sitting there opens with a quick check whose question
// the record stores, that question comes first, with a line to answer it on,
// and the row follows it. Under it, every workspace in the library, most
// recently opened first, with what the record says of its day.
//
// A step this Mac needs before any lesson can run takes the first row's
// place. A choice that leaves lessons running (which copy of the program to
// keep) is a note at the foot of the page instead, under the workspaces and
// the folder actions: it never takes Return, and lessons do not wait on it.

import { useEffect, useLayoutEffect, useRef, useState } from "react";

import { mark } from "../harness";
import { homeDraftFor, keepHomeDraft } from "../homeDraft";
import { checksAlone, dayFacts, firstQuestion, firstStep, namedSessions, nextOf, planDay, type Next } from "../next";
import { Line, Rich } from "../surface/Rich";
import type { Core, Setup, Workspace } from "../types";
import { displayName, longDay, sessionName, whenOpened } from "../words";

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
  /** Just the quick checks due in a folder, in a session of their own; `answer`, words written for the first. */
  onChecks: (folder: string, answer?: { words: string; question: string }) => void;
  /** How many quick checks are due in a folder, as Home read it. */
  onChecksDue?: (folder: string, due: number) => void;
  /** The key for Just the quick checks, as the learner set it. */
  checksKey?: string;
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
type Read = { next: Next | null; goal: string | null; first: string | null; checks: number; question: { text: string; meta: string } | null; redos: number; facts: string; at: number };

/**
 * How long Home waits for the lead workspace's plan before it draws the first row without it. The plan is read
 * through `magnesium digest`, which the core gives ten seconds; past this, Home draws the row it drew before the plan
 * led (the last session, else New session), keeps it for this visit, and keeps the plan read for the next one.
 */
export const PLAN_WAIT_MS = 400;

// What Home read last, by folder, so that coming back to Home draws the first row at once while it reads again.
const known = new Map<string, Read>();
// What each workspace's row last said of its day, by folder: drawn at once on the next visit while it is read again.
const factsKnown = new Map<string, string>();

async function readNext(api: Core, folder: string): Promise<Read> {
  const at = Date.now();
  const [plan, sessions, home] = await Promise.all([api.plan(folder).catch(() => null), api.lessons(folder).catch(() => []), api.home(folder).catch(() => null)]);
  const goal = sessions.some((info) => !info.title) ? (home?.last?.goal ?? null) : null;
  return {
    next: nextOf(plan?.items ?? [], namedSessions(sessions, goal)),
    goal,
    first: firstStep(home),
    checks: checksAlone(home),
    question: firstQuestion(home),
    redos: home?.sitting ? 0 : (home?.opening?.redos ?? 0),
    facts: dayFacts(home),
    at,
  };
}

/** Today, as "2026-11-17". */
function isoToday(): string {
  const now = new Date();
  return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
}

function NoteBlock({ note }: { note: Note }) {
  return (
    <div className="home-note" role="group" aria-label="Which Magnesium lessons use">
      <p className="page-note">{note.text}</p>
      <p className="page-acts">
        {/* programNote puts the choice it recommends first: it is in the text's colour, the other in the quiet grey. */}
        {note.choices.map((choice, at) => (
          <button key={choice.label} className={at === 0 ? "quiet-act strong" : "quiet-act"} onClick={choice.run}>
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
  const fallback: Read | null = lead && gaveUp === lead.folder ? { next: null, goal: null, first: null, checks: 0, question: null, redos: 0, facts: "", at: 0 } : null;
  const now = (lead && read?.folder === lead.folder ? read.read : null) ?? fallback;
  const next = now?.next ?? null;

  // What the session Return opens starts with. A read kept from an earlier visit is not trusted for it once a lesson
  // in that folder has moved since (its first message may have begun the sitting, and its checks been answered). The
  // line is drawn only with the row's first drawing on this visit: a later read may take it away, never put it there.
  const trusted = read !== null && read.read === now && (read.here || !lead?.latest || lead.latest.last_ms <= read.read.at);
  const firstNow = trusted ? (now?.first ?? null) : null;
  const firstDrawn = useRef<{ folder: string; line: boolean; question: boolean } | null>(null);

  const name = recent?.latest ? sessionName(recent.latest, now?.goal ?? null) : null;
  // Until the lead workspace's plan is read the first row is not known: the page waits for it, the first time only and
  // for PLAN_WAIT_MS at most, rather than drawing one row and putting another in its place.
  const waiting = lead !== null && now === null && !props.step;
  const questionNow = trusted && !props.step ? (now?.question ?? null) : null;
  if (lead && !waiting && props.workspaces !== null && firstDrawn.current?.folder !== lead.folder)
    firstDrawn.current = { folder: lead.folder, line: firstNow !== null, question: questionNow !== null };
  const drawnFirst = lead && firstDrawn.current?.folder === lead.folder ? firstDrawn.current : null;
  // The first check's question is set over the row only if it was there when the row was first drawn: coming later,
  // it would push the row down and take the keyboard from under the learner. A later read may take it away.
  const question = drawnFirst?.question ? questionNow : null;
  // Under the question, the row's line about what its session starts with stays only for paper redos, which the
  // question's sitting of checks alone does not take.
  const first = drawnFirst?.line && !(question && (now?.redos ?? 0) === 0) ? firstNow : null;
  const firstLine = first ? <span className="next-first">{first}</span> : null;
  // Quick checks due in the lead workspace can be taken alone, in a few minutes, instead of the row above. Shown only
  // from a read the row's own line trusts: a kept one may count checks a lesson has answered since.
  const checks = lead && trusted && !question && (now?.checks ?? 0) > 0 ? lead.folder : null;
  const { onChecksDue } = props;
  useEffect(() => {
    if (read?.here) onChecksDue?.(read.folder, read.read.checks);
  }, [read, onChecksDue]);
  const checksAct = checks ? (
    <p className="page-acts next-acts">
      <button className="quiet-act" data-drive="just-checks" title={props.checksKey ? `Just the quick checks, ${props.checksKey}` : undefined} onClick={() => props.onChecks(checks)}>
        {(now?.checks ?? 0) === 1 ? "Just the quick check" : "Just the quick checks"}
      </button>
    </p>
  ) : null;
  // The answer line, when a question is set, then the rows: ↑↓ move between them.
  const rows = () => Array.from(list.current?.querySelectorAll<HTMLElement>("textarea.home-answer, button.home-row") ?? []);
  const asked = question !== null;
  useEffect(() => {
    if (props.focus && !props.step) rows()[0]?.focus({ preventScroll: true });
    // The answer line keeps the keyboard once the learner is in it: a fresh read must not move it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.focus, props.step, props.workspaces, asked ? null : now, asked]);

  const onKeyDown = (event: React.KeyboardEvent) => {
    const step = event.key === "ArrowDown" ? 1 : event.key === "ArrowUp" ? -1 : 0;
    if (step === 0 || event.metaKey || event.altKey) return;
    const all = rows();
    const at = all.indexOf(document.activeElement as HTMLElement);
    if (at === -1) return;
    // In the answer line, ↑↓ move its caret until it is at the words' start or end.
    const field = document.activeElement instanceof HTMLTextAreaElement ? document.activeElement : null;
    if (field && (step > 0 ? field.selectionEnd < field.value.length : field.selectionStart > 0)) return;
    event.preventDefault();
    all[Math.min(all.length - 1, Math.max(0, at + step))]?.focus();
  };

  const note = props.note ? <NoteBlock note={props.note} /> : null;
  // For the harness: when Home mounted, and when its column showed.
  mark("home");
  const { onShown } = props;
  const shown = !waiting && props.workspaces !== null;
  useEffect(() => {
    if (!shown) return;
    mark("home-shown");
    onShown?.();
  }, [shown, onShown]);

  // What each workspace's row says of its day. Each is read through the runtime, so the rows are drawn first, with
  // what was read on an earlier visit, and each is filled in when read: the lead workspace's from Home's own read, the
  // others one after another once the page is up, so they never hold up its first drawing.
  const [facts, setFacts] = useState<Record<string, string>>(() => Object.fromEntries(factsKnown));
  const leadFacts = read?.here ? { folder: read.folder, said: read.read.facts } : null;
  useEffect(() => {
    if (!leadFacts) return;
    factsKnown.set(leadFacts.folder, leadFacts.said);
    setFacts((old) => (old[leadFacts.folder] === leadFacts.said ? old : { ...old, [leadFacts.folder]: leadFacts.said }));
  }, [leadFacts?.folder, leadFacts?.said]);
  useEffect(() => {
    if (!shown || !props.api) return;
    const api = props.api;
    let live = true;
    const others = workspaces.filter((workspace) => workspace.folder !== lead?.folder);
    void (async () => {
      for (const workspace of others) {
        const home = await api.home(workspace.folder).catch(() => null);
        if (!live) return;
        const said = dayFacts(home);
        factsKnown.set(workspace.folder, said);
        setFacts((old) => (old[workspace.folder] === said ? old : { ...old, [workspace.folder]: said }));
      }
    })();
    return () => {
      live = false;
    };
    // Read again when the library changes, not on every drawing.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [shown, props.api, props.workspaces]);

  // The answer to the first check, as typed on Home. Return starts a sitting of the due checks alone and gives it
  // these words, which go as the answer once the tutor has posed that check.
  // Kept apart from the page (src/homeDraft.ts) until the words are sent: Home is drawn again when it comes back.
  const [answer, setAnswerState] = useState(() => (lead && question ? homeDraftFor(lead.folder, question.text) : ""));
  const setAnswer = (words: string) => {
    setAnswerState(words);
    if (lead && question) keepHomeDraft({ folder: lead.folder, question: question.text, words });
  };
  const questionText = question?.text ?? null;
  useEffect(() => {
    if (lead && questionText) setAnswerState((old) => old || homeDraftFor(lead.folder, questionText));
  }, [lead?.folder, questionText]);
  const answerBox = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => {
    const el = answerBox.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
  }, [answer, asked]);
  const onAnswerKey = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key !== "Enter" || event.shiftKey || event.altKey || event.metaKey || event.ctrlKey || event.nativeEvent.isComposing) return;
    event.preventDefault();
    if (!lead) return;
    const words = answer.trim();
    props.onChecks(lead.folder, words === "" || !question ? undefined : { words, question: question.text });
  };

  return (
    <div className="page home-page" data-page="home">
      <div className="page-column" ref={list} onKeyDown={onKeyDown} style={waiting ? { visibility: "hidden" } : undefined}>
        {workspaces.length > 0 && <p className="home-day">{longDay(isoToday())}</p>}
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
            {lead && question && (
              <section className="home-question" aria-label="A quick check">
                <p className="home-question-meta">{question.meta}</p>
                {/* Set as the open task is on a lesson's folded page. Its links stay shut, as they are there. */}
                <div className="answer-task home-task" onClick={() => answerBox.current?.focus()}>
                  <Rich text={question.text} final linksOpen={false} />
                </div>
                <div className="foot home-foot">
                  <div className={`line lesson first${answer.trim() !== "" ? " typed" : ""}`}>
                    <div className="field-row">
                      <textarea
                        ref={answerBox}
                        className="field home-answer"
                        data-drive="home-answer"
                        aria-label="Your answer"
                        rows={1}
                        spellCheck
                        placeholder="Your answer"
                        value={answer}
                        onChange={(event) => setAnswer(event.target.value)}
                        onKeyDown={onAnswerKey}
                      />
                      <span className="line-tools">
                        {answer.trim() !== "" && (
                          <span className="line-end" aria-hidden="true">
                            <span className="keys">↩</span> answer
                          </span>
                        )}
                      </span>
                    </div>
                  </div>
                </div>
              </section>
            )}
            {lead && !props.step && next && (
              <section className={`home-first next${question ? " then" : ""}`} aria-label={question ? "Then" : "Next"}>
                <p className="list-head">{question ? "Then" : "Next"}</p>
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
                {checksAct}
              </section>
            )}
            {lead && !props.step && !next && (
              <section className={`home-first${question ? " then" : ""}`} aria-label={question ? "Then" : "Carry on"}>
                {(recent || question) && <p className="list-head">{question ? "Then" : "Carry on"}</p>}
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
                {checksAct}
              </section>
            )}
            <section className="home-list" aria-label="Workspaces">
              {workspaces.map((workspace) => (
                <button key={workspace.folder} className="home-row" data-drive={`workspace-${displayName(workspace.name)}`} onClick={() => props.onWorkspace(workspace.folder)}>
                  <span className="row-name">{displayName(workspace.name)}</span>
                  <span className="row-when">{facts[workspace.folder] ?? ""}</span>
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
        {note}
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
