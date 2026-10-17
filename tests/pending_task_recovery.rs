//! Local deterministic reproduction of the two rejected live 487142f calls.
//! These subprocess tests do not run a tutoring model or infer delivery.
use rusqlite::Connection;
use serde_json::{Value, json};
use support::{TestEnvironment, mcp::McpClient, v2};

const REPRO: &str = include_str!("fixtures/learner_model/pending-task-rejected-updates.json");

fn recipe(text: &str) -> Value {
    let body = text
        .split_once("<!-- executable-recipe: planned_task_after_failed_start -->\n```json\n")
        .expect("planned task recipe is served")
        .1;
    serde_json::from_str(body.split_once("\n```").unwrap().0).unwrap()
}
fn object_branch(node: &Value) -> &Value {
    if node["properties"].is_object() {
        return node;
    }
    node["anyOf"]
        .as_array()
        .unwrap()
        .iter()
        .find(|branch| branch["properties"].is_object())
        .unwrap()
}
#[test]
fn generated_schema_explains_planned_tasks_and_real_proposal_delivery_refs() {
    let environment = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&environment);
    let mut client = McpClient::spawn(&environment, true);
    client.initialize();
    let listed = client.request(2, "tools/list", json!({}));
    let schema = &listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "learning_step")
        .unwrap()["inputSchema"];
    let update = schema["anyOf"]
        .as_array()
        .unwrap()
        .iter()
        .find(|branch| branch["properties"]["checkpoint"].is_object())
        .unwrap();
    let checkpoint = object_branch(&update["properties"]["checkpoint"]);
    let task_node = &checkpoint["properties"]["pending_task"];
    let task = object_branch(task_node);
    assert!(
        task_node["description"]
            .as_str()
            .unwrap_or_default()
            .contains("next_action")
    );
    assert!(
        checkpoint["properties"]["next_action"]["description"]
            .as_str()
            .unwrap_or_default()
            .contains("planned")
    );
    assert!(
        task["properties"]["proposal_event_ref"]["description"]
            .as_str()
            .unwrap_or_default()
            .contains("captured")
    );
    assert!(
        task["properties"]["observed_delivery_refs"]["description"]
            .as_str()
            .unwrap_or_default()
            .contains("observed")
    );
    assert_eq!(
        task["required"],
        json!(["proposal_event_ref", "conditions"])
    );
    assert_eq!(task["additionalProperties"], false);
    assert_eq!(task["properties"].as_object().unwrap().len(), 5);
    client.close_cleanly();
}

