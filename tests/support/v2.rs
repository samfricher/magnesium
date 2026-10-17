//! Subprocess helpers for actual v2 host hooks and adapter tools.
use super::TestEnvironment;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::io::Write;
use std::process::{Output, Stdio};

pub fn init(environment: &TestEnvironment) {
    let output = environment.command().arg("init").output().unwrap();
    assert!(output.status.success(), "{output:?}");
}
pub fn host_env(host: &str) -> &str {
    match host {
        "codex" => "CODEX_THREAD_ID",
        "claude" => "CLAUDE_CODE_SESSION_ID",
        "opencode" => "OPENCODE_SESSION_ID",
        _ => panic!("unsupported test host"),
    }
}
pub fn tool_output(
    environment: &TestEnvironment,
    host: &str,
    thread: &str,
    tool: &str,
    request: Value,
) -> Output {
    let mut child = environment
        .command()
        .env_remove("CODEX_THREAD_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("OPENCODE_SESSION_ID")
        .env(host_env(host), thread)
        .args([
            "adapter",
            "call",
            tool,
            "--protocol-version",
            "2",
            "--json-input",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&request).unwrap())
        .unwrap();
    child.wait_with_output().unwrap()
}
pub fn tool(
    environment: &TestEnvironment,
    host: &str,
    thread: &str,
    name: &str,
    request: Value,
) -> Value {
    let output = tool_output(environment, host, thread, name, request);
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(value.get("error").is_none(), "{value}");
    value
}
pub fn start(environment: &TestEnvironment, host: &str, thread: &str) -> Value {
    tool(
        environment,
        host,
        thread,
        "learning_step",
        json!({"schema":"learner-model/v2","action":"update","request_id":format!("start-{host}-{thread}"),"session":{"operation":"start","client_ref":"session","goal":"Learn coefficient roles"}}),
    )
}
pub fn prompt(
    environment: &TestEnvironment,
    host: &str,
    thread: &str,
    turn: Option<&str>,
    text: &str,
) -> Output {
    let mut input = json!({"session_id":thread,"cwd":environment.workspace,"hook_event_name":"UserPromptSubmit","prompt":text,"host_kind":host});
    if let Some(turn) = turn {
        input["turn_id"] = json!(turn);
    }
    super::run_hook(environment, "user-prompt-submit", input)
}
pub fn count(environment: &TestEnvironment, table: &str) -> i64 {
    Connection::open(
        environment
            .workspace
            .join(crate::CONTROL_DIR)
            .join("state.sqlite"),
    )
    .unwrap()
    .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
    .unwrap()
}
pub fn context(output: &Output) -> String {
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice::<Value>(&output.stdout).unwrap()["hookSpecificOutput"]["additionalContext"].as_str().unwrap().to_owned()
}
