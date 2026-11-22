use crate::support::{TestEnvironment, v2};
use rusqlite::Connection;
use serde_json::{Value, json};

const ORIGINAL: &str = "original";
const FORK: &str = "fork";

fn db(e: &TestEnvironment) -> Connection {
    Connection::open(
        e.workspace
            .join(crate::support::CONTROL_DIR)
            .join("state.sqlite"),
    )
    .unwrap()
}

fn call(e: &TestEnvironment, thread: &str, request: Value) -> Value {
    let output = v2::tool_output(e, "claude", thread, "learning_step", request);
    serde_json::from_slice(&output.stdout).unwrap_or_else(|_| panic!("{output:?}"))
}

fn active(e: &TestEnvironment) -> Vec<(String, bool)> {
    db(e)
        .prepare(
            "SELECT host_thread_id, learning_active FROM host_bindings ORDER BY host_thread_id",
        )
        .unwrap()
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get(1)?)))
        .unwrap()
        .map(|row| {
            let (thread, active) = row.unwrap();
            (thread.rsplit(':').next().unwrap().to_owned(), active)
        })
        .collect()
}

fn ticket(hook: &std::process::Output) -> String {
    v2::context(hook)
        .split_once("native_capture.ticket=")
        .expect("inactive conversation offers native proof")
        .1
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}

fn observation(event: &str) -> Value {
    json!([{"client_ref":"fork_work","kind":"learner_work","description":"Answered in the forked conversation.","events":[{"event_id":event,"role":"learner_response"}],"assistance_text":"Help is not established by this test."}])
}

fn lesson_started_in_original() -> (TestEnvironment, String) {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let started = v2::start(&e, "claude", ORIGINAL);
    let session = started["references"]["session"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        v2::context(&v2::prompt(
            &e,
            "claude",
            ORIGINAL,
            Some("o-1"),
            "first answer"
        ))
        .contains("current event e-")
    );
    (e, session)
}

#[test]
fn rejected_fork_resume_names_the_retry_and_completing_in_the_fork_stops_the_original() {
    let (e, session) = lesson_started_in_original();
    let text = "wider, because 99% needs a bigger multiplier";
    let ticket = ticket(&v2::prompt(&e, "claude", FORK, Some("f-1"), text));

    let rejected = call(
        &e,
        FORK,
        json!({"schema":"learner-model/v2","action":"update","request_id":"fork-resume","native_capture":{"ticket":ticket,"text":text},"session":{"operation":"resume","session_id":session,"expected_revision":1},"observations":observation("$NATIVE")}),
    );
    assert_eq!(rejected["error"]["code"], "unknown_reference", "{rejected}");
    let captured = rejected["error"]["captured_event_ids"][0]
        .as_str()
        .unwrap()
        .to_owned();
    let recovery = &rejected["error"]["recovery"];
    assert_eq!(
        recovery["captured_event_ids"],
        json!([captured]),
        "{rejected}"
    );
    assert_eq!(
        recovery["retry_attachment_ids"],
        json!([captured]),
        "{rejected}"
    );
    let instruction = recovery["instruction"].as_str().unwrap();
    for required in [
        "\"resume\"",
        "selected_unbound_event_ids",
        "omit native_capture",
        "session.use fails",
    ] {
        assert!(instruction.contains(required), "{required}: {instruction}");
    }

    let retried = call(
        &e,
        FORK,
        json!({"schema":"learner-model/v2","action":"update","request_id":"fork-resume-retry","session":{"operation":"resume","session_id":session,"expected_revision":1,"selected_unbound_event_ids":[captured]},"observations":observation(&captured)}),
    );
    assert_eq!(retried["update_status"], "committed", "{retried}");
    assert_eq!(active(&e), [(FORK.into(), true), (ORIGINAL.into(), true)]);

    let completed = call(
        &e,
        FORK,
        json!({"schema":"learner-model/v2","action":"update","request_id":"fork-complete","session":{"operation":"complete","session_id":session,"expected_revision":2}}),
    );
    assert_eq!(completed["update_status"], "committed", "{completed}");
    assert_eq!(active(&e), [(FORK.into(), false), (ORIGINAL.into(), false)]);
    let again = call(
        &e,
        ORIGINAL,
        json!({"schema":"learner-model/v2","action":"update","request_id":"original-complete","session":{"operation":"complete","session_id":session,"expected_revision":3}}),
    );
    assert_eq!(again["error"]["code"], "learning_inactive", "{again}");
    assert_eq!(
        again["error"]["recovery"]["session_status"], "completed",
        "{again}"
    );
    assert!(
        again["error"]["recovery"]["instruction"]
            .as_str()
            .unwrap()
            .contains("already completed, possibly from another conversation")
    );
    let events = v2::count(&e, "learning_events");
    let back = v2::prompt(&e, "claude", ORIGINAL, Some("o-2"), "back in the original");
    assert!(
        v2::context(&back).contains("native_capture.ticket="),
        "{back:?}"
    );
    assert_eq!(v2::count(&e, "learning_events"), events);
}

