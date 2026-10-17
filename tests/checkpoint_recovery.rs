//! The error path preserves intended lesson state rather than encouraging its removal.
use rusqlite::Connection;
use serde_json::{Value, json};
use support::{TestEnvironment, v2};

fn call(e: &TestEnvironment, request: Value) -> Value {
    let output = v2::tool_output(e, "codex", "checkpoint", "learning_step", request);
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn missing_checkpoint_guard_retains_raw_and_supplies_executable_recovery() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let started = v2::start(&e, "codex", "checkpoint");
    let session = started["references"]["session"].as_str().unwrap();
    v2::prompt(
        &e,
        "codex",
        "checkpoint",
        Some("turn-1"),
        "I used b=-5 and got roots 2 and 3.",
    );
    let db = Connection::open(e.workspace.join(support::CONTROL_DIR).join("state.sqlite")).unwrap();
    let event: String = db
        .query_row(
            "SELECT event_id FROM learning_events ORDER BY event_no DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let checkpoint = json!({"current_problem":"Check signed coefficients","working_diagnosis":"Correct result on one example","next_action":"Ask what b denotes in a different equation"});
    let mut request = json!({"schema":"learner-model/v2","action":"update","request_id":"missing-guard","turn_ref":event,"observations":[{"client_ref":"work","kind":"learner_work","description":"Correctly used the negative signed coefficient in one example","events":[{"event_id":event,"role":"learner_response"}]}],"memory_edits":[{"operation":"create","client_ref":"memory","record":{"title":"Signed coefficient use","scope":"quadratics","summary":"One correct use of b=-5","state_text":"The learner used b=-5 correctly on one example. Transfer is not yet established.","current_basis":[{"new_observation":"work"}]},"reason":"Preserve concrete work with its limit","change_basis":[{"new_observation":"work"}]}],"event_dispositions":[{"event_id":event,"outcome":"processed"}],"checkpoint":checkpoint});
    let rejected = call(&e, request.clone());
    assert_eq!(rejected["error"]["code"], "session_guard_required");
    assert_eq!(rejected["error"]["captured_event_ids"], json!([event]));
    assert_eq!(v2::count(&e, "observations"), 0);
    assert_eq!(v2::count(&e, "learner_memories"), 0);
    assert_eq!(
        db.query_row(
            "SELECT processing FROM learning_events WHERE event_id=?1",
            [&event],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "pending"
    );
    let recovery = &rejected["error"]["recovery"];
    assert_eq!(recovery["suggested_session"]["session_id"], session);
    assert_eq!(recovery["suggested_session"]["expected_revision"], 1);
    let status = v2::tool(
        &e,
        "codex",
        "checkpoint",
        "context",
        recovery["next_call"]["arguments"].clone(),
    );
    assert_eq!(status["session"]["session_id"], session);
    request["session"] = recovery["suggested_session"].clone();
    request["request_id"] = json!("guard-repaired");
    let saved = call(&e, request);
    assert_eq!(saved["update_status"], "committed", "{saved}");
    assert_eq!(v2::count(&e, "observations"), 1);
    assert_eq!(v2::count(&e, "learner_memories"), 1);
    let current = v2::tool(
        &e,
        "codex",
        "checkpoint",
        "context",
        json!({"schema":"learner-model/v2","action":"status"}),
    );
    assert_eq!(
        current["session"]["checkpoint"]["next_action"],
        checkpoint["next_action"]
    );
    assert_eq!(current["session"]["revision"], 2);
}

#[test]
fn stale_checkpoint_guard_exposes_a_valid_session_read_without_overwrite() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let started = v2::start(&e, "codex", "checkpoint");
    let session = &started["references"]["session"];
    let mut request = json!({"schema":"learner-model/v2","action":"update","request_id":"checkpoint-first","session":{"operation":"use","session_id":session,"expected_revision":1},"checkpoint":{"current_problem":"First problem","working_diagnosis":"Uncertain","next_action":"First useful next step"}});
    assert_eq!(call(&e, request.clone())["update_status"], "committed");
    request["request_id"] = json!("checkpoint-stale");
    request["checkpoint"]["next_action"] = json!("New intended next step");
    let rejected = call(&e, request.clone());
    assert_eq!(rejected["error"]["code"], "revision_conflict");
    let recovery = &rejected["error"]["recovery"];
    assert_eq!(recovery["next_call"]["tool"], "context");
    let current = v2::tool(
        &e,
        "codex",
        "checkpoint",
        "context",
        recovery["next_call"]["arguments"].clone(),
    );
    assert_eq!(
        current["session"]["checkpoint"]["next_action"],
        "First useful next step"
    );
    request["session"]["expected_revision"] = current["session"]["revision"].clone();
    request["request_id"] = json!("checkpoint-reconciled");
    assert_eq!(call(&e, request)["update_status"], "committed");
    let current = v2::tool(
        &e,
        "codex",
        "checkpoint",
        "context",
        json!({"schema":"learner-model/v2","action":"status"}),
    );
    assert_eq!(
        current["session"]["checkpoint"]["next_action"],
        "New intended next step"
    );
}

#[test]
fn missing_guard_does_not_suggest_using_a_session_paused_by_another_host() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let started = v2::start(&e, "codex", "checkpoint");
    let session = &started["references"]["session"];
    v2::tool(
        &e,
        "codex",
        "other-host",
        "learning_step",
        json!({"schema":"learner-model/v2","action":"update","request_id":"other-resume","session":{"operation":"resume","session_id":session,"expected_revision":1}}),
    );
    v2::tool(
        &e,
        "codex",
        "other-host",
        "learning_step",
        json!({"schema":"learner-model/v2","action":"update","request_id":"other-pause","session":{"operation":"pause","session_id":session,"expected_revision":2}}),
    );
    // Pausing the shared lesson stops capture in this conversation too, so the
    // update is refused before the guard and nothing suggests the paused session.
    let rejected = call(
        &e,
        json!({"schema":"learner-model/v2","action":"update","request_id":"checkpoint-after-other-pause","checkpoint":{"current_problem":"Problem","working_diagnosis":"Uncertain","next_action":"Read current session state"}}),
    );
    assert_eq!(rejected["error"]["code"], "learning_inactive", "{rejected}");
    assert!(rejected["error"]["recovery"]["suggested_session"].is_null());
    assert!(
        rejected["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Start or resume explicitly")
    );
}

