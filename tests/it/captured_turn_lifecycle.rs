use crate::support::{TestEnvironment, v2};
use rusqlite::Connection;
use serde_json::{Value, json};

const THREAD: &str = "closing-work";
const ANSWER: &str = "b=-5 gives roots 2 and 3; substitution checks both. Let's stop here.";

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

fn captured_answer() -> (TestEnvironment, String, String) {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let started = v2::start(&e, "claude", THREAD);
    let session = started["references"]["session"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        v2::prompt(&e, "claude", THREAD, Some("closing-answer"), ANSWER)
            .status
            .success()
    );
    let event = db(&e)
        .query_row(
            "SELECT event_id FROM learning_events WHERE text=?1",
            [ANSWER],
            |r| r.get(0),
        )
        .unwrap();
    (e, session, event)
}

fn complete(e: &TestEnvironment, session: &str) {
    let result = call(
        e,
        THREAD,
        json!({"schema":"learner-model/v2","action":"update","request_id":"complete-lesson","session":{"operation":"complete","session_id":session,"expected_revision":1}}),
    );
    assert_eq!(result["update_status"], "committed", "{result}");
}

fn closing_update(session: &str, event: &str) -> Value {
    json!({"schema":"learner-model/v2","action":"update","request_id":"complete-closing-answer","turn_ref":event,
      "session":{"operation":"use","session_id":session,"expected_revision":1},
      "observations":[{"client_ref":"answer","kind":"learner_work","description":"Used b=-5, obtained roots 2 and 3, and checked both by substitution.","events":[{"event_id":event,"role":"learner_response"}],"assistance_text":"Prior help is not established by this test."}],
      "memory_edits":[{"operation":"create","client_ref":"memory","record":{"title":"Signed coefficient substitution","scope":"Quadratic equations","summary":"One correct solution and check.","state_text":"Used the signed coefficient correctly and checked both roots on this equation. Transfer is not established.","current_basis":[{"new_observation":"answer"}]},"reason":"Preserve the closing work.","change_basis":[{"new_observation":"answer"}]}],
      "checkpoint":{"current_problem":"Closing solution","working_diagnosis":"Both roots checked on this example","next_action":"Learning remains stopped; await an explicit start or resume","memory_refs":[{"memory_id":"memory","revision":"after_edit"}],"pending_task":null},
      "event_dispositions":[{"event_id":event,"outcome":"processed"}]})
}

fn processing(e: &TestEnvironment, event: &str) -> String {
    db(e)
        .query_row(
            "SELECT processing FROM learning_events WHERE event_id=?1",
            [event],
            |r| r.get(0),
        )
        .unwrap()
}

fn lesson_active(e: &TestEnvironment) -> bool {
    db(e).query_row("SELECT learning_active FROM host_bindings WHERE host_kind='claude' AND host_thread_id=?1", [format!("magnesium-h1:claude:{THREAD}")], |r| r.get(0)).unwrap()
}

fn failed_first_update(e: &TestEnvironment, thread: &str, key: &str) -> String {
    let failure = call(
        e,
        thread,
        json!({"schema":"learner-model/v2","action":"update","request_id":format!("failed-{key}"),"session":{"operation":"start","client_ref":"s","goal":"Earlier unfinished goal"},"reported_events":[{"external_event_key":key,"speaker":"learner","kind":"message","text":format!("Earlier captured learner answer {key}")}],"observations":[{"client_ref":"invalid","kind":"learner_work","description":"Bad semantic reference","events":[{"event_id":"does-not-exist","role":"learner_response"}]}]}),
    );
    assert!(failure.get("error").is_some(), "{failure}");
    let id = failure["error"]["captured_event_ids"][0]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(processing(e, &id), "pending");
    id
}

fn event_generation(e: &TestEnvironment) -> i64 {
    db(e)
        .query_row("SELECT event_generation FROM workspace_meta", [], |r| {
            r.get(0)
        })
        .unwrap()
}

