//! The same v2 JSON API is served by CLI and MCP; no retired assessment aliases.
use serde_json::json;
use support::mcp::McpClient;
use support::{TestEnvironment, v2::*};

#[test]
fn adapter_call_protocol_mismatch_fails_closed() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    let output = e
        .command()
        .args([
            "adapter",
            "call",
            "context",
            "--protocol-version",
            "999",
            "--json-input",
            "{}",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!e.workspace.join(support::CONTROL_DIR).exists());
}
#[test]
fn inactive_context_does_not_initialize_then_active_status_and_prepare_are_bounded() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    let status = tool(
        &e,
        "codex",
        "thread",
        "context",
        json!({"schema":"learner-model/v2","action":"status"}),
    );
    assert_eq!(status["active"], false);
    assert!(!e.workspace.join(support::CONTROL_DIR).exists());
    init(&e);
    let status = tool(
        &e,
        "codex",
        "thread",
        "context",
        json!({"schema":"learner-model/v2","action":"status"}),
    );
    assert_eq!(status["schema"], "learner-model/v2");
    assert!(status["workspace_id"].is_string());
    let context = tool(
        &e,
        "codex",
        "thread",
        "context",
        json!({"schema":"learner-model/v2","action":"prepare","goal":"quadratic coefficients"}),
    );
    assert_eq!(context["coverage"]["catalog_complete"], true);
    assert_eq!(context["memories"], json!([]));
    assert!(context.to_string().len() < 24 * 1024);
}
#[test]
fn cli_and_mcp_current_directory_match_without_creating_learning_events() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    init(&e);
    let request = json!({"schema":"learner-model/v2","action":"browse"});
    let cli = tool(&e, "codex", "thread", "memory", request.clone());
    let mut client = McpClient::spawn_with_env(&e, true, &[("CODEX_THREAD_ID", "thread")]);
    client.initialize();
    let mcp = client.call_value(2, "memory", request);
    assert_eq!(cli, mcp);
    assert_eq!(count(&e, "learning_events"), 0);
    client.close_cleanly();
}
#[test]
fn source_search_remains_separate_and_reads_plain_sources() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    init(&e);
    std::fs::write(
        e.workspace.join("algebra.md"),
        "A fixed coefficient is different from an unknown.",
    )
    .unwrap();
    support::sources::prepare_text_fixture_separately(
        &e,
        "algebra.md",
        "A fixed coefficient is different from an unknown.",
    );
    let result = tool(
        &e,
        "codex",
        "thread",
        "search_sources",
        json!({"query":"coefficient","limit":4}),
    );
    assert!(
        result["hits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|hit| hit["path"] == "algebra.md")
    );
    assert_eq!(count(&e, "learning_events"), 0);
}
