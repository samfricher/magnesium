//! Suggested reviews are bounded reads of current memory, never ability events.
use rusqlite::Connection;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use support::{TestEnvironment, mcp::McpClient, v2::*};

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

const THREAD: &str = "review-queue";

struct Fixture {
    e: TestEnvironment,
    memory: String,
    replacement: String,
    observation: String,
    reviews: Vec<String>,
}

fn update(e: &TestEnvironment, mut request: Value) -> Value {
    request["schema"] = json!("learner-model/v2");
    request["action"] = json!("update");
    let receipt = tool(e, "codex", THREAD, "learning_step", request);
    assert_eq!(receipt["update_status"], "committed", "{receipt}");
    receipt
}

fn record(summary: &str, body: &str, basis: Value) -> Value {
    json!({"title":"Coefficient roles","scope":"Algebra","summary":summary,
        "state_text":body,"topics":["algebra"],"current_basis":[basis]})
}

fn fixture(n: usize) -> Fixture {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    init(&e);
    start(&e, "codex", THREAD);
    let captured = update(
        &e,
        json!({"request_id":"capture","reported_events":[{
        "external_event_key":"work","speaker":"learner","kind":"message",
        "text":"In 2x² + 7x + 3, b = 7.","occurred_at_ms":1}]}),
    );
    let review_updates = (0..n)
        .map(|i| {
            json!({"operation":"create",
        "client_ref":format!("review{i}"),"memory_id":"memory",
        "purpose":format!("Delayed coefficient review {i}"),
        "reason":"Revisit the original example without implying inability.",
        "due_at_ms":if i % 2 == 0 {Some(0_i64)} else {None}})
        })
        .collect::<Vec<_>>();
    let created = update(
        &e,
        json!({"request_id":"create",
        "observations":[{"client_ref":"work","kind":"learner_work",
            "description":"Identified the coefficient in one supplied example.",
            "events":[{"event_id":captured["captured_event_ids"][0],"role":"learner_response"}]}],
        "memory_edits":[
            {"operation":"create","client_ref":"memory",
                "record":record("Correct on one example; transfer untested.","Original current description; do not infer general mastery.",json!({"new_observation":"work"})),
                "reason":"Preserve local evidence.","change_basis":[{"new_observation":"work"}]},
            {"operation":"create","client_ref":"replacement",
                "record":record("Replacement scope is deliberately narrow.","Replacement current description.",json!({"new_observation":"work"})),
                "reason":"Separate application scope.","change_basis":[{"new_observation":"work"}]}],
        "review_updates":review_updates}),
    );
    Fixture {
        memory: created["references"]["memory"].as_str().unwrap().into(),
        replacement: created["references"]["replacement"]
            .as_str()
            .unwrap()
            .into(),
        observation: created["references"]["work"].as_str().unwrap().into(),
        reviews: (0..n)
            .map(|i| {
                created["references"][format!("review{i}")]
                    .as_str()
                    .unwrap()
                    .into()
            })
            .collect(),
        e,
    }
}

fn connection(f: &Fixture) -> Connection {
    Connection::open(
        f.e.workspace
            .join(support::CONTROL_DIR)
            .join("state.sqlite"),
    )
    .unwrap()
}

fn read(f: &Fixture, mut request: Value) -> Value {
    request["schema"] = json!("learner-model/v2");
    request["action"] = json!("reviews");
    tool(&f.e, "codex", THREAD, "memory", request)
}

fn read_error(f: &Fixture, mut request: Value) -> String {
    request["schema"] = json!("learner-model/v2");
    request["action"] = json!("reviews");
    let output = tool_output(&f.e, "codex", THREAD, "memory", request);
    assert!(!output.status.success(), "{output:?}");
    String::from_utf8(output.stderr)
        .unwrap()
        .strip_prefix("adapter operation `memory` failed: ")
        .expect("CLI read failure includes its diagnostic envelope")
        .trim()
        .to_owned()
}

