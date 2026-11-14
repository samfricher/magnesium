//! The direct engine: the window runs the lesson, and Claude Code is only the
//! way to reach the model on the learner's own subscription.
//!
//! Each turn, code reads the lesson state from the runtime, sends the
//! learner's words and that state to the model, shows the reply as it
//! streams, and saves the record itself. The model answers in plain text and
//! ends with a record block of its judgment (see [`crate::record`]), which
//! is never shown. Its only tools look things up: Read in the study folder,
//! and Magnesium's `search_sources` and `sources`.
//!
//! Claude Code is started as the model alone: Magnesium's own system prompt
//! in place of Claude Code's, no plugin, no hooks, no settings files, no
//! CLAUDE.md, no account connectors. See [`direct_args`].
//!
//! The record goes through the runtime's host protocols, as a client (see
//! [`crate::host_client`]): the same hooks and `learning_step` a lesson run
//! by the plugin uses, so the same checks, tickets and spacing apply.

use std::collections::VecDeque;
use std::env;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use chrono::Local;
use serde_json::{json, Value};

use crate::agent::{
    Account, AgentError, AgentEvent, AgentResult, Answer, Conversation, Decision, EventSink,
    Harness, Intent, NoticeKind, Payment, StartOptions,
};
use crate::claude::{self, ClaudeCode};
use crate::gaplog;
use crate::host_client::{self, HostClient};
use crate::lesson_state::{self, Facts};
use crate::paths;
use crate::record::{self, Control, Cutter, DueCheck, Lesson, Message, NotARedo, Record};
use crate::store::Store;

/// Set this to a file to use its text as the tutor's prompt in place of the
/// one built in. A conversation keeps the prompt it started with: Claude
/// Code records it once per conversation, so a changed file reaches new
/// conversations only.
pub const PROMPT_ENV: &str = "MAGNESIUM_TUTOR_PROMPT";
/// The tutor's prompt, built in.
pub const PROMPT: &str = include_str!("../prompts/tutor.md");

/// Put before the words of a new lesson or of the window's opening line, so
/// the engine can tell them from words the learner typed. It never reaches
/// the model or the record.
const LESSON_MARK: &str = "\u{1}lesson\u{1}";
/// Put before words the window sends as talk, not an answer: the turn can
/// mark nothing and keeps the open task open. It never reaches the model or
/// the record.
const TALK_MARK: &str = "\u{1}talk\u{1}";

/// The lines the window opens a returning sitting with (`app/src/lesson.ts`).
const BACK: &str = "I'm back. Let's continue.";
const NO_CHECKS: &str = "No checks right now. Let's continue where I stopped.";
const WORK_ON: &str = "Let's work on ";

/// Magnesium's tools the model may not see: the record is the engine's.
const RECORD_TOOLS: [&str; 3] = [
    "mcp__magnesium__context",
    "mcp__magnesium__learning_step",
    "mcp__magnesium__memory",
];
/// Magnesium's tools the model may use, both to look something up. Read is
/// allowed only inside the study root: see [`direct_args`].
const LOOKUP_TOOLS: [&str; 2] = ["mcp__magnesium__search_sources", "mcp__magnesium__sources"];
/// How long without a message ends a sitting, as the runtime counts it.
const SITTING_GAP: Duration = Duration::from_secs(3 * 60 * 60);

/// The nearest folder at or above `folder` that Magnesium set up: the study
/// root, where course material and plans live.
pub fn study_root(folder: &Path) -> PathBuf {
    folder
        .ancestors()
        .find(|dir| dir.join(".magnesium").is_dir())
        .unwrap_or(folder)
        .to_path_buf()
}

/// Claude Code's arguments for a direct lesson, beyond the program itself.
///
/// - `--system-prompt` replaces Claude Code's prompt with the tutor's.
/// - `--setting-sources ""` loads no settings file, so no plugin, hook,
///   skill or tool server of the learner's or the folder's comes in;
///   `--settings` turns hooks off for good measure.
/// - `--tools Read,Edit,Write` keeps three of Claude Code's tools. Read is
///   allowed inside the study root, and Edit and Write on its plan files
///   only (`Study/<plan>/plan.md` and `graph.yaml`), so the tutor can change
///   the plan as the plugin engine's tutor does; `.magnesium/` is denied
///   outright. Magnesium's tool server is the only one
///   (`--strict-mcp-config`), and its three record tools are disallowed,
///   which takes their definitions out of the model's context.
/// - Nothing ever asks the learner for permission (`--permission-prompts
///   none`): the allowed tools run, and anything else is refused.
/// - `--add-dir` lets Read reach a study root above the open folder.
pub fn direct_args(
    folder: &Path,
    root: &Path,
    runtime: &Path,
    version: &str,
    prompt: &str,
    session: &str,
    resume: bool,
) -> Vec<String> {
    let server = json!({"mcpServers": {"magnesium": {
        "command": runtime,
        "args": ["_internal", "mcp-server", "--plugin-version", version,
                 "--adapter-protocol-version", "2", "--host-kind", "claude"],
    }}});
    let mut args: Vec<String> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--model",
        claude::MODEL,
        "--system-prompt",
        prompt,
        "--setting-sources",
        "",
        "--settings",
        r#"{"disableAllHooks":true}"#,
        "--disable-slash-commands",
        "--tools",
        "Read,Edit,Write",
        "--strict-mcp-config",
        "--mcp-config",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    args.push(server.to_string());
    args.push("--disallowedTools".into());
    args.push(
        RECORD_TOOLS
            .iter()
            .map(|tool| (*tool).to_owned())
            .chain(
                ["Edit", "Write"]
                    .iter()
                    .map(|tool| format!("{tool}(/{}/.magnesium/**)", root.display())),
            )
            .collect::<Vec<_>>()
            .join(","),
    );
    // A bare Read would approve every path on the disk: a live check on
    // 2026-11-11 read a file outside the study folder with it. Scoped to the
    // study root, a read outside it is refused.
    args.push("--allowedTools".into());
    args.push(
        std::iter::once(format!("Read(/{}/**)", root.display()))
            .chain(plan_rules(root))
            .chain(LOOKUP_TOOLS.iter().map(|tool| (*tool).to_owned()))
            .collect::<Vec<_>>()
            .join(","),
    );
    args.push("--permission-prompts".into());
    args.push("none".into());
    if root != folder {
        args.push("--add-dir".into());
        args.push(root.display().to_string());
    }
    // One token each, so an id can never be read as another flag.
    args.push(if resume {
        format!("--resume={session}")
    } else {
        format!("--session-id={session}")
    });
    args
}

/// The files of a study plan a tutor may change, in Claude Code's rule
/// syntax: Edit and Write on `Study/<plan>/plan.md` and
/// `Study/<plan>/graph.yaml` in the study root, and nothing else. `*` takes
/// one folder name, never a `/`, so nothing deeper and nothing outside
/// `Study/` matches. Claude Code's Edit rules cover every tool that edits.
pub fn plan_rules(root: &Path) -> Vec<String> {
    let mut rules = Vec::new();
    for file in ["plan.md", "graph.yaml"] {
        for tool in ["Edit", "Write"] {
            rules.push(format!("{tool}(/{}/Study/*/{file})", root.display()));
        }
    }
    rules
}

/// The environment Claude Code gets, besides the window's own.
pub const ISOLATION: [(&str, &str); 3] = [
    ("CLAUDE_CODE_DISABLE_CLAUDE_MDS", "1"),
    ("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1"),
    (claude::CONNECTORS_ENV, "false"),
];

fn new_session_id() -> String {
    let mut bytes = [0u8; 16];
    let random = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes));
    if random.is_err() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        bytes = (nanos ^ (u128::from(std::process::id()) << 64)).to_le_bytes();
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// The runtime's version, as `magnesium --version` prints it.
fn runtime_version(program: &Path) -> Option<String> {
    let mut command = paths::light_command(program);
    command.arg("--version");
    let out = paths::run(command, Duration::from_secs(5)).ok()?;
    out.split_whitespace().last().map(str::to_owned)
}

/// The tutor's prompt: the file named by [`PROMPT_ENV`], or the one built in.
pub fn prompt() -> String {
    env::var_os(PROMPT_ENV)
        .filter(|path| !path.is_empty())
        .and_then(|path| std::fs::read_to_string(path).ok())
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| PROMPT.to_owned())
}

/// The direct engine. Claude Code's sign-in, account and payment are
/// Claude Code's own and are reached through [`ClaudeCode`].
pub struct DirectTutor {
    claude: ClaudeCode,
    runtime: Option<PathBuf>,
    /// The window's store, where a recording-off the learner asked for is
    /// kept across conversations.
    store: Option<Arc<Store>>,
}

impl Default for DirectTutor {
    fn default() -> Self {
        Self::new()
    }
}

impl DirectTutor {
    pub fn new() -> Self {
        Self {
            claude: ClaudeCode::new(),
            runtime: None,
            store: None,
        }
    }

    /// Keep the learner's recording-off in the window's store, so it holds
    /// in every later conversation in the folder until they turn it on.
    pub fn with_store(mut self, store: Arc<Store>) -> Self {
        self.store = Some(store);
        self
    }

    /// Use these programs for Claude Code and the runtime. For tests.
    pub fn with_programs(claude: PathBuf, runtime: PathBuf) -> Self {
        Self {
            claude: ClaudeCode::with_program(claude),
            runtime: Some(runtime),
            store: None,
        }
    }

    fn runtime(&self) -> AgentResult<PathBuf> {
        self.runtime
            .clone()
            .or_else(crate::runtime::program)
            .ok_or_else(|| AgentError("The magnesium program is not installed.".into()))
    }
}

impl Harness for DirectTutor {
    fn locate(&self) -> AgentResult<PathBuf> {
        self.claude.locate()
    }

    fn start(&self, options: StartOptions, sink: EventSink) -> AgentResult<Box<dyn Conversation>> {
        let program = self.locate()?;
        let runtime = self.runtime()?;
        let version = runtime_version(&runtime).ok_or_else(|| {
            AgentError(
                "The magnesium program did not say its version. In a terminal, run: magnesium info"
                    .into(),
            )
        })?;
        let folder = options.folder.clone();
        let root = study_root(&folder);
        let resumed = options
            .resume
            .clone()
            .filter(|id| claude::is_session_id(id));
        let session = resumed.clone().unwrap_or_else(new_session_id);
        let version = version.split('+').next().unwrap_or_default().to_owned();
        let mut command = paths::command(&program);
        claude::apply_payment(&mut command, options.payment);
        command
            .current_dir(&folder)
            .env("PWD", &folder)
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .envs(ISOLATION)
            .args(direct_args(
                &folder,
                &root,
                &runtime,
                &version,
                &prompt(),
                &session,
                resumed.is_some(),
            ))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|e| AgentError(format!("The window could not start Claude Code: {e}.")))?;
        let stdin = child.stdin.take();
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AgentError("Claude Code gave no output pipe.".into()))?;
        let stderr = child.stderr.take();
        let (events_in, events) = channel();
        let host = HostClient::new(runtime.clone(), folder.clone(), session.clone(), version);
        let recording_off = self
            .store
            .as_ref()
            .is_some_and(|store| store.folder(&root).recording_off);
        let memo = self
            .store
            .as_ref()
            .and_then(|store| store.folder(&folder).direct)
            .and_then(|value| serde_json::from_value::<Memo>(value).ok());
        let shared = Arc::new(Shared {
            store: self.store.clone(),
            root,
            sink,
            stdin: Mutex::new(stdin),
            child: Mutex::new(Some(child)),
            host,
            folder,
            session: session.clone(),
            resumed: resumed.is_some(),
            payment: options.payment,
            queue: Mutex::new(VecDeque::new()),
            wake: Condvar::new(),
            events: Mutex::new(events),
            phase: AtomicU8::new(IDLE),
            gate: Mutex::new(()),
            interrupting: AtomicBool::new(false),
            interrupt_sent: AtomicBool::new(false),
            turn_seq: AtomicU64::new(0),
            working: AtomicBool::new(false),
            closing: AtomicBool::new(false),
            ready_sent: AtomicBool::new(false),
            interrupts: AtomicU64::new(0),
            counter: AtomicU64::new(0),
            stderr_tail: Mutex::new(String::new()),
            state: Mutex::new({
                let mut state = State {
                    halt: recording_off.then_some(Halt::RecordingOff),
                    ..State::default()
                };
                state.restore(memo, &session, resumed.is_some());
                state
            }),
            trace: env::var_os(claude::TRACE_ENV)
                .filter(|path| !path.is_empty())
                .and_then(|path| {
                    std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                        .ok()
                })
                .map(Mutex::new),
            started: Instant::now(),
        });
        if let Some(mut stderr) = stderr {
            let shared = Arc::clone(&shared);
            thread::spawn(move || {
                let mut buffer = [0u8; 2048];
                while let Ok(read) = stderr.read(&mut buffer) {
                    if read == 0 {
                        break;
                    }
                    let mut tail = lock(&shared.stderr_tail);
                    tail.push_str(&String::from_utf8_lossy(&buffer[..read]));
                    if tail.len() > 4000 {
                        let cut = (tail.len() - 2000..tail.len())
                            .find(|i| tail.is_char_boundary(*i))
                            .unwrap_or(0);
                        *tail = tail[cut..].to_owned();
                    }
                }
            });
        }
        {
            let shared = Arc::clone(&shared);
            thread::spawn(move || read_loop(&shared, stdout, &events_in));
        }
        {
            let shared = Arc::clone(&shared);
            thread::spawn(move || work_loop(&shared));
        }
        // The runtime learns of the conversation as it would from Claude
        // Code. A hook fails open, so a failure here never stops the lesson.
        // With recording off, nothing of this conversation reaches it.
        let source = if shared.resumed { "resume" } else { "startup" };
        if recording_off {
            shared.note("recording is off in this study folder: no hooks, no record");
        } else if let Err(why) =
            shared
                .host
                .hook("session-start", "SessionStart", json!({"source": source}))
        {
            shared.note(&format!("session-start hook failed: {why}"));
        }
        // Ready, and so remembered, only once Claude Code shows it has the
        // conversation: it reports after the first message.
        Ok(Box::new(DirectConversation { shared }))
    }

    fn new_lesson(&self, goal: &str) -> String {
        format!("{LESSON_MARK}{}", goal.trim())
    }

    /// A paper is handed in on the plugin engine; these words are never
    /// sent to a direct lesson.
    fn papers(&self, request: &str) -> String {
        request.trim().to_owned()
    }

    fn account(&self, payment: Payment) -> Account {
        self.claude.account(payment)
    }

    fn sign_in(&self, payment: Payment) -> AgentResult<()> {
        self.claude.sign_in(payment)
    }
}

