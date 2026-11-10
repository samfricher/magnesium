//! Exercise real MCP envelopes: which conversation a tool call belongs to comes
//! only from the server's own launch, never from the call itself.
use rusqlite::Connection;
use serde_json::{Value, json};
use std::io::BufReader;
use std::process::Stdio;
use support::{
    TestEnvironment,
    mcp::{McpClient, write_message},
    test_binary, v2,
};

fn client(e: &TestEnvironment, name: &str, variables: &[(&str, &str)]) -> McpClient {
    let mut command = e.command_with(test_binary(), &e.workspace);
    for (name, value) in variables {
        command.env(name, value);
    }
    let mut child = command
        .args([
            "_internal",
            "mcp-server",
            "--plugin-version",
            env!("CARGO_PKG_VERSION"),
            "--adapter-protocol-version",
            "2",
            "--host-kind",
            "claude",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    let stdout = BufReader::new(child.stdout.take().unwrap());
    let mut c = McpClient {
        child,
        stdin,
        stdout,
    };
    c.request(
        1,
        "initialize",
        json!({
            "protocolVersion":"2025-11-25", "capabilities":{},
            "clientInfo":{"name":name,"version":"test"}
        }),
    );
    write_message(
        &mut c.stdin,
        &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    );
    c
}

fn call(c: &mut McpClient, id: u64, tool: &str, arguments: Value, meta: Option<Value>) -> Value {
    let mut params = json!({"name":tool,"arguments":arguments});
    if let Some(meta) = meta {
        params["_meta"] = meta;
    }
    c.request(id, "tools/call", params)
}

fn result(response: Value) -> Value {
    assert_ne!(response["result"]["isError"], true, "{response}");
    serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
}

fn status(c: &mut McpClient, id: u64, meta: Option<Value>) -> Value {
    result(call(
        c,
        id,
        "context",
        json!({"schema":"learner-model/v2","action":"status"}),
        meta,
    ))
}

#[test]
fn each_conversations_server_binds_native_tickets_to_its_own_session() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let mut sessions = vec![];
    for (index, thread) in ["monday", "tuesday"].into_iter().enumerate() {
        let text = format!("Teach me the meaning of b on {thread}.");
        let notice = v2::context(&v2::prompt(&e, "claude", thread, Some("first-turn"), &text));
        let ticket = notice
            .split_once("native_capture.ticket=")
            .unwrap()
            .1
            .split(';')
            .next()
            .unwrap();
        let payload = json!({
            "schema":"learner-model/v2","action":"update","request_id":format!("start-{thread}"),
            "session":{"operation":"start","client_ref":"session","goal":"Coefficient meaning"},
            "native_capture":{"ticket":ticket,"text":text}
        });
        let mut c = client(&e, "claude-code", &[("CLAUDE_CODE_SESSION_ID", thread)]);
        let id = 10 + index as u64 * 10;
        let receipt = result(call(&mut c, id, "learning_step", payload.clone(), None));
        assert_eq!(receipt["update_status"], "committed", "{receipt}");
        assert_eq!(receipt["host_kind"], "claude");
        assert_eq!(
            receipt["host_thread_id"],
            format!("magnesium-h1:claude:{thread}")
        );
        assert_eq!(receipt["captured_event_ids"].as_array().unwrap().len(), 1);
        let replay = result(call(&mut c, id + 1, "learning_step", payload, None));
        assert_eq!(receipt, replay);
        sessions.push(receipt["references"]["session"].clone());
        c.close_cleanly();
    }
    assert_ne!(sessions[0], sessions[1]);
    for (index, thread) in ["monday", "tuesday"].into_iter().enumerate() {
        let mut c = client(&e, "claude-code", &[("CLAUDE_CODE_SESSION_ID", thread)]);
        let packet = status(&mut c, 30 + index as u64, None);
        assert_eq!(
            packet["host_binding"]["host_thread_id"],
            format!("magnesium-h1:claude:{thread}"),
            "{packet}"
        );
        assert_eq!(packet["session"]["session_id"], sessions[index], "{packet}");
        c.close_cleanly();
    }
    let mut c = client(&e, "claude-code", &[]);
    let unknown = status(&mut c, 40, None);
    assert_eq!(
        unknown["host_binding"]["host_kind"], "tool_only",
        "{unknown}"
    );
    assert!(unknown["session"].is_null(), "{unknown}");
    c.close_cleanly();
    let db = Connection::open(e.workspace.join(support::CONTROL_DIR).join("state.sqlite")).unwrap();
    let mut statement = db
        .prepare(
            "SELECT host_kind,host_thread_id,origin FROM learning_events ORDER BY host_thread_id",
        )
        .unwrap();
    let rows: Vec<(String, String, String)> = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        rows,
        vec![
            (
                "claude".into(),
                "magnesium-h1:claude:monday".into(),
                "host_observed".into()
            ),
            (
                "claude".into(),
                "magnesium-h1:claude:tuesday".into(),
                "host_observed".into()
            ),
        ]
    );
}

