use crate::support::{TestEnvironment, mcp::McpClient, v2};
use rusqlite::Connection;
use serde_json::{Value, json};

const CORRECTION_AND_PAUSE: &str = r"I’d substitute \(b=-2\), because \(-2\) multiplies \(x\). \(x\) is the unknown I solve for. Could we pause here briefly?";

fn db(e: &TestEnvironment) -> Connection {
    Connection::open(
        e.workspace
            .join(crate::support::CONTROL_DIR)
            .join("state.sqlite"),
    )
    .unwrap()
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
            "'host_kind',host_kind,'host_thread_id',host_thread_id,'session_id',session_id,'learning_active',learning_active",
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

fn store_with_pending_correction() -> (TestEnvironment, String, String, String) {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let start = v2::tool(
        &e,
        "claude",
        "tuesday",
        "learning_step",
        json!({"schema":"learner-model/v2","action":"update","request_id":"tuesday-start","session":{"operation":"start","client_ref":"s","goal":"Identify quadratic coefficients"},"reported_events":[{"external_event_key":"old-work","speaker":"learner","kind":"message","text":"I think b is another unknown."}]}),
    );
    let session = start["references"]["s"].as_str().unwrap().to_owned();
    let event = start["captured_event_ids"][0].as_str().unwrap();
    let saved = v2::tool(
        &e,
        "claude",
        "tuesday",
        "learning_step",
        json!({"schema":"learner-model/v2","action":"update","request_id":"old-interpretation","observations":[{"client_ref":"o","kind":"learner_work","description":"Calls b another unknown.","events":[{"event_id":event,"role":"learner_response"}],"assistance_text":"Role distinction was then explained; later answer not yet available."}],"memory_edits":[{"operation":"create","client_ref":"m","record":{"title":"Quadratic coefficient roles","scope":"Algebra","summary":"Earlier formula success; coefficient roles uncertain.","state_text":"Most recently called b another unknown. Preserve earlier formula success.","current_basis":[{"new_observation":"o"}]},"reason":"Record the limited work.","change_basis":[{"new_observation":"o"}]}],"event_dispositions":[{"event_id":event,"outcome":"processed"}],"session":{"operation":"use","session_id":session,"expected_revision":1},"checkpoint":{"current_problem":"Coefficient versus unknown","working_diagnosis":"Role distinction remains uncertain","next_action":"Ask a fresh b check after the explanation","memory_refs":[{"memory_id":"m","revision":"after_edit"}]}}),
    );
    let memory = saved["references"]["m"].as_str().unwrap().to_owned();
    let native = v2::prompt(
        &e,
        "claude",
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
    let (e, session, memory, correction) = store_with_pending_correction();
    let before = canonical_snapshot(&e);
    let instructions = v2::tool(
        &e,
        "claude",
        "wednesday",
        "context",
        json!({"action":"instructions","host":"claude","stage":"consolidate_and_resume","operation":"tool_recipes"}),
    );
    let text = instructions["text"].as_str().unwrap();
    assert!(text.contains("pending_evidence"));
    let mut client =
        McpClient::spawn_with_env(&e, true, &[("CLAUDE_CODE_SESSION_ID", "wednesday")]);
    client.initialize();
    let mut call_id = 10;
    for mcp in [false, true] {
        let mut call = |name: &str, mut request: Value| {
            request["schema"] = json!("learner-model/v2");
            call_id += 1;
            if mcp {
                client.call_value(call_id, name, request)
            } else {
                v2::tool(&e, "claude", "wednesday", name, request)
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
fn an_ended_lessons_reads_do_not_write_traces_or_cross_workspace() {
    let (e, session, _, correction) = store_with_pending_correction();
    v2::tool(
        &e,
        "claude",
        "tuesday",
        "learning_step",
        json!({"schema":"learner-model/v2","action":"update","request_id":"complete","session":{"operation":"complete","session_id":session,"expected_revision":2}}),
    );
    let other = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&other);
    let foreign = v2::tool(
        &other,
        "claude",
        "other",
        "learning_step",
        json!({"schema":"learner-model/v2","action":"update","request_id":"foreign-start","session":{"operation":"start","client_ref":"session","goal":"FOREIGN WORKSPACE GOAL"},"reported_events":[{"external_event_key":"foreign-one","speaker":"learner","kind":"message","text":"FOREIGN PRIVATE WORK ONE"},{"external_event_key":"foreign-two","speaker":"learner","kind":"message","text":"FOREIGN PRIVATE WORK TWO"}]}),
    );
    let foreign_session = foreign["references"]["session"].as_str().unwrap();
    let before = canonical_snapshot(&e);
    for action in ["prepare", "status", "session_choices"] {
        let packet = v2::tool(
            &e,
            "claude",
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
        "claude",
        "tuesday",
        "memory",
        json!({"schema":"learner-model/v2","action":"read","refs":[{"kind":"event","id":correction}]}),
    );
    assert_eq!(packet["records"][0]["processing"], "pending");
    assert_eq!(packet["trace_status"], "disabled");
    let rejected = v2::tool_output(
        &other,
        "claude",
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
        "claude",
        "tuesday",
        Some("private-later"),
        "PRIVATE MESSAGE MUST NOT BE RECORDED",
    );
    assert!(ignored.status.success());
    assert_eq!(canonical_snapshot(&e), before);
}
