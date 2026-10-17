//! Actual adapter reads of unprocessed native work across host conversations.
//! These deterministic checks establish discovery and control behavior, not
//! whether a live tutor chooses or interprets the recovered evidence correctly.
use rusqlite::Connection;
use serde_json::{Value, json};
use support::{TestEnvironment, mcp::McpClient, v2};

const CORRECTION_AND_PAUSE: &str = r"I’d substitute \(b=-2\), because \(-2\) multiplies \(x\). \(x\) is the unknown I solve for. Could we pause here briefly?";

fn db(e: &TestEnvironment) -> Connection {
    Connection::open(e.workspace.join(support::CONTROL_DIR).join("state.sqlite")).unwrap()
}

fn canonical_snapshot(e: &TestEnvironment) -> Value {
    let c = db(e);
    let mut state = serde_json::Map::new();
    for (table, fields) in [
        (
            "workspace_meta",
            "'memory_generation',memory_generation,'observation_generation',observation_generation",
        ),
        (
            "host_bindings",
            "'host_kind',host_kind,'host_thread_id',host_thread_id,'session_id',session_id,'learning_active',learning_active,'recording_enabled',recording_enabled",
        ),
        (
            "learning_sessions",
            "'session_id',session_id,'status',status,'revision',revision,'checkpoint',checkpoint_json",
        ),
        (
            "learning_events",
            "'event_id',event_id,'session_id',session_id,'text',text,'processing',processing,'request',processed_request_id",
        ),
        (
            "learner_memories",
            "'memory_id',memory_id,'revision',revision,'state_text',state_text,'basis',current_basis_json",
        ),
    ] {
        let text: String = c
            .query_row(
                &format!("SELECT json_group_array(json_object({fields})) FROM {table}"),
                [],
                |r| r.get(0),
            )
            .unwrap();
        state.insert(table.into(), serde_json::from_str(&text).unwrap());
    }
    for table in [
        "observations",
        "memory_changes",
        "operation_receipts",
        "context_receipts",
    ] {
        let count: i64 = c
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        state.insert(table.into(), json!(count));
    }
    Value::Object(state)
}

fn paused_store() -> (TestEnvironment, String, String, String) {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let start = v2::tool(
        &e,
        "codex",
        "tuesday",
        "learning_step",
        json!({"schema":"learner-model/v2","action":"update","request_id":"tuesday-start","session":{"operation":"start","client_ref":"s","goal":"Identify quadratic coefficients"},"reported_events":[{"external_event_key":"old-work","speaker":"learner","kind":"message","text":"I think b is another unknown."}]}),
    );
    let session = start["references"]["s"].as_str().unwrap().to_owned();
    let event = start["captured_event_ids"][0].as_str().unwrap();
    let saved = v2::tool(
        &e,
        "codex",
        "tuesday",
        "learning_step",
        json!({"schema":"learner-model/v2","action":"update","request_id":"old-interpretation","observations":[{"client_ref":"o","kind":"learner_work","description":"Calls b another unknown.","events":[{"event_id":event,"role":"learner_response"}],"assistance_text":"Role distinction was then explained; later answer not yet available."}],"memory_edits":[{"operation":"create","client_ref":"m","record":{"title":"Quadratic coefficient roles","scope":"Algebra","summary":"Earlier formula success; coefficient roles uncertain.","state_text":"Most recently called b another unknown. Preserve earlier formula success.","current_basis":[{"new_observation":"o"}]},"reason":"Record the limited work.","change_basis":[{"new_observation":"o"}]}],"event_dispositions":[{"event_id":event,"outcome":"processed"}],"session":{"operation":"use","session_id":session,"expected_revision":1},"checkpoint":{"current_problem":"Coefficient versus unknown","working_diagnosis":"Role distinction remains uncertain","next_action":"Ask a fresh b check after the explanation","memory_refs":[{"memory_id":"m","revision":"after_edit"}]}}),
    );
    let memory = saved["references"]["m"].as_str().unwrap().to_owned();
    let native = v2::prompt(
        &e,
        "codex",
        "tuesday",
        Some("correct-and-pause"),
        CORRECTION_AND_PAUSE,
    );
    assert!(native.status.success(), "{native:?}");
    let correction: String = db(&e)
        .query_row(
            "SELECT event_id FROM learning_events WHERE text=?1",
            [CORRECTION_AND_PAUSE],
            |r| r.get(0),
        )
        .unwrap();
    let paused = v2::tool(
        &e,
        "codex",
        "tuesday",
        "learning_step",
        json!({"schema":"learner-model/v2","action":"control","request_id":"pause","control":"pause","turn_ref":correction}),
    );
    assert_eq!(paused["durable"], true);
    (e, session, memory, correction)
}

