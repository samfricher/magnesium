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
    EventSink, Harness, Intent, NoticeKind, Payment, Permissions, Question, StartOptions,
};
use crate::paths;

pub const PROGRAM_ENV: &str = "MAGNESIUM_APP_AGENT";

pub const TRACE_ENV: &str = "MAGNESIUM_APP_TRACE";

const PLUGIN: &str = "magnesium";
const AUTO: &str = "auto";
const NAME: &str = "Claude Code";
const INSTALL_PAGE: &str = "https://code.claude.com/docs/en/setup";
const SIGN_IN_LIMIT: Duration = Duration::from_secs(15 * 60);

pub const SURFACE_ENV: &str = "MAGNESIUM_SURFACE";
const SURFACE: &str = "window";
pub const DRAWS: &str = "This lesson is shown in Magnesium's study window, which draws your replies as Markdown and draws mathematics with KaTeX. Write every formula in TeX, even a short one inside a sentence: inline as `$\\frac{1}{1+e^{-s}}$` (no space after the opening dollar), display as `$$...$$` on its own lines. Maths written as plain text, such as 1/(1 + e^(−s)), is shown as typed, never as a formula. A diagram is a fenced `mermaid` block.";

pub const CONNECTORS_ENV: &str = "ENABLE_CLAUDEAI_MCP_SERVERS";

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

pub(crate) fn push_pick(args: &mut Vec<String>, model: Option<&str>, effort: Option<&str>) {
    if let Some(model) = model.filter(|m| crate::models::usable_model(m)) {
        args.push("--model".into());
        args.push(model.into());
    }
    if let Some(effort) = effort.filter(|e| crate::models::usable_effort(e)) {
        args.push("--effort".into());
        args.push(effort.into());
    }
}

fn plugin_dir() -> Option<PathBuf> {
    let dir = paths::home().join("Library/Application Support/magnesium/claude-plugin");
    dir.join(".claude-plugin/plugin.json")
        .is_file()
        .then_some(dir)
}

pub fn lesson_args(
    folder: &Path,
    plugin: Option<&Path>,
    model: Option<&str>,
    effort: Option<&str>,
    permissions: Permissions,
) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "--setting-sources".into(),
        "project,local".into(),
        "--tools".into(),
        TOOLS.join(","),
        "--append-system-prompt".into(),
        DRAWS.into(),
    ];
    if permissions == Permissions::Auto {
        args.push("--permission-mode".into());
        args.push(AUTO.into());
    }
    push_pick(&mut args, model, effort);
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
    let root = crate::direct::study_root(folder);
    let mut roots = vec![root.clone()];
    if let Ok(real) = std::fs::canonicalize(&root) {
        if real != root {
            roots.push(real);
        }
    }
    let deny: Vec<String> = folders
        .iter()
        .chain(roots.iter().filter(|root| !folders.contains(root)))
        .flat_map(|folder| {
            let rule = format!("/{}/.magnesium/**", folder.display());
            [format!("Write({rule})"), format!("Edit({rule})")]
        })
        .collect();
    let allow: Vec<String> = roots
        .iter()
        .flat_map(|root| crate::direct::plan_rules(root))
        .collect();
    let settings = json!({
        "sandbox": {
            "enabled": true,
            "autoAllowBashIfSandboxed": false,
            "allowUnsandboxedCommands": false,
        },
        "permissions": {"deny": deny, "allow": allow},
        "enableAllProjectMcpServers": false,
    });
    args.push("--settings".into());
    args.push(settings.to_string());
    args
}