const IDLE: u8 = 0;
/// Reading the record and sending the message, before the first word.
const PREPARING: u8 = 1;
/// The model is writing the reply.
const WRITING: u8 = 2;
/// The reply is whole; the record is being saved.
const SAVING: u8 = 3;

enum Item {
    Words(String),
    /// Claude Code's output closed.
    Ended,
}

/// A stop the learner asked for, in their own words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Halt {
    /// Nothing more reaches the record, in this conversation or a later one
    /// in the folder, until the learner turns recording on.
    RecordingOff,
    /// The lesson is paused, or stopped, in this conversation: the record
    /// keeps what came before. Opening a lesson from the window goes on.
    Paused,
    Stopped,
}

impl Halt {
    /// What the tutor is told while it lasts.
    fn line(self) -> &'static str {
        match self {
            Self::RecordingOff => "Recording is off, as the learner asked: nothing from here on is kept in their record, and their answers get no mark. Teach only what they ask for, and set no quiz they did not ask for. Recording comes back on only when they ask for it.",
            Self::Paused => "The learner paused the lesson: their record keeps what came before, and nothing from here on. Set no task and ask no question unless they ask to go on. To go on with their record, they open the lesson again from the course page (Continue).",
            Self::Stopped => "The learner stopped the lesson: their record keeps what came before, and nothing from here on. Set no task and ask no question unless they ask to go on. To go on with their record, they open the lesson again from the course page (Continue).",
        }
    }
}

/// What the engine carries from turn to turn, besides the lesson.
#[derive(Default)]
struct State {
    lesson: Lesson,
    /// A stop, pause or recording off in force.
    halt: Option<Halt>,
    /// Whether the lesson's whole state has gone to the model.
    told: bool,
    /// Help lines from the last save, to tell the model next turn.
    help: Vec<String>,
    /// The quick-check names last told to the model.
    labels_told: Vec<String>,
    labels: Vec<String>,
    /// When the learner last sent a message, to tell a new sitting. The
    /// wall clock, as the runtime's: on macOS an `Instant` stops while the
    /// Mac sleeps, so a window left open overnight would see no new sitting.
    last_message: Option<SystemTime>,
    /// The runtime was asked for what this sitting opens with.
    checks_asked: bool,
    /// The turn in which the learner chose to go on without what the
    /// sitting opens with. It holds for what that turn brings, and for
    /// nothing a later sitting opens with.
    skip_turn: Option<u64>,
    /// What a sitting opens with, when it came with a turn's save, after
    /// the state had gone to the model: the next turn tells it.
    late: Option<Due>,
    /// The timed paper redo that is open, by its rows, and when the window
    /// showed the reply that posed it whole. The wall clock, so that time
    /// the Mac sleeps counts.
    redo_clock: Option<(Vec<String>, SystemTime)>,
    /// What the record made of the last paper redo or due quick check, to
    /// tell the tutor.
    outcome: Option<String>,
    /// Why no task is open although the last reply may have set one, to
    /// tell the tutor with the next message.
    no_task: Option<String>,
    /// Repairs newly waiting, to list for the tutor with the next message.
    repairs_new: Vec<record::WaitingRepair>,
    /// What the record made of the last repair, to tell the tutor.
    repair_outcome: Option<String>,
    /// The store showed that the last message was a sitting ago.
    known_new_sitting: bool,
    /// The learner's memories the tutor has been shown, by the short id it
    /// names them with: ("m1", the runtime's memory id).
    memories: Vec<(String, String)>,
    /// Lines on memories made since the tutor was last told.
    memories_new: Vec<String>,
    turn: u64,
}

/// What a sitting opens with: its quick checks, and the gap-log rows whose
/// redo is due, as the runtime sends them.
type Due = (Vec<DueCheck>, Value);

/// What the engine keeps in the window's store about the sitting, so that a
/// window started again knows it: when the learner last wrote, what the
/// sitting taught, and, for the same conversation, the repairs, checks and
/// redos it listed by number. Without it, a restarted window would number
/// repairs from one again and bind a finished one to the wrong rows.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
struct Memo {
    session: String,
    /// The wall clock, in milliseconds.
    last_message_ms: i64,
    #[serde(default)]
    sitting_labels: Vec<String>,
    #[serde(default)]
    sitting_unknown: bool,
    #[serde(default)]
    today: String,
    #[serde(default)]
    answered_today: Vec<String>,
    #[serde(default)]
    repairs: Vec<record::WaitingRepair>,
    #[serde(default)]
    repair_numbers: i64,
    #[serde(default)]
    checks: Vec<DueCheck>,
    #[serde(default)]
    posed_checks: Vec<DueCheck>,
    #[serde(default)]
    redos: Vec<record::DueRedo>,
}

