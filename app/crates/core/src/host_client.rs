//! The Magnesium runtime, reached the way Claude Code reaches it.
//!
//! The direct engine keeps the learner record from code. It writes through
//! the runtime's two host protocols, as a client, so that the same checks,
//! capture tickets and spacing apply as in a lesson Claude Code runs:
//!
//! - hook events: `magnesium _internal hook <event>`, one process each, with
//!   the JSON Claude Code would send on standard input;
//! - the tool server: one `magnesium _internal mcp-server` for the whole
//!   conversation, spoken to over JSON-RPC on its standard input and output,
//!   with the conversation's id in `CLAUDE_CODE_SESSION_ID`, as Claude Code
//!   sets it.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};

use crate::paths;

/// The adapter protocol the hooks and the tool server speak.
const PROTOCOL: &str = "2";
const HOST_KIND: &str = "claude";
/// Claude Code's own session variable: the tool server's only way to know
/// the conversation.
const SESSION_ENV: &str = "CLAUDE_CODE_SESSION_ID";
const HOOK_LIMIT: Duration = Duration::from_secs(10);
const CALL_LIMIT: Duration = Duration::from_secs(60);

/// The runtime, for one conversation in one study folder.
pub struct HostClient {
    program: PathBuf,
    folder: PathBuf,
    session: String,
    version: String,
    server: Mutex<Option<Server>>,
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
    /// `version` is the runtime's own version: a hook or tool call names
    /// the plugin version it comes from, and the runtime takes only its own.
    pub fn new(program: PathBuf, folder: PathBuf, session: String, version: String) -> Self {
        // Build metadata, as in "0.3.0+abc123", is not part of the version
        // the runtime compares.
        let version = version.split('+').next().unwrap_or_default().to_owned();
        Self {
            program,
            folder,
            session,
            version,
            server: Mutex::new(None),
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

    /// Send one hook event and return its `additionalContext`, if it gave
    /// one. `fields` adds the event's own fields to the common ones.
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
            let _ = stdin.write_all(payload.to_string().as_bytes());
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
        server.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "magnesium-window", "version": env!("CARGO_PKG_VERSION")},
            }),
        )?;
        server.notify("notifications/initialized")?;
        Ok(server)
    }

    /// Call one of the runtime's tools and return its JSON result. A result
    /// the tool marks as an error comes back as `Err` with its text.
    pub fn call(&self, tool: &str, arguments: Value) -> Result<Value, String> {
        let mut guard = self.server.lock().unwrap_or_else(|p| p.into_inner());
        // A server that died is started again, once.
        for attempt in 0..2 {
            if guard.is_none() {
                *guard = Some(self.start_server()?);
            }
            let server = guard.as_mut().expect("started above");
            match server.request("tools/call", json!({"name": tool, "arguments": arguments})) {
                Ok(result) => {
                    let text: String = result["content"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|block| block["text"].as_str())
                        .collect();
                    if result["isError"].as_bool() == Some(true) {
                        return Err(text);
                    }
                    return serde_json::from_str(&text)
                        .map_err(|e| format!("the {tool} result is not JSON: {e}"));
                }
                Err(error) if attempt == 0 && error.starts_with("closed") => {
                    *guard = None;
                }
                Err(error) => return Err(error),
            }
        }
        Err("the tool server stopped".into())
    }

    /// End the tool server.
    pub fn close(&self) {
        let mut guard = self.server.lock().unwrap_or_else(|p| p.into_inner());
        *guard = None;
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }
}

impl Server {
    fn send(&mut self, value: &Value) -> Result<(), String> {
        let mut line = value.to_string();
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .and_then(|()| self.stdin.flush())
            .map_err(|e| format!("closed: {e}"))
    }

    fn notify(&mut self, method: &str) -> Result<(), String> {
        self.send(&json!({"jsonrpc": "2.0", "method": method}))
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.next += 1;
        let id = self.next;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;
        loop {
            let line = self.lines.recv_timeout(CALL_LIMIT).map_err(|e| match e {
                std::sync::mpsc::RecvTimeoutError::Timeout => {
                    "the tool server took too long".to_owned()
                }
                std::sync::mpsc::RecvTimeoutError::Disconnected => {
                    "closed: the tool server ended".to_owned()
                }
            })?;
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if value["id"].as_u64() != Some(id) {
                continue;
            }
            if let Some(error) = value.get("error") {
                return Err(error["message"].as_str().unwrap_or("error").to_owned());
            }
            return Ok(value["result"].clone());
        }
    }
}

/// The capture ticket in a prompt hook's notice, issued when no lesson is
/// active in this conversation.
pub fn ticket(notice: &str) -> Option<String> {
    let start = notice.find("native_capture.ticket=")? + "native_capture.ticket=".len();
    let ticket: String = notice[start..]
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != ';')
        .collect();
    (!ticket.is_empty()).then_some(ticket)
}

/// The event a prompt hook captured the learner's words as, while a lesson
/// is active.
pub fn current_event(notice: &str) -> Option<String> {
    let start = notice.find("current event e-")? + "current event ".len();
    let id: String = notice[start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    (id.len() > 2).then_some(id)
}

/// The quick checks due in this sitting, by the prompt hook's count.
pub fn checks_due(notice: &str) -> Option<i64> {
    let start = notice.find("Sitting quick checks: due ")? + "Sitting quick checks: due ".len();
    let digits: String = notice[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
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
        assert_eq!(checks_due(active), Some(2));
        assert_eq!(
            current_event("current event unavailable (use labeled reported events)"),
            None
        );
    }
}
