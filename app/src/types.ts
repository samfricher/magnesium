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
  checks_due: number | null;
  checks_alone?: number | null;
  redos_due: number;
  opening: { checks: number; redos: number } | null;
  first_check?: string | null;
  first_check_id?: string | null;
  posed_check?: string | null;
  can_count_today?: number | null;
  first_redo: string | null;
  papers: Paper[];
  can_resume: boolean;
  sitting: PaperSitting | null;
  block_minutes: number;
};

export type Proofs = { earned: number; total: number; proven: number; skills: number; level: number; to_next: number; levelled: boolean };

export type Card = {
  folder: string;
  name: string;
  state: HomeState;
  exam: Exam | null;
  goal: string | null;
  days_ago: number | null;
  proofs: Proofs;
  due: number;
  sitting: PaperSitting | null;
};

export type Activity = { today: string; days: string[]; run: number; studied_today: boolean };

export type Turn = {
  session: string;
  ms: number;
  learner: string;
  tutor: string;
  interrupted: boolean;
  intent?: Intent | null;
  task?: string | null;
  posed?: boolean;
  noted?: boolean;
  mark_ids?: string[] | null;
  closing?: string[] | null;
  step?: StepBegun | null;
};

export type StepBegun = { n: number; of: number; name: string };

export type LessonInfo = {
  session: string;
  first_ms: number;
  last_ms: number;
  turns: number;
  opening: string;
  current: boolean;
  title: string | null;
  opened_ms: number;
  goal?: string | null;
};

export type Workspace = { folder: string; name: string; opened_ms: number | null; latest: LessonInfo | null };

export type MenuSpec = {
  keys: Record<string, string>;
  workspaces: { id: string; title: string }[];
  tabs: { id: string; title: string }[];
  disabled: string[];
  titles: Record<string, string>;
};

export type MenuChoice = { command: string; arg: string | null };

export type Where = { folder?: string; session?: string | null; title?: string | null };

export type Engine = "plugin" | "direct";

export type EngineChoice = { chosen: Engine; conversation: Engine | null };

export type ModelOption = { value: string; resolved?: string; name: string; description: string; efforts?: string[]; current: boolean };

export type Pick = { model: string; name?: string; effort?: string };

export type ModelChoice = { models: ModelOption[]; failed: boolean; pick: Pick | null; not_offered: boolean; on?: On | null };

export type On = { model: string; effort?: string | null; live: boolean };

export type ModelAt = { folder: string; session: string | null; live: boolean };

export type Opened = { resumed: boolean; turns: Turn[]; engine: Engine };

export type Step = { step: string; status: "todo" | "doing" | "done" | string };

export type Rail = { goal: string | null; steps: Step[]; next_action: string | null };

export type Intent = "talk" | "answer";

export type PlanItem = {
  id: string;
  title: string;
  status: "todo" | "doing" | "done";
  day: string | null;
  note: string | null;
};

export type Plan = {
  target: string | null;
  title: string | null;
  exam_date: string | null;
  goal: string | null;
  steps: Step[];
  items: PlanItem[];
  text: string | null;
  path: string | null;
  changed_ms: number | null;
};

export type PlanPart = "exam_date" | "items" | "goal" | "steps" | "text";

export type Dots = [number, number, number];

export type Row = {
  dots: Dots;
  name: string;
  what: string;
  note: string;
  due: boolean;
  next_day: string | null;
  needs_repair?: boolean;
  last_answer?: string | null;
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
  checks_unknown: boolean;
  proofs: Proofs;
  level_unknown: boolean;
  today: Today | null;
  message: string | null;
};

export type Today = { day: string; answered: number; started: number; proofs: number; due_tomorrow: number };

export type Choice = { label: string; description: string | null };

export type Question = { question: string; header: string | null; options: Choice[]; multi: boolean };

export type Decision = "allow_once" | "allow_always" | "refuse";

export type Answer = { question: string; chosen: string[] };

export type NoticeKind = "usage_limit" | "sign_in" | "retry" | "turn" | "other" | "record_unopened" | "record_unopened_at_effort" | "model_unavailable";

export type AgentEvent =
  | { type: "ready"; session_id: string; problems: string[]; plugin_missing: boolean; model?: string | null }
  | { type: "turn_started" }
  | { type: "text_delta"; text: string }
  | { type: "text_done"; text: string }
  | { type: "working"; doing: string | null }
  | { type: "permission"; id: string; title: string; detail: string; exact?: string | null; can_always: boolean; always?: string | null }
  | { type: "question"; id: string; questions: Question[] }
  | { type: "withdrawn"; id: string }
  | { type: "turn_ended"; ok: boolean; interrupted: boolean; error: string | null }
  | { type: "notice"; kind: NoticeKind; text: string }
  | { type: "exited"; expected: boolean; detail: string | null }
  | { type: "plan_changed"; what: PlanPart[] }
  | { type: "marked"; mark: Mark }
  | { type: "open_task"; task: string | null; posed: boolean; fallback?: boolean }
  | { type: "closing"; lines: string[] }
  | ({ type: "step_begun" } & StepBegun)
  | { type: "settled" };

export type Words = { shown: string; sent: string };

export type Added = { saved: string[]; keys: number; lines: string[] };

export type Mark = {
  id: string;
  at: string;
  label: string;
  mark: "right" | "partly" | "wrong" | "declined" | string;
  help: "none" | "hint" | "step" | "solution" | string;
  cold: boolean;
  paper: string | null;
  pass?: number | null;
  next_day?: string | null;
  receipt?: string | null;
};