fn epoch_ms(time: SystemTime) -> i64 {
    time.duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

impl State {
    /// Take back what the store kept of the sitting. A sitting still under
    /// way keeps what it taught, in any conversation in the folder, and,
    /// in the same conversation, what it listed by number. A resumed
    /// conversation with nothing kept cannot know what its sitting taught,
    /// and sends nothing cold until a new one begins.
    fn restore(&mut self, memo: Option<Memo>, session: &str, resumed: bool) {
        let Some(memo) = memo else {
            self.lesson.sitting_unknown = resumed;
            return;
        };
        let same = memo.session == session;
        if same {
            // A number is never given twice in a conversation.
            self.lesson.repair_numbers = memo.repair_numbers;
        }
        let last = std::time::UNIX_EPOCH
            + Duration::from_millis(u64::try_from(memo.last_message_ms).unwrap_or(0));
        let gap = SystemTime::now().duration_since(last).unwrap_or_default();
        if gap > SITTING_GAP {
            self.known_new_sitting = true;
            return;
        }
        self.last_message = Some(last);
        self.lesson.sitting_labels = memo.sitting_labels;
        self.lesson.sitting_unknown = memo.sitting_unknown || (resumed && !same);
        self.lesson.today = memo.today;
        self.lesson.answered_today = memo.answered_today;
        if same {
            self.lesson.repairs = memo.repairs;
            self.lesson.checks = memo.checks;
            self.lesson.posed_checks = memo.posed_checks;
            self.lesson.redos = memo.redos;
            self.checks_asked = true;
        }
    }

    fn memo(&self, session: &str) -> Option<Memo> {
        Some(Memo {
            session: session.to_owned(),
            last_message_ms: epoch_ms(self.last_message?),
            sitting_labels: self.lesson.sitting_labels.clone(),
            sitting_unknown: self.lesson.sitting_unknown,
            today: self.lesson.today.clone(),
            answered_today: self.lesson.answered_today.clone(),
            repairs: self.lesson.repairs.clone(),
            repair_numbers: self.lesson.repair_numbers,
            checks: self.lesson.checks.clone(),
            posed_checks: self.lesson.posed_checks.clone(),
            redos: self.lesson.redos.clone(),
        })
    }
}

impl State {
    /// A new sitting: what the last one listed and did not pose is gone.
    /// It is listed again if it is still due. Not knowing what the sitting
    /// taught lasts until a new sitting is known to begin.
    fn begin_sitting(&mut self, known: bool) {
        let unknown = self.lesson.sitting_unknown && !known;
        self.checks_asked = false;
        self.lesson.begin_sitting();
        self.lesson.sitting_unknown = unknown;
    }
}

/// The learner's notice for a right answer to a paper redo that the record
/// did not count: no pass and no fail.
const REDO_NO_COUNT: &str = "Your answer is saved, but it did not count for the paper question this time. That question stays due.";
/// The same, for a right answer the record holds as a fail.
const REDO_FAILED: &str = "Your answer was right, but with help or over the time it does not pass the paper question. That question comes back after a repair.";
/// For a task the reply calls a paper redo and the window has none for.
const REDO_UNLISTED: &str = "The tutor called this a paper redo, but the window has none to record it for. Your answer will be saved as an ordinary answer.";
/// For a paper redo that gave way to something else before its answer.
const REDO_SET_ASIDE: &str = "The paper redo was set aside before it was answered. It stays due.";
/// The learner's notice for a right answer to a due quick check that the
/// record holds no pass for.
const CHECK_NO_PASS: &str = "Your answer is saved, but it did not count as a pass for the quick check this time. That check stays due.";
/// What the tutor is told after a reply the learner stopped part-way.
const STOPPED: &str = "Your last reply was stopped part-way by the learner, and nothing of it was recorded: no task is open, and an answer to what it asked gets no mark. Pose the task again before you mark an answer to it.";
/// What the tutor is told after recording comes back on.
const RECORDING_BACK: &str = "Recording is back on. Nothing said while it was off was recorded, so no task is open: pose the task again before you mark an answer to it.";
/// What the tutor is told after a reply whose record could not be read.
const UNREAD: &str = "Your last reply's record block was unreadable, so nothing of it was recorded: no task is open, and an answer to what it asked gets no mark. Pose the task again before you mark an answer to it.";

struct Shared {
    store: Option<Arc<Store>>,
    /// The study root, by which a recording-off is kept.
    root: PathBuf,
    sink: EventSink,
    stdin: Mutex<Option<ChildStdin>>,
    child: Mutex<Option<Child>>,
    host: HostClient,
    folder: PathBuf,
    session: String,
    resumed: bool,
    payment: Payment,
    queue: Mutex<VecDeque<Item>>,
    wake: Condvar,
    events: Mutex<Receiver<Option<Value>>>,
    phase: AtomicU8,
    /// Held while the phase changes and while an interrupt decides what to
    /// do, so an interrupt never falls between two phases.
    gate: Mutex<()>,
    interrupting: AtomicBool,
    /// The interrupt was sent to Claude Code during this reply.
    interrupt_sent: AtomicBool,
    /// Counts turns, so an interrupt's watchdog acts only on its own.
    turn_seq: AtomicU64,
    /// A message is being handled, its record included. Changed under the
    /// queue's lock, and `wake` tells who waits.
    working: AtomicBool,
    closing: AtomicBool,
    ready_sent: AtomicBool,
    interrupts: AtomicU64,
    counter: AtomicU64,
    stderr_tail: Mutex<String>,
    state: Mutex<State>,
    trace: Option<Mutex<std::fs::File>>,
    started: Instant,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}

impl Shared {
    fn emit(&self, event: AgentEvent) {
        (self.sink)(event);
    }

    fn ready(&self) {
        if !self.ready_sent.swap(true, Ordering::SeqCst) {
            self.emit(AgentEvent::Ready {
                session_id: self.session.clone(),
                problems: Vec::new(),
                plugin_missing: false,
            });
        }
    }

    fn trace(&self, direction: &str, line: &str) {
        if let Some(file) = &self.trace {
            let ms = self.started.elapsed().as_millis();
            let _ = writeln!(lock(file), "{direction} {ms} {}", line.trim_end());
        }
    }

    /// A line in the trace about what the engine did, such as an answer
    /// left with no mark.
    fn note(&self, text: &str) {
        self.trace("#", text);
    }

    fn write(&self, value: &Value) -> AgentResult<()> {
        let mut guard = lock(&self.stdin);
        let stdin = guard
            .as_mut()
            .ok_or_else(|| AgentError("The tutor is not running.".into()))?;
        let mut line = value.to_string();
        self.trace(">", &line);
        line.push('\n');
        stdin
            .write_all(line.as_bytes())
            .and_then(|()| stdin.flush())
            .map_err(|e| AgentError(format!("The tutor stopped listening: {e}.")))
    }

    fn send_user(&self, text: &str) -> AgentResult<()> {
        self.write(&json!({
            "type": "user",
            "message": {"role": "user", "content": text},
            "parent_tool_use_id": null,
            "session_id": "default",
        }))
    }

    fn next_id(&self) -> String {
        let n = self.counter.fetch_add(1, Ordering::Relaxed) + 1;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        format!("req_{n}_{nanos:08x}")
    }

    fn signal(&self, name: &str) {
        if let Some(child) = lock(&self.child).as_ref() {
            let _ = std::process::Command::new("kill")
                .arg(format!("-{name}"))
                .arg(child.id().to_string())
                .status();
        }
    }

    fn busy(&self) -> bool {
        self.phase.load(Ordering::SeqCst) != IDLE || !lock(&self.queue).is_empty()
    }

    /// The message in hand is done, its record kept.
    fn done(&self) {
        let _queue = lock(&self.queue);
        self.working.store(false, Ordering::SeqCst);
        self.wake.notify_all();
    }

    /// Wait, at most `limit`, until no message is in hand or waiting.
    fn wait_idle(&self, limit: Duration) -> bool {
        let deadline = Instant::now() + limit;
        let mut queue = lock(&self.queue);
        loop {
            let waiting = queue.iter().any(|item| matches!(item, Item::Words(_)));
            if !self.working.load(Ordering::SeqCst) && !waiting {
                return true;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            queue = self
                .wake
                .wait_timeout(queue, left)
                .map(|(guard, _)| guard)
                .unwrap_or_else(|p| p.into_inner().0);
        }
    }

    fn push(&self, item: Item) {
        lock(&self.queue).push_back(item);
        self.wake.notify_all();
    }
}

/// Read Claude Code's output. What concerns a turn goes to the worker; the
/// rest is handled here.
fn read_loop(shared: &Arc<Shared>, stdout: impl Read, events: &Sender<Option<Value>>) {
    let mut reader = BufReader::new(stdout);
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        match reader.read_until(b'\n', &mut bytes) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let line = String::from_utf8_lossy(&bytes);
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        shared.trace("<", line);
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match (value["type"].as_str(), value["subtype"].as_str()) {
            (Some("system"), Some("init")) => on_init(shared, &value),
            (Some("system"), Some("api_retry")) => on_retry(shared, &value),
            (Some("rate_limit_event"), _) => on_rate_limit(shared, &value),
            (Some("control_request"), _) => {
                // Nothing here asks the learner: every tool is either allowed
                // or refused by the launch.
                if let Some(id) = value["request_id"].as_str() {
                    let _ = shared.write(&json!({
                        "type": "control_response",
                        "response": {"subtype": "error", "request_id": id, "error": "the Magnesium window does not handle this request"},
                    }));
                }
            }
            (Some("stream_event" | "assistant" | "result"), _) => {
                let _ = events.send(Some(value));
            }
            _ => {}
        }
    }
    let _ = events.send(None);
    shared.push(Item::Ended);
}

fn on_init(shared: &Arc<Shared>, value: &Value) {
    if let Some(text) = claude::payment_notice(shared.payment, value) {
        shared.emit(AgentEvent::Notice {
            kind: NoticeKind::SignIn,
            text,
        });
    }
    let searching = value["mcp_servers"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|server| {
            server["name"] == "magnesium"
                && matches!(server["status"].as_str(), Some("connected" | "pending"))
        });
    if value.get("mcp_servers").is_some() && !searching {
        shared.emit(AgentEvent::Notice {
            kind: NoticeKind::Other,
            text: "Magnesium's search did not start, so the tutor cannot search your course material. In a terminal, run: magnesium info".into(),
        });
    }
    shared.ready();
}

fn on_retry(shared: &Arc<Shared>, value: &Value) {
    let (kind, text) = match value["error"].as_str().unwrap_or_default() {
        "authentication_failed" | "oauth_org_not_allowed" => {
            (NoticeKind::SignIn, claude::SIGN_IN.to_owned())
        }
        "rate_limit" => (
            NoticeKind::UsageLimit,
            "Claude Code is at a usage limit and is waiting to try again.".to_owned(),
        ),
        "billing_error" | "account_on_hold" => (
            NoticeKind::SignIn,
            "Claude Code reports a billing problem on your account. Fix it in a terminal or on the web, then send your message again.".to_owned(),
        ),
        _ => (
            NoticeKind::Retry,
            format!(
                "The connection dropped. Trying again, attempt {}.",
                value["attempt"].as_i64().unwrap_or(1)
            ),
        ),
    };
    shared.emit(AgentEvent::Notice { kind, text });
}

fn on_rate_limit(shared: &Arc<Shared>, value: &Value) {
    let info = &value["rate_limit_info"];
    if info["status"].as_str() != Some("rejected")
        || info["overageStatus"]
            .as_str()
            .is_some_and(|status| status.starts_with("allowed"))
    {
        return;
    }
    let resets = info["resetsAt"]
        .as_i64()
        .or_else(|| info["resets_at"].as_i64())
        .map(|stamp| {
            if stamp > 100_000_000_000 {
                stamp / 1000
            } else {
                stamp
            }
        })
        .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
        .map(|utc| {
            utc.with_timezone(&Local)
                .format("%H:%M on %a %-d %b")
                .to_string()
        });
    shared.emit(AgentEvent::Notice {
        kind: NoticeKind::UsageLimit,
        text: match resets {
            Some(when) => format!("You reached your Claude usage limit. It resets at {when}."),
            None => "You reached your Claude usage limit.".to_owned(),
        },
    });
}

/// Take the learner's messages one at a time. A message sent while a turn
/// is under way, its hidden second request included, waits here.
fn work_loop(shared: &Arc<Shared>) {
    loop {
        let item = {
            let mut queue = lock(&shared.queue);
            loop {
                if let Some(item) = queue.pop_front() {
                    shared.working.store(true, Ordering::SeqCst);
                    break item;
                }
                queue = shared.wake.wait(queue).unwrap_or_else(|p| p.into_inner());
            }
        };
        match item {
            Item::Words(words) => {
                {
                    let _gate = lock(&shared.gate);
                    shared.turn_seq.fetch_add(1, Ordering::SeqCst);
                    shared.interrupt_sent.store(false, Ordering::SeqCst);
                    shared.phase.store(PREPARING, Ordering::SeqCst);
                }
                let ended = turn(shared, &words);
                remember(shared);
                shared.phase.store(IDLE, Ordering::SeqCst);
                shared.done();
                if ended {
                    exited(shared);
                    return;
                }
            }
            Item::Ended => {
                shared.done();
                exited(shared);
                return;
            }
        }
    }
}

/// Keep what the sitting knows in the window's store, for a window started
/// again.
fn remember(shared: &Arc<Shared>) {
    let Some(store) = &shared.store else {
        return;
    };
    let Some(memo) = lock(&shared.state).memo(&shared.session) else {
        return;
    };
    if let Ok(value) = serde_json::to_value(memo) {
        store.update(&shared.folder, |folder| folder.direct = Some(value));
    }
}

fn exited(shared: &Arc<Shared>) {
    let status = {
        let mut guard = lock(&shared.child);
        guard.take().and_then(|mut child| {
            let waited = Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => break Some(status),
                    Ok(None) if waited.elapsed() > Duration::from_secs(12) => {
                        let _ = child.kill();
                        break child.wait().ok();
                    }
                    Ok(None) => thread::sleep(Duration::from_millis(20)),
                    Err(_) => break None,
                }
            }
        })
    };
    *lock(&shared.stdin) = None;
    shared.host.close();
    let expected = shared.closing.load(Ordering::SeqCst);
    let tail = lock(&shared.stderr_tail).trim().to_owned();
    if !expected && claude::looks_signed_out(&tail) {
        shared.emit(AgentEvent::Notice {
            kind: NoticeKind::SignIn,
            text: claude::SIGN_IN.into(),
        });
    }
    let detail =
        (!expected).then(
            || match (tail.lines().rev().find(|l| !l.trim().is_empty()), status) {
                (Some(line), _) => claude::clip(line, 300),
                (None, Some(status)) if !status.success() => format!("It ended with {status}."),
                _ => "It ended without saying why.".to_owned(),
            },
        );
    shared.emit(AgentEvent::Exited { expected, detail });
}

/// What the model said in one request.
struct Said {
    cutter: Cutter,
    result: Option<Value>,
    /// Claude Code's output closed before the result.
    ended: bool,
}

/// Read one request's events to its result. With `show`, the reply goes to
/// the window as it is written; without, nothing does (the hidden request).
fn listen(shared: &Arc<Shared>, show: bool) -> Said {
    let events = lock(&shared.events);
    let mut said = Said {
        cutter: Cutter::new(),
        result: None,
        ended: false,
    };
    let mut started = false;
    let mut whole: Vec<String> = Vec::new();
    let mut whole_seen: Vec<(String, String)> = Vec::new();
    loop {
        let Ok(Some(value)) = events.recv() else {
            said.ended = true;
            break;
        };
        let main = value["parent_tool_use_id"].is_null();
        match value["type"].as_str() {
            Some("stream_event") if main => {
                let event = &value["event"];
                if show && !started {
                    started = true;
                    shared.emit(AgentEvent::TurnStarted);
                }
                match event["type"].as_str() {
                    Some("content_block_start") => match event["content_block"]["type"].as_str() {
                        // Text after a tool call is a new paragraph.
                        Some("text") if !said.cutter.is_empty() => {
                            let fresh = said.cutter.push("\n\n");
                            if show && !fresh.is_empty() {
                                shared.emit(AgentEvent::TextDelta { text: fresh });
                            }
                        }
                        Some("thinking" | "redacted_thinking") if show => {
                            shared.emit(AgentEvent::Working {
                                doing: Some("thinking".into()),
                            })
                        }
                        Some("tool_use" | "server_tool_use") if show => {
                            shared.emit(AgentEvent::Working {
                                doing: event["content_block"]["name"].as_str().and_then(|tool| {
                                    claude::doing(tool, &Value::Null, &shared.folder)
                                }),
                            })
                        }
                        _ => {}
                    },
                    Some("content_block_delta") if event["delta"]["type"] == "text_delta" => {
                        let text = event["delta"]["text"].as_str().unwrap_or_default();
                        let fresh = said.cutter.push(text);
                        if show && !fresh.is_empty() {
                            shared.emit(AgentEvent::TextDelta { text: fresh });
                        }
                    }
                    _ => {}
                }
            }
            Some("assistant") if main => {
                let id = value["message"]["id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                for block in value["message"]["content"].as_array().into_iter().flatten() {
                    match block["type"].as_str() {
                        // Claude Code may send a message once per block or
                        // once whole: each block counts once.
                        Some("text") => {
                            let text = block["text"].as_str().unwrap_or_default().to_owned();
                            if !whole_seen.contains(&(id.clone(), text.clone())) {
                                whole_seen.push((id.clone(), text.clone()));
                                whole.push(text);
                            }
                        }
                        Some("tool_use") if show => shared.emit(AgentEvent::Working {
                            doing: block["name"].as_str().and_then(|tool| {
                                claude::doing(tool, &block["input"], &shared.folder)
                            }),
                        }),
                        _ => {}
                    }
                }
            }
            Some("result") => {
                said.result = Some(value);
                break;
            }
            _ => {}
        }
    }
    // The finished text is the reply: a message streamed twice, or cut
    // short, or never streamed in pieces at all, still ends as Claude Code
    // finished it. The window's TextDone replaces the pieces shown.
    if !whole.is_empty() {
        let mut finished = Cutter::new();
        let fresh = finished.push(&whole.join("\n\n"));
        if finished.reply() != said.cutter.reply() || finished.block() != said.cutter.block() {
            if said.cutter.is_empty() && show && !fresh.is_empty() {
                shared.emit(AgentEvent::TextDelta { text: fresh });
            }
            shared.note(
                "the finished text differs from the pieces streamed: the finished text is kept",
            );
            said.cutter = finished;
        }
    }
    said
}

fn local_now() -> String {
    Local::now().format("%a %-d %b %Y, %H:%M").to_string()
}

/// The words a window opening line stands for, if these are one: whether
/// the learner skips the checks, and the course title of "Let's work on".
fn opening(words: &str) -> Option<(bool, Option<String>)> {
    if words == NO_CHECKS {
        return Some((true, None));
    }
    if words.starts_with(BACK) {
        return Some((false, None));
    }
    words
        .strip_prefix(WORK_ON)
        .map(|rest| (false, Some(rest.trim_end_matches('.').to_owned())))
}

/// What the record says of the open task, so a later conversation can
/// carry it on. It goes in the checkpoint's plan_text.
fn task_from_checkpoint(checkpoint: &Value) -> Option<record::OpenTask> {
    let plan = checkpoint["plan_text"].as_str()?;
    let text = plan
        .lines()
        .find_map(|line| line.strip_prefix(record::TASK_LINE))?
        .trim()
        .to_owned();
    let help = plan
        .lines()
        .find_map(|line| line.strip_prefix(record::HELP_LINE))
        .map(|rest| rest.trim().trim_end_matches('.'))
        .and_then(|word| serde_json::from_value(json!(word)).ok())
        .unwrap_or_default();
    let check = plan
        .lines()
        .find_map(|line| line.strip_prefix(record::CHECK_LINE))
        .map(|rest| rest.trim().trim_matches('"').to_owned());
    let redo = plan
        .lines()
        .find_map(|line| line.strip_prefix(record::REDO_LINE))
        .and_then(|rest| serde_json::from_str(rest.trim()).ok());
    Some(record::OpenTask {
        text,
        help,
        guess: plan.lines().any(|line| line == record::GUESS_LINE),
        check,
        redo,
        again: plan.lines().any(|line| line == record::AGAIN_LINE),
        stuck: false,
        skill: String::new(),
    })
}

