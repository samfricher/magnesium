// The lesson. The whole window is one column: what the tutor just said, and
// one box to answer in. The goal's steps and the clock are two small marks in
// the top strip. Not on screen: tool calls, save messages, token counts, the
// model's name, other sessions, a sidebar.

import { useDeferredValue, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";

import { core } from "../bridge";
import { busy, opening, tutorText, type Card, type SessionState } from "../lesson";
import { hasMath, renderPlain } from "../render";
import { Line, Rich } from "../ui/Rich";
import { Icon } from "../ui/icons";
import { PaletteKey, Ring, TopBar, composing, useNow } from "../ui/bits";
import type { Answer, Decision, Question, Rail as RailData } from "../types";

const CONFIDENCE: [string, string][] = [
  ["sure", "Sure"],
  ["unsure", "Unsure"],
  ["guess", "Guess"],
];

/** The learner's words as typed, with their mathematics drawn. */
function Said({ text, className }: { text: string; className: string }) {
  const html = useMemo(() => renderPlain(text), [text]);
  return <div className={className} dangerouslySetInnerHTML={{ __html: html }} />;
}

/**
 * What the learner said in one turn. The line the window sent to open the
 * sitting is not theirs, so it is one quiet line and not a bubble.
 */
function Turn({ text }: { text: string }) {
  const opener = opening(text);
  if (!opener) return <Said text={text} className="you selectable" />;
  return (
    <div className="resumed">
      {opener.stoppedAt ? (
        <>
          <span>Picking up at</span>
          <Line text={opener.stoppedAt} />
        </>
      ) : (
        <span>Sitting started</span>
      )}
    </div>
  );
}

/** The goal's steps as a row of marks, and the step in hand in words. A click shows them all. */
function Track({ rail }: { rail: RailData | null }) {
  const [open, setOpen] = useState(false);
  const holder = useRef<HTMLDivElement>(null);
  const steps = rail?.steps ?? [];
  // The step in hand: the one the tutor marked, else the first not done.
  const marked = steps.findIndex((s) => s.status === "doing");
  const firstOpen = steps.findIndex((s) => s.status !== "done");
  const current = marked >= 0 ? marked : firstOpen;
  const label = current >= 0 ? steps[current].step : (rail?.goal ?? rail?.next_action ?? null);

  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => {
      if (!holder.current?.contains(event.target as Node)) setOpen(false);
    };
    const key = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", key);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", key);
    };
  }, [open]);

  if (!label) return null;
  return (
    <div className="track-holder" ref={holder}>
      <button className={open ? "track on" : "track"} onClick={() => setOpen(!open)} title="The goal and its steps" data-role="track">
        {steps.length > 0 && (
          <span className="pips" aria-hidden="true">
            {steps.map((step, index) => (
              <i key={index} className={step.status === "done" ? "done" : index === current ? "cur" : undefined} />
            ))}
          </span>
        )}
        <Line text={label} className="track-label" />
      </button>
      {open && (
        <div className="plan" role="dialog" aria-label="The goal and its steps">
          <span className="eyebrow">Goal</span>
          {rail?.goal ? (
            <p className="plan-goal selectable">
              <Line text={rail.goal} />
            </p>
          ) : (
            <p className="plan-goal dim">The tutor sets the goal in its first reply.</p>
          )}
          {steps.length > 0 ? (
            <ol>
              {steps.map((step, index) => (
                <li key={index} className={step.status === "done" ? "done" : index === current ? "cur" : undefined}>
                  <i>{step.status === "done" && <Icon name="check" size={11} />}</i>
                  <Line text={step.step} />
                </li>
              ))}
            </ol>
          ) : (
            rail?.next_action && (
              <ol>
                <li className="cur">
                  <i />
                  <Line text={rail.next_action} />
                </li>
              </ol>
            )
          )}
        </div>
      )}
    </div>
  );
}

/** The block's clock as a ring: how much of the block is used, and the minutes left. */
function Clock({ startedMs, blockMinutes }: { startedMs: number | null; blockMinutes: number }) {
  const now = useNow(15_000);
  const used = startedMs === null ? 0 : Math.max(0, Math.floor((now - startedMs) / 60_000));
  const left = blockMinutes - used;
  const over = left < 0;
  // The ring starts full and empties, as the paper's clock does.
  return (
    <span className={over ? "clock over" : "clock"} title={`${used} of ${blockMinutes} minutes used in this block`} data-role="clock">
      <Ring share={over ? 1 : left / blockMinutes} size={26} stroke={3} tone={over ? "amber" : "ink"} />
      <b>{over ? `+${-left}` : left}</b>
      <small>{over ? "min over" : "min left"}</small>
    </span>
  );
}

