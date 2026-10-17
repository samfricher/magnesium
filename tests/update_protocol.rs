//! Real MCP schema loading and rejected/repaired UPDATE boundaries.
use rusqlite::Connection;
use serde_json::{Value, json};
use support::{TestEnvironment, mcp::McpClient, v2};

fn call(client: &mut McpClient, id: u64, request: Value) -> Value {
    let response = client.request(
        id,
        "tools/call",
        json!({"name":"learning_step","arguments":request}),
    );
    serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn live_mcp_requires_current_v2_structured_fields_and_rejects_retired_fields() {
    let environment = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&environment);
    let mut client = McpClient::spawn_with_env(
        &environment,
        true,
        &[("OPENCODE_SESSION_ID", "opencode-v2-contract")],
    );
    client.initialize();
    let listed = client.request(2, "tools/list", json!({}));
    let advertised = listed.to_string();
    for guidance in [
        "record.current_basis selects 1–8 observations",
        "Use self_report for an explicitly stated preference",
        "This is not an append-only history",
        "this object has no reason field",
        "not in a metadata.image_path field",
    ] {
        assert!(
            advertised.contains(guidance),
            "missing tool guidance: {guidance}"
        );
    }

    let schema = &listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "learning_step")
        .unwrap()["inputSchema"];
    assert_eq!(schema["type"], "object");
    let update = schema["anyOf"]
        .as_array()
        .unwrap()
        .iter()
        .find(|branch| branch["properties"]["memory_edits"].is_object())
        .unwrap();
    assert_eq!(update["additionalProperties"], false);
    let properties = &update["properties"];
    for field in [
        "reported_events",
        "observations",
        "memory_edits",
        "event_dispositions",
        "review_updates",
    ] {
        assert_eq!(properties[field]["type"], "array", "{field}");
    }
    let session_variants = properties["session"]["anyOf"].as_array().unwrap();
    assert!(
        session_variants
            .iter()
            .any(|variant| variant["type"] == "null")
    );
    let session = session_variants
        .iter()
        .find(|variant| variant["type"] != "null")
        .unwrap();
    for operation in session["oneOf"].as_array().unwrap() {
        assert_eq!(operation["type"], "object");
        assert_eq!(operation["additionalProperties"], false);
    }
    assert_eq!(properties["checkpoint"]["type"], json!(["object", "null"]));
    assert_eq!(properties["checkpoint"]["additionalProperties"], false);
    let valid = json!({
        "schema": "learner-model/v2",
        "action": "update",
        "request_id": "v2-structured",
        "session": {
            "operation": "start",
            "client_ref": "session",
            "goal": "Learn coefficient roles",
        },
    });
    let mut id = 3;
    for (field, value) in [
        ("session", valid["session"].to_string()),
        ("reported_events", "[]".to_owned()),
        ("observations", "[]".to_owned()),
        ("memory_edits", "[]".to_owned()),
        ("checkpoint", "{}".to_owned()),
        ("event_dispositions", "[]".to_owned()),
        ("review_updates", "[]".to_owned()),
    ] {
        let mut request = valid.clone();
        request["request_id"] = json!(format!("string-{field}"));
        request[field] = json!(value);
        let rejected = call(&mut client, id, request);
        id += 1;
        assert_eq!(rejected["error"]["code"], "invalid_variant", "{rejected}");
        assert_eq!(
            rejected["error"]["recovery"]["field_path"], field,
            "{rejected}"
        );
        assert_eq!(rejected["error"]["recovery"]["tool"], "context");
        assert_eq!(
            rejected["error"]["recovery"]["arguments"]["host"],
            "opencode"
        );
        assert_eq!(v2::count(&environment, "learning_sessions"), 0);
        assert_eq!(v2::count(&environment, "observations"), 0);
        assert_eq!(v2::count(&environment, "learner_memories"), 0);
    }
    for (field, value) in [
        ("sources", json!({"path": "lesson.md"})),
        ("enabled", json!(true)),
    ] {
        let mut request = valid.clone();
        request["request_id"] = json!(format!("retired-{field}"));
        request[field] = value;
        let rejected = call(&mut client, id, request);
        id += 1;
        assert_eq!(rejected["error"]["code"], "invalid_variant", "{rejected}");
        assert_eq!(
            rejected["error"]["recovery"]["field_path"], field,
            "{rejected}"
        );
        assert!(
            rejected["error"]["message"]
                .as_str()
                .unwrap()
                .contains("unknown field")
        );
        assert_eq!(v2::count(&environment, "learning_sessions"), 0);
        assert_eq!(v2::count(&environment, "observations"), 0);
        assert_eq!(v2::count(&environment, "learner_memories"), 0);
    }
    let saved = call(&mut client, id, valid);
    assert_eq!(saved["update_status"], "committed", "{saved}");
    assert_eq!(saved["host_kind"], "opencode");
    assert_eq!(
        saved["host_thread_id"],
        "magnesium-h1:opencode:opencode-v2-contract"
    );
    let session = saved["references"]["session"].as_str().unwrap();
    let connection = Connection::open(
        environment
            .workspace
            .join(support::CONTROL_DIR)
            .join("state.sqlite"),
    )
    .unwrap();
    let stored: (String, String) = connection
        .query_row("SELECT session_id,goal FROM learning_sessions", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    assert_eq!(
        stored,
        (session.to_owned(), "Learn coefficient roles".to_owned())
    );
    assert_eq!(v2::count(&environment, "learning_sessions"), 1);
    client.close_cleanly();
}

