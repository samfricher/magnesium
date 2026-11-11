//! Claude Code, driven through the program the learner installed.
//!
//! The window starts `claude` as Anthropic publishes it, in the study folder,
//! and talks to it over its own standard input and output. It never reads a
//! credential and never changes how Claude Code signs in. The session loads
//! Magnesium's plugin, with its hooks, skills and tool server, as a terminal
//! session does, so `--bare` is never passed and the first report is checked
//! for the Magnesium plugin. Of the learner's own setup it loads nothing else:
//! see [`lesson_args`].
//!
//! The wire format is the one the open-source Agent SDKs use: one JSON object
//! per line each way, with `control_request` and `control_response` objects for
//! permission prompts and interrupts.

use std::collections::{HashMap, HashSet};
use std::env;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::agent::{
    Account, AgentError, AgentEvent, AgentResult, Answer, Choice, Conversation, Decision,
    EventSink, Harness, NoticeKind, Payment, Question, StartOptions,
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
const NAME: &str = "Claude Code";
/// Anthropic's page for installing Claude Code. The window links to it and
/// never runs an installer itself.
const INSTALL_PAGE: &str = "https://code.claude.com/docs/en/setup";
/// How long a sign-in may wait for the learner in the browser.
const SIGN_IN_LIMIT: Duration = Duration::from_secs(15 * 60);

/// Set on every Claude Code the window starts, and so on Magnesium's hooks
/// and tool server under it. The tool server then offers its `reply` tool,
/// whose input the window draws as the tutor's words while they are written.
pub const SURFACE_ENV: &str = "MAGNESIUM_SURFACE";
const SURFACE: &str = "window";
/// Claude Code's switch for the tool servers of the learner's Claude account.
pub const CONNECTORS_ENV: &str = "ENABLE_CLAUDEAI_MCP_SERVERS";

/// The model a lesson runs on: the one Claude Code chose by default on
/// 2026-11-11. It is named, so a later change of that default cannot change
/// the tutor unseen.
pub const MODEL: &str = "claude-fable-5-2";

/// Claude Code's own tools a lesson may use. Everything else Claude Code
/// offers is left out, and so is the learner's own setup: no other tool
/// server, account connector, plugin or skill. Magnesium's five tools come
/// from its plugin, which is loaded by itself.
pub const TOOLS: [&str; 9] = [
    "Read",
    "Write",
    "Edit",
    "Glob",
    "Grep",
    "AskUserQuestion",
    "WebSearch",
    "WebFetch",
    "Bash",
];

/// Where `magnesium init` keeps the plugin for Claude Code on this Mac.
fn plugin_dir() -> Option<PathBuf> {
    let dir = paths::home().join("Library/Application Support/magnesium/claude-plugin");
    dir.join(".claude-plugin/plugin.json")
        .is_file()
        .then_some(dir)
}

/// What a lesson loads, beyond the pipe itself.
///
/// - Settings come from the study folder only (`project,local`), so the
///   learner's user settings, with their plugins, hooks and skills, stay out.
///   Tool servers and account connectors from the learner's account come
///   with those settings, and are left out with them.
/// - Magnesium's plugin is loaded from where `magnesium init` put it, so its
///   hooks, skills and tool server load as in a terminal. Its tools are in
///   the session from the first turn, as there are too few to defer.
/// - Bash runs inside Claude Code's sandbox, and Write and Edit never touch
///   `.magnesium/`. Permission cards are unchanged: a sandboxed command still
///   asks.
/// - The model is named. The effort is Claude Code's default, as in a
///   terminal.
///
/// Without the plugin on this Mac, the folder's settings still name it, and
/// the first report then says the plugin did not load.
pub fn lesson_args(folder: &Path, plugin: Option<&Path>) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "--setting-sources".into(),
        "project,local".into(),
        "--tools".into(),
        TOOLS.join(","),
        "--model".into(),
        MODEL.into(),
    ];
    // No effort is passed: a lesson thinks as hard as in a terminal. Low
    // effort was tried on 2026-11-11 and called context.prepare at 1 of 11
    // lesson starts, so it waits for a timing on the final build that shows
    // a large gap, and then for the terminal too.
    if let Some(plugin) = plugin {
        args.push("--plugin-dir".into());
        args.push(plugin.display().to_string());
    }
    let mut folders = vec![folder.to_path_buf()];
    if let Ok(real) = std::fs::canonicalize(folder) {
        if real != folder {
            folders.push(real);
        }
    }
    let deny: Vec<String> = folders
        .iter()
        .flat_map(|folder| {
            let rule = format!("/{}/.magnesium/**", folder.display());
            [format!("Write({rule})"), format!("Edit({rule})")]
        })
        .collect();
    let settings = json!({
        "sandbox": {
            "enabled": true,
            "autoAllowBashIfSandboxed": false,
            "allowUnsandboxedCommands": false,
        },
        "permissions": {"deny": deny},
        "enableAllProjectMcpServers": false,
    });
    args.push("--settings".into());
    args.push(settings.to_string());
    args
}

