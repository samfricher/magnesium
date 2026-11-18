// Home: what the window opens on, set on the lesson's own column with each part's name in the margin. Today: what
// to study next, in the workspace studied in most recently, by the rule that workspace's own page uses (src/next.ts):
// the plan's item in hand, which Return starts or carries on. With no plan, one that cannot be read, or every item
// done, it carries on the most recent session, else starts a new one. When the next sitting there opens with a quick
// check whose question the record stores, that question takes Today, with a line to answer it on, and the step
// follows it under Then. Under it, Courses: every workspace in the library, most recently opened first, each with the
// one thing the record says of its day.
//
// A step this Mac needs before any lesson can run takes Today's place. A choice that leaves lessons running (which
// copy of the program to keep) is a note at the foot of the page instead, under the courses and the folder actions:
// it never takes Return, and lessons do not wait on it.

import { useEffect, useLayoutEffect, useRef, useState } from "react";

import { mark } from "../harness";
import { homeDraftFor, keepHomeDraft } from "../homeDraft";
import { checksAlone, courseFact, dueWords, firstQuestion, latestSkill, namedSessions, nextOf, openedDay, startsWithWords, planDay, type Next } from "../next";
import { Line, Rich } from "../surface/Rich";
import { Cue, Part, Row, useMargin } from "./Margin";
import { useClaimKeyboard } from "./TabPage";
import type { Core, Home, Setup, Workspace } from "../types";
import { displayName, sessionName, shortDay } from "../words";

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
      text: "Lessons use the version of Magnesium already on this Mac. This app comes with a newer one, which updates each space’s record the first time it opens it, after keeping a copy.",
      choices: [{ label: "Use the newer version", run: act.replace }, keep],
    };
  if (newer < 0)
    return {
      text: "Lessons use the version of Magnesium already on this Mac, which is newer than the one this app comes with. Switching would put the older one in its place for every space.",
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
  /** Home is in front, with nothing over it: the step takes the focus. */
  focus: boolean;
  /** Home's tab is in front. Behind another tab Home stays on the page, read again as the library changes. */
  front: boolean;
  notes: string[];
  onContinue: (folder: string, session: string, title: string) => void;
  onStartIn: (folder: string) => void;
  /** A new session in a folder, away from the plan's item: "Something else…". */
  onNewSession: (folder: string) => void;
  /** Just the quick checks due in a folder, in a session of their own; `answer`, words written for the first. */
  onChecks: (folder: string, answer?: { words: string; question: string; id: string | null }) => void;
  /** How many quick checks are due in a folder, as Home read it. */
  onChecksDue?: (folder: string, due: number) => void;
  /** The key for Just the quick checks, as the learner set it. */
  checksKey?: string;
  onWorkspace: (folder: string) => void;
  onPickFolder: () => void;
  onNewFolder: () => void;
  onFindFolders: () => void;
  /** Home's column is drawn with its step: the first screen is ready. */
  onShown?: () => void;
};

/** The most recent session in the library, and its workspace. */
export function mostRecent(workspaces: Workspace[]): Workspace | null {
  let best: Workspace | null = null;
  for (const each of workspaces) if (each.latest && (!best || each.latest.opened_ms > best.latest!.opened_ms)) best = each;
  return best;
}

/** The workspace Home's step is about: the one studied in most recently; in a library with no session yet, the one opened most recently. */
export function leadOf(workspaces: Workspace[]): Workspace | null {
  return mostRecent(workspaces) ?? workspaces[0] ?? null;
}

/** What a course's row says of its day comes from these, as the record reads them. */
type Facts = Pick<Home, "checks_due" | "redos_due" | "sitting" | "exam" | "can_count_today">;
const factsOf = (home: Home | null): Facts | null => (home ? { checks_due: home.checks_due, redos_due: home.redos_due, sitting: home.sitting, exam: home.exam, can_count_today: home.can_count_today } : null);

/**
 * What Home last read of a workspace: its Next, its last goal (the name of a session an earlier window kept), what
 * the session Return opens starts with (src/next.ts), where the plan's item
 * stands in the plan, the skill of the record's latest marked answer, and when it was read.
 */
type Read = {
  next: Next | null;
  goal: string | null;
  opening: string | null;
  steps: { at: number; of: number } | null;
  checks: number;
  question: { text: string; meta: string; id: string | null } | null;
  redos: number;
  facts: Facts | null;
  last: string | null;
  at: number;
};

