use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::{TestEnvironment, test_binary};

pub struct McpClient {
    pub child: Child,
    pub stdin: ChildStdin,
    pub stdout: BufReader<ChildStdout>,
}

impl McpClient {
    pub fn spawn(environment: &TestEnvironment, initialized: bool) -> Self {
        Self::spawn_with_env(environment, initialized, &[])
    }

    pub fn spawn_with_env(
        environment: &TestEnvironment,
        initialized: bool,
        variables: &[(&str, &str)],
    ) -> Self {
        Self::spawn_bound_with_env(
            environment,
            &environment.workspace.clone(),
            None,
            initialized,
            variables,
        )
    }

    pub fn spawn_bound(
        environment: &TestEnvironment,
        current_dir: &Path,
        workspace_root: Option<&Path>,
        initialized: bool,
    ) -> Self {
        Self::spawn_bound_with_env(environment, current_dir, workspace_root, initialized, &[])
    }

    /// A server launched as the Claude Code plugin launches it.
    pub fn spawn_claude(environment: &TestEnvironment, variables: &[(&str, &str)]) -> Self {
        Self::spawn_with_args(
            environment,
            &environment.workspace.clone(),
            None,
            true,
            variables,
            &["--host-kind", "claude"],
        )
    }

    /// A server launched as the Codex plugin launches it.
    pub fn spawn_codex(environment: &TestEnvironment, variables: &[(&str, &str)]) -> Self {
        Self::spawn_with_args(
            environment,
            &environment.workspace.clone(),
            None,
            true,
            variables,
            &["--host-kind", "codex"],
        )
    }

    pub fn spawn_bound_with_env(
        environment: &TestEnvironment,
        current_dir: &Path,
        workspace_root: Option<&Path>,
        initialized: bool,
        variables: &[(&str, &str)],
    ) -> Self {
        Self::spawn_with_args(
            environment,
            current_dir,
            workspace_root,
            initialized,
            variables,
            &[],
        )
    }

    fn spawn_with_args(
        environment: &TestEnvironment,
        current_dir: &Path,
        workspace_root: Option<&Path>,
        initialized: bool,
        variables: &[(&str, &str)],
        extra_args: &[&str],
    ) -> Self {
        if initialized {
            assert!(current_dir.join(crate::CONTROL_DIR).is_dir());
        }
        let mut command = environment.command_with(test_binary(), current_dir);
        if let Some(root) = workspace_root {
            command.env("MAGNESIUM_WORKSPACE_ROOT", root);
        }
        for (name, value) in variables {
            command.env(name, value);
        }
        let mut child = command
            .args([
                "_internal",
                "mcp-server",
                "--plugin-version",
                "0.2.0",
                "--adapter-protocol-version",
                "2",
            ])
            .args(extra_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn MCP process");
        let stdin = child.stdin.take().expect("MCP stdin");
        let stdout = BufReader::new(child.stdout.take().expect("MCP stdout"));
        Self {
            child,
            stdin,
            stdout,
        }
    }

    pub fn initialize(&mut self) -> Value {
        let response = self.request(
            1,
            "initialize",
            json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "magnesium-test-client", "version": "0.2.0" }
            }),
        );
        write_message(
            &mut self.stdin,
            &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        );
        response
    }

    pub fn call_value(&mut self, id: u64, name: &str, arguments: Value) -> Value {
        let response = self.request(
            id,
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        );
        assert_ne!(
            response["result"]["isError"], true,
            "tool error: {response}"
        );
        serde_json::from_str(
            response["result"]["content"][0]["text"]
                .as_str()
                .expect("tool text"),
        )
        .expect("tool result JSON")
    }

    pub fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        write_message(
            &mut self.stdin,
            &json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
        );
        let response = read_message(&mut self.stdout);
        assert_eq!(response["id"], id);
        assert!(response.get("error").is_none(), "MCP error: {response}");
        response
    }

    pub fn close_cleanly(mut self) {
        drop(self.stdin);
        drop(self.stdout);
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(status) = self.child.try_wait().expect("poll MCP process") {
                let mut stderr = String::new();
                self.child
                    .stderr
                    .take()
                    .expect("MCP stderr")
                    .read_to_string(&mut stderr)
                    .expect("read MCP stderr");
                assert!(
                    status.success(),
                    "MCP process exited with {status}; stderr: {stderr}"
                );
                assert!(stderr.is_empty(), "unexpected MCP stderr: {stderr}");
                return;
            }
            if Instant::now() >= deadline {
                self.child.kill().expect("kill stalled MCP process");
                panic!("MCP process did not stop after stdin closed");
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

pub fn write_message(writer: &mut impl Write, message: &Value) {
    serde_json::to_writer(&mut *writer, message).expect("serialize MCP message");
    writer.write_all(b"\n").expect("terminate MCP message");
    writer.flush().expect("flush MCP message");
}

pub fn read_message(reader: &mut impl BufRead) -> Value {
    let mut line = String::new();
    reader.read_line(&mut line).expect("read MCP response");
    assert!(!line.is_empty(), "MCP process closed before responding");
    serde_json::from_str(&line).expect("parse MCP response")
}