/// What a sitting opens with, when this result carries it: the runtime's
/// `sitting_checks`, and the gap-log rows of `sitting_redos` as redos.
fn due_of(result: &Value) -> Option<Due> {
    let checks = serde_json::from_value(result.get("sitting_checks")?.clone()).ok()?;
    Some((checks, result["sitting_redos"].clone()))
}

/// The most course graphs a state names.
const MAX_GRAPHS: usize = 8;

/// The course graphs in the study root, as paths from it: each
/// `Study/<plan>/graph.yaml`, where the teaching files look for one. The
/// tutor reads the one whose tags cover the goal, as a plugin lesson does. A
/// file is named only when it is a regular file that is still inside the
/// root once every link on its way is followed, since the model's Read
/// reaches the root and nothing else.
fn course_graphs(root: &Path) -> Vec<String> {
    let Ok(inside) = root.canonicalize() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(root.join("Study")) else {
        return Vec::new();
    };
    let mut plans: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|plan| !plan.starts_with('.') && !plan.chars().any(char::is_control))
        .filter(|plan| {
            root.join("Study")
                .join(plan)
                .join("graph.yaml")
                .canonicalize()
                .is_ok_and(|file| file.starts_with(&inside) && file.is_file())
        })
        .collect();
    plans.sort();
    plans
        .into_iter()
        .take(MAX_GRAPHS)
        .map(|plan| format!("Study/{plan}/graph.yaml"))
        .collect()
}

/// The study plans in the study root, one line each for the state: the
/// file the tutor may edit, the exam it is for, and its date as the plan's
/// When cell gives it. Only a plan file that is a regular file inside the
/// root is named, as for the course graphs.
fn plan_lines(root: &Path) -> Vec<String> {
    let Ok(inside) = root.canonicalize() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(root.join("Study")) else {
        return Vec::new();
    };
    let today = Local::now().date_naive();
    let mut plans: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|plan| !plan.starts_with('.') && !plan.chars().any(char::is_control))
        .filter(|plan| {
            root.join("Study")
                .join(plan)
                .join("plan.md")
                .canonicalize()
                .is_ok_and(|file| file.starts_with(&inside) && file.is_file())
        })
        .collect();
    plans.sort();
    plans
        .into_iter()
        .take(MAX_GRAPHS)
        .map(|plan| {
            let path = format!("Study/{plan}/plan.md");
            let text = std::fs::read_to_string(root.join(&path)).unwrap_or_default();
            let facts = gaplog::table(gaplog::section(&text, "Facts"));
            let fact = |name: &str| {
                facts
                    .rows
                    .iter()
                    .find(|row| facts.cell(row, &["Item"]).eq_ignore_ascii_case(name))
                    .map(|row| facts.cell(row, &["Value"]).to_owned())
                    .filter(|value| !value.is_empty())
            };
            let title = fact("Target").unwrap_or_else(|| plan.clone());
            let when = match fact("When").as_deref().and_then(gaplog::first_date) {
                Some(date) => format!("on {date}, {} days from today", (date - today).num_days()),
                None => "with no date".to_owned(),
            };
            record::clip(&format!("{path}: {title}, {when}"), 300)
        })
        .collect()
}

/// What a turn knows before the model writes.
struct Prepared {
    /// The event the runtime captured the learner's words as.
    event: Option<String>,
    /// The lesson state to send.
    state: String,
    /// The open task is a timed paper redo with no clock running.
    no_clock: bool,
}

impl Prepared {
    /// For a turn of which nothing reaches the record.
    fn unrecorded(state: String) -> Self {
        Self {
            event: None,
            state,
            no_clock: false,
        }
    }
}

