//! A lesson moving between conversations, as in a forked Claude Code session.
use rusqlite::Connection;
use serde_json::{Value, json};
use support::{TestEnvironment, v2};

const ORIGINAL: &str = "original";
const FORK: &str = "fork";

fn db(e: &TestEnvironment) -> Connection {
    Connection::open(e.workspace.join(support::CONTROL_DIR).join("state.sqlite")).unwrap()
}

fn call(e: &TestEnvironment, thread: &str, request: Value) -> Value {
    let output = v2::tool_output(e, "claude", thread, "learning_step", request);
    serde_json::from_slice(&output.stdout).unwrap_or_else(|_| panic!("{output:?}"))
}

/// `learning_active` per conversation, keyed by the native thread name.
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
fn rejected_fork_resume_names_the_retry_and_pausing_the_fork_stops_the_original() {
    let (e, session) = lesson_started_in_original();
    let text = "wider, because 99% needs a bigger multiplier";
    let ticket = ticket(&v2::prompt(&e, "claude", FORK, Some("f-1"), text));

    // The tutor cites the input it is capturing before an ID exists.
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
    // Fresh native input is pending and unbound, so it is the one to select.
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

    // One retry exactly as the recovery describes commits.
    let retried = call(
        &e,
        FORK,
        json!({"schema":"learner-model/v2","action":"update","request_id":"fork-resume-retry","session":{"operation":"resume","session_id":session,"expected_revision":1,"selected_unbound_event_ids":[captured]},"observations":observation(&captured)}),
    );
    assert_eq!(retried["update_status"], "committed", "{retried}");
    // The fork shares the lesson; the original conversation stays active.
    assert_eq!(active(&e), [(FORK.into(), true), (ORIGINAL.into(), true)]);

    // Pausing from the fork pauses the lesson in both conversations, so
    // returning to the original records nothing into the paused lesson.
    let paused = call(
        &e,
        FORK,
        json!({"schema":"learner-model/v2","action":"update","request_id":"fork-pause","session":{"operation":"pause","session_id":session,"expected_revision":2}}),
    );
    assert_eq!(paused["update_status"], "committed", "{paused}");
    assert_eq!(active(&e), [(FORK.into(), false), (ORIGINAL.into(), false)]);
    // Pausing again from the original says the lesson is already paused.
    let again = call(
        &e,
        ORIGINAL,
        json!({"schema":"learner-model/v2","action":"update","request_id":"original-pause","session":{"operation":"pause","session_id":session,"expected_revision":3}}),
    );
    assert_eq!(again["error"]["code"], "learning_inactive", "{again}");
    assert_eq!(
        again["error"]["recovery"]["session_status"], "paused",
        "{again}"
    );
    assert!(
        again["error"]["recovery"]["instruction"]
            .as_str()
            .unwrap()
            .contains("already paused, possibly from another conversation")
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
fn served_pause_control_recipe_pauses_the_current_conversation() {
    let (e, _) = lesson_started_in_original();
    let served = v2::tool(
        &e,
        "claude",
        ORIGINAL,
        "context",
        json!({"action":"instructions","host":"claude","stage":"consolidate_and_resume"}),
    );
    let text = served["text"].as_str().unwrap();
    let body = text
        .split_once("<!-- executable-recipe: control_pause -->\n```json\n")
        .expect("pause control recipe is served")
        .1;
    let current: String = db(&e)
        .query_row(
            "SELECT event_id FROM learning_events ORDER BY event_no DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let recipe = body
        .split_once("\n```")
        .unwrap()
        .0
        .replace("$CURRENT_EVENT_ID", &current);
    let result = call(&e, ORIGINAL, serde_json::from_str(&recipe).unwrap());
    assert_eq!(result["durable"], true, "{result}");
    assert_eq!(active(&e), [(ORIGINAL.into(), false)]);
}

#[test]
fn a_control_field_on_an_update_names_the_control_action() {
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
        instruction.contains("\"action\":\"control\""),
        "{instruction}"
    );
    assert!(instruction.contains("session.pause"), "{instruction}");
}

/// Admission errors arrive as text; each must show the whole request shape
/// so a tutor repairs it in one retry instead of one missing field at a time.
#[test]
fn admission_errors_name_the_complete_control_and_session_shapes() {
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
        "\"action\":\"control\"",
        "\"turn_ref\"",
        "\"reported_event\":{\"external_event_key\"",
        "\"speaker\":\"learner\",\"kind\":\"message\"",
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

/// Controls stay conversation-scoped, matching the deny marker and admission
/// lock that protect them when their write fails; the receipt points the tutor
/// at session.pause, which stops the lesson everywhere atomically.
#[test]
fn a_pause_control_in_a_fork_names_the_lesson_wide_pause_for_the_original() {
    let (e, session) = lesson_started_in_original();
    let resumed = call(
        &e,
        FORK,
        json!({"schema":"learner-model/v2","action":"update","request_id":"fork-resume","session":{"operation":"resume","session_id":session,"expected_revision":1}}),
    );
    assert_eq!(resumed["update_status"], "committed", "{resumed}");
    let current = v2::context(&v2::prompt(&e, "claude", FORK, Some("f-1"), "pause please"))
        .split_once("current event ")
        .unwrap()
        .1
        .split('.')
        .next()
        .unwrap()
        .to_owned();
    let paused = call(
        &e,
        FORK,
        json!({"schema":"learner-model/v2","action":"control","request_id":"pause","control":"pause","turn_ref":current}),
    );
    assert_eq!(paused["durable"], true, "{paused}");
    assert_eq!(
        paused["lesson_active_in_other_conversations"], 1,
        "{paused}"
    );
    assert!(
        paused["next_step"]
            .as_str()
            .unwrap()
            .contains("session.pause")
    );
    assert_eq!(active(&e), [(FORK.into(), false), (ORIGINAL.into(), true)]);

    let lesson_paused = call(
        &e,
        FORK,
        json!({"schema":"learner-model/v2","action":"update","request_id":"lesson-pause","session":{"operation":"pause","session_id":session,"expected_revision":2}}),
    );
    assert_eq!(
        lesson_paused["update_status"], "committed",
        "{lesson_paused}"
    );
    assert_eq!(active(&e), [(FORK.into(), false), (ORIGINAL.into(), false)]);
}

/// The recovery a rejected resume returns must itself be a valid next request:
/// an already-bound learner turn_ref is cited context, not input to select.
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
    let paused = call(
        &e,
        ORIGINAL,
        json!({"schema":"learner-model/v2","action":"update","request_id":"pause","session":{"operation":"pause","session_id":session,"expected_revision":1}}),
    );
    assert_eq!(paused["update_status"], "committed", "{paused}");
    let rejected = call(
        &e,
        ORIGINAL,
        json!({"schema":"learner-model/v2","action":"update","request_id":"resume","turn_ref":bound,"session":{"operation":"resume","session_id":session,"expected_revision":2},"observations":observation("$PLACEHOLDER")}),
    );
    assert_eq!(rejected["error"]["code"], "unknown_reference", "{rejected}");
    let recovery = &rejected["error"]["recovery"];
    // What a tutor following the instruction would select on the retry.
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

/// A lesson paused from another conversation can still hold captured work
/// here; the recovery must keep the completion route, not declare it saved.
#[test]
fn work_captured_before_a_cross_conversation_pause_can_still_be_completed() {
    let (e, session) = lesson_started_in_original();
    let pending: String = db(&e)
        .query_row(
            "SELECT event_id FROM learning_events WHERE processing='pending'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    for (key, operation, revision) in [("fork-resume", "resume", 1), ("fork-pause", "pause", 2)] {
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
    assert!(instruction.contains("complete_captured"), "{instruction}");
    assert!(!instruction.contains("nothing is left"), "{instruction}");

    let completed = call(
        &e,
        ORIGINAL,
        json!({"schema":"learner-model/v2","action":"update","mode":"complete_captured","request_id":"complete","turn_ref":pending,"session":{"operation":"use","session_id":session,"expected_revision":recovery["session_revision"]},"observations":observation(&pending),"event_dispositions":[{"event_id":pending,"outcome":"processed"}]}),
    );
    assert_eq!(completed["update_status"], "committed", "{completed}");
    assert_eq!(active(&e), [(FORK.into(), false), (ORIGINAL.into(), false)]);
}
