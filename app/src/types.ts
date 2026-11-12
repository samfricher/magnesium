// The shapes the core sends. They mirror the Rust structs in crates/core.

export type Exam = {
  target: string;
  title: string;
  date: string | null;
  day: string | null;
  days_left: number | null;
};

export type Last = { days_ago: number | null; goal: string; stopped_at: string | null; next_action: string | null };

export type Paper = { target: string; name: string; path: string; minutes: number | null; sat: boolean };

export type PaperSitting = {
  target: string;
  name: string;
  path: string | null;
  minutes: number;
  started_ms: number;
};

export type HomeState = "ready" | "empty" | "not_set_up" | "no_runtime" | "needs_upgrade" | "unreadable";

export type Home = {
  folder: string;
  name: string;
  state: HomeState;
  message: string | null;
  exam: Exam | null;
  last: Last | null;
  /** null when the window cannot tell: the runtime keeps the checks where only it works out what is due. */
  checks_due: number | null;
  redos_due: number;
  first_redo: string | null;
  papers: Paper[];
  can_resume: boolean;
  sitting: PaperSitting | null;
  block_minutes: number;
};

/** One proof is one answer reproduced alone on a separate day. Three prove a skill. */
export type Proofs = { earned: number; total: number; proven: number; skills: number; level: number; to_next: number; levelled: boolean };

/** One study folder in the library, read without opening it. */
export type Card = {
  folder: string;
  name: string;
  state: HomeState;
  exam: Exam | null;
  goal: string | null;
  days_ago: number | null;
  proofs: Proofs;
  /** Paper redos and quick checks due today or earlier. */
  due: number;
  sitting: PaperSitting | null;
};

/**
 * The days the learner studied, and the run they make. A day is one with a
 * marked answer, in any course in the library, each as "2026-11-10".
 */
export type Activity = { today: string; days: string[]; run: number; studied_today: boolean };

export type Turn = { session: string; ms: number; learner: string; tutor: string; interrupted: boolean };

/**
 * What runs a lesson. "plugin": Claude Code runs it with Magnesium's plugin,
 * as in a terminal. "direct": Magnesium runs it and asks Claude Code only for
 * the tutor's words. Both use the learner's own Claude Code sign-in. The
 * learner sees them as "Claude Code" and "Magnesium".
 */
export type Engine = "plugin" | "direct";

/** The learner's pick, and the engine of the open folder's saved conversation, or null when there is none. */
export type EngineChoice = { chosen: Engine; conversation: Engine | null };

/** `engine` is the engine of the conversation that was opened. */
export type Opened = { resumed: boolean; turns: Turn[]; engine: Engine };

export type Step = { step: string; status: "todo" | "doing" | "done" | string };

export type Rail = { goal: string | null; steps: Step[]; next_action: string | null };

/**
 * How the window sends a message. "talk": conversation (a question, a change
 * to the plan, something off the plan) that is never marked, and the open
 * task stays open. "answer": an answer to the open task, marked as any answer
 * is. Omitted: the tutor decides, as before.
 */
export type Intent = "talk" | "answer";

/** One thing on the plan's list of what to study, as the plan file holds it. */
export type PlanItem = {
  /** Stable while the item's title is: the plan's own number when it has one. */
  id: string;
  title: string;
  status: "todo" | "doing" | "done";
  /** The day the plan puts it on, "2026-11-14", when it names one. */
  day: string | null;
  /** The plan's own words about it: its purpose or length. */
  note: string | null;
};

/**
 * The open folder's study plan, read fresh from the runtime and the plan
 * files. Every field is empty when the folder has none: no plan is never
 * "behind".
 */
export type Plan = {
  /** The plan's key, e.g. "midterm-2". */
  target: string | null;
  /** e.g. "Midterm 2". */
  title: string | null;
  /** "2026-11-20", or null when undated. */
  exam_date: string | null;
  /** The goal being worked on. */
  goal: string | null;
  /** That goal's route. */
  steps: Step[];
  /** The plan's ordered list of what to study, as the plan holds it. */
  items: PlanItem[];
  /** The plan file's Markdown, for a concept that shows it as a document. */
  text: string | null;
  /** The plan file, relative to the study folder: "Study/midterm-2/plan.md". */
  path: string | null;
  /** When any of the above last changed, ms since the epoch. */
  changed_ms: number | null;
};

/** A part of the plan that a turn changed. */
export type PlanPart = "exam_date" | "items" | "goal" | "steps" | "text";

/** 2 is a pass that counts, 1 a pass made before a later failure, 0 a pass still to make. */
export type Dots = [number, number, number];

export type Row = {
  dots: Dots;
  name: string;
  what: string;
  note: string;
  /** Whether the next proof can be earned today. */
  due: boolean;
  /** The day the next proof can be earned, as "2026-11-12". */
  next_day: string | null;
};

