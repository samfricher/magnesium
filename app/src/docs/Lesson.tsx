// A lesson is a page the learner and the tutor write together. The tutor's
// words run down the page; a task is a block with a place for the answer
// inside it, and what is written there is the answer. Anything written on
// the page itself, outside the task, is talk, and is never marked. The
// record's marks appear in the gutter beside the answer they belong to.

import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";

import { HELP, splitConfidence, type Card, type Notice, type SessionState } from "../lesson";
import type { Page, Exchange } from "../page";
import { dayName } from "../page";
import { renderInline, renderPlain } from "../render";
import type { SourceRef } from "../sources";
import { taskLine } from "../task";
import type { Answer, Decision, Mark } from "../types";
import { Rich } from "../ui/Rich";
import { Writer, type WriterHandle } from "./Writer";

export type Place = "answer" | "talk";
export type Confidence = "sure" | "unsure" | "guess";
export type HelpKind = keyof typeof HELP;

export type LessonProps = {
  page: Page;
  session: SessionState;
  title: string;
  /** The conversation is open: the tutor can be written to. */
  running: boolean;
  /** An earlier lesson, to read only. */
  readOnly?: boolean;
  /** The lesson has never started: a new lesson's first line. */
  fresh: boolean;
  /** For a new lesson: what the plan has next, the first one Return starts. */
  suggestions: string[];
  place: Place;
  setPlace: (place: Place) => void;
  /** Bumped when the writing place should take the keyboard. */
  focusTick: number;
  drafts: Record<string, string>;
  onDraft: (place: string, text: string) => void;
  onAnswer: (text: string, confidence: Confidence | null, photos: string[]) => Promise<boolean>;
  onTalk: (text: string) => Promise<boolean>;
  onHelp: (kind: HelpKind) => void;
  onContinue: () => void;
  onNewLesson: (goal: string) => void;
  onSource: (source: SourceRef) => void;
  onDecide: (id: string, decision: Decision) => void;
  onCardAnswer: (id: string, answers: Answer[], shown: string) => void;
  onDismiss: (text: string) => void;
  onPhotos: () => Promise<string[]>;
  onNote: (text: string) => void;
  registerWriter: (place: Place, handle: WriterHandle | null) => void;
  /** The scrolling element of the pane, for the pinned task. */
  scroller: () => HTMLElement | null;
};

const MARK_WORD: Record<string, string> = { right: "Right", partly: "Partly right", wrong: "Not right yet", declined: "Declined" };
const HELP_WORD: Record<string, string> = { hint: "with a hint", step: "with a step shown", solution: "after the solution" };

/** When something happened, as a day a person would say: "today at 16:40", "on Tuesday at 16:40", "on Thursday 5 November at 13:12". Never a count of days. */
export function whenWords(ms: number, now = Date.now()): string {
  const time = new Date(ms).toTimeString().slice(0, 5);
  const start = (t: number) => {
    const d = new Date(t);
    d.setHours(0, 0, 0, 0);
    return d.getTime();
  };
  const days = Math.round((start(now) - start(ms)) / 86_400_000);
  if (days <= 0) return `today at ${time}`;
  if (days === 1) return `yesterday at ${time}`;
  if (days < 7) return `on ${dayName(ms).split(" ")[0]} at ${time}`;
  return `on ${dayName(ms)} at ${time}`;
}

/** What the gutter says of one row of the record: four words or fewer. */
export function markWords(mark: Mark): string {
  const word = MARK_WORD[mark.mark] ?? mark.mark;
  if (mark.help && mark.help !== "none") return `${word}, ${HELP_WORD[mark.help] ?? "with help"}`;
  if (mark.mark === "right" && mark.cold) return "Right, from a blank page";
  return word;
}

