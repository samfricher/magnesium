use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::paths;

const PROTOCOL: &str = "2";
const HOST_KIND: &str = "claude";
const SESSION_ENV: &str = "CLAUDE_CODE_SESSION_ID";
const HOOK_LIMIT: Duration = Duration::from_secs(5);
const CALL_LIMIT: Duration = Duration::from_secs(8);
pub const CALL_LIMIT_ENV: &str = "MAGNESIUM_APP_RUNTIME_CALL_MS";
const CHECKS_ENV: &str = "MAGNESIUM_SITTING_CHECKS";
const UNKNOWN: &str = "unknown outcome: ";

pub fn unknown(error: &str) -> bool {
    error.starts_with(UNKNOWN)
}

fn call_limit() -> Duration {
    std::env::var(CALL_LIMIT_ENV)
        .ok()
        .and_then(|ms| ms.parse().ok())
        .map(Duration::from_millis)
        .unwrap_or(CALL_LIMIT)
}

pub struct HostClient {
    program: PathBuf,
    folder: PathBuf,
    session: String,
    version: String,
    server: Mutex<Option<Server>>,
    sitting_ended: std::sync::atomic::AtomicBool,
}

struct Server {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    next: u64,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl HostClient {
    pub fn new(program: PathBuf, folder: PathBuf, session: String, version: String) -> Self {
        let version = version.split('+').next().unwrap_or_default().to_owned();
        Self {
            program,
            folder,
            session,
            version,
            server: Mutex::new(None),
            sitting_ended: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn session(&self) -> &str {
        &self.session
    }

    fn flags(&self) -> [String; 6] {
        [
            "--plugin-version".into(),
            self.version.clone(),
            "--adapter-protocol-version".into(),
            PROTOCOL.into(),
            "--host-kind".into(),
            HOST_KIND.into(),
        ]
    }

    pub fn hook(&self, event: &str, name: &str, fields: Value) -> Result<Option<String>, String> {
        let mut payload = json!({
            "session_id": self.session,
            "cwd": self.folder,
            "hook_event_name": name,
        });
        if let (Some(payload), Some(fields)) = (payload.as_object_mut(), fields.as_object()) {
            payload.extend(fields.clone());
        }
        let mut command = paths::light_command(&self.program);
        command
            .current_dir(&self.folder)
            .env_remove(SESSION_ENV)
            .args(["_internal", "hook", event])
            .args(self.flags())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command
            .spawn()
            .map_err(|e| format!("the hook did not start: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            let payload = payload.to_string();
            std::thread::spawn(move || {
                let _ = stdin.write_all(payload.as_bytes());
            });
        }
        let mut stdout = child.stdout.take().ok_or("the hook gave no output pipe")?;
        let reader = std::thread::spawn(move || {
            let mut text = String::new();
            let _ = std::io::Read::read_to_string(&mut stdout, &mut text);
            text
        });
        let started = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if started.elapsed() > HOOK_LIMIT => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("the hook took too long".into());
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                Err(e) => return Err(e.to_string()),
            }
        }
        let out = reader.join().unwrap_or_default();
        let out = out.trim();
        if out.is_empty() {
            return Ok(None);
        }
        let value: Value =
            serde_json::from_str(out).map_err(|e| format!("the hook's answer is not JSON: {e}"))?;
        Ok(value["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .map(str::to_owned))
    }

    fn start_server(&self) -> Result<Server, String> {
        let mut command = paths::light_command(&self.program);
        command
            .current_dir(&self.folder)
            .env(SESSION_ENV, &self.session)
            .env(CHECKS_ENV, "1")
            .args(["_internal", "mcp-server"])
            .args(self.flags())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command
            .spawn()
            .map_err(|e| format!("the tool server did not start: {e}"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or("the tool server gave no input pipe")?;
        let stdout = child
            .stdout
            .take()
            .ok_or("the tool server gave no output pipe")?;
        let (sender, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        let mut server = Server {
            child,
            stdin,
            lines,
            next: 0,
        };
        server
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "magnesium-window", "version": env!("CARGO_PKG_VERSION")},
                }),
                call_limit(),
            )
            .map_err(Failure::text)?;
        server.notify("notifications/initialized")?;
        Ok(server)
    }

    pub fn call(&self, tool: &str, arguments: Value) -> Result<Value, String> {
        self.call_by(tool, arguments, Instant::now() + call_limit() * 2)
    }

    pub fn call_by(
        &self,
        tool: &str,
        arguments: Value,
        deadline: Instant,
    ) -> Result<Value, String> {
        let mut guard = self.server.lock().unwrap_or_else(|p| p.into_inner());
        let mut last: Option<String> = None;
        for _ in 0..2 {
            let left = deadline.saturating_duration_since(Instant::now());
            if left < Duration::from_millis(100) {
                break;
            }
            if guard.is_none() {
                *guard = Some(self.start_server()?);
            }
            let server = guard.as_mut().expect("started above");
            match server.request(
                "tools/call",
                json!({"name": tool, "arguments": arguments}),
                left.min(call_limit()),
            ) {
                Ok(result) => {
                    let answered = answer(tool, &result);
                    if tool == "learning_step"
                        && answered
                            .as_ref()
                            .is_ok_and(|value| value["sitting_ended"] == true)
                    {
                        self.sitting_ended
                            .store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                    return answered;
                }
                Err(Failure::Refused(error)) => return Err(error),
                Err(Failure::NotSent(error)) => {
                    *guard = None;
                    last = Some(error);
                }
                Err(Failure::Unknown(error)) => {
                    *guard = None;
                    last = Some(format!("{UNKNOWN}{error}"));
                }
            }
        }
        Err(last.unwrap_or_else(|| "no time was left for the call".to_owned()))
    }

    pub fn take_sitting_ended(&self) -> bool {
        self.sitting_ended
            .swap(false, std::sync::atomic::Ordering::SeqCst)
    }

    pub fn close(&self) {
        let mut guard = self.server.lock().unwrap_or_else(|p| p.into_inner());
        *guard = None;
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }
}

enum Failure {
    Refused(String),
    NotSent(String),
    Unknown(String),
}

impl Failure {
    fn text(self) -> String {
        match self {
            Self::Refused(text) | Self::NotSent(text) | Self::Unknown(text) => text,
        }
    }
}

fn answer(tool: &str, result: &Value) -> Result<Value, String> {
    let text: String = result["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|block| block["text"].as_str())
        .collect();
    if result["isError"].as_bool() == Some(true) {
        return Err(text);
    }
    let value: Value =
        serde_json::from_str(&text).map_err(|e| format!("the {tool} result is not JSON: {e}"))?;
    if let Some(error) = value.get("error").filter(|e| e.is_object()) {
        return Err(format!(
            "{}: {}",
            error["code"].as_str().unwrap_or("error"),
            error["message"].as_str().unwrap_or_default()
        ));
    }
    Ok(value)
}

impl Server {
    fn send(&mut self, value: &Value) -> Result<(), String> {
        let mut line = value.to_string();
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .and_then(|()| self.stdin.flush())
            .map_err(|e| format!("the tool server is not listening: {e}"))
    }

    fn notify(&mut self, method: &str) -> Result<(), String> {
        self.send(&json!({"jsonrpc": "2.0", "method": method}))
    }

    fn request(&mut self, method: &str, params: Value, limit: Duration) -> Result<Value, Failure> {
        self.next += 1;
        let id = self.next;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .map_err(Failure::NotSent)?;
        let deadline = Instant::now() + limit;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let line = self.lines.recv_timeout(left).map_err(|e| {
                Failure::Unknown(match e {
                    std::sync::mpsc::RecvTimeoutError::Timeout => {
                        "the tool server took too long".to_owned()
                    }
                    std::sync::mpsc::RecvTimeoutError::Disconnected => {
                        "the tool server ended".to_owned()
                    }
                })
            })?;
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if value["id"].as_u64() != Some(id) {
                continue;
            }
            if let Some(error) = value.get("error") {
                return Err(Failure::Refused(
                    error["message"].as_str().unwrap_or("error").to_owned(),
                ));
            }
            return Ok(value["result"].clone());
        }
    }
}

pub fn ticket(notice: &str) -> Option<String> {
    let start = notice.find("native_capture.ticket=")? + "native_capture.ticket=".len();
    let ticket: String = notice[start..]
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != ';')
        .collect();
    (!ticket.is_empty()).then_some(ticket)
}

pub fn current_event(notice: &str) -> Option<String> {
    let start = notice.find("current event e-")? + "current event ".len();
    let id: String = notice[start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    (id.len() > 2).then_some(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_hooks_notices_are_read() {
        let inactive = "learner-model/v2 native_capture.ticket=f4acccbef9d1724a; start/resume:exact text; compaction:context.prepare; unbound:0.";
        assert_eq!(ticket(inactive).as_deref(), Some("f4acccbef9d1724a"));
        assert_eq!(current_event(inactive), None);
        let active = "Magnesium learner-model/v2; generation 0; pending 1; unbound 0; learning true; recording true; current event e-36c4b639. Read context.prepare. Sitting quick checks: due 2, posed 0.";
        assert_eq!(current_event(active).as_deref(), Some("e-36c4b639"));
        assert_eq!(ticket(active), None);
        assert_eq!(
            current_event("current event unavailable (use labeled reported events)"),
            None
        );
    }
}