pub(crate) fn apply_payment(command: &mut std::process::Command, payment: Payment) {
    if payment == Payment::Plan {
        command.env_remove("ANTHROPIC_API_KEY");
        command.env_remove("ANTHROPIC_AUTH_TOKEN");
    }
}

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
            .args(lesson_args(
                &options.folder,
                plugin_dir().as_deref(),
                options.model.as_deref(),
                options.effort.as_deref(),
                options.permissions,
            ))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(id) = options.resume.as_deref().filter(|id| is_session_id(id)) {
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
            held: Mutex::new(Vec::new()),
            held_writing: Mutex::new(String::new()),
            replied: AtomicBool::new(false),
            reply_tool: AtomicBool::new(true),
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
            permissions: Mutex::new(options.permissions),
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
            asked: crate::models::Asked::default(),
            failed_code: Mutex::new(None),
            opening: Opening {
                new: options.resume.is_none(),
                effort: Mutex::new(options.effort.clone().filter(|_| options.resume.is_none())),
                ..Opening::default()
            },
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

        let id = shared.next_id();
        *lock(&shared.opening.initialize) = Some(id.clone());
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

    fn models(&self, payment: Payment) -> AgentResult<Option<Vec<crate::models::ModelOption>>> {
        let program = self.locate()?;
        let mut command = paths::command(&program);
        apply_payment(&mut command, payment);
        let folder = env::temp_dir();
        command
            .current_dir(&folder)
            .env("PWD", &folder)
            .env(CONNECTORS_ENV, "false")
            .args([
                "-p",
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--verbose",
                "--setting-sources",
                "project,local",
                "--no-session-persistence",
            ]);
        crate::models::list(command)
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
        let mut command = paths::light_command(&program);
        apply_payment(&mut command, payment);
        command.args(["auth", "status", "--json"]);
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

pub(crate) fn is_session_id(id: &str) -> bool {
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
    allowed: Mutex<HashSet<String>>,
    seen_text: Mutex<HashMap<String, Vec<String>>>,
    reply_blocks: Mutex<HashMap<u64, ReplyStream>>,
    replies: Mutex<Vec<String>>,
    held: Mutex<Vec<String>>,
    held_writing: Mutex<String>,
    replied: AtomicBool,
    reply_tool: AtomicBool,
    folder: PathBuf,
    stderr_tail: Mutex<String>,
    closing: AtomicBool,
    interrupting: AtomicBool,
    awaiting: AtomicBool,
    in_turn: AtomicBool,
    ready_sent: AtomicBool,
    counter: AtomicU64,
    interrupts: AtomicU64,
    payment: Payment,
    permissions: Mutex<Permissions>,
    trace: Option<Mutex<std::fs::File>>,
    started: Instant,
    opening: Opening,
    asked: crate::models::Asked,
    failed_code: Mutex<Option<String>>,
}

#[derive(Default)]
struct Opening {
    initialize: Mutex<Option<String>>,
    new: bool,
    effort: Mutex<Option<String>>,
    model: Mutex<Option<String>>,
    models: Mutex<Vec<crate::models::ModelOption>>,
    looked: AtomicBool,
    record_opened: AtomicBool,
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

    fn trace(&self, direction: &str, line: &str) {
        if let Some(file) = &self.trace {
            let ms = self.started.elapsed().as_millis();
            let _ = writeln!(lock(file), "{direction} {ms} {}", line.trim_end());
        }
    }

    fn turn_active(&self) -> bool {
        self.awaiting.load(Ordering::SeqCst) || self.in_turn.load(Ordering::SeqCst)
    }

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

    fn ask(&self, request: Value) -> Result<Value, String> {
        self.asked
            .ask(self.next_id(), request, |line| self.write(line))
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
        if let Ok(value) = serde_json::from_str::<Value>(line) {
            handle(shared, &value);
        }
    }

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
    shared.asked.drop_all();
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
        release_held(shared);
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

pub(crate) const SIGN_IN: &str =
    "Claude Code is signed out. Sign in, then send your message again.";

pub(crate) fn looks_signed_out(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    text.contains("/login")
        || text.contains("invalid api key")
        || text.contains("not logged in")
        || text.contains("authentication_error")
        || text.contains("oauth token")
        || text.contains("failed to authenticate")
        || text.contains("oauth session")
}

pub(crate) fn ended_signed_out(result: &Value, said: &str) -> bool {
    result["api_error_status"].as_i64() == Some(401) || looks_signed_out(said)
}

pub(crate) fn is_own_error(value: &Value) -> bool {
    value["is_api_error_message"].as_bool() == Some(true) || value["error"].is_string()
}

pub(crate) const BILLING: &str = "Claude Code reports a billing problem on your account. Fix it in a terminal or on the web, then send your message again.";

pub(crate) fn failure_notice(
    code: Option<&str>,
    said: &str,
    models: &[crate::models::ModelOption],
) -> Option<(NoticeKind, String)> {
    match code? {
        "model_not_found" => Some((
            NoticeKind::ModelUnavailable,
            unavailable_words(said, models),
        )),
        "overloaded" | "server_error" => Some((
            NoticeKind::Turn,
            "Claude couldn’t answer just now. Send your message again in a moment.".into(),
        )),
        "billing_error" | "account_on_hold" => Some((NoticeKind::SignIn, BILLING.into())),
        "verification_required" | "oauth_org_not_allowed" => {
            Some((NoticeKind::SignIn, SIGN_IN.into()))
        }
        _ => None,
    }
}

pub(crate) fn unavailable_words(said: &str, models: &[crate::models::ModelOption]) -> String {
    let named = said
        .split_once("model (")
        .and_then(|(_, rest)| rest.split_once(')'))
        .and_then(|(id, _)| crate::models::find(models, id.trim()))
        .map(|row| row.name.clone());
    match named {
        Some(name) => {
            format!("{name} isn’t available to you. Pick another model, then send your message again.")
        }
        None => "This session’s model isn’t available to you. Pick another model, then send your message again.".into(),
    }
}

pub(crate) fn looks_like_a_limit(text: &str) -> bool {
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
        Some("user") => on_tool_results(shared, value),
        Some("stream_event") => on_stream(shared, value),
        Some("assistant") => on_assistant(shared, value),
        Some("result") => on_result(shared, value),
        Some("rate_limit_event") => on_rate_limit(shared, value),
        Some("control_request") => on_control_request(shared, value),
        Some("control_response") => on_control_response(shared, value),
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

fn on_tool_results(shared: &Arc<Shared>, value: &Value) {
    if value["message"]["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|part| part["type"] == "tool_result")
        .any(|part| says_sitting_ended(&part["content"]))
    {
        shared.emit(AgentEvent::SittingEnded);
    }
}

fn says_sitting_ended(content: &Value) -> bool {
    match content {
        Value::String(text) => {
            serde_json::from_str::<Value>(text).is_ok_and(|parsed| parsed["sitting_ended"] == true)
        }
        Value::Array(parts) => parts.iter().any(|part| says_sitting_ended(&part["text"])),
        _ => false,
    }
}

fn on_control_response(shared: &Arc<Shared>, value: &Value) {
    if shared.asked.deliver(value) {
        return;
    }
    let response = &value["response"];
    let asked = lock(&shared.opening.initialize).clone();
    if asked.is_none() || response["request_id"].as_str() != asked.as_deref() {
        return;
    }
    *lock(&shared.opening.initialize) = None;
    let models = (response["subtype"] == "success")
        .then(|| crate::models::parse(&response["response"]))
        .flatten();
    if let Some(list) = &models {
        *lock(&shared.opening.models) = list.clone();
    }
    shared.emit(AgentEvent::Models { models });
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
    let can_tell = value.get("plugins").is_some() || !commands.is_empty();
    if can_tell && !has_plugin {
        problems.push(
            "The Magnesium plugin did not load, so this would be a plain chat with no teaching rules. In a terminal, run: magnesium update"
                .to_owned(),
        );
    }
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
    let has_tools = tools
        .iter()
        .any(|tool| tool.starts_with("mcp__") && tool.contains(PLUGIN));
    match server_status(value) {
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

fn server_status(init: &Value) -> Option<&str> {
    let servers = init["mcp_servers"].as_array()?;
    let named = |test: &dyn Fn(&str) -> bool| {
        servers
            .iter()
            .find(|s| s["name"].as_str().is_some_and(test))
    };
    named(&|name| name.starts_with("plugin:magnesium:"))
        .or_else(|| named(&|name| name.contains(PLUGIN)))
        .and_then(|s| s["status"].as_str())
}

fn can_reply_by_tool(init: &Value) -> bool {
    init.get("tools").is_none()
        || names(&init["tools"])
            .iter()
            .any(|tool| is_reply_tool(Some(tool)))
        || server_status(init) == Some("pending")
}

fn on_init(shared: &Arc<Shared>, value: &Value) {
    if let Some(id) = value["session_id"].as_str() {
        *lock(&shared.session_id) = Some(id.to_owned());
    }
    shared
        .reply_tool
        .store(can_reply_by_tool(value), Ordering::SeqCst);
    if !shared.ready_sent.swap(true, Ordering::SeqCst) {
        let session_id = lock(&shared.session_id).clone().unwrap_or_default();
        if let Some(text) = payment_notice(shared.payment, value) {
            shared.emit(AgentEvent::Notice {
                kind: NoticeKind::SignIn,
                text,
            });
        }
        if let Some(text) = auto_notice(*lock(&shared.permissions), value) {
            shared.emit(AgentEvent::Notice {
                kind: NoticeKind::Other,
                text,
            });
        }
        let problems = init_problems(value);
        let model = runs_on(value);
        lock(&shared.opening.model).clone_from(&model);
        shared.emit(AgentEvent::Ready {
            session_id,
            plugin_missing: problems
                .iter()
                .any(|problem| problem.contains("plugin did not load")),
            problems,
            model,
        });
    }
}

pub(crate) fn auto_notice(permissions: Permissions, init: &Value) -> Option<String> {
    let mode = init["permissionMode"].as_str()?;
    (permissions == Permissions::Auto && mode != AUTO).then(|| {
        "Claude Code's auto mode is not available for this lesson, so you are asked before the tutor uses a tool.".to_owned()
    })
}

pub(crate) fn runs_on(init: &Value) -> Option<String> {
    init["model"]
        .as_str()
        .filter(|model| crate::models::usable_model(model))
        .map(str::to_owned)
}

pub(crate) fn payment_notice(payment: Payment, init: &Value) -> Option<String> {
    let source = init["apiKeySource"].as_str()?;
    let plan = source == "none";
    match payment {
        Payment::Plan if !plan => Some(format!(
            "This lesson is paid with {}, not your Claude plan. To use the plan, sign in with your Claude account.",
            key_words(Some(source))
        )),
        Payment::ApiKey if plan => Some(
            "This lesson uses your Claude plan, because Claude Code found no API key. To pay per use, sign in with an Anthropic Console account."
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
        "authentication_failed" | "oauth_org_not_allowed" => {
            (NoticeKind::SignIn, SIGN_IN.to_owned())
        }
        "rate_limit" => (
            NoticeKind::UsageLimit,
            "Claude Code is at a usage limit and is waiting to try again.".to_owned(),
        ),
        "billing_error" | "account_on_hold" => (NoticeKind::SignIn, BILLING.to_owned()),
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

pub(crate) fn limit_words(said: &str) -> String {
    let said = clip(said, 200);
    let said = said.trim_end();
    let stop = if said.ends_with(['.', '!', '?']) {
        ""
    } else {
        "."
    };
    format!("{said}{stop} Fix it in a terminal, then send your message again.")
}

fn on_rate_limit(shared: &Arc<Shared>, value: &Value) {
    let info = &value["rate_limit_info"];
    if info["status"].as_str() != Some("rejected") {
        return;
    }
    if info["overageStatus"]
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
    if !is_main_thread(value) {
        return;
    }
    let event = &value["event"];
    let index = event["index"].as_u64().unwrap_or(0);
    match event["type"].as_str() {
        Some("message_start") => {
            lock(&shared.reply_blocks).clear();
            lock(&shared.held_writing).clear();
            begin_turn(shared);
        }
        Some("content_block_start") => match event["content_block"]["type"].as_str() {
            Some("text") => {
                let mut writing = lock(&shared.held_writing);
                if !writing.is_empty() {
                    writing.push_str("\n\n");
                }
            }
            None => {}
            Some("tool_use") if is_reply_tool(event["content_block"]["name"].as_str()) => {
                begin_turn(shared);
                shared.reply_tool.store(true, Ordering::SeqCst);
                lock(&shared.reply_blocks).insert(index, ReplyStream::default());
            }
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
                let Some(text) = event["delta"]["text"].as_str().filter(|t| !t.is_empty()) else {
                    return;
                };
                begin_turn(shared);
                if shared.reply_tool.load(Ordering::SeqCst) {
                    lock(&shared.held_writing).push_str(text);
                } else {
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
                    shared.replied.store(true, Ordering::SeqCst);
                    shared.emit(AgentEvent::TextDelta { text: fresh });
                }
            }
            _ => {}
        },
        _ => {}
    }
}

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
        _ if magnesium("search_sources") => "searching your material".into(),
        _ if magnesium("sources") => "reading your material".into(),
        _ if magnesium("context") || magnesium("memory") => "reading your record".into(),
        _ if magnesium("learning_step") => "saving your work".into(),
        _ => return None,
    };
    Some(line)
}

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

const THINKING: &str = "thinking";

fn is_reply_tool(name: Option<&str>) -> bool {
    name.is_some_and(|name| {
        name.starts_with("mcp__") && name.contains(PLUGIN) && name.ends_with("__reply")
    })
}

#[derive(Default)]
struct ReplyStream {
    input: String,
    shown: usize,
}

impl ReplyStream {
    fn push(&mut self, more: &str) -> String {
        self.input.push_str(more);
        let text = partial_text(&self.input);
        let fresh = text.get(self.shown..).unwrap_or_default().to_owned();
        self.shown = text.len();
        fresh
    }
}

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
    if is_own_error(value) {
        *lock(&shared.failed_code) = value["error"].as_str().map(str::to_owned);
        match value["error"].as_str() {
            Some("authentication_failed") => shared.emit(AgentEvent::Notice {
                kind: NoticeKind::SignIn,
                text: SIGN_IN.into(),
            }),
            Some("rate_limit") => shared.emit(AgentEvent::Notice {
                kind: NoticeKind::UsageLimit,
                text: "You reached your Claude usage limit.".into(),
            }),
            _ => {}
        }
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
                if !fresh {
                    continue;
                }
                begin_turn(shared);
                if shared.reply_tool.load(Ordering::SeqCst) {
                    lock(&shared.held_writing).clear();
                    lock(&shared.held).push(text.to_owned());
                } else {
                    shared.emit(AgentEvent::TextDone {
                        text: text.to_owned(),
                    });
                }
            }
            Some("tool_use") if is_reply_tool(block["name"].as_str()) => {
                shared.reply_tool.store(true, Ordering::SeqCst);
                let Some(text) = block["input"]["text"]
                    .as_str()
                    .filter(|t| !t.trim().is_empty())
                else {
                    continue;
                };
                begin_turn(shared);
                shared.replied.store(true, Ordering::SeqCst);
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
                if block["name"].as_str().is_some_and(is_context_tool) || asks_for_checks(block) {
                    shared.opening.record_opened.store(true, Ordering::SeqCst);
                }
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
}

fn release_held(shared: &Arc<Shared>) {
    let mut held = std::mem::take(&mut *lock(&shared.held));
    let unfinished = std::mem::take(&mut *lock(&shared.held_writing));
    if !unfinished.trim().is_empty() {
        held.push(unfinished);
    }
    let replied = shared.replied.swap(false, Ordering::SeqCst);
    for text in held {
        if replied {
            let quoted = serde_json::to_string(&text).unwrap_or_default();
            shared.trace(
                "#",
                &format!("kept off the page beside the reply: {quoted}"),
            );
        } else {
            shared.emit(AgentEvent::TextDone { text });
        }
    }
}

fn on_result(shared: &Arc<Shared>, value: &Value) {
    shared.in_turn.store(false, Ordering::SeqCst);
    shared.awaiting.store(false, Ordering::SeqCst);
    release_held(shared);
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
    let code = lock(&shared.failed_code).take();
    if failed && !interrupted {
        let said = what_went_wrong(value);
        let plain = failure_notice(code.as_deref(), &said, &lock(&shared.opening.models));
        if ended_signed_out(value, &said) {
            shared.emit(AgentEvent::Notice {
                kind: NoticeKind::SignIn,
                text: SIGN_IN.into(),
            });
        } else if looks_like_a_limit(&said) {
            shared.emit(AgentEvent::Notice {
                kind: NoticeKind::UsageLimit,
                text: limit_words(&said),
            });
        } else if let Some((kind, text)) = plain {
            shared.emit(AgentEvent::Notice { kind, text });
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
    look_at_first_reply(shared, !failed && !interrupted);
}

fn is_context_tool(name: &str) -> bool {
    name.starts_with("mcp__") && name.contains(PLUGIN) && name.ends_with("__context")
}

fn asks_for_checks(block: &Value) -> bool {
    block["name"].as_str().is_some_and(|name| {
        name.starts_with("mcp__") && name.contains(PLUGIN) && name.ends_with("__learning_step")
    }) && block["input"]["action"] == "just_checks"
}

fn look_at_first_reply(shared: &Arc<Shared>, finished: bool) {
    let opening = &shared.opening;
    if !opening.new
        || opening.looked.swap(true, Ordering::SeqCst)
        || !finished
        || opening.record_opened.load(Ordering::SeqCst)
    {
        return;
    }
    let unopened =
        "This lesson started without opening your record, so its answers may not be marked.";
    let effort = lock(&opening.effort).clone();
    let (kind, text) = match effort.as_deref() {
        Some(effort) => (
            NoticeKind::RecordUnopenedAtEffort,
            format!("{unopened} It ran at {} effort.", effort_words(effort)),
        ),
        None => {
            let model = lock(&opening.model).clone();
            let name = model.map(|id| {
                crate::models::find(&lock(&opening.models), &id)
                    .map_or(id.clone(), |row| row.name.clone())
            });
            (
                NoticeKind::RecordUnopened,
                match name {
                    Some(name) => {
                        format!("{unopened} It ran on {name} at Claude Code’s usual effort.")
                    }
                    None => unopened.to_owned(),
                },
            )
        }
    };
    shared.emit(AgentEvent::Notice { kind, text });
}

pub(crate) fn effort_words(level: &str) -> String {
    match level {
        "xhigh" => "extra high".into(),
        other => other.to_owned(),
    }
}

pub(crate) fn what_went_wrong(value: &Value) -> String {
    value["result"]
        .as_str()
        .map(str::to_owned)
        .filter(|text| !text.trim().is_empty())
        .or_else(|| {
            value["errors"].as_array().map(|errors| {
                errors
                    .iter()
                    .filter_map(Value::as_str)
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
    let ask = crate::asks::describe(&tool, &input, request, &shared.folder);
    let grant = crate::asks::grant(request, &shared.folder);
    let suggestions = Value::Array(grant.as_ref().map(|g| g.rules.clone()).unwrap_or_default());
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
        title: ask.title,
        detail: ask.detail,
        exact: ask.exact,
        can_always: grant.is_some(),
        always: grant.map(|g| g.label),
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

pub(crate) fn clip(text: &str, chars: usize) -> String {
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

pub const TALK_LEAD: &str = "(This is not an answer to the task, so please don't mark it.)";

impl Conversation for ClaudeConversation {
    fn send_with(&mut self, text: &str, intent: Option<Intent>) -> AgentResult<()> {
        match intent {
            Some(Intent::Talk) => self.send(&format!("{TALK_LEAD}\n\n{text}")),
            Some(Intent::Answer) | None => self.send(text),
        }
    }

    fn send(&mut self, text: &str) -> AgentResult<()> {
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
        if !self.shared.turn_active() {
            return Ok(());
        }
        let id = self.shared.next_id();
        self.shared.interrupting.store(true, Ordering::SeqCst);
        let sent = self.shared.write(&json!({
            "type": "control_request",
            "request_id": id,
            "request": {"subtype": "interrupt", "cancel_queued": true},
        }));
        if let Err(error) = sent {
            self.shared.interrupting.store(false, Ordering::SeqCst);
            return Err(error);
        }
        let mine = self.shared.interrupts.fetch_add(1, Ordering::SeqCst) + 1;
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

    fn change_model(
        &mut self,
        model: &str,
        effort: Option<&str>,
    ) -> AgentResult<crate::agent::Applied> {
        let shared = &self.shared;
        let changed = crate::models::change(|request| shared.ask(request), model, effort)?;
        *lock(&shared.opening.model) = Some(changed.model.clone());
        *lock(&shared.opening.effort) = effort.map(str::to_owned);
        Ok(changed)
    }

    fn set_permissions(&mut self, permissions: Permissions) -> AgentResult<()> {
        let mode = match permissions {
            Permissions::Auto => AUTO,
            Permissions::Ask => "default",
        };
        self.shared
            .ask(json!({"subtype": "set_permission_mode", "mode": mode}))
            .map_err(|why| AgentError(format!("Claude Code did not change who decides: {why}.")))?;
        *lock(&self.shared.permissions) = permissions;
        Ok(())
    }

    fn applied(&mut self) -> AgentResult<crate::agent::Applied> {
        let shared = &self.shared;
        crate::models::applied(|request| shared.ask(request))
    }

    fn close(&mut self) {
        self.shared.closing.store(true, Ordering::SeqCst);
        let saving_notes = self.shared.turn_active() && !lock(&self.shared.replies).is_empty();
        if self.shared.turn_active() && !saving_notes {
            self.shared.interrupting.store(true, Ordering::SeqCst);
            let id = self.shared.next_id();
            let _ = self.shared.write(&json!({
                "type": "control_request",
                "request_id": id,
                "request": {"subtype": "interrupt", "cancel_queued": true},
            }));
        }
        *lock(&self.shared.stdin) = None;
        let shared = Arc::clone(&self.shared);
        let grace = Duration::from_secs(if saving_notes { 30 } else { 6 });
        thread::spawn(move || {
            thread::sleep(grace);
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
    fn asking_for_the_checks_alone_opens_the_record() {
        let tool = format!("mcp__{PLUGIN}__learning_step");
        assert!(asks_for_checks(
            &json!({"name": tool, "input": {"schema": "learner-model/v2", "action": "just_checks"}})
        ));
        assert!(!asks_for_checks(
            &json!({"name": tool, "input": {"schema": "learner-model/v2", "action": "update"}})
        ));
        assert!(!asks_for_checks(
            &json!({"name": "Bash", "input": {"action": "just_checks"}})
        ));
    }

    #[test]
    fn a_tool_result_that_says_a_sitting_ended_is_noticed() {
        let text = |body: &str| json!([{"type": "text", "text": body}]);
        assert!(says_sitting_ended(&text(
            r#"{"action":"end_sitting","sitting_ended":true}"#
        )));
        assert!(says_sitting_ended(&json!(
            r#"{"update_status":"committed","sitting_ended":true}"#
        )));
        assert!(!says_sitting_ended(&text(r#"{"sitting_ended":false}"#)));
        assert!(!says_sitting_ended(&text("the sitting_ended: true")));
        assert!(!says_sitting_ended(&json!(null)));
    }

    #[test]
    fn the_account_is_read_from_claude_codes_own_words() {
        let plan = read_account(
            &json!({"loggedIn": true, "authMethod": "claude.ai", "apiProvider": "firstParty", "subscriptionType": "pro"}),
            Payment::Plan,
        );
        assert_eq!(plan.paid_with, Some(Payment::Plan));
        assert_eq!(plan.detail.as_deref(), Some("your Claude Pro plan"));
        assert_eq!(plan.problem, None);
        let wanted_key = read_account(
            &json!({"loggedIn": true, "authMethod": "claude.ai", "subscriptionType": "max"}),
            Payment::ApiKey,
        );
        assert!(wanted_key.problem.unwrap().contains("found no API key"));
        let console = read_account(
            &json!({"loggedIn": true, "authMethod": "api_key", "apiKeySource": "/login managed key"}),
            Payment::Plan,
        );
        assert_eq!(console.paid_with, Some(Payment::ApiKey));
        assert!(console.detail.unwrap().contains("Console"));
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
    fn the_programs_own_diagnostics_are_not_shown() {
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
    fn a_model_the_account_cannot_use_is_a_notice_in_the_windows_words() {
        let (shared, seen) = listening();
        *lock(&shared.opening.models) =
            crate::models::parse(&crate::models::tests::fourteen()).unwrap();
        let said = "There's an issue with the selected model (claude-opus-4-6). It may not exist or you may not have access to it. Run --model to pick a different model.";
        handle(
            &shared,
            &json!({"type": "assistant", "parent_tool_use_id": null, "error": "model_not_found",
                    "is_api_error_message": true, "api_error_status": 404,
                    "message": {"id": "m1", "model": "<synthetic>", "content": [{"type": "text", "text": said}]}}),
        );
        handle(
            &shared,
            &json!({"type": "result", "subtype": "success", "is_error": true, "api_error_status": 404, "result": said}),
        );
        let seen = lock(&seen);
        assert!(
            seen.iter().any(|event| matches!(event, AgentEvent::Notice { kind: NoticeKind::ModelUnavailable, text }
                if text == "Opus 4.6 isn’t available to you. Pick another model, then send your message again.")),
            "{seen:#?}"
        );
        assert!(
            seen.iter().any(|event| matches!(
                event,
                AgentEvent::TurnEnded {
                    ok: false,
                    error: None,
                    ..
                }
            )),
            "{seen:#?}"
        );
        assert!(!format!("{seen:?}").contains("--model"), "{seen:#?}");
        assert_eq!(
            unavailable_words("There's an issue with the selected model (claude-nope-1).", &[]),
            "This session’s model isn’t available to you. Pick another model, then send your message again."
        );
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
            held: Mutex::new(Vec::new()),
            held_writing: Mutex::new(String::new()),
            replied: AtomicBool::new(false),
            reply_tool: AtomicBool::new(true),
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
            permissions: Mutex::new(Permissions::Auto),
            trace: None,
            started: Instant::now(),
            asked: crate::models::Asked::default(),
            failed_code: Mutex::new(None),
            opening: Opening::default(),
        });
        let mut conversation = ClaudeConversation {
            shared: Arc::clone(&shared),
        };
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
            Some("searching your material")
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
        let args = lesson_args(
            folder,
            Some(plugin),
            Some("claude-opus-5-5"),
            None,
            Permissions::Auto,
        );
        let after = |flag: &str| {
            let at = args
                .iter()
                .position(|a| a == flag)
                .unwrap_or_else(|| panic!("{flag} in {args:?}"));
            args[at + 1].clone()
        };
        assert_eq!(after("--setting-sources"), "project,local");
        assert!(
            after("--append-system-prompt").contains("$$...$$"),
            "the tutor is told the window draws TeX"
        );
        assert_eq!(
            after("--tools"),
            "Read,Write,Edit,Glob,Grep,AskUserQuestion,WebSearch,WebFetch,Bash"
        );
        assert_eq!(after("--model"), "claude-opus-5-5");
        assert!(
            !args.iter().any(|a| a == "--effort"),
            "with no effort picked, a lesson thinks as in a terminal"
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
        let allow: Vec<&str> = settings["permissions"]["allow"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert_eq!(
            allow,
            vec![
                "Edit(//study/stats/Study/*/plan.md)",
                "Write(//study/stats/Study/*/plan.md)",
            ]
        );
        assert!(!args.iter().any(|a| a == "--bare"));
        let asking = lesson_args(folder, None, None, None, Permissions::Ask);
        assert!(!asking.iter().any(|a| a == "--plugin-dir"));
        assert_eq!(after("--permission-mode"), "auto");
        assert!(!asking.iter().any(|a| a == "--permission-mode"));
        assert!(!args.iter().any(|a| a.contains("bypassPermissions")));
    }

    #[test]
    fn auto_mode_not_given_is_said_once_and_asking_is_not() {
        let init =
            |mode: &str| json!({"type": "system", "subtype": "init", "permissionMode": mode});
        assert_eq!(auto_notice(Permissions::Auto, &init("auto")), None);
        assert!(auto_notice(Permissions::Auto, &init("default"))
            .is_some_and(|text| text.contains("not available")));
        assert_eq!(auto_notice(Permissions::Ask, &init("default")), None);
        assert_eq!(auto_notice(Permissions::Auto, &json!({})), None);
    }

    #[test]
    fn a_lesson_starts_on_the_pick_and_never_on_a_word_that_reads_as_a_flag() {
        let folder = Path::new("/study/stats");
        let args = lesson_args(
            folder,
            None,
            Some("claude-sonnet-4-6"),
            Some("high"),
            Permissions::Ask,
        );
        let at = args.iter().position(|a| a == "--model").unwrap();
        assert_eq!(args[at + 1], "claude-sonnet-4-6");
        let at = args.iter().position(|a| a == "--effort").unwrap();
        assert_eq!(args[at + 1], "high");
        let own = lesson_args(folder, None, None, None, Permissions::Ask);
        assert!(!own.iter().any(|a| a == "--model" || a == "--effort"));
        let odd = lesson_args(folder, None, Some("--bare"), Some("--x"), Permissions::Ask);
        assert!(!odd
            .iter()
            .any(|a| a == "--model" || a == "--effort" || a == "--bare"));
    }

    #[test]
    fn the_reply_to_initialize_gives_the_models_and_no_other_reply_does() {
        let (shared, seen) = listening();
        *lock(&shared.opening.initialize) = Some("req_1".into());
        let reply = |id: &str, subtype: &str| {
            json!({"type": "control_response", "response": {"subtype": subtype, "request_id": id, "response": {"models": [
                {"value": "default", "resolvedModel": "claude-opus-5-5", "displayName": "Default (recommended)", "description": "Use the default model"},
                {"value": "opus", "resolvedModel": "claude-opus-5-5", "displayName": "Opus", "description": "Best for everyday, complex tasks", "supportsEffort": true, "supportedEffortLevels": ["low", "high"]},
            ]}}})
        };
        handle(&shared, &reply("req_9", "success"));
        assert!(lock(&seen).is_empty(), "a reply to another request");
        handle(&shared, &reply("req_1", "success"));
        let Some(AgentEvent::Models {
            models: Some(models),
        }) = lock(&seen).last().cloned()
        else {
            panic!("no models in {:?}", lock(&seen));
        };
        assert_eq!(models.len(), 2);
        assert_eq!(models[1].efforts, ["low", "high"]);
        handle(&shared, &reply("req_1", "success"));
        assert_eq!(lock(&seen).len(), 1);
        let (shared, seen) = listening();
        *lock(&shared.opening.initialize) = Some("req_1".into());
        handle(
            &shared,
            &json!({"type": "control_response", "response": {"subtype": "error", "request_id": "req_1", "error": "no"}}),
        );
        assert_eq!(
            lock(&seen).last(),
            Some(&AgentEvent::Models { models: None })
        );
    }

    #[test]
    fn a_hooks_message_to_the_terminal_is_not_drawn_on_the_page() {
        let (shared, seen) = listening();
        handle(
            &shared,
            &json!({"type": "system", "subtype": "informational", "level": "info",
                "content": "PostToolUse:mcp__plugin_magnesium_magnesium__learning_step says: First of three days. Not right on Fri 13 Nov: sign of b. It held through the 3 days since you last answered it.",
                "session_id": "s1", "uuid": "u1"}),
        );
        assert!(lock(&seen).is_empty(), "{:?}", lock(&seen));
    }

    #[test]
    fn a_new_lessons_first_reply_that_never_opened_the_record_is_said_once() {
        let said = |new: bool, effort: Option<&str>, tools: &[&str]| {
            let (shared, seen) = listening();
            let shared = Arc::new(Shared {
                opening: Opening {
                    new,
                    effort: Mutex::new(effort.map(str::to_owned)),
                    ..Opening::default()
                },
                ..Arc::try_unwrap(shared).ok().unwrap()
            });
            *lock(&shared.opening.models) =
                crate::models::parse(&crate::models::tests::fourteen()).unwrap();
            handle(
                &shared,
                &json!({"type": "system", "subtype": "init", "session_id": "s1", "model": "claude-haiku-5-5"}),
            );
            let content: Vec<Value> = tools
                .iter()
                .map(|name| json!({"type": "tool_use", "id": "t", "name": name, "input": {}}))
                .collect();
            handle(
                &shared,
                &json!({"type": "assistant", "parent_tool_use_id": null, "message": {"id": "m1", "content": content}}),
            );
            handle(&shared, &json!({"type": "result", "subtype": "success"}));
            handle(&shared, &json!({"type": "result", "subtype": "success"}));
            let notices: Vec<(NoticeKind, String)> = lock(&seen)
                .iter()
                .filter_map(|event| match event {
                    AgentEvent::Notice { kind, text }
                        if matches!(
                            kind,
                            NoticeKind::RecordUnopened | NoticeKind::RecordUnopenedAtEffort
                        ) =>
                    {
                        Some((*kind, text.clone()))
                    }
                    _ => None,
                })
                .collect();
            notices
        };
        assert_eq!(
            said(true, Some("low"), &["Read"]),
            [(
                NoticeKind::RecordUnopenedAtEffort,
                "This lesson started without opening your record, so its answers may not be marked. It ran at low effort.".to_owned()
            )]
        );
        assert_eq!(
            said(true, None, &["Read"]),
            [(
                NoticeKind::RecordUnopened,
                "This lesson started without opening your record, so its answers may not be marked. It ran on Haiku at Claude Code’s usual effort.".to_owned()
            )]
        );
        assert!(said(true, None, &["mcp__plugin_magnesium_magnesium__context"]).is_empty());
        assert!(said(
            true,
            Some("low"),
            &["mcp__plugin_magnesium_magnesium__context"]
        )
        .is_empty());
        assert!(said(false, None, &["Read"]).is_empty());
    }

    #[test]
    fn the_first_message_says_the_model_the_conversation_runs_on() {
        let (shared, seen) = listening();
        handle(
            &shared,
            &json!({"type": "system", "subtype": "init", "session_id": "s1", "model": "claude-opus-5-5"}),
        );
        assert!(matches!(
            lock(&seen).iter().find(|e| matches!(e, AgentEvent::Ready { .. })),
            Some(AgentEvent::Ready { model: Some(model), .. }) if model == "claude-opus-5-5"
        ));
        let (shared, seen) = listening();
        handle(
            &shared,
            &json!({"type": "system", "subtype": "init", "session_id": "s1", "model": "--bare"}),
        );
        assert!(matches!(
            lock(&seen)
                .iter()
                .find(|e| matches!(e, AgentEvent::Ready { .. })),
            Some(AgentEvent::Ready { model: None, .. })
        ));
    }

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
            held: Mutex::new(Vec::new()),
            held_writing: Mutex::new(String::new()),
            replied: AtomicBool::new(false),
            reply_tool: AtomicBool::new(true),
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
            permissions: Mutex::new(Permissions::Auto),
            trace: None,
            started: Instant::now(),
            asked: crate::models::Asked::default(),
            failed_code: Mutex::new(None),
            opening: Opening::default(),
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
        handle(
            &shared,
            &json!({"type": "assistant", "parent_tool_use_id": null, "message": {"id": "m2", "content": [
            {"type": "text", "text": "(Saved.)"}]}}),
        );
        on_result(&shared, &json!({"type": "result", "subtype": "success"}));
        let events = lock(&seen).clone();
        let (deltas, done) = words(&events);
        assert_eq!(deltas, reply, "{events:?}");
        assert_eq!(done, [reply]);
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, AgentEvent::Working { .. })),
            "{events:?}"
        );
        assert!(lock(&shared.replies).is_empty());
        assert!(!shared.replied.load(Ordering::SeqCst));
    }

    fn words(events: &[AgentEvent]) -> (String, Vec<String>) {
        let deltas = events
            .iter()
            .filter_map(|e| match e {
                AgentEvent::TextDelta { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        let done = events
            .iter()
            .filter_map(|e| match e {
                AgentEvent::TextDone { text } => Some(text.clone()),
                _ => None,
            })
            .collect();
        (deltas, done)
    }

    const REPLY: &str = "mcp__plugin_magnesium_magnesium__reply";

    fn plain(shared: &Arc<Shared>, id: &str, text: &str) {
        let stream = |event: Value| {
            handle(
                shared,
                &json!({"type": "stream_event", "parent_tool_use_id": null, "event": event}),
            )
        };
        stream(json!({"type": "message_start", "message": {"id": id}}));
        stream(
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
        );
        for piece in text.split_inclusive(' ') {
            stream(
                json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": piece}}),
            );
        }
        handle(
            shared,
            &json!({"type": "assistant", "parent_tool_use_id": null, "message": {"id": id, "content": [
            {"type": "text", "text": text}]}}),
        );
    }

    fn call(shared: &Arc<Shared>, id: &str, name: &str, input: Value) {
        let stream = |event: Value| {
            handle(
                shared,
                &json!({"type": "stream_event", "parent_tool_use_id": null, "event": event}),
            )
        };
        stream(json!({"type": "message_start", "message": {"id": id}}));
        stream(
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "tool_use", "name": name, "input": {}}}),
        );
        let whole = input.to_string();
        let mut at = 0;
        while at < whole.len() {
            let mut end = (at + 7).min(whole.len());
            while !whole.is_char_boundary(end) {
                end += 1;
            }
            stream(
                json!({"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": &whole[at..end]}}),
            );
            at = end;
        }
        handle(
            shared,
            &json!({"type": "assistant", "parent_tool_use_id": null, "message": {"id": id, "content": [
            {"type": "tool_use", "name": name, "input": input}]}}),
        );
    }

    fn shown(events: &[AgentEvent], line: &str) -> bool {
        events.iter().any(|e| match e {
            AgentEvent::TextDelta { text } | AgentEvent::TextDone { text } => text.contains(line),
            _ => false,
        })
    }

    #[test]
    fn the_tutors_note_before_its_reply_never_reaches_the_page() {
        let (shared, seen) = listening();
        let note = "Fresh topic, no history. First opening question: one easy concrete case.";
        plain(&shared, "m1", note);
        call(
            &shared,
            "m1",
            "mcp__plugin_magnesium_magnesium__context",
            json!({}),
        );
        let reply = "Take $f(x) = x^2$.\n\n**Task.** What is $f(3)$?";
        call(&shared, "m2", REPLY, json!({"text": reply}));
        on_result(&shared, &json!({"type": "result", "subtype": "success"}));
        let events = lock(&seen).clone();
        assert!(!shown(&events, "Fresh topic"), "{events:?}");
        let (deltas, done) = words(&events);
        assert_eq!(deltas, reply);
        assert_eq!(done, [reply]);
        assert!(matches!(
            events.last(),
            Some(AgentEvent::TurnEnded { ok: true, .. })
        ));
    }

    #[test]
    fn the_tutors_note_after_its_reply_never_reaches_the_page() {
        let (shared, seen) = listening();
        let reply =
            "Not quite: the sign flips.\n\n**Task.** Try $-2$. If something else, say what.";
        call(
            &shared,
            "m1",
            REPLY,
            json!({"text": reply, "save_after": true}),
        );
        call(
            &shared,
            "m1",
            "mcp__plugin_magnesium_magnesium__learning_step",
            json!({"action": "update"}),
        );
        plain(&shared, "m2", "Retrying without the disposition.");
        call(
            &shared,
            "m3",
            "mcp__plugin_magnesium_magnesium__learning_step",
            json!({"action": "update"}),
        );
        plain(&shared, "m4", "Saved.");
        on_result(&shared, &json!({"type": "result", "subtype": "success"}));
        let events = lock(&seen).clone();
        assert!(!shown(&events, "Retrying"), "{events:?}");
        assert!(!shown(&events, "Saved."), "{events:?}");
        let (deltas, done) = words(&events);
        assert_eq!(deltas, reply);
        assert_eq!(done, [reply]);
    }

    #[test]
    fn a_turn_without_a_reply_call_still_shows_its_plain_text_once() {
        let (shared, seen) = listening();
        let first = "Two thirds, because the remaining third is the one you left out.";
        let second = "**Task.** Why is it not three quarters?";
        plain(&shared, "m1", first);
        handle(
            &shared,
            &json!({"type": "assistant", "parent_tool_use_id": null, "message": {"id": "m1", "content": [
            {"type": "text", "text": first}]}}),
        );
        call(
            &shared,
            "m1",
            "mcp__plugin_magnesium_magnesium__learning_step",
            json!({"action": "update"}),
        );
        plain(&shared, "m2", second);
        assert_eq!(words(&lock(&seen)), (String::new(), vec![]));
        on_result(&shared, &json!({"type": "result", "subtype": "success"}));
        let events = lock(&seen).clone();
        let (_, done) = words(&events);
        assert_eq!(done, [first, second], "{events:?}");
        let at = |wanted: fn(&AgentEvent) -> bool| events.iter().position(wanted).unwrap();
        assert!(
            at(|e| matches!(e, AgentEvent::TextDone { .. }))
                < at(|e| matches!(e, AgentEvent::TurnEnded { .. }))
        );
        assert!(lock(&shared.held).is_empty());
    }

    #[test]
    fn plain_text_cut_off_by_the_programs_end_is_still_shown() {
        let text = "The derivative of $x^2$ is $2x$, because";
        let line = |value: Value| value.to_string();
        let start = line(
            json!({"type": "stream_event", "parent_tool_use_id": null, "event": {"type": "message_start", "message": {"id": "m1"}}}),
        );
        let block = line(
            json!({"type": "stream_event", "parent_tool_use_id": null, "event": {"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}}),
        );
        let pieces: Vec<String> = text
            .split_inclusive(' ')
            .map(|piece| {
                line(
                    json!({"type": "stream_event", "parent_tool_use_id": null, "event": {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": piece}}}),
                )
            })
            .collect();
        let finished = line(
            json!({"type": "assistant", "parent_tool_use_id": null, "message": {"id": "m1", "content": [{"type": "text", "text": text}]}}),
        );
        for whole in [true, false] {
            let (shared, seen) = listening();
            shared.awaiting.store(true, Ordering::SeqCst);
            let mut lines = vec![start.clone(), block.clone()];
            lines.extend(pieces.iter().cloned());
            if whole {
                lines.push(finished.clone());
            }
            read_loop(&shared, lines.join("\n").as_bytes());
            let events = lock(&seen).clone();
            assert_eq!(words(&events).1, [text], "whole: {whole}, {events:?}");
            assert!(matches!(
                events.iter().rev().nth(1),
                Some(AgentEvent::TurnEnded { ok: false, .. })
            ));
        }
    }

    #[test]
    fn a_plain_text_reply_stopped_part_way_keeps_what_was_written() {
        let (shared, seen) = listening();
        let stream = |event: Value| {
            handle(
                &shared,
                &json!({"type": "stream_event", "parent_tool_use_id": null, "event": event}),
            )
        };
        stream(json!({"type": "message_start", "message": {"id": "m1"}}));
        stream(
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
        );
        for piece in ["word0", " word1", " word2"] {
            stream(
                json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": piece}}),
            );
        }
        shared.interrupting.store(true, Ordering::SeqCst);
        handle(
            &shared,
            &json!({"type": "result", "subtype": "error_during_execution", "is_error": true,
                "errors": ["[ede_diagnostic] result_type=user last_content_type=n/a stop_reason=null"]}),
        );
        let events = lock(&seen).clone();
        assert_eq!(
            words(&events),
            (String::new(), vec!["word0 word1 word2".to_owned()]),
            "{events:?}"
        );
        assert!(matches!(
            events.last(),
            Some(AgentEvent::TurnEnded {
                interrupted: true,
                ..
            })
        ));
        assert!(lock(&shared.held_writing).is_empty() && lock(&shared.held).is_empty());
    }

    #[test]
    fn the_pieces_of_a_request_given_up_on_are_not_kept_beside_its_retry() {
        let (shared, seen) = listening();
        let stream = |event: Value| {
            handle(
                &shared,
                &json!({"type": "stream_event", "parent_tool_use_id": null, "event": event}),
            )
        };
        stream(json!({"type": "message_start", "message": {"id": "m1"}}));
        stream(
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
        );
        stream(
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Two th"}}),
        );
        plain(&shared, "m2", "Two thirds.");
        on_result(&shared, &json!({"type": "result", "subtype": "success"}));
        assert_eq!(words(&lock(&seen)).1, ["Two thirds."]);
    }

    #[test]
    fn without_the_reply_tool_plain_text_is_the_reply_and_streams() {
        let (shared, seen) = listening();
        handle(
            &shared,
            &json!({"type": "system", "subtype": "init", "session_id": "s1",
                "tools": ["Read", "Bash"],
                "plugins": [{"name": "magnesium", "path": "/x"}],
                "slash_commands": ["magnesium:learn"],
                "mcp_servers": [{"name": "plugin:magnesium:magnesium", "status": "failed"}]}),
        );
        let text = "Two thirds. **Task.** Why?";
        plain(&shared, "m1", text);
        let (deltas, done) = words(&lock(&seen));
        assert_eq!(deltas, text, "shown as it is written");
        assert_eq!(done, [text]);
        on_result(&shared, &json!({"type": "result", "subtype": "success"}));
        assert_eq!(words(&lock(&seen)).1, [text], "and only once");
    }

    #[test]
    fn the_report_of_what_claude_code_loaded_says_whether_a_reply_tool_can_come() {
        let server =
            |status: &str| json!([{"name": "plugin:magnesium:magnesium", "status": status}]);
        let init = |tools: Value, servers: Value| json!({"tools": tools, "mcp_servers": servers});
        assert!(can_reply_by_tool(&init(
            json!(["Read", REPLY]),
            server("connected")
        )));
        assert!(can_reply_by_tool(&init(json!(["Read"]), server("pending"))));
        assert!(can_reply_by_tool(
            &json!({"mcp_servers": server("connected")})
        ));
        assert!(!can_reply_by_tool(&init(json!(["Read"]), server("failed"))));
        assert!(!can_reply_by_tool(&init(json!(["Read"]), json!([]))));
        assert!(!can_reply_by_tool(&init(
            json!(["Read", "mcp__plugin_magnesium_magnesium__learning_step"]),
            server("connected")
        )));
        assert!(!can_reply_by_tool(&init(
            json!(["mcp__other__reply"]),
            server("connected")
        )));
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