export type Materials = { names: string[]; files: number; keys: number };

export type Source = {
  name: string;
  kind: "pdf" | "text" | "image";
  text: string | null;
  truncated: boolean;
  image: string | null;
};

export type Payment = "plan" | "api_key";

export type Account = {
  name: string;
  installed: boolean;
  signed_in: boolean;
  paid_with: Payment | null;
  detail: string | null;
  problem: string | null;
  install_page: string | null;
};

export type Program = { shown_path: string; version: string };

export type RuntimeState = "ready" | "missing" | "different" | "kept" | "too_old" | "failed";

export type RuntimeSetup = {
  state: RuntimeState;
  installed: Program | null;
  bundled: Program | null;
  installed_now: boolean;
  message: string | null;
};

export type Permissions = "auto" | "ask";

export type Setup = { runtime: RuntimeSetup; account: Account; payment: Payment; permissions: Permissions };

export type About = { version: string; commit: string | null };

export interface Core {
  initialFolder(): Promise<string | null>;
  recent(): Promise<string[]>;
  library(): Promise<Card[]>;
  workspaces(): Promise<Workspace[]>;
  forgetFolder(path: string): Promise<void>;
  findFolders(): Promise<number>;
  activity(): Promise<Activity>;
  pickFolder(): Promise<string | null>;
  pickFiles(): Promise<string[]>;
  pickMaterial(): Promise<string[]>;
  pickNewFolder(): Promise<string | null>;
  openFolder(path: string): Promise<Home>;
  visitFolder(path: string): Promise<Home>;
  newFolder(path: string): Promise<Home>;
  home(folder?: string): Promise<Home>;
  setUp(folder?: string): Promise<Home>;
  upgradeFolder(folder?: string): Promise<Home>;
  setup(): Promise<Setup>;
  replaceRuntime(): Promise<RuntimeSetup>;
  keepRuntime(): Promise<RuntimeSetup>;
  account(): Promise<Account>;
  setPayment(payment: Payment): Promise<Account>;
  setPermissions(permissions: Permissions): Promise<void>;
  engine(): Promise<EngineChoice>;
  setEngine(engine: Engine): Promise<EngineChoice>;
  models(): Promise<ModelChoice>;
  askModels(): Promise<ModelChoice>;
  setPick(model: string, effort: string | null): Promise<ModelChoice>;
  sessionModels(at: ModelAt): Promise<ModelChoice>;
  setSessionModel(at: ModelAt, model: string, effort: string | null): Promise<ModelChoice>;
  signIn(): Promise<void>;
  about(): Promise<About>;
  showWindow(): Promise<void>;
  closeWindow(): Promise<void>;
  turns(): Promise<Turn[]>;
  lessons(folder: string): Promise<LessonInfo[]>;
  lessonTurns(folder: string, session: string): Promise<Turn[]>;
  touchSession(folder: string, session: string): Promise<void>;
  renameSession(folder: string, session: string, title: string): Promise<void>;
  shareFile(path: string, folder: string, at: { x: number; y: number; width: number; height: number }): Promise<void>;
  openSession(resume: boolean, where?: Where): Promise<Opened>;
  send(text: string, intent?: Intent): Promise<void>;
  sendNewLesson(goal: string): Promise<void>;
  sendShown(shown: string, sent: string): Promise<void>;
  interrupt(): Promise<void>;
  decide(id: string, decision: Decision): Promise<void>;
  answer(id: string, answers: Answer[]): Promise<void>;
  stop(stoppedAt: string | null): Promise<void>;
  rail(): Promise<Rail>;
  plan(folder?: string): Promise<Plan>;
  progress(target: string | null, folder?: string): Promise<Progress>;
  setBlockMinutes(minutes: number): Promise<void>;
  attach(path: string, group: string, folder?: string): Promise<string>;
  attachBytes(name: string, bytes: Uint8Array, group: string, folder?: string): Promise<string>;
  addMaterial(paths: string[], folder?: string): Promise<Added>;
  addPastedText(text: string, folder?: string): Promise<Added>;
  materialList(folder?: string): Promise<Materials>;
  marks(folder?: string): Promise<Mark[] | null>;
  onDrop(handler: (paths: string[]) => void, hover: (over: boolean) => void): Promise<() => void>;
  source(path: string, folder?: string): Promise<Source>;
  sourcePdf(path: string, folder?: string): Promise<ArrayBuffer>;
  openFile(path: string, folder?: string): Promise<void>;
  openUrl(url: string): Promise<void>;
  image(path: string, folder?: string): Promise<string>;
  paperStart(paper: Paper, minutes: number, folder?: string): Promise<PaperSitting>;
  paperEnd(files: string[], folder?: string): Promise<Words | null>;
  windowState(): Promise<unknown>;
  setWindowState(value: unknown): Promise<void>;
  setMenu(spec: MenuSpec): Promise<void>;
  onMenu(handler: (choice: MenuChoice) => void): Promise<() => void>;
  onEvent(handler: (event: AgentEvent) => void): Promise<() => void>;
  selftest(): Promise<boolean>;
  selftestReport(report: unknown): Promise<void>;
  glassState(): Promise<GlassState>;
  setGlass(material: GlassMaterial | null, dark: boolean | null): Promise<void>;
}

export type GlassState = { can: boolean; liquid: boolean; reduce: boolean };
export type GlassMaterial = "frosted" | "liquid";