struct Tools<'a> {
    environment: &'a TestEnvironment,
    client: Option<McpClient>,
    id: u64,
}
impl Tools<'_> {
    fn call(&mut self, name: &str, arguments: Value) -> Value {
        self.id += 1;
        if let Some(client) = &mut self.client {
            let response = client.request(
                self.id,
                "tools/call",
                json!({"name":name,"arguments":arguments}),
            );
            serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap()
        } else {
            let output = v2::tool_output(self.environment, "codex", "halves", name, arguments);
            serde_json::from_slice(&output.stdout).unwrap()
        }
    }
}
fn reproduce_and_repair(mcp: bool) {
    let reproduction: Value = serde_json::from_str(REPRO).unwrap();
    let original = &reproduction["failed_calls"];
    let environment = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&environment);
    let connection = Connection::open(
        environment
            .workspace
            .join(support::CONTROL_DIR)
            .join("state.sqlite"),
    )
    .unwrap();
    // Separate synthetic earlier input exercises the no-bulk-attachment boundary.
    connection.execute("INSERT INTO learning_events(event_id,host_kind,host_thread_id,external_event_key,speaker,kind,origin,text,metadata_json,captured_at_ms,processing) VALUES('unrelated-algebra','codex','earlier-thread','earlier','learner','message','host_observed','I confuse b with x.','{}',0,'pending')",[]).unwrap();
    let learner_text = original[0]["native_capture"]["text"].as_str().unwrap();
    let notice = v2::context(&v2::prompt(
        &environment,
        "codex",
        "halves",
        Some("halves-first"),
        learner_text,
    ));
    let ticket = notice
        .split_once("native_capture.ticket=")
        .unwrap()
        .1
        .split(';')
        .next()
        .unwrap();
    let mut client = mcp.then(|| {
        let mut client =
            McpClient::spawn_with_env(&environment, true, &[("CODEX_THREAD_ID", "halves")]);
        client.initialize();
        client
    });
    let mut tools = Tools {
        environment: &environment,
        client: client.take(),
        id: 10,
    };
    let mut first = original[0].clone();
    first["native_capture"]["ticket"] = json!(ticket);
    let rejected = tools.call("learning_step", first);
    assert_eq!(rejected["error"]["code"], "invalid_variant", "{rejected}");
    assert_eq!(
        rejected["error"]["recovery"]["field_path"],
        reproduction["expected_error_paths"][0]
    );
    let event = rejected["error"]["captured_event_ids"][0]
        .as_str()
        .unwrap()
        .to_owned();
    let mut second = original[1].clone();
    second["event_dispositions"][0]["event_id"] = json!(event);
    let rejected = tools.call("learning_step", second);
    assert_eq!(
        rejected["error"]["recovery"]["field_path"],
        reproduction["expected_error_paths"][1]
    );
    assert!(
        rejected["error"]["message"]
            .as_str()
            .unwrap()
            .contains("proposal_event_ref")
    );
    assert_eq!(v2::count(&environment, "learning_sessions"), 0);
    assert_eq!(v2::count(&environment, "observations"), 0);
    let pending: (String, Option<String>) = connection
        .query_row(
            "SELECT processing,session_id FROM learning_events WHERE event_id=?1",
            [&event],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(pending, ("pending".into(), None));
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM learning_events WHERE speaker='tutor'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );

    let served=tools.call("context",json!({"action":"instructions","host":"codex","stage":"interpret_and_repair","operation":"tool_recipes"}));
    let mut repaired = recipe(served["text"].as_str().unwrap());
    repaired["turn_ref"] = json!(event);
    assert_eq!(
        repaired["session"]["selected_unbound_event_ids"],
        json!(["$CURRENT_EVENT_ID"])
    );
    repaired["session"]["selected_unbound_event_ids"] = json!([event]);
    repaired["event_dispositions"][0]["event_id"] = json!(event);
    repaired["session"]["goal"] = original[0]["session"]["goal"].clone();
    for field in ["current_problem", "working_diagnosis", "next_action"] {
        repaired["checkpoint"][field] = original[0]["checkpoint"][field].clone();
    }
    let intended = &original[0]["checkpoint"]["pending_task"];
    repaired["checkpoint"]["plan_text"] = json!(format!(
        "Planned question: {} Conditions: {} Proposed help: {}",
        intended["prompt"].as_str().unwrap(),
        intended["conditions"].as_str().unwrap(),
        intended["known_help"].as_str().unwrap()
    ));
    assert_eq!(repaired["checkpoint"]["pending_task"], Value::Null);
    assert_eq!(repaired["event_dispositions"][0]["outcome"], "no_change");
    let saved = tools.call("learning_step", repaired.clone());
    assert_eq!(saved["update_status"], "committed", "{saved}");
    let session = saved["references"]["halves_session"].as_str().unwrap();
    let status = tools.call(
        "context",
        json!({"schema":"learner-model/v2","action":"status"}),
    );
    let stored = &status["session"]["checkpoint"];
    for field in [
        "current_problem",
        "working_diagnosis",
        "next_action",
        "plan_text",
    ] {
        assert_eq!(stored[field], repaired["checkpoint"][field]);
    }
    assert!(stored["pending_task"].is_null());
    let current: (String, Option<String>) = connection
        .query_row(
            "SELECT processing,session_id FROM learning_events WHERE event_id=?1",
            [&event],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(current, ("no_change".into(), Some(session.into())));
    assert_eq!(
        connection
            .query_row(
                "SELECT session_id FROM learning_events WHERE event_id='unrelated-algebra'",
                [],
                |row| row.get::<_, Option<String>>(0)
            )
            .unwrap(),
        None
    );
    assert_eq!(v2::count(&environment, "learning_events"), 2);
    assert_eq!(v2::count(&environment, "learner_memories"), 0);
    assert_eq!(v2::count(&environment, "observations"), 0);

    // An actual recorded proposal can later back a task; it still does not prove delivery.
    let proposal=tools.call("learning_step",json!({"schema":"learner-model/v2","action":"update","request_id":"record-proposal","reported_events":[{"external_event_key":"halves-proposal","speaker":"tutor","kind":"task_proposal","text":original[0]["checkpoint"]["pending_task"]["prompt"],"metadata":{"delivery":"proposed"}}]}));
    assert_eq!(proposal["update_status"], "committed", "{proposal}");
    let proposal_id = &proposal["captured_event_ids"][0];
    let mut checkpoint = stored.clone();
    checkpoint["pending_task"] = json!({"proposal_event_ref":proposal_id,"conditions":original[0]["checkpoint"]["pending_task"]["conditions"],"observed_delivery_refs":[],"help_refs":[],"reference_answer_ref":null});
    let saved=tools.call("learning_step",json!({"schema":"learner-model/v2","action":"update","request_id":"save-recorded-proposal","session":{"operation":"use","session_id":session,"expected_revision":1},"checkpoint":checkpoint}));
    assert_eq!(saved["update_status"], "committed", "{saved}");
    checkpoint["plan_text"] = json!(
        "After a response, contrast equal and unequal groups. No further proposal or observed delivery exists yet."
    );
    let saved=tools.call("learning_step",json!({"schema":"learner-model/v2","action":"update","request_id":"preserve-existing-task","session":{"operation":"use","session_id":session,"expected_revision":2},"checkpoint":checkpoint}));
    assert_eq!(saved["update_status"], "committed", "{saved}");
    let status = tools.call(
        "context",
        json!({"schema":"learner-model/v2","action":"status"}),
    );
    assert_eq!(
        status["session"]["checkpoint"]["pending_task"]["proposal_event_ref"],
        *proposal_id
    );
    assert_eq!(
        status["session"]["checkpoint"]["pending_task"]["observed_delivery_refs"],
        json!([])
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT metadata_json FROM learning_events WHERE event_id=?1",
                [proposal_id.as_str().unwrap()],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
        r#"{"delivery":"proposed"}"#
    );
    if let Some(client) = tools.client.take() {
        client.close_cleanly();
    }
}
#[test]
fn exact_rejected_halves_calls_repair_through_cli_without_erasing_task() {
    reproduce_and_repair(false);
}
#[test]
fn exact_rejected_halves_calls_repair_through_mcp_without_erasing_task() {
    reproduce_and_repair(true);
}
