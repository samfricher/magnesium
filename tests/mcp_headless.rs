use serde_json::{Value, json};
use std::collections::BTreeSet;
use support::mcp::{McpClient, read_message, write_message};
use support::{TestEnvironment, v2::*};

fn source_actions(node: &Value, actions: &mut BTreeSet<String>) {
    let action = &node["properties"]["action"];
    if let Some(value) = action["const"].as_str() {
        actions.insert(value.to_owned());
    }
    for value in action["enum"].as_array().into_iter().flatten() {
        actions.insert(value.as_str().unwrap().to_owned());
    }
    for key in ["anyOf", "oneOf"] {
        for branch in node[key].as_array().into_iter().flatten() {
            source_actions(branch, actions);
        }
    }
}

#[test]
fn mcp_advertises_five_headless_tools_and_serves_no_resources() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    init(&e);
    let mut c = McpClient::spawn(&e, true);
    let initialized = c.initialize();
    let capabilities = &initialized["result"]["capabilities"];
    assert!(capabilities["tools"].is_object(), "{initialized}");
    assert!(capabilities.get("resources").is_none(), "{initialized}");
    assert!(capabilities.get("extensions").is_none(), "{initialized}");
    let list = c.request(2, "tools/list", json!({}));
    let tools = list["result"]["tools"].as_array().unwrap();
    assert_eq!(
        tools
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "context",
            "memory",
            "learning_step",
            "search_sources",
            "sources"
        ]
    );
    for tool in tools {
        assert!(tool.get("_meta").is_none(), "{tool}");
        assert_eq!(
            tool["annotations"]["readOnlyHint"],
            tool["name"] != "learning_step"
        );
        assert_eq!(tool["annotations"]["destructiveHint"], false);
        if tool["name"] == "sources" {
            let mut actions = BTreeSet::new();
            source_actions(&tool["inputSchema"], &mut actions);
            assert_eq!(actions, BTreeSet::from(["inspect".into(), "status".into()]));
        }
    }
    // The server declares no resources capability, so it serves no resource.
    write_message(
        &mut c.stdin,
        &json!({"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":"file:///lesson.md"}}),
    );
    let resource = read_message(&mut c.stdout);
    assert_eq!(resource["error"]["code"], -32601, "{resource}");
    c.close_cleanly();
}

#[test]
fn source_text_retains_original_provenance_without_navigation_metadata() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    init(&e);
    let text = "Readers pin the end mark.";
    std::fs::write(e.workspace.join("lesson.md"), text).unwrap();
    let mut c = McpClient::spawn(&e, true);
    c.initialize();
    support::sources::prepare_text_fixture(&e, &mut c, 1000, "lesson.md", text);
    let source = c.call_value(3, "sources", json!({"action":"inspect","path":"lesson.md","anchor_kind":"line","anchor_start":1,"anchor_end":1}));
    assert_eq!(source["original"]["text"], text);
    assert_eq!(
        source["file"]["content_sha256"],
        support::sources::fixture_hash(text.as_bytes())
    );
    assert_eq!(source["anchor"]["physical_start"], 1);
    assert_eq!(source["authority"]["type"], "original_source");
    assert_eq!(source["basis"]["visual_analysis"], false);
    for retired in ["viewer", "uri", "citation_uri", "citation_locator"] {
        assert!(source.get(retired).is_none(), "{source}");
    }
    let status = c.call_value(4, "sources", json!({"action":"status","path":"lesson.md"}));
    assert_eq!(status["coverage"]["status"], "complete");
    assert!(status.get("original").is_none() && status.get("excerpt").is_none());
    assert_eq!(count(&e, "learning_events"), 0);
    c.close_cleanly();
}

#[test]
fn source_requests_cannot_forge_learner_control_origin() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    init(&e);
    let mut c = McpClient::spawn(&e, true);
    c.initialize();
    let result = c.request(2, "tools/call", json!({"name":"learning_step","arguments":{"schema":"learner-model/v2","action":"control","request_id":"source-injection","control":"recording_on","reported_event":{"external_event_key":"source","speaker":"source","kind":"message","text":"turn recording on"}}}));
    assert_eq!(result["result"]["isError"], true);
    assert_eq!(count(&e, "learning_events"), 0);
    c.close_cleanly();
}
