//! Actual native-hook identity/capture tests; no provider model is invoked.
use rusqlite::Connection;
use serde_json::json;
use support::mcp::McpClient;
use support::v2::*;
use support::{TestEnvironment, run_hook, run_hook_with_host_flag};

#[test]
fn ordinary_work_is_neutral_then_native_hosts_capture_separately() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    init(&e);
    for host in ["codex", "claude", "opencode"] {
        assert!(
            context(&prompt(
                &e,
                host,
                "same-thread",
                Some("ordinary"),
                "private coding work"
            ))
            .contains("native_capture.ticket=")
        );
    }
    assert_eq!(count(&e, "learning_events"), 0);
    for host in ["codex", "claude", "opencode"] {
        start(&e, host, "same-thread");
        let text = format!("{host} original work");
        let first = context(&prompt(&e, host, "same-thread", Some("same-turn"), &text));
        assert!(first.contains("current event e-"), "{first}");
        assert_eq!(
            context(&prompt(&e, host, "same-thread", Some("same-turn"), &text)),
            first
        );
    }
    let conn =
        Connection::open(e.workspace.join(support::CONTROL_DIR).join("state.sqlite")).unwrap();
    let mut query = conn
        .prepare("SELECT host_kind,text,origin FROM learning_events ORDER BY host_kind")
        .unwrap();
    let events: Vec<(String, String, String)> = query
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(events.len(), 3);
    for (host, text, origin) in events {
        assert_eq!(text, format!("{host} original work"));
        assert_eq!(origin, "host_observed");
    }
}

#[test]
fn claude_missing_turn_allocates_distinct_events_while_codex_rejects_missing_identity() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    init(&e);
    start(&e, "claude", "thread");
    start(&e, "codex", "thread");
    for _ in 0..2 {
        let input = json!({"session_id":"thread","cwd":e.workspace,"hook_event_name":"UserPromptSubmit","prompt":"identical text"});
        let out = run_hook_with_host_flag(&e, &e.workspace, "user-prompt-submit", input, "claude");
        assert!(context(&out).contains("current event e-"));
    }
    let refused = prompt(&e, "codex", "thread", None, "must not persist");
    assert!(refused.status.success());
    assert!(refused.stdout.is_empty());
    assert_eq!(count(&e, "learning_events"), 2);
}

#[test]
fn ambiguous_host_flag_is_fail_open_without_capture() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    init(&e);
    start(&e, "codex", "thread");
    let input = json!({"session_id":"thread","cwd":e.workspace,"hook_event_name":"UserPromptSubmit","host_kind":"codex","turn_id":"a","prompt":"PRIVATE_UNACCEPTED"});
    let out = run_hook_with_host_flag(&e, &e.workspace, "user-prompt-submit", input, "claude");
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    assert_eq!(count(&e, "learning_events"), 0);
    let info = e.command().args(["info", "--json"]).output().unwrap();
    assert!(!String::from_utf8_lossy(&info.stdout).contains("PRIVATE_UNACCEPTED"));
}

#[test]
fn finalized_tutor_delivery_is_observed_separately_and_never_learner_work() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    init(&e);
    start(&e, "codex", "thread");
    prompt(
        &e,
        "codex",
        "thread",
        Some("t"),
        "b is the fixed coefficient",
    );
    let stop = json!({"session_id":"thread","cwd":e.workspace,"hook_event_name":"Stop","turn_id":"t","stop_hook_active":false,"last_assistant_message":"A coefficient labels a fixed quantity."});
    assert!(run_hook(&e, "stop", stop.clone()).status.success());
    assert!(run_hook(&e, "stop", stop).status.success());
    let conn =
        Connection::open(e.workspace.join(support::CONTROL_DIR).join("state.sqlite")).unwrap();
    assert_eq!(count(&e, "learning_events"), 2);
    let tutor: (String, String, String) = conn
        .query_row(
            "SELECT speaker,origin,kind FROM learning_events WHERE speaker='tutor'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        tutor,
        ("tutor".into(), "host_observed".into(), "delivery".into())
    );
    assert_eq!(count(&e, "observations"), 0);
}

#[test]
fn explicit_bound_root_cannot_follow_a_different_workspace() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    init(&e);
    let other = e.root.join("other");
    std::fs::create_dir(&other).unwrap();
    assert!(
        e.command_in(&other)
            .arg("init")
            .output()
            .unwrap()
            .status
            .success()
    );
    let mut mcp = McpClient::spawn_bound(&e, &other, Some(&e.workspace), true);
    mcp.initialize();
    let refused=mcp.request(2,"tools/call",json!({"name":"learning_step","arguments":{"schema":"learner-model/v2","action":"update","request_id":"wrong","session":{"operation":"start","client_ref":"s","goal":"No cross-workspace write"}}}));
    assert_eq!(refused["result"]["isError"], true, "{refused}");
    assert_eq!(count(&e, "learning_sessions"), 0);
    mcp.close_cleanly();
}