/// Start Claude Code so that the learner's choice of payment holds. Started
/// by another program, Claude Code uses an API key in its environment over
/// the learner's plan, and does not ask. With the plan chosen, it gets none.
/// Keys Claude Code keeps itself, such as a Console sign-in, stay as they
/// are: `account` reports them, so the window never claims the plan wrongly.
fn apply_payment(command: &mut std::process::Command, payment: Payment) {
    if payment == Payment::Plan {
        command.env_remove("ANTHROPIC_API_KEY");
        command.env_remove("ANTHROPIC_AUTH_TOKEN");
    }
}

/// What `claude auth status --json` says, in the window's words.
fn read_account(status: &Value, payment: Payment) -> Account {
    let mut account = Account {
        name: NAME.into(),
        installed: true,
        ..Account::default()
    };
    account.signed_in = status["loggedIn"].as_bool() == Some(true);
    if !account.signed_in {
        account.problem = Some(
            match payment {
                Payment::Plan => "Claude Code is not signed in. Sign in with your Claude account.",
                Payment::ApiKey => {
                    "Claude Code is not signed in. Sign in with an Anthropic Console account, which pays per use."
                }
            }
            .into(),
        );
        return account;
    }
    let text = |key: &str| {
        status[key]
            .as_str()
            .map(str::trim)
            .filter(|t| !t.is_empty())
    };
    let provider = text("apiProvider").unwrap_or("firstParty");
    let plan =
        provider == "firstParty" && matches!(text("authMethod"), Some("claude.ai" | "oauth_token"));
    if plan {
        account.paid_with = Some(Payment::Plan);
        let tier = match text("subscriptionType") {
            Some("max") => "your Claude Max plan".to_owned(),
            Some("pro") => "your Claude Pro plan".to_owned(),
            Some("team") => "your Claude Team plan".to_owned(),
            Some("enterprise") => "your Claude Enterprise plan".to_owned(),
            _ => "your Claude plan".to_owned(),
        };
        account.detail = Some(tier);
        if text("subscriptionType") == Some("free") {
            account.problem = Some(
                "Your Claude account has no paid plan, and Claude Code needs one: Pro or Max, or an Anthropic Console account that pays per use."
                    .into(),
            );
            return account;
        }
    } else {
        account.paid_with = Some(Payment::ApiKey);
        account.detail = Some(if provider != "firstParty" {
            format!("your {provider} account, per use")
        } else {
            key_words(text("apiKeySource"))
        });
    }
    account.problem = match (payment, account.paid_with) {
        (Payment::Plan, Some(Payment::ApiKey)) => Some(format!(
            "Claude Code would pay with {}, not your Claude plan. Sign in with your Claude account to use the plan.",
            account.detail.as_deref().unwrap_or("an API key")
        )),
        (Payment::ApiKey, Some(Payment::Plan)) => Some(format!(
            "Claude Code found no API key, so it would use {}. Sign in with an Anthropic Console account to pay per use.",
            account.detail.as_deref().unwrap_or("your Claude plan")
        )),
        _ => None,
    };
    account
}

