//! Real subprocess API/schema checks. These do not simulate a tutoring model.
use rusqlite::Connection;
use serde_json::{Value, json};
use support::mcp::McpClient;
use support::{TestEnvironment, v2::*};

fn action_schema<'a>(schema: &'a Value, action: &str) -> Option<&'a Value> {
    if schema["properties"]["action"]["const"] == action {
        return Some(schema);
    }
    match schema {
        Value::Object(object) => object
            .values()
            .find_map(|child| action_schema(child, action)),
        Value::Array(array) => array.iter().find_map(|child| action_schema(child, action)),
        _ => None,
    }
}

#[test]
fn advertised_context_and_memory_shapes_accept_their_documented_read_actions() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    init(&e);
    let c = Connection::open(e.workspace.join(support::CONTROL_DIR).join("state.sqlite")).unwrap();
    c.execute("INSERT INTO learner_memories(memory_id,title,scope,summary,state_text,current_basis_json,lifecycle,revision,created_at_ms,updated_at_ms) VALUES('m','Symbol roles','Algebra','Substitution is sound; labels uncertain','Do not infer a factoring gap.','[]','active',1,0,0)",[]).unwrap();
    c.execute("INSERT INTO observations(observation_id,kind,description,source_refs_json,created_at_ms) VALUES('o','question','Which letter is fixed?','[]',0)",[]).unwrap();
    c.execute("INSERT INTO learning_events(event_id,host_kind,host_thread_id,external_event_key,speaker,kind,origin,text,metadata_json,captured_at_ms,processing) VALUES('e','test','seed','turn','learner','question','model_reported','b ≠ x','{}',0,'pending')",[]).unwrap();
    c.execute(
        "INSERT INTO observation_events VALUES('o','e','learner_response','[]')",
        [],
    )
    .unwrap();
    c.execute("INSERT INTO memory_observations VALUES('m','o')", [])
        .unwrap();
    let mut client = McpClient::spawn_with_env(&e, true, &[("CODEX_THREAD_ID", "read-surface")]);
    client.initialize();
    let listed = client.request(2, "tools/list", json!({}));
    let tools = listed["result"]["tools"].as_array().unwrap();
    let context_schema =
        tools.iter().find(|t| t["name"] == "context").unwrap()["inputSchema"].to_string();
    for required in [
        "instructions",
        "orient_and_probe",
        "prepare",
        "session_choices",
        "pending_events",
    ] {
        assert!(context_schema.contains(required), "missing {required}");
    }
    let memory_schema = &tools.iter().find(|t| t["name"] == "memory").unwrap()["inputSchema"];
    let read = action_schema(memory_schema, "read").expect("typed read variant");
    let search = action_schema(memory_schema, "search").expect("typed search variant");
    let observations =
        action_schema(memory_schema, "observations").expect("typed observations variant");
    assert_eq!(read["properties"]["refs"]["maxItems"], 32);
    assert_eq!(search["properties"]["intents"]["maxItems"], 4);
    assert_eq!(search["properties"]["literal_terms"]["maxItems"], 4);
    for key in ["observation_id", "event_id", "cursor"] {
        assert!(
            observations["properties"].get(key).is_some(),
            "missing observations.{key}"
        );
    }
    assert!(
        action_schema(memory_schema, "history").unwrap()["properties"]
            .get("intents")
            .is_none()
    );
    assert!(action_schema(memory_schema, "prepare").is_none());
    assert!(action_schema(memory_schema, "pending_work").is_some());
    let instructions = client.call_value(
        3,
        "context",
        json!({"action":"instructions","host":"codex","stage":"orient_and_probe"}),
    );
    assert_eq!(instructions["status"], "served", "{instructions}");
    assert!(
        instructions["files"]
            .as_array()
            .unwrap()
            .contains(&json!("workflows/orient-and-probe.md")),
        "{instructions}"
    );
    assert!(
        instructions["text"]
            .as_str()
            .is_some_and(|text| !text.trim().is_empty()),
        "{instructions}"
    );
    for (i,(name,mut request)) in [
        ("context",json!({"action":"status"})),
        ("context",json!({"action":"prepare","mode":"auto","goal":"quadratic coefficients"})),
        ("context",json!({"action":"session_choices"})),
        ("context",json!({"action":"pending_events"})),
        ("memory",json!({"action":"search","literal_terms":["labels"]})),
        ("memory",json!({"action":"browse"})),
        ("memory",json!({"action":"topics"})),
        ("memory",json!({"action":"read","refs":[{"kind":"current_memory","id":"m"},{"kind":"observation","id":"o"},{"kind":"event","id":"e","start_byte":0,"end_byte":1}]})),
        ("memory",json!({"action":"history","memory_id":"m"})),
        ("memory",json!({"action":"observations","memory_id":"m"})),
        ("memory",json!({"action":"observations","kind":"event","literal_terms":["≠"]})),
        ("memory",json!({"action":"observations","kind":"memory_refs","observation_id":"o"})),
        ("memory",json!({"action":"observations","kind":"event_refs","observation_id":"o"})),
        ("memory",json!({"action":"observations","kind":"event_memory_refs","event_id":"e"})),
    ].into_iter().enumerate() {
        request["schema"]=json!("learner-model/v2");let p=client.call_value(10+i as u64,name,request);
        assert_eq!(p["schema"],"learner-model/v2");assert!(serde_json::to_vec(&p).unwrap().len()<=24576,"{name} {p}");
    }
    client.close_cleanly();
    assert_eq!(count(&e, "learning_events"), 1);
    assert_eq!(count(&e, "context_receipts"), 0);
}