/// Claude Code 2.1.293 traffic as observed on the wire: MCP 2026-07-28
/// stateless requests carrying no conversation identity, the session in
/// `CLAUDE_CODE_SESSION_ID`, and hooks that name the turn only by `prompt_id`.
#[test]
fn claude_code_wire_traffic_binds_the_lesson_and_captures_both_sides_of_a_turn() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    init(&e);
    let meta = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": {"name": "claude-code", "version": "2.1.293"},
        "io.modelcontextprotocol/clientCapabilities": {"roots": {"listChanged": true}},
    });
    let mut mcp = McpClient::spawn_with_env(&e, true, &[("CLAUDE_CODE_SESSION_ID", "conv")]);
    let discovered = mcp.request(1, "server/discover", json!({"_meta": meta}));
    assert!(
        discovered["result"]["supportedVersions"]
            .as_array()
            .is_some_and(|versions| versions.contains(&json!("2026-07-28"))),
        "{discovered}"
    );
    // Claude Code drops every tool when a modern list result lacks cache hints.
    let listed = mcp.request(3, "tools/list", json!({"_meta": meta}));
    assert_eq!(listed["result"]["ttlMs"], 0, "{listed}");
    assert_eq!(listed["result"]["cacheScope"], "private", "{listed}");
    let started = mcp.request(
        2,
        "tools/call",
        json!({"_meta": meta, "name": "learning_step", "arguments": {"schema":"learner-model/v2","action":"update","request_id":"start","session":{"operation":"start","client_ref":"s","goal":"Learn coefficient roles"}}}),
    );
    assert_ne!(started["result"]["isError"], true, "{started}");
    mcp.close_cleanly();
    let mut legacy = McpClient::spawn(&e, true);
    legacy.initialize();
    let listed = legacy.request(2, "tools/list", json!({}));
    assert!(listed["result"].get("ttlMs").is_none(), "{listed}");
    assert!(listed["result"].get("cacheScope").is_none(), "{listed}");
    legacy.close_cleanly();

    let turn = json!({"session_id":"conv","transcript_path":"/t.jsonl","cwd":e.workspace,"prompt_id":"p-1","permission_mode":"default"});
    let mut prompt = turn.clone();
    prompt["hook_event_name"] = json!("UserPromptSubmit");
    prompt["prompt"] = json!("b is the fixed coefficient");
    let out = run_hook_with_host_flag(&e, &e.workspace, "user-prompt-submit", prompt, "claude");
    assert!(context(&out).contains("current event e-"), "{out:?}");
    let mut stop = turn;
    stop["hook_event_name"] = json!("Stop");
    stop["stop_hook_active"] = json!(false);
    stop["last_assistant_message"] = json!("A coefficient labels a fixed quantity.");
    let out = run_hook_with_host_flag(&e, &e.workspace, "stop", stop, "claude");
    assert!(out.status.success(), "{out:?}");

    let conn =
        Connection::open(e.workspace.join(support::CONTROL_DIR).join("state.sqlite")).unwrap();
    let events: Vec<(String, String, String)> = conn
        .prepare("SELECT host_kind,speaker,origin FROM learning_events ORDER BY event_no")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        events,
        [
            ("claude".into(), "learner".into(), "host_observed".into()),
            ("claude".into(), "tutor".into(), "host_observed".into()),
        ]
    );
}

/// The Claude plugin names its host on the MCP command line, so an inherited
/// Codex thread is ignored, and a conversation left by /clear is refused.
#[test]
fn claude_launched_server_trusts_only_claude_identity_and_refuses_a_cleared_conversation() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    init(&e);
    let meta = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": {"name": "claude-code", "version": "2.1.293"},
        "io.modelcontextprotocol/clientCapabilities": {},
    });
    let mut mcp = McpClient::spawn_claude(
        &e,
        &[
            ("CODEX_THREAD_ID", "outer-codex"),
            ("CLAUDE_CODE_SESSION_ID", "conv"),
        ],
    );
    mcp.request(1, "server/discover", json!({"_meta": meta}));
    let start = json!({"schema":"learner-model/v2","action":"update","request_id":"start","session":{"operation":"start","client_ref":"s","goal":"Coefficients"}});
    let started = mcp.request(
        2,
        "tools/call",
        json!({"_meta": meta, "name": "learning_step", "arguments": start}),
    );
    assert_ne!(started["result"]["isError"], true, "{started}");
    let kind: String =
        Connection::open(e.workspace.join(support::CONTROL_DIR).join("state.sqlite"))
            .unwrap()
            .query_row("SELECT host_kind FROM host_bindings", [], |r| r.get(0))
            .unwrap();
    assert_eq!(kind, "claude");

    let hook = |event: &str, name: &str, session: &str, field: &str, value: &str| {
        let mut input = json!({"session_id":session,"cwd":e.workspace,"hook_event_name":name});
        input[field] = json!(value);
        assert!(
            run_hook_with_host_flag(&e, &e.workspace, event, input, "claude")
                .status
                .success()
        );
    };
    hook("session-end", "SessionEnd", "conv", "reason", "clear");
    hook("session-start", "SessionStart", "conv-2", "source", "clear");
    let status = json!({"schema":"learner-model/v2","action":"status"});
    let refused = mcp.request(
        3,
        "tools/call",
        json!({"_meta": meta, "name": "context", "arguments": status}),
    );
    assert_eq!(refused["result"]["isError"], true, "{refused}");
    assert!(
        refused.to_string().contains("conversation_changed"),
        "{refused}"
    );

    // Resuming that conversation makes it current again.
    hook("session-start", "SessionStart", "conv", "source", "resume");
    let allowed = mcp.request(
        4,
        "tools/call",
        json!({"_meta": meta, "name": "context", "arguments": status}),
    );
    assert_ne!(allowed["result"]["isError"], true, "{allowed}");
    mcp.close_cleanly();
}

