//! Real adapter regressions for closing captured work and explicit pending ownership.
use rusqlite::Connection;
use serde_json::{Value, json};
use support::{TestEnvironment, v2};

const THREAD: &str = "closing-work";
const ANSWER: &str = "b=-5 gives roots 2 and 3; substitution checks both. Let's stop here.";

fn db(e: &TestEnvironment) -> Connection {
    Connection::open(e.workspace.join(support::CONTROL_DIR).join("state.sqlite")).unwrap()
}

fn call(e: &TestEnvironment, thread: &str, request: Value) -> Value {
    let output = v2::tool_output(e, "codex", thread, "learning_step", request);
    serde_json::from_slice(&output.stdout).unwrap_or_else(|_| panic!("{output:?}"))
}

fn captured_answer() -> (TestEnvironment, String, String) {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let started = v2::start(&e, "codex", THREAD);
    let session = started["references"]["session"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        v2::prompt(&e, "codex", THREAD, Some("closing-answer"), ANSWER)
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

fn control(e: &TestEnvironment, event: &str, action: &str) {
    let result = call(
        e,
        THREAD,
        json!({"schema":"learner-model/v2","action":"control","request_id":format!("control-{action}"),"control":action,"turn_ref":event}),
    );
    assert_eq!(result["durable"], true, "{result}");
}

fn completion(session: &str, event: &str) -> Value {
    json!({"schema":"learner-model/v2","action":"update","mode":"complete_captured","request_id":"complete-closing-answer","turn_ref":event,
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

fn binding(e: &TestEnvironment) -> (bool, bool) {
    db(e).query_row("SELECT learning_active,recording_enabled FROM host_bindings WHERE host_kind='codex' AND host_thread_id=?1", [THREAD], |r| Ok((r.get(0)?, r.get(1)?))).unwrap()
}

#[test]
fn answer_plus_pause_or_stop_finishes_without_reopening_or_a_next_task() {
    for action in ["pause", "stop"] {
        let (e, session, event) = captured_answer();
        control(&e, &event, action);
        assert_eq!(binding(&e), (false, true));
        assert_eq!(
            processing(&e, &event),
            "pending",
            "Controls cannot discard mixed answer-plus-stop work"
        );
        let result = call(&e, THREAD, completion(&session, &event));
        assert_eq!(result["update_status"], "committed", "{result}");
        assert_eq!(binding(&e), (false, true));
        assert_eq!(processing(&e, &event), "processed");
        assert_eq!(v2::count(&e, "learning_events"), 1);
        assert_eq!(v2::count(&e, "observations"), 1);
        assert_eq!(v2::count(&e, "learner_memories"), 1);
        let checkpoint: String = db(&e)
            .query_row(
                "SELECT checkpoint_json FROM learning_sessions WHERE session_id=?1",
                [&session],
                |r| r.get(0),
            )
            .unwrap();
        let checkpoint: Value = serde_json::from_str(&checkpoint).unwrap();
        assert_eq!(
            checkpoint["next_action"],
            "Learning remains stopped; await an explicit start or resume",
            "the completion's checkpoint must be stored: {checkpoint}"
        );
        assert!(checkpoint["pending_task"].is_null());
        let receipts = v2::count(&e, "context_receipts");
        let ignored = v2::prompt(
            &e,
            "codex",
            THREAD,
            Some("after-stop"),
            "A new answer after stop must not be captured.",
        );
        assert!(ignored.status.success());
        assert_eq!(v2::count(&e, "learning_events"), 1);
        assert_eq!(v2::count(&e, "context_receipts"), receipts);
    }
}

#[test]
fn recording_off_blocks_completion_text_and_leaves_original_pending() {
    let (e, session, event) = captured_answer();
    control(&e, &event, "recording_off");
    let before_checkpoint: String = db(&e)
        .query_row(
            "SELECT checkpoint_json FROM learning_sessions WHERE session_id=?1",
            [&session],
            |r| r.get(0),
        )
        .unwrap();
    let receipts = v2::count(&e, "context_receipts");
    let result = call(&e, THREAD, completion(&session, &event));
    assert_eq!(result["error"]["code"], "recording_disabled", "{result}");
    assert_eq!(processing(&e, &event), "pending");
    assert_eq!(v2::count(&e, "learning_events"), 1);
    assert_eq!(v2::count(&e, "observations"), 0);
    assert_eq!(v2::count(&e, "learner_memories"), 0);
    assert_eq!(v2::count(&e, "context_receipts"), receipts);
    let after_checkpoint: String = db(&e)
        .query_row(
            "SELECT checkpoint_json FROM learning_sessions WHERE session_id=?1",
            [&session],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(after_checkpoint, before_checkpoint);
}

#[test]
fn failed_completion_preserves_stop_pending_work_and_session_revision() {
    let (e, session, event) = captured_answer();
    control(&e, &event, "stop");
    let mut request = completion(&session, &event);
    request["session"]["expected_revision"] = json!(99);
    let result = call(&e, THREAD, request);
    assert_eq!(result["error"]["code"], "revision_conflict", "{result}");
    assert_eq!(binding(&e), (false, true));
    assert_eq!(processing(&e, &event), "pending");
    assert_eq!(v2::count(&e, "observations"), 0);
    assert_eq!(v2::count(&e, "learner_memories"), 0);
    let revision: i64 = db(&e)
        .query_row(
            "SELECT revision FROM learning_sessions WHERE session_id=?1",
            [&session],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(revision, 1);
}

#[test]
fn completion_replay_cannot_clear_later_stop_or_privacy_controls() {
    let (e, session, event) = captured_answer();
    let request = completion(&session, &event);
    let first = call(&e, THREAD, request.clone());
    assert_eq!(first["update_status"], "committed", "{first}");
    control(&e, &event, "stop");
    control(&e, &event, "recording_off");
    let replay = call(&e, THREAD, request);
    assert_eq!(replay, first);
    assert_eq!(binding(&e), (false, false));
    assert_eq!(v2::count(&e, "observations"), 1);
    assert_eq!(v2::count(&e, "learner_memories"), 1);
}

#[test]
fn completion_rejects_new_raw_input_tasks_and_unscoped_work_without_capture() {
    for variant in ["raw", "task", "resume", "foreign", "unknown_turn"] {
        let (e, session, event) = captured_answer();
        control(&e, &event, "stop");
        let mut request = completion(&session, &event);
        match variant {
            "raw" => {
                request["reported_events"] = json!([{"external_event_key":"forbidden","speaker":"learner","kind":"message","text":"NEW COMPLETION INPUT MUST NOT BE CAPTURED"}])
            }
            "task" => {
                request["checkpoint"]["pending_task"] = json!({"proposal_event_ref":event,"conditions":"A fresh task","observed_delivery_refs":[],"help_refs":[]})
            }
            "resume" => request["session"]["operation"] = json!("resume"),
            "foreign" => {
                let foreign = v2::tool(
                    &e,
                    "codex",
                    "unrelated",
                    "learning_step",
                    json!({"schema":"learner-model/v2","action":"update","request_id":"foreign-start","session":{"operation":"start","client_ref":"s","goal":"Unrelated topic"},"reported_events":[{"external_event_key":"foreign","speaker":"learner","kind":"message","text":"Unrelated learner work"}]}),
                );
                request["observations"][0]["events"][0]["event_id"] =
                    foreign["captured_event_ids"][0].clone();
            }
            "unknown_turn" => request["turn_ref"] = json!("never-captured"),
            _ => unreachable!(),
        }
        let count = v2::count(&e, "learning_events");
        let result = call(&e, THREAD, request);
        assert!(result.get("error").is_some(), "{variant}: {result}");
        if variant == "unknown_turn" {
            assert_eq!(result["error"]["captured_event_ids"], json!([]));
        }
        assert_eq!(binding(&e), (false, true));
        assert_eq!(v2::count(&e, "learning_events"), count);
        assert_eq!(v2::count(&e, "observations"), 0);
        assert_eq!(processing(&e, &event), "pending");
    }
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
    let hook = v2::prompt(&e, "codex", THREAD, Some("initial-native"), ANSWER);
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
    control(&e, event, "stop");
    assert_eq!(event_generation(&e), 2);
    let request = completion(session, event);
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

fn prior_memory(e: &TestEnvironment) -> (String, String, String) {
    let raw = call(
        e,
        THREAD,
        json!({"schema":"learner-model/v2","action":"update","request_id":"older-raw","reported_events":[{"external_event_key":"older-work","speaker":"learner","kind":"message","text":"Earlier I called b a second unknown."},{"external_event_key":"prior-help","speaker":"tutor","kind":"delivery","text":"b is the signed fixed coefficient of x, not a second unknown."}]}),
    );
    let event = &raw["captured_event_ids"][0];
    let saved = call(
        e,
        THREAD,
        json!({"schema":"learner-model/v2","action":"update","request_id":"older-memory","observations":[{"client_ref":"prior","kind":"learner_work","description":"Earlier called b a second unknown.","events":[{"event_id":event,"role":"learner_response"}]}],"memory_edits":[{"operation":"create","client_ref":"prior-memory","record":{"title":"Signed coefficient roles","scope":"Quadratics","summary":"Earlier role uncertainty","state_text":"The earlier example confused the coefficient with the unknown.","current_basis":[{"new_observation":"prior"}]},"reason":"Record earlier concrete work","change_basis":[{"new_observation":"prior"}]}],"event_dispositions":[{"event_id":event,"outcome":"processed"}]}),
    );
    assert_eq!(saved["update_status"], "committed", "{saved}");
    (
        saved["references"]["prior-memory"]
            .as_str()
            .unwrap()
            .to_owned(),
        saved["references"]["prior"].as_str().unwrap().to_owned(),
        raw["captured_event_ids"][1].as_str().unwrap().to_owned(),
    )
}

fn closing_answer_with_prior_memory() -> (TestEnvironment, String, String, String, String, String) {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let start = v2::start(&e, "codex", THREAD);
    let session = start["references"]["session"].as_str().unwrap().to_owned();
    let (memory, observation, help) = prior_memory(&e);
    assert!(
        v2::prompt(&e, "codex", THREAD, Some("closing-after-help"), ANSWER)
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
    (e, session, event, memory, observation, help)
}

#[test]
fn completion_rejects_each_unrelated_memory_edit_or_review_atomically() {
    for variant in ["retire_empty", "retire_old", "revise_old", "review_old"] {
        let (e, session, event, old_memory, old_observation, _) =
            closing_answer_with_prior_memory();
        control(&e, &event, "stop");
        let before_generation = event_generation(&e);
        let mut request = completion(&session, &event);
        if variant == "review_old" {
            request["review_updates"] = json!([{"operation":"update","review_id":"unrelated-review","expected_revision":1,"status":"dismissed"}]);
        } else {
            let unrelated = if variant == "revise_old" {
                json!({"operation":"revise","memory_id":old_memory,"expected_revision":1,"record":{"title":"Unrelated revision","scope":"Quadratics","summary":"Unrelated rewrite","state_text":"This rewrite has only older support.","current_basis":[{"observation_id":old_observation}]},"change_kind":"reinterpretation","reason":"Only old context","change_basis":[{"observation_id":old_observation}]})
            } else {
                let mut edit = json!({"operation":"retire","memory_id":old_memory,"expected_revision":1,"reason":"Unrelated retirement"});
                if variant == "retire_old" {
                    edit["change_basis"] = json!([{"observation_id":old_observation}]);
                }
                edit
            };
            request["memory_edits"]
                .as_array_mut()
                .unwrap()
                .push(unrelated);
        }
        let result = call(&e, THREAD, request);
        assert_eq!(
            result["error"]["code"], "invalid_variant",
            "{variant}: {result}"
        );
        assert_eq!(binding(&e), (false, true));
        assert_eq!(processing(&e, &event), "pending");
        assert_eq!(v2::count(&e, "observations"), 1);
        assert_eq!(v2::count(&e, "learner_memories"), 1);
        assert_eq!(event_generation(&e), before_generation);
        let state: (i64, String) = db(&e)
            .query_row(
                "SELECT revision,lifecycle FROM learner_memories WHERE memory_id=?1",
                [&old_memory],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(state, (1, "active".to_owned()));
    }
}

#[test]
fn completion_revision_can_keep_historical_support_and_original_help_with_current_turn_basis() {
    let (e, session, event, old_memory, old_observation, help) = closing_answer_with_prior_memory();
    control(&e, &event, "stop");
    let mut request = completion(&session, &event);
    request["observations"][0]["events"]
        .as_array_mut()
        .unwrap()
        .push(json!({"event_id":help,"role":"assistance"}));
    request["observations"][0]["assistance_text"] =
        json!("A prior tutor explanation distinguished b from x; independence is not established.");
    let mut record = request["memory_edits"][0]["record"].clone();
    record["current_basis"] =
        json!([{"observation_id":old_observation},{"new_observation":"answer"}]);
    request["memory_edits"] = json!([{"operation":"revise","memory_id":old_memory,"expected_revision":1,"record":record,"change_kind":"new_observation","reason":"Closing answer revises the earlier role uncertainty while retaining prior context","change_basis":[{"observation_id":old_observation},{"new_observation":"answer"}]}]);
    request["checkpoint"]["memory_refs"][0]["memory_id"] = json!(old_memory);
    let result = call(&e, THREAD, request);
    assert_eq!(result["update_status"], "committed", "{result}");
    assert_eq!(result["revisions"][&old_memory], 2);
    assert_eq!(binding(&e), (false, true));
    assert_eq!(processing(&e, &event), "processed");
    assert_eq!(processing(&e, &help), "not_applicable");
    assert_eq!(v2::count(&e, "learner_memories"), 1);
    let basis: String = db(&e)
        .query_row(
            "SELECT current_basis_json FROM learner_memories WHERE memory_id=?1",
            [&old_memory],
            |r| r.get(0),
        )
        .unwrap();
    let basis: Value = serde_json::from_str(&basis).unwrap();
    assert!(basis.as_array().unwrap().contains(&json!(old_observation)));
    assert!(
        basis
            .as_array()
            .unwrap()
            .contains(&result["references"]["answer"])
    );
}

#[test]
fn review_only_completion_uses_actual_closing_activity_without_inventing_performance() {
    for (status, disposition) in [("dismissed", "no_change"), ("done", "processed")] {
        let (e, session, event, memory, _, _) = closing_answer_with_prior_memory();
        let scheduled = call(
            &e,
            THREAD,
            json!({"schema":"learner-model/v2","action":"update","request_id":"scheduled-review","review_updates":[{"operation":"create","client_ref":"review","memory_id":memory,"purpose":"Revisit coefficient labels","reason":"Earlier uncertainty remains worth checking"}]}),
        );
        assert_eq!(scheduled["update_status"], "committed", "{scheduled}");
        let review = scheduled["references"]["review"].as_str().unwrap();
        control(&e, &event, "stop");
        let result = call(
            &e,
            THREAD,
            json!({"schema":"learner-model/v2","action":"update","mode":"complete_captured","request_id":"review-closing-activity","turn_ref":event,"review_updates":[{"operation":"update","review_id":review,"expected_revision":1,"status":status,"activity_event_id":event}],"event_dispositions":[{"event_id":event,"outcome":disposition}]}),
        );
        assert_eq!(result["update_status"], "committed", "{result}");
        assert_eq!(binding(&e), (false, true));
        assert_eq!(processing(&e, &event), disposition);
        assert_eq!(v2::count(&e, "observations"), 1);
        assert_eq!(v2::count(&e, "learner_memories"), 1);
        let saved: (String, i64) = db(&e)
            .query_row(
                "SELECT status,revision FROM review_items WHERE review_id=?1",
                [review],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(saved, (status.to_owned(), 2));
        let revision: i64 = db(&e)
            .query_row(
                "SELECT revision FROM learning_sessions WHERE session_id=?1",
                [&session],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(revision, 1);
    }
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
    assert_eq!(owner, "another-thread");
}

#[test]
fn resume_does_not_import_a_threads_unselected_failed_start_backlog() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let earlier = failed_first_update(&e, THREAD, "before-resume");
    let started = v2::start(&e, "codex", "session-owner");
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
    v2::start(&e, "codex", THREAD);
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
        "codex",
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
    let hook = v2::prompt(e, "codex", THREAD, Some(key), text);
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
fn paused_native_resume_uses_same_session_and_exactly_one_new_event() {
    let (e, session, event) = captured_answer();
    control(&e, &event, "pause");
    let text = "Please resume the same quadratic lesson.";
    let ticket = native_ticket(&e, "resume-same-native", text);
    let served = v2::tool(
        &e,
        "codex",
        THREAD,
        "context",
        json!({"action":"instructions","host":"codex","stage":"consolidate_and_resume","operation":"tool_recipes"}),
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

    let result = call(&e, THREAD, request.clone());
    assert_eq!(result["update_status"], "committed", "{result}");
    assert_eq!(result["revisions"][&session], 2);
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
    assert_eq!(binding(&e), (true, true));
    assert_eq!(call(&e, THREAD, request), result);
    assert_eq!(v2::count(&e, "learning_events"), 2);
}

#[test]
fn active_native_turn_can_reference_original_work_while_resuming_another_session() {
    let (e, original, event) = captured_answer();
    let unrelated = failed_first_update(&e, "unrelated-pending", "unrelated-pending");
    let target = v2::start(&e, "codex", "saved-lesson")["references"]["session"]
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

fn paused_with_task() -> (TestEnvironment, String, String, Value) {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let session = v2::start(&e, "codex", THREAD)["references"]["session"]
        .as_str()
        .unwrap()
        .to_owned();
    let proposal = call(
        &e,
        THREAD,
        json!({"schema":"learner-model/v2","action":"update","request_id":"task-delivery","reported_events":[{"external_event_key":"actual-task-delivery","speaker":"tutor","kind":"delivery","text":"Solve x squared minus five x plus six equals zero, then check the roots.","metadata":{"delivery":"observed"}}]}),
    );
    assert_eq!(proposal["update_status"], "committed", "{proposal}");
    let task_event = proposal["captured_event_ids"][0].as_str().unwrap();
    let task = json!({"proposal_event_ref":task_event,"conditions":"Check the roots by substitution.","observed_delivery_refs":[task_event],"help_refs":[],"reference_answer_ref":null});
    let saved = call(
        &e,
        THREAD,
        json!({"schema":"learner-model/v2","action":"update","request_id":"save-task","session":{"operation":"use","session_id":session,"expected_revision":1},"checkpoint":{"current_problem":"Solve and check the quadratic","working_diagnosis":"No answer yet","next_action":"Await learner reasoning","pending_task":task}}),
    );
    assert_eq!(saved["update_status"], "committed", "{saved}");
    let partial = "I found one root is 2. I haven't checked the other yet; pause here.";
    assert!(
        v2::prompt(&e, "codex", THREAD, Some("partial-pause"), partial)
            .status
            .success()
    );
    let event: String = db(&e)
        .query_row(
            "SELECT event_id FROM learning_events WHERE text=?1",
            [partial],
            |row| row.get(0),
        )
        .unwrap();
    control(&e, &event, "pause");
    (e, session, event, task)
}

fn partial_completion(session: &str, event: &str) -> Value {
    let mut request = completion(session, event);
    request["session"]["expected_revision"] = json!(2);
    request["observations"][0]["description"] =
        json!("Identified one root as 2 and explicitly left the other check unfinished.");
    request["memory_edits"][0]["record"]["summary"] = json!("Partial solution; checks unfinished.");
    request["memory_edits"][0]["record"]["state_text"] = json!(
        "The learner found one root as 2. The other root and checking work remain unfinished; this turn does not establish a complete solution."
    );
    request["checkpoint"]["working_diagnosis"] =
        json!("Found one root; checking the other is unfinished.");
    request
}

#[test]
fn completion_preserves_typed_existing_task_while_updating_stopping_point() {
    let (e, session, event, task) = paused_with_task();
    let mut request = partial_completion(&session, &event);
    request["session"]["expected_revision"] = json!(2);
    request["checkpoint"]["pending_task"] = task.clone();
    // Omitted defaults are semantically equal despite different JSON fields.
    request["checkpoint"]["pending_task"]
        .as_object_mut()
        .unwrap()
        .remove("help_refs");
    request["checkpoint"]["pending_task"]
        .as_object_mut()
        .unwrap()
        .remove("reference_answer_ref");
    request["checkpoint"]["working_diagnosis"] =
        json!("Found one root; checking the other is unfinished.");
    let result = call(&e, THREAD, request);
    assert_eq!(result["update_status"], "committed", "{result}");
    assert_eq!(result["revisions"][&session], 3);
    let memory = result["references"]["memory"].as_str().unwrap();
    assert_eq!(result["revisions"][memory], 1);
    assert_eq!(v2::count(&e, "learning_events"), 2);
    assert_eq!(v2::count(&e, "observations"), 1);
    assert_eq!(v2::count(&e, "learner_memories"), 1);
    assert_eq!(binding(&e), (false, true));
    let checkpoint: String = db(&e)
        .query_row(
            "SELECT checkpoint_json FROM learning_sessions WHERE session_id=?1",
            [&session],
            |row| row.get(0),
        )
        .unwrap();
    let checkpoint: Value = serde_json::from_str(&checkpoint).unwrap();
    assert_eq!(checkpoint["pending_task"], task);
    assert_eq!(
        checkpoint["working_diagnosis"],
        "Found one root; checking the other is unfinished."
    );
}

#[test]
fn task_completion_change_conflict_and_privacy_failures_keep_task_and_raw_answer() {
    for variant in ["changed_task", "revision_conflict", "recording_off"] {
        let (e, session, event, task) = paused_with_task();
        let before: String = db(&e)
            .query_row(
                "SELECT checkpoint_json FROM learning_sessions WHERE session_id=?1",
                [&session],
                |row| row.get(0),
            )
            .unwrap();
        let mut request = partial_completion(&session, &event);
        request["session"]["expected_revision"] = json!(2);
        request["checkpoint"]["pending_task"] = task;
        let code = match variant {
            "changed_task" => {
                request["checkpoint"]["pending_task"]["conditions"] = json!("A changed problem");
                "invalid_variant"
            }
            "revision_conflict" => {
                request["session"]["expected_revision"] = json!(1);
                "revision_conflict"
            }
            _ => {
                control(&e, &event, "recording_off");
                "recording_disabled"
            }
        };
        let result = call(&e, THREAD, request);
        assert_eq!(result["error"]["code"], code, "{variant}: {result}");
        let after: String = db(&e)
            .query_row(
                "SELECT checkpoint_json FROM learning_sessions WHERE session_id=?1",
                [&session],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(before, after);
        assert_eq!(processing(&e, &event), "pending");
        assert_eq!(v2::count(&e, "observations"), 0);
        assert_eq!(v2::count(&e, "learner_memories"), 0);
        assert!(!binding(&e).0);
    }
}

#[test]
fn native_resume_failed_semantics_keeps_one_unbound_capture_for_same_session_retry() {
    let (e, session, event) = captured_answer();
    control(&e, &event, "pause");
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
    assert_eq!(binding(&e), (false, true));
    assert_eq!(v2::count(&e, "learning_sessions"), 1);
    request["request_id"] = json!("resume-reconciled");
    request["session"]["expected_revision"] = json!(1);
    let result = call(&e, THREAD, request);
    assert_eq!(result["update_status"], "committed", "{result}");
    assert_eq!(result["captured_event_ids"], json!([input]));
    assert_eq!(result["revisions"][&session], 2);
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
fn native_resume_preserves_exact_text_host_freshness_and_recording_guards() {
    for variant in ["text", "host", "stale", "recording_off"] {
        let (e, session, event) = captured_answer();
        control(&e, &event, "pause");
        let text = "Resume the same saved lesson.";
        let ticket = native_ticket(&e, "guarded-native-resume", text);
        let mut request = json!({"schema":"learner-model/v2","action":"update","request_id":"guarded-resume","native_capture":{"ticket":ticket,"text":text},"session":{"operation":"resume","session_id":session,"expected_revision":1}});
        let thread = match variant {
            "text" => {
                request["native_capture"]["text"] = json!("Paraphrased input");
                THREAD
            }
            "host" => "wrong-host",
            "stale" => {
                native_ticket(
                    &e,
                    "newer-native-turn",
                    "Actually wait for the next message.",
                );
                THREAD
            }
            _ => {
                control(&e, &event, "recording_off");
                THREAD
            }
        };
        let output = v2::tool_output(&e, "codex", thread, "learning_step", request);
        assert!(!output.status.success(), "{variant}: {output:?}");
        assert_eq!(v2::count(&e, "learning_events"), 1, "{variant}");
        assert_eq!(v2::count(&e, "learning_sessions"), 1, "{variant}");
        assert!(!binding(&e).0, "{variant}");
    }
}

#[test]
fn completion_can_preserve_whole_checkpoint_or_explicitly_clear_existing_task() {
    for keep_checkpoint in [true, false] {
        let (e, session, event, task) = paused_with_task();
        let before: String = db(&e)
            .query_row(
                "SELECT checkpoint_json FROM learning_sessions WHERE session_id=?1",
                [&session],
                |row| row.get(0),
            )
            .unwrap();
        let mut request = partial_completion(&session, &event);
        request["session"]["expected_revision"] = json!(2);
        if keep_checkpoint {
            request.as_object_mut().unwrap().remove("checkpoint");
        }
        let result = call(&e, THREAD, request);
        assert_eq!(result["update_status"], "committed", "{result}");
        let after: String = db(&e)
            .query_row(
                "SELECT checkpoint_json FROM learning_sessions WHERE session_id=?1",
                [&session],
                |row| row.get(0),
            )
            .unwrap();
        if keep_checkpoint {
            assert_eq!(after, before);
            assert_eq!(
                serde_json::from_str::<Value>(&after).unwrap()["pending_task"],
                task
            );
        } else {
            assert!(serde_json::from_str::<Value>(&after).unwrap()["pending_task"].is_null());
        }
        assert_eq!(binding(&e), (false, true));
    }
}