#[test]
fn completing_a_shared_lesson_stops_capture_in_every_conversation() {
    let (e, session) = lesson_started_in_original();
    let resumed = call(
        &e,
        FORK,
        json!({"schema":"learner-model/v2","action":"update","request_id":"fork-resume","session":{"operation":"resume","session_id":session,"expected_revision":1}}),
    );
    assert_eq!(resumed["update_status"], "committed", "{resumed}");
    let completed = call(
        &e,
        ORIGINAL,
        json!({"schema":"learner-model/v2","action":"update","request_id":"complete","session":{"operation":"complete","session_id":session,"expected_revision":2}}),
    );
    assert_eq!(completed["update_status"], "committed", "{completed}");
    assert_eq!(active(&e), [(FORK.into(), false), (ORIGINAL.into(), false)]);
    let events = v2::count(&e, "learning_events");
    assert!(
        v2::prompt(&e, "claude", FORK, Some("f-1"), "after completion")
            .status
            .success()
    );
    assert_eq!(v2::count(&e, "learning_events"), events);
}

#[test]
fn a_later_conversation_can_complete_a_lesson_it_never_joined() {
    let (e, session) = lesson_started_in_original();
    const LATER: &str = "later";
    let other = v2::start(&e, "claude", LATER);
    let other_session = other["references"]["session"].as_str().unwrap();
    let event = v2::context(&v2::prompt(
        &e,
        "claude",
        LATER,
        Some("l-1"),
        "close the old lesson",
    ))
    .split_once("current event ")
    .unwrap()
    .1
    .split('.')
    .next()
    .unwrap()
    .to_owned();

    let mixed = call(
        &e,
        LATER,
        json!({"schema":"learner-model/v2","action":"update","request_id":"mixed","session":{"operation":"complete","session_id":session,"expected_revision":1},"observations":observation(&event)}),
    );
    assert_eq!(mixed["error"]["code"], "event_scope_conflict", "{mixed}");

    let completed = call(
        &e,
        LATER,
        json!({"schema":"learner-model/v2","action":"update","request_id":"complete-old","session":{"operation":"complete","session_id":session,"expected_revision":1}}),
    );
    assert_eq!(completed["update_status"], "committed", "{completed}");
    assert_eq!(active(&e), [(LATER.into(), true), (ORIGINAL.into(), false)]);
    let statuses: Vec<(String, String)> = db(&e)
        .prepare("SELECT session_id, status FROM learning_sessions ORDER BY session_id = ?1 DESC")
        .unwrap()
        .query_map([&session], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        statuses,
        [
            (session.clone(), "completed".to_owned()),
            (other_session.to_owned(), "active".to_owned())
        ]
    );
    let reopened = call(
        &e,
        LATER,
        json!({"schema":"learner-model/v2","action":"update","request_id":"complete-completed","session":{"operation":"complete","session_id":session,"expected_revision":2}}),
    );
    assert_eq!(
        reopened["error"]["code"], "event_scope_conflict",
        "{reopened}"
    );

    let (e, session) = lesson_started_in_original();
    let completed = call(
        &e,
        "fresh",
        json!({"schema":"learner-model/v2","action":"update","request_id":"complete-from-fresh","session":{"operation":"complete","session_id":session,"expected_revision":1}}),
    );
    assert_eq!(completed["update_status"], "committed", "{completed}");
    assert_eq!(active(&e), [(ORIGINAL.into(), false)]);
}

#[test]
fn served_end_sitting_recipe_keeps_the_lesson_open_in_the_current_conversation() {
    let (e, _) = lesson_started_in_original();
    let served = v2::tool(
        &e,
        "claude",
        ORIGINAL,
        "context",
        json!({"action":"instructions","host":"claude","stage":"consolidate_and_resume"}),
    );
    let text = served["text"].as_str().unwrap();
    let recipe = text
        .split_once("<!-- executable-recipe: end_sitting -->\n```json\n")
        .expect("end_sitting recipe is served")
        .1
        .split_once("\n```")
        .unwrap()
        .0;
    let result = call(&e, ORIGINAL, serde_json::from_str(recipe).unwrap());
    assert_eq!(result["action"], "end_sitting", "{result}");
    assert_eq!(active(&e), [(ORIGINAL.into(), true)]);
}

#[test]
fn a_control_field_on_an_update_names_end_sitting() {
    let (e, _) = lesson_started_in_original();
    let rejected = call(
        &e,
        ORIGINAL,
        json!({"schema":"learner-model/v2","action":"update","request_id":"pause","control":"pause"}),
    );
    assert_eq!(rejected["error"]["code"], "invalid_variant", "{rejected}");
    let recovery = &rejected["error"]["recovery"];
    assert_eq!(recovery["field_path"], "control");
    assert_eq!(recovery["arguments"]["stage"], "consolidate_and_resume");
    let instruction = recovery["instruction"].as_str().unwrap();
    assert!(
        instruction.contains("\"action\":\"end_sitting\""),
        "{instruction}"
    );
    assert!(!instruction.contains("session.pause"), "{instruction}");
}