/// Where an API key came from, in plain words. Never the key itself.
fn key_words(source: Option<&str>) -> String {
    match source {
        Some("ANTHROPIC_API_KEY") => "an API key set in ANTHROPIC_API_KEY, per use".to_owned(),
        Some("ANTHROPIC_AUTH_TOKEN") => "a token set in ANTHROPIC_AUTH_TOKEN, per use".to_owned(),
        Some("apiKeyHelper") => "the API key helper in Claude Code's settings, per use".to_owned(),
        Some("/login managed key") => "your Anthropic Console sign-in, per use".to_owned(),
        Some(other) => format!("an API key ({other}), per use"),
        None => "an API key, per use".to_owned(),
    }
}

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
    fn locate(&self) -> AgentResult<PathBuf> {
        let named = self.program.clone().or_else(|| {
            env::var_os(PROGRAM_ENV)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        });
        match named {
            Some(program) => Some(program).filter(|p| p.is_file()),
            None => paths::find_program("claude"),
        }
        .ok_or_else(|| {
            AgentError(
                "Claude Code is not installed, or the window cannot find it. Install it, then check again from the start screen."
                    .into(),
            )
        })
    }

    fn start(&self, options: StartOptions, sink: EventSink) -> AgentResult<Box<dyn Conversation>> {
        let program = self.locate()?;
        let mut command = paths::command(&program);
        apply_payment(&mut command, options.payment);
        command
            .current_dir(&options.folder)
            .env("PWD", &options.folder)
            .env(SURFACE_ENV, SURFACE)
            // The learner's account connectors (mail, calendar, drive) come
            // from their Claude account, not their settings, so the setting
            // sources alone leave them in: a live check on 2026-11-11 found
            // 62 of their tools in a window lesson without this.
            .env(CONNECTORS_ENV, "false")
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
            .args(lesson_args(&options.folder, plugin_dir().as_deref()))
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
            reply_blocks: Mutex::new(HashMap::new()),
            replies: Mutex::new(Vec::new()),
            folder: options.folder.clone(),
            stderr_tail: Mutex::new(String::new()),
            closing: AtomicBool::new(false),
            interrupting: AtomicBool::new(false),
            awaiting: AtomicBool::new(false),
            in_turn: AtomicBool::new(false),
            ready_sent: AtomicBool::new(false),
            counter: AtomicU64::new(0),
            interrupts: AtomicU64::new(0),
            payment: options.payment,
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

    fn account(&self, payment: Payment) -> Account {
        let Ok(program) = self.locate() else {
            return Account {
                name: NAME.into(),
                problem: Some("Claude Code is not on this Mac, or the window cannot find it. Install it, then check again.".into()),
                install_page: Some(INSTALL_PAGE.into()),
                ..Account::default()
            };
        };
        // A short question: the PATH a terminal has is not needed, and the
        // start screen does not wait for the shell to give it.
        let mut command = paths::light_command(&program);
        apply_payment(&mut command, payment);
        command.args(["auth", "status", "--json"]);
        // Signed out, it ends with 1 and still prints its answer.
        let answer =
            paths::run_to_end(command, Duration::from_secs(20)).and_then(|(_, out, err)| {
                serde_json::from_str::<Value>(out.trim()).map_err(|_| {
                    let said = err
                        .lines()
                        .rev()
                        .find(|l| !l.trim().is_empty())
                        .unwrap_or("it gave no answer");
                    clip(said, 200)
                })
            });
        match answer {
            Ok(status) => read_account(&status, payment),
            Err(why) => Account {
                name: NAME.into(),
                installed: true,
                problem: Some(format!("Claude Code could not say whether you are signed in ({why}). It may be too old: update it, then check again.")),
                ..Account::default()
            },
        }
    }

    fn sign_in(&self, payment: Payment) -> AgentResult<()> {
        let program = self.locate()?;
        let mut command = paths::light_command(&program);
        apply_payment(&mut command, payment);
        command
            .args([
                "auth",
                "login",
                match payment {
                    Payment::Plan => "--claudeai",
                    Payment::ApiKey => "--console",
                },
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = command.spawn().map_err(|e| {
            AgentError(format!(
                "The window could not start Claude Code's sign-in: {e}."
            ))
        })?;
        // It opens Anthropic's page and waits for the browser to report
        // back. It is left to finish, and stopped if nobody ever does.
        thread::spawn(move || {
            let started = Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(_)) | Err(_) => return,
                    Ok(None) if started.elapsed() > SIGN_IN_LIMIT => {
                        let _ = child.kill();
                        let _ = child.wait();
                        return;
                    }
                    Ok(None) => thread::sleep(Duration::from_millis(250)),
                }
            }
        });
        Ok(())
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
    /// The `reply` calls being written, by their block's index.
    reply_blocks: Mutex<HashMap<u64, ReplyStream>>,
    /// The replies already shown this turn. Text written besides them is held
    /// back until it is whole, and shown only when it says something new.
    replies: Mutex<Vec<String>>,
    /// The study folder, to name the files the tutor works on.
    folder: PathBuf,
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
    /// How the learner chose to pay, to check against what the agent uses.
    payment: Payment,
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
    "Claude Code is signed out. Press ⌘K and choose Sign in to Claude Code, then send your message again.";

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
        if let Some(text) = payment_notice(shared.payment, value) {
            shared.emit(AgentEvent::Notice {
                kind: NoticeKind::SignIn,
                text,
            });
        }
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

/// A lesson paid another way than the learner chose, in one line. Claude
/// Code's first report names the key it uses, or "none" for the plan.
fn payment_notice(payment: Payment, init: &Value) -> Option<String> {
    let source = init["apiKeySource"].as_str()?;
    let plan = source == "none";
    match payment {
        Payment::Plan if !plan => Some(format!(
            "This lesson is paid with {}, not your Claude plan. To use the plan, sign in with your Claude account (⌘K, Sign in to Claude Code).",
            key_words(Some(source))
        )),
        Payment::ApiKey if plan => Some(
            "This lesson uses your Claude plan, because Claude Code found no API key. To pay per use, sign in with an Anthropic Console account (⌘K, Sign in to Claude Code)."
                .into(),
        ),
        _ => None,
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
    let index = event["index"].as_u64().unwrap_or(0);
    match event["type"].as_str() {
        Some("message_start") => {
            lock(&shared.reply_blocks).clear();
            begin_turn(shared);
        }
        Some("content_block_start") => match event["content_block"]["type"].as_str() {
            Some("text") | None => {}
            Some("tool_use") if is_reply_tool(event["content_block"]["name"].as_str()) => {
                begin_turn(shared);
                lock(&shared.reply_blocks).insert(index, ReplyStream::default());
            }
            // Each step is named when it starts, from the tool's name alone,
            // so the line moves while a long record is still being written.
            // The finished block names the file, where there is one.
            Some("tool_use" | "server_tool_use") => {
                begin_turn(shared);
                shared.emit(AgentEvent::Working {
                    doing: event["content_block"]["name"]
                        .as_str()
                        .and_then(|tool| doing(tool, &Value::Null, &shared.folder)),
                });
            }
            Some("thinking" | "redacted_thinking") => {
                begin_turn(shared);
                shared.emit(AgentEvent::Working {
                    doing: Some(THINKING.into()),
                });
            }
            Some(_) => shared.emit(AgentEvent::Working { doing: None }),
        },
        Some("content_block_delta") => match event["delta"]["type"].as_str() {
            Some("text_delta") => {
                // Once the tutor has replied through the tool, other text
                // waits until it is whole, so the reply is never shown twice.
                if !lock(&shared.replies).is_empty() {
                    return;
                }
                if let Some(text) = event["delta"]["text"].as_str().filter(|t| !t.is_empty()) {
                    begin_turn(shared);
                    shared.emit(AgentEvent::TextDelta {
                        text: text.to_owned(),
                    });
                }
            }
            Some("input_json_delta") => {
                let more = event["delta"]["partial_json"].as_str().unwrap_or_default();
                let fresh = lock(&shared.reply_blocks)
                    .get_mut(&index)
                    .map(|stream| stream.push(more))
                    .unwrap_or_default();
                if !fresh.is_empty() {
                    shared.emit(AgentEvent::TextDelta { text: fresh });
                }
            }
            _ => {}
        },
        _ => {}
    }
}

/// The work a tool call does, in a few plain words for the line the lesson
/// shows while the tutor works, or None when the window cannot tell. A file
/// is named only when it is inside the study folder and outside
/// `.magnesium/`; a tool's name, a command and an outside path never are.
pub fn doing(tool: &str, input: &Value, folder: &Path) -> Option<String> {
    let file = || {
        input["file_path"]
            .as_str()
            .or_else(|| input["path"].as_str())
            .and_then(|path| file_words(path, folder))
    };
    let magnesium = |name: &str| tool.starts_with("mcp__") && tool.ends_with(&format!("__{name}"));
    let line = match tool {
        "Write" | "Edit" | "MultiEdit" => match file() {
            Some(name) => format!("writing {name}"),
            None => "writing a file".into(),
        },
        "Read" => match file() {
            Some(name) => format!("reading {name}"),
            None => "reading".into(),
        },
        "Grep" | "Glob" => "looking through your files".into(),
        "WebSearch" => "searching the web".into(),
        "WebFetch" => "reading a web page".into(),
        "Bash" => {
            let command = input["command"].as_str().unwrap_or_default();
            if ["latexmk", "pdflatex", "xelatex", "lualatex", "build.sh"]
                .iter()
                .any(|word| command.contains(word))
            {
                "building the papers".into()
            } else {
                return None;
            }
        }
        _ if magnesium("search_sources") => "searching your course material".into(),
        _ if magnesium("sources") => "reading your course material".into(),
        _ if magnesium("context") || magnesium("memory") => "reading your record".into(),
        _ if magnesium("learning_step") => "saving your work".into(),
        _ => return None,
    };
    Some(line)
}

/// A file's name in plain words, as "paper 2" for `Study/stats/paper-2.md`,
/// when the file is in the study folder and outside `.magnesium/`.
fn file_words(path: &str, folder: &Path) -> Option<String> {
    let path = Path::new(path);
    let inside = |root: &Path| path.strip_prefix(root).ok().map(Path::to_path_buf);
    let relative = if path.is_absolute() {
        inside(folder).or_else(|| {
            std::fs::canonicalize(folder)
                .ok()
                .and_then(|real| inside(&real))
        })?
    } else {
        path.to_path_buf()
    };
    if relative.components().any(|part| {
        matches!(part, std::path::Component::ParentDir)
            || part.as_os_str().to_string_lossy().starts_with('.')
    }) {
        return None;
    }
    let stem = relative.file_stem()?.to_string_lossy();
    let words = stem
        .split(|c: char| c == '-' || c == '_' || c == '.' || c.is_whitespace())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    (!words.is_empty() && words.len() <= 60).then_some(words)
}

/// The line while the tutor thinks, before it starts any step.
const THINKING: &str = "thinking";

/// Magnesium's `reply` tool, under whichever name Claude Code gives it:
/// `mcp__plugin_magnesium_magnesium__reply` when it comes from the plugin.
fn is_reply_tool(name: Option<&str>) -> bool {
    name.is_some_and(|name| {
        name.starts_with("mcp__") && name.contains(PLUGIN) && name.ends_with("__reply")
    })
}

/// A `reply` call as it streams: the input so far, and how much of its text
/// is already on screen.
#[derive(Default)]
struct ReplyStream {
    input: String,
    shown: usize,
}

impl ReplyStream {
    /// Take more of the call's input, and give back the text not yet shown.
    fn push(&mut self, more: &str) -> String {
        self.input.push_str(more);
        let text = partial_text(&self.input);
        let fresh = text.get(self.shown..).unwrap_or_default().to_owned();
        self.shown = text.len();
        fresh
    }
}

/// The `text` field of a JSON object that is still being written, decoded as
/// far as it is whole. An escape cut off at the end waits for the rest.
pub fn partial_text(input: &str) -> String {
    let Some(key) = input.find("\"text\"") else {
        return String::new();
    };
    let rest = input[key + 6..].trim_start();
    let Some(rest) = rest.strip_prefix(':') else {
        return String::new();
    };
    let Some(body) = rest.trim_start().strip_prefix('"') else {
        return String::new();
    };
    let mut text = String::new();
    let mut chars = body.chars().peekable();
    let mut pending_high: Option<u32> = None;
    while let Some(c) = chars.next() {
        match c {
            '"' => break,
            '\\' => {
                let Some(escape) = chars.next() else { break };
                let decoded = match escape {
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    'b' => '\u{8}',
                    'f' => '\u{c}',
                    'u' => {
                        let hex: String = chars.by_ref().take(4).collect();
                        let Ok(unit) =
                            u32::from_str_radix(&hex, 16).map_err(|_| ()).and_then(|u| {
                                if hex.len() == 4 {
                                    Ok(u)
                                } else {
                                    Err(())
                                }
                            })
                        else {
                            break;
                        };
                        if (0xD800..0xDC00).contains(&unit) {
                            pending_high = Some(unit);
                            continue;
                        }
                        let code = match pending_high.take() {
                            Some(high) if (0xDC00..0xE000).contains(&unit) => {
                                0x10000 + ((high - 0xD800) << 10) + (unit - 0xDC00)
                            }
                            _ => unit,
                        };
                        char::from_u32(code).unwrap_or('\u{fffd}')
                    }
                    other => other,
                };
                text.push(decoded);
            }
            c => text.push(c),
        }
    }
    text
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
                // Text that repeats a reply sent through the tool is not
                // shown again.
                if lock(&shared.replies)
                    .iter()
                    .any(|reply| reply.contains(text.trim()))
                {
                    continue;
                }
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
            Some("tool_use") if is_reply_tool(block["name"].as_str()) => {
                let Some(text) = block["input"]["text"]
                    .as_str()
                    .filter(|t| !t.trim().is_empty())
                else {
                    continue;
                };
                begin_turn(shared);
                let mut replies = lock(&shared.replies);
                if !replies.iter().any(|said| said == text) {
                    replies.push(text.to_owned());
                    drop(replies);
                    shared.emit(AgentEvent::TextDone {
                        text: text.to_owned(),
                    });
                }
            }
            Some("tool_use" | "server_tool_use") => {
                begin_turn(shared);
                shared.emit(AgentEvent::Working {
                    doing: block["name"]
                        .as_str()
                        .and_then(|tool| doing(tool, &block["input"], &shared.folder)),
                });
            }
            Some("thinking") => {
                begin_turn(shared);
                shared.emit(AgentEvent::Working { doing: None });
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
    shared.in_turn.store(false, Ordering::SeqCst);
    shared.awaiting.store(false, Ordering::SeqCst);
    lock(&shared.seen_text).clear();
    lock(&shared.replies).clear();
    lock(&shared.reply_blocks).clear();
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
        // An interrupt marked just after the last turn ended belongs to no
        // turn, and must not mark this one.
        self.shared.interrupting.store(false, Ordering::SeqCst);
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

    fn close(&mut self) {
        self.shared.closing.store(true, Ordering::SeqCst);
        // Closing the input does not stop a reply under way, so the reply is
        // interrupted first.
        if self.shared.turn_active() {
            self.shared.interrupting.store(true, Ordering::SeqCst);
            let id = self.shared.next_id();
            let _ = self.shared.write(&json!({
                "type": "control_request",
                "request_id": id,
                "request": {"subtype": "interrupt", "cancel_queued": true},
            }));
        }
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
    fn the_account_is_read_from_claude_codes_own_words() {
        let plan = read_account(
            &json!({"loggedIn": true, "authMethod": "claude.ai", "apiProvider": "firstParty", "subscriptionType": "pro"}),
            Payment::Plan,
        );
        assert_eq!(plan.paid_with, Some(Payment::Plan));
        assert_eq!(plan.detail.as_deref(), Some("your Claude Pro plan"));
        assert_eq!(plan.problem, None);
        // The plan in use when the learner chose an API key.
        let wanted_key = read_account(
            &json!({"loggedIn": true, "authMethod": "claude.ai", "subscriptionType": "max"}),
            Payment::ApiKey,
        );
        assert!(wanted_key.problem.unwrap().contains("found no API key"));
        // A Console sign-in pays per use.
        let console = read_account(
            &json!({"loggedIn": true, "authMethod": "api_key", "apiKeySource": "/login managed key"}),
            Payment::Plan,
        );
        assert_eq!(console.paid_with, Some(Payment::ApiKey));
        assert!(console.detail.unwrap().contains("Console"));
        // Signed out, and a free account.
        let out = read_account(
            &json!({"loggedIn": false, "authMethod": "none"}),
            Payment::ApiKey,
        );
        assert!(!out.signed_in && out.problem.unwrap().contains("Console"));
        let free = read_account(
            &json!({"loggedIn": true, "authMethod": "claude.ai", "subscriptionType": "free"}),
            Payment::Plan,
        );
        assert!(free.problem.unwrap().contains("no paid plan"));
    }

    #[test]
    fn a_lesson_paid_another_way_than_chosen_is_named() {
        let plan = json!({"apiKeySource": "none"});
        let key = json!({"apiKeySource": "ANTHROPIC_API_KEY"});
        assert_eq!(payment_notice(Payment::Plan, &plan), None);
        assert_eq!(payment_notice(Payment::ApiKey, &key), None);
        assert!(payment_notice(Payment::Plan, &key)
            .unwrap()
            .contains("ANTHROPIC_API_KEY"));
        assert!(payment_notice(Payment::ApiKey, &plan)
            .unwrap()
            .contains("Claude plan"));
        // An older Claude Code that does not say is not guessed at.
        assert_eq!(payment_notice(Payment::Plan, &json!({})), None);
    }

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
    fn an_interrupt_that_came_too_late_does_not_mark_the_next_turn() {
        let mut child = std::process::Command::new("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&seen);
        let shared = Arc::new(Shared {
            sink: Arc::new(move |event| lock(&kept).push(event)),
            stdin: Mutex::new(child.stdin.take()),
            child: Mutex::new(None),
            session_id: Mutex::new(None),
            pending: Mutex::new(HashMap::new()),
            allowed: Mutex::new(HashSet::new()),
            seen_text: Mutex::new(HashMap::new()),
            reply_blocks: Mutex::new(HashMap::new()),
            replies: Mutex::new(Vec::new()),
            folder: PathBuf::from("/study"),
            stderr_tail: Mutex::new(String::new()),
            closing: AtomicBool::new(false),
            interrupting: AtomicBool::new(false),
            awaiting: AtomicBool::new(false),
            in_turn: AtomicBool::new(false),
            ready_sent: AtomicBool::new(false),
            counter: AtomicU64::new(0),
            interrupts: AtomicU64::new(0),
            payment: Payment::Plan,
            trace: None,
            started: Instant::now(),
        });
        let mut conversation = ClaudeConversation {
            shared: Arc::clone(&shared),
        };
        // The last turn ended just before the interrupt was marked.
        shared.interrupting.store(true, Ordering::SeqCst);
        conversation.send("next").unwrap();
        on_result(
            &shared,
            &json!({"type": "result", "subtype": "error_during_execution", "is_error": true, "result": "API Error: overloaded"}),
        );
        let ended = lock(&seen).last().cloned();
        assert_eq!(
            ended,
            Some(AgentEvent::TurnEnded {
                ok: false,
                interrupted: false,
                error: Some("API Error: overloaded".into()),
            })
        );
        conversation.close();
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn a_session_id_cannot_be_a_flag() {
        assert!(is_session_id("0b9f1c1e-7a52-4f6e-9a57-3f1b8d2c4e10"));
        assert!(!is_session_id("--dangerously-skip-permissions"));
        assert!(!is_session_id("a b"));
        assert!(!is_session_id(""));
    }

    #[test]
    fn a_reply_streams_from_its_unfinished_input() {
        let whole = r#"{"text": "Right: $\\frac{3}{4}$.\n\n**Task.** Say \"why\" é 😀"}"#;
        let decoded = "Right: $\\frac{3}{4}$.\n\n**Task.** Say \"why\" é 😀";
        assert_eq!(partial_text(whole), decoded);
        // Fed a few bytes at a time, the pieces add up to the whole text, and
        // an escape cut in two waits for its other half.
        let mut stream = ReplyStream::default();
        let mut shown = String::new();
        let bytes = whole.as_bytes();
        let mut start = 0;
        while start < bytes.len() {
            let mut end = (start + 3).min(bytes.len());
            while !whole.is_char_boundary(end) {
                end += 1;
            }
            shown.push_str(&stream.push(&whole[start..end]));
            assert!(decoded.starts_with(&shown), "{shown:?}");
            start = end;
        }
        assert_eq!(shown, decoded);
        assert_eq!(partial_text(r#"{"te"#), "");
        assert_eq!(partial_text(r#"{"text": "a\"#), "a");
        assert_eq!(partial_text(r#"{"text": "a\u00"#), "a");
    }

    #[test]
    fn the_reply_tool_is_known_by_its_plugin_name() {
        assert!(is_reply_tool(Some(
            "mcp__plugin_magnesium_magnesium__reply"
        )));
        assert!(!is_reply_tool(Some("mcp__other__reply")));
        assert!(!is_reply_tool(Some(
            "mcp__plugin_magnesium_magnesium__learning_step"
        )));
        assert!(!is_reply_tool(None));
    }

    #[test]
    fn the_working_line_names_the_work_and_never_a_tool_command_or_outside_path() {
        let folder = Path::new("/study/stats");
        let line = |tool: &str, input: Value| doing(tool, &input, folder);
        assert_eq!(
            line(
                "Write",
                json!({"file_path": "/study/stats/Study/midterm/paper-2.md"})
            )
            .as_deref(),
            Some("writing paper 2")
        );
        assert_eq!(
            line(
                "Read",
                json!({"file_path": "/study/stats/week4-slides.pdf"})
            )
            .as_deref(),
            Some("reading week4 slides")
        );
        assert_eq!(
            line("Read", json!({"file_path": "Study/midterm/answer_key.md"})).as_deref(),
            Some("reading answer key")
        );
        // Outside the study folder, in .magnesium, or up and out: no name.
        for path in [
            "/Users/me/secret-plans.md",
            "/study/stats/.magnesium/config.toml",
            "/study/stats/../other/x.md",
        ] {
            assert_eq!(
                line("Read", json!({"file_path": path})).as_deref(),
                Some("reading"),
                "{path}"
            );
        }
        assert_eq!(
            line(
                "Bash",
                json!({"command": "cd Study && latexmk -pdf paper-2.tex"})
            )
            .as_deref(),
            Some("building the papers")
        );
        assert_eq!(
            line(
                "Bash",
                json!({"command": "rm -rf /tmp/x", "description": "Clean up"})
            ),
            None
        );
        assert_eq!(
            line("mcp__plugin_magnesium_magnesium__search_sources", json!({})).as_deref(),
            Some("searching your course material")
        );
        assert_eq!(line("mcp__other__thing", json!({})), None);
        for tool in [
            "Write",
            "Read",
            "Grep",
            "WebSearch",
            "WebFetch",
            "Bash",
            "mcp__plugin_magnesium_magnesium__learning_step",
        ] {
            let words = line(
                tool,
                json!({"file_path": "/study/stats/a.md", "command": "latexmk"}),
            )
            .unwrap();
            assert!(
                !words.contains(tool) && !words.contains("mcp") && !words.contains("latexmk"),
                "{words}"
            );
        }
    }

    #[test]
    fn a_lesson_loads_the_nine_tools_magnesiums_plugin_and_nothing_of_the_learners() {
        let folder = Path::new("/study/stats");
        let plugin = Path::new("/home/me/Library/Application Support/magnesium/claude-plugin");
        let args = lesson_args(folder, Some(plugin));
        let after = |flag: &str| {
            let at = args
                .iter()
                .position(|a| a == flag)
                .unwrap_or_else(|| panic!("{flag} in {args:?}"));
            args[at + 1].clone()
        };
        assert_eq!(after("--setting-sources"), "project,local");
        assert_eq!(
            after("--tools"),
            "Read,Write,Edit,Glob,Grep,AskUserQuestion,WebSearch,WebFetch,Bash"
        );
        assert_eq!(after("--model"), MODEL);
        assert!(
            !args.iter().any(|a| a == "--effort"),
            "a lesson thinks as in a terminal"
        );
        assert_eq!(after("--plugin-dir"), plugin.display().to_string());
        let settings: Value = serde_json::from_str(&after("--settings")).unwrap();
        assert_eq!(settings["sandbox"]["enabled"], true);
        assert_eq!(settings["sandbox"]["allowUnsandboxedCommands"], false);
        assert_eq!(settings["sandbox"]["autoAllowBashIfSandboxed"], false);
        let deny: Vec<&str> = settings["permissions"]["deny"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert!(
            deny.contains(&"Write(//study/stats/.magnesium/**)"),
            "{deny:?}"
        );
        assert!(
            deny.contains(&"Edit(//study/stats/.magnesium/**)"),
            "{deny:?}"
        );
        assert!(!args.iter().any(|a| a == "--bare"));
        assert!(!lesson_args(folder, None)
            .iter()
            .any(|a| a == "--plugin-dir"));
    }

    /// A conversation with no program behind it, and the events it shows.
    fn listening() -> (Arc<Shared>, Arc<Mutex<Vec<AgentEvent>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&seen);
        let shared = Arc::new(Shared {
            sink: Arc::new(move |event| lock(&kept).push(event)),
            stdin: Mutex::new(None),
            child: Mutex::new(None),
            session_id: Mutex::new(None),
            pending: Mutex::new(HashMap::new()),
            allowed: Mutex::new(HashSet::new()),
            seen_text: Mutex::new(HashMap::new()),
            reply_blocks: Mutex::new(HashMap::new()),
            replies: Mutex::new(Vec::new()),
            folder: PathBuf::from("/study"),
            stderr_tail: Mutex::new(String::new()),
            closing: AtomicBool::new(false),
            interrupting: AtomicBool::new(false),
            awaiting: AtomicBool::new(false),
            in_turn: AtomicBool::new(false),
            ready_sent: AtomicBool::new(false),
            counter: AtomicU64::new(0),
            interrupts: AtomicU64::new(0),
            payment: Payment::Plan,
            trace: None,
            started: Instant::now(),
        });
        (shared, seen)
    }

    #[test]
    fn the_window_shows_a_reply_from_the_tool_as_it_is_written_and_only_once() {
        let (shared, seen) = listening();
        let stream = |event: Value| {
            handle(
                &shared,
                &json!({"type": "stream_event", "parent_tool_use_id": null, "event": event}),
            )
        };
        let name = "mcp__plugin_magnesium_magnesium__reply";
        stream(json!({"type": "message_start"}));
        stream(
            json!({"type": "content_block_start", "index": 1, "content_block": {"type": "tool_use", "name": name, "input": {}}}),
        );
        for piece in [
            r#"{"te"#,
            r#"xt": "Two"#,
            r#" thirds.\n\n**Task.**"#,
            r#" Why?"}"#,
        ] {
            stream(
                json!({"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": piece}}),
            );
        }
        let reply = "Two thirds.\n\n**Task.** Why?";
        handle(
            &shared,
            &json!({"type": "assistant", "parent_tool_use_id": null, "message": {"id": "m1", "content": [
            {"type": "tool_use", "name": name, "input": {"text": reply}}]}}),
        );
        // The tutor writes the same words again as ordinary text: nothing new
        // is shown, not even while it streams.
        stream(json!({"type": "message_start"}));
        stream(
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text"}}),
        );
        stream(
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Two thirds."}}),
        );
        handle(
            &shared,
            &json!({"type": "assistant", "parent_tool_use_id": null, "message": {"id": "m2", "content": [
            {"type": "text", "text": "Two thirds."}]}}),
        );
        // Text that says something new is still shown, whole.
        handle(
            &shared,
            &json!({"type": "assistant", "parent_tool_use_id": null, "message": {"id": "m2", "content": [
            {"type": "text", "text": "(Saved.)"}]}}),
        );
        let events = lock(&seen).clone();
        let deltas: String = events
            .iter()
            .filter_map(|e| match e {
                AgentEvent::TextDelta { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(deltas, reply, "{events:?}");
        let done: Vec<&str> = events
            .iter()
            .filter_map(|e| match e {
                AgentEvent::TextDone { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(done, [reply, "(Saved.)"]);
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, AgentEvent::Working { .. })),
            "{events:?}"
        );
        // The next turn starts with no replies remembered.
        on_result(&shared, &json!({"type": "result", "subtype": "success"}));
        assert!(lock(&shared.replies).is_empty());
    }

    #[test]
    fn each_step_is_named_when_it_starts() {
        let (shared, seen) = listening();
        let stream = |event: Value| {
            handle(
                &shared,
                &json!({"type": "stream_event", "parent_tool_use_id": null, "event": event}),
            )
        };
        stream(json!({"type": "message_start"}));
        stream(
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking"}}),
        );
        stream(
            json!({"type": "content_block_start", "index": 1, "content_block": {"type": "tool_use", "name": "mcp__plugin_magnesium_magnesium__learning_step", "input": {}}}),
        );
        stream(
            json!({"type": "content_block_start", "index": 2, "content_block": {"type": "tool_use", "name": "Bash", "input": {}}}),
        );
        let lines: Vec<Option<String>> = lock(&seen)
            .iter()
            .filter_map(|e| match e {
                AgentEvent::Working { doing } => Some(doing.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            lines,
            [
                Some("thinking".to_owned()),
                Some("saving your work".to_owned()),
                None
            ]
        );
    }
}
