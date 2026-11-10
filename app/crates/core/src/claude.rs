//! Claude Code, driven through the program the learner installed.
//!
//! The window starts `claude` as Anthropic publishes it, in the study folder,
//! and talks to it over its own standard input and output. It never reads a
//! credential and never changes how Claude Code signs in. The session loads
//! the folder's hooks, skills and tool server exactly as a terminal session
//! does, so `--bare` is never passed and the first report is checked for the
//! Magnesium plugin.
//!
//! The wire format is the one the open-source Agent SDKs use: one JSON object
//! per line each way, with `control_request` and `control_response` objects for
//! permission prompts and interrupts.

use std::collections::{HashMap, HashSet};
use std::env;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::agent::{
    AgentError, AgentEvent, AgentResult, Answer, Choice, Conversation, Decision, EventSink,
    Harness, NoticeKind, Question, StartOptions,
};
use crate::paths;

/// Set this to use another program in place of `claude`. The tests set it to
/// a stand-in that speaks the same lines.
pub const PROGRAM_ENV: &str = "MAGNESIUM_APP_AGENT";

/// Set this to a file path to keep every line that passes between the window
/// and the agent. It holds the whole conversation, so it is off unless asked
/// for. The live check turns it on, to settle what the real program sends.
pub const TRACE_ENV: &str = "MAGNESIUM_APP_TRACE";

const PLUGIN: &str = "magnesium";

#[derive(Default)]
pub struct ClaudeCode {
    program: Option<PathBuf>,
}

impl ClaudeCode {
    pub fn new() -> Self {
        Self::default()
    }

    /// Use this program and ignore the environment. For tests.
    pub fn with_program(program: PathBuf) -> Self {
        Self {
            program: Some(program),
        }
    }
}

impl Harness for ClaudeCode {
    fn name(&self) -> &'static str {
        "claude"
    }

    fn locate(&self) -> AgentResult<PathBuf> {
        if let Some(program) = &self.program {
            return Ok(program.clone());
        }
        if let Some(named) = env::var_os(PROGRAM_ENV).filter(|v| !v.is_empty()) {
            return Ok(PathBuf::from(named));
        }
        paths::find_program("claude").ok_or_else(|| {
            AgentError(
                "Claude Code is not installed, or the window cannot find it. Install it, sign in once in a terminal, then open the window again."
                    .into(),
            )
        })
    }

    fn start(&self, options: StartOptions, sink: EventSink) -> AgentResult<Box<dyn Conversation>> {
        let program = self.locate()?;
        let mut command = paths::command(&program);
        command
            .current_dir(&options.folder)
            .env("PWD", &options.folder)
            .args([
                "-p",
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--verbose",
                "--include-partial-messages",
                "--permission-prompt-tool",
                "stdio",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(id) = options.resume.as_deref().filter(|id| is_session_id(id)) {
            // One token, so an id can never be read as another flag.
            command.arg(format!("--resume={id}"));
        }
        let mut child = command
            .spawn()
            .map_err(|e| AgentError(format!("The window could not start Claude Code: {e}.")))?;
        let stdin = child.stdin.take();
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AgentError("Claude Code gave no output pipe.".into()))?;
        let stderr = child.stderr.take();

        let shared = Arc::new(Shared {
            sink,
            stdin: Mutex::new(stdin),
            child: Mutex::new(Some(child)),
            session_id: Mutex::new(options.resume.clone()),
            pending: Mutex::new(HashMap::new()),
            allowed: Mutex::new(HashSet::new()),
            seen_text: Mutex::new(HashMap::new()),
            stderr_tail: Mutex::new(String::new()),
            closing: AtomicBool::new(false),
            interrupting: AtomicBool::new(false),
            awaiting: AtomicBool::new(false),
            in_turn: AtomicBool::new(false),
            ready_sent: AtomicBool::new(false),
            counter: AtomicU64::new(0),
            interrupts: AtomicU64::new(0),
            trace: env::var_os(TRACE_ENV)
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
                        let cut = tail.len() - 2000;
                        let cut = (cut..tail.len())
                            .find(|i| tail.is_char_boundary(*i))
                            .unwrap_or(0);
                        *tail = tail[cut..].to_owned();
                    }
                }
            });
        }
        {
            let shared = Arc::clone(&shared);
            thread::spawn(move || read_loop(&shared, stdout));
        }

        // The SDKs open every conversation this way. Its reply is not judged:
        // it can come before a folder's plugin has finished installing, so it
        // cannot say the plugin is missing. The first `system/init` can.
        let id = shared.next_id();
        shared.write(&json!({
            "type": "control_request",
            "request_id": id,
            "request": {"subtype": "initialize", "hooks": null},
        }))?;
        Ok(Box::new(ClaudeConversation { shared }))
    }

    fn new_lesson(&self, goal: &str) -> String {
        format!("/magnesium:learn {}", goal.trim())
    }

    fn papers(&self, request: &str) -> String {
        format!("/magnesium:papers {}", request.trim())
    }
}

