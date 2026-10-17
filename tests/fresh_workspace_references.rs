//! A22 real CLI adapter proof against a disposable workspace recreated by its
//! owner. No live account, model, migration, import, or user's control directory is used.
use rusqlite::Connection;
use serde_json::{Value, json};
use support::{TestEnvironment, v2};

const HOST: &str = "claude";
const THREAD: &str = "fresh-workspace-reference-test";
const SCHEMA: &str = "learner-model/v2";

fn tool(environment: &TestEnvironment, name: &str, request: Value) -> Value {
    v2::tool(environment, HOST, THREAD, name, request)
}
fn state(environment: &TestEnvironment) -> Vec<String> {
    let conn = Connection::open(
        environment
            .workspace
            .join(support::CONTROL_DIR)
            .join("state.sqlite"),
    )
    .unwrap();
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    tables
        .into_iter()
        .map(|table| {
            let mut query = conn.prepare(&format!("SELECT * FROM \"{table}\"")).unwrap();
            let columns = query.column_count();
            let mut rows: Vec<String> = query
                .query_map([], |row| {
                    Ok((0..columns)
                        .map(|i| format!("{:?}", row.get_ref(i).unwrap()))
                        .collect::<Vec<_>>()
                        .join("|"))
                })
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            rows.sort();
            format!("{table}: {rows:?}")
        })
        .collect()
}

#[test]
fn recreated_workspace_rejects_old_memory_session_observation_event_and_receipt_refs() {
    let environment = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&environment);
    let start = v2::start(&environment, HOST, THREAD);
    let old_workspace = start["workspace_id"].clone();
    let old_session = start["references"]["session"].clone();
    let captured = tool(
        &environment,
        "learning_step",
        json!({"schema":SCHEMA,"action":"update","request_id":"capture-original","reported_events":[{"external_event_key":"original-turn","speaker":"learner","kind":"message","text":"b is a fixed coefficient, not x."}]}),
    );
    let old_event = captured["captured_event_ids"][0].clone();
    let original_request = json!({"schema":SCHEMA,"action":"update","request_id":"original-learning","turn_ref":old_event,"observations":[{"client_ref":"work","kind":"learner_work","description":"Identifies a fixed coefficient","events":[{"event_id":old_event,"role":"learner_response"}]}],"memory_edits":[{"operation":"create","client_ref":"memory","record":{"title":"Coefficient roles","scope":"Algebra","summary":"Distinguishes the coefficient from the unknown","state_text":"This example distinguishes b from x; broader transfer is not established.","current_basis":[{"new_observation":"work"}]},"reason":"Original learner work","change_basis":[{"new_observation":"work"}]}],"event_dispositions":[{"event_id":old_event,"outcome":"processed"}]});
    let original = tool(&environment, "learning_step", original_request.clone());
    let old_memory = original["references"]["memory"].clone();
    let old_observation = original["references"]["work"].clone();
    for value in [
        &old_workspace,
        &old_session,
        &old_event,
        &old_memory,
        &old_observation,
    ] {
        assert!(value.as_str().is_some_and(|id| !id.is_empty()));
    }
    assert_eq!(v2::count(&environment, "learner_memories"), 1);

    // Every CLI process has exited and no connection is retained. This exact
    // directory belongs to TestEnvironment; the test never discovers a real
    // user workspace or deletes outside its disposable root.
    let owned_control = environment.workspace.join(support::CONTROL_DIR);
    assert!(owned_control.starts_with(&environment.root));
    std::fs::remove_dir_all(&owned_control).unwrap();
    v2::init(&environment);
    let fresh_start = v2::start(&environment, HOST, THREAD);
    assert_ne!(fresh_start["workspace_id"], old_workspace);
    assert_ne!(fresh_start["references"]["session"], old_session);
    assert_eq!(v2::count(&environment, "learner_memories"), 0);
    assert_eq!(v2::count(&environment, "observations"), 0);
    assert_eq!(v2::count(&environment, "learning_events"), 0);
    let before = state(&environment);

    for (label, name, request) in [
        (
            "memory read",
            "memory",
            json!({"schema":SCHEMA,"action":"read","refs":[{"kind":"current_memory","id":old_memory}]}),
        ),
        (
            "observation read",
            "memory",
            json!({"schema":SCHEMA,"action":"read","refs":[{"kind":"observation","id":old_observation}]}),
        ),
        (
            "event read",
            "memory",
            json!({"schema":SCHEMA,"action":"read","refs":[{"kind":"event","id":old_event}]}),
        ),
        (
            "memory history",
            "memory",
            json!({"schema":SCHEMA,"action":"history","memory_id":old_memory}),
        ),
        (
            "known memory",
            "context",
            json!({"schema":SCHEMA,"action":"prepare","known_revisions":[{"memory_id":old_memory,"revision":1}]}),
        ),
        (
            "session selection",
            "context",
            json!({"schema":SCHEMA,"action":"prepare","session_id":old_session}),
        ),
        (
            "session write",
            "learning_step",
            json!({"schema":SCHEMA,"action":"update","request_id":"old-session","session":{"operation":"use","session_id":old_session,"expected_revision":1}}),
        ),
        (
            "memory write",
            "learning_step",
            json!({"schema":SCHEMA,"action":"update","request_id":"old-memory","memory_edits":[{"operation":"retire","memory_id":old_memory,"expected_revision":1,"reason":"Obsolete old reference"}]}),
        ),
        (
            "turn reference",
            "learning_step",
            json!({"schema":SCHEMA,"action":"update","request_id":"old-turn","turn_ref":old_event}),
        ),
        (
            "event disposition",
            "learning_step",
            json!({"schema":SCHEMA,"action":"update","request_id":"old-event","event_dispositions":[{"event_id":old_event,"outcome":"no_change"}]}),
        ),
        (
            "observation basis",
            "learning_step",
            json!({"schema":SCHEMA,"action":"update","request_id":"old-observation","memory_edits":[{"operation":"create","client_ref":"memory","record":{"title":"Stale support","scope":"Algebra","summary":"Must not be imported","state_text":"An old workspace reference grants no current support.","current_basis":[{"observation_id":old_observation}]},"reason":"Rejected stale reference","change_basis":[{"observation_id":old_observation}]}]}),
        ),
        ("old successful request", "learning_step", original_request),
    ] {
        let output = v2::tool_output(&environment, HOST, THREAD, name, request);
        if output.status.success() {
            let packet: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(
                packet["error"]["code"], "unknown_reference",
                "{label}: {packet}"
            );
        }
        let message = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(message.contains("unknown_reference"), "{label}: {message}");
        assert_eq!(state(&environment), before, "{label} changed fresh state");
    }
}