function MarkGlyph({ mark }: { mark: string }) {
  const path =
    mark === "right"
      ? "M3.5 8.5l3 3 6-7"
      : mark === "wrong"
        ? "M4.5 4.5l7 7M11.5 4.5l-7 7"
        : mark === "partly"
          ? "M3.5 9.5c1.5-2 3-2 4.5 0s3 2 4.5 0"
          : "M4 8h8";
  return (
    <svg className="glyph" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
      <path d={path} fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

function Marks({ marks }: { marks: Mark[] }) {
  if (marks.length === 0) return null;
  return (
    <div className="marks" data-role="marks">
      {marks.map((mark) => (
        <div className="mark" key={mark.id} data-mark={mark.mark} title={`From the record: ${mark.label}, ${mark.at}`}>
          <MarkGlyph mark={mark.mark} />
          <span>{markWords(mark)}</span>
        </div>
      ))}
    </div>
  );
}

/** The learner's own words as they wrote them, with the confidence and photo lines read as what they are. */
function Own({ text }: { text: string }) {
  const { text: words, confidence } = splitConfidence(text);
  const photo = /(?:^|\n\n)Photos? of my working: ([^\n]+)$/.exec(words);
  const body = photo ? words.slice(0, photo.index).trim() : words;
  const count = photo ? photo[1].split(",").length : 0;
  return (
    <>
      {body !== "" && <div className="own-text" dangerouslySetInnerHTML={{ __html: renderPlain(body) }} />}
      {(confidence || count > 0) && (
        <div className="own-note">
          {[confidence ? `${confidence}` : null, count > 0 ? (count === 1 ? "with a photo of my working" : `with ${count} photos of my working`) : null]
            .filter(Boolean)
            .join(", ")}
        </div>
      )}
    </>
  );
}

const HELP_SAID: Record<string, string> = {
  [HELP.hint]: "You asked for a hint.",
  [HELP.lost]: "You said you were lost.",
  [HELP.unknown]: "You said you don't know.",
};

type AnswerSpaceProps = Pick<LessonProps, "drafts" | "onDraft" | "onAnswer" | "onHelp" | "setPlace" | "onPhotos" | "registerWriter" | "onNote"> & {
  composer: boolean;
  busy: boolean;
};

/** The place inside the open task where the answer is written. */
const AnswerSpace = memo(function AnswerSpace(props: AnswerSpaceProps) {
  const [confidence, setConfidence] = useState<Confidence | null>(null);
  const [photos, setPhotos] = useState<string[]>([]);
  const writer = useRef<WriterHandle | null>(null);
  const latest = useRef(props);
  latest.current = props;
  const { registerWriter } = props;
  const bind = useCallback(
    (handle: WriterHandle | null) => {
      writer.current = handle;
      registerWriter("answer", handle);
    },
    [registerWriter],
  );

  const addPhotos = useCallback(async () => {
    const added = await latest.current.onPhotos();
    if (added.length > 0) setPhotos((old) => [...old, ...added]);
  }, []);

  useEffect(() => {
    const onPhotoKey = () => void addPhotos();
    window.addEventListener("mg:photos", onPhotoKey);
    return () => window.removeEventListener("mg:photos", onPhotoKey);
  }, [addPhotos]);

  const onKey = (event: React.KeyboardEvent<HTMLTextAreaElement>, text: string): boolean => {
    const command = event.metaKey || event.ctrlKey;
    const key = event.key.toLowerCase();
    if (event.key === "Escape") {
      event.preventDefault();
      latest.current.setPlace("talk");
      return true;
    }
    if (event.altKey && !command && ["1", "2", "3"].includes(event.code.replace("Digit", ""))) {
      event.preventDefault();
      const pick = (["sure", "unsure", "guess"] as Confidence[])[Number(event.code.replace("Digit", "")) - 1];
      setConfidence((old) => (old === pick ? null : pick));
      return true;
    }
    if (command && event.shiftKey && (key === "h" || key === "l" || key === "d")) {
      event.preventDefault();
      if (!latest.current.busy) latest.current.onHelp(key === "h" ? "hint" : key === "l" ? "lost" : "unknown");
      return true;
    }
    if (command && event.shiftKey && key === "a") {
      event.preventDefault();
      void addPhotos();
      return true;
    }
    if (event.key === "Enter" && !event.shiftKey && text.trim() === "" && photos.length === 0) {
      event.preventDefault();
      return true;
    }
    return false;
  };

  return (
    <div className="answer-space" data-role="answer-space">
      <Writer
        ref={bind}
        drive="answer"
        composer={props.composer}
        label="Your answer to the task"
        placeholder="Write your answer"
        initial={props.drafts.answer ?? ""}
        preview
        onDraft={(text) => props.onDraft("answer", text)}
        onFocus={() => props.setPlace("answer")}
        onKey={onKey}
        onSubmit={async (text) => {
          if (latest.current.busy) {
            latest.current.onNote("The tutor is still writing. Your answer is kept here; send it when the reply ends, or stop the reply with ⌘.");
            return false;
          }
          const taken = await latest.current.onAnswer(text.trim(), confidence, photos);
          if (taken) {
            setConfidence(null);
            setPhotos([]);
          }
          return taken;
        }}
      />
      {photos.length > 0 && (
        <div className="photos">
          {photos.map((path) => (
            <span key={path} className="photo-name">
              {path.split("/").pop()}
            </span>
          ))}
        </div>
      )}
      <div className="task-tools">
        <button type="button" tabIndex={-1} onClick={() => props.onHelp("hint")} title="⌘⇧H">
          Hint
        </button>
        <button type="button" tabIndex={-1} onClick={() => props.onHelp("lost")} title="⌘⇧L">
          I'm lost
        </button>
        <button type="button" tabIndex={-1} onClick={() => props.onHelp("unknown")} title="⌘⇧D">
          I don't know
        </button>
        <span className="spacer" />
        {(["sure", "unsure", "guess"] as Confidence[]).map((word, n) => (
          <button
            key={word}
            type="button"
            tabIndex={-1}
            className={confidence === word ? "chosen" : undefined}
            aria-pressed={confidence === word}
            title={`⌥${n + 1}`}
            onClick={() => setConfidence((old) => (old === word ? null : word))}
          >
            {word}
          </button>
        ))}
      </div>
    </div>
  );
});

type ExchangeProps = {
  exchange: Exchange;
  index: number;
  aboutTask: string | null;
  aboutIsAbove: boolean;
  isOpen: boolean;
  answerSpace: React.ReactNode;
  keptDraft: string | null;
  linksOpen: boolean;
  onSource: (source: SourceRef) => void;
  onNote: (text: string) => void;
};

const ExchangeView = memo(function ExchangeView(props: ExchangeProps) {
  const { exchange: ex } = props;
  const plain = splitConfidence(ex.learner).text.trim();
  const live = ex.live;
  const writing = live !== null && live.status !== "idle";
  const tutorShown = ex.tutor.trim() !== "";
  const before = ex.split ? ex.split.before : ex.tutor;
  return (
    <section className="exchange" data-said={ex.said} data-index={props.index}>
      {ex.day && (
        <div className="dateline" role="separator">
          <span>{ex.day}</span>
        </div>
      )}
      {ex.said === "opening" ? null : ex.said === "help" ? (
        <div className={`in-task help-said${props.aboutIsAbove ? " follows" : ""}`}>{HELP_SAID[plain] ?? plain}</div>
      ) : ex.said === "answer" ? (
        <div className={`in-task answer${props.aboutIsAbove ? " follows" : ""}`} data-role="answer">
          {props.aboutTask && !props.aboutIsAbove && <div className="answer-to" dangerouslySetInnerHTML={{ __html: renderInline(props.aboutTask) }} />}
          <Own text={ex.learner} />
          <Marks marks={ex.marks} />
        </div>
      ) : ex.learner.trim() !== "" ? (
        <div className={`said${live?.pick ? " picked" : ""}`} data-role="said">
          <Own text={ex.learner} />
        </div>
      ) : null}
      {tutorShown && (
        <div className="tutor" data-role="tutor">
          {before.trim() !== "" && <Rich text={before} final={!writing} stopped={ex.interrupted} onSource={props.onSource} onNote={props.onNote} linksOpen={props.linksOpen} />}
          {ex.split && (
            <div className={`task-block${props.isOpen ? " open" : ""}`} data-role="task" data-open={props.isOpen || undefined}>
              <Rich text={ex.split.task} final onSource={props.onSource} onNote={props.onNote} linksOpen={false} />
              {props.isOpen && props.answerSpace}
              {props.isOpen && !props.answerSpace && props.keptDraft && <div className="kept-draft" dangerouslySetInnerHTML={{ __html: renderPlain(props.keptDraft) }} />}
            </div>
          )}
        </div>
      )}
      {ex.interrupted && <p className="quiet-line">Stopped.</p>}
      {live?.error && !live.interrupted && <p className="quiet-line problem">{live.error}</p>}
    </section>
  );
});

function Working({ doing, waiting }: { doing: string | null; waiting: boolean }) {
  return (
    <div className="working" data-role="working" aria-live="polite">
      <span className="pulse" aria-hidden="true" />
      <span>{doing ? `${doing[0].toUpperCase()}${doing.slice(1)}` : waiting ? "" : ""}</span>
    </div>
  );
}

function CardView({ card, onDecide, onCardAnswer }: { card: Card; onDecide: LessonProps["onDecide"]; onCardAnswer: LessonProps["onCardAnswer"] }) {
  const first = useRef<HTMLButtonElement>(null);
  const [chosen, setChosen] = useState<Record<number, string[]>>({});
  useEffect(() => first.current?.focus(), [card]);
  if (card.kind === "permission") {
    return (
      <div className="card" role="group" aria-label={card.title} data-role="permission">
        <p className="card-title">The tutor wants to {card.title.charAt(0).toLowerCase() + card.title.slice(1)}.</p>
        <p className="card-detail">{card.detail}</p>
        <div className="card-actions">
          <button ref={first} type="button" onClick={() => onDecide(card.id, "allow_once")}>
            Allow once
          </button>
          {card.canAlways && (
            <button type="button" onClick={() => onDecide(card.id, "allow_always")}>
              Always allow
            </button>
          )}
          <button type="button" onClick={() => onDecide(card.id, "refuse")}>
            Don't allow
          </button>
        </div>
      </div>
    );
  }
  const send = () => {
    const answers: Answer[] = card.questions.map((q, n) => ({ question: q.question, chosen: chosen[n] ?? [] }));
    onCardAnswer(card.id, answers, answers.map((a) => a.chosen.join(", ")).filter(Boolean).join("\n"));
  };
  return (
    <div className="card" role="group" data-role="question">
      {card.questions.map((q, n) => (
        <div key={q.question} className="card-question">
          <p className="card-title">{q.question}</p>
          <div className="card-options">
            {q.options.map((option, i) => {
              const on = (chosen[n] ?? []).includes(option.label);
              return (
                <button
                  key={option.label}
                  ref={n === 0 && i === 0 ? first : undefined}
                  type="button"
                  className={on ? "chosen" : undefined}
                  aria-pressed={on}
                  onClick={() => {
                    if (!q.multi && card.questions.length === 1) {
                      onCardAnswer(card.id, [{ question: q.question, chosen: [option.label] }], option.label);
                      return;
                    }
                    setChosen((old) => {
                      const list = old[n] ?? [];
                      const next = q.multi ? (on ? list.filter((l) => l !== option.label) : [...list, option.label]) : [option.label];
                      return { ...old, [n]: next };
                    });
                  }}
                >
                  {option.label}
                  {option.description && <span className="option-note">{option.description}</span>}
                </button>
              );
            })}
          </div>
        </div>
      ))}
      {(card.questions.length > 1 || card.questions.some((q) => q.multi)) && (
        <div className="card-actions">
          <button type="button" onClick={send}>
            Send
          </button>
        </div>
      )}
    </div>
  );
}

function NoticeLine({ notice, onDismiss }: { notice: Notice; onDismiss: (text: string) => void }) {
  return (
    <div className="notice" data-kind={notice.kind}>
      <span>{notice.text}</span>
      <button type="button" className="dismiss" aria-label="Dismiss" onClick={() => onDismiss(notice.text)}>
        ×
      </button>
    </div>
  );
}

export function LessonDoc(props: LessonProps) {
  const { page, session } = props;
  const live = session.live;
  const busy = live !== null && live.status !== "idle";
  const open = page.open;
  const openTask = open !== null ? page.exchanges[open]?.split?.task ?? null : null;
  const talk = useRef<WriterHandle | null>(null);
  const answerRef = useRef<WriterHandle | null>(null);
  const taskEl = useRef<HTMLElement | null>(null);
  const [pinned, setPinned] = useState(false);
  const latest = useRef(props);
  latest.current = props;
  const { registerWriter } = props;

  const showAnswer = props.running && !props.readOnly && open !== null;
  const place: Place = showAnswer ? props.place : "talk";

  const bindTalk = useCallback(
    (handle: WriterHandle | null) => {
      talk.current = handle;
      registerWriter("talk", handle);
    },
    [registerWriter],
  );
  const bindAnswer = useCallback(
    (place: Place, handle: WriterHandle | null) => {
      if (place === "answer") answerRef.current = handle;
      registerWriter(place, handle);
    },
    [registerWriter],
  );

  // The writing place takes the keyboard when asked to.
  useLayoutEffect(() => {
    if (props.readOnly) return;
    if (place === "answer") answerRef.current?.focus();
    else talk.current?.focus();
  }, [props.focusTick, place, props.readOnly, showAnswer, open]);

  // The open task pins to the top of the pane while it is scrolled away.
  useEffect(() => {
    const root = props.scroller();
    const el = root?.querySelector<HTMLElement>('[data-role="task"][data-open]') ?? null;
    taskEl.current = el;
    if (!root || !el) {
      setPinned(false);
      return;
    }
    const watch = new IntersectionObserver(
      ([entry]) => setPinned(!entry.isIntersecting && entry.boundingClientRect.top < (entry.rootBounds?.top ?? 0) + 40),
      // A task with only its last few pixels showing counts as scrolled away.
      { root, threshold: 0, rootMargin: "-72px 0px 0px 0px" },
    );
    watch.observe(el);
    return () => watch.disconnect();
  }, [open, page.exchanges.length, props.scroller]);

  const answerSpace = useMemo(
    () =>
      showAnswer ? (
        <AnswerSpace
          composer={place === "answer"}
          busy={busy}
          drafts={props.drafts}
          onDraft={props.onDraft}
          onAnswer={props.onAnswer}
          onHelp={props.onHelp}
          setPlace={props.setPlace}
          onPhotos={props.onPhotos}
          onNote={props.onNote}
          registerWriter={bindAnswer}
        />
      ) : null,
    // The answer space is the same element while its task is open.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [showAnswer, place, busy, open],
  );

  const jumpToTask = () => {
    props.setPlace("answer");
    taskEl.current?.scrollIntoView({ block: "center" });
  };

  const onTalkKey = (event: React.KeyboardEvent<HTMLTextAreaElement>, text: string): boolean => {
    if ((event.key === "Tab" && !event.shiftKey) || (event.key === "ArrowUp" && (event.metaKey || event.ctrlKey))) {
      if (!showAnswer) return false;
      event.preventDefault();
      jumpToTask();
      return true;
    }
    if (event.key === "Enter" && !event.shiftKey && text.trim() === "" && !props.running && !props.readOnly) {
      event.preventDefault();
      if (props.fresh) props.onNewLesson(props.suggestions[0] ?? "");
      else props.onContinue();
      return true;
    }
    return false;
  };

  const lastMs = page.exchanges.length > 0 ? page.exchanges[page.exchanges.length - 1].key : null;
  const stopped = !props.running && !props.fresh && lastMs !== null;
  const stoppedOn = lastMs !== null ? whenWords(lastMs) : "";

  return (
    <article className="lesson" data-doc="lesson" data-running={props.running || undefined}>
      {pinned && openTask && (
        <button type="button" className="pinned" onClick={jumpToTask} data-role="pinned">
          <span className="pinned-text" dangerouslySetInnerHTML={{ __html: renderInline(taskLine(openTask)) }} />
          <kbd>Tab</kbd>
        </button>
      )}
      <h1 className="doc-title">{props.title}</h1>
      {page.exchanges.map((ex, index) => {
        const about = ex.about !== null ? page.exchanges[ex.about] : null;
        return (
          <ExchangeView
            key={`${ex.key}-${index}`}
            exchange={ex}
            index={index}
            aboutTask={about?.split ? taskLine(about.split.task) : null}
            aboutIsAbove={ex.about !== null && ex.about === index - 1}
            isOpen={open === index && !props.readOnly}
            answerSpace={open === index ? answerSpace : null}
            keptDraft={open === index && !showAnswer ? (props.drafts.answer ?? "").trim() || null : null}
            linksOpen={open !== index}
            onSource={props.onSource}
            onNote={props.onNote}
          />
        );
      })}
      {busy && (live.status === "waiting" || live.status === "working" || live.streaming === "") && <Working doing={live.doing} waiting={live.status === "waiting"} />}
      {session.cards.map((card) => (
        <CardView key={card.id} card={card} onDecide={props.onDecide} onCardAnswer={props.onCardAnswer} />
      ))}
      {session.notices.map((notice) => (
        <NoticeLine key={notice.text} notice={notice} onDismiss={props.onDismiss} />
      ))}
      {props.fresh && !props.readOnly ? (
        <div className="new-lesson" data-role="new-lesson">
          <p className="ask">What do you want to work on?</p>
          <Writer
            ref={bindTalk}
            drive="talk"
            composer
            label="What to work on"
            placeholder={props.suggestions[0]?.replace(/[$\\{}_^]/g, "") ?? "A topic, a chapter, a question from your course"}
            initial={props.drafts.talk ?? ""}
            onDraft={(text) => props.onDraft("talk", text)}
            onKey={onTalkKey}
            onSubmit={(text) => {
              props.onNewLesson(text.trim() || props.suggestions[0] || "");
              return true;
            }}
          />
          {props.suggestions.length > 0 && (
            <ul className="suggestions">
              {props.suggestions.slice(0, 4).map((item, n) => (
                <li key={item}>
                  <button type="button" data-drive={n === 0 ? "start" : undefined} onClick={() => props.onNewLesson(item)}>
                    <span dangerouslySetInnerHTML={{ __html: renderInline(item) }} />
                    {n === 0 && <kbd>Return</kbd>}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      ) : !props.readOnly ? (
        <div className={`page-end${place === "talk" ? " here" : ""}`} data-role="page-end">
          {stopped && (
            <p className="stopped-line">
              Stopped {stoppedOn}.
              <button type="button" className="continue" data-drive="start" onClick={props.onContinue}>
                Continue <kbd>Return</kbd>
              </button>
            </p>
          )}
          <Writer
            ref={bindTalk}
            drive="talk"
            composer={place === "talk"}
            label="Write to your tutor"
            placeholder={stopped ? "Or write to your tutor" : showAnswer ? (place === "answer" ? "Esc to say something else" : "Say anything. Tab goes back to the task.") : "Write to your tutor"}
            initial={props.drafts.talk ?? ""}
            onDraft={(text) => props.onDraft("talk", text)}
            onKey={onTalkKey}
            onFocus={() => props.setPlace("talk")}
            onSubmit={async (text) => {
              const words = text.trim();
              if (words === "") return false;
              if (busy) {
                props.onNote("The tutor is still writing. What you wrote is kept; send it when the reply ends, or stop the reply with ⌘.");
                return false;
              }
              return props.onTalk(words);
            }}
          />
        </div>
      ) : null}
    </article>
  );
}