#[test]
fn live_mcp_role_and_disposition_repair_retains_native_capture_and_checkpoint() {
    let environment = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&environment);
    let thread = "typed-update";
    let text = "I solved x + 3 = 5 and got x = 2.";
    let notice = v2::context(&v2::prompt(
        &environment,
        "codex",
        thread,
        Some("first"),
        text,
    ));
    let ticket = notice
        .split_once("native_capture.ticket=")
        .unwrap()
        .1
        .split(';')
        .next()
        .unwrap();
    let mut client = McpClient::spawn_with_env(&environment, true, &[("CODEX_THREAD_ID", thread)]);
    client.initialize();
    let listed = client.request(2, "tools/list", json!({}));
    let schema = &listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "learning_step")
        .unwrap()["inputSchema"];
    assert_eq!(schema["type"], "object");
    let branches = schema["anyOf"].as_array().unwrap();
    let update = branches
        .iter()
        .find(|branch| branch["properties"]["memory_edits"].is_object())
        .unwrap();
    let properties = &update["properties"];
    assert_eq!(
        properties["observations"]["items"]["properties"]["events"]["items"]["properties"]["role"]
            ["enum"],
        json!([
            "learner_response",
            "task_context",
            "assistance",
            "teaching_context",
            "check_result",
            "correction_context"
        ])
    );
    assert_eq!(
        properties["event_dispositions"]["items"]["properties"]["outcome"]["enum"],
        json!(["processed", "no_change"])
    );

    let checkpoint = json!({"current_problem":"A linear equation","working_diagnosis":"One successful rearrangement","next_action":"Ask for the reasoning","memory_refs":[{"memory_id":"memory","revision":"after_edit"}]});
    let mut request = json!({"schema":"learner-model/v2","action":"update","request_id":"invalid-role","native_capture":{"ticket":ticket,"text":text},"session":{"operation":"start","client_ref":"lesson","goal":"Explain rearrangement"},"observations":[{"client_ref":"work","kind":"learner_work","description":"Learner reports x=2 for x+3=5","events":[{"event_id":"pending","role":"answer"}]}],"memory_edits":[{"operation":"create","client_ref":"memory","record":{"title":"Linear rearrangement","scope":"algebra","summary":"One correct reported solution","state_text":"The learner reported x=2; the reasoning is not yet established.","current_basis":[{"new_observation":"work"}]},"reason":"Describe the reported work","change_basis":[{"new_observation":"work"}]}],"checkpoint":checkpoint});
    let rejected = call(&mut client, 3, request.clone());
    assert_eq!(
        rejected["error"]["recovery"]["field_path"], "observations[0].events[0].role",
        "{rejected}"
    );
    let events = rejected["error"]["captured_event_ids"].clone();
    assert_eq!(events.as_array().unwrap().len(), 1);
    let event = events[0].as_str().unwrap();
    assert_eq!(v2::count(&environment, "learning_events"), 1);
    assert_eq!(v2::count(&environment, "learning_sessions"), 0);

    request["request_id"] = json!("invalid-disposition-reason");
    request["observations"][0]["events"][0]["role"] = json!("learner_response");
    request["observations"][0]["events"][0]["event_id"] = json!(event);
    request["event_dispositions"] =
        json!([{"event_id":event,"outcome":"processed","reason":"unsupported extra field"}]);
    let rejected = call(&mut client, 4, request.clone());
    assert_eq!(
        rejected["error"]["recovery"]["field_path"], "event_dispositions[0].reason",
        "{rejected}"
    );
    assert_eq!(rejected["error"]["captured_event_ids"], events);
    assert_eq!(v2::count(&environment, "learning_events"), 1);
    assert_eq!(v2::count(&environment, "observations"), 0);

    request["request_id"] = json!("repaired");
    request["event_dispositions"][0]
        .as_object_mut()
        .unwrap()
        .remove("reason");
    let saved = call(&mut client, 5, request);
    assert_eq!(saved["update_status"], "committed", "{saved}");
    assert_eq!(saved["captured_event_ids"], events);
    assert_eq!(v2::count(&environment, "learning_events"), 1);
    let connection = Connection::open(
        environment
            .workspace
            .join(support::CONTROL_DIR)
            .join("state.sqlite"),
    )
    .unwrap();
    let stored: String = connection
        .query_row("SELECT checkpoint_json FROM learning_sessions", [], |row| {
            row.get(0)
        })
        .unwrap();
    let stored: Value = serde_json::from_str(&stored).unwrap();
    assert_eq!(stored["next_action"], checkpoint["next_action"]);
    assert_eq!(
        stored["memory_refs"][0]["memory_id"],
        saved["references"]["memory"]
    );
    assert_eq!(stored["memory_refs"][0]["revision"], 1);
    client.close_cleanly();
}