/**
 * How long Home waits for the lead workspace's plan before it draws the step without it. The plan is read through
 * `magnesium digest`, which the core gives ten seconds; past this, Home draws the step it drew before the plan led
 * (the last session, else New session), keeps it for this visit, and keeps the plan read for the next one.
 */
export const PLAN_WAIT_MS = 400;

// What Home read last, by folder, so that coming back to Home draws the step at once while it reads again.
const known = new Map<string, Read>();
// What each workspace's row last read of its day, by folder: drawn at once on the next visit while it is read again.
const factsKnown = new Map<string, Facts | null>();

/**
 * Reads what Home's step says of a folder, for the window to wait on before it shows Home, or null when Home knows it
 * from an earlier visit and can draw the step at once.
 */
export function readAhead(api: Core, folder: string): Promise<void> | null {
  if (known.has(folder)) return null;
  return readNext(api, folder).then((fresh) => void known.set(folder, fresh));
}

async function readNext(api: Core, folder: string): Promise<Read> {
  const at = Date.now();
  const [plan, sessions, home, record] = await Promise.all([
    api.plan(folder).catch(() => null),
    api.lessons(folder).catch(() => []),
    api.home(folder).catch(() => null),
    Promise.all([api.progress(null, folder), api.marks(folder).catch(() => null)]).catch(() => null),
  ]);
  const goal = sessions.some((info) => !info.title) ? (home?.last?.goal ?? null) : null;
  const items = plan?.items ?? [];
  const next = nextOf(items, namedSessions(sessions, goal));
  const rows = record ? [...record[0].in_progress, ...record[0].confirmed, ...record[0].untested] : [];
  return {
    next,
    goal,
    opening: startsWithWords(home),
    steps: next ? { at: items.indexOf(next.item) + 1, of: items.length } : null,
    checks: checksAlone(home),
    question: firstQuestion(home),
    redos: home?.sitting ? 0 : (home?.opening?.redos ?? 0),
    facts: factsOf(home),
    last: record ? latestSkill(rows, record[1] ?? []) : null,
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

/** A control the step's own Return also takes, as "Start ↩" says: the one word on the page in the ink. */
function Start({ label, run, focus, drive }: { label: string; run: () => void; focus?: boolean; drive?: string }) {
  return (
    // Return on the step already does this, so the keyboard passes over it unless it is the page's first control.
    <button className="mg-start" data-drive={drive} tabIndex={focus ? undefined : -1} autoFocus={focus} onClick={run}>
      {label} <span className="keys">↩</span>
    </button>
  );
}

export function HomePage(props: Props) {
  const list = useRef<HTMLDivElement>(null);
  const workspaces = props.workspaces ?? [];
  const recent = mostRecent(workspaces);
  const lead = leadOf(workspaces);
  // `here`: read on this visit, rather than kept from an earlier one.
  const [read, setRead] = useState<{ folder: string; read: Read; here: boolean } | null>(() => {
    const kept = lead ? known.get(lead.folder) : undefined;
    return lead && kept ? { folder: lead.folder, read: kept, here: false } : null;
  });
  // The lead workspace whose plan took longer than PLAN_WAIT_MS: for this visit its step is drawn without it.
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
      // A step already drawn without the plan stays as it is, rather than changing under the learner.
      if (live && gaveUpOn.current !== folder) setRead({ folder, read: fresh, here: true });
    });
    return () => {
      live = false;
      window.clearTimeout(limit);
    };
  }, [lead?.folder, props.api, props.workspaces]);
  const fallback: Read | null =
    lead && gaveUp === lead.folder ? { next: null, goal: null, opening: null, steps: null, checks: 0, question: null, redos: 0, facts: null, last: null, at: 0 } : null;
  const now = (lead && read?.folder === lead.folder ? read.read : null) ?? fallback;
  const next = now?.next ?? null;

  // What the session Return opens starts with, or the quick checks due when it carries a sitting on. A read kept from
  // an earlier visit is not trusted for it once a lesson in that folder has moved since (its first message may have
  // begun the sitting, and its checks been answered). The words are drawn only with the step's first drawing on this
  // visit: a later read may take them away, never put them there.
  const trusted = read !== null && read.read === now && (read.here || !lead?.latest || lead.latest.last_ms <= read.read.at);
  const saidNow = trusted ? (now?.opening ?? dueWords(now?.facts ?? null)) : null;
  const firstDrawn = useRef<{ folder: string; line: boolean; question: boolean } | null>(null);
  // Behind another tab nobody sees the step, so a read then may still set the question over it: what Home holds when
  // it comes to the front is what it is first drawn with.
  if (!props.front) firstDrawn.current = null;

  const name = recent?.latest ? sessionName(recent.latest, now?.goal ?? null) : null;
  // Until the lead workspace's plan is read the step is not known: the page waits for it, the first time only and
  // for PLAN_WAIT_MS at most, rather than drawing one step and putting another in its place.
  const waiting = lead !== null && now === null && !props.step;
  const questionNow = trusted && !props.step ? (now?.question ?? null) : null;
  if (lead && !waiting && props.workspaces !== null && firstDrawn.current?.folder !== lead.folder)
    firstDrawn.current = { folder: lead.folder, line: saidNow !== null, question: questionNow !== null };
  const drawnFirst = lead && firstDrawn.current?.folder === lead.folder ? firstDrawn.current : null;
  // The first check's question takes Today only if it was there when the step was first drawn: coming later, it would
  // push the step down and take the keyboard from under the learner. A later read may take it away.
  const question = drawnFirst?.question ? questionNow : null;
  const said = drawnFirst?.line && !question ? saidNow : null;
  // Under the question, what the step's session starts with is said only for paper redos, which the question's
  // sitting of checks alone does not take.
  const thenSaid = drawnFirst?.line && question && (now?.redos ?? 0) > 0 ? saidNow : null;
  // Quick checks due in the lead workspace can be taken alone, in a few minutes, instead of the step. Offered only
  // from a read the cue trusts: a kept one may count checks a lesson has answered since.
  const checks = lead && trusted && !question && (now?.checks ?? 0) > 0 ? lead.folder : null;
  const { onChecksDue } = props;
  useEffect(() => {
    if (read?.here) onChecksDue?.(read.folder, read.read.checks);
  }, [read, onChecksDue]);
  // The day in Today's margin, which moves on at midnight when Home is left open.
  const [day, setDay] = useState(isoToday);
  useEffect(() => {
    const now = new Date();
    const midnight = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1, 0, 0, 1).getTime();
    const timer = window.setTimeout(() => setDay(isoToday()), Math.max(1000, midnight - now.getTime()));
    return () => window.clearTimeout(timer);
  }, [day]);
  const hung = useMargin(list, ".page", [workspaces.length, props.step]);

  // The answer line, when a question is set, then the step and the courses: ↑↓ move between them.
  const rows = () => Array.from(list.current?.querySelectorAll<HTMLElement>("[data-row]") ?? []);
  const asked = question !== null;
  // Whether a question was set in Today while Home was last in front. Not what the render that sends Home behind
  // computes: that render may already hold a question Home read but did not draw (its step was drawn).
  const askedShown = useRef(false);
  useEffect(() => {
    if (props.front) askedShown.current = asked;
  }, [props.front, asked]);
  // The same, kept as Home went behind another tab: null while Home is in front.
  const askedAsLeft = useRef<boolean | null>(null);
  useEffect(() => {
    if (!props.front) askedAsLeft.current = askedShown.current;
  }, [props.front]);
  const claimKeyboard = useClaimKeyboard();
  useEffect(() => {
    if (!props.focus || props.step) return;
    // A question set while Home was behind takes the keyboard as Home comes back, over the step that had it: typing
    // then writes the answer, and Return starts the checks with it.
    const newQuestion = asked && askedAsLeft.current === false;
    askedAsLeft.current = null;
    if (newQuestion) {
      rows()[0]?.focus({ preventScroll: true });
      claimKeyboard();
      return;
    }
    // Otherwise a fresh read never takes the keyboard off a row or the answer line the learner is on: Home stays on
    // the page behind other tabs and reads again as the library changes, and its tab gives the keyboard back.
    if (!list.current?.contains(document.activeElement)) rows()[0]?.focus({ preventScroll: true });
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

  // What each course's row says of its day. Each is read through the runtime, so the rows are drawn first, with what
  // was read on an earlier visit, and each is filled in when read: the lead workspace's from Home's own read, the
  // others one after another once the page is up, so they never hold up its first drawing.
  const [facts, setFacts] = useState<Record<string, Facts | null>>(() => Object.fromEntries(factsKnown));
  const leadFacts = read?.here ? { folder: read.folder, said: read.read.facts } : null;
  useEffect(() => {
    if (!leadFacts) return;
    factsKnown.set(leadFacts.folder, leadFacts.said);
    setFacts((old) => (old[leadFacts.folder] === leadFacts.said ? old : { ...old, [leadFacts.folder]: leadFacts.said }));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [leadFacts?.folder, leadFacts?.said]);
  // A row's fact changes with its workspace's record and the day, so behind other tabs a row reads again only when its
  // workspace was studied since or the day moved on: each read runs the runtime twice, and the library is read again
  // after every turn of a lesson. Each time Home's tab comes to the front every row reads again, for what changed
  // where the window does not hear of it (a lesson in a terminal).
  const [visit, setVisit] = useState(0);
  useEffect(() => {
    if (props.front) setVisit((n) => n + 1);
  }, [props.front]);
  const factsRead = useRef(new Map<string, string>());
  useEffect(() => {
    if (!shown || !props.api) return;
    const api = props.api;
    let live = true;
    const others = workspaces.filter((workspace) => workspace.folder !== lead?.folder);
    void (async () => {
      for (const workspace of others) {
        const as = `${workspace.latest?.last_ms ?? 0} ${day} ${visit}`;
        if (factsRead.current.get(workspace.folder) === as) continue;
        const home = await api.home(workspace.folder).catch(() => null);
        if (!live) return;
        factsRead.current.set(workspace.folder, as);
        const said = factsOf(home);
        factsKnown.set(workspace.folder, said);
        setFacts((old) => (JSON.stringify(old[workspace.folder]) === JSON.stringify(said) ? old : { ...old, [workspace.folder]: said }));
      }
    })();
    return () => {
      live = false;
    };
    // Read again when the library changes, the day moves on or Home comes to the front, not on every drawing.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [shown, props.api, props.workspaces, day, visit]);

  // The answer to the first check, as typed on Home. Return starts a sitting of the due checks alone and gives it
  // these words, which go as the answer once the tutor has posed that check.
  // Kept apart from the page (src/homeDraft.ts) until the words are sent, by the session that takes them. The line
  // reads them from there again when the question changes and when Home comes to the front, so words sent since are
  // gone from it, and words for one check never stand under another.
  const [answer, setAnswerState] = useState(() => (lead && question ? homeDraftFor(lead.folder, question.text) : ""));
  const setAnswer = (words: string) => {
    setAnswerState(words);
    if (lead && question) keepHomeDraft({ folder: lead.folder, question: question.text, words });
  };
  const questionText = question?.text ?? null;
  useEffect(() => {
    if (lead && questionText) setAnswerState(homeDraftFor(lead.folder, questionText));
  }, [lead?.folder, questionText, props.front]);
  const answerBox = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => {
    const el = answerBox.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
  }, [answer, asked]);
  const sendAnswer = () => {
    if (!lead) return;
    const words = answer.trim();
    props.onChecks(lead.folder, words === "" || !question ? undefined : { words, question: question.text, id: question.id });
  };
  const onAnswerKey = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key !== "Enter" || event.shiftKey || event.altKey || event.metaKey || event.ctrlKey || event.nativeEvent.isComposing) return;
    event.preventDefault();
    sendAnswer();
  };

  // The step: the plan's item, else the most recent session carried on, else a new session.
  const course = lead ? displayName(lead.name) : "";
  const carries = next ? next.carried !== null : recent?.latest !== undefined && recent?.latest !== null;
  const start = () => {
    if (!lead) return;
    if (next) next.carried ? props.onContinue(lead.folder, next.carried.info.session, next.carried.title) : props.onStartIn(lead.folder);
    else if (recent?.latest) props.onContinue(recent.folder, recent.latest.session, name ?? "");
    else props.onStartIn(lead.folder);
  };
  const title = next ? next.item.title : recent?.latest ? (name ?? "") : "New session";
  const planned = next ? planDay(next.item.day) : "";
  const steps = next ? now?.steps : null;
  const where = next
    ? `${course}${steps && steps.at > 0 ? `, step ${steps.at} of ${steps.of}` : ""}${planned && planned !== "today" ? `, planned for ${planned}` : ""}.`
    : recent?.latest
      ? `${course}, last opened ${openedDay(recent.latest.opened_ms)}.`
      : `${course}.`;
  const startLabel = carries ? "Carry on" : "Start";

  const nowBlock = lead && (
    <>
      <button className="mg-step" data-drive="start" data-row="" onClick={start}>
        <Line text={title} />
      </button>
      {next?.item.note && (
        <p className="mg-note">
          <Line text={next.item.note} />
        </p>
      )}
      <Cue lead={where} said={said} last={now?.last ?? null} />
      <p className="mg-controls">
        <Start label={startLabel} run={start} />
        {checks && (
          <button className="mg-act" data-drive="just-checks" title={props.checksKey ? `Just the quick checks, ${props.checksKey}` : undefined} onClick={() => props.onChecks(checks)}>
            {(now?.checks ?? 0) === 1 ? "Just the quick check" : "Just the quick checks"}
          </button>
        )}
        {(next || recent?.latest) && (
          <button className="mg-act" data-drive="something-else" onClick={() => props.onNewSession(lead.folder)}>
            Something else…
          </button>
        )}
      </p>
    </>
  );

  const asking = lead && question && (
    <>
      <p className="mg-cue mg-kicker">{question.meta}</p>
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
              data-row=""
              aria-label="Your answer"
              rows={1}
              spellCheck
              placeholder="Your answer"
              value={answer}
              onChange={(event) => setAnswer(event.target.value)}
              onKeyDown={onAnswerKey}
            />
            <span className="line-tools">
              {/* Return on the line does this; the words say so in every look of the line. */}
              <button className="quiet-act mg-check" tabIndex={-1} onClick={sendAnswer}>
                Check <span className="keys">↩</span>
              </button>
            </span>
          </div>
        </div>
      </div>
    </>
  );

  const today = shortDay(day);
  return (
    <div className="page home-page" data-page="home">
      <div className={`page-column mg${hung ? "" : " stacked"}`} ref={list} onKeyDown={onKeyDown} style={waiting ? { visibility: "hidden" } : undefined}>
        {props.step && (
          <Part head="Today" sub={today} className="mg-now" label="This Mac">
            <h1 className="mg-step-text">{props.step.next}</h1>
            {props.step.where && <p className="mg-where">{props.step.where}</p>}
            <p className="mg-controls" role="group">
              {props.step.choices.map((choice, at) =>
                at === 0 ? (
                  <Start key={choice.label} label={choice.label} run={choice.run} focus={props.focus} />
                ) : (
                  <button key={choice.label} className="mg-act" onClick={choice.run}>
                    {choice.label}
                  </button>
                ),
              )}
            </p>
          </Part>
        )}
        {props.workspaces === null ? null : workspaces.length === 0 ? (
          !props.step && (
            <Part head="Today" sub={today} className="mg-now">
              <h1 className="mg-step-text">Create a space to start learning</h1>
              <p className="mg-controls">
                <Start label="Open a folder…" run={props.onPickFolder} focus={props.focus} drive="open-folder" />
                <button className="mg-act" onClick={props.onNewFolder}>
                  Make a new folder…
                </button>
                <button className="mg-act" onClick={props.onFindFolders}>
                  Find spaces on this Mac
                </button>
              </p>
            </Part>
          )
        ) : (
          <>
            {lead && !props.step && (
              <Part head="Today" sub={today} className={`mg-now${question ? " asking" : ""}`}>
                {question ? asking : nowBlock}
              </Part>
            )}
            {lead && !props.step && question && (
              <Part head="Then" className="mg-then-part">
                <p className="mg-then">
                  <span>
                    <Line text={title} />, in {course}.{thenSaid ? ` ${thenSaid}` : ""}
                  </span>
                  <button className="mg-act" data-drive="start" data-row="" onClick={start}>
                    {carries ? "Carry on instead" : "Start it instead"}
                  </button>
                </p>
              </Part>
            )}
            <Part head="Spaces" className="mg-courses">
              <div role="list">
                {workspaces.map((workspace) => (
                  <Row
                    key={workspace.folder}
                    drive={`workspace-${displayName(workspace.name)}`}
                    onClick={() => props.onWorkspace(workspace.folder)}
                    name={displayName(workspace.name)}
                    end={workspace.folder in facts || workspace.latest ? courseFact(facts[workspace.folder] ?? null, workspace.latest?.last_ms, !props.step && workspace.folder === lead?.folder) : ""}
                  />
                ))}
              </div>
            </Part>
          </>
        )}
        {workspaces.length > 0 && (
          <p className="mg-controls mg-folder-acts">
            <button className="mg-act" data-drive="open-folder" onClick={props.onPickFolder}>
              Open a folder…
            </button>
            <button className="mg-act" onClick={props.onNewFolder}>
              Make a new folder…
            </button>
            <button className="mg-act" onClick={props.onFindFolders}>
              Find spaces on this Mac
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