fn recipe(text: &str, name: &str) -> Value {
    let marker = format!("<!-- executable-recipe: {name} -->\n```json\n");
    serde_json::from_str(
        text.split_once(&marker)
            .unwrap()
            .1
            .split_once("\n```")
            .unwrap()
            .0,
    )
    .unwrap()
}

#[test]
fn fresh_cli_and_mcp_recover_raw_correction_without_resuming_or_assessing() {
    let (e, session, memory, correction) = paused_store();
    let before = canonical_snapshot(&e);
    let instructions = v2::tool(
        &e,
        "codex",
        "wednesday",
        "context",
        json!({"action":"instructions","host":"codex","stage":"consolidate_and_resume","operation":"tool_recipes"}),
    );
    let text = instructions["text"].as_str().unwrap();
    assert!(text.contains("pending_evidence"));
    let mut client = McpClient::spawn_with_env(&e, true, &[("CODEX_THREAD_ID", "wednesday")]);
    client.initialize();
    let mut call_id = 10;
    for mcp in [false, true] {
        let mut call = |name: &str, mut request: Value| {
            request["schema"] = json!("learner-model/v2");
            call_id += 1;
            if mcp {
                client.call_value(call_id, name, request)
            } else {
                v2::tool(&e, "codex", "wednesday", name, request)
            }
        };
        let fresh = call(
            "context",
            json!({"action":"prepare","goal":"Check current understanding, then solve a new quadratic","known_revisions":[{"memory_id":memory,"revision":1}]}),
        );
        assert_eq!(
            fresh["pending_evidence"]["workspace_pending_event_count"],
            1
        );
        assert_eq!(
            fresh["pending_evidence"]["current_host_pending_event_count"],
            0
        );
        assert_eq!(
            fresh["pending_evidence"]["interpretation_status"],
            "uninterpreted"
        );
        assert_eq!(fresh["revision_notices"][0]["status"], "unchanged");
        assert_eq!(fresh["memories"][0]["revision"], 1);
        assert!(fresh["pending_event_refs"].as_array().unwrap().is_empty());
        assert!(!fresh.to_string().contains("b=-2"));
        let choices = call("context", recipe(text, "pending_session_choices"));
        let choice = &choices["session_choices"][0];
        assert_eq!(choice["session_id"], session);
        assert_eq!(choice["pending_event_count"], 1);
        let mut pending_recipe = recipe(text, "pending_session_events");
        pending_recipe["session_id"] = json!(session);
        let pending = call("context", pending_recipe);
        assert_eq!(pending["pending_event_refs"][0]["id"], correction);
        let selected = call("context", json!({"action":"prepare","session_id":session}));
        assert_eq!(selected["pending_event_refs"][0], correction);
        let mut read_recipe = recipe(text, "pending_event_read");
        read_recipe["refs"][0]["id"] = json!(correction);
        let original = call("memory", read_recipe);
        assert_eq!(original["records"][0]["text"], CORRECTION_AND_PAUSE);
        assert_eq!(original["records"][0]["origin"], "host_observed");
        assert_eq!(original["records"][0]["processing"], "pending");
        assert_eq!(original["trace_status"], "disabled");
        assert_eq!(canonical_snapshot(&e), before);
    }
    client.close_cleanly();
    assert_eq!(canonical_snapshot(&e), before);
}