#[test]
fn call_metadata_and_tool_arguments_cannot_choose_the_host_identity() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let mut c = client(
        &e,
        "claude-code",
        &[("CLAUDE_CODE_SESSION_ID", "process-thread")],
    );
    let own = "magnesium-h1:claude:process-thread";
    for (index, thread) in [
        json!("other-thread"),
        json!(12),
        json!(""),
        json!("invalid:thread"),
    ]
    .into_iter()
    .enumerate()
    {
        let packet = status(&mut c, 10 + index as u64, Some(json!({"threadId":thread})));
        assert_eq!(packet["host_binding"]["host_thread_id"], own, "{packet}");
    }
    let fallback = status(&mut c, 21, None);
    assert_eq!(fallback["host_binding"]["host_thread_id"], own);
    let spoof = call(
        &mut c,
        22,
        "learning_step",
        json!({
            "schema":"learner-model/v2","action":"update","request_id":"spoof",
            "threadId":"other-thread","session":{"operation":"start","client_ref":"s","goal":"Rejected spoof"}
        }),
        Some(json!({"threadId":"process-thread"})),
    );
    let rejected = result(spoof);
    assert_eq!(rejected["error"]["code"], "invalid_variant", "{rejected}");
    assert!(
        rejected["error"]["message"]
            .as_str()
            .unwrap()
            .contains("threadId")
    );
    assert_eq!(v2::count(&e, "learning_sessions"), 0);
    c.close_cleanly();
}

#[test]
fn call_metadata_is_not_a_native_identity_without_the_session_environment() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let mut c = client(&e, "other-client", &[]);
    let a = status(&mut c, 2, Some(json!({"threadId":"pretend-native"})));
    let b = status(&mut c, 3, Some(json!({"threadId":"different-native"})));
    assert_eq!(a["host_binding"]["host_kind"], "tool_only");
    assert_eq!(a["host_binding"], b["host_binding"]);
    assert_ne!(a["host_binding"]["host_thread_id"], "pretend-native");
    c.close_cleanly();
}

/// A plugin copy from another release can reach this runtime, so the recovery
/// must say what refreshes it.
#[test]
fn incompatible_plugin_version_gives_a_recovery_that_names_the_refresh() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let mut child = e
        .command_with(test_binary(), &e.workspace)
        .args([
            "_internal",
            "mcp-server",
            "--plugin-version",
            "0.0.1",
            "--adapter-protocol-version",
            "2",
            "--host-kind",
            "claude",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    let stdout = BufReader::new(child.stdout.take().unwrap());
    let mut c = McpClient {
        child,
        stdin,
        stdout,
    };
    c.request(
        1,
        "initialize",
        json!({
            "protocolVersion":"2025-11-25", "capabilities":{},
            "clientInfo":{"name":"stale-plugin","version":"test"}
        }),
    );
    write_message(
        &mut c.stdin,
        &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    );
    let response = call(
        &mut c,
        2,
        "context",
        json!({"schema":"learner-model/v2","action":"status"}),
        None,
    );
    assert_eq!(response["result"]["isError"], true, "{response}");
    let body: Value =
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(body["outcome"], "incompatible_adapter", "{body}");
    assert_eq!(body["observed"]["plugin_version"], "0.0.1", "{body}");
    let recovery = body["recovery"].as_str().unwrap();
    assert!(recovery.contains("magnesium init"), "{recovery}");
    c.close_cleanly();
}
