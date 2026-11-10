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

export type HomeState = "ready" | "empty" | "not_set_up" | "no_runtime" | "unreadable";

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

export type Turn = { session: string; ms: number; learner: string; tutor: string; interrupted: boolean };

export type Opened = { resumed: boolean; turns: Turn[] };

export type Step = { step: string; status: "todo" | "doing" | "done" | string };

export type Rail = { goal: string | null; steps: Step[]; next_action: string | null };

/** 2 is a pass that counts, 1 a pass made before a later failure, 0 a pass still to make. */
export type Dots = [number, number, number];

export type Row = { dots: Dots; name: string; what: string; note: string };

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
  view: string;
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

/** Everything the page may ask of the core. The window and the demo both implement it. */
export interface Core {
  initialFolder(): Promise<string | null>;
  recent(): Promise<string[]>;
  pickFolder(): Promise<string | null>;
  pickFiles(): Promise<string[]>;
  openFolder(path: string): Promise<Home>;
  home(): Promise<Home>;
  setUp(): Promise<Home>;
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
  setProgressView(view: string): Promise<void>;
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