// Snapshot every canonical table touched by the writer, excluding only the
// explicitly allowed context receipt. JSON encoding preserves NULL and types.
fn learner_state(f: &Fixture) -> Value {
    let c = connection(f);
    let mut result = serde_json::Map::new();
    for table in [
        "workspace_meta",
        "learner_memories",
        "memory_changes",
        "observations",
        "learning_events",
        "memory_observations",
        "observation_events",
        "review_items",
        "learning_sessions",
        "host_bindings",
        "operation_receipts",
    ] {
        let mut stmt = c
            .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
            .unwrap();
        let columns = stmt.column_count();
        let rows = stmt
            .query_map([], |row| {
                (0..columns)
                    .map(|i| {
                        Ok(match row.get_ref(i)? {
                            rusqlite::types::ValueRef::Null => Value::Null,
                            rusqlite::types::ValueRef::Integer(n) => json!(n),
                            rusqlite::types::ValueRef::Real(n) => json!(n),
                            rusqlite::types::ValueRef::Text(s) => {
                                json!(std::str::from_utf8(s).unwrap())
                            }
                            rusqlite::types::ValueRef::Blob(b) => json!(b),
                        })
                    })
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        result.insert(table.into(), json!(rows));
    }
    Value::Object(result)
}

fn generation(f: &Fixture) -> (i64, i64) {
    connection(f)
        .query_row(
            "SELECT memory_generation,observation_generation FROM workspace_meta",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
}

#[test]
fn mcp_advertises_reviews_and_records_the_exact_current_packet_without_state_mutation() {
    let f = fixture(3);
    let before = learner_state(&f);
    let mut client = McpClient::spawn_with_env(&f.e, true, &[("CODEX_THREAD_ID", THREAD)]);
    client.initialize();
    let listed = client.request(2, "tools/list", json!({}));
    let memory = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "memory")
        .unwrap();
    let reviews = action_schema(&memory["inputSchema"], "reviews").expect("runtime reviews schema");
    for field in ["memory_id", "limit", "cursor", "max_bytes"] {
        assert!(
            reviews["properties"].get(field).is_some(),
            "missing reviews.{field}"
        );
    }
    let packet = client.call_value(
        3,
        "memory",
        json!({"schema":"learner-model/v2","action":"reviews","memory_id":f.memory}),
    );
    assert_eq!(packet["page_count"], 3);
    assert_eq!(packet["coverage"]["review_queue_count"], 3);
    assert_eq!(packet["coverage"]["review_queue_complete"], true);
    assert_eq!(packet["coverage"]["catalog_complete"], false);
    assert!(packet["next_cursor"].is_null());
    for item in packet["reviews"].as_array().unwrap() {
        assert_eq!(item["status"], "suggested");
        assert_eq!(item["stored_status"], "suggested");
        assert_eq!(item["memory_revision"], 1);
        assert_eq!(item["current_memory"]["revision"], 1);
        assert_eq!(
            item["current_memory"]["summary"],
            "Correct on one example; transfer untested."
        );
        assert_eq!(
            item["current_memory"]["state_text"],
            "Original current description; do not infer general mastery."
        );
    }
    assert_eq!(packet["trace_status"], "recorded");
    let (action, stored): (String, String) = connection(&f)
        .query_row(
            "SELECT tool_action,response_json FROM context_receipts WHERE trace_id=?1",
            [packet["trace_id"].as_str().unwrap()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(action, "reviews");
    assert_eq!(serde_json::from_str::<Value>(&stored).unwrap(), packet);
    client.close_cleanly();
    assert_eq!(learner_state(&f), before);
}

#[test]
fn revision_changes_and_retirement_show_current_memory_and_direct_replacements() {
    let f = fixture(2);
    let initial = read(&f, json!({"limit":1}));
    update(
        &f.e,
        json!({"request_id":"revise","memory_edits":[{"operation":"revise",
        "memory_id":f.memory,"expected_revision":1,"change_kind":"reinterpretation",
        "record":record("No coefficient difficulty is established.","Current reinterpretation replaces the old diagnosis.",json!({"observation_id":f.observation})),
        "reason":"Clarify the limit of the original evidence.","change_basis":[{"observation_id":f.observation}]}]}),
    );
    assert!(read_error(&f, json!({"cursor":initial["next_cursor"]})).starts_with("cursor_stale:"));
    let current = read(&f, json!({}));
    for item in current["reviews"].as_array().unwrap() {
        assert_eq!(item["status"], "needs_reconsideration");
        assert_eq!(item["stored_status"], "suggested");
        assert_eq!(item["memory_revision"], 1);
        assert_eq!(item["current_memory"]["revision"], 2);
        assert_eq!(
            item["current_memory"]["summary"],
            "No coefficient difficulty is established."
        );
        assert_eq!(
            item["current_memory"]["state_text"],
            "Current reinterpretation replaces the old diagnosis."
        );
    }
    update(
        &f.e,
        json!({"request_id":"retire","memory_edits":[{"operation":"retire",
        "memory_id":f.memory,"expected_revision":2,"reason":"Use the narrower scope.",
        "replacement_ids":[f.replacement]}]}),
    );
    let retired = read(&f, json!({"memory_id":f.memory}));
    assert_eq!(retired["coverage"]["review_queue_count"], 2);
    for item in retired["reviews"].as_array().unwrap() {
        assert_eq!(item["status"], "needs_reconsideration");
        assert_eq!(item["current_memory"]["revision"], 3);
        assert_eq!(item["current_memory"]["lifecycle"], "retired");
        assert_eq!(item["current_memory"]["detail_status"], "retired");
        assert_eq!(
            item["current_memory"]["replacement_ids"],
            json!([f.replacement])
        );
        assert!(item["current_memory"]["summary"].is_null());
        assert!(item["current_memory"]["state_text"].is_null());
    }
    assert!(!retired.to_string().contains("Original current description"));
    assert!(
        !retired
            .to_string()
            .contains("Replacement current description")
    );
    let filtered = read(&f, json!({"memory_id":f.replacement}));
    assert_eq!(filtered["coverage"]["review_queue_count"], 0);
    assert_eq!(filtered["coverage"]["review_queue_complete"], true);
}

#[test]
fn byte_bounded_pages_keep_whole_suggestions_and_honest_continuations() {
    let f = fixture(7);
    let first = read(&f, json!({"limit":1}));
    let budget = serde_json::to_vec(&first).unwrap().len();
    let mut cursor = Value::Null;
    let mut seen = Vec::new();
    loop {
        let packet = read(&f, json!({"max_bytes":budget,"cursor":cursor}));
        assert!(serde_json::to_vec(&packet).unwrap().len() <= budget);
        assert_eq!(packet["coverage"]["review_queue_count"], 7);
        assert_eq!(packet["coverage"]["review_queue_complete"], false);
        let rows = packet["reviews"].as_array().unwrap();
        assert!(!rows.is_empty());
        assert_eq!(packet["page_count"], rows.len());
        for row in rows {
            seen.push(row["review_id"].as_str().unwrap().to_owned());
            assert_eq!(
                row["reason"],
                "Revisit the original example without implying inability."
            );
            assert_eq!(
                row["current_memory"]["summary"],
                "Correct on one example; transfer untested."
            );
        }
        cursor = packet["next_cursor"].clone();
        if cursor.is_null() {
            break;
        }
        assert!(seen.len() < 7);
    }
    assert_eq!(seen.len(), 7);
    assert_eq!(
        seen.iter().cloned().collect::<BTreeSet<_>>(),
        f.reviews.iter().cloned().collect()
    );
    assert!(seen.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(read_error(&f, json!({"max_bytes":1})).starts_with("budget_too_small:"));
    assert!(read_error(&f, json!({"limit":0})).starts_with("field_too_large:"));
    assert!(read_error(&f, json!({"limit":33})).starts_with("field_too_large:"));
}

#[test]
fn review_only_writes_stale_cursors_without_changing_memory_or_creating_observations() {
    let f = fixture(3);
    let before_generation = generation(&f);
    let before_observations = count(&f.e, "observations");
    let first = read(&f, json!({"limit":1}));
    update(
        &f.e,
        json!({"request_id":"dismiss","review_updates":[{"operation":"update",
        "review_id":f.reviews[0],"expected_revision":1,"status":"dismissed"}]}),
    );
    assert_eq!(generation(&f), before_generation);
    assert_eq!(count(&f.e, "observations"), before_observations);
    assert!(read_error(&f, json!({"cursor":first["next_cursor"]})).starts_with("cursor_stale:"));
    let second = read(&f, json!({"limit":1}));
    assert_ne!(first["review_generation"], second["review_generation"]);
    assert_eq!(second["coverage"]["review_queue_count"], 2);
    update(
        &f.e,
        json!({"request_id":"insert","review_updates":[{"operation":"create",
        "client_ref":"new_review","memory_id":f.memory,"purpose":"Fresh suggestion",
        "reason":"A suggestion is not evidence."}]}),
    );
    assert_eq!(generation(&f), before_generation);
    assert_eq!(count(&f.e, "observations"), before_observations);
    assert!(read_error(&f, json!({"cursor":second["next_cursor"]})).starts_with("cursor_stale:"));
    let queue = read(&f, json!({}));
    assert_eq!(queue["coverage"]["review_queue_count"], 3);
    assert!(
        queue["reviews"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["review_id"] != f.reviews[0])
    );
}

#[test]
fn recording_off_leaves_no_receipt_or_ability_changes() {
    let f = fixture(2);
    assert_eq!(count(&f.e, "context_receipts"), 0);
    tool(
        &f.e,
        "codex",
        THREAD,
        "learning_step",
        json!({"schema":"learner-model/v2",
        "action":"control","request_id":"off","control":"recording_off",
        "reported_event":{"external_event_key":"off","speaker":"learner","kind":"message","text":"Stop recording."}}),
    );
    let before = learner_state(&f);
    for _ in 0..2 {
        let packet = read(&f, json!({}));
        assert_eq!(packet["trace_status"], "disabled");
        assert!(packet["trace_id"].is_null());
        assert_eq!(packet["coverage"]["review_queue_count"], 2);
        assert!(
            packet["reviews"]
                .as_array()
                .unwrap()
                .iter()
                .all(|item| item["status"] == "suggested")
        );
    }
    assert_eq!(count(&f.e, "context_receipts"), 0);
    assert_eq!(learner_state(&f), before);
}

#[test]
fn cursor_scope_and_tampering_are_rejected_and_large_current_text_has_a_read_reference() {
    let f = fixture(2);
    let first = read(&f, json!({"limit":1}));
    let cursor = first["next_cursor"].clone();
    assert!(
        read_error(&f, json!({"cursor":cursor,"memory_id":f.replacement}))
            .starts_with("invalid_cursor:")
    );
    let mut tampered = cursor.clone();
    tampered["last"] = json!("r-another");
    assert!(read_error(&f, json!({"cursor":tampered})).starts_with("invalid_cursor:"));
    let mut unknown = cursor;
    unknown["extra"] = json!(true);
    let unknown_error = read_error(&f, json!({"cursor":unknown}));
    assert!(
        unknown_error.starts_with("invalid_variant:"),
        "{unknown_error}"
    );
    update(
        &f.e,
        json!({"request_id":"long-body","memory_edits":[{"operation":"revise",
        "memory_id":f.memory,"expected_revision":1,"change_kind":"editorial",
        "record":record("Keep this complete qualification: not inability.",&"Long current description. ".repeat(500),json!({"observation_id":f.observation})),
        "reason":"Expand explanation without new performance.","change_basis":[{"observation_id":f.observation}]}]}),
    );
    let packet = read(&f, json!({"limit":1,"max_bytes":3000}));
    let card = &packet["reviews"][0]["current_memory"];
    assert_eq!(
        card["summary"],
        "Keep this complete qualification: not inability."
    );
    assert_eq!(card["detail_status"], "read_required");
    assert!(card["state_text"].is_null());
    assert_eq!(
        card["read_ref"],
        json!({"kind":"current_memory","id":f.memory})
    );
    assert!(serde_json::to_vec(&packet).unwrap().len() <= 3000);
}