#[test]
fn stopped_recording_off_reads_do_not_write_traces_or_cross_workspace() {
    let (e, session, _, correction) = paused_store();
    v2::tool(
        &e,
        "codex",
        "tuesday",
        "learning_step",
        json!({"schema":"learner-model/v2","action":"control","request_id":"privacy","control":"recording_off","reported_event":{"external_event_key":"privacy-control","speaker":"learner","kind":"message","text":"Turn recording off."}}),
    );
    let other = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&other);
    let foreign = v2::tool(
        &other,
        "codex",
        "other",
        "learning_step",
        json!({"schema":"learner-model/v2","action":"update","request_id":"foreign-start","session":{"operation":"start","client_ref":"session","goal":"FOREIGN WORKSPACE GOAL"},"reported_events":[{"external_event_key":"foreign-one","speaker":"learner","kind":"message","text":"FOREIGN PRIVATE WORK ONE"},{"external_event_key":"foreign-two","speaker":"learner","kind":"message","text":"FOREIGN PRIVATE WORK TWO"}]}),
    );
    let foreign_session = foreign["references"]["session"].as_str().unwrap();
    let before = canonical_snapshot(&e);
    for action in ["prepare", "status", "session_choices"] {
        let packet = v2::tool(
            &e,
            "codex",
            "tuesday",
            "context",
            json!({"schema":"learner-model/v2","action":action}),
        );
        assert_eq!(
            packet["pending_evidence"]["workspace_pending_event_count"],
            1
        );
        assert_eq!(packet["trace_status"], "disabled");
        assert!(!packet.to_string().contains(foreign_session));
    }
    let packet = v2::tool(
        &e,
        "codex",
        "tuesday",
        "memory",
        json!({"schema":"learner-model/v2","action":"read","refs":[{"kind":"event","id":correction}]}),
    );
    assert_eq!(packet["records"][0]["processing"], "pending");
    assert_eq!(packet["trace_status"], "disabled");
    let rejected = v2::tool_output(
        &other,
        "codex",
        "other",
        "context",
        json!({"schema":"learner-model/v2","action":"pending_events","session_id":session}),
    );
    let failure = format!(
        "{}{}",
        String::from_utf8_lossy(&rejected.stdout),
        String::from_utf8_lossy(&rejected.stderr)
    );
    assert!(failure.contains("unknown_reference"), "{rejected:?}");
    let ignored = v2::prompt(
        &e,
        "codex",
        "tuesday",
        Some("private-later"),
        "PRIVATE MESSAGE MUST NOT BE RECORDED",
    );
    assert!(ignored.status.success());
    assert_eq!(canonical_snapshot(&e), before);
}

#[test]
fn pause_stop_and_recording_off_honor_a_non_null_pending_task() {
    for control in ["pause", "stop", "recording_off"] {
        let e = TestEnvironment::from_fixture("basic").unwrap();
        v2::init(&e);
        let start = v2::start(&e, "codex", "pending-task");
        let session = start["references"]["session"].as_str().unwrap();
        let proposal = v2::tool(
            &e,
            "codex",
            "pending-task",
            "learning_step",
            json!({"schema":"learner-model/v2","action":"update","request_id":"proposal","reported_events":[{"external_event_key":"task-proposal","speaker":"tutor","kind":"task_proposal","text":"Identify a, b, and c in 5x² − 2x + 4 = 0.","metadata":{"delivery":"proposed"}}]}),
        );
        let proposed_event = &proposal["captured_event_ids"][0];
        let pending_task = json!({"proposal_event_ref":proposed_event,"conditions":"A proposed coefficient check; delivery not established.","observed_delivery_refs":[],"help_refs":[],"reference_answer_ref":null});
        v2::tool(
            &e,
            "codex",
            "pending-task",
            "learning_step",
            json!({"schema":"learner-model/v2","action":"update","request_id":"checkpoint","session":{"operation":"use","session_id":session,"expected_revision":1},"checkpoint":{"current_problem":"Coefficient roles","working_diagnosis":"No answer supplied","next_action":"Await the requested operation or honor learner control","pending_task":pending_task},"event_dispositions":[{"event_id":proposed_event,"outcome":"no_change"}]}),
        );
        let before_control = canonical_snapshot(&e);
        let controlled = v2::tool(
            &e,
            "codex",
            "pending-task",
            "learning_step",
            json!({"schema":"learner-model/v2","action":"control","request_id":"control","control":control,"reported_event":{"external_event_key":"learner-control","speaker":"learner","kind":"message","text":format!("Please {control} now.")}}),
        );
        assert_eq!(controlled["durable"], true);
        let after = canonical_snapshot(&e);
        assert_eq!(
            after["learning_sessions"],
            before_control["learning_sessions"]
        );
        assert_eq!(after["learning_events"], before_control["learning_events"]);
        assert_eq!(after["observations"], before_control["observations"]);
        let status = v2::tool(
            &e,
            "codex",
            "pending-task",
            "context",
            json!({"schema":"learner-model/v2","action":"status"}),
        );
        assert_eq!(
            status["session"]["checkpoint"]["pending_task"],
            pending_task
        );
        assert_eq!(status["trace_status"], "disabled");
        let ignored = v2::prompt(
            &e,
            "codex",
            "pending-task",
            Some("after-control"),
            "UNREQUESTED FOLLOW-UP MUST NOT BE CAPTURED",
        );
        assert!(ignored.status.success(), "{ignored:?}");
        assert_eq!(canonical_snapshot(&e), after);
        let binding = &after["host_bindings"][0];
        if control == "recording_off" {
            assert_eq!(binding["recording_enabled"], 0);
        } else {
            assert_eq!(binding["learning_active"], 0);
        }
    }
}