export type Score = {
  paper: string;
  date: string;
  score: string;
  sure_right: number | null;
  sure_total: number | null;
};

export type Progress = {
  targets: Exam[];
  exam: Exam | null;
  confirmed: Row[];
  in_progress: Row[];
  untested: Row[];
  scores: Score[];
  helped: string[];
  helped_count: number;
  unreadable: number;
  /** The quick checks are left out, because the window cannot tell their passes. */
  checks_unknown: boolean;
  proofs: Proofs;
  /** The runtime could not say how far the course is proved, so no level is shown. */
  level_unknown: boolean;
  /** What the learner did today in this folder. null when the runtime cannot say. */
  today: Today | null;
  message: string | null;
};

/**
 * Today's counts in one study folder, from the runtime's record: answers the
 * tutor marked, skills answered for the first time, proofs earned, and quick
 * checks and redos that come due tomorrow. `day` is "2026-11-11".
 */
export type Today = { day: string; answered: number; started: number; proofs: number; due_tomorrow: number };

export type Choice = { label: string; description: string | null };

export type Question = { question: string; header: string | null; options: Choice[]; multi: boolean };

export type Decision = "allow_once" | "allow_always" | "refuse";

export type Answer = { question: string; chosen: string[] };

export type NoticeKind = "usage_limit" | "sign_in" | "retry" | "other";

export type AgentEvent =
  | { type: "ready"; session_id: string; problems: string[]; plugin_missing: boolean }
  | { type: "turn_started" }
  | { type: "text_delta"; text: string }
  | { type: "text_done"; text: string }
  | { type: "working"; doing: string | null }
  | { type: "permission"; id: string; title: string; detail: string; can_always: boolean }
  | { type: "question"; id: string; questions: Question[] }
  | { type: "withdrawn"; id: string }
  | { type: "turn_ended"; ok: boolean; interrupted: boolean; error: string | null }
  | { type: "notice"; kind: NoticeKind; text: string }
  | { type: "exited"; expected: boolean; detail: string | null }
  /** Sent after a turn that changed the plan, the goal, its steps or the exam date. Read `plan()` for how it stands. */
  | { type: "plan_changed"; what: PlanPart[] }
  /** The record has a new mark for the last message, so no screen polls `marks`. */
  | { type: "marked"; mark: Mark };

export type Words = { shown: string; sent: string };

/**
 * What adding course material did. `lines` is what the window says, one
 * plain line each: what was added, what search cannot read, and any file that
 * could not be added. A file that looks like an answer key is counted and
 * never named.
 */
export type Added = { saved: string[]; keys: number; lines: string[] };

/**
 * One answer the runtime has marked, as its record has it. The lesson says a
 * mark only from a row that was not there before the answer was sent.
 * `mark` is the first try: "right", "partly", "wrong" or "declined". `help` is
 * "none", "hint", "step" or "solution". `at` is local time, "2026-11-12 09:41".
 */
export type Mark = {
  id: string;
  at: string;
  label: string;
  mark: "right" | "partly" | "wrong" | "declined" | string;
  help: "none" | "hint" | "step" | "solution" | string;
  cold: boolean;
  /** The paper it was given on, or null in a lesson. */
  paper: string | null;
};

/**
 * What a course's material folder holds. `names` are the most recent files,
 * newest first, as the folder names them ("Week 4/slides.pdf" for a file in a
 * folder). `files` counts every one that is not an answer key; `keys` counts
 * the answer keys, which are never named.
 */
export type Materials = { names: string[]; files: number; keys: number };

/** A source a reply cited, ready for the panel beside the lesson. */
export type Source = {
  name: string;
  kind: "pdf" | "text" | "image";
  text: string | null;
  truncated: boolean;
  image: string | null;
};

/** How lessons are paid for: the learner's Claude plan, or an API key paid per use. */
export type Payment = "plan" | "api_key";

/** The agent on this Mac, as it reports itself. The window never reads a credential. */
export type Account = {
  name: string;
  installed: boolean;
  signed_in: boolean;
  paid_with: Payment | null;
  detail: string | null;
  problem: string | null;
  install_page: string | null;
};

/** A magnesium program, and the version it reports: "0.3.0+<commit>" from the download's build. */
export type Program = { shown_path: string; version: string };

/** "too_old": the installed program is older than this window needs. It cannot be kept, and no lesson starts. */
export type RuntimeState = "ready" | "missing" | "different" | "kept" | "too_old" | "failed";

export type RuntimeSetup = {
  state: RuntimeState;
  installed: Program | null;
  /** The copy inside the app. null for the window run from source. */
  bundled: Program | null;
  installed_now: boolean;
  message: string | null;
};

export type Setup = { runtime: RuntimeSetup; account: Account; payment: Payment };

