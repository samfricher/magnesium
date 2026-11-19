import { useEffect, useLayoutEffect, useRef, useState } from "react";

import { mark } from "../harness";
import { selecting } from "../selection";
import { homeDraftFor, keepHomeDraft } from "../homeDraft";
import { checksAlone, courseFact, dueWords, firstQuestion, latestSkill, namedSessions, nextOf, openedDay, startsWithWords, planDay, type Next } from "../next";
import { Line, Rich } from "../surface/Rich";
import { Cue, Part, Row, useMargin } from "./Margin";
import { useClaimKeyboard } from "./TabPage";
import type { Core, Home, Setup, Workspace } from "../types";
import { displayName, sessionName, shortDay } from "../words";

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

export type Note = { text: string; choices: { label: string; run: () => void }[] };

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
  step: Step | null;
  note: Note | null;
  focus: boolean;
  front: boolean;
  notes: string[];
  onContinue: (folder: string, session: string, title: string) => void;
  onStartIn: (folder: string) => void;
  onNewSession: (folder: string) => void;
  onChecks: (folder: string, answer?: { words: string; question: string; id: string | null }) => void;
  onChecksDue?: (folder: string, due: number) => void;
  checksKey?: string;
  onWorkspace: (folder: string) => void;
  onPickFolder: () => void;
  onNewFolder: () => void;
  onFindFolders: () => void;
  onShown?: () => void;
};

export function mostRecent(workspaces: Workspace[]): Workspace | null {
  let best: Workspace | null = null;
  for (const each of workspaces) if (each.latest && (!best || each.latest.opened_ms > best.latest!.opened_ms)) best = each;
  return best;
}

export function leadOf(workspaces: Workspace[]): Workspace | null {
  return mostRecent(workspaces) ?? workspaces[0] ?? null;
}

type Facts = Pick<Home, "checks_due" | "redos_due" | "sitting" | "exam" | "can_count_today">;
const factsOf = (home: Home | null): Facts | null => (home ? { checks_due: home.checks_due, redos_due: home.redos_due, sitting: home.sitting, exam: home.exam, can_count_today: home.can_count_today } : null);

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

export const PLAN_WAIT_MS = 400;

const known = new Map<string, Read>();
const factsKnown = new Map<string, Facts | null>();

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

function isoToday(): string {
  const now = new Date();
  return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
}

function NoteBlock({ note }: { note: Note }) {
  return (
    <div className="home-note" role="group" aria-label="Which Magnesium lessons use">
      <p className="page-note">{note.text}</p>
      <p className="page-acts">
        {note.choices.map((choice, at) => (
          <button key={choice.label} className={at === 0 ? "quiet-act strong" : "quiet-act"} onClick={choice.run}>
            {choice.label}
          </button>
        ))}
      </p>
    </div>
  );
}

function Start({ label, run, focus, drive }: { label: string; run: () => void; focus?: boolean; drive?: string }) {
  return (
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
  const [read, setRead] = useState<{ folder: string; read: Read; here: boolean } | null>(() => {
    const kept = lead ? known.get(lead.folder) : undefined;
    return lead && kept ? { folder: lead.folder, read: kept, here: false } : null;
  });
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

  const trusted = read !== null && read.read === now && (read.here || !lead?.latest || lead.latest.last_ms <= read.read.at);
  const saidNow = trusted ? (now?.opening ?? dueWords(now?.facts ?? null)) : null;
  const firstDrawn = useRef<{ folder: string; line: boolean; question: boolean } | null>(null);
  if (!props.front) firstDrawn.current = null;

  const name = recent?.latest ? sessionName(recent.latest, now?.goal ?? null) : null;
  const waiting = lead !== null && now === null && !props.step;
  const questionNow = trusted && !props.step ? (now?.question ?? null) : null;
  if (lead && !waiting && props.workspaces !== null && firstDrawn.current?.folder !== lead.folder)
    firstDrawn.current = { folder: lead.folder, line: saidNow !== null, question: questionNow !== null };
  const drawnFirst = lead && firstDrawn.current?.folder === lead.folder ? firstDrawn.current : null;
  const question = drawnFirst?.question ? questionNow : null;
  const said = drawnFirst?.line && !question ? saidNow : null;
  const thenSaid = drawnFirst?.line && question && (now?.redos ?? 0) > 0 ? saidNow : null;
  const checks = lead && trusted && !question && (now?.checks ?? 0) > 0 ? lead.folder : null;
  const { onChecksDue } = props;
  useEffect(() => {
    if (read?.here) onChecksDue?.(read.folder, read.read.checks);
  }, [read, onChecksDue]);
  const [day, setDay] = useState(isoToday);
  useEffect(() => {
    const now = new Date();
    const midnight = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1, 0, 0, 1).getTime();
    const timer = window.setTimeout(() => setDay(isoToday()), Math.max(1000, midnight - now.getTime()));
    return () => window.clearTimeout(timer);
  }, [day]);
  const hung = useMargin(list, ".page", [workspaces.length, props.step]);

  const rows = () => Array.from(list.current?.querySelectorAll<HTMLElement>("[data-row]") ?? []);
  const asked = question !== null;
  const askedShown = useRef(false);
  useEffect(() => {
    if (props.front) askedShown.current = asked;
  }, [props.front, asked]);
  const askedAsLeft = useRef<boolean | null>(null);
  useEffect(() => {
    if (!props.front) askedAsLeft.current = askedShown.current;
  }, [props.front]);
  const claimKeyboard = useClaimKeyboard();
  useEffect(() => {
    if (!props.focus || props.step) return;
    const newQuestion = asked && askedAsLeft.current === false;
    askedAsLeft.current = null;
    if (newQuestion) {
      rows()[0]?.focus({ preventScroll: true });
      claimKeyboard();
      return;
    }
    if (!list.current?.contains(document.activeElement)) rows()[0]?.focus({ preventScroll: true });
  }, [props.focus, props.step, props.workspaces, asked ? null : now, asked]);

  const onKeyDown = (event: React.KeyboardEvent) => {
    const step = event.key === "ArrowDown" ? 1 : event.key === "ArrowUp" ? -1 : 0;
    if (step === 0 || event.metaKey || event.altKey) return;
    const all = rows();
    const at = all.indexOf(document.activeElement as HTMLElement);
    if (at === -1) return;
    const field = document.activeElement instanceof HTMLTextAreaElement ? document.activeElement : null;
    if (field && (step > 0 ? field.selectionEnd < field.value.length : field.selectionStart > 0)) return;
    event.preventDefault();
    all[Math.min(all.length - 1, Math.max(0, at + step))]?.focus();
  };

  const note = props.note ? <NoteBlock note={props.note} /> : null;
  mark("home");
  const { onShown } = props;
  const shown = !waiting && props.workspaces !== null;
  useEffect(() => {
    if (!shown) return;
    mark("home-shown");
    onShown?.();
  }, [shown, onShown]);

  const [facts, setFacts] = useState<Record<string, Facts | null>>(() => Object.fromEntries(factsKnown));
  const leadFacts = read?.here ? { folder: read.folder, said: read.read.facts } : null;
  useEffect(() => {
    if (!leadFacts) return;
    factsKnown.set(leadFacts.folder, leadFacts.said);
    setFacts((old) => (old[leadFacts.folder] === leadFacts.said ? old : { ...old, [leadFacts.folder]: leadFacts.said }));
  }, [leadFacts?.folder, leadFacts?.said]);
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
  }, [shown, props.api, props.workspaces, day, visit]);

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
      <div className="answer-task home-task" onClick={() => !selecting() && answerBox.current?.focus()}>
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
