//! Native lifecycle and bounded MCP transport for the current local runtime.
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::io::{BufRead, Write};
use support::mcp::McpClient;
use support::v2::*;
use support::{TestEnvironment, run_hook};

#[test]
fn inactive_hooks_leave_no_workspace_trace() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    for (command, event, extra) in [
        ("session-start", "SessionStart", json!({"source":"startup"})),
        ("post-compact", "PostCompact", json!({"trigger":"auto"})),
        ("session-end", "SessionEnd", json!({"reason":"other"})),
    ] {
        let mut input = json!({"session_id":"thread","cwd":e.workspace,"hook_event_name":event});
        input
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let out = run_hook(&e, command, input);
        assert!(out.status.success());
        assert!(out.stdout.is_empty());
    }
    assert!(!e.workspace.join(support::CONTROL_DIR).exists());
}
#[test]
fn startup_never_scans_subject_sources_and_reports_bounded_refresh_notice() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    init(&e);
    for n in 0..128 {
        std::fs::write(
            e.workspace.join(format!("source-{n}.md")),
            "PRIVATE_SOURCE_SENTINEL".repeat(100),
        )
        .unwrap();
    }
    let index = e
        .workspace
        .join(support::CONTROL_DIR)
        .join("index/source-search.sqlite");
    let before = std::fs::read(&index).ok();
    let out = run_hook(
        &e,
        "session-start",
        json!({"session_id":"thread","cwd":e.workspace,"hook_event_name":"SessionStart","source":"startup"}),
    );
    let notice = context(&out);
    assert!(notice.contains("context.prepare"));
    assert!(notice.len() < 2048);
    assert!(!notice.contains("PRIVATE_SOURCE_SENTINEL"));
    assert_eq!(std::fs::read(index).ok(), before);
    assert_eq!(count(&e, "learning_events"), 0);
}
#[test]
fn compact_and_session_end_preserve_recorded_work_without_creating_observations() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    init(&e);
    start(&e, "codex", "thread");
    prompt(
        &e,
        "codex",
        "thread",
        Some("turn"),
        "I distinguish fixed coefficients from x.",
    );
    for (command, event, extra) in [
        ("post-compact", "PostCompact", json!({"trigger":"auto"})),
        ("session-end", "SessionEnd", json!({"reason":"other"})),
    ] {
        let mut input = json!({"session_id":"thread","cwd":e.workspace,"hook_event_name":event});
        input
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        assert!(run_hook(&e, command, input).status.success());
    }
    assert_eq!(count(&e, "learning_events"), 1);
    assert_eq!(count(&e, "observations"), 0);
    let notice = context(&prompt(
        &e,
        "codex",
        "thread",
        Some("next"),
        "What is the next step?",
    ));
    assert!(notice.contains("context.prepare"));
    assert!(notice.contains("pending 2"));
}
#[test]
fn recording_off_stops_subsequent_native_prompt_and_delivery_capture() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    init(&e);
    start(&e, "codex", "thread");
    let receipt = tool(
        &e,
        "codex",
        "thread",
        "learning_step",
        json!({"schema":"learner-model/v2","action":"control","request_id":"off","control":"recording_off","reported_event":{"external_event_key":"off-turn","speaker":"learner","kind":"message","text":"Stop recording."}}),
    );
    assert!(receipt.get("error").is_none());
    prompt(&e, "codex", "thread", Some("private"), "PRIVATE_OFF_PROMPT");
    let out = run_hook(
        &e,
        "stop",
        json!({"session_id":"thread","cwd":e.workspace,"hook_event_name":"Stop","turn_id":"private","stop_hook_active":false,"last_assistant_message":"PRIVATE_OFF_DELIVERY"}),
    );
    assert!(out.status.success());
    let conn =
        rusqlite::Connection::open(e.workspace.join(support::CONTROL_DIR).join("state.sqlite"))
            .unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM learning_events WHERE text LIKE '%PRIVATE_OFF%'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}
#[test]
fn malformed_hook_fails_open_and_reports_content_free_diagnostic() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    init(&e);
    let out = run_hook(
        &e,
        "user-prompt-submit",
        json!({"malformed":"PRIVATE_INVALID_PROMPT"}),
    );
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    let info = e.command().args(["info", "--json"]).output().unwrap();
    let text = String::from_utf8_lossy(&info.stdout);
    assert!(!text.contains("PRIVATE_INVALID_PROMPT"));
    assert!(text.contains("InputReadDecode"));
}
#[test]
fn mcp_discards_oversized_frame_then_handles_queued_requests_and_shutdown() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    init(&e);
    let mut client = McpClient::spawn(&e, true);
    client
        .stdin
        .write_all(&vec![b'x'; 1024 * 1024 + 1])
        .unwrap();
    client.stdin.write_all(b"\n").unwrap();
    client.stdin.flush().unwrap();
    let init = client.initialize();
    assert_eq!(
        init["result"]["serverInfo"]["version"],
        env!("CARGO_PKG_VERSION")
    );
    let listed = client.request(2, "tools/list", json!({}));
    let names: BTreeSet<_> = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        BTreeSet::from([
            "context",
            "memory",
            "learning_step",
            "search_sources",
            "sources"
        ])
    );
    for id in 100..164 {
        serde_json::to_writer(&mut client.stdin,&json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"context","arguments":{"schema":"learner-model/v2","action":"status"}}})).unwrap();
        client.stdin.write_all(b"\n").unwrap();
    }
    client.stdin.flush().unwrap();
    let mut remaining: BTreeSet<u64> = (100..164).collect();
    while !remaining.is_empty() {
        let mut line = String::new();
        client.stdout.read_line(&mut line).unwrap();
        let value: Value = serde_json::from_str(&line).unwrap();
        assert!(remaining.remove(&value["id"].as_u64().unwrap()));
        assert_ne!(value["result"]["isError"], true);
    }
    client.close_cleanly();
}