fn is_session_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && !id.starts_with('-')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

enum Pending {
    Permission {
        tool: String,
        input: Value,
        suggestions: Value,
    },
    Question {
        input: Value,
    },
}

struct Shared {
    sink: EventSink,
    stdin: Mutex<Option<ChildStdin>>,
    child: Mutex<Option<Child>>,
    session_id: Mutex<Option<String>>,
    pending: Mutex<HashMap<String, Pending>>,
    /// Exact calls the learner allowed for the rest of the sitting.
    allowed: Mutex<HashSet<String>>,
    /// Text blocks already shown, by the id of the message they belong to.
    seen_text: Mutex<HashMap<String, Vec<String>>>,
    stderr_tail: Mutex<String>,
    closing: AtomicBool,
    interrupting: AtomicBool,
    /// A message was sent and its turn has not ended. True before the tutor's
    /// first word, when `in_turn` is still false.
    awaiting: AtomicBool,
    in_turn: AtomicBool,
    ready_sent: AtomicBool,
    counter: AtomicU64,
    /// Counts interrupts, so a watchdog only acts on the one that set it.
    interrupts: AtomicU64,
    trace: Option<Mutex<std::fs::File>>,
    started: Instant,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Shared {
    fn emit(&self, event: AgentEvent) {
        (self.sink)(event);
    }

    fn next_id(&self) -> String {
        let n = self.counter.fetch_add(1, Ordering::Relaxed) + 1;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        format!("req_{n}_{nanos:08x}")
    }

    /// One line of the trace: its direction, the milliseconds since the
    /// conversation started, and the line as it passed.
    fn trace(&self, direction: &str, line: &str) {
        if let Some(file) = &self.trace {
            let ms = self.started.elapsed().as_millis();
            let _ = writeln!(lock(file), "{direction} {ms} {}", line.trim_end());
        }
    }

    /// Whether a turn is under way, from the learner's message to its result.
    fn turn_active(&self) -> bool {
        self.awaiting.load(Ordering::SeqCst) || self.in_turn.load(Ordering::SeqCst)
    }

    /// Send a signal by name, with the system's own `kill`.
    fn signal(&self, name: &str) {
        if let Some(child) = lock(&self.child).as_ref() {
            let _ = std::process::Command::new("kill")
                .arg(format!("-{name}"))
                .arg(child.id().to_string())
                .status();
        }
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

    fn respond(&self, request_id: &str, response: Value) -> AgentResult<()> {
        self.write(&json!({
            "type": "control_response",
            "response": {"subtype": "success", "request_id": request_id, "response": response},
        }))
    }

    fn refuse_request(&self, request_id: &str, reason: &str) {
        let _ = self.write(&json!({
            "type": "control_response",
            "response": {"subtype": "error", "request_id": request_id, "error": reason},
        }));
    }
}

fn read_loop(shared: &Arc<Shared>, stdout: impl Read) {
    let started = Instant::now();
    let mut reader = BufReader::new(stdout);
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        // Read bytes, not text: one byte that is not UTF-8 must not end the
        // lesson.
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
        // A line that is not JSON is noise from a tool, not a message.
        if let Ok(value) = serde_json::from_str::<Value>(line) {
            handle(shared, &value);
        }
    }

    // The pipe closed, so the process is ending.
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
    lock(&shared.pending).clear();
    let expected = shared.closing.load(Ordering::SeqCst);
    let tail = lock(&shared.stderr_tail).trim().to_owned();
    let detail = (!expected).then(|| {
        let last = tail.lines().rev().find(|l| !l.trim().is_empty());
        match (last, status) {
            (Some(line), _) => clip(line, 300),
            (None, Some(status)) if !status.success() => format!("It ended with {status}."),
            _ if started.elapsed() < Duration::from_secs(3) => {
                "It ended as soon as it started.".to_owned()
            }
            _ => "It ended without saying why.".to_owned(),
        }
    });
    if !expected && looks_signed_out(&tail) {
        shared.emit(AgentEvent::Notice {
            kind: NoticeKind::SignIn,
            text: SIGN_IN.into(),
        });
    }
    let was_awaiting = shared.awaiting.swap(false, Ordering::SeqCst);
    if shared.in_turn.swap(false, Ordering::SeqCst) || was_awaiting {
        let interrupted = shared.interrupting.swap(false, Ordering::SeqCst);
        shared.emit(AgentEvent::TurnEnded {
            ok: expected || interrupted,
            interrupted,
            error: (!expected && !interrupted)
                .then(|| "The tutor stopped before it finished.".into()),
        });
    }
    shared.emit(AgentEvent::Exited { expected, detail });
}

const SIGN_IN: &str =
    "Claude Code is signed out. Run claude in a terminal, sign in, then send your message again.";

fn looks_signed_out(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    text.contains("/login")
        || text.contains("invalid api key")
        || text.contains("not logged in")
        || text.contains("authentication_error")
        || text.contains("oauth token")
}

fn looks_like_a_limit(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    text.contains("usage limit")
        || text.contains("rate limit")
        || text.contains("limit reached")
        || text.contains("hit your limit")
        || text.contains("credit balance")
}

fn handle(shared: &Arc<Shared>, value: &Value) {
    match value["type"].as_str() {
        Some("system") => match value["subtype"].as_str() {
            Some("init") => on_init(shared, value),
            Some("api_retry") => on_retry(shared, value),
            Some("hook_response") => on_hook(shared, value),
            _ => {}
        },
        Some("stream_event") => on_stream(shared, value),
        Some("assistant") => on_assistant(shared, value),
        Some("result") => on_result(shared, value),
        Some("rate_limit_event") => on_rate_limit(shared, value),
        Some("control_request") => on_control_request(shared, value),
        Some("control_cancel_request") => {
            if let Some(id) = value["request_id"].as_str() {
                if lock(&shared.pending).remove(id).is_some() {
                    shared.emit(AgentEvent::Withdrawn { id: id.to_owned() });
                }
            }
        }
        _ => {}
    }
}

fn names(list: &Value) -> Vec<String> {
    list.as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().or_else(|| item["name"].as_str()))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// What is wrong with what the agent loaded, in plain sentences.
fn init_problems(value: &Value) -> Vec<String> {
    let mut problems = Vec::new();
    let plugins = names(&value["plugins"]);
    let commands: Vec<String> = names(&value["slash_commands"])
        .into_iter()
        .chain(names(&value["skills"]))
        .collect();
    let tools = names(&value["tools"]);
    let has_plugin = plugins.iter().any(|name| name.contains(PLUGIN))
        || commands.iter().any(|name| name.contains("magnesium:"));
    // A report that lists neither plugins nor commands cannot say the plugin
    // is missing, so it is not called missing.
    let can_tell = value.get("plugins").is_some() || !commands.is_empty();
    if can_tell && !has_plugin {
        problems.push(
            "The Magnesium plugin did not load, so this would be a plain chat with no teaching rules. In a terminal, run: magnesium update"
                .to_owned(),
        );
    }
    // An error that names the plugin counts only when the plugin is absent.
    // A second copy at another scope can complain while the first one works.
    if !has_plugin {
        for error in value["plugin_errors"].as_array().into_iter().flatten() {
            if error.to_string().contains(PLUGIN) {
                let message = error["message"].as_str().unwrap_or("it failed to load");
                problems.push(format!(
                    "The Magnesium plugin reported: {}",
                    clip(message, 200)
                ));
            }
        }
    }
    let servers = value["mcp_servers"].as_array().cloned().unwrap_or_default();
    // The plugin's own server is named plugin:<plugin>:<server>. It is looked
    // for first, so another server with a similar name cannot decide this.
    let named = |test: &dyn Fn(&str) -> bool| {
        servers
            .iter()
            .find(|s| s["name"].as_str().is_some_and(test))
    };
    let server = named(&|name| name.starts_with("plugin:magnesium:"))
        .or_else(|| named(&|name| name.contains(PLUGIN)));
    let has_tools = tools
        .iter()
        .any(|tool| tool.starts_with("mcp__") && tool.contains(PLUGIN));
    match server.and_then(|s| s["status"].as_str()) {
        Some("connected" | "pending") => {}
        Some(status) => problems.push(format!(
            "Magnesium's tool server is {status}, so the tutor cannot read or save your record. In a terminal, run: magnesium info"
        )),
        None if has_plugin && !has_tools && value.get("mcp_servers").is_some() => problems.push(
            "Magnesium's tool server did not start, so the tutor cannot read or save your record. In a terminal, run: magnesium info"
                .to_owned(),
        ),
        None => {}
    }
    problems
}

fn on_init(shared: &Arc<Shared>, value: &Value) {
    if let Some(id) = value["session_id"].as_str() {
        *lock(&shared.session_id) = Some(id.to_owned());
    }
    if !shared.ready_sent.swap(true, Ordering::SeqCst) {
        let session_id = lock(&shared.session_id).clone().unwrap_or_default();
        let problems = init_problems(value);
        shared.emit(AgentEvent::Ready {
            session_id,
            plugin_missing: problems
                .iter()
                .any(|problem| problem.contains("plugin did not load")),
            problems,
        });
    }
}

fn on_hook(shared: &Arc<Shared>, value: &Value) {
    let event = value["hook_event"]
        .as_str()
        .or_else(|| value["hook_event_name"].as_str())
        .or_else(|| value["hook_name"].as_str())
        .unwrap_or_default();
    if !event.starts_with("SessionStart") {
        return;
    }
    let failed = value["outcome"].as_str() == Some("error")
        || value["exit_code"].as_i64().is_some_and(|code| code != 0);
    if failed {
        let why = value["stderr"]
            .as_str()
            .and_then(|s| s.lines().find(|l| !l.trim().is_empty()))
            .unwrap_or("it gave no reason");
        shared.emit(AgentEvent::Notice {
            kind: NoticeKind::Other,
            text: format!(
                "A session-start hook failed ({}). If it is Magnesium's, the tutor has lost its rules. In a terminal, run: magnesium info",
                clip(why, 160)
            ),
        });
    }
}

fn on_retry(shared: &Arc<Shared>, value: &Value) {
    let (kind, text) = match value["error"].as_str().unwrap_or_default() {
        "authentication_failed" | "oauth_org_not_allowed" => (NoticeKind::SignIn, SIGN_IN.to_owned()),
        "rate_limit" => (
            NoticeKind::UsageLimit,
            "Claude Code is at a usage limit and is waiting to try again.".to_owned(),
        ),
        "billing_error" | "account_on_hold" => (
            NoticeKind::SignIn,
            "Claude Code reports a billing problem on your account. Fix it in a terminal or on the web, then send your message again."
                .to_owned(),
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
    if info["status"].as_str() != Some("rejected") {
        return;
    }
    // Past the limit with extra usage still allowed, the lesson goes on.
    if info["overageStatus"]
        .as_str()
        .is_some_and(|status| status.starts_with("allowed"))
    {
        return;
    }
    let resets = info["resetsAt"]
        .as_i64()
        .or_else(|| info["resets_at"].as_i64())
        // Seconds, in the live trace of 2026-11-10. A stamp in milliseconds
        // is still read, in case a later version changes it.
        .map(|stamp| {
            if stamp > 100_000_000_000 {
                stamp / 1000
            } else {
                stamp
            }
        })
        .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
        .map(|utc| {
            utc.with_timezone(&chrono::Local)
                .format("%H:%M on %a %-d %b")
                .to_string()
        });
    let text = match resets {
        Some(when) => format!("You reached your Claude usage limit. It resets at {when}."),
        None => "You reached your Claude usage limit.".to_owned(),
    };
    shared.emit(AgentEvent::Notice {
        kind: NoticeKind::UsageLimit,
        text,
    });
}

fn is_main_thread(value: &Value) -> bool {
    value["parent_tool_use_id"].is_null()
}

fn on_stream(shared: &Arc<Shared>, value: &Value) {
    // A helper agent's words are never shown as the tutor's.
    if !is_main_thread(value) {
        return;
    }
    let event = &value["event"];
    match event["type"].as_str() {
        Some("message_start") => begin_turn(shared),
        Some("content_block_start") => match event["content_block"]["type"].as_str() {
            Some("text") | None => {}
            Some(_) => shared.emit(AgentEvent::Working),
        },
        Some("content_block_delta") if event["delta"]["type"].as_str() == Some("text_delta") => {
            if let Some(text) = event["delta"]["text"].as_str().filter(|t| !t.is_empty()) {
                begin_turn(shared);
                shared.emit(AgentEvent::TextDelta {
                    text: text.to_owned(),
                });
            }
        }
        _ => {}
    }
}

fn begin_turn(shared: &Arc<Shared>) {
    if !shared.in_turn.swap(true, Ordering::SeqCst) {
        shared.emit(AgentEvent::TurnStarted);
    }
}

fn on_assistant(shared: &Arc<Shared>, value: &Value) {
    if !is_main_thread(value) {
        return;
    }
    let message = &value["message"];
    let id = message["id"].as_str().unwrap_or_default().to_owned();
    let Some(blocks) = message["content"].as_array() else {
        return;
    };
    for block in blocks {
        match block["type"].as_str() {
            Some("text") => {
                let Some(text) = block["text"].as_str().filter(|t| !t.trim().is_empty()) else {
                    continue;
                };
                // The agent may send a message once per block or once whole.
                // Either way each block is shown one time.
                let fresh = {
                    let mut seen = lock(&shared.seen_text);
                    let texts = seen.entry(id.clone()).or_default();
                    if texts.iter().any(|t| t == text) {
                        false
                    } else {
                        texts.push(text.to_owned());
                        true
                    }
                };
                if fresh {
                    begin_turn(shared);
                    shared.emit(AgentEvent::TextDone {
                        text: text.to_owned(),
                    });
                }
            }
            Some("tool_use" | "server_tool_use" | "thinking") => {
                begin_turn(shared);
                shared.emit(AgentEvent::Working);
            }
            _ => {}
        }
    }
    if let Some(error) = value["error"].as_str() {
        match error {
            "authentication_failed" => shared.emit(AgentEvent::Notice {
                kind: NoticeKind::SignIn,
                text: SIGN_IN.into(),
            }),
            "rate_limit" => shared.emit(AgentEvent::Notice {
                kind: NoticeKind::UsageLimit,
                text: "You reached your Claude usage limit.".into(),
            }),
            _ => {}
        }
    }
}

fn on_result(shared: &Arc<Shared>, value: &Value) {
    if let Some(id) = value["session_id"].as_str() {
        *lock(&shared.session_id) = Some(id.to_owned());
    }
    shared.in_turn.store(false, Ordering::SeqCst);
    shared.awaiting.store(false, Ordering::SeqCst);
    lock(&shared.seen_text).clear();
    let waiting: Vec<String> = lock(&shared.pending).drain().map(|(id, _)| id).collect();
    for id in waiting {
        shared.emit(AgentEvent::Withdrawn { id });
    }
    let interrupted = shared.interrupting.swap(false, Ordering::SeqCst);
    let failed = value["is_error"].as_bool().unwrap_or(false)
        || value["subtype"]
            .as_str()
            .is_some_and(|subtype| subtype != "success");
    let mut error = None;
    if failed && !interrupted {
        let said = what_went_wrong(value);
        if looks_signed_out(&said) {
            shared.emit(AgentEvent::Notice {
                kind: NoticeKind::SignIn,
                text: SIGN_IN.into(),
            });
        } else if looks_like_a_limit(&said) {
            shared.emit(AgentEvent::Notice {
                kind: NoticeKind::UsageLimit,
                text: format!(
                    "{} Fix it in a terminal, then send your message again.",
                    clip(&said, 200)
                ),
            });
        } else {
            error = Some(match value["subtype"].as_str() {
                Some("error_max_turns") => {
                    "The tutor ran out of steps for one reply. Send your message again.".to_owned()
                }
                _ if said.trim().is_empty() => {
                    "The reply ended with an error. Send your message again.".to_owned()
                }
                _ => clip(&said, 300),
            });
        }
    }
    shared.emit(AgentEvent::TurnEnded {
        ok: !failed || interrupted,
        interrupted,
        error,
    });
}

/// The words a failed result gives for its failure, for the learner.
fn what_went_wrong(value: &Value) -> String {
    value["result"]
        .as_str()
        .map(str::to_owned)
        .filter(|text| !text.trim().is_empty())
        .or_else(|| {
            value["errors"].as_array().map(|errors| {
                errors
                    .iter()
                    .filter_map(Value::as_str)
                    // A line such as "[ede_diagnostic] result_type=user" is
                    // for the program's makers, not the learner.
                    .filter(|error| !error.trim_start().starts_with("[ede_diagnostic]"))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
        })
        .unwrap_or_default()
}

fn on_control_request(shared: &Arc<Shared>, value: &Value) {
    let Some(id) = value["request_id"].as_str() else {
        return;
    };
    let request = &value["request"];
    if request["subtype"].as_str() != Some("can_use_tool") {
        // The window registers no hooks and no in-process tool servers.
        shared.refuse_request(id, "the Magnesium window does not handle this request");
        return;
    }
    let tool = request["tool_name"].as_str().unwrap_or_default().to_owned();
    let input = request["input"].clone();
    if tool == "AskUserQuestion" {
        let questions = parse_questions(&input);
        if questions.is_empty() {
            let _ = shared.respond(
                id,
                json!({"behavior": "deny", "message": "The question had no text. Ask it in your reply."}),
            );
            return;
        }
        lock(&shared.pending).insert(id.to_owned(), Pending::Question { input });
        shared.emit(AgentEvent::Question {
            id: id.to_owned(),
            questions,
        });
        return;
    }
    if lock(&shared.allowed).contains(&call_key(&tool, &input)) {
        let _ = shared.respond(id, json!({"behavior": "allow", "updatedInput": input}));
        return;
    }
    let (title, mut detail) = describe(&tool, &input);
    if detail.is_empty() {
        detail = request["description"]
            .as_str()
            .or_else(|| request["title"].as_str())
            .map(|text| clip(text, 300))
            .unwrap_or_default();
    }
    let suggestions = request["permission_suggestions"].clone();
    lock(&shared.pending).insert(
        id.to_owned(),
        Pending::Permission {
            tool,
            input,
            suggestions,
        },
    );
    shared.emit(AgentEvent::Permission {
        id: id.to_owned(),
        title,
        detail,
        can_always: true,
    });
}

fn call_key(tool: &str, input: &Value) -> String {
    format!("{tool}\u{1f}{input}")
}

fn parse_questions(input: &Value) -> Vec<Question> {
    input["questions"]
        .as_array()
        .map(|questions| {
            questions
                .iter()
                .filter_map(|q| {
                    // Kept exactly as written: the answer is keyed by it.
                    let question = q["question"].as_str()?.to_owned();
                    if question.trim().is_empty() {
                        return None;
                    }
                    let options = q["options"]
                        .as_array()
                        .map(|options| {
                            options
                                .iter()
                                .filter_map(|option| {
                                    Some(Choice {
                                        label: option["label"].as_str()?.to_owned(),
                                        description: option["description"]
                                            .as_str()
                                            .map(str::to_owned)
                                            .filter(|d| !d.is_empty()),
                                    })
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    Some(Question {
                        question,
                        header: q["header"].as_str().map(str::to_owned),
                        options,
                        multi: q["multiSelect"].as_bool().unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A permission request as a plain title and the thing it would touch.
pub fn describe(tool: &str, input: &Value) -> (String, String) {
    let field = |name: &str| {
        input[name]
            .as_str()
            .map(|text| clip(text, 400))
            .unwrap_or_default()
    };
    match tool {
        "Bash" => ("Run a command".into(), field("command")),
        "Write" => ("Write a file".into(), field("file_path")),
        "Edit" | "MultiEdit" => ("Change a file".into(), field("file_path")),
        "NotebookEdit" => ("Change a notebook".into(), field("notebook_path")),
        "Read" => ("Read a file".into(), field("file_path")),
        "Glob" | "Grep" => ("Search your files".into(), field("pattern")),
        "WebFetch" => ("Open a web page".into(), field("url")),
        "WebSearch" => ("Search the web".into(), field("query")),
        "Skill" => ("Use a skill".into(), field("skill")),
        "Agent" | "Task" => ("Start a helper".into(), field("description")),
        other if other.starts_with("mcp__") => {
            let mut parts = other.trim_start_matches("mcp__").rsplitn(2, "__");
            let name = parts.next().unwrap_or(other);
            let server = parts.next().unwrap_or_default();
            let server = server.rsplit([':', '_']).next().unwrap_or(server);
            (
                "Use a tool".into(),
                if server.is_empty() {
                    name.to_owned()
                } else {
                    format!("{name}, from {server}")
                },
            )
        }
        other => (format!("Use {other}"), clip(&input.to_string(), 200)),
    }
}

fn clip(text: &str, chars: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= chars {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

struct ClaudeConversation {
    shared: Arc<Shared>,
}

impl Conversation for ClaudeConversation {
    fn send(&mut self, text: &str) -> AgentResult<()> {
        self.shared.write(&json!({
            "type": "user",
            "message": {"role": "user", "content": text},
            "parent_tool_use_id": null,
            "session_id": "default",
        }))?;
        self.shared.awaiting.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn interrupt(&mut self) -> AgentResult<()> {
        // A turn is under way from the moment the message is sent, which is
        // well before the tutor's first word.
        if !self.shared.turn_active() {
            return Ok(());
        }
        let id = self.shared.next_id();
        // Marked before it is sent: the agent can end the turn before this
        // thread runs again, and that end must read as an interrupt, not a
        // failure.
        self.shared.interrupting.store(true, Ordering::SeqCst);
        let sent = self.shared.write(&json!({
            "type": "control_request",
            "request_id": id,
            // Also drop a message the agent has taken but not started on.
            "request": {"subtype": "interrupt", "cancel_queued": true},
        }));
        if let Err(error) = sent {
            self.shared.interrupting.store(false, Ordering::SeqCst);
            return Err(error);
        }
        let mine = self.shared.interrupts.fetch_add(1, Ordering::SeqCst) + 1;
        // An agent that does not answer is sent the signal that ends a turn,
        // and after that the one that ends the process. The conversation is
        // on disk and continues by its id.
        let shared = Arc::clone(&self.shared);
        thread::spawn(move || {
            for signal in ["INT", "TERM"] {
                thread::sleep(Duration::from_secs(8));
                let still_mine = shared.interrupts.load(Ordering::SeqCst) == mine
                    && shared.interrupting.load(Ordering::SeqCst)
                    && shared.turn_active();
                if !still_mine {
                    return;
                }
                shared.signal(signal);
            }
        });
        Ok(())
    }

    fn decide(&mut self, id: &str, decision: Decision) -> AgentResult<()> {
        let Some(Pending::Permission {
            tool,
            input,
            suggestions,
        }) = lock(&self.shared.pending).remove(id)
        else {
            return Err(AgentError("That request is no longer waiting.".into()));
        };
        let response = match decision {
            Decision::AllowOnce => json!({"behavior": "allow", "updatedInput": input}),
            Decision::AllowAlways => {
                // The agent's own suggestion is the rule a terminal would
                // save for "don't ask again". Only a rule for this folder or
                // this sitting is echoed back, never one for every folder.
                // Without one, the window remembers this exact call until the
                // sitting ends.
                let rules: Vec<Value> = suggestions
                    .as_array()
                    .map(|list| {
                        list.iter()
                            .filter(|rule| {
                                matches!(
                                    rule["destination"].as_str(),
                                    Some("localSettings" | "session")
                                )
                            })
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default();
                if !rules.is_empty() {
                    json!({"behavior": "allow", "updatedInput": input, "updatedPermissions": rules})
                } else {
                    lock(&self.shared.allowed).insert(call_key(&tool, &input));
                    json!({"behavior": "allow", "updatedInput": input})
                }
            }
            Decision::Refuse => json!({
                "behavior": "deny",
                "message": "The learner refused this. Carry on without it, or say why it is needed.",
            }),
        };
        self.shared.respond(id, response)
    }

    fn answer(&mut self, id: &str, answers: &[Answer]) -> AgentResult<()> {
        let Some(Pending::Question { mut input }) = lock(&self.shared.pending).remove(id) else {
            return Err(AgentError("That question is no longer waiting.".into()));
        };
        let mut map = serde_json::Map::new();
        for answer in answers {
            map.insert(
                answer.question.clone(),
                Value::String(answer.chosen.join(", ")),
            );
        }
        if let Some(object) = input.as_object_mut() {
            object.insert("answers".into(), Value::Object(map));
        }
        self.shared
            .respond(id, json!({"behavior": "allow", "updatedInput": input}))
    }

    fn session_id(&self) -> Option<String> {
        lock(&self.shared.session_id).clone()
    }

    fn close(&mut self) {
        self.shared.closing.store(true, Ordering::SeqCst);
        // Closing the input is how the agent is told the conversation is over.
        // It then runs its session-end hooks and writes the last of the
        // transcript, so it is given time before it is told to stop, and more
        // time before it is stopped.
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

impl Drop for ClaudeConversation {
    fn drop(&mut self) {
        if !self.shared.closing.load(Ordering::SeqCst) {
            self.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_without_the_plugin_is_a_problem() {
        let value = json!({"plugins": [], "mcp_servers": [], "tools": ["Read"], "slash_commands": ["help"]});
        let problems = init_problems(&value);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("plugin did not load"));
    }

    #[test]
    fn a_report_with_the_plugin_and_its_server_is_clean() {
        let value = json!({
            "plugins": [{"name": "magnesium", "path": "/x"}],
            "mcp_servers": [{"name": "plugin:magnesium:magnesium", "status": "connected"}],
            "tools": ["Read", "mcp__plugin_magnesium_magnesium__context"],
            "slash_commands": ["magnesium:learn"],
        });
        assert!(init_problems(&value).is_empty());
    }

    #[test]
    fn a_failed_tool_server_is_named() {
        let value = json!({
            "plugins": [{"name": "magnesium"}],
            "mcp_servers": [{"name": "plugin:magnesium:magnesium", "status": "failed"}],
            "tools": ["Read"],
        });
        let problems = init_problems(&value);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("tool server is failed"));
    }

    #[test]
    fn a_complaint_about_a_second_copy_does_not_break_a_working_plugin() {
        let value = json!({
            "plugins": [{"name": "magnesium"}],
            "plugin_errors": [{"plugin": "magnesium@other", "type": "duplicate", "message": "already installed"}],
            "mcp_servers": [
                {"name": "magnesium-notes", "status": "failed"},
                {"name": "plugin:magnesium:magnesium", "status": "connected"},
            ],
            "tools": ["mcp__plugin_magnesium_magnesium__context"],
        });
        assert!(init_problems(&value).is_empty());
    }

    #[test]
    fn an_old_report_that_lists_nothing_is_not_called_broken() {
        assert!(init_problems(&json!({"tools": ["Read"]})).is_empty());
    }

    #[test]
    fn requests_read_as_plain_sentences() {
        assert_eq!(
            describe("Bash", &json!({"command": "tectonic paper.tex"})),
            ("Run a command".into(), "tectonic paper.tex".into())
        );
        assert_eq!(
            describe("mcp__plugin_magnesium_magnesium__learning_step", &json!({})),
            ("Use a tool".into(), "learning_step, from magnesium".into())
        );
    }

    #[test]
    fn the_programs_own_diagnostics_are_not_shown() {
        // As the real program ended a turn interrupted before its first word.
        let ended = json!({
            "type": "result",
            "subtype": "error_during_execution",
            "is_error": true,
            "errors": ["[ede_diagnostic] result_type=user last_content_type=n/a stop_reason=null"],
        });
        assert_eq!(what_went_wrong(&ended), "");
        let plain =
            json!({"subtype": "error_during_execution", "errors": ["API Error: overloaded"]});
        assert_eq!(what_went_wrong(&plain), "API Error: overloaded");
    }

    #[test]
    fn a_session_id_cannot_be_a_flag() {
        assert!(is_session_id("0b9f1c1e-7a52-4f6e-9a57-3f1b8d2c4e10"));
        assert!(!is_session_id("--dangerously-skip-permissions"));
        assert!(!is_session_id("a b"));
        assert!(!is_session_id(""));
    }
}