function PermissionCard(props: {
  card: Extract<Card, { kind: "permission" }>;
  onDecide: (id: string, decision: Decision) => void;
}) {
  const { card } = props;
  // The card never takes the keyboard. A learner typing ahead would allow it
  // with a space or an Enter they meant for the answer box.
  return (
    <div className="ask" data-card="permission">
      <span className="eyebrow">
        <Icon name="lock" size={13} /> The tutor asks to
      </span>
      <h3>{card.title}</h3>
      {card.detail && <code className="detail selectable">{card.detail}</code>}
      <div className="row">
        <button className="btn primary" onClick={() => props.onDecide(card.id, "allow_once")}>
          Allow once
        </button>
        <button className="btn" onClick={() => props.onDecide(card.id, "refuse")}>
          Refuse
        </button>
        <span className="grow" />
        {card.canAlways && (
          <button className="btn ghost small" onClick={() => props.onDecide(card.id, "allow_always")}>
            Always allow this
          </button>
        )}
      </div>
    </div>
  );
}

function QuestionCard(props: {
  card: Extract<Card, { kind: "question" }>;
  onAnswer: (id: string, answers: Answer[]) => void;
}) {
  const { card } = props;
  const [chosen, setChosen] = useState<Record<number, string[]>>({});
  const [own, setOwn] = useState<Record<number, string>>({});
  const single = card.questions.length === 1 && !card.questions[0].multi;

  const answersNow = (extra?: { index: number; label: string }): Answer[] =>
    card.questions.map((q, index) => {
      const typed = (own[index] ?? "").trim();
      const picked = extra && extra.index === index ? [extra.label] : (chosen[index] ?? []);
      return { question: q.question, chosen: typed !== "" ? [...picked, typed] : picked };
    });
  const complete = answersNow().every((a) => a.chosen.length > 0);

  const pick = (index: number, q: Question, label: string) => {
    if (single) {
      props.onAnswer(card.id, answersNow({ index, label }));
      return;
    }
    setChosen((old) => {
      const had = old[index] ?? [];
      const next = q.multi ? (had.includes(label) ? had.filter((l) => l !== label) : [...had, label]) : [label];
      return { ...old, [index]: next };
    });
  };

  return (
    <div className="ask" data-card="question">
      {card.questions.map((q, index) => (
        <div className="ask-q" key={index}>
          {q.header && <span className="eyebrow">{q.header}</span>}
          <h3>
            <Line text={q.question} />
          </h3>
          <div className="options">
            {q.options.map((option, n) => (
              <button
                key={option.label}
                className={(chosen[index] ?? []).includes(option.label) ? "option on" : "option"}
                onClick={() => pick(index, q, option.label)}
              >
                <i>{String.fromCharCode(65 + n)}</i>
                <span>
                  <Line text={option.label} />
                  {option.description && (
                    <small>
                      <Line text={option.description} />
                    </small>
                  )}
                </span>
              </button>
            ))}
          </div>
          <input
            className="field"
            placeholder="Or say it in your own words"
            value={own[index] ?? ""}
            onChange={(e) => setOwn((old) => ({ ...old, [index]: e.target.value }))}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !composing(e) && complete) props.onAnswer(card.id, answersNow());
            }}
          />
        </div>
      ))}
      {(!single || (own[0] ?? "").trim() !== "") && (
        <div className="row">
          <button className="btn primary" disabled={!complete} onClick={() => props.onAnswer(card.id, answersNow())}>
            Answer <kbd>&#8629;</kbd>
          </button>
        </div>
      )}
    </div>
  );
}

