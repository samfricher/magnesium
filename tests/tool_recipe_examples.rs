//! Execute shipped model-facing JSON without supplying driver-only shape hints.
use rusqlite::Connection;
use serde_json::{Value, json};
use support::{TestEnvironment, mcp::McpClient, v2};

fn recipe(text: &str, name: &str) -> Value {
    let marker = format!("<!-- executable-recipe: {name} -->\n```json\n");
    let body = text
        .split_once(&marker)
        .expect("recipe is actually served")
        .1;
    serde_json::from_str(body.split_once("\n```").unwrap().0).unwrap()
}
fn substitute(value: &mut Value, key: &str, replacement: &Value) {
    match value {
        Value::String(s) if s == key => *value = replacement.clone(),
        Value::Array(a) => a.iter_mut().for_each(|v| substitute(v, key, replacement)),
        Value::Object(o) => o.values_mut().for_each(|v| substitute(v, key, replacement)),
        _ => (),
    }
}
fn run_examples(mcp: bool) {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let served = v2::tool(
        &e,
        "codex",
        "monday",
        "context",
        json!({"action":"instructions","host":"codex","stage":"interpret_and_repair","operation":"tool_recipes"}),
    );
    let text = served["text"].as_str().expect("instruction text");
    assert!(text.contains("session.client_ref"));
    assert!(text.contains("native_capture"));
    let mut monday = mcp.then(|| {
        let mut c = McpClient::spawn_with_env(&e, true, &[("CODEX_THREAD_ID", "monday")]);
        c.initialize();
        c
    });
    let mut id = 10;
    let mut call = |thread: &str, request: Value, client: &mut Option<McpClient>| {
        id += 1;
        let result = if let Some(c) = client {
            c.call_value(id, "learning_step", request)
        } else {
            v2::tool(&e, "codex", thread, "learning_step", request)
        };
        assert_eq!(result["update_status"], "committed", "{result}");
        result
    };
    let first = call("monday", recipe(text, "start_capture"), &mut monday);
    let mut create = recipe(text, "observe_create");
    substitute(
        &mut create,
        "$MONDAY_EVENT_ID",
        &first["captured_event_ids"][0],
    );
    substitute(
        &mut create,
        "$MONDAY_SESSION_ID",
        &first["references"]["monday_session"],
    );
    create["session"]["expected_revision"] =
        first["revisions"][first["references"]["monday_session"].as_str().unwrap()].clone();
    let created = call("monday", create.clone(), &mut monday);
    let replayed = call("monday", create, &mut monday);
    assert_eq!(created["references"], replayed["references"]);
    if let Some(c) = monday.take() {
        c.close_cleanly();
    }
    let mut tuesday = mcp.then(|| {
        let mut c = McpClient::spawn_with_env(&e, true, &[("CODEX_THREAD_ID", "tuesday")]);
        c.initialize();
        c
    });
    let next = call(
        "tuesday",
        recipe(text, "new_conversation_capture"),
        &mut tuesday,
    );
    assert_ne!(
        first["references"]["monday_session"],
        next["references"]["tuesday_session"]
    );
    let mut revise = recipe(text, "revise_memory");
    let memory = &created["references"]["quadratic_memory"];
    for (key, value) in [
        (
            "$MONDAY_OBSERVATION_ID",
            &created["references"]["monday_work"],
        ),
        ("$MEMORY_ID", memory),
        ("$TUESDAY_EVENT_1_ID", &next["captured_event_ids"][0]),
        ("$TUESDAY_EVENT_2_ID", &next["captured_event_ids"][1]),
    ] {
        substitute(&mut revise, key, value);
    }
    revise["memory_edits"][0]["expected_revision"] =
        created["revisions"][memory.as_str().unwrap()].clone();
    substitute(
        &mut revise,
        "$TUESDAY_SESSION_ID",
        &next["references"]["tuesday_session"],
    );
    revise["session"]["expected_revision"] =
        next["revisions"][next["references"]["tuesday_session"].as_str().unwrap()].clone();
    let revised = call("tuesday", revise, &mut tuesday);
    assert_eq!(revised["revisions"][memory.as_str().unwrap()], 2);
    if let Some(c) = tuesday.take() {
        c.close_cleanly();
    }
    let db = Connection::open(e.workspace.join(support::CONTROL_DIR).join("state.sqlite")).unwrap();
    let (state, basis): (String, String) = db
        .query_row(
            "SELECT state_text,current_basis_json FROM learner_memories WHERE memory_id=?1",
            [memory.as_str().unwrap()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(state.contains("Monday") && state.contains("Tuesday"));
    assert!(state.contains("Next distinguish"));
    let basis: Value = serde_json::from_str(&basis).unwrap();
    assert_eq!(basis.as_array().unwrap().len(), 2);
    assert!(
        basis
            .as_array()
            .unwrap()
            .contains(&created["references"]["monday_work"])
    );
    assert_eq!(v2::count(&e, "learner_memories"), 1);
    assert_eq!(v2::count(&e, "observations"), 2);
    assert_eq!(v2::count(&e, "learning_sessions"), 2);
    for (session, phrase) in [
        (
            &first["references"]["monday_session"],
            "Confirm the correct work",
        ),
        (&next["references"]["tuesday_session"], "Contrast b"),
    ] {
        let (revision, checkpoint): (i64, String) = db
            .query_row(
                "SELECT revision,checkpoint_json FROM learning_sessions WHERE session_id=?1",
                [session.as_str().unwrap()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(revision, 2);
        let checkpoint: Value = serde_json::from_str(&checkpoint).unwrap();
        assert!(checkpoint["next_action"].as_str().unwrap().contains(phrase));
        assert_eq!(checkpoint["memory_refs"][0]["memory_id"], *memory);
    }
    let pending: i64 = db
        .query_row(
            "SELECT count(*) FROM learning_events WHERE processing='pending'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pending, 0);
}
#[test]
fn shipped_recipes_execute_through_cli_across_new_conversations() {
    run_examples(false);
}
#[test]
fn shipped_recipes_execute_through_mcp_across_new_conversations() {
    run_examples(true);
}

#[test]
fn checkpoint_recipes_are_served_for_every_host_and_stage_within_budget() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    for host in ["codex", "claude", "opencode"] {
        for stage in [
            "orient_and_probe",
            "plan_and_route",
            "teach_and_practice",
            "interpret_and_repair",
            "review_and_transfer",
            "consolidate_and_resume",
            "papers",
            "papers_loop",
        ] {
            let served = v2::tool(
                &e,
                host,
                "recipe-budget",
                "context",
                json!({"action":"instructions","host":host,"stage":stage,"operation":"tool_recipes"}),
            );
            assert_eq!(served["status"], "served", "{host}/{stage}: {served}");
            assert!(
                served["files"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("contracts/tool-recipes.md")),
                "{host}/{stage}: recipe was deferred"
            );
            assert!(
                served["files"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(format!("workflows/{}.md", stage.replace('_', "-")))),
                "{host}/{stage}: workflow was not served"
            );
            assert!(served["bytes_served"].as_u64().unwrap() <= 32 * 1024);
            assert!(
                served["text"]
                    .as_str()
                    .unwrap()
                    .contains("Do not drop the checkpoint")
            );
        }
    }
}