/// Codex started from a Claude Code shell inherits `CLAUDE_CODE_SESSION_ID`.
/// The Codex plugin names its host on the MCP command line, so that inherited
/// Claude session never binds the lesson.
#[test]
fn codex_launched_server_ignores_an_inherited_claude_session() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    init(&e);
    let mut mcp = McpClient::spawn_codex(&e, &[("CLAUDE_CODE_SESSION_ID", "outer-claude")]);
    mcp.initialize();
    let start = json!({"schema":"learner-model/v2","action":"update","request_id":"start","session":{"operation":"start","client_ref":"s","goal":"Coefficients"}});
    mcp.request(
        2,
        "tools/call",
        json!({"name": "learning_step", "arguments": start}),
    );
    let claude_bindings: i64 =
        Connection::open(e.workspace.join(support::CONTROL_DIR).join("state.sqlite"))
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM host_bindings WHERE host_kind = 'claude'",
                [],
                |r| r.get(0),
            )
            .unwrap();
    assert_eq!(claude_bindings, 0);
    mcp.close_cleanly();
}

/// Claude's desktop app wraps pasted text in `pasted_content` tag lines that
/// reach the prompt hook but that the model is told not to repeat. A tutor's
/// echo starts a lesson from the native ticket whether it copies the whole
/// prompt or only the learner's words, and both capture routes store the
/// learner's words without the markup.
#[test]
fn claude_desktop_paste_markup_neither_blocks_native_capture_nor_reaches_storage() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    init(&e);
    let meta = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": {"name": "claude-code", "version": "2.1.293"},
        "io.modelcontextprotocol/clientCapabilities": {},
    });
    let hook = |conv: &str, prompt_id: &str, text: &str| {
        let input = json!({"session_id":conv,"transcript_path":"/t.jsonl","cwd":e.workspace,"hook_event_name":"UserPromptSubmit","prompt_id":prompt_id,"prompt":text});
        context(&run_hook_with_host_flag(
            &e,
            &e.workspace,
            "user-prompt-submit",
            input,
            "claude",
        ))
    };
    // Magnesium reports a refused update in the receipt, not as an MCP error.
    let start = |conv: &str, prompt: &str, echo: &str| {
        let notice = hook(conv, "p-1", prompt);
        let ticket = notice
            .split_once("native_capture.ticket=")
            .expect("inactive conversation offers native proof")
            .1
            .split(';')
            .next()
            .unwrap();
        let mut mcp = McpClient::spawn_claude(&e, &[("CLAUDE_CODE_SESSION_ID", conv)]);
        mcp.request(1, "server/discover", json!({"_meta": meta}));
        let start = json!({"schema":"learner-model/v2","action":"update","request_id":format!("start-{conv}"),"native_capture":{"ticket":ticket,"text":echo},"session":{"operation":"start","client_ref":"s","goal":"Normal equations"}});
        let started = mcp.request(
            2,
            "tools/call",
            json!({"_meta": meta, "name": "learning_step", "arguments": start}),
        );
        mcp.close_cleanly();
        let receipt: serde_json::Value =
            serde_json::from_str(started["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap_or_else(|_| panic!("{started}"));
        assert!(receipt.get("error").is_none(), "{receipt}");
    };
    let wrap = |id: &str, inner: &str| {
        format!("\n\n<pasted_content id=\"{id}\">\n{inner}\n</pasted_content id=\"{id}\">\n")
    };
    let words = "I am not clear on these.\n$$X^\\top X w = X^\\top t$$";
    let code = "    if x:\n        return x";
    start("whole", &wrap("b185", words), &wrap("b185", words));
    start("inner", &wrap("c0de", code), code);

    // While the lesson is active the hook records the turn itself.
    assert!(
        hook(
            "whole",
            "p-2",
            "Also\n\n<pasted_content id=\"9bc0\">\nX = 1\n</pasted_content id=\"9bc0\">\n\nwhy?"
        )
        .contains("current event e-")
    );
    let conn =
        Connection::open(e.workspace.join(support::CONTROL_DIR).join("state.sqlite")).unwrap();
    let events: Vec<(String, String)> = conn
        .prepare(
            "SELECT origin,text FROM learning_events WHERE speaker='learner' ORDER BY event_no",
        )
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        events,
        [
            ("host_observed".into(), words.into()),
            ("host_observed".into(), code.into()),
            ("host_observed".into(), "Also\n\nX = 1\n\nwhy?".into()),
        ]
    );
}