#[test]
fn failed_native_start_explicit_selection_attaches_input_and_generations_change_only_on_mutation() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    assert_eq!(event_generation(&e), 0);
    let hook = v2::prompt(&e, "claude", THREAD, Some("initial-native"), ANSWER);
    let notice = v2::context(&hook);
    let ticket = notice
        .split_once("native_capture.ticket=")
        .unwrap()
        .1
        .split(';')
        .next()
        .unwrap();
    let request = json!({"schema":"learner-model/v2","action":"update","request_id":"invalid-native-first","session":{"operation":"start","client_ref":"s","goal":"Signed coefficients"},"native_capture":{"ticket":ticket,"text":ANSWER},"observations":[{"client_ref":"bad","kind":"learner_work","description":"Unresolved observation support","events":[{"event_id":"not-recorded","role":"learner_response"}]}]});
    let failure = call(&e, THREAD, request.clone());
    assert!(failure.get("error").is_some(), "{failure}");
    let event = failure["error"]["captured_event_ids"][0].as_str().unwrap();
    assert_eq!(event_generation(&e), 1);
    let repeated_failure = call(&e, THREAD, request);
    assert_eq!(repeated_failure["error"]["captured_event_ids"][0], event);
    assert_eq!(
        event_generation(&e),
        1,
        "Same raw external identity cannot revise event enumeration"
    );
    let selected = json!({"schema":"learner-model/v2","action":"update","request_id":"explicit-native-turn-start","turn_ref":event,"session":{"operation":"start","client_ref":"s","goal":"Signed coefficients","selected_unbound_event_ids":[event]}});
    let result = call(&e, THREAD, selected.clone());
    assert_eq!(result["update_status"], "committed", "{result}");
    let session = result["references"]["s"].as_str().unwrap();
    let attached: Option<String> = db(&e)
        .query_row(
            "SELECT session_id FROM learning_events WHERE event_id=?1",
            [event],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(attached.as_deref(), Some(session));
    assert_eq!(
        event_generation(&e),
        2,
        "Association change revises raw event enumeration once"
    );
    assert_eq!(call(&e, THREAD, selected), result);
    assert_eq!(event_generation(&e), 2);
    let request = closing_update(session, event);
    let saved = call(&e, THREAD, request.clone());
    assert_eq!(saved["update_status"], "committed", "{saved}");
    assert_eq!(
        event_generation(&e),
        3,
        "Disposition changes the raw event row once"
    );
    assert_eq!(call(&e, THREAD, request), saved);
    assert_eq!(event_generation(&e), 3);
}

