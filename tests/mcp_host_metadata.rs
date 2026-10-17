//! Exercise real MCP envelopes, not process-environment stand-ins for Codex.
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
    for name in [
        "CODEX_THREAD_ID",
        "CLAUDE_CODE_SESSION_ID",
        "OPENCODE_SESSION_ID",
    ] {
        command.env_remove(name);
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
fn per_call_codex_metadata_binds_native_tickets_and_separate_threads_on_one_connection() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let mut c = client(&e, "codex-mcp-client", &[]);
    let mut sessions = vec![];
    for (index, thread) in ["monday", "tuesday"].into_iter().enumerate() {
        let text = format!("Teach me the meaning of b on {thread}.");
        let notice = v2::context(&v2::prompt(&e, "codex", thread, Some("first-turn"), &text));
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
        let id = 10 + index as u64 * 10;
        let receipt = result(call(
            &mut c,
            id,
            "learning_step",
            payload.clone(),
            Some(json!({"threadId":thread})),
        ));
        assert_eq!(receipt["update_status"], "committed", "{receipt}");
        assert_eq!(receipt["host_kind"], "codex");
        assert_eq!(receipt["host_thread_id"], thread);
        assert_eq!(receipt["captured_event_ids"].as_array().unwrap().len(), 1);
        let replay = result(call(
            &mut c,
            id + 1,
            "learning_step",
            payload,
            Some(json!({"threadId":thread})),
        ));
        assert_eq!(receipt, replay);
        sessions.push(receipt["references"]["session"].clone());
    }
    assert_ne!(sessions[0], sessions[1]);
    for (index, thread) in ["monday", "tuesday"].into_iter().enumerate() {
        let packet = status(&mut c, 30 + index as u64, Some(json!({"threadId":thread})));
        assert_eq!(packet["host_binding"]["host_thread_id"], thread, "{packet}");
        assert_eq!(packet["session"]["session_id"], sessions[index], "{packet}");
    }
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
            ("codex".into(), "monday".into(), "host_observed".into()),
            ("codex".into(), "tuesday".into(), "host_observed".into()),
        ]
    );
}

#[test]
fn metadata_identity_rejects_conflicts_and_cannot_be_chosen_in_arguments() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let mut c = client(
        &e,
        "codex-mcp-client",
        &[("CODEX_THREAD_ID", "process-thread")],
    );
    for (index, thread) in [
        json!("other-thread"),
        json!(12),
        json!(""),
        json!("invalid:thread"),
    ]
    .into_iter()
    .enumerate()
    {
        let response = call(
            &mut c,
            10 + index as u64,
            "context",
            json!({"schema":"learner-model/v2","action":"status"}),
            Some(json!({"threadId":thread})),
        );
        assert_eq!(response["result"]["isError"], true, "{response}");
    }
    let same = status(&mut c, 20, Some(json!({"threadId":"process-thread"})));
    assert_eq!(same["host_binding"]["host_thread_id"], "process-thread");
    let fallback = status(&mut c, 21, None);
    assert_eq!(fallback["host_binding"]["host_thread_id"], "process-thread");
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
    // Claude Code exports CLAUDE_CODE_SESSION_ID to every subprocess, so a
    // Codex launched from a Claude shell inherits it without being a Claude
    // process. Other host identities still conflict with Codex metadata.
    for (index, (name, conflicts)) in [
        ("CLAUDE_CODE_SESSION_ID", false),
        ("OPENCODE_SESSION_ID", true),
    ]
    .into_iter()
    .enumerate()
    {
        let mut c = client(&e, "codex-mcp-client", &[(name, "process-thread")]);
        let response = call(
            &mut c,
            30 + index as u64,
            "context",
            json!({"schema":"learner-model/v2","action":"status"}),
            Some(json!({"threadId":"process-thread"})),
        );
        assert_eq!(
            response["result"]["isError"] == true,
            conflicts,
            "{name}: {response}"
        );
        c.close_cleanly();
    }
}

#[test]
fn unsupported_client_metadata_is_not_native_codex_identity() {
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

/// A plugin copy from another release can reach this runtime from any host,
/// so the recovery must not name one host and must say what refreshes it.
#[test]
fn incompatible_plugin_version_gives_a_recovery_for_every_host() {
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
    assert!(!recovery.contains("Codex"), "{recovery}");
    assert!(recovery.contains("magnesium init"), "{recovery}");
    c.close_cleanly();
}