#[test]
fn status_receipt_matches_actual_cli_and_mcp_packets_with_identity() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    init(&e);
    start(&e, "codex", "receipt-thread");
    let request = json!({"schema":"learner-model/v2","action":"status"});
    let cli = tool(&e, "codex", "receipt-thread", "context", request.clone());
    let mut client = McpClient::spawn_with_env(&e, true, &[("CODEX_THREAD_ID", "receipt-thread")]);
    client.initialize();
    let mcp = client.call_value(2, "context", request);
    let c = Connection::open(e.workspace.join(support::CONTROL_DIR).join("state.sqlite")).unwrap();
    for packet in [cli, mcp] {
        assert_eq!(
            packet["identity"]["teaching_policy_version"],
            "teaching-policy/v2"
        );
        let stored: String = c
            .query_row(
                "SELECT response_json FROM context_receipts WHERE trace_id=?1",
                [packet["trace_id"].as_str().unwrap()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(serde_json::from_str::<Value>(&stored).unwrap(), packet);
    }
    client.close_cleanly();
}

#[test]
fn typed_source_prefix_and_original_anchor_contract_match_the_live_schema() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    init(&e);
    for dir in ["inside", "outside"] {
        std::fs::create_dir(e.workspace.join(dir)).unwrap();
        std::fs::write(
            e.workspace.join(dir).join("equation.md"),
            "orbitconstant: b ≠ x; a fixed coefficient is not the unknown.\n",
        )
        .unwrap();
    }
    let mut client = McpClient::spawn(&e, true);
    client.initialize();
    let listed = client.request(2, "tools/list", json!({}));
    let tools = listed["result"]["tools"].as_array().unwrap();
    let schema = &tools
        .iter()
        .find(|t| t["name"] == "search_sources")
        .unwrap()["inputSchema"];
    assert!(schema["properties"].get("path_prefix").is_some());
    assert_eq!(schema["additionalProperties"], false);
    support::sources::prepare_text_fixture(
        &e,
        &mut client,
        1000,
        "inside/equation.md",
        "orbitconstant: b ≠ x; a fixed coefficient is not the unknown.\n",
    );
    support::sources::prepare_text_fixture(
        &e,
        &mut client,
        2000,
        "outside/equation.md",
        "orbitconstant: b ≠ x; a fixed coefficient is not the unknown.\n",
    );
    let found = client.call_value(
        3,
        "search_sources",
        json!({"query":"orbitconstant","path_prefix":"inside","limit":8}),
    );
    let hits = found["hits"].as_array().unwrap();
    assert!(!hits.is_empty());
    assert!(
        hits.iter()
            .all(|h| h["path"].as_str().unwrap().starts_with("inside/"))
    );
    let h = &hits[0];
    let original=client.call_value(4,"sources",json!({"action":"inspect","path":h["path"],"content_sha256":h["content_sha256"],"anchor_kind":h["anchor_kind"],"anchor_start":h["anchor_start"],"anchor_end":h["anchor_end"]}));
    assert!(
        original["original"]["text"]
            .as_str()
            .unwrap()
            .contains("b ≠ x")
    );
    client.close_cleanly();
    assert_eq!(count(&e, "learning_events"), 0);
}