/// Before the model: capture the learner's words, start or resume the
/// runtime's lesson when none is active here, and gather the state to send.
fn prepare(
    shared: &Arc<Shared>,
    words: &str,
    from_window: bool,
    talk: bool,
    prompt_id: &str,
) -> Prepared {
    let host = &shared.host;
    let mut state = lock(&shared.state);
    let first = state.last_message.is_none();
    let gap_passed = state.last_message.is_some_and(|last| {
        SystemTime::now()
            .duration_since(last)
            .is_ok_and(|gap| gap > SITTING_GAP)
    });
    // A new sitting is known when the learner last wrote a sitting ago, in
    // this window or, as the store says, before it started.
    let known_new = gap_passed || std::mem::take(&mut state.known_new_sitting);
    if first || gap_passed {
        state.begin_sitting(known_new);
    }
    state.last_message = Some(SystemTime::now());
    let mut facts = Facts {
        now: local_now(),
        full: !state.told && !shared.resumed,
        talk,
        ..Facts::default()
    };
    let opened = from_window.then(|| opening(words)).flatten();
    facts.skip_checks = opened.as_ref().is_some_and(|(skip, _)| *skip);
    if facts.skip_checks {
        state.skip_turn = Some(state.turn);
    }
    state
        .lesson
        .set_today(&Local::now().format("%Y-%m-%d").to_string());
    // Opening a lesson from the window starts or resumes it explicitly, so
    // a pause or stop ends there. A recording-off does not: only the
    // learner's own request turns recording on.
    if from_window && matches!(state.halt, Some(Halt::Paused | Halt::Stopped)) {
        state.halt = None;
    }
    if let Some(halt) = state.halt {
        // Nothing goes to the runtime: not the words, not the hooks.
        facts.halt = Some(halt.line().to_owned());
        state.told = true;
        return Prepared::unrecorded(lesson_state::text(&state.lesson, &facts));
    }
    let notice = match host.hook(
        "user-prompt-submit",
        "UserPromptSubmit",
        json!({"prompt_id": prompt_id, "prompt": words}),
    ) {
        Ok(notice) => notice.unwrap_or_default(),
        Err(why) => {
            shared.note(&format!("user-prompt-submit hook failed: {why}"));
            String::new()
        }
    };
    shared.trace("@", &format!("user-prompt-submit: {notice}"));
    // Recording is off for this conversation in the runtime, as a control
    // from another process can leave it: keep nothing.
    if notice.contains("recording false") {
        state.halt = Some(Halt::RecordingOff);
        facts.halt = Some(Halt::RecordingOff.line().to_owned());
        state.told = true;
        return Prepared::unrecorded(lesson_state::text(&state.lesson, &facts));
    }
    let mut event = host_client::current_event(&notice);
    // What the sitting opens with comes once, with the first result of the
    // runtime's tools after the sitting's first message.
    let mut found: Option<Due> = state.late.take();
    let mut asked = false;
    if let Some(ticket) = host_client::ticket(&notice) {
        // No lesson is active in this conversation: start one, or resume
        // the one the learner comes back to.
        let request_id = format!("{}-open-{}", shared.session, shared.next_id());
        let mut request = json!({
            "schema": "learner-model/v2",
            "action": "update",
            "request_id": request_id,
            "native_capture": {"ticket": ticket, "text": words},
        });
        let new_goal = from_window && opened.is_none();
        let mut resume: Option<(String, i64, String)> = None;
        if !new_goal {
            match host.call(
                "context",
                json!({"schema": "learner-model/v2", "action": "session_choices", "limit": 3}),
            ) {
                Ok(choices) => {
                    asked = true;
                    if found.is_none() {
                        found = due_of(&choices);
                    }
                    let known = state.lesson.session_id.clone();
                    let list = choices["session_choices"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                    let chosen = list
                        .iter()
                        .find(|choice| {
                            known.is_some() && choice["session_id"].as_str() == known.as_deref()
                        })
                        .or_else(|| list.first());
                    if let Some(choice) = chosen {
                        if let (Some(id), Some(revision)) =
                            (choice["session_id"].as_str(), choice["revision"].as_i64())
                        {
                            let goal = choice["goal"].as_str().unwrap_or_default().to_owned();
                            resume = Some((id.to_owned(), revision, goal));
                        }
                    }
                }
                Err(why) => shared.note(&format!("session_choices failed: {why}")),
            }
        }
        match &resume {
            Some((id, revision, _)) => {
                request["session"] =
                    json!({"operation": "resume", "session_id": id, "expected_revision": revision});
            }
            None => {
                let goal = opened
                    .as_ref()
                    .and_then(|(_, title)| title.clone())
                    .unwrap_or_else(|| words.to_owned());
                request["session"] = json!({"operation": "start", "client_ref": "lesson", "goal": record::clip(&goal, 500)});
                state.lesson.goal = Some(goal);
            }
        }
        match host.call("learning_step", request) {
            Ok(result) => {
                asked = true;
                event = result["captured_event_ids"][0].as_str().map(str::to_owned);
                let session = match &resume {
                    Some((id, _, _)) => Some(id.clone()),
                    None => result["references"]["lesson"].as_str().map(str::to_owned),
                };
                if let Some(session) = session {
                    state.lesson.revision = result["revisions"][&session].as_i64();
                    state.lesson.session_id = Some(session);
                }
                if let Some(answers) = result["earlier_answers"].as_array() {
                    facts.earlier_answers = answers.clone();
                    let today = state.lesson.today.clone();
                    for answer in answers.iter().filter(|a| a["day"].as_str() == Some(&today)) {
                        if let Some(label) = answer["label"].as_str() {
                            state.lesson.answered(label);
                        }
                    }
                }
                if found.is_none() {
                    found = due_of(&result);
                }
            }
            Err(why) => shared.note(&format!("the lesson could not start in the record: {why}")),
        }
        if let Some((_, _, goal)) = &resume {
            // The resumed lesson's goal, steps and open task, and what the
            // record knows of the learner, as a prepare for its goal gives.
            if let Ok(prepared) = host.call(
                "context",
                json!({"schema": "learner-model/v2", "action": "prepare", "goal": goal}),
            ) {
                adopt(&mut state.lesson, &mut facts, &prepared);
                facts.known = known(&mut state, &prepared);
                if found.is_none() {
                    found = due_of(&prepared);
                }
            }
        }
    } else if event.is_some() && state.lesson.session_id.is_none() {
        // A lesson is active here, as after Claude Code resumed this
        // conversation in a new process: read it back.
        if let Ok(status) = host.call(
            "context",
            json!({"schema": "learner-model/v2", "action": "status"}),
        ) {
            asked = true;
            adopt(&mut state.lesson, &mut facts, &status);
            if found.is_none() {
                found = due_of(&status);
            }
        }
    }
    // A conversation's first state shows the learner's memories for the
    // goal, each with a short id the tutor names to revise one.
    if facts.full && state.memories.is_empty() {
        if let Some(goal) = state.lesson.goal.clone() {
            if let Ok(prepared) = host.call(
                "context",
                json!({"schema": "learner-model/v2", "action": "prepare", "goal": goal}),
            ) {
                asked = true;
                facts.known = known(&mut state, &prepared);
                if found.is_none() {
                    found = due_of(&prepared);
                }
            }
        }
    }
    // A new sitting has no open task: one posed a sitting ago, read back
    // from the record or kept here, is not what is on screen now. A check
    // posed and not answered is still due, and the runtime lists it again.
    // The turn's save writes the checkpoint without it.
    if known_new && state.lesson.task.take().is_some() {
        state.redo_clock = None;
        shared.note("a new sitting: the task open in the last one is set aside");
    }
    facts.known.extend(std::mem::take(&mut state.memories_new));
    // When the calls above made none, in a sitting not asked about yet, one
    // is made: the prompt hook's notice counts the quick checks due, and
    // says nothing of paper redos.
    let sitting_starts = !state.checks_asked;
    let mut status_now: Option<Value> = None;
    if found.is_none() && !asked && !state.checks_asked {
        if let Ok(status) = host.call(
            "context",
            json!({"schema": "learner-model/v2", "action": "status"}),
        ) {
            found = due_of(&status);
            status_now = Some(status);
        }
    }
    // A sitting starts with the paper questions waiting for a repair, as
    // the record lists them.
    if sitting_starts {
        let status = status_now.or_else(|| {
            host.call(
                "context",
                json!({"schema": "learner-model/v2", "action": "status"}),
            )
            .ok()
        });
        if let Some(rows) = status
            .as_ref()
            .and_then(|status| status.get("repairs_waiting"))
        {
            let made = state.lesson.take_waiting(rows, false);
            state.repairs_new.extend(made);
        }
    }
    state.checks_asked = true;
    if let Some((checks, rows)) = found {
        if facts.skip_checks {
            state.lesson.checks.clear();
            state.lesson.redos.clear();
        } else {
            // A tag already answered today is left out: no answer on it
            // could be cold today, so its redo could only fail.
            let redos = record::redos_from(&rows, &|tag| state.lesson.answered_earlier_today(tag));
            facts.new_checks = checks.clone();
            facts.new_redos = redos.clone();
            state.lesson.checks = checks;
            state.lesson.redos = redos;
        }
    }
    // The time a timed paper redo took: from the reply that posed it, shown
    // whole, to this message, whatever was said in between. With no clock
    // on it, as after a resume, its answer cannot earn a pass.
    let mut no_clock = false;
    if let Some(redo) = state
        .lesson
        .task
        .as_ref()
        .and_then(|task| task.redo.as_ref())
        .filter(|redo| redo.timed)
    {
        facts.took = state
            .redo_clock
            .as_ref()
            .filter(|(rows, _)| *rows == redo.rows)
            .and_then(|(_, posed)| SystemTime::now().duration_since(*posed).ok());
        no_clock = facts.took.is_none();
        facts.no_clock = no_clock;
    }
    facts.outcome = state.outcome.take();
    facts.no_task = state.no_task.take();
    facts.new_repairs = std::mem::take(&mut state.repairs_new);
    facts.repair_outcome = state.repair_outcome.take();
    if facts.full {
        facts.graphs = course_graphs(&shared.root);
        facts.plans = plan_lines(&shared.root);
    }
    facts.help = std::mem::take(&mut state.help);
    if state.labels != state.labels_told {
        facts.open_labels = state.labels.clone();
        state.labels_told = state.labels.clone();
    }
    state.told = true;
    let text = lesson_state::text(&state.lesson, &facts);
    Prepared {
        event,
        state: text,
        no_clock,
    }
}

/// Give a memory a short id the tutor can name, once.
fn short_id(state: &mut State, memory_id: &str) -> String {
    if let Some((short, _)) = state.memories.iter().find(|(_, id)| id == memory_id) {
        return short.clone();
    }
    let short = format!("m{}", state.memories.len() + 1);
    state.memories.push((short.clone(), memory_id.to_owned()));
    short
}

/// What the record knows of the learner, one line per memory, as a prepare
/// lists them, each with its short id.
fn known(state: &mut State, prepared: &Value) -> Vec<String> {
    prepared["memories"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|memory| {
            memory["lifecycle"]
                .as_str()
                .is_none_or(|life| life == "active")
        })
        .take(8)
        .filter_map(|memory| {
            let id = memory["memory_id"].as_str()?;
            let title = memory["title"].as_str()?;
            let summary = memory["summary"].as_str().unwrap_or_default();
            let short = short_id(state, id);
            Some(record::clip(&format!("{short}: {title}: {summary}"), 300))
        })
        .collect()
}

/// Take the goal, steps and open task of a lesson the record already holds.
fn adopt(lesson: &mut Lesson, facts: &mut Facts, status: &Value) {
    let session = &status["session"];
    let Some(id) = session["session_id"].as_str() else {
        return;
    };
    lesson.session_id = Some(id.to_owned());
    lesson.revision = session["revision"].as_i64();
    lesson.goal = session["goal"].as_str().map(str::to_owned);
    let checkpoint = &session["checkpoint"];
    if let Ok(steps) = serde_json::from_value(checkpoint["goal_steps"].clone()) {
        lesson.steps = steps;
    }
    lesson.task = task_from_checkpoint(checkpoint);
    if lesson.task.is_none() {
        facts.stopped_at = checkpoint["next_action"]
            .as_str()
            .map(str::to_owned)
            .filter(|text| !text.trim().is_empty());
    }
    facts.full = true;
}

/// One turn. Returns true when Claude Code ended under it.
fn turn(shared: &Arc<Shared>, text: &str) -> bool {
    let (text, talk) = match text.strip_prefix(TALK_MARK) {
        Some(words) => (words, true),
        None => (text, false),
    };
    let (words, from_window) = match text.strip_prefix(LESSON_MARK) {
        Some(words) => (words.trim().to_owned(), true),
        None => (text.trim().to_owned(), false),
    };
    let turn_no = {
        let mut state = lock(&shared.state);
        state.turn += 1;
        state.turn
    };
    // Unique across processes too: a resumed conversation counts its turns
    // from one again, and the runtime keeps one capture per turn id.
    let prompt_id = format!("{}-{turn_no}-{}", shared.session, shared.next_id());
    let Prepared {
        event,
        state: state_text,
        no_clock,
    } = prepare(shared, &words, from_window, talk, &prompt_id);
    {
        // An interrupt either lands before the message goes, and the turn
        // ends here, or after, and is sent to Claude Code.
        let _gate = lock(&shared.gate);
        if shared.interrupting.swap(false, Ordering::SeqCst) {
            drop(_gate);
            shared.note("interrupted before the message was sent: no record for this turn");
            shared.emit(AgentEvent::TurnEnded {
                ok: true,
                interrupted: true,
                error: None,
            });
            return false;
        }
        shared.phase.store(WRITING, Ordering::SeqCst);
        if let Err(error) = shared.send_user(&lesson_state::message(&state_text, &words)) {
            drop(_gate);
            shared.emit(AgentEvent::TurnEnded {
                ok: false,
                interrupted: false,
                error: Some(error.0),
            });
            return false;
        }
    }
    let said = listen(shared, true);
    let reply = said.cutter.reply();
    if !reply.is_empty() {
        shared.emit(AgentEvent::TextDone {
            text: reply.clone(),
        });
    }
    let shown = SystemTime::now();
    // The reply ended. It was interrupted only if the interrupt reached
    // Claude Code and the reply did not finish anyway.
    let sent = {
        let _gate = lock(&shared.gate);
        shared.phase.store(SAVING, Ordering::SeqCst);
        shared.interrupting.store(false, Ordering::SeqCst);
        shared.interrupt_sent.swap(false, Ordering::SeqCst)
    };
    let finished = said
        .result
        .as_ref()
        .is_some_and(|result| result["subtype"].as_str() == Some("success"));
    if said.ended {
        let interrupted = sent;
        let expected = shared.closing.load(Ordering::SeqCst);
        shared.emit(AgentEvent::TurnEnded {
            ok: expected || interrupted,
            interrupted,
            error: (!expected && !interrupted)
                .then(|| "The tutor stopped before it finished.".into()),
        });
        return true;
    }
    // An interrupted reply gets no second request and no mark: the open
    // task stays open. Claude Code runs no Stop hook after an interrupt, so
    // a plugin lesson keeps no reply for it either.
    if sent && !finished {
        shared.note("interrupted: no record for this turn");
        // What was shown before the stop may have marked the open task, or
        // posed another: the engine cannot know, so no task is kept, and
        // the next answer is never saved against the old question. A stop
        // before the first word showed nothing new, and keeps the task.
        if !reply.is_empty() {
            let forgot = forget_open_task(
                shared,
                &reply,
                "The reply was stopped part-way, so the task that was open is set aside.",
                STOPPED,
            );
            if forgot.is_some_and(|task| task.redo.is_some()) {
                shared.emit(AgentEvent::Notice {
                    kind: NoticeKind::Turn,
                    text: REDO_SET_ASIDE.into(),
                });
            }
            // A block cut short that had begun to name a finished repair:
            // nothing is recorded, and both are told.
            let block: String = said
                .cutter
                .block()
                .unwrap_or_default()
                .split_whitespace()
                .collect();
            if block.contains("\"repair\":{") {
                repair_lost(
                    shared,
                    "Your last reply was stopped, so the repair its record named was not recorded. Its paper question still waits for a repair.",
                    REPAIR_NOT_SAVED,
                );
            }
        }
        tell_task(shared, false);
        shared.emit(AgentEvent::TurnEnded {
            ok: true,
            interrupted: true,
            error: None,
        });
        return false;
    }
    let result = said.result.unwrap_or_default();
    let failed = result["is_error"].as_bool().unwrap_or(false)
        || result["subtype"].as_str().is_some_and(|s| s != "success");
    if failed {
        let why = claude::what_went_wrong(&result);
        let error = if claude::looks_signed_out(&why) {
            shared.emit(AgentEvent::Notice {
                kind: NoticeKind::SignIn,
                text: claude::SIGN_IN.into(),
            });
            None
        } else if claude::looks_like_a_limit(&why) {
            shared.emit(AgentEvent::Notice {
                kind: NoticeKind::UsageLimit,
                text: claude::limit_words(&why),
            });
            None
        } else if why.trim().is_empty() {
            Some("The reply ended with an error. Send your message again.".to_owned())
        } else {
            Some(claude::clip(&why, 300))
        };
        shared.emit(AgentEvent::TurnEnded {
            ok: false,
            interrupted: false,
            error,
        });
        return false;
    }
    // The reply is whole, so the turn ends for the learner now; its record
    // is kept behind it, within one bounded budget. A message sent
    // meanwhile waits for it, the hidden second request included.
    shared.emit(AgentEvent::TurnEnded {
        ok: true,
        interrupted: false,
        error: None,
    });
    let deadline = Instant::now() + SAVE_BUDGET;
    let mut ended = false;
    let record = match record::parse(said.cutter.block().unwrap_or_default()) {
        Ok(record) => Some(record),
        Err(why) => {
            shared.note(&format!("record block invalid ({why}); asking again"));
            let (record, closed) = ask_again(shared, &why);
            ended = closed;
            record
        }
    };
    let halted = lock(&shared.state).halt.is_some();
    // A control the learner asked for is made as the plugin engine makes it,
    // and the turn that asked for it records nothing more.
    if let Some(control) = record.as_ref().and_then(|record| record.control) {
        apply_control(shared, control, &words, event.as_deref());
        tell_task(shared, false);
        // The turn that asks for a control records nothing more, a repair
        // included: both are told.
        if let Some(note) = record.as_ref().and_then(|record| record.repair.as_ref()) {
            shared.note(&format!(
                "repair {} not recorded: the block also asked for a control",
                note.number
            ));
            repair_lost(
                shared,
                &format!(
                    "Repair {} was not recorded: the same reply asked to pause, stop or change recording, and records nothing more. Its paper question still waits for a repair.",
                    note.number
                ),
                REPAIR_NOT_SAVED,
            );
        }
        return ended;
    }
    if halted {
        // Nothing of this turn reaches the record, and an answer with no
        // mark is what the learner asked for.
        shared
            .note("recording is off, or the lesson is paused or stopped: no record for this turn");
        tell_task(
            shared,
            record.as_ref().is_some_and(|record| record.task.is_some()),
        );
        return ended;
    }
    let message = Message {
        words: words.clone(),
        event_id: event,
        no_clock,
        talk,
    };
    match &record {
        Some(record) => {
            let saved = save(shared, record, &message, &reply, deadline, shown);
            // A repair is recorded only behind a turn the record holds, and
            // never while recording is off or the lesson is paused or
            // stopped: the save may have found it so.
            let halted = lock(&shared.state).halt.is_some();
            match (&record.repair, saved, halted) {
                (_, _, true) => {}
                (Some(note), true, false) => repair(shared, note, deadline),
                (Some(note), false, false) => {
                    shared.note(&format!(
                        "repair {} not recorded: the turn's record was not kept",
                        note.number
                    ));
                    repair_lost(
                        shared,
                        &format!(
                            "Repair {} was not recorded: the record could not keep your last reply. Its paper question still waits for a repair.",
                            note.number
                        ),
        REPAIR_NOT_SAVED,
    );
                }
                (None, _, false) if record.repair_unreadable => {
                    shared.note("the block named a repair that could not be read");
                    repair_lost(
                        shared,
                        "Your block named a repair, but without its number or a line on what was done, so nothing was recorded: its paper question still waits for a repair.",
        REPAIR_NOT_SAVED,
    );
                }
                _ => {}
            }
        }
        None => {
            // The reply may have posed a new task, which the engine cannot
            // know: the task in hand is forgotten, so the next answer is
            // never saved against the wrong question.
            let open = forget_open_task(
                shared,
                &reply,
                "A reply's record could not be read, so the task that was open is set aside.",
                UNREAD,
            )
            .is_some();
            shared.note("no valid record after asking again: this turn has no mark, and the open task is forgotten");
            if open {
                shared.emit(AgentEvent::Notice {
                    kind: NoticeKind::Turn,
                    text: NOT_MARKED.into(),
                });
            }
            // The reply may have finished a repair: none is recorded.
            if !lock(&shared.state).lesson.repairs.is_empty() {
                shared.emit(AgentEvent::Notice {
                    kind: NoticeKind::Turn,
                    text: REPAIR_NOT_SAVED.into(),
                });
            }
        }
    }
    tell_task(
        shared,
        record.as_ref().is_some_and(|record| record.task.is_some()),
    );
    // The reply as the learner saw it, for the record.
    if let Err(why) = shared.host.hook(
        "stop",
        "Stop",
        json!({"prompt_id": prompt_id, "stop_hook_active": false, "last_assistant_message": reply}),
    ) {
        shared.note(&format!("stop hook failed: {why}"));
    }
    ended
}

/// Tell the window which task is open now, and whether this turn's reply
/// posed it, so the field for an answer can sit inside it. The words are the
/// tutor's own, from its record block; nothing here marks anything.
fn tell_task(shared: &Arc<Shared>, posed: bool) {
    let task = lock(&shared.state)
        .lesson
        .task
        .as_ref()
        .map(|task| task.text.clone());
    shared.emit(AgentEvent::OpenTask { task, posed });
}

/// The learner's notice for a repair that may have been finished and was
/// not recorded.
const REPAIR_NOT_SAVED: &str = "If the tutor finished a repair just now, it was not saved: that paper question still waits for its repair.";

/// A repair the tutor finished and the record does not hold: the learner is
/// told now, and the tutor, with `why`, on the next turn.
fn repair_lost(shared: &Arc<Shared>, why: &str, notice: &str) {
    lock(&shared.state).repair_outcome = Some(why.to_owned());
    shared.emit(AgentEvent::Notice {
        kind: NoticeKind::Turn,
        text: notice.into(),
    });
}

/// The learner's notice for a repair whose save the record could not
/// confirm, even asked again.
const REPAIR_NOT_KNOWN: &str = "Your record did not answer in time, so the repair may not be saved. If it was not, that paper question still waits for its repair.";

/// Record the repair the tutor finished, by `deadline`: for each of its rows
/// the record says, just now, is waiting for one, never another, and then
/// read back what the record made of each. The tutor is told on the next
/// turn; the learner now when nothing could be recorded.
fn repair(shared: &Arc<Shared>, note: &record::RepairNote, deadline: Instant) {
    let Some(waiting) = lock(&shared.state)
        .lesson
        .repairs
        .iter()
        .find(|repair| repair.number == note.number)
        .cloned()
    else {
        shared.note(&format!(
            "the tutor named repair {}, which is not listed",
            note.number
        ));
        repair_lost(
            shared,
            &format!(
                "No Repair {} is listed, so nothing was recorded. Name a repair only by a number the state lists; a repair that still waits is listed again.",
                note.number
            ),
            REPAIR_NOT_SAVED,
        );
        return;
    };
    // What waits now, as the record says: a repair on a row whose redo is
    // already set would move that redo.
    let status = shared.host.call_by(
        "context",
        json!({"schema": "learner-model/v2", "action": "status"}),
        deadline,
    );
    let Some(now) = status
        .ok()
        .and_then(|status| status.get("repairs_waiting").cloned())
        .and_then(|rows| rows.as_array().cloned())
    else {
        shared.note("the rows waiting for a repair could not be read: no repair recorded");
        repair_lost(
            shared,
            &format!(
                "Repair {} was not recorded: the record could not say which rows wait for it. Its paper question still waits for a repair.",
                note.number
            ),
        REPAIR_NOT_SAVED,
    );
        return;
    };
    let rows: Vec<String> = waiting
        .rows
        .iter()
        .filter(|row| {
            now.iter()
                .any(|listed| listed["item"].as_str() == Some(row.as_str()))
        })
        .cloned()
        .collect();
    if rows.is_empty() {
        shared.note(&format!(
            "repair {}: no row of it waits any more",
            note.number
        ));
        lock(&shared.state)
            .lesson
            .repairs
            .retain(|repair| repair.number != note.number);
        repair_lost(
            shared,
            &format!(
                "The record holds no row of Repair {} as waiting any more, so nothing was recorded for it.",
                note.number
            ),
            REPAIR_NOT_SAVED,
        );
        return;
    }
    let change = |next: bool| -> Vec<Value> {
        rows.iter()
            .map(|row| {
                let mut change = json!({"item": row, "repaired": note.done});
                if let (true, Some(n)) = (next, &note.next) {
                    change["next"] = json!({"question": n.question, "answer_key": n.key});
                }
                change
            })
            .collect()
    };
    let request_id = format!("{}-repair-{}", shared.session, shared.next_id());
    let mut request = json!({
        "schema": "learner-model/v2",
        "action": "update",
        "request_id": request_id,
        "practice": change(true),
    });
    let mut outcome = shared
        .host
        .call_by("learning_step", request.clone(), deadline);
    // A next question the runtime will not store never costs the repair.
    if let Err(why) = &outcome {
        if !host_client::unknown(why) && why.contains(".next") && note.next.is_some() {
            request["practice"] = json!(change(false));
            request["request_id"] = json!(format!("{request_id}-again"));
            outcome = shared
                .host
                .call_by("learning_step", request.clone(), deadline);
        }
    }
    // Unknown: ask again with the same request, which the record answers
    // with its first result if it kept it, and keeps once if it did not.
    if outcome.as_ref().is_err_and(|why| host_client::unknown(why)) {
        outcome = shared
            .host
            .call_by("learning_step", request, Instant::now() + READ_BACK);
    }
    let result = match outcome {
        Ok(result) => result,
        Err(why) => {
            shared.note(&format!("repair {} not recorded: {why}", note.number));
            if why.contains("recording_disabled") {
                lock(&shared.state).halt = Some(Halt::RecordingOff);
                return;
            }
            // Unknown even asked again: it may have been kept.
            if host_client::unknown(&why) {
                repair_lost(
                    shared,
                    &format!(
                        "The record did not answer in time, so Repair {} may not have been recorded. Say nothing of when its question comes back; the state lists it again if it still waits.",
                        note.number
                    ),
                    REPAIR_NOT_KNOWN,
                );
                return;
            }
            repair_lost(
                shared,
                &format!(
                    "Repair {} was not recorded: the record refused it. Its paper question still waits for a repair.",
                    note.number
                ),
                REPAIR_NOT_SAVED,
            );
            return;
        }
    };
    // What the record made of each row: scheduled, or still waiting.
    let items = result["practice"]["items"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut scheduled: Vec<(String, String)> = Vec::new();
    for row in &rows {
        // The result names a row in a plan as "<plan>#<n>", as it was
        // sent, and a row in no plan, sent by its id, by its label.
        if let Some(next) = items
            .iter()
            .find(|item| {
                let name = item["item"].as_str();
                name == Some(row.as_str())
                    || (!row.contains('#') && name == Some(waiting.tag.as_str()))
            })
            .and_then(|item| item["next_redo"].as_str())
        {
            scheduled.push((row.clone(), next.to_owned()));
        }
    }
    let mut state = lock(&shared.state);
    if let Some(repair) = state
        .lesson
        .repairs
        .iter_mut()
        .find(|repair| repair.number == note.number)
    {
        repair
            .rows
            .retain(|row| !scheduled.iter().any(|(done, _)| done == row));
    }
    state
        .lesson
        .repairs
        .retain(|repair| !repair.rows.is_empty());
    let dates: Vec<&str> = scheduled.iter().map(|(_, date)| date.as_str()).collect();
    state.repair_outcome = Some(match dates.first() {
        Some(first) if dates.len() == rows.len() => format!(
            "The record holds Repair {} as done: its paper question comes back as a redo on {first}. You may tell the learner so.",
            note.number
        ),
        Some(first) => format!(
            "The record holds Repair {} as done for some of its paper questions, which come back as a redo on {first}; the others still wait for a repair.",
            note.number
        ),
        None => format!(
            "The record did not schedule a redo for Repair {}: its paper question still waits for a repair.",
            note.number
        ),
    });
    if dates.is_empty() {
        drop(state);
        shared.note(&format!(
            "repair {} saved, but no row was scheduled: {result}",
            note.number
        ));
        shared.emit(AgentEvent::Notice {
            kind: NoticeKind::Turn,
            text: REPAIR_NOT_SAVED.into(),
        });
    }
}

/// The notice for an answer the record could not keep.
const NOT_MARKED: &str =
    "Your last answer was not marked: it could not be saved to your record. The lesson goes on.";

/// The notice for an answer whose save the record could not confirm, even
/// asked again.
const NOT_KNOWN: &str = "Your record did not answer in time, so your last answer may not be marked. The lesson goes on.";

/// The most time a turn's record may take after its reply, all calls
/// together: the next message waits for it.
const SAVE_BUDGET: Duration = Duration::from_secs(20);
/// The time, beyond that budget, to ask the record whether a save whose
/// outcome is unknown went through.
const READ_BACK: Duration = Duration::from_secs(10);

/// The hidden second request, for a reply whose record block was missing
/// or could not be read. The learner sees nothing of it.
fn ask_again(shared: &Arc<Shared>, why: &str) -> (Option<Record>, bool) {
    let request = format!(
        "<lesson-state>\nYour last reply's record block could not be read: {why}. Write only the record block for that reply now, with nothing before or after it. The learner does not see this message.\n</lesson-state>"
    );
    if shared.send_user(&request).is_err() {
        return (None, false);
    }
    let said = listen(shared, false);
    let text = match said.cutter.block() {
        Some(block) => block.to_owned(),
        None => said.cutter.reply(),
    };
    match record::parse(&text) {
        Ok(record) => (Some(record), said.ended),
        Err(why) => {
            shared.note(&format!("record block invalid again ({why})"));
            (None, said.ended)
        }
    }
}

/// Whether a refusal says the turn is already in the record: a request
/// whose outcome was unknown had committed after all.
fn already_saved(why: &str) -> bool {
    why.contains("events_already_processed") || why.contains("already processed")
}

/// Forget the open task, because the engine no longer knows what is on
/// screen: `reply` was shown and has no record. The paper redos it showed
/// are not posed again in the sitting; a quick check that was open goes
/// back on the list, since it is still due. The tutor is told `why` with
/// the next message. When a task was open, the record's checkpoint is
/// rewritten without it, so that a conversation resumed later does not
/// read it back and save an answer against it. Returns the task that was
/// open.
fn forget_open_task(
    shared: &Arc<Shared>,
    reply: &str,
    note: &str,
    why: &str,
) -> Option<record::OpenTask> {
    let mut state = lock(&shared.state);
    state.lesson.spend_shown(reply);
    state.redo_clock = None;
    let (task, back) = state.lesson.forget_task();
    state.no_task = Some(match &back {
        Some(check) => format!(
            "{why} Quick check {} is still due and was not marked: pose it again, as \"Quick check {}:\", when you go on.",
            check.number, check.number
        ),
        None => why.to_owned(),
    });
    // With recording off, or the lesson paused or stopped, nothing goes to
    // the record, the checkpoint included.
    if task.is_none() || state.halt.is_some() {
        return task;
    }
    let cleared = Record {
        note: note.to_owned(),
        ..Record::default()
    };
    for attempt in 0..2 {
        let request_id = format!("{}-forget-{}", shared.session, shared.next_id());
        let write = state
            .lesson
            .apply(&cleared, &Message::default(), "", &request_id);
        if write.request.get("checkpoint").is_none() {
            break;
        }
        match shared.host.call("learning_step", write.request) {
            Ok(result) => {
                if let Some(session) = state.lesson.session_id.clone() {
                    if let Some(revision) = result["revisions"][&session].as_i64() {
                        state.lesson.revision = Some(revision);
                    }
                }
                break;
            }
            // Another conversation moved the lesson on: read its revision
            // and write once more.
            Err(why) if attempt == 0 && why.contains("revision") => {
                if let Ok(status) = shared.host.call(
                    "context",
                    json!({"schema": "learner-model/v2", "action": "status"}),
                ) {
                    state.lesson.revision = status["session"]["revision"].as_i64();
                }
            }
            Err(why) => {
                shared.note(&format!(
                    "the open task is still in the record's checkpoint: {why}"
                ));
                break;
            }
        }
    }
    task
}

/// Keep the turn's record in the runtime, by `deadline`. `shown` is when
/// the window showed the reply whole. Returns whether the record holds it.
fn save(
    shared: &Arc<Shared>,
    record: &Record,
    message: &Message,
    reply: &str,
    deadline: Instant,
    shown: SystemTime,
) -> bool {
    let mut state = lock(&shared.state);
    let request_id = format!("{}-turn-{}", shared.session, shared.next_id());
    let write = state.lesson.apply(record, message, reply, &request_id);
    // A timed redo's clock starts with the reply that posed it, and runs
    // until the redo is no longer the open task.
    let timed = state
        .lesson
        .task
        .as_ref()
        .and_then(|task| task.redo.as_ref())
        .filter(|redo| redo.timed)
        .map(|redo| redo.rows.clone());
    match timed {
        Some(rows) => {
            if state
                .redo_clock
                .as_ref()
                .is_none_or(|(open, _)| *open != rows)
            {
                state.redo_clock = Some((rows, shown));
            }
        }
        None => state.redo_clock = None,
    }
    // A task that looks like a paper redo and is none for the record.
    if let Some(why) = write.not_a_redo {
        shared.emit(AgentEvent::Notice {
            kind: NoticeKind::Turn,
            text: match why {
                NotARedo::Unlisted => REDO_UNLISTED,
                NotARedo::SetAside => REDO_SET_ASIDE,
            }
            .into(),
        });
    }
    let mut redo_answer = write.redo.clone();
    // An answer the record cannot keep is never lost in silence.
    let lost = |why: &str| {
        shared.note(&format!("an answer was not marked: {why}"));
        shared.emit(AgentEvent::Notice {
            kind: NoticeKind::Turn,
            text: NOT_MARKED.into(),
        });
    };
    if let Some(why) = write.lost {
        lost(why);
    }
    let mut request = write.request;
    // A misunderstanding found or cleared revises the learner's memory of
    // that skill, in the same request as the observation it rests on.
    // The tutor names the memory it revises by the short id the state
    // gave it; without one, a new memory is made. Code never guesses.
    if let (Some(note), true) = (&record.memory, request.get("observations").is_some()) {
        let memory_id = note.id.as_deref().and_then(|short| {
            let found = state.memories.iter().find(|(id, _)| id == short).map(|(_, id)| id.clone());
            if found.is_none() {
                shared.note(&format!("the tutor named memory {short}, which it was never shown: a new memory is made"));
            }
            found
        });
        // The memory as it is now, read whole, with its revision.
        let current = memory_id.and_then(|id| {
            shared
                .host
                .call_by(
                    "memory",
                    json!({"schema": "learner-model/v2", "action": "read", "refs": [{"kind": "current_memory", "id": id}]}),
                    deadline,
                )
                .ok()
                .and_then(|read| read["records"].get(0).cloned())
        });
        let goal = state.lesson.goal.clone().unwrap_or_default();
        match (note.id.is_some(), &current) {
            // Named, and could not be read: no edit rather than a new copy.
            (true, None) if state.memories.iter().any(|(id, _)| Some(id) == note.id.as_ref()) => {
                shared.note("the named memory could not be read: no memory edit");
            }
            _ => match record::memory_edit(note, current.as_ref(), &goal) {
                Some(edit) => request["memory_edits"] = json!([edit]),
                None => shared.note(
                    "the memory's description is at the runtime's limit: no memory edit, and the memory is as it was",
                ),
            },
        }
    }
    let worth = request.get("observations").is_some()
        || request.get("checkpoint").is_some()
        || request.get("event_dispositions").is_some()
        || request.get("practice").is_some();
    if !worth {
        return true;
    }
    // A call whose outcome is unknown is replayed as it was by the host
    // client. A refusal is answered once, changed. `sent` is the request
    // whose outcome `outcome` is.
    let mut sent = request.clone();
    let mut outcome = shared
        .host
        .call_by("learning_step", request.clone(), deadline);
    if let Err(why) = outcome.as_ref().map(|_| ()) {
        let why = why.clone();
        shared.note(&format!("learning_step refused: {why}"));
        if !already_saved(&why) && !host_client::unknown(&why) {
            // Without the memory edit, so that a memory the runtime refuses
            // never costs the mark; not cold, if the runtime refused cold;
            // and with the session's revision read again, if another
            // conversation moved it on.
            let mut retry = request.clone();
            if let Some(edits) = retry.as_object_mut() {
                edits.remove("memory_edits");
            }
            if why.contains("cold") {
                for observation in retry["observations"].as_array_mut().into_iter().flatten() {
                    if observation.get("attempt").is_some() {
                        observation["attempt"]["cold"] = json!(false);
                    }
                }
            }
            // The runtime names the field it refuses. A next question it
            // will not store never costs the mark. Nor does a redo: rows it
            // no longer knows are left out, and the answer is kept as an
            // ordinary one, not cold; a tag it will not take as a label
            // gives way to the skill the tutor named.
            let rows_refused = why.contains(".attempt.counts_for");
            let next_refused =
                why.contains(".attempt.next:") || why.contains("practice[") || rows_refused;
            if next_refused {
                if let Some(fields) = retry.as_object_mut() {
                    fields.remove("practice");
                }
            }
            for observation in retry["observations"].as_array_mut().into_iter().flatten() {
                let Some(attempt) = observation
                    .get_mut("attempt")
                    .and_then(Value::as_object_mut)
                else {
                    continue;
                };
                if next_refused {
                    attempt.remove("next");
                }
                if rows_refused {
                    attempt.remove("counts_for");
                    attempt.remove("slow");
                    attempt.insert("cold".into(), json!(false));
                }
                if let (Some(redo), true) = (&redo_answer, why.contains(".attempt.label:")) {
                    attempt.insert("label".into(), json!(redo.other_label));
                }
            }
            if rows_refused {
                if let Some(redo) = redo_answer.take() {
                    shared.note(
                        "the redo's rows were refused: the answer is kept as an ordinary one",
                    );
                    if redo.right {
                        shared.emit(AgentEvent::Notice {
                            kind: NoticeKind::Turn,
                            text: REDO_NO_COUNT.into(),
                        });
                    }
                }
            }
            if why.contains("revision") {
                if let Ok(status) = shared.host.call_by(
                    "context",
                    json!({"schema": "learner-model/v2", "action": "status"}),
                    deadline,
                ) {
                    if let Some(revision) = status["session"]["revision"].as_i64() {
                        retry["session"]["expected_revision"] = json!(revision);
                    }
                }
            }
            retry["request_id"] = json!(format!("{request_id}-again"));
            sent = retry.clone();
            outcome = shared.host.call_by("learning_step", retry, deadline);
        }
    }
    // Still unknown: the save may have gone through. Before the learner is
    // told it did not, the runtime is asked with the same request again. It
    // answers a request it has kept with its first result, and keeps one it
    // has not, once: one answer is never two attempts.
    if outcome.as_ref().is_err_and(|why| host_client::unknown(why)) {
        shared.note("the save's outcome is unknown: asking the record");
        outcome = shared
            .host
            .call_by("learning_step", sent, Instant::now() + READ_BACK);
    }
    match outcome {
        Ok(result) => {
            if let Some(session) = state.lesson.session_id.clone() {
                if let Some(revision) = result["revisions"][&session].as_i64() {
                    state.lesson.revision = Some(revision);
                }
            }
            if let Some(levels) = result.get("help_levels") {
                state.help = lesson_state::help_lines(levels);
            }
            if let Some(labels) = result["quick_checks"]["open_labels"].as_array() {
                state.labels = labels
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect();
            }
            // A sitting that began with this message: the next turn's
            // state lists what it opens with, unless this is the turn in
            // which the learner chose to go on without it.
            if let Some(due) = due_of(&result) {
                if state.skip_turn != Some(state.turn) {
                    state.late = Some(due);
                }
            }
            // What the record made of the answer to a paper redo or a due
            // quick check. The tutor is told next turn, and the learner now
            // when the tutor marked it right and the record holds no pass.
            let effects: Vec<&str> = result["practice"]["attempts"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|attempt| attempt["observation"] == "answer")
                .flat_map(|attempt| attempt["items"].as_array().into_iter().flatten())
                .filter_map(|item| item["effect"].as_str())
                .collect();
            let passed = effects.iter().any(|e| matches!(*e, "passed" | "closed"));
            let failed = effects.contains(&"failed");
            let declined = effects.contains(&"declined");
            if let Some(right) = write.check {
                state.outcome = Some(
                    if passed {
                        "The record holds the last quick check as a pass."
                    } else if failed {
                        "The record holds the last quick check as a miss: it comes back the next day."
                    } else if declined {
                        "The record holds the last quick check as declined: it stays due."
                    } else if effects.contains(&"helped") {
                        "The record holds the last quick check as answered with help: no pass, and it stays due."
                    } else {
                        "The record did not count the last quick check: no pass and no fail."
                    }
                    .to_owned(),
                );
                if right && !passed {
                    shared.note(&format!("a right check answer earned no pass: {effects:?}"));
                    shared.emit(AgentEvent::Notice {
                        kind: NoticeKind::Turn,
                        text: CHECK_NO_PASS.into(),
                    });
                }
            }
            if let Some(redo) = &redo_answer {
                state.outcome = Some(
                    if passed {
                        "The record holds the last paper redo as a pass."
                    } else if failed {
                        "The record holds the last paper redo as a fail: its question waits for a repair."
                    } else if declined {
                        "The record holds the last paper redo as declined: it stays due."
                    } else {
                        "The record did not count the last paper redo: no pass and no fail."
                    }
                    .to_owned(),
                );
                // Its failed rows now wait for a repair. Which wait, the
                // record says: its list names the rows as `counts_for` did,
                // a row in no plan by its id, where the save's result names
                // it by its label.
                if failed {
                    let waiting = shared
                        .host
                        .call_by(
                            "context",
                            json!({"schema": "learner-model/v2", "action": "status"}),
                            deadline,
                        )
                        .ok()
                        .and_then(|status| status.get("repairs_waiting").cloned());
                    let rows: Vec<Value> = waiting
                        .as_ref()
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter(|row| {
                            row["item"]
                                .as_str()
                                .is_some_and(|item| redo.rows.iter().any(|r| r == item))
                        })
                        .map(|row| {
                            let mut row = row.clone();
                            row["tag"] = json!(redo.tag);
                            row
                        })
                        .collect();
                    if rows.is_empty() {
                        shared.note("a failed redo's rows were not listed as waiting for a repair");
                    }
                    let made = state.lesson.take_waiting(&json!(rows), true);
                    state.repairs_new.extend(made);
                }
                if redo.right && !passed {
                    shared.note(&format!("a right redo answer earned no pass: {effects:?}"));
                    shared.emit(AgentEvent::Notice {
                        kind: NoticeKind::Turn,
                        text: if failed { REDO_FAILED } else { REDO_NO_COUNT }.into(),
                    });
                }
            }
            if let (Some(id), Some(note)) =
                (result["references"]["memory"].as_str(), &record.memory)
            {
                let short = short_id(&mut state, id);
                let line = format!("{short}: {}: new, as you noted", note.skill);
                state.memories_new.push(line);
            }
            true
        }
        Err(why) if already_saved(&why) => {
            // It committed: only the session's revision is not known. The
            // next write reads it again if it is refused for it.
            shared.note("the turn was already in the record");
            true
        }
        // The record could not say, even asked again: the learner is told
        // the answer may not be marked.
        Err(why) if host_client::unknown(&why) => {
            shared.note(&format!("the save's outcome is still unknown: {why}"));
            if write.marked {
                shared.emit(AgentEvent::Notice {
                    kind: NoticeKind::Turn,
                    text: NOT_KNOWN.into(),
                });
            }
            false
        }
        Err(why) => {
            shared.note(&format!("the record was not saved: {why}"));
            // Recording off, or a lesson paused or stopped elsewhere: the
            // runtime keeps nothing, as the learner asked.
            if why.contains("recording_disabled") {
                state.halt = Some(Halt::RecordingOff);
                return false;
            }
            if why.contains("learning_inactive") {
                state.halt = Some(Halt::Stopped);
                return false;
            }
            if write.marked {
                lost(&why);
            }
            false
        }
    }
}

/// Make a control the learner asked for, as the plugin engine makes it: a
/// `learning_step` `action:"control"` on the message that asked for it.
fn apply_control(shared: &Arc<Shared>, control: Control, words: &str, event: Option<&str>) {
    let mut request = json!({
        "schema": "learner-model/v2",
        "action": "control",
        "request_id": format!("{}-control-{}", shared.session, shared.next_id()),
        "control": control.word(),
    });
    match event {
        Some(event) => request["turn_ref"] = json!(event),
        None => {
            request["reported_event"] = json!({
                "external_event_key": format!("{}-said-{}", shared.session, shared.next_id()),
                "speaker": "learner",
                "kind": "message",
                "text": words,
            })
        }
    }
    let outcome = shared.host.call("learning_step", request);
    if let Err(why) = &outcome {
        shared.note(&format!("control {} refused: {why}", control.word()));
    }
    let remember = |off: bool| {
        if let Some(store) = &shared.store {
            store.update(&shared.root, |state| state.recording_off = off);
        }
    };
    let mut state = lock(&shared.state);
    match control {
        // The engine stops sending whatever the runtime answered: a stop the
        // learner asked for holds in the window even if the record's failed.
        Control::RecordingOff => {
            state.halt = Some(Halt::RecordingOff);
            remember(true);
        }
        Control::Pause => state.halt = Some(Halt::Paused),
        Control::Stop => state.halt = Some(Halt::Stopped),
        // If the record still has recording off for this conversation, the
        // next prompt hook says so, and the engine stops again.
        Control::RecordingOn => {
            state.halt = None;
            remember(false);
        }
    }
    drop(state);
    // While recording was off, replies may have set tasks the engine never
    // read: when it comes back, no task is open, in the checkpoint too.
    if control == Control::RecordingOn {
        forget_open_task(
            shared,
            "",
            "Recording came back on, so the task that was open before it went off is set aside.",
            RECORDING_BACK,
        );
    }
    if outcome.is_err() && control != Control::RecordingOn {
        shared.emit(AgentEvent::Notice {
            kind: NoticeKind::Other,
            text: "Your record could not confirm this. The window sends nothing more to it from this lesson.".into(),
        });
    }
}

struct DirectConversation {
    shared: Arc<Shared>,
}

impl Conversation for DirectConversation {
    fn send_with(&mut self, text: &str, intent: Option<Intent>) -> AgentResult<()> {
        match intent {
            Some(Intent::Talk) => self.send(&format!("{TALK_MARK}{text}")),
            // An answer is marked as any answer is.
            Some(Intent::Answer) | None => self.send(text),
        }
    }

    fn send(&mut self, text: &str) -> AgentResult<()> {
        if lock(&self.shared.stdin).is_none() {
            return Err(AgentError("The tutor is not running.".into()));
        }
        // An interrupt marked after the last turn ended belongs to none.
        if !self.shared.busy() {
            self.shared.interrupting.store(false, Ordering::SeqCst);
        }
        self.shared.push(Item::Words(text.to_owned()));
        Ok(())
    }

    fn interrupt(&mut self) -> AgentResult<()> {
        // Words waiting their turn are dropped, as Claude Code drops a
        // queued message on an interrupt.
        lock(&self.shared.queue).retain(|item| matches!(item, Item::Ended));
        // Decided under the gate, so the turn cannot change phase meanwhile.
        let _gate = lock(&self.shared.gate);
        match self.shared.phase.load(Ordering::SeqCst) {
            PREPARING => {
                self.shared.interrupting.store(true, Ordering::SeqCst);
                Ok(())
            }
            WRITING => {
                let id = self.shared.next_id();
                self.shared.write(&json!({
                    "type": "control_request",
                    "request_id": id,
                    "request": {"subtype": "interrupt", "cancel_queued": true},
                }))?;
                self.shared.interrupt_sent.store(true, Ordering::SeqCst);
                let mine = self.shared.interrupts.fetch_add(1, Ordering::SeqCst) + 1;
                // The watchdog belongs to this turn: it never signals a later
                // one, however soon the learner writes again.
                let turn = self.shared.turn_seq.load(Ordering::SeqCst);
                let shared = Arc::clone(&self.shared);
                thread::spawn(move || {
                    for signal in ["INT", "TERM"] {
                        thread::sleep(Duration::from_secs(8));
                        let still = {
                            let _gate = lock(&shared.gate);
                            shared.interrupts.load(Ordering::SeqCst) == mine
                                && shared.turn_seq.load(Ordering::SeqCst) == turn
                                && shared.phase.load(Ordering::SeqCst) == WRITING
                        };
                        if !still {
                            return;
                        }
                        shared.signal(signal);
                    }
                });
                Ok(())
            }
            // The reply is whole and being saved, or nothing runs.
            _ => Ok(()),
        }
    }

    fn decide(&mut self, _id: &str, _decision: Decision) -> AgentResult<()> {
        Err(AgentError("That request is no longer waiting.".into()))
    }

    fn answer(&mut self, _id: &str, _answers: &[Answer]) -> AgentResult<()> {
        Err(AgentError("That question is no longer waiting.".into()))
    }

    fn wait_saved(&self, limit: Duration) -> bool {
        self.shared.wait_idle(limit)
    }

    fn saved_waiter(&self) -> Option<crate::agent::SavedWaiter> {
        let shared = Arc::clone(&self.shared);
        Some(Arc::new(move |limit| shared.wait_idle(limit)))
    }

    fn close(&mut self) {
        if self.shared.closing.swap(true, Ordering::SeqCst) {
            return;
        }
        if self.shared.phase.load(Ordering::SeqCst) == WRITING {
            let _ = self.interrupt();
        }
        lock(&self.shared.queue).retain(|item| matches!(item, Item::Ended));
        // A reply already whole keeps its record before the lesson ends,
        // within the save's budget: the end screen reads the record next.
        if self.shared.phase.load(Ordering::SeqCst) == SAVING
            && !self
                .shared
                .wait_idle(SAVE_BUDGET + READ_BACK + Duration::from_secs(2))
        {
            self.shared
                .note("the lesson ended before its last record was kept");
        }
        *lock(&self.shared.stdin) = None;
        let shared = Arc::clone(&self.shared);
        thread::spawn(move || {
            thread::sleep(Duration::from_secs(6));
            shared.signal("TERM");
            thread::sleep(Duration::from_secs(5));
            if let Some(child) = lock(&shared.child).as_mut() {
                let _ = child.kill();
            }
        });
    }
}

impl Drop for DirectConversation {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn a_course_graph_is_named_only_inside_the_study_root() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let root = base.join("Course");
        let study = root.join("Study");
        assert!(course_graphs(&root).is_empty(), "no Study folder yet");
        for plan in ["midterm-1", "final", "empty"] {
            std::fs::create_dir_all(study.join(plan)).unwrap();
        }
        std::fs::write(study.join("midterm-1/graph.yaml"), "tags:\n").unwrap();
        std::fs::write(study.join("final/graph.yaml"), "tags:\n").unwrap();
        // A graph.yaml that is a folder, and links that lead out of the root.
        std::fs::create_dir_all(study.join("odd/graph.yaml")).unwrap();
        let outside = base.join("elsewhere");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("graph.yaml"), "tags:\n").unwrap();
        std::os::unix::fs::symlink(&outside, study.join("linked")).unwrap();
        std::fs::create_dir_all(study.join("filelink")).unwrap();
        std::os::unix::fs::symlink(
            outside.join("graph.yaml"),
            study.join("filelink/graph.yaml"),
        )
        .unwrap();
        // A link that stays inside the root is followed.
        std::os::unix::fs::symlink(study.join("final"), study.join("alias")).unwrap();
        assert_eq!(
            course_graphs(&root),
            [
                "Study/alias/graph.yaml",
                "Study/final/graph.yaml",
                "Study/midterm-1/graph.yaml"
            ]
        );
        // A Study folder that is itself a link out of the root names nothing.
        let other = base.join("Other");
        std::fs::create_dir_all(&other).unwrap();
        std::os::unix::fs::symlink(&study, other.join("Study")).unwrap();
        assert!(course_graphs(&other).is_empty());
    }

    #[test]
    fn a_direct_lesson_loads_the_model_the_two_search_tools_and_the_plan_files_only() {
        let folder = Path::new("/study/stats");
        let args = direct_args(
            folder,
            Path::new("/study"),
            Path::new("/bin/magnesium"),
            "0.2.0",
            "You are the tutor.",
            "0b0e7c3a-1111-4222-8333-444455556666",
            false,
        );
        let after = |flag: &str| {
            args.iter()
                .position(|arg| arg == flag)
                .map(|at| args[at + 1].clone())
                .unwrap_or_else(|| panic!("{flag} is missing: {args:?}"))
        };
        assert_eq!(after("--model"), claude::MODEL);
        assert_eq!(after("--system-prompt"), "You are the tutor.");
        assert_eq!(after("--setting-sources"), "");
        assert_eq!(after("--tools"), "Read,Edit,Write");
        assert_eq!(after("--permission-prompts"), "none");
        assert_eq!(after("--add-dir"), "/study");
        assert_eq!(
            after("--disallowedTools"),
            "mcp__magnesium__context,mcp__magnesium__learning_step,mcp__magnesium__memory,\
             Edit(//study/.magnesium/**),Write(//study/.magnesium/**)"
        );
        assert_eq!(
            after("--allowedTools"),
            "Read(//study/**),\
             Edit(//study/Study/*/plan.md),Write(//study/Study/*/plan.md),\
             Edit(//study/Study/*/graph.yaml),Write(//study/Study/*/graph.yaml),\
             mcp__magnesium__search_sources,mcp__magnesium__sources",
            "Read reaches the study root, Edit and Write its plan files, and nothing else"
        );
        assert!(args.contains(&"--strict-mcp-config".to_owned()));
        assert!(args.contains(&"--disable-slash-commands".to_owned()));
        assert!(!args
            .iter()
            .any(|arg| arg == "--plugin-dir" || arg == "--effort"));
        let server: Value = serde_json::from_str(&after("--mcp-config")).unwrap();
        assert_eq!(
            server["mcpServers"]["magnesium"]["command"],
            "/bin/magnesium"
        );
        assert_eq!(
            args.last().unwrap(),
            "--session-id=0b0e7c3a-1111-4222-8333-444455556666"
        );
        // A resumed lesson in the study root itself.
        let resumed = direct_args(
            folder,
            folder,
            Path::new("/bin/magnesium"),
            "0.2.0",
            "p",
            "abc",
            true,
        );
        assert!(!resumed.contains(&"--add-dir".to_owned()));
        assert_eq!(resumed.last().unwrap(), "--resume=abc");
    }

    #[test]
    fn a_new_sitting_drops_the_redos_the_last_one_listed_and_did_not_pose() {
        let mut state = State {
            checks_asked: true,
            ..State::default()
        };
        state.lesson.redos =
            record::redos_from(&json!([{"item": "m#1", "plan": "m", "tag": "km"}]), &|_| {
                false
            });
        assert_eq!(state.lesson.redos.len(), 1);
        state.begin_sitting(true);
        assert!(state.lesson.redos.is_empty());
        assert!(!state.checks_asked);
    }

    #[test]
    fn a_new_sitting_drops_the_checks_the_last_one_listed_and_did_not_pose() {
        let mut state = State {
            checks_asked: true,
            ..State::default()
        };
        state.lesson.checks = ["sign of b", "reading a"]
            .into_iter()
            .zip(1..)
            .map(|(label, number)| record::DueCheck {
                number,
                label: label.into(),
                misses: vec![],
                question: None,
                key: None,
            })
            .collect();
        // Check 1 is posed; check 2 is listed and not posed.
        state.lesson.apply(
            &record::parse(r#"{"answer": null, "task": "What is b in x² − 5x?"}"#).unwrap(),
            &Message::default(),
            "Quick check 1: What is b in x² − 5x?",
            "r-1",
        );
        assert_eq!(state.lesson.checks.len(), 1);
        // Hours later a new sitting begins and brings no list, as when
        // nothing is due or another conversation took the opener. The
        // tutor remembers check 2 and poses it: it binds nothing.
        state.begin_sitting(true);
        assert!(state.lesson.checks.is_empty());
        state.lesson.apply(
            &record::parse(r#"{"answer": null, "task": "What is a in 3x² + x?"}"#).unwrap(),
            &Message::default(),
            "Quick check 2: What is a in 3x² + x?",
            "r-2",
        );
        assert_eq!(state.lesson.task.as_ref().unwrap().check, None);
    }

    #[test]
    fn a_resumed_conversation_with_nothing_kept_sends_nothing_cold_until_a_new_sitting() {
        let mut state = State::default();
        state.restore(None, "s", true);
        // The first message of the window is no known new sitting.
        state.begin_sitting(false);
        assert!(state.lesson.sitting_unknown);
        state.lesson.task = Some(record::OpenTask {
            text: "Solve x² = 4.".into(),
            help: record::Help::None,
            guess: false,
            check: None,
            redo: None,
            again: false,
            stuck: false,
            skill: String::new(),
        });
        let write = state.lesson.apply(
            &record::parse(r#"{"answer": "right", "skill": "roots", "cold": true}"#).unwrap(),
            &Message {
                words: "x = 2 or -2".into(),
                event_id: Some("e-1".into()),
                ..Message::default()
            },
            "Right.",
            "r-1",
        );
        assert_eq!(write.request["observations"][0]["attempt"]["cold"], false);
        state.begin_sitting(true);
        assert!(!state.lesson.sitting_unknown);
    }

    #[test]
    fn a_new_session_id_is_a_uuid() {
        let id = new_session_id();
        assert_eq!(id.len(), 36);
        assert!(claude::is_session_id(&id));
        assert_ne!(id, new_session_id());
    }

    /// Every double-quoted string in a page source file.
    fn quoted(source: &str) -> Vec<String> {
        source
            .split('"')
            .skip(1)
            .step_by(2)
            .map(str::to_owned)
            .collect()
    }

    /// The page owns the words its buttons and opening lines send. The
    /// engine's copies must be the same, or a change of wording on the page
    /// would quietly turn a hint into an answer.
    #[test]
    fn the_engine_knows_the_words_the_page_sends() {
        let pages = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src");
        let read = |name: &str| std::fs::read_to_string(pages.join(name)).unwrap();
        let session = read("surface/Session.tsx");
        let lesson = read("lesson.ts");
        let strings: Vec<String> = quoted(&session)
            .into_iter()
            .chain(quoted(&lesson))
            .collect();
        for words in [
            record::HINT_WORDS,
            record::DONT_KNOW_WORDS,
            record::LOST_WORDS,
        ] {
            assert!(
                strings.iter().any(|s| s == words),
                "the page no longer sends {words:?}: change record.rs and the tutor prompt with it"
            );
            assert!(
                PROMPT.contains(words),
                "the tutor prompt does not name {words:?}"
            );
        }
        // The opening lines, from openingWords in lesson.ts.
        for words in [BACK, NO_CHECKS, WORK_ON] {
            assert!(
                lesson.contains(words),
                "lesson.ts no longer opens a sitting with {words:?}: change direct.rs with it"
            );
        }
    }

    #[test]
    fn the_windows_opening_lines_are_known() {
        assert_eq!(opening(NO_CHECKS), Some((true, None)));
        assert_eq!(
            opening("I'm back. Let's continue. I stopped at: step 2"),
            Some((false, None))
        );
        assert_eq!(
            opening("Let's work on Stats 101."),
            Some((false, Some("Stats 101".into())))
        );
        assert_eq!(opening("Teach me Bayes"), None);
    }

    #[test]
    fn an_open_task_comes_back_from_the_checkpoint() {
        let mut lesson = Lesson {
            session_id: Some("s".into()),
            revision: Some(1),
            ..Lesson::default()
        };
        lesson.task = Some(record::OpenTask {
            text: "Solve x² = 4".into(),
            help: record::Help::Hint,
            guess: false,
            check: Some("sign of b".into()),
            redo: None,
            again: false,
            stuck: false,
            skill: String::new(),
        });
        let write = lesson.apply(
            &record::parse(r#"{"answer": null}"#).unwrap(),
            &Message::default(),
            "",
            "r",
        );
        let task = task_from_checkpoint(&write.request["checkpoint"]).unwrap();
        assert_eq!(task, lesson.task.clone().unwrap());
        // A paper redo comes back with its rows, its tag and its clock.
        lesson.task = Some(record::OpenTask {
            text: "The plot crosses at −0.5: what is Km?".into(),
            help: record::Help::None,
            guess: false,
            check: None,
            redo: Some(record::Redo {
                number: 2,
                rows: vec!["midterm-1#1".into(), "plan, with \"marks\"#2".into()],
                tag: "km".into(),
                timed: true,
            }),
            again: false,
            stuck: false,
            skill: String::new(),
        });
        let write = lesson.apply(
            &record::parse(r#"{"answer": null}"#).unwrap(),
            &Message::default(),
            "",
            "r-2",
        );
        let task = task_from_checkpoint(&write.request["checkpoint"]).unwrap();
        assert_eq!(task, lesson.task.unwrap());
        assert!(task_from_checkpoint(&json!({"plan_text": "Confirm the work."})).is_none());
    }
}
