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
use std::time::{Duration, Instant};

use chrono::Local;
use serde_json::{json, Value};

use crate::agent::{
    Account, AgentError, AgentEvent, AgentResult, Answer, Conversation, Decision, EventSink,
    Harness, NoticeKind, Payment, StartOptions,
};
use crate::claude::{self, ClaudeCode};
use crate::host_client::{self, HostClient};
use crate::lesson_state::{self, Facts};
use crate::paths;
use crate::record::{self, Cutter, DueCheck, Lesson, Message, Record};

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
/// The tools the model may use, all of them to look something up.
const LOOKUP_TOOLS: [&str; 3] = [
    "Read",
    "mcp__magnesium__search_sources",
    "mcp__magnesium__sources",
];
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
/// - `--tools Read` keeps one of Claude Code's tools. Magnesium's tool
///   server is the only one (`--strict-mcp-config`), and its three record
///   tools are disallowed, which takes their definitions out of the
///   model's context.
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
        "Read",
        "--strict-mcp-config",
        "--mcp-config",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    args.push(server.to_string());
    args.push("--disallowedTools".into());
    args.push(RECORD_TOOLS.join(","));
    args.push("--allowedTools".into());
    args.push(LOOKUP_TOOLS.join(","));
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
        }
    }

    /// Use these programs for Claude Code and the runtime. For tests.
    pub fn with_programs(claude: PathBuf, runtime: PathBuf) -> Self {
        Self {
            claude: ClaudeCode::with_program(claude),
            runtime: Some(runtime),
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
        let shared = Arc::new(Shared {
            sink,
            stdin: Mutex::new(stdin),
            child: Mutex::new(Some(child)),
            host,
            runtime,
            folder,
            session: session.clone(),
            resumed: resumed.is_some(),
            payment: options.payment,
            queue: Mutex::new(VecDeque::new()),
            wake: Condvar::new(),
            events: Mutex::new(events),
            phase: AtomicU8::new(IDLE),
            interrupting: AtomicBool::new(false),
            closing: AtomicBool::new(false),
            ready_sent: AtomicBool::new(false),
            interrupts: AtomicU64::new(0),
            counter: AtomicU64::new(0),
            stderr_tail: Mutex::new(String::new()),
            state: Mutex::new(State::default()),
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
        let source = if shared.resumed { "resume" } else { "startup" };
        if let Err(why) =
            shared
                .host
                .hook("session-start", "SessionStart", json!({"source": source}))
        {
            shared.note(&format!("session-start hook failed: {why}"));
        }
        // A new conversation's id is the engine's own, so it is ready now.
        // A resumed one is ready once Claude Code shows it has it: Claude
        // Code reports only after the first message.
        if !shared.resumed {
            shared.ready();
        }
        Ok(Box::new(DirectConversation { shared }))
    }

    fn new_lesson(&self, goal: &str) -> String {
        format!("{LESSON_MARK}{}", goal.trim())
    }

    /// Paper work runs on the plugin engine; these words are never sent to
    /// a direct lesson.
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

/// What the engine carries from turn to turn, besides the lesson.
#[derive(Default)]
struct State {
    lesson: Lesson,
    /// Whether the lesson's whole state has gone to the model.
    told: bool,
    /// Help lines from the last save, to tell the model next turn.
    help: Vec<String>,
    /// The quick-check names last told to the model.
    labels_told: Vec<String>,
    labels: Vec<String>,
    /// When the learner last sent a message, to tell a new sitting.
    last_message: Option<Instant>,
    /// The runtime was asked for this sitting's checks.
    checks_asked: bool,
    turn: u64,
}

struct Shared {
    sink: EventSink,
    stdin: Mutex<Option<ChildStdin>>,
    child: Mutex<Option<Child>>,
    host: HostClient,
    runtime: PathBuf,
    folder: PathBuf,
    session: String,
    resumed: bool,
    payment: Payment,
    queue: Mutex<VecDeque<Item>>,
    wake: Condvar,
    events: Mutex<Receiver<Option<Value>>>,
    phase: AtomicU8,
    interrupting: AtomicBool,
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
                    break item;
                }
                queue = shared.wake.wait(queue).unwrap_or_else(|p| p.into_inner());
            }
        };
        match item {
            Item::Words(words) => {
                shared.phase.store(PREPARING, Ordering::SeqCst);
                let ended = turn(shared, &words);
                shared.phase.store(IDLE, Ordering::SeqCst);
                if ended {
                    exited(shared);
                    return;
                }
            }
            Item::Ended => {
                exited(shared);
                return;
            }
        }
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
                for block in value["message"]["content"].as_array().into_iter().flatten() {
                    match block["type"].as_str() {
                        Some("text") => {
                            whole.push(block["text"].as_str().unwrap_or_default().to_owned())
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
    // A Claude Code that sent no pieces still sent the finished text.
    if said.cutter.is_empty() && !whole.is_empty() {
        let fresh = said.cutter.push(&whole.join("\n\n"));
        if show && !fresh.is_empty() {
            shared.emit(AgentEvent::TextDelta { text: fresh });
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
    Some(record::OpenTask {
        text,
        help,
        guess: plan.lines().any(|line| line == record::GUESS_LINE),
        check,
        again: plan.lines().any(|line| line == record::AGAIN_LINE),
        stuck: false,
    })
}

fn checks_of(result: &Value) -> Option<Vec<DueCheck>> {
    serde_json::from_value(result.get("sitting_checks")?.clone()).ok()
}

/// Paper questions due for a redo, from `magnesium progress --json`.
fn redos_due(shared: &Shared) -> u64 {
    let mut command = paths::light_command(&shared.runtime);
    command
        .current_dir(&shared.folder)
        .args(["progress", "--json"]);
    paths::run(command, Duration::from_secs(10))
        .ok()
        .and_then(|out| serde_json::from_str::<Value>(&out).ok())
        .and_then(|value| value["due"]["redos"].as_u64())
        .unwrap_or(0)
}

/// Before the model: capture the learner's words, start or resume the
/// runtime's lesson when none is active here, and gather the state to send.
fn prepare(
    shared: &Arc<Shared>,
    words: &str,
    from_window: bool,
    prompt_id: &str,
) -> (Option<String>, String) {
    let host = &shared.host;
    let mut state = lock(&shared.state);
    let new_sitting = state
        .last_message
        .is_none_or(|last| last.elapsed() > SITTING_GAP);
    if new_sitting {
        state.checks_asked = false;
    }
    state.last_message = Some(Instant::now());
    let mut facts = Facts {
        now: local_now(),
        full: !state.told && !shared.resumed,
        ..Facts::default()
    };
    let opened = from_window.then(|| opening(words)).flatten();
    facts.skip_checks = opened.as_ref().is_some_and(|(skip, _)| *skip);
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
    let mut event = host_client::current_event(&notice);
    let mut found_checks: Option<Vec<DueCheck>> = None;
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
                    if found_checks.is_none() {
                        found_checks = checks_of(&choices);
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
                }
                if found_checks.is_none() {
                    found_checks = checks_of(&result);
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
                facts.known = known(&prepared);
                if found_checks.is_none() {
                    found_checks = checks_of(&prepared);
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
            adopt(&mut state.lesson, &mut facts, &status);
            found_checks = checks_of(&status);
        }
        state.checks_asked = true;
    }
    // A new sitting's quick checks come once, with the first record call
    // after its first message. When the calls above made none, one is made.
    if found_checks.is_none()
        && !state.checks_asked
        && host_client::checks_due(&notice).is_some_and(|due| due > 0)
    {
        if let Ok(status) = host.call(
            "context",
            json!({"schema": "learner-model/v2", "action": "status"}),
        ) {
            found_checks = checks_of(&status);
        }
    }
    if let Some(checks) = found_checks {
        state.checks_asked = true;
        if facts.skip_checks {
            state.lesson.checks.clear();
        } else {
            facts.new_checks = checks.clone();
            state.lesson.checks = checks;
        }
    }
    if facts.full || (new_sitting && !shared.resumed) || facts.stopped_at.is_some() {
        facts.redos_due = redos_due(shared);
    }
    facts.help = std::mem::take(&mut state.help);
    if state.labels != state.labels_told {
        facts.open_labels = state.labels.clone();
        state.labels_told = state.labels.clone();
    }
    state.told = true;
    let text = lesson_state::text(&state.lesson, &facts);
    (event, text)
}

/// What the record knows of the learner, one line per memory, as a prepare
/// lists them.
fn known(prepared: &Value) -> Vec<String> {
    prepared["memories"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|memory| {
            let title = memory["title"].as_str()?;
            let summary = memory["summary"].as_str().unwrap_or_default();
            Some(record::clip(&format!("{title}: {summary}"), 300))
        })
        .take(8)
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
    let (event, state_text) = prepare(shared, &words, from_window, &prompt_id);
    if shared.interrupting.swap(false, Ordering::SeqCst) {
        shared.emit(AgentEvent::TurnEnded {
            ok: true,
            interrupted: true,
            error: None,
        });
        return false;
    }
    shared.phase.store(WRITING, Ordering::SeqCst);
    if let Err(error) = shared.send_user(&lesson_state::message(&state_text, &words)) {
        shared.emit(AgentEvent::TurnEnded {
            ok: false,
            interrupted: false,
            error: Some(error.0),
        });
        return false;
    }
    let said = listen(shared, true);
    let reply = said.cutter.reply();
    if !reply.is_empty() {
        shared.emit(AgentEvent::TextDone {
            text: reply.clone(),
        });
    }
    if said.ended {
        let interrupted = shared.interrupting.swap(false, Ordering::SeqCst);
        let expected = shared.closing.load(Ordering::SeqCst);
        shared.emit(AgentEvent::TurnEnded {
            ok: expected || interrupted,
            interrupted,
            error: (!expected && !interrupted)
                .then(|| "The tutor stopped before it finished.".into()),
        });
        return true;
    }
    shared.phase.store(SAVING, Ordering::SeqCst);
    // An interrupted reply gets no second request and no mark: the open
    // task stays open.
    if shared.interrupting.swap(false, Ordering::SeqCst) {
        shared.note("interrupted: no record for this turn");
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
                text: format!(
                    "{} Fix it in a terminal, then send your message again.",
                    claude::clip(&why, 200)
                ),
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
    let message = Message {
        words: words.clone(),
        event_id: event,
    };
    match &record {
        Some(record) => save(shared, record, &message, &reply),
        None => {
            let open = lock(&shared.state).lesson.task.is_some();
            shared.note("no valid record after asking again: this turn has no mark");
            if open {
                shared.emit(AgentEvent::Notice {
                    kind: NoticeKind::Other,
                    text: "Your last answer was not marked: the tutor's notes on it could not be read. The lesson goes on.".into(),
                });
            }
        }
    }
    // The reply as the learner saw it, for the record.
    if let Err(why) = shared.host.hook(
        "stop",
        "Stop",
        json!({"prompt_id": prompt_id, "stop_hook_active": false, "last_assistant_message": reply}),
    ) {
        shared.note(&format!("stop hook failed: {why}"));
    }
    shared.emit(AgentEvent::TurnEnded {
        ok: true,
        interrupted: false,
        error: None,
    });
    ended
}

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

/// Keep the turn's record in the runtime.
fn save(shared: &Arc<Shared>, record: &Record, message: &Message, reply: &str) {
    let mut state = lock(&shared.state);
    let request_id = format!("{}-turn-{}", shared.session, shared.next_id());
    let write = state.lesson.apply(record, message, reply, &request_id);
    let mut request = write.request;
    // A misunderstanding found or cleared revises the learner's memory of
    // that skill, in the same request as the observation it rests on.
    if let (Some(note), true) = (&record.memory, request.get("observations").is_some()) {
        let found = shared
            .host
            .call(
                "memory",
                json!({"schema": "learner-model/v2", "action": "search", "goal": note.skill, "original_query": note.skill}),
            )
            .ok();
        let memory = found
            .as_ref()
            .and_then(|found| record::memory_for(found, &note.skill));
        let goal = state.lesson.goal.clone().unwrap_or_default();
        let day = Local::now().format("%Y-%m-%d").to_string();
        request["memory_edits"] = json!([record::memory_edit(note, memory, &goal, &day)]);
    }
    let worth = request.get("observations").is_some()
        || request.get("checkpoint").is_some()
        || request.get("event_dispositions").is_some();
    if !worth {
        return;
    }
    let mut outcome = shared.host.call("learning_step", request.clone());
    if let Err(why) = &outcome {
        shared.note(&format!("learning_step refused: {why}"));
        // Asked once more: with the session's revision read again, if
        // another conversation moved it on, and without the memory edit,
        // so that a memory the runtime refuses never costs the mark.
        let mut retry = request.clone();
        if let Some(edits) = retry.as_object_mut() {
            edits.remove("memory_edits");
        }
        if why.contains("revision") {
            if let Ok(status) = shared.host.call(
                "context",
                json!({"schema": "learner-model/v2", "action": "status"}),
            ) {
                if let Some(revision) = status["session"]["revision"].as_i64() {
                    retry["session"]["expected_revision"] = json!(revision);
                }
            }
        }
        retry["request_id"] = json!(format!("{request_id}-again"));
        outcome = shared.host.call("learning_step", retry);
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
            if let Some(checks) = checks_of(&result) {
                state.lesson.checks = checks;
            }
        }
        Err(why) => {
            shared.note(&format!("the record was not saved: {why}"));
            if write.marked {
                shared.emit(AgentEvent::Notice {
                    kind: NoticeKind::Other,
                    text: "Your last answer could not be saved to your record. The lesson goes on."
                        .into(),
                });
            }
        }
    }
}

struct DirectConversation {
    shared: Arc<Shared>,
}

impl Conversation for DirectConversation {
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
        match self.shared.phase.load(Ordering::SeqCst) {
            PREPARING => {
                self.shared.interrupting.store(true, Ordering::SeqCst);
                Ok(())
            }
            WRITING => {
                self.shared.interrupting.store(true, Ordering::SeqCst);
                let id = self.shared.next_id();
                self.shared.write(&json!({
                    "type": "control_request",
                    "request_id": id,
                    "request": {"subtype": "interrupt", "cancel_queued": true},
                }))?;
                let mine = self.shared.interrupts.fetch_add(1, Ordering::SeqCst) + 1;
                let shared = Arc::clone(&self.shared);
                thread::spawn(move || {
                    for signal in ["INT", "TERM"] {
                        thread::sleep(Duration::from_secs(8));
                        if shared.interrupts.load(Ordering::SeqCst) != mine
                            || shared.phase.load(Ordering::SeqCst) != WRITING
                        {
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

    fn close(&mut self) {
        if self.shared.closing.swap(true, Ordering::SeqCst) {
            return;
        }
        if self.shared.phase.load(Ordering::SeqCst) == WRITING {
            let _ = self.interrupt();
        }
        lock(&self.shared.queue).retain(|item| matches!(item, Item::Ended));
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

    #[test]
    fn a_direct_lesson_loads_the_model_and_the_two_search_tools_only() {
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
        assert_eq!(after("--tools"), "Read");
        assert_eq!(after("--permission-prompts"), "none");
        assert_eq!(after("--add-dir"), "/study");
        assert_eq!(
            after("--disallowedTools"),
            "mcp__magnesium__context,mcp__magnesium__learning_step,mcp__magnesium__memory"
        );
        assert_eq!(
            after("--allowedTools"),
            "Read,mcp__magnesium__search_sources,mcp__magnesium__sources"
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
    fn a_new_session_id_is_a_uuid() {
        let id = new_session_id();
        assert_eq!(id.len(), 36);
        assert!(claude::is_session_id(&id));
        assert_ne!(id, new_session_id());
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
            again: false,
            stuck: false,
        });
        let write = lesson.apply(
            &record::parse(r#"{"answer": null}"#).unwrap(),
            &Message::default(),
            "",
            "r",
        );
        let task = task_from_checkpoint(&write.request["checkpoint"]).unwrap();
        assert_eq!(task, lesson.task.unwrap());
        assert!(task_from_checkpoint(&json!({"plan_text": "Confirm the work."})).is_none());
    }
}
