//! Execute shipped model-facing JSON without supplying driver-only shape hints.
use crate::support::{TestEnvironment, mcp::McpClient, v2};
use rusqlite::Connection;
use serde_json::{Value, json};

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
        "claude",
        "monday",
        "context",
        json!({"action":"instructions","host":"claude","stage":"interpret_and_repair","operation":"tool_recipes"}),
    );
    let text = served["text"].as_str().expect("instruction text");
    assert!(text.contains("session.client_ref"));
    assert!(text.contains("native_capture"));
    assert!(text.contains("Do not drop the checkpoint"));
    let mut monday = mcp.then(|| {
        let mut c = McpClient::spawn_with_env(&e, true, &[("CLAUDE_CODE_SESSION_ID", "monday")]);
        c.initialize();
        c
    });
    let mut id = 10;
    let mut call = |thread: &str, request: Value, client: &mut Option<McpClient>| {
        id += 1;
        let result = if let Some(c) = client {
            c.call_value(id, "learning_step", request)
        } else {
            v2::tool(&e, "claude", thread, "learning_step", request)
        };
        assert_eq!(result["update_status"], "committed", "{result}");
        result
    };
    let first = call("monday", recipe(text, "start_capture"), &mut monday);
    let mut answer = recipe(text, "observe_answer");
    substitute(
        &mut answer,
        "$MONDAY_EVENT_ID",
        &first["captured_event_ids"][0],
    );
    let session = first["references"]["monday_session"].clone();
    substitute(&mut answer, "$MONDAY_SESSION_ID", &session);
    answer["session"]["expected_revision"] = first["revisions"][session.as_str().unwrap()].clone();
    let answered = call("monday", answer.clone(), &mut monday);
    let replayed = call("monday", answer, &mut monday);
    assert_eq!(answered["references"], replayed["references"]);
    let mut describe = recipe(text, "describe_and_checkpoint");
    substitute(
        &mut describe,
        "$MONDAY_OBSERVATION_ID",
        &answered["references"]["monday_work"],
    );
    substitute(&mut describe, "$MONDAY_SESSION_ID", &session);
    describe["session"]["expected_revision"] =
        answered["revisions"][session.as_str().unwrap()].clone();
    let mut created = call("monday", describe, &mut monday);
    created["references"]["monday_work"] = answered["references"]["monday_work"].clone();
    if let Some(c) = monday.take() {
        c.close_cleanly();
    }
    let mut tuesday = mcp.then(|| {
        let mut c = McpClient::spawn_with_env(&e, true, &[("CLAUDE_CODE_SESSION_ID", "tuesday")]);
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
    let db = Connection::open(
        e.workspace
            .join(crate::support::CONTROL_DIR)
            .join("state.sqlite"),
    )
    .unwrap();
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
        assert!(revision >= 2, "{revision}");
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

/// A tutor that cites, as a new observation, the one it named in its
/// previous request gets it, and is not refused and made to retry.
#[test]
fn a_memory_edit_may_cite_the_observation_its_previous_request_named() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let text = v2::tool(
        &e,
        "claude",
        "monday",
        "context",
        json!({"action":"instructions","host":"claude","stage":"interpret_and_repair","operation":"tool_recipes"}),
    )["text"]
        .as_str()
        .unwrap()
        .to_owned();
    let call = |request: Value| v2::tool(&e, "claude", "monday", "learning_step", request);
    let first = call(recipe(&text, "start_capture"));
    let session = first["references"]["monday_session"].clone();
    let mut answer = recipe(&text, "observe_answer");
    substitute(
        &mut answer,
        "$MONDAY_EVENT_ID",
        &first["captured_event_ids"][0],
    );
    substitute(&mut answer, "$MONDAY_SESSION_ID", &session);
    answer["session"]["expected_revision"] = first["revisions"][session.as_str().unwrap()].clone();
    let answered = call(answer);
    assert_eq!(answered["update_status"], "committed", "{answered}");

    let mut describe = recipe(&text, "describe_and_checkpoint");
    let by_name = json!({"new_observation": "monday_work"});
    describe["memory_edits"][0]["record"]["current_basis"][0] = by_name.clone();
    describe["memory_edits"][0]["change_basis"][0] = by_name;
    substitute(&mut describe, "$MONDAY_SESSION_ID", &session);
    describe["session"]["expected_revision"] =
        answered["revisions"][session.as_str().unwrap()].clone();
    let described = call(describe);
    assert_eq!(described["update_status"], "committed", "{described}");

    // A name no earlier request gave is still refused.
    let mut unknown = recipe(&text, "describe_and_checkpoint");
    unknown["request_id"] = json!("monday-unknown");
    unknown["memory_edits"][0]["client_ref"] = json!("other_memory");
    unknown["memory_edits"][0]["record"]["current_basis"][0] =
        json!({"new_observation": "never_named"});
    unknown["memory_edits"][0]["change_basis"][0] = json!({"new_observation": "never_named"});
    unknown.as_object_mut().unwrap().remove("checkpoint");
    unknown.as_object_mut().unwrap().remove("session");
    let refused = v2::tool_output(&e, "claude", "monday", "learning_step", unknown);
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&refused.stdout),
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(said.contains("unknown_reference"), "{said}");
}

/// A lesson start carries what context.prepare returns for its goal, so a
/// new conversation has the learner's record without a call of its own.
#[test]
fn a_session_start_carries_the_memory_on_its_goal() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let text = v2::tool(
        &e,
        "claude",
        "monday",
        "context",
        json!({"action":"instructions","host":"claude","stage":"interpret_and_repair","operation":"tool_recipes"}),
    )["text"]
        .as_str()
        .unwrap()
        .to_owned();
    let call =
        |thread: &str, request: Value| v2::tool(&e, "claude", thread, "learning_step", request);
    let first = call("monday", recipe(&text, "start_capture"));
    assert!(
        first["context"].is_object(),
        "an empty record still has its context: {first}"
    );
    let session = first["references"]["monday_session"].clone();
    let mut answer = recipe(&text, "observe_answer");
    substitute(
        &mut answer,
        "$MONDAY_EVENT_ID",
        &first["captured_event_ids"][0],
    );
    substitute(&mut answer, "$MONDAY_SESSION_ID", &session);
    answer["session"]["expected_revision"] = first["revisions"][session.as_str().unwrap()].clone();
    let answered = call("monday", answer);
    let mut describe = recipe(&text, "describe_and_checkpoint");
    substitute(
        &mut describe,
        "$MONDAY_OBSERVATION_ID",
        &answered["references"]["monday_work"],
    );
    substitute(&mut describe, "$MONDAY_SESSION_ID", &session);
    describe["session"]["expected_revision"] =
        answered["revisions"][session.as_str().unwrap()].clone();
    let described = call("monday", describe);
    let memory = described["references"]["quadratic_memory"].clone();

    // Another conversation starts a lesson on the same goal.
    let started = call(
        "tuesday",
        json!({"schema":"learner-model/v2","action":"update","request_id":"tuesday-start",
            "session":{"operation":"start","client_ref":"s","goal":"Coefficients"}}),
    );
    assert_eq!(started["update_status"], "committed", "{started}");
    let memories = started["context"]["memories"].to_string();
    assert!(memories.contains(memory.as_str().unwrap()), "{started}");
    // An update that starts no session carries no context.
    assert!(described.get("context").is_none(), "{described}");
}
