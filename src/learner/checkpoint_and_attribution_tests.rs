//! Real writer/read regressions for bounded checkpoint basis and task event facts.
use super::*;
use serde_json::{Value, json};

struct Fixture {
    store: LearnerStore,
    host: HostContext,
    path: std::path::PathBuf,
    session: String,
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "magnesium-checkpoint-attribution-{}.sqlite",
            store::new_id("test").unwrap()
        ));
        let store = LearnerStore::initialize(&path, "checkpoint-attribution").unwrap();
        let host = HostContext {
            host_kind: "test".into(),
            host_thread_id: "session-a".into(),
        };
        let session = store.update(&host, json!({"schema":SCHEMA,"action":"update","request_id":"start-a","session":{"operation":"start","client_ref":"session","goal":"Coefficient labels"}})).unwrap().references["session"].clone();
        Self {
            store,
            host,
            path,
            session,
        }
    }
    fn capture(
        &self,
        key: &str,
        speaker: Speaker,
        delivery: Option<Delivery>,
        origin: &str,
    ) -> String {
        self.store
            .capture(
                &self.host,
                &[RawEvent {
                    external_event_key: key.into(),
                    speaker,
                    kind: "message".into(),
                    text: format!("Original {key}"),
                    metadata: Some(EventMetadata {
                        delivery,
                        ..Default::default()
                    }),
                    occurred_at_ms: Some(42),
                }],
                origin,
                false,
            )
            .unwrap()
            .remove(0)
    }
    fn update(&self, mut request: Value) -> Result<UpdateResult> {
        request["schema"] = json!(SCHEMA);
        request["action"] = json!("update");
        self.store.update(&self.host, request)
    }
    fn memory(&self, key: &str) -> (String, String) {
        let event = self.capture(key, Speaker::Learner, None, "host_observed");
        let result = self.update(json!({"request_id":key,"observations":[{"client_ref":"work","kind":"learner_work","description":"Correct formula use; labels still need clarification","events":[{"event_id":event,"role":"learner_response"}]}],"memory_edits":[{"operation":"create","client_ref":"memory","record":{"title":"Coefficient labels","scope":"algebra","summary":"Can apply the quadratic formula","state_text":"Formula application is demonstrated; coefficient identification needs repair.","current_basis":[{"new_observation":"work"}]},"reason":"Observed work","change_basis":[{"new_observation":"work"}]}],"event_dispositions":[{"event_id":event,"outcome":"processed"}]})).unwrap();
        (
            result.references["memory"].clone(),
            result.references["work"].clone(),
        )
    }
    fn checkpoint(
        &self,
        refs: Value,
        task: Value,
        request_id: &str,
        revision: i64,
    ) -> Result<UpdateResult> {
        self.update(json!({"request_id":request_id,"session":{"operation":"use","session_id":self.session,"expected_revision":revision},"checkpoint":{"current_problem":"Identify b","working_diagnosis":"Coefficient identification needs repair","next_action":"Contrast fixed coefficients with variable x","memory_refs":refs,"pending_task":task}}))
    }
    fn read(&self, mut request: Value) -> std::result::Result<Value, String> {
        request["schema"] = json!(SCHEMA);
        read::test_support::execute(&self.store.conn, "fresh-host", "fresh-thread", request)
    }
    fn stored_state(&self) -> Value {
        let checkpoint: String = self
            .store
            .conn
            .query_row(
                "SELECT checkpoint_json FROM learning_sessions WHERE session_id=?1",
                [&self.session],
                |r| r.get(0),
            )
            .unwrap();
        let session_revision: i64 = self
            .store
            .conn
            .query_row(
                "SELECT revision FROM learning_sessions WHERE session_id=?1",
                [&self.session],
                |r| r.get(0),
            )
            .unwrap();
        let mut statement = self.store.conn.prepare("SELECT memory_id,revision,lifecycle,state_text FROM learner_memories ORDER BY memory_id").unwrap();
        let memories = statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        let mut statement = self
            .store
            .conn
            .prepare("SELECT event_id,processing,session_id FROM learning_events ORDER BY event_id")
            .unwrap();
        let events = statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        json!({"checkpoint":checkpoint,"session_revision":session_revision,"memories":memories,"events":events})
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[test]
fn checkpoint_basis_changed_on_fresh_host_without_known_revisions() {
    let f = Fixture::new();
    let (memory, observation) = f.memory("initial-work");
    f.checkpoint(
        json!([{"memory_id":memory,"revision":1}]),
        Value::Null,
        "checkpoint-a",
        1,
    )
    .unwrap();
    let host_b = HostContext {
        host_kind: "test".into(),
        host_thread_id: "session-b".into(),
    };
    f.store.update(&host_b, json!({"schema":SCHEMA,"action":"update","request_id":"start-b","session":{"operation":"start","client_ref":"session","goal":"Later work"}})).unwrap();
    let later_event = f
        .store
        .capture(
            &host_b,
            &[RawEvent {
                external_event_key: "later-coefficient-work".into(),
                speaker: Speaker::Learner,
                kind: "message".into(),
                text: "For x² - 7x + 6, b is the fixed coefficient -7.".into(),
                metadata: None,
                occurred_at_ms: Some(100),
            }],
            "host_observed",
            false,
        )
        .unwrap()
        .remove(0);
    f.store.update(&host_b, json!({"schema":SCHEMA,"action":"update","request_id":"revise-b","observations":[{"client_ref":"later-work","kind":"learner_work","description":"Correctly identifies b as the fixed coefficient -7","events":[{"event_id":later_event,"role":"learner_response"}]}],"memory_edits":[{"operation":"revise","memory_id":memory,"expected_revision":1,"record":{"title":"Coefficient labels","scope":"algebra","summary":"Can apply the formula and identify coefficients","state_text":"Correct formula application remains demonstrated. Coefficient identification is now correct on the observed task.","current_basis":[{"observation_id":observation},{"new_observation":"later-work"}]},"change_kind":"new_observation","reason":"Later observed coefficient identification","change_basis":[{"new_observation":"later-work"}]}],"event_dispositions":[{"event_id":later_event,"outcome":"processed"}]})).unwrap();
    let before = f.stored_state();
    for action in ["status", "prepare"] {
        let mut request = json!({"action":action,"session_id":f.session});
        if action == "prepare" {
            request["known_revisions"] = json!([]);
        }
        let packet = f.read(request).unwrap();
        assert_eq!(
            packet["checkpoint_basis"]["status"],
            "needs_reconsideration"
        );
        let notice = &packet["checkpoint_basis"]["memories"][0];
        assert_eq!(notice["used_revision"], 1);
        assert_eq!(notice["current_revision"], 2);
        assert_eq!(notice["lifecycle"], "active");
        assert_eq!(notice["status"], "changed");
        assert!(packet["revision_notices"].as_array().unwrap().is_empty());
        assert_eq!(
            packet["session"]["checkpoint"]["working_diagnosis"],
            "Coefficient identification needs repair"
        );
        let call: read::NextCall = serde_json::from_value(notice["next_call"].clone()).unwrap();
        let call = serde_json::to_value(call).unwrap();
        let current = f.read(call["arguments"].clone()).unwrap();
        assert!(
            current["records"][0]["state_text"]
                .as_str()
                .unwrap()
                .contains("Correct formula application remains demonstrated")
        );
        assert_eq!(current["records"][0]["revision"], 2);
    }
    assert_eq!(f.stored_state(), before);
}

#[test]
fn checkpoint_basis_unrelated_retired_unreferenced_and_budget() {
    let f = Fixture::new();
    let (memory, _) = f.memory("basis-work");
    let (replacement, _) = f.memory("unrelated-work");
    f.checkpoint(
        json!([{"memory_id":memory,"revision":1}]),
        Value::Null,
        "save",
        1,
    )
    .unwrap();
    f.update(json!({"request_id":"retire-unrelated","memory_edits":[{"operation":"retire","memory_id":replacement,"expected_revision":1,"reason":"Unrelated retirement"}]})).unwrap();
    let unchanged = f
        .read(json!({"action":"status","session_id":f.session}))
        .unwrap();
    assert_eq!(unchanged["checkpoint_basis"]["status"], "unchanged");
    let (direct, observation) = f.memory("replacement-work");
    f.update(json!({"request_id":"flag-basis","memory_edits":[{"operation":"flag","memory_id":memory,"expected_revision":1,"review_note":"Reconsider label description","reason":"Unresolved question","change_basis":[{"observation_id":observation}]}]})).unwrap();
    let flagged = f
        .read(json!({"action":"status","session_id":f.session}))
        .unwrap();
    assert_eq!(
        flagged["checkpoint_basis"]["memories"][0]["has_review_note"],
        true
    );
    assert!(
        flagged["checkpoint_basis"]["memories"][0]
            .get("review_note")
            .is_none()
    );
    let full = f
        .read(flagged["checkpoint_basis"]["memories"][0]["next_call"]["arguments"].clone())
        .unwrap();
    assert_eq!(
        full["records"][0]["review_note"],
        "Reconsider label description"
    );
    f.update(json!({"request_id":"retire-basis","memory_edits":[{"operation":"retire","memory_id":memory,"expected_revision":2,"reason":"Use direct replacement","replacement_ids":[direct]}]})).unwrap();
    let retired = f
        .read(json!({"action":"status","session_id":f.session}))
        .unwrap();
    assert_eq!(
        retired["checkpoint_basis"]["status"],
        "needs_reconsideration"
    );
    let notice = &retired["checkpoint_basis"]["memories"][0];
    assert_eq!(notice["status"], "retired");
    assert_eq!(notice["replacement_ids"], json!([direct]));
    let read = f.read(notice["next_call"]["arguments"].clone()).unwrap();
    assert_eq!(read["records"][0]["lifecycle"], "retired");
    assert!(read["records"][0]["state_text"].is_null());
    let tiny = f.read(json!({"action":"prepare","mode":"focused","session_id":f.session,"known_revisions":[],"max_bytes":1200}));
    match tiny {
        Ok(packet) => assert_eq!(
            packet["checkpoint_basis"]["status"],
            "needs_reconsideration"
        ),
        Err(error) => {
            assert!(error.contains("budget_too_small") && error.contains("increase max_bytes"))
        }
    }
    f.checkpoint(json!([]), Value::Null, "unreferenced", 2)
        .unwrap();
    assert_eq!(
        f.read(json!({"action":"status","session_id":f.session}))
            .unwrap()["checkpoint_basis"]["status"],
        "unreferenced"
    );
    let empty = Fixture::new();
    assert_eq!(
        empty
            .read(json!({"action":"status","session_id":empty.session}))
            .unwrap()["checkpoint_basis"]["status"],
        "unreferenced"
    );
}

#[test]
fn observed_delivery_rejects_contradictions_and_preserves_raw_capture() {
    for (speaker, delivery) in [
        (Speaker::Tutor, Some(Delivery::Proposed)),
        (Speaker::Learner, Some(Delivery::Observed)),
        (Speaker::Tutor, Some(Delivery::Unknown)),
        (Speaker::Tutor, None),
    ] {
        let f = Fixture::new();
        let proposal = f.capture(
            "proposal",
            Speaker::Tutor,
            Some(Delivery::Proposed),
            "host_observed",
        );
        let valid = f.capture(
            "valid",
            Speaker::Tutor,
            Some(Delivery::Observed),
            "host_observed",
        );
        let invalid = f.capture("invalid", speaker, delivery, "host_observed");
        let semantic_input = f.capture("semantic-input", Speaker::Learner, None, "host_observed");
        let before = f.stored_state();
        let mut request = json!({"request_id":"bad-delivery","reported_events":[{"external_event_key":"independent-raw","speaker":"learner","kind":"message","text":"Raw capture must survive semantic rollback"}],"session":{"operation":"use","session_id":f.session,"expected_revision":1},"checkpoint":{"current_problem":"Task","working_diagnosis":"No answer","next_action":"Await answer","pending_task":{"proposal_event_ref":proposal,"conditions":"Given task","observed_delivery_refs":[valid,invalid],"help_refs":[]}}});
        request["observations"] = json!([{"client_ref":"rolled-back-work","kind":"learner_work","description":"Valid semantic observation must roll back","events":[{"event_id":semantic_input,"role":"learner_response"}]}]);
        request["memory_edits"] = json!([{"operation":"create","client_ref":"rolled-back-memory","record":{"title":"Atomic rollback","scope":"algebra","summary":"No state should persist","state_text":"Valid memory before rejected task delivery","current_basis":[{"new_observation":"rolled-back-work"}]},"reason":"Observed work","change_basis":[{"new_observation":"rolled-back-work"}]}]);
        request["event_dispositions"] = json!([{"event_id":semantic_input,"outcome":"processed"}]);
        let error = f.update(request).unwrap_err();
        assert_eq!(error.code, "invalid_variant");
        assert_eq!(
            error.recovery.as_ref().unwrap()["field_path"],
            "checkpoint.pending_task.observed_delivery_refs[1]"
        );
        assert!(error.message.contains("leave observed_delivery_refs empty"));
        assert_eq!(error.captured_event_ids.len(), 1);
        let after = f.stored_state();
        assert_eq!(after["checkpoint"], before["checkpoint"]);
        assert_eq!(after["session_revision"], before["session_revision"]);
        assert_eq!(after["memories"], before["memories"]);
        assert_eq!(
            f.store
                .conn
                .query_row("SELECT count(*) FROM observations", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            f.store
                .conn
                .query_row(
                    "SELECT processing FROM learning_events WHERE event_id=?1",
                    [&semantic_input],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            "pending"
        );
        assert_eq!(
            after["events"].as_array().unwrap().len(),
            before["events"].as_array().unwrap().len() + 1
        );
        assert_eq!(
            f.store
                .conn
                .query_row(
                    "SELECT count(*) FROM operation_receipts WHERE request_id='bad-delivery'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
    }
}

#[test]
fn valid_delivery_origin_unknown_task_and_other_context_roles() {
    for origin in ["host_observed", "model_reported"] {
        let f = Fixture::new();
        let proposal = f.capture("proposal", Speaker::Tutor, Some(Delivery::Proposed), origin);
        let delivery = f.capture("delivery", Speaker::Tutor, Some(Delivery::Observed), origin);
        let learner_context = f.capture("learner-context", Speaker::Learner, None, "host_observed");
        let task = json!({"proposal_event_ref":proposal,"conditions":"Use learner supplied work as context","observed_delivery_refs":[delivery],"help_refs":[learner_context],"reference_answer_ref":learner_context});
        f.checkpoint(json!([]), task.clone(), "valid-task", 1)
            .unwrap();
        let packet = f
            .read(json!({"action":"read","refs":[{"kind":"event","id":delivery}]}))
            .unwrap();
        assert_eq!(packet["records"][0]["origin"], origin);
        assert_eq!(packet["records"][0]["metadata"]["delivery"], "observed");
        let pause = f.update(json!({"request_id":"pause","session":{"operation":"pause","session_id":f.session,"expected_revision":2}})).unwrap();
        f.update(json!({"request_id":"resume","session":{"operation":"resume","session_id":f.session,"expected_revision":pause.revisions[&f.session]}})).unwrap();
        assert_eq!(
            f.read(json!({"action":"status","session_id":f.session}))
                .unwrap()["session"]["checkpoint"]["pending_task"],
            task
        );
        let unknown = f.capture("unknown", Speaker::Tutor, Some(Delivery::Unknown), origin);
        let task = json!({"proposal_event_ref":unknown,"conditions":"Delivery unestablished","observed_delivery_refs":[],"help_refs":[],"reference_answer_ref":null});
        f.checkpoint(json!([]), task.clone(), "unknown-task", 4)
            .unwrap();
        assert_eq!(
            f.read(json!({"action":"status","session_id":f.session}))
                .unwrap()["session"]["checkpoint"]["pending_task"],
            task
        );
    }
}

#[test]
fn checkpoint_basis_shares_session_read_snapshot() {
    use crate::request::{OperationCancellation, enter_request_cancellation};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let f = Fixture::new();
    f.store
        .conn
        .execute_batch("PRAGMA journal_mode=WAL")
        .unwrap();
    let (memory, observation) = f.memory("snapshot-work");
    f.checkpoint(
        json!([{"memory_id":memory,"revision":1}]),
        Value::Null,
        "snapshot-checkpoint",
        1,
    )
    .unwrap();
    let path = f.path.clone();
    let host = f.host.clone();
    let changed = Arc::new(AtomicBool::new(false));
    let changed_hook = changed.clone();
    let cited_memory = memory.clone();
    let cancellation = OperationCancellation::none().with_test_hook(Arc::new(move |name| {
        if name == "learner_snapshot_open" && !changed_hook.swap(true, Ordering::SeqCst) {
            let writer = LearnerStore::open(&path, "checkpoint-attribution").unwrap();
            writer.update(&host, json!({"schema":SCHEMA,"action":"update","request_id":"concurrent-change","memory_edits":[{"operation":"flag","memory_id":cited_memory,"expected_revision":1,"review_note":"Concurrent unresolved review","reason":"Recorded question","change_basis":[{"observation_id":observation}]}]})).unwrap();
        }
    }));
    let scope = enter_request_cancellation(&cancellation);
    let packet = f
        .read(json!({"action":"status","session_id":f.session}))
        .unwrap();
    drop(scope);
    assert!(changed.load(Ordering::SeqCst));
    assert_eq!(packet["session"]["revision"], 2);
    assert_eq!(packet["checkpoint_basis"]["status"], "unchanged");
    assert_eq!(
        packet["checkpoint_basis"]["memories"][0]["current_revision"],
        1
    );
    let later = f
        .read(json!({"action":"status","session_id":f.session}))
        .unwrap();
    assert_eq!(
        later["checkpoint_basis"]["memories"][0]["current_revision"],
        2
    );
    assert_eq!(later["checkpoint_basis"]["status"], "needs_reconsideration");
}

#[test]
fn checkpoint_decode_is_shared_strict_and_bounded() {
    let f = Fixture::new();
    let (memory, _) = f.memory("decoder-work");
    for checkpoint in [
        json!({"working_diagnosis":"Missing required fields"}),
        json!({"current_problem":"p","working_diagnosis":"d","next_action":"n","unexpected":true}),
        json!({"current_problem":"p","working_diagnosis":"d","next_action":"n","memory_refs":[{"memory_id":memory,"revision":"after_edit"}]}),
        json!({"current_problem":"p","working_diagnosis":"d","next_action":"n","memory_refs":[{"memory_id":memory,"revision":0}]}),
        json!({"current_problem":"p","working_diagnosis":"d","next_action":"n","memory_refs":vec![json!({"memory_id":memory,"revision":1});17]}),
    ] {
        f.store
            .conn
            .execute(
                "UPDATE learning_sessions SET checkpoint_json=?2 WHERE session_id=?1",
                rusqlite::params![f.session, checkpoint.to_string()],
            )
            .unwrap();
        let before = f.stored_state();
        let error = f
            .read(json!({"action":"status","session_id":f.session}))
            .unwrap_err();
        assert!(error.contains("stored_data_invalid"), "{error}");
        assert_eq!(f.stored_state(), before);
        assert!(records::StoredCheckpoint::decode(&checkpoint.to_string(), &f.session).is_err());
    }
    assert!(records::StoredCheckpoint::decode("{}", &f.session).is_ok());
}
