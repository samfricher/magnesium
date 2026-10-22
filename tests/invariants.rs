//! Local v2 invariants replace the former numbered assessment/group graph.
use rusqlite::Connection;
use serde_json::{Value, json};
use support::{TestEnvironment, v2};

const THREAD: &str = "invariants";

fn open_state(environment: &TestEnvironment) -> Connection {
    Connection::open(
        environment
            .workspace
            .join(support::CONTROL_DIR)
            .join("state.sqlite"),
    )
    .unwrap()
}

fn row_count(environment: &TestEnvironment, table: &str) -> i64 {
    open_state(environment)
        .query_row(&format!("SELECT count(*) FROM \"{table}\""), [], |row| {
            row.get(0)
        })
        .unwrap()
}

fn update(environment: &TestEnvironment, request: Value) -> Value {
    let receipt = v2::tool(environment, "codex", THREAD, "learning_step", request);
    assert_eq!(receipt["update_status"], "committed", "{receipt}");
    receipt
}

/// One captured event, one observation, one memory and one review suggestion.
fn seed_learner_state(environment: &TestEnvironment) {
    let started = v2::start(environment, "codex", THREAD);
    let session = started["references"]["session"].as_str().unwrap();
    let captured = update(
        environment,
        json!({"schema":"learner-model/v2","action":"update","request_id":"capture",
        "session":{"operation":"use","session_id":session,"expected_revision":1},
        "reported_events":[{"external_event_key":"work","speaker":"learner","kind":"message",
            "text":"In 2x² + 7x + 3, b = 7.","occurred_at_ms":1}]}),
    );
    let event = &captured["captured_event_ids"][0];
    update(
        environment,
        json!({"schema":"learner-model/v2","action":"update","request_id":"lesson",
        "session":{"operation":"use","session_id":session,"expected_revision":2},
        "observations":[{"client_ref":"work","kind":"learner_work",
            "description":"Identified b in one example.",
            "events":[{"event_id":event,"role":"learner_response"}]}],
        "memory_edits":[{"operation":"create","client_ref":"memory",
            "record":{"title":"Coefficient roles","scope":"Algebra","summary":"Correct on one example.",
                "state_text":"Transfer untested.","current_basis":[{"new_observation":"work"}]},
            "reason":"Keep local evidence.","change_basis":[{"new_observation":"work"}]}],
        "review_updates":[{"operation":"create","client_ref":"review","memory_id":"memory",
            "purpose":"Name b in a new quadratic","reason":"Spaced retrieval.","due_at_ms":0}],
        "event_dispositions":[{"event_id":event,"outcome":"processed"}]}),
    );
}

fn state(environment: &TestEnvironment) -> Vec<String> {
    let connection = open_state(environment);
    let mut rows = connection.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE '%fts%' ORDER BY name").unwrap();
    let tables: Vec<String> = rows
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    tables
        .into_iter()
        .map(|table| {
            let mut statement = connection
                .prepare(&format!("SELECT * FROM \"{table}\" ORDER BY rowid"))
                .unwrap();
            let width = statement.column_count();
            let values: Vec<String> = statement
                .query_map([], |row| {
                    Ok((0..width)
                        .map(|index| format!("{:?}", row.get_ref(index).unwrap()))
                        .collect::<Vec<_>>()
                        .join("|"))
                })
                .unwrap()
                .map(Result::unwrap)
                .collect();
            format!("{table}: {values:?}")
        })
        .collect()
}

#[test]
fn diagnostics_and_reindex_do_not_change_canonical_learning_state() {
    let environment = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&environment);
    seed_learner_state(&environment);
    for table in [
        "learning_events",
        "observations",
        "learner_memories",
        "memory_changes",
        "review_items",
    ] {
        assert!(row_count(&environment, table) > 0, "{table} was not seeded");
    }
    let before = state(&environment);
    for arguments in [vec!["info", "--json"], vec!["memory", "reindex"]] {
        let output = environment.command().args(arguments).output().unwrap();
        assert!(output.status.success(), "{output:?}");
        assert_eq!(state(&environment), before);
    }
}
