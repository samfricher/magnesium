// The lesson. On screen: the goal and its steps, the tutor's current reply,
// one answer box, and the clock. Not on screen: tool calls, save messages,
// token counts, the model's name, other sessions.

import { useDeferredValue, useEffect, useLayoutEffect, useRef, useState } from "react";

import { core } from "./bridge";
import { Markdown } from "./Markdown";
import { busy, tutorText, type Card, type SessionState } from "./lesson";
import type { Answer, Decision, Question, Rail as RailData } from "./types";

const CONFIDENCE: Record<string, string> = { S: "sure", U: "unsure", G: "guess" };

/** The time now, refreshed on a slow tick. */
export function useNow(everyMs: number): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), everyMs);
    return () => window.clearInterval(timer);
  }, [everyMs]);
  return now;
}

function Rail(props: {
  rail: RailData | null;
  startedMs: number | null;
  blockMinutes: number;
  canStop: boolean;
  onStop: () => void;
}) {
  const now = useNow(20_000);
  const { rail } = props;
  const used = props.startedMs === null ? 0 : Math.max(0, Math.floor((now - props.startedMs) / 60_000));
  const share = Math.min(100, Math.round((used / props.blockMinutes) * 100));
  const steps = rail?.steps ?? [];
  // The step in hand: the one the tutor marked, else the first not done.
  const current = steps.findIndex((s) => s.status === "doing");
  const firstOpen = steps.findIndex((s) => s.status !== "done");
  const cur = current >= 0 ? current : firstOpen;
  return (
    <aside className="rail">
      <div className="raildrag" data-tauri-drag-region />
      <span className="lbl">Goal</span>
      {rail?.goal ? (
        <div className="goal selectable">{rail.goal}</div>
      ) : (
        <div className="goal unset">The tutor sets the goal in its first reply.</div>
      )}
      {steps.length > 0 ? (
        <ol className="steps">
          {steps.map((step, index) => (
            <li key={index} className={step.status === "done" ? "done" : index === cur ? "cur" : undefined}>
              {step.step}
            </li>
          ))}
        </ol>
      ) : (
        rail?.next_action && (
          <ol className="steps">
            <li className="cur">{rail.next_action}</li>
          </ol>
        )
      )}
      <div className={used > props.blockMinutes ? "clock over" : "clock"}>
        <div className="bar">
          <i style={{ width: `${share}%` }} />
        </div>
        <div className="t">
          <span className="mins">
            {used} / {props.blockMinutes} min
          </span>
          <button className="btn ghost sm" onClick={props.onStop} disabled={!props.canStop}>
            Stop
          </button>
        </div>
      </div>
    </aside>
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
    <div className="ask">
      <span className="lbl">The tutor asks to</span>
      <div className="tq">{card.title}</div>
      {card.detail && <code className="detail selectable">{card.detail}</code>}
      <div className="row">
        <button className="btn pri" onClick={() => props.onDecide(card.id, "allow_once")}>
          Allow once
        </button>
        {card.canAlways && (
          <button className="btn" onClick={() => props.onDecide(card.id, "allow_always")}>
            Always allow
          </button>
        )}
        <button className="btn ghost" onClick={() => props.onDecide(card.id, "refuse")}>
          Refuse
        </button>
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
    <div className="ask">
      {card.questions.map((q, index) => (
        <div className="qq" key={index}>
          <span className="lbl">{q.header ?? "The tutor asks"}</span>
          <div className="tq">{q.question}</div>
          <span className="opts">
            {q.options.map((option, n) => (
              <button
                key={option.label}
                className={(chosen[index] ?? []).includes(option.label) ? "btn sm on" : "btn sm"}
                onClick={() => pick(index, q, option.label)}
              >
                {String.fromCharCode(97 + n)} · {option.label}
                {option.description && <small>{option.description}</small>}
              </button>
            ))}
          </span>
          <input
            className="in"
            style={{ marginTop: 8 }}
            placeholder="or say it in your own words"
            value={own[index] ?? ""}
            onChange={(e) => setOwn((old) => ({ ...old, [index]: e.target.value }))}
            onKeyDown={(e) => {
              if (e.key === "Enter" && complete) props.onAnswer(card.id, answersNow());
            }}
          />
        </div>
      ))}
      {(!single || (own[0] ?? "").trim() !== "") && (
        <div className="row">
          <button className="btn pri" disabled={!complete} onClick={() => props.onAnswer(card.id, answersNow())}>
            Answer &#8629;
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

  // The box grows with what is typed, up to about eight lines.
  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight + 2, 190)}px`;
  }, [text]);

  useEffect(() => {
    if (!props.disabled) box.current?.focus();
  }, [props.disabled]);

  const ready = !props.disabled && (text.trim() !== "" || files.length > 0);

  const send = () => {
    if (!ready) return;
    const parts = [text.trim()];
    if (confidence) parts.push(`Confidence: ${CONFIDENCE[confidence]}`);
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

  return (
    <div className="task">
      <textarea
        ref={box}
        className="in"
        rows={1}
        placeholder="your answer"
        value={text}
        autoFocus
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
            e.preventDefault();
            send();
          }
        }}
      />
      <div className="row">
        <div className="sug" title="How sure are you? Optional.">
          {Object.entries(CONFIDENCE).map(([letter, word]) => (
            <button
              key={letter}
              className={confidence === letter ? "on" : undefined}
              onClick={() => setConfidence(confidence === letter ? null : letter)}
            >
              {letter}
              <i>{word}</i>
            </button>
          ))}
        </div>
        <button className="btn ghost sm" onClick={addPhoto}>
          + photo of your working
        </button>
        {files.map((file) => (
          <span className="chip-file" key={file}>
            {file.split("/").pop()}
            <button aria-label="Remove" onClick={() => setFiles((old) => old.filter((f) => f !== file))}>
              &times;
            </button>
          </span>
        ))}
      </div>
      <div className="row">
        <button className="btn pri" disabled={!ready} onClick={send}>
          Answer &#8629;
        </button>
        <button className="btn ghost" disabled={props.disabled} onClick={() => props.onSend("I don't know")}>
          I don't know
        </button>
        <button className="btn ghost" disabled={props.disabled} onClick={() => props.onSend("I'm lost")}>
          I'm lost
        </button>
      </div>
    </div>
  );
}

function WorkingLine(props: { since: number; status: string; onInterrupt: () => void }) {
  const now = useNow(5_000);
  const seconds = Math.max(0, Math.floor((now - props.since) / 1000));
  // A plan can take half an hour to build. One quiet line says the window is
  // alive, and it changes once a minute, not once a second.
  const label =
    props.status === "writing" && seconds < 60 ? "" : seconds < 60 ? "working" : `working · ${Math.floor(seconds / 60)} min`;
  return (
    <div className="working">
      <span>{label}</span>
      <button className="btn ghost sm" onClick={props.onInterrupt} title="Stop this reply">
        Stop reply
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
  const liveRef = useRef<HTMLDivElement>(null);
  const stopInput = useRef<HTMLInputElement>(null);

  // A new exchange starts at the top of the view and stays there. The reply
  // grows downward and the page does not chase it, so reading is not jerked.
  useEffect(() => {
    liveRef.current?.scrollIntoView({ block: "start" });
  }, [live?.since]);

  const beginStop = () => {
    setLine(props.rail?.next_action ?? "");
    setStopping(true);
    window.setTimeout(() => stopInput.current?.focus(), 0);
  };

  const text = useDeferredValue(live ? tutorText(live) : "");
  const earlier = state.earlier.length;

  return (
    <div className="session">
      <Rail
        rail={props.rail}
        startedMs={state.startedMs}
        blockMinutes={props.blockMinutes}
        canStop={!stopping}
        onStop={beginStop}
      />
      <div className="main">
        <div className="top" data-tauri-drag-region>
          <span className="grow" data-tauri-drag-region />
          <button className="kbd" title="Everything else lives here" onClick={props.onPalette}>
            &#8984;K
          </button>
        </div>
        <div className="scroll">
          <div className="column">
            {earlier > 0 && (
              <button className="fold" onClick={() => setShowEarlier(!showEarlier)}>
                {showEarlier ? "▾" : "▸"} {earlier} earlier {earlier === 1 ? "turn" : "turns"}
              </button>
            )}
            {showEarlier && (
              <div className="earlier">
                {state.earlier.map((turn, index) => (
                  <div className="turn" key={index}>
                    {turn.learner && (
                      <p className="said selectable">
                        <b>You</b>
                        {turn.learner}
                      </p>
                    )}
                    <div className="tutor">
                      <Markdown text={turn.tutor} final onNote={props.onNote} />
                    </div>
                  </div>
                ))}
              </div>
            )}
            <div ref={liveRef} style={{ scrollMarginTop: 4 }}>
              {live?.learner && (
                <p className="said selectable">
                  <b>You</b>
                  {live.learner}
                </p>
              )}
              {text !== "" && (
                <div className="tutor">
                  <Markdown text={text} final={!isBusy} onNote={props.onNote} />
                </div>
              )}
              {!live && <p className="said">Say what you want to work on.</p>}
              {live && isBusy && state.cards.length === 0 && (
                <WorkingLine since={live.since} status={live.status} onInterrupt={props.onInterrupt} />
              )}
              {live?.interrupted && !isBusy && <div className="working">stopped by you</div>}
              {live?.error && !isBusy && <div className="note warn selectable">{live.error}</div>}
            </div>
            {state.cards.map((card) =>
              card.kind === "permission" ? (
                <PermissionCard key={card.id} card={card} onDecide={props.onDecide} />
              ) : (
                <QuestionCard key={card.id} card={card} onAnswer={props.onAnswer} />
              ),
            )}
            {state.notices.map((notice) => (
              <div className="note warn selectable" key={notice.text}>
                <span>{notice.text}</span>
                <button className="x" aria-label="Dismiss" onClick={() => props.onDismiss(notice.text)}>
                  &times;
                </button>
              </div>
            ))}
          </div>
        </div>
        <div className="composer">
          <div className="column">
            {stopping && (
              <div className="stopbox">
                <span className="lbl">Stopped at</span>
                <input
                  ref={stopInput}
                  className="in"
                  value={line}
                  placeholder="one line: where you are, and what comes next"
                  onChange={(e) => setLine(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") props.onStop(line);
                    if (e.key === "Escape") setStopping(false);
                  }}
                />
                <div className="row">
                  <button className="btn pri" onClick={() => props.onStop(line)}>
                    Save &#8629;
                  </button>
                  <button className="btn ghost" onClick={() => setStopping(false)}>
                    Keep going
                  </button>
                  <span className="lock">the next sitting opens on this line</span>
                </div>
              </div>
            )}
            <Composer disabled={isBusy || stopping} onSend={props.onSend} onNote={props.onNote} />
          </div>
        </div>
      </div>
    </div>
  );
}