/** The window's version, and the commit the download was built from. */
export type About = { version: string; commit: string | null };

/** One conversation the window kept, as a lesson the learner can open again. */
export type LessonInfo = {
  session: string;
  first_ms: number;
  last_ms: number;
  turns: number;
  /** The first thing the learner said, on one line. */
  opening: string;
  /** The folder's conversation: the one Continue resumes. */
  current: boolean;
};

/** One file under `Study/`, from the study root. */
export type StudyFile = { path: string; changed_ms: number };

/** What `Study/` holds, newest first. Answer keys are counted, never named. */
export type StudyFiles = { files: StudyFile[]; keys: number; more: boolean };

/** Everything the page may ask of the core. The window and the demo both implement it. */
export interface Core {
  initialFolder(): Promise<string | null>;
  recent(): Promise<string[]>;
  library(): Promise<Card[]>;
  forgetFolder(path: string): Promise<void>;
  /** Look for study folders set up in a terminal. Resolves to how many were new. */
  findFolders(): Promise<number>;
  activity(): Promise<Activity>;
  pickFolder(): Promise<string | null>;
  pickFiles(): Promise<string[]>;
  /** Files of any kind to add as course material, from a file dialog. */
  pickMaterial(): Promise<string[]>;
  /** A path for a new study folder, from a save dialog, or null. */
  pickNewFolder(): Promise<string | null>;
  openFolder(path: string): Promise<Home>;
  newFolder(path: string): Promise<Home>;
  home(): Promise<Home>;
  setUp(): Promise<Home>;
  upgradeFolder(): Promise<Home>;
  setup(): Promise<Setup>;
  replaceRuntime(): Promise<RuntimeSetup>;
  keepRuntime(): Promise<RuntimeSetup>;
  account(): Promise<Account>;
  setPayment(payment: Payment): Promise<Account>;
  /** Which engine runs lessons, and which one the open folder's conversation began on. */
  engine(): Promise<EngineChoice>;
  /** Pick the engine for the next conversation. It is saved at once. */
  setEngine(engine: Engine): Promise<EngineChoice>;
  signIn(): Promise<void>;
  about(): Promise<About>;
  openSession(resume: boolean): Promise<Opened>;
  /** Send the learner's words. `intent` says what they are; omitted, the tutor decides. */
  send(text: string, intent?: Intent): Promise<void>;
  sendNewLesson(goal: string): Promise<void>;
  sendShown(shown: string, sent: string): Promise<void>;
  interrupt(): Promise<void>;
  decide(id: string, decision: Decision): Promise<void>;
  answer(id: string, answers: Answer[]): Promise<void>;
  stop(stoppedAt: string | null): Promise<void>;
  rail(): Promise<Rail>;
  /** The open folder's study plan, read fresh. */
  plan(): Promise<Plan>;
  progress(target: string | null): Promise<Progress>;
  setBlockMinutes(minutes: number): Promise<void>;
  attach(path: string, group: string): Promise<string>;
  /** Copy files or folders into the study folder, and say what search can read of them. */
  addMaterial(paths: string[]): Promise<Added>;
  /** Save pasted text as a text file in the same place. */
  addPastedText(text: string): Promise<Added>;
  /** The files already in this course's material folder. */
  materialList(): Promise<Materials>;
  /** The open folder's lessons, newest first. */
  lessons(): Promise<LessonInfo[]>;
  /** The turns of one lesson, without starting the agent. */
  lessonTurns(session: string): Promise<Turn[]>;
  /** What the open folder's `Study/` holds. */
  studyFiles(): Promise<StudyFiles>;
  /** Save the learner's edit of a plan file; refused when it changed since `expectedMs`. Resolves to its new time. */
  writePlan(path: string, text: string, expectedMs: number): Promise<number>;
  /** The answers the runtime has marked, oldest first, or null when this runtime has no `attempts` command. */
  marks(): Promise<Mark[] | null>;
  /**
   * Called with the paths of files dropped on the window, and with whether
   * files are being dragged over it. Resolves to a function that stops it.
   */
  onDrop(handler: (paths: string[]) => void, hover: (over: boolean) => void): Promise<() => void>;
  /** A source a reply cited. Refused outside the study folder and for an answer key. */
  source(path: string): Promise<Source>;
  /** The bytes of a cited PDF. */
  sourcePdf(path: string): Promise<ArrayBuffer>;
  openFile(path: string): Promise<void>;
  openUrl(url: string): Promise<void>;
  image(path: string): Promise<string>;
  paperStart(paper: Paper, minutes: number): Promise<PaperSitting>;
  paperEnd(files: string[]): Promise<Words | null>;
  onEvent(handler: (event: AgentEvent) => void): Promise<() => void>;
  selftest(): Promise<boolean>;
  selftestReport(report: unknown): Promise<void>;
}