#[test]
fn memory_and_terminal_review_conflicts_return_executable_reads_and_current_review() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    v2::start(&e, "codex", "checkpoint");
    let raw = call(
        &e,
        json!({"schema":"learner-model/v2","action":"update","request_id":"raw-support","reported_events":[{"external_event_key":"support","speaker":"learner","kind":"message","text":"I still confuse b with the unknown x."}]}),
    );
    let event = &raw["captured_event_ids"][0];
    let created = call(
        &e,
        json!({"schema":"learner-model/v2","action":"update","request_id":"memory-review","observations":[{"client_ref":"o","kind":"learner_work","description":"Names b as the unknown.","events":[{"event_id":event,"role":"learner_response"}]}],"memory_edits":[{"operation":"create","client_ref":"m","record":{"title":"Coefficient roles","scope":"Algebra","summary":"Reported confusion","state_text":"The learner reports confusion; scope remains uncertain.","current_basis":[{"new_observation":"o"}]},"reason":"Remember reported work","change_basis":[{"new_observation":"o"}]}],"review_updates":[{"operation":"create","client_ref":"r","memory_id":"m","purpose":"Distinguish b and x","reason":"Revisit uncertainty"}]}),
    );
    assert_eq!(created["update_status"], "committed", "{created}");
    let memory = &created["references"]["m"];
    let review = &created["references"]["r"];
    let dismissed = call(
        &e,
        json!({"schema":"learner-model/v2","action":"update","request_id":"dismiss-review","review_updates":[{"operation":"update","review_id":review,"expected_revision":1,"status":"dismissed"}]}),
    );
    assert_eq!(dismissed["update_status"], "committed");
    let conflicts = [
        json!({"schema":"learner-model/v2","action":"update","request_id":"stale-memory","memory_edits":[{"operation":"flag","memory_id":memory,"expected_revision":0,"review_note":"Reconcile","reason":"Review uncertainty","change_basis":[{"event_id":event}]}]}),
        json!({"schema":"learner-model/v2","action":"update","request_id":"stale-review","review_updates":[{"operation":"update","review_id":review,"expected_revision":1,"status":"suggested"}]}),
    ];
    for (index, request) in conflicts.into_iter().enumerate() {
        let result = call(&e, request);
        assert_eq!(result["error"]["code"], "revision_conflict", "{result}");
        let recovery = &result["error"]["recovery"];
        let next = &recovery["next_call"];
        let current = v2::tool(
            &e,
            "codex",
            "checkpoint",
            next["tool"].as_str().unwrap(),
            next["arguments"].clone(),
        );
        assert!(current.get("error").is_none(), "{current}");
        if index == 1 {
            assert_eq!(recovery["current_review"]["review_id"], *review);
            assert_eq!(recovery["current_review"]["status"], "dismissed");
            assert_eq!(recovery["current_review"]["revision"], 2);
        }
    }
}