function Composer(props: { disabled: boolean; onSend: (text: string) => void; onNote: (text: string) => void }) {
  const [text, setText] = useState("");
  const [confidence, setConfidence] = useState<string | null>(null);
  const [files, setFiles] = useState<string[]>([]);
  const box = useRef<HTMLTextAreaElement>(null);
  const preview = useDeferredValue(text);

  // The box grows with what is typed, up to about eight lines.
  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 200)}px`;
  }, [text]);

  useEffect(() => {
    if (!props.disabled) box.current?.focus();
  }, [props.disabled]);

  const ready = !props.disabled && (text.trim() !== "" || files.length > 0);

  const send = () => {
    if (!ready) return;
    const parts = [text.trim()];
    if (confidence) parts.push(`Confidence: ${confidence}`);
    if (files.length > 0) parts.push(`${files.length === 1 ? "Photo" : "Photos"} of my working: ${files.join(", ")}`);
    props.onSend(parts.filter((part) => part !== "").join("\n\n"));
    setText("");
    setConfidence(null);
    setFiles([]);
  };

  const addPhoto = async () => {
    const api = await core();
    try {
      for (const path of await api.pickFiles()) {
        const saved = await api.attach(path, "working");
        setFiles((old) => [...old, saved]);
      }
    } catch (error) {
      props.onNote(String(error));
    }
  };

  /** Put dollar signs round the selection, or an empty pair at the caret. */
  const addMath = () => {
    const el = box.current;
    if (!el) return;
    const from = el.selectionStart;
    const to = el.selectionEnd;
    const next = `${text.slice(0, from)}$${text.slice(from, to)}$${text.slice(to)}`;
    setText(next);
    window.requestAnimationFrame(() => {
      el.focus();
      el.setSelectionRange(from + 1, to + 1);
    });
  };

  return (
    <div className={props.disabled ? "composer off" : "composer"}>
      {hasMath(preview) && <Said text={preview} className="preview" />}
      <div className="box">
        <textarea
          ref={box}
          rows={1}
          placeholder="Your answer"
          aria-label="Your answer"
          value={text}
          autoFocus
          spellCheck={false}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey && !composing(e)) {
              e.preventDefault();
              send();
            }
          }}
        />
        {files.length > 0 && (
          <div className="files">
            {files.map((file) => (
              <span className="file" key={file}>
                <Icon name="camera" size={13} />
                {file.split("/").pop()}
                <button aria-label="Remove" onClick={() => setFiles((old) => old.filter((f) => f !== file))}>
                  <Icon name="close" size={12} />
                </button>
              </span>
            ))}
          </div>
        )}
        <div className="tools">
          <button className="icon-btn" title="Add a photo of your working" aria-label="Add a photo of your working" onClick={addPhoto}>
            <Icon name="camera" size={17} />
          </button>
          <button className="icon-btn" title="Write mathematics between $ signs" aria-label="Insert mathematics" onClick={addMath}>
            <Icon name="sigma" size={17} />
          </button>
          <span className="grow" />
          {text.trim() !== "" && (
            <span className="sure" role="group" aria-label="How sure are you? Optional." title="Optional. The tutor repairs sure mistakes first.">
              <span className="sure-label">How sure?</span>
              {CONFIDENCE.map(([word, label]) => (
                <button key={word} className={confidence === word ? "on" : undefined} onClick={() => setConfidence(confidence === word ? null : word)}>
                  {label}
                </button>
              ))}
            </span>
          )}
          <button className="send" disabled={!ready} onClick={send} title="Send (Enter)" aria-label="Send">
            <Icon name="send" size={17} />
          </button>
        </div>
      </div>
      <div className="outs">
        <button disabled={props.disabled} onClick={() => props.onSend("I don't know")}>
          I don&apos;t know
        </button>
        <button disabled={props.disabled} onClick={() => props.onSend("I'm lost")}>
          I&apos;m lost
        </button>
      </div>
    </div>
  );
}

function Thinking(props: { since: number; status: string; quiet: boolean; onInterrupt: () => void }) {
  const now = useNow(5_000);
  const seconds = Math.max(0, Math.floor((now - props.since) / 1000));
  // A plan can take half an hour to build. One quiet line says the window is
  // alive, and it changes once a minute, not once a second.
  const label = seconds < 60 ? (props.status === "working" && seconds >= 8 ? "Working" : "") : `Working · ${Math.floor(seconds / 60)} min`;
  return (
    <div className="thinking" data-role="thinking">
      {!props.quiet && (
        <span className="pulse" aria-label="The tutor is thinking">
          <i />
          <i />
          <i />
        </span>
      )}
      <span className="thinking-label">{label}</span>
      <button className="btn ghost small" onClick={props.onInterrupt} title="Stop this reply (⌘.)">
        <Icon name="stop" size={13} fill /> Stop
      </button>
    </div>
  );
}

export function Session(props: {
  state: SessionState;
  rail: RailData | null;
  blockMinutes: number;
  onSend: (text: string) => void;
  onInterrupt: () => void;
  onDecide: (id: string, decision: Decision) => void;
  onAnswer: (id: string, answers: Answer[]) => void;
  onStop: (line: string) => void;
  onNote: (text: string) => void;
  onDismiss: (text: string) => void;
  onPalette: () => void;
}) {
  const { state } = props;
  const live = state.live;
  const isBusy = busy(state);
  const [showEarlier, setShowEarlier] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [line, setLine] = useState("");
  // The block's end is said once. "Keep going" means it.
  const [nudged, setNudged] = useState(false);
  const liveRef = useRef<HTMLDivElement>(null);
  const stopInput = useRef<HTMLInputElement>(null);
  const now = useNow(20_000);

  // A new exchange starts at the top of the view and stays there. The reply
  // grows downward and the page does not chase it, so reading is not jerked.
  useEffect(() => {
    liveRef.current?.scrollIntoView({ block: "start" });
  }, [live?.since]);

  // Opened, the earlier turns go above the exchange in hand, as in any
  // conversation. The view stays on that exchange, with the end of the turn
  // before it showing, so it is plain that there is more to scroll up to.
  useLayoutEffect(() => {
    if (!showEarlier) return;
    const el = liveRef.current;
    el?.scrollIntoView({ block: "start" });
    el?.closest(".scroll")?.scrollBy(0, -72);
  }, [showEarlier]);

  const beginStop = () => {
    setLine(props.rail?.next_action ?? "");
    setStopping(true);
    window.setTimeout(() => stopInput.current?.focus(), 0);
  };

  const text = useDeferredValue(live ? tutorText(live) : "");
  const earlier = state.earlier.length;
  const blockOver = state.startedMs !== null && now - state.startedMs >= props.blockMinutes * 60_000;

  return (
    <section className="screen session" data-screen="session">
      <TopBar
        left={
          <button className="crumb" onClick={beginStop} disabled={stopping} title="End this sitting" data-role="end">
            <Icon name="close" size={15} />
            End
          </button>
        }
        center={<Track rail={props.rail} />}
        right={
          <>
            <Clock startedMs={state.startedMs} blockMinutes={props.blockMinutes} />
            <PaletteKey onClick={props.onPalette} />
          </>
        }
      />
      <div className="scroll stage">
        <div className="column">
          {earlier > 0 && (
            <button className="earlier-toggle" onClick={() => setShowEarlier(!showEarlier)} data-role="earlier">
              <Icon name="history" size={14} />
              {showEarlier ? "Hide" : "Show"} {earlier} earlier {earlier === 1 ? "turn" : "turns"}
            </button>
          )}
          {showEarlier && (
            <div className="earlier">
              {state.earlier.map((turn, index) => (
                <div className="exchange" key={index}>
                  {turn.learner && <Turn text={turn.learner} />}
                  <div className="tutor">
                    <Rich text={turn.tutor} final onNote={props.onNote} />
                  </div>
                </div>
              ))}
            </div>
          )}
          <div className={showEarlier ? "exchange now anchored" : "exchange now"} ref={liveRef}>
            {live?.learner && <Turn text={live.learner} />}
            {text !== "" && (
              <div className={isBusy ? "tutor writing" : "tutor"} data-role="tutor">
                <Rich text={text} final={!isBusy} onNote={props.onNote} />
              </div>
            )}
            {!live && <p className="hint">Say what you want to work on.</p>}
            {live && isBusy && state.cards.length === 0 && (
              <Thinking since={live.since} status={live.status} quiet={text !== "" && live.status !== "working"} onInterrupt={props.onInterrupt} />
            )}
            {live?.interrupted && !isBusy && <div className="stopped">Stopped by you</div>}
            {live?.error && !isBusy && <div className="banner bad selectable">{live.error}</div>}
          </div>
          {state.cards.map((card) =>
            card.kind === "permission" ? (
              <PermissionCard key={card.id} card={card} onDecide={props.onDecide} />
            ) : (
              <QuestionCard key={card.id} card={card} onAnswer={props.onAnswer} />
            ),
          )}
          {state.notices.map((notice) => (
            <div className="banner warn selectable" key={notice.text}>
              <span>{notice.text}</span>
              <button className="icon-btn" aria-label="Dismiss" onClick={() => props.onDismiss(notice.text)}>
                <Icon name="close" size={14} />
              </button>
            </div>
          ))}
        </div>
      </div>
      <div className="dock">
        <div className="column">
          {stopping ? (
            <div className="sheet" data-role="stop-sheet">
              <span className="eyebrow">Where did you stop?</span>
              <input
                ref={stopInput}
                className="field"
                value={line}
                placeholder="One line: where you are, and what comes next"
                onChange={(e) => setLine(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && !composing(e)) props.onStop(line);
                  if (e.key === "Escape") {
                    e.stopPropagation();
                    setStopping(false);
                  }
                }}
              />
              <div className="row">
                <button className="btn primary" onClick={() => props.onStop(line)}>
                  Save and end <kbd>&#8629;</kbd>
                </button>
                <button className="btn ghost" onClick={() => setStopping(false)}>
                  Keep going
                </button>
                <span className="aside">Your next sitting opens on this line.</span>
              </div>
            </div>
          ) : (
            <>
              {blockOver && !nudged && !isBusy && (
                <div className="banner nudge">
                  <span>
                    <b>{props.blockMinutes} minutes done.</b> A good place to stop?
                  </span>
                  <span className="grow" />
                  <button className="btn small" onClick={beginStop}>
                    End here
                  </button>
                  <button className="btn ghost small" onClick={() => setNudged(true)}>
                    Keep going
                  </button>
                </div>
              )}
              {state.cards.length === 0 && <Composer disabled={isBusy} onSend={props.onSend} onNote={props.onNote} />}
            </>
          )}
        </div>
      </div>
    </section>
  );
}