#[test]
fn admission_errors_name_end_sitting_for_a_control_and_the_session_shapes() {
    let (e, _) = lesson_started_in_original();
    let stderr = |request: Value| {
        let output = v2::tool_output(&e, "claude", ORIGINAL, "learning_step", request);
        assert!(!output.status.success(), "{output:?}");
        String::from_utf8(output.stderr).unwrap()
    };
    let control = stderr(
        json!({"schema":"learner-model/v2","action":"control","request_id":"p","control":"pause"}),
    );
    for required in [
        "control_removed",
        "\"action\":\"end_sitting\"",
        "session.complete",
    ] {
        assert!(control.contains(required), "{required}: {control}");
    }
    let start = stderr(
        json!({"schema":"learner-model/v2","action":"update","request_id":"s","session":{"op":"start"},"native_capture":{"ticket":"00","text":"x"}}),
    );
    assert!(
        start.contains("\"session\":{\"operation\":\"start\",\"client_ref\""),
        "{start}"
    );
}

#[test]
fn rejected_resume_with_a_bound_turn_ref_prescribes_a_retry_that_commits() {
    let (e, session) = lesson_started_in_original();
    let bound: String = db(&e)
        .query_row(
            "SELECT event_id FROM learning_events WHERE session_id IS NOT NULL AND speaker='learner'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let completed = call(
        &e,
        ORIGINAL,
        json!({"schema":"learner-model/v2","action":"update","request_id":"complete","session":{"operation":"complete","session_id":session,"expected_revision":1}}),
    );
    assert_eq!(completed["update_status"], "committed", "{completed}");
    let rejected = call(
        &e,
        ORIGINAL,
        json!({"schema":"learner-model/v2","action":"update","request_id":"resume","turn_ref":bound,"session":{"operation":"resume","session_id":session,"expected_revision":2},"observations":observation("$PLACEHOLDER")}),
    );
    assert_eq!(rejected["error"]["code"], "unknown_reference", "{rejected}");
    let recovery = &rejected["error"]["recovery"];
    let selected = recovery
        .get("retry_attachment_ids")
        .unwrap_or(&recovery["captured_event_ids"])
        .clone();
    let retried = call(
        &e,
        ORIGINAL,
        json!({"schema":"learner-model/v2","action":"update","request_id":"resume-retry","turn_ref":bound,"session":{"operation":"resume","session_id":session,"expected_revision":2,"selected_unbound_event_ids":selected},"observations":observation(&bound)}),
    );
    assert_eq!(retried["update_status"], "committed", "{retried}");
    assert_eq!(v2::count(&e, "learning_events"), 1);
}

#[test]
fn work_captured_before_a_cross_conversation_completion_is_named_and_finished_after_resume() {
    let (e, session) = lesson_started_in_original();
    let pending: String = db(&e)
        .query_row(
            "SELECT event_id FROM learning_events WHERE processing='pending'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    for (key, operation, revision) in [
        ("fork-resume", "resume", 1),
        ("fork-complete", "complete", 2),
    ] {
        let result = call(
            &e,
            FORK,
            json!({"schema":"learner-model/v2","action":"update","request_id":key,"session":{"operation":operation,"session_id":session,"expected_revision":revision}}),
        );
        assert_eq!(result["update_status"], "committed", "{result}");
    }
    let refused = call(
        &e,
        ORIGINAL,
        json!({"schema":"learner-model/v2","action":"update","request_id":"interpret","session":{"operation":"use","session_id":session,"expected_revision":3},"observations":observation(&pending),"event_dispositions":[{"event_id":pending,"outcome":"processed"}]}),
    );
    assert_eq!(refused["error"]["code"], "learning_inactive", "{refused}");
    let recovery = &refused["error"]["recovery"];
    assert_eq!(recovery["pending_learner_events"], 1, "{refused}");
    let instruction = recovery["instruction"].as_str().unwrap();
    assert!(instruction.contains("completed"), "{instruction}");
    assert!(
        instruction.contains("only in an open lesson"),
        "{instruction}"
    );

    let finished = call(
        &e,
        ORIGINAL,
        json!({"schema":"learner-model/v2","action":"update","request_id":"resume-and-interpret","turn_ref":pending,"session":{"operation":"resume","session_id":session,"expected_revision":recovery["session_revision"]},"observations":observation(&pending),"event_dispositions":[{"event_id":pending,"outcome":"processed"}]}),
    );
    assert_eq!(finished["update_status"], "committed", "{finished}");
    assert_eq!(active(&e), [(FORK.into(), false), (ORIGINAL.into(), true)]);
}
