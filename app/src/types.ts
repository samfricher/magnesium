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
export type Proofs = { earned: number; total: number; proven: number; skills: number; level: number; to_next: number };

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

export type Opened = { resumed: boolean; turns: Turn[] };

export type Step = { step: string; status: "todo" | "doing" | "done" | string };

export type Rail = { goal: string | null; steps: Step[]; next_action: string | null };

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
  message: string | null;
};

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
  | { type: "working" }
  | { type: "permission"; id: string; title: string; detail: string; can_always: boolean }
  | { type: "question"; id: string; questions: Question[] }
  | { type: "withdrawn"; id: string }
  | { type: "turn_ended"; ok: boolean; interrupted: boolean; error: string | null }
  | { type: "notice"; kind: NoticeKind; text: string }
  | { type: "exited"; expected: boolean; detail: string | null };

export type Words = { shown: string; sent: string };

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

export type RuntimeState = "ready" | "missing" | "different" | "kept" | "failed";

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
  signIn(): Promise<void>;
  about(): Promise<About>;
  openSession(resume: boolean): Promise<Opened>;
  send(text: string): Promise<void>;
  sendNewLesson(goal: string): Promise<void>;
  sendShown(shown: string, sent: string): Promise<void>;
  interrupt(): Promise<void>;
  decide(id: string, decision: Decision): Promise<void>;
  answer(id: string, answers: Answer[]): Promise<void>;
  stop(stoppedAt: string | null): Promise<void>;
  rail(): Promise<Rail>;
  progress(target: string | null): Promise<Progress>;
  setBlockMinutes(minutes: number): Promise<void>;
  attach(path: string, group: string): Promise<string>;
  openFile(path: string): Promise<void>;
  openUrl(url: string): Promise<void>;
  image(path: string): Promise<string>;
  paperStart(paper: Paper, minutes: number): Promise<PaperSitting>;
  paperEnd(files: string[]): Promise<Words | null>;
  onEvent(handler: (event: AgentEvent) => void): Promise<() => void>;
  selftest(): Promise<boolean>;
  selftestReport(report: unknown): Promise<void>;
}