#[test]
fn explicit_turn_ref_can_support_a_new_lesson_without_reassigning_bound_work() {
    let (e, original, event) = captured_answer();
    let result = call(
        &e,
        THREAD,
        json!({"schema":"learner-model/v2","action":"update","request_id":"wrong-topic-turn-ref","turn_ref":event,"session":{"operation":"start","client_ref":"s","goal":"An unrelated goal"}}),
    );
    assert_eq!(result["update_status"], "committed", "{result}");
    let attached: Option<String> = db(&e)
        .query_row(
            "SELECT session_id FROM learning_events WHERE event_id=?1",
            [&event],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(attached.as_deref(), Some(original.as_str()));
    assert_eq!(v2::count(&e, "learning_sessions"), 2);
}

#[test]
fn unrelated_start_attaches_only_current_capture_and_explicit_selected_unbound_work() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let earlier = failed_first_update(&e, THREAD, "old");
    let selected = failed_first_update(&e, "another-thread", "selected");
    let result = call(
        &e,
        THREAD,
        json!({"schema":"learner-model/v2","action":"update","request_id":"unrelated-start","session":{"operation":"start","client_ref":"s","goal":"A different goal","selected_unbound_event_ids":[selected]},"reported_events":[{"external_event_key":"current","speaker":"learner","kind":"message","text":"Current requested new goal"}]}),
    );
    assert_eq!(result["update_status"], "committed", "{result}");
    let session = result["references"]["s"].as_str().unwrap();
    let c = db(&e);
    let association = |id: &str| {
        c.query_row(
            "SELECT session_id FROM learning_events WHERE event_id=?1",
            [id],
            |r| r.get::<_, Option<String>>(0),
        )
        .unwrap()
    };
    assert_eq!(association(&earlier), None);
    assert_eq!(association(&selected).as_deref(), Some(session));
    assert_eq!(
        association(result["captured_event_ids"][0].as_str().unwrap()).as_deref(),
        Some(session)
    );
    let owner: String = c
        .query_row(
            "SELECT host_thread_id FROM learning_events WHERE event_id=?1",
            [&selected],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(owner, "magnesium-h1:claude:another-thread");
}

#[test]
fn resume_does_not_import_a_threads_unselected_failed_start_backlog() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let earlier = failed_first_update(&e, THREAD, "before-resume");
    let started = v2::start(&e, "claude", "session-owner");
    let session = started["references"]["session"].as_str().unwrap();
    let result = call(
        &e,
        THREAD,
        json!({"schema":"learner-model/v2","action":"update","request_id":"resume-selected-session","session":{"operation":"resume","session_id":session,"expected_revision":1},"reported_events":[{"external_event_key":"current-resume","speaker":"learner","kind":"message","text":"Resume this existing lesson"}]}),
    );
    assert_eq!(result["update_status"], "committed", "{result}");
    let association: Option<String> = db(&e)
        .query_row(
            "SELECT session_id FROM learning_events WHERE event_id=?1",
            [&earlier],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(association, None);
    let current: Option<String> = db(&e)
        .query_row(
            "SELECT session_id FROM learning_events WHERE event_id=?1",
            [result["captured_event_ids"][0].as_str().unwrap()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(current.as_deref(), Some(session));
}

#[test]
fn selected_unbound_attachment_cannot_steal_bound_work_or_duplicate_selection() {
    for invalid in ["bound", "duplicate"] {
        let e = TestEnvironment::from_fixture("basic").unwrap();
        v2::init(&e);
        let selected = if invalid == "bound" {
            let result = call(
                &e,
                "original-owner",
                json!({"schema":"learner-model/v2","action":"update","request_id":"bound-start","session":{"operation":"start","client_ref":"s","goal":"Original lesson"},"reported_events":[{"external_event_key":"bound-work","speaker":"learner","kind":"message","text":"This answer already belongs to the original lesson"}]}),
            );
            result["captured_event_ids"][0].as_str().unwrap().to_owned()
        } else {
            failed_first_update(&e, "original-owner", "duplicate")
        };
        let before: Option<String> = db(&e)
            .query_row(
                "SELECT session_id FROM learning_events WHERE event_id=?1",
                [&selected],
                |r| r.get(0),
            )
            .unwrap();
        let mut ids = vec![selected.clone()];
        if invalid == "duplicate" {
            ids.push(selected.clone());
        }
        let sessions = v2::count(&e, "learning_sessions");
        let result = call(
            &e,
            THREAD,
            json!({"schema":"learner-model/v2","action":"update","request_id":"invalid-selection","session":{"operation":"start","client_ref":"s","goal":"Different lesson","selected_unbound_event_ids":ids}}),
        );
        assert!(result.get("error").is_some(), "{invalid}: {result}");
        let after: Option<String> = db(&e)
            .query_row(
                "SELECT session_id FROM learning_events WHERE event_id=?1",
                [&selected],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(after, before);
        assert_eq!(v2::count(&e, "learning_sessions"), sessions);
    }
}

#[test]
fn ordinary_tutor_and_source_capture_remain_referenceable_without_pending_learner_work() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    v2::start(&e, "claude", THREAD);
    let result = call(
        &e,
        THREAD,
        json!({"schema":"learner-model/v2","action":"update","request_id":"context-only","reported_events":[{"external_event_key":"help","speaker":"tutor","kind":"delivery","text":"Remember that b includes its sign."},{"external_event_key":"source","speaker":"source","kind":"source_context","text":"The problem states x²-5x+6=0."}]}),
    );
    assert_eq!(result["update_status"], "committed", "{result}");
    for id in result["captured_event_ids"].as_array().unwrap() {
        assert_eq!(processing(&e, id.as_str().unwrap()), "not_applicable");
    }
    let read = v2::tool(
        &e,
        "claude",
        THREAD,
        "memory",
        json!({"schema":"learner-model/v2","action":"read","refs":[{"kind":"event","id":result["captured_event_ids"][0]}]}),
    );
    assert_eq!(
        read["records"][0]["text"],
        "Remember that b includes its sign."
    );
    assert_eq!(read["records"][0]["processing"], "not_applicable");
}

fn native_ticket(e: &TestEnvironment, key: &str, text: &str) -> String {
    let hook = v2::prompt(e, "claude", THREAD, Some(key), text);
    assert!(hook.status.success(), "{hook:?}");
    v2::context(&hook)
        .split_once("native_capture.ticket=")
        .expect("paused input offers independent native proof")
        .1
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}

#[test]
fn completed_native_resume_uses_same_session_and_exactly_one_new_event() {
    let (e, session, _) = captured_answer();
    complete(&e, &session);
    let text = "Please resume the same quadratic lesson.";
    let ticket = native_ticket(&e, "resume-same-native", text);
    let served = v2::tool(
        &e,
        "claude",
        THREAD,
        "context",
        json!({"action":"instructions","host":"claude","stage":"consolidate_and_resume","operation":"tool_recipes"}),
    );
    let recipe = served["text"]
        .as_str()
        .unwrap()
        .split_once("<!-- executable-recipe: native_resume -->\n```json\n")
        .unwrap()
        .1
        .split_once("\n```")
        .unwrap()
        .0;
    let mut request: Value = serde_json::from_str(recipe).unwrap();
    request["native_capture"] = json!({"ticket":ticket,"text":text});
    request["session"]["session_id"] = json!(session);
    request["session"]["expected_revision"] = json!(2);

    let result = call(&e, THREAD, request.clone());
    assert_eq!(result["update_status"], "committed", "{result}");
    assert_eq!(result["revisions"][&session], 3);
    assert_eq!(v2::count(&e, "learning_sessions"), 1);
    assert_eq!(v2::count(&e, "learning_events"), 2);
    let new_event = result["captured_event_ids"][0].as_str().unwrap();
    let owner: String = db(&e)
        .query_row(
            "SELECT session_id FROM learning_events WHERE event_id=?1",
            [new_event],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(owner, session);
    assert!(lesson_active(&e));
    assert_eq!(call(&e, THREAD, request), result);
    assert_eq!(v2::count(&e, "learning_events"), 2);
}

#[test]
fn active_native_turn_can_reference_original_work_while_resuming_another_session() {
    let (e, original, event) = captured_answer();
    let unrelated = failed_first_update(&e, "unrelated-pending", "unrelated-pending");
    let target = v2::start(&e, "claude", "saved-lesson")["references"]["session"]
        .as_str()
        .unwrap()
        .to_owned();
    let result = call(
        &e,
        THREAD,
        json!({"schema":"learner-model/v2","action":"update","request_id":"resume-saved-context","turn_ref":event,"session":{"operation":"resume","session_id":target,"expected_revision":1},"observations":[{"client_ref":"context","kind":"learner_work","description":"Previous work is context for returning to the saved lesson.","events":[{"event_id":event,"role":"learner_response"}]}]}),
    );
    assert_eq!(result["update_status"], "committed", "{result}");
    assert_eq!(result["revisions"][&target], 2);
    let owner: String = db(&e)
        .query_row(
            "SELECT session_id FROM learning_events WHERE event_id=?1",
            [&event],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(owner, original);
    let observed_in: String = db(&e)
        .query_row(
            "SELECT session_id FROM observations WHERE observation_id=?1",
            [result["references"]["context"].as_str().unwrap()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(observed_in, target);
    let pending_owner: Option<String> = db(&e)
        .query_row(
            "SELECT session_id FROM learning_events WHERE event_id=?1",
            [&unrelated],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(pending_owner, None);
    assert_eq!(processing(&e, &unrelated), "pending");
    assert_eq!(v2::count(&e, "learning_sessions"), 2);
}

#[test]
fn explicit_selection_of_bound_work_still_fails_atomically() {
    let (e, original, event) = captured_answer();
    let before = event_generation(&e);
    let result = call(
        &e,
        THREAD,
        json!({"schema":"learner-model/v2","action":"update","request_id":"explicit-reassignment","turn_ref":event,"session":{"operation":"start","client_ref":"s","goal":"New lesson","selected_unbound_event_ids":[event]}}),
    );
    assert_eq!(result["error"]["code"], "event_scope_conflict", "{result}");
    assert_eq!(result["error"]["captured_event_ids"], json!([event]));
    let owner: String = db(&e)
        .query_row(
            "SELECT session_id FROM learning_events WHERE event_id=?1",
            [&event],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(owner, original);
    assert_eq!(event_generation(&e), before);
    assert_eq!(v2::count(&e, "learning_sessions"), 1);
}

#[test]
fn native_resume_failed_semantics_keeps_one_unbound_capture_for_same_session_retry() {
    let (e, session, _) = captured_answer();
    complete(&e, &session);
    let text = "Resume our existing lesson without losing its task.";
    let ticket = native_ticket(&e, "resume-retry", text);
    let mut request = json!({"schema":"learner-model/v2","action":"update","request_id":"resume-conflict","native_capture":{"ticket":ticket,"text":text},"session":{"operation":"resume","session_id":session,"expected_revision":99}});
    let failed = call(&e, THREAD, request.clone());
    assert_eq!(failed["error"]["code"], "revision_conflict", "{failed}");
    let input = failed["error"]["captured_event_ids"][0].as_str().unwrap();
    let owner: Option<String> = db(&e)
        .query_row(
            "SELECT session_id FROM learning_events WHERE event_id=?1",
            [input],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(owner, None);
    assert!(!lesson_active(&e));
    assert_eq!(v2::count(&e, "learning_sessions"), 1);
    request["request_id"] = json!("resume-reconciled");
    request["session"]["expected_revision"] = json!(2);
    let result = call(&e, THREAD, request);
    assert_eq!(result["update_status"], "committed", "{result}");
    assert_eq!(result["captured_event_ids"], json!([input]));
    assert_eq!(result["revisions"][&session], 3);
    assert_eq!(v2::count(&e, "learning_events"), 2);
    assert_eq!(v2::count(&e, "learning_sessions"), 1);
    let owner: String = db(&e)
        .query_row(
            "SELECT session_id FROM learning_events WHERE event_id=?1",
            [input],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(owner, session);
}

#[test]
fn native_resume_preserves_exact_text_host_and_freshness_guards() {
    for variant in ["text", "host", "stale"] {
        let (e, session, _) = captured_answer();
        complete(&e, &session);
        let text = "Resume the same saved lesson.";
        let ticket = native_ticket(&e, "guarded-native-resume", text);
        let mut request = json!({"schema":"learner-model/v2","action":"update","request_id":"guarded-resume","native_capture":{"ticket":ticket,"text":text},"session":{"operation":"resume","session_id":session,"expected_revision":2}});
        let thread = match variant {
            "text" => {
                request["native_capture"]["text"] = json!("Paraphrased input");
                THREAD
            }
            "host" => "wrong-host",
            _ => {
                native_ticket(
                    &e,
                    "newer-native-turn",
                    "Actually wait for the next message.",
                );
                THREAD
            }
        };
        let output = v2::tool_output(&e, "claude", thread, "learning_step", request);
        assert!(!output.status.success(), "{variant}: {output:?}");
        assert_eq!(v2::count(&e, "learning_events"), 1, "{variant}");
        assert_eq!(v2::count(&e, "learning_sessions"), 1, "{variant}");
        assert!(!lesson_active(&e), "{variant}");
    }
}
