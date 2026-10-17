use super::*;
use serde_json::{Value, json};
struct Fixture {
    store: LearnerStore,
    host: HostContext,
    path: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "magnesium-{}.sqlite",
            store::new_id("test").unwrap()
        ));
        let store = LearnerStore::initialize(&path, "workspace-test").unwrap();
        Self {
            store,
            host: HostContext {
                host_kind: "test".into(),
                host_thread_id: "monday".into(),
            },
            path,
        }
    }
    fn start(&self) -> UpdateResult {
        self.store.update(&self.host,json!({"schema":SCHEMA,"action":"update","request_id":"start","session":{"operation":"start","client_ref":"session","goal":"algebra"}})).unwrap()
    }
    fn event(&self, key: &str, text: &str, time: Option<i64>) -> String {
        self.store
            .capture(
                &self.host,
                &[RawEvent {
                    external_event_key: key.into(),
                    speaker: crate::learner::Speaker::Learner,
                    kind: "message".into(),
                    text: text.into(),
                    metadata: Some(crate::learner::EventMetadata::default()),
                    occurred_at_ms: time,
                }],
                "host_observed",
                false,
            )
            .unwrap()
            .remove(0)
    }
    fn learning(&self, key: &str, event: &str) -> Value {
        json!({"schema":SCHEMA,"action":"update","request_id":key,"turn_ref":event,"observations":[{"client_ref":"work","kind":"learner_question","description":"Can apply formula; asks which coefficient b denotes.","events":[{"event_id":event,"role":"learner_response","excerpts":[]}]}],"memory_edits":[{"operation":"create","client_ref":"memory","record":{"title":"Coefficient roles","scope":"algebra","summary":"Formula application works; coefficient roles uncertain.","state_text":"Correct formula use does not establish understanding of fixed coefficient labels.","current_basis":[{"new_observation":"work"}],"topics":["Algebra"],"retrieval_cues":["which letter"]},"reason":"Natural learner question","change_basis":[{"new_observation":"work"}]}],"event_dispositions":[{"event_id":event,"outcome":"processed"}]})
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
#[test]
fn bundled_schema_roundtrip_and_retry() {
    let f = Fixture::new();
    f.start();
    let e = f.event("1", "Is b another unknown?", Some(100));
    let payload = f.learning("write", &e);
    let first = f.store.update(&f.host, payload.clone()).unwrap();
    let replay = f.store.update(&f.host, payload.clone()).unwrap();
    assert_eq!(first.references, replay.references);
    let memory = &first.references["memory"];
    let row = update::memory_snapshot(&f.store.conn, memory).unwrap();
    assert_eq!(row["last_observed_at_ms"], 100);
    assert_eq!(row["topics"], json!(["algebra"]));
    assert_eq!(row["revision"], 1);
    let mut lost = payload;
    lost["request_id"] = json!("lost-receipt");
    assert_eq!(
        f.store.update(&f.host, lost).unwrap_err().code,
        "events_already_processed"
    );
    let count: i64 = f
        .store
        .conn
        .query_row("SELECT count(*) FROM observations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
}
#[test]
fn raw_capture_survives_invalid_semantic_shape() {
    let f = Fixture::new();
    f.start();
    let result=f.store.update(&f.host,json!({"schema":SCHEMA,"action":"update","request_id":"bad","reported_events":[{"external_event_key":"pending","speaker":"learner","kind":"message","text":"x² − 4 = 0"}],"memory_edits":[{"operation":"create","unexpected":true}]}));
    let err = result.unwrap_err();
    assert_eq!(err.captured_event_ids.len(), 1);
    let text: String = f
        .store
        .conn
        .query_row(
            "SELECT text FROM learning_events WHERE processing='pending'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(text, "x² − 4 = 0");
    assert_eq!(
        f.store
            .conn
            .query_row("SELECT count(*) FROM operation_receipts", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}
#[test]
fn capture_deduplicates_external_identity_not_text() {
    let f = Fixture::new();
    f.start();
    let first = f.event("a", "same", None);
    assert_eq!(first, f.event("a", "same", None));
    assert_ne!(first, f.event("b", "same", None));
    let err = f
        .store
        .capture(
            &f.host,
            &[RawEvent {
                external_event_key: "a".into(),
                speaker: crate::learner::Speaker::Learner,
                kind: "message".into(),
                text: "changed".into(),
                metadata: Some(crate::learner::EventMetadata::default()),
                occurred_at_ms: None,
            }],
            "host_observed",
            false,
        )
        .unwrap_err();
    assert_eq!(err.code, "event_key_conflict");
}
#[test]
fn failed_fts_commits_canonical_and_marks_dirty() {
    let f = Fixture::new();
    f.start();
    f.store.conn.execute_batch("DROP TABLE memory_fts").unwrap();
    let e = f.event("a", "question", None);
    let result = f.store.update(&f.host, f.learning("write", &e)).unwrap();
    assert_eq!(result.index_status, "dirty");
    assert!(update::memory_snapshot(&f.store.conn, &result.references["memory"]).is_ok());
    let generations: (i64, i64) = f
        .store
        .conn
        .query_row(
            "SELECT memory_generation,memory_fts_generation FROM workspace_meta",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_ne!(generations.0, generations.1);
}
#[test]
fn conflict_rolls_back_observations_and_keeps_capture() {
    let f = Fixture::new();
    f.start();
    let e = f.event("a", "question", Some(100));
    let first = f.store.update(&f.host, f.learning("write", &e)).unwrap();
    let memory = &first.references["memory"];
    let e2 = f.event("b", "new answer", Some(200));
    let mut payload = f.learning("conflict", &e2);
    payload["memory_edits"][0] = json!({"operation":"flag","memory_id":memory,"expected_revision":0,"review_note":"reconsider","reason":"challenge","change_basis":[{"event_id":e2}]});
    assert_eq!(
        f.store.update(&f.host, payload).unwrap_err().code,
        "revision_conflict"
    );
    assert_eq!(
        f.store
            .conn
            .query_row("SELECT count(*) FROM observations", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        f.store
            .conn
            .query_row(
                "SELECT processing FROM learning_events WHERE event_id=?1",
                [e2],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "pending"
    );
}
#[test]
fn recording_control_does_not_depend_on_semantic_success() {
    let f = Fixture::new();
    f.start();
    let control = RawEvent {
        external_event_key: "off".into(),
        speaker: crate::learner::Speaker::Learner,
        kind: "message".into(),
        text: "Stop recording".into(),
        metadata: Some(crate::learner::EventMetadata::default()),
        occurred_at_ms: None,
    };
    f.store
        .control(&f.host, "off", "recording_off", &control)
        .unwrap();
    assert_eq!(
        f.store
            .update(
                &f.host,
                json!({"schema":SCHEMA,"action":"update","request_id":"blocked"})
            )
            .unwrap_err()
            .code,
        "recording_disabled"
    );
    assert!(!f.store.binding(&f.host).unwrap().2);
    assert_eq!(
        f.store
            .conn
            .query_row("SELECT count(*) FROM learning_events", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}
#[test]
fn unknown_store_and_workspace_rejected_without_mutation() {
    let f = Fixture::new();
    assert_eq!(
        LearnerStore::open(&f.path, "wrong").err().unwrap().code,
        "workspace_mismatch"
    );
    f.store
        .conn
        .execute_batch(
            "UPDATE workspace_meta SET schema_version=2; CREATE TABLE unrelated(value TEXT)",
        )
        .unwrap();
    let count: i64 = f
        .store
        .conn
        .query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))
        .unwrap();
    assert!(LearnerStore::open(&f.path, "wrong").is_err());
    assert_eq!(
        count,
        f.store
            .conn
            .query_row("SELECT count(*) FROM sqlite_master", [], |r| r
                .get::<_, i64>(0))
            .unwrap()
    );
}
#[test]
fn new_conversation_revises_same_memory_and_editorial_keeps_recency() {
    let f = Fixture::new();
    let start = f.start();
    let e = f.event("monday", "I can apply the formula", Some(100));
    let first = f
        .store
        .update(&f.host, f.learning("monday-write", &e))
        .unwrap();
    let memory = first.references["memory"].clone();
    let observation = first.references["work"].clone();
    let tuesday = HostContext {
        host_kind: "test".into(),
        host_thread_id: "tuesday".into(),
    };
    f.store.update(&tuesday,json!({"schema":SCHEMA,"action":"update","request_id":"resume","session":{"operation":"resume","session_id":start.references["session"],"expected_revision":1}})).unwrap();
    let revision = json!({"schema":SCHEMA,"action":"update","request_id":"editorial","memory_edits":[{"operation":"revise","memory_id":memory,"expected_revision":1,"change_kind":"editorial","record":{"title":"Coefficient roles clarified","scope":"algebra","summary":"Formula application works; understanding of b remains uncertain.","state_text":"Earlier formula application was successful; it does not establish ability to distinguish fixed coefficients from the unknown.","current_basis":[{"observation_id":observation}]},"reason":"Clarify scope without new performance","change_basis":[{"observation_id":observation}]}]});
    let second = f.store.update(&tuesday, revision).unwrap();
    assert_eq!(second.revisions[&memory], 2);
    let current = update::memory_snapshot(&f.store.conn, &memory).unwrap();
    assert_eq!(current["last_observed_at_ms"], 100);
    assert_eq!(current["topics"], json!(["algebra"]));
    assert_eq!(
        f.store
            .conn
            .query_row(
                "SELECT count(*) FROM memory_changes WHERE memory_id=?1",
                [&memory],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        2
    );
    let flag = json!({"schema":SCHEMA,"action":"update","request_id":"flag","memory_edits":[{"operation":"flag","memory_id":memory,"expected_revision":2,"review_note":"Learner disputes diagnosis","reason":"Reconsider against original work","change_basis":[{"observation_id":observation}]}]});
    f.store.update(&tuesday, flag).unwrap();
    let mut invalid = json!({"schema":SCHEMA,"action":"update","request_id":"clear","memory_edits":[{"operation":"revise","memory_id":memory,"expected_revision":3,"change_kind":"reinterpretation","record":{"title":"Corrected","scope":"algebra","summary":"Original work shows successful coefficient identification.","state_text":"Previous interpretation was mistaken.","current_basis":[{"observation_id":observation}]},"reason":"Original work correction","change_basis":[{"observation_id":observation}]}]});
    assert_eq!(
        f.store.update(&tuesday, invalid.clone()).unwrap_err().code,
        "invalid_variant"
    );
    invalid["memory_edits"][0]["review_resolution"] =
        json!("Checked original response; dispute was valid");
    f.store.update(&tuesday, invalid).unwrap();
    assert_eq!(
        update::memory_snapshot(&f.store.conn, &memory).unwrap()["last_observed_at_ms"],
        100
    );
}
#[test]
fn exact_excerpts_preserve_negation_and_unicode() {
    let f = Fixture::new();
    f.start();
    let e = f.event("a", "I cannot solve x² − 1 = 0", None);
    let mut payload = f.learning("bad-excerpt", &e);
    payload["observations"][0]["events"][0]["excerpts"] = json!(["I can solve x² − 1 = 0"]);
    assert_eq!(
        f.store.update(&f.host, payload).unwrap_err().code,
        "excerpt_mismatch"
    );
}
#[test]
fn canonical_receipt_hash_ignores_object_key_order_and_rejects_changed_payload() {
    let f = Fixture::new();
    let start = f.start();
    let payload = json!({"request_id":"start","action":"update","schema":SCHEMA,"session":{"goal":"algebra","client_ref":"session","operation":"start"}});
    assert_eq!(
        f.store.update(&f.host, payload.clone()).unwrap().references,
        start.references
    );
    let mut changed = payload;
    changed["session"]["goal"] = json!("geometry");
    assert_eq!(
        f.store.update(&f.host, changed).unwrap_err().code,
        "request_key_conflict"
    );
}
#[test]
fn receipts_cannot_be_replayed_from_another_host() {
    let f = Fixture::new();
    f.start();
    let other = HostContext {
        host_kind: "test".into(),
        host_thread_id: "another-thread".into(),
    };
    let request = json!({"schema":SCHEMA,"action":"update","request_id":"start","session":{"operation":"start","client_ref":"session","goal":"algebra"}});
    assert_eq!(
        f.store.update(&other, request).unwrap_err().code,
        "request_key_conflict"
    );
    assert!(!f.store.binding(&other).unwrap().1);
}
#[test]
fn malformed_start_preserves_raw_but_does_not_activate_learning() {
    let f = Fixture::new();
    let payload = json!({"schema":SCHEMA,"action":"update","request_id":"bad-start","session":{"operation":"start","client_ref":"session","goal":"algebra"},"reported_events":[{"external_event_key":"start-turn","speaker":"learner","kind":"message","text":"Teach me algebra"}],"memory_edits":[{"operation":"flag","memory_id":"missing","expected_revision":1,"review_note":"bad","reason":"bad","change_basis":[]}]});
    let error = f.store.update(&f.host, payload).unwrap_err();
    assert_eq!(error.captured_event_ids.len(), 1);
    assert!(!f.store.binding(&f.host).unwrap().1);
    assert_eq!(
        f.store
            .conn
            .query_row("SELECT count(*) FROM learning_sessions", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}
#[test]
fn metadata_cannot_be_an_untyped_profile_or_spoof_origin() {
    let f = Fixture::new();
    f.start();
    for metadata in [json!({"mastery":1.0}), json!({"origin":"host_observed"})] {
        let request = json!({"schema":SCHEMA,"action":"update","request_id":"bad-metadata","reported_events":[{"external_event_key":"event","speaker":"learner","kind":"message","text":"answer","metadata":metadata}]});
        assert_eq!(
            f.store.update(&f.host, request).unwrap_err().code,
            "invalid_variant"
        );
    }
}
#[test]
fn atomic_local_replacement_checkpoint_and_review_keep_original_support() {
    let f = Fixture::new();
    let start = f.start();
    let e = f.event("a", "Formula work", Some(100));
    let first = f.store.update(&f.host, f.learning("initial", &e)).unwrap();
    let old = &first.references["memory"];
    let observation = &first.references["work"];
    let request = json!({"schema":SCHEMA,"action":"update","request_id":"replace","session":{"operation":"use","session_id":start.references["session"],"expected_revision":1},"memory_edits":[{"operation":"retire","memory_id":old,"expected_revision":1,"reason":"Replace with a clearer description","replacement_ids":["replacement"]},{"operation":"create","client_ref":"replacement","record":{"title":"Symbol roles","scope":"algebra","summary":"Preserve formula strength while clarifying coefficient uncertainty.","state_text":"The original work supports successful formula application, with uncertainty about labels.","current_basis":[{"observation_id":observation}]},"reason":"Local descriptive reorganization","change_basis":[{"observation_id":observation}]}],"checkpoint":{"current_problem":"Explain b","working_diagnosis":"Distinguish fixed labels","next_action":"Contrast b and x","memory_refs":[{"memory_id":"replacement","revision":"after_edit"}]},"review_updates":[{"operation":"create","client_ref":"review","memory_id":"replacement","purpose":"Revisit labels","reason":"Useful next teaching example"}]});
    let result = f.store.update(&f.host, request).unwrap();
    let replacement = &result.references["replacement"];
    assert_eq!(
        update::memory_snapshot(&f.store.conn, old).unwrap()["replacement_ids"],
        json!([replacement])
    );
    assert_eq!(
        update::memory_snapshot(&f.store.conn, old).unwrap()["lifecycle"],
        "retired"
    );
    assert_eq!(
        update::memory_snapshot(&f.store.conn, replacement).unwrap()["last_observed_at_ms"],
        100
    );
    let checkpoint: String = f
        .store
        .conn
        .query_row(
            "SELECT checkpoint_json FROM learning_sessions WHERE session_id=?1",
            [&start.references["session"]],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&checkpoint).unwrap()["memory_refs"][0]["memory_id"],
        replacement.as_str()
    );
    assert!(result.references.contains_key("review"));
    assert_eq!(
        f.store
            .conn
            .query_row("SELECT count(*) FROM observations", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}
#[test]
fn editorial_cannot_attach_new_performance() {
    let f = Fixture::new();
    f.start();
    let first_event = f.event("a", "Monday work", Some(100));
    let initial = f
        .store
        .update(&f.host, f.learning("initial", &first_event))
        .unwrap();
    let event = f.event("b", "Tuesday work", Some(200));
    let mut request = f.learning("editorial-new-work", &event);
    let mut edit = request["memory_edits"][0].clone();
    edit["operation"] = json!("revise");
    edit.as_object_mut().unwrap().remove("client_ref");
    edit["memory_id"] = json!(initial.references["memory"]);
    edit["expected_revision"] = json!(1);
    edit["change_kind"] = json!("editorial");
    request["memory_edits"][0] = edit;
    assert_eq!(
        f.store.update(&f.host, request).unwrap_err().code,
        "invalid_variant"
    );
    assert_eq!(
        update::memory_snapshot(&f.store.conn, &initial.references["memory"]).unwrap()["last_observed_at_ms"],
        100
    );
    assert_eq!(
        f.store
            .conn
            .query_row("SELECT count(*) FROM observations", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}
#[test]
fn paused_session_requires_explicit_resume() {
    let f = Fixture::new();
    let start = f.start();
    let session = &start.references["session"];
    let pause = json!({"schema":SCHEMA,"action":"update","request_id":"pause","session":{"operation":"pause","session_id":session,"expected_revision":1}});
    f.store.update(&f.host, pause.clone()).unwrap();
    assert!(f.store.update(&f.host, pause).is_ok());
    let other = HostContext {
        host_kind: "test".into(),
        host_thread_id: "other".into(),
    };
    f.store.update(&other,json!({"schema":SCHEMA,"action":"update","request_id":"other-start","session":{"operation":"start","client_ref":"other","goal":"another goal"}})).unwrap();
    let use_request = json!({"schema":SCHEMA,"action":"update","request_id":"use-paused","session":{"operation":"use","session_id":session,"expected_revision":2}});
    assert_eq!(
        f.store.update(&other, use_request).unwrap_err().code,
        "invalid_variant"
    );
}
#[test]
fn raw_event_and_client_reference_bounds_keep_receipts_readable() {
    let f = Fixture::new();
    f.start();
    let event =
        json!({"external_event_key":"key","speaker":"learner","kind":"message","text":"answer"});
    let oversized = json!({"schema":SCHEMA,"action":"update","request_id":"too-many","reported_events":vec![event;33]});
    assert_eq!(
        f.store.update(&f.host, oversized).unwrap_err().code,
        "field_too_large"
    );
    let request = json!({"schema":SCHEMA,"action":"update","request_id":"long-ref","session":{"operation":"start","client_ref":"x".repeat(257),"goal":"algebra"}});
    assert_eq!(
        f.store.update(&f.host, request).unwrap_err().code,
        "field_too_large"
    );
    let message = "π".repeat(100000);
    let error = LearnerError::new("unknown_reference", message);
    assert!(serde_json::to_vec(&error).unwrap().len() < 4096);
    assert!(error.message.ends_with("[error detail truncated]"));
}
#[test]
fn enormous_support_excerpt_is_rejected_without_losing_original() {
    let f = Fixture::new();
    f.start();
    let text = "a".repeat(17000);
    let event = f.event("long", &text, None);
    let mut request = f.learning("oversize-excerpt", &event);
    request["observations"][0]["events"][0]["excerpts"] = json!([text]);
    assert_eq!(
        f.store.update(&f.host, request).unwrap_err().code,
        "field_too_large"
    );
    let length: i64 = f
        .store
        .conn
        .query_row(
            "SELECT length(text) FROM learning_events WHERE event_id=?1",
            [event],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(length, 17000);
}
fn native_start_payload(key: &str, text: &str) -> Value {
    json!({"schema":SCHEMA,"action":"update","request_id":key,"session":{"operation":"start","client_ref":"session","goal":"algebra"},"native_capture":{"ticket":"validated-by-adapter-in-this-unit-test","text":text}})
}
fn trusted_native(key: &str, text: &str) -> RawEvent {
    RawEvent {
        external_event_key: key.into(),
        speaker: crate::learner::Speaker::Learner,
        kind: "message".into(),
        text: text.into(),
        metadata: Some(crate::learner::EventMetadata::default()),
        occurred_at_ms: None,
    }
}
#[test]
fn transient_native_capture_survives_bad_semantics_without_activating_binding() {
    let f = Fixture::new();
    let native = trusted_native("native:turn", "Teach me why b is fixed");
    let mut request = native_start_payload("bad-native-start", &native.text);
    request["memory_edits"] = json!([{"operation":"invalid"}]);
    let error = f
        .store
        .update_with_native_capture(&f.host, request, Some(&native))
        .unwrap_err();
    assert_eq!(error.captured_event_ids.len(), 1);
    assert!(!f.store.binding(&f.host).unwrap().1);
    let record: (String, String, Option<i64>) = f
        .store
        .conn
        .query_row(
            "SELECT origin,text,occurred_at_ms FROM learning_events",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(record, ("host_observed".into(), native.text, None));
    assert_eq!(
        f.store
            .conn
            .query_row("SELECT count(*) FROM operation_receipts", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}
#[test]
fn transient_native_success_replay_retains_raw_identity_and_original_request_hash() {
    let f = Fixture::new();
    let native = trusted_native("native:turn", "Teach me quadratic coefficients");
    let request = native_start_payload("native-start", &native.text);
    let first = f
        .store
        .update_with_native_capture(&f.host, request.clone(), Some(&native))
        .unwrap();
    assert_eq!(first.captured_event_ids.len(), 1);
    let replay = f.store.replay(&f.host, &request).unwrap().unwrap();
    assert_eq!(first.captured_event_ids, replay.captured_event_ids);
    assert_eq!(
        f.store
            .update(&f.host, request.clone())
            .unwrap()
            .captured_event_ids,
        first.captured_event_ids
    );
    let mut changed = request;
    changed["native_capture"]["ticket"] = json!("changed-proof");
    assert_eq!(
        f.store.replay(&f.host, &changed).unwrap_err().code,
        "request_key_conflict"
    );
    assert_eq!(
        f.store
            .conn
            .query_row("SELECT count(*) FROM learning_events", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}
#[test]
fn transient_native_requires_trusted_adapter_and_obeys_recording_off() {
    let f = Fixture::new();
    let native = trusted_native("native:turn", "Teach me algebra");
    let request = native_start_payload("native-start", &native.text);
    assert_eq!(
        f.store.update(&f.host, request.clone()).unwrap_err().code,
        "invalid_variant"
    );
    f.store
        .control(&f.host, "off", "recording_off", &native)
        .unwrap();
    assert_eq!(
        f.store
            .update_with_native_capture(&f.host, request, Some(&native))
            .unwrap_err()
            .code,
        "recording_disabled"
    );
    assert_eq!(
        f.store
            .conn
            .query_row("SELECT count(*) FROM learning_events", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}
#[test]
fn verified_native_survives_malformed_fallback_capture_envelope() {
    let f = Fixture::new();
    let native = trusted_native("native:turn", "Native text stays pending");
    let mut request = native_start_payload("native-bad-fallback", &native.text);
    request["reported_events"] = json!([{"not_an_event":true}]);
    let error = f
        .store
        .update_with_native_capture(&f.host, request, Some(&native))
        .unwrap_err();
    assert_eq!(error.captured_event_ids.len(), 1);
    assert_eq!(
        f.store
            .conn
            .query_row("SELECT processing FROM learning_events", [], |row| row
                .get::<_, String>(
                0
            ))
            .unwrap(),
        "pending"
    );
}
#[test]
fn one_native_first_message_cannot_start_two_sessions_with_different_request_keys() {
    let f = Fixture::new();
    let native = trusted_native("native:turn", "Teach me algebra");
    let request = native_start_payload("first-start", &native.text);
    let first = f
        .store
        .update_with_native_capture(&f.host, request.clone(), Some(&native))
        .unwrap();
    let mut retry = request;
    retry["request_id"] = json!("lost-response-new-key");
    let error = f
        .store
        .update_with_native_capture(&f.host, retry, Some(&native))
        .unwrap_err();
    assert_eq!(error.code, "events_already_processed");
    assert_eq!(error.captured_event_ids, first.captured_event_ids);
    assert_eq!(
        f.store
            .conn
            .query_row("SELECT count(*) FROM learning_sessions", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}
#[test]
fn session_only_update_reports_missing_index_as_dirty() {
    let f = Fixture::new();
    f.store.conn.execute_batch("DROP TABLE memory_fts").unwrap();
    assert_eq!(f.start().index_status, "dirty");
}
#[test]
fn two_real_connections_reconcile_one_revision_without_overwrite() {
    use std::sync::{Arc, Barrier};
    let f = Fixture::new();
    f.start();
    let event = f.event("initial", "Coefficient question", Some(100));
    let initial = f
        .store
        .update(&f.host, f.learning("initial", &event))
        .unwrap();
    let memory = initial.references["memory"].clone();
    let observation = initial.references["work"].clone();
    let barrier = Arc::new(Barrier::new(2));
    let mut workers = vec![];
    for n in 0..2 {
        let path = f.path.clone();
        let memory = memory.clone();
        let observation = observation.clone();
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || {let store=LearnerStore::open(&path,"workspace-test").unwrap();let host=HostContext{host_kind:"test".into(),host_thread_id:format!("concurrent-{n}")};store.update(&host,json!({"schema":SCHEMA,"action":"update","request_id":format!("start-{n}"),"session":{"operation":"start","client_ref":"session","goal":"algebra"}})).unwrap();barrier.wait();store.update(&host,json!({"schema":SCHEMA,"action":"update","request_id":format!("flag-{n}"),"memory_edits":[{"operation":"flag","memory_id":memory,"expected_revision":1,"review_note":format!("Reconsider from conversation {n}"),"reason":"Review original work","change_basis":[{"observation_id":observation}]}]})).map(|r|r.update_status).map_err(|e|e.code)}));
    }
    let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r,Err(code) if code=="revision_conflict"))
            .count(),
        1
    );
    assert_eq!(
        update::memory_snapshot(&f.store.conn, &memory).unwrap()["revision"],
        2
    );
    assert_eq!(
        f.store
            .conn
            .query_row(
                "SELECT count(*) FROM memory_changes WHERE memory_id=?1",
                [memory],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        2
    );
}
#[test]
fn failed_new_session_leaves_new_input_unbound_and_resume_attaches_selected_session() {
    let f = Fixture::new();
    let first = f.start();
    let second=f.store.update(&f.host,json!({"schema":SCHEMA,"action":"update","request_id":"second-session","session":{"operation":"start","client_ref":"second","goal":"Another lesson"}})).unwrap();
    let request = json!({"schema":SCHEMA,"action":"update","request_id":"failed-resume","session":{"operation":"resume","session_id":first.references["session"],"expected_revision":99},"reported_events":[{"external_event_key":"resume-turn","speaker":"learner","kind":"message","text":"Return to the earlier lesson"}]});
    let error = f.store.update(&f.host, request.clone()).unwrap_err();
    assert_eq!(error.code, "revision_conflict");
    let raw = &error.captured_event_ids[0];
    let session: Option<String> = f
        .store
        .conn
        .query_row(
            "SELECT session_id FROM learning_events WHERE event_id=?1",
            [raw],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(session, None);
    assert_eq!(
        f.store.binding(&f.host).unwrap().0,
        Some(second.references["second"].clone())
    );
    let mut retry = request;
    retry["request_id"] = json!("valid-resume");
    retry["session"]["expected_revision"] = json!(1);
    f.store.update(&f.host, retry).unwrap();
    assert_eq!(
        f.store
            .conn
            .query_row(
                "SELECT session_id FROM learning_events WHERE event_id=?1",
                [raw],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
        first.references["session"]
    );
}

fn pending_observation_with_help(f: &Fixture) -> (String, String, String) {
    f.start();
    let event = f.event(
        "closing-answer",
        "b=-5 gives roots 2 and 3. Let's stop.",
        None,
    );
    let help = f
        .store
        .capture(
            &f.host,
            &[RawEvent {
                external_event_key: "prior-help".into(),
                speaker: Speaker::Tutor,
                kind: "delivery".into(),
                text: "Use the signed coefficient, including its minus sign.".into(),
                metadata: Some(EventMetadata::default()),
                occurred_at_ms: None,
            }],
            "host_observed",
            false,
        )
        .unwrap()
        .remove(0);
    let saved = f.store.update(&f.host, json!({"schema":SCHEMA,"action":"update","request_id":"observation-only",
        "observations":[{"client_ref":"closing","kind":"learner_work","description":"Used b=-5 after a hint; independence is not established.",
            "assistance_text":"The tutor reminded the learner to include the sign.","events":[{"event_id":event,"role":"learner_response"},{"event_id":help,"role":"assistance"}]}]})).unwrap();
    (event, saved.references["closing"].clone(), help)
}

fn existing_observation_edit(f: &Fixture, event: &str, observation: &str) -> Value {
    let mut request = f.learning("reuse-existing-observation", event);
    request["observations"] = json!([]);
    request["memory_edits"][0]["record"]["current_basis"] = json!([{"observation_id":observation}]);
    request["memory_edits"][0]["change_basis"] = json!([{"observation_id":observation}]);
    request
}

#[test]
fn existing_observation_support_completes_pending_turn_after_stop_without_duplicate_or_lost_help() {
    let f = Fixture::new();
    let (event, observation, help) = pending_observation_with_help(&f);
    let control = RawEvent {
        external_event_key: "stop".into(),
        speaker: Speaker::Learner,
        kind: "message".into(),
        text: "Stop".into(),
        metadata: None,
        occurred_at_ms: None,
    };
    f.store.control(&f.host, "stop", "stop", &control).unwrap();
    let mut request = existing_observation_edit(&f, &event, &observation);
    request["mode"] = json!("complete_captured");
    let first = f.store.update(&f.host, request.clone()).unwrap();
    assert_eq!(first.update_status, "committed");
    assert!(!f.store.binding(&f.host).unwrap().1);
    assert!(f.store.binding(&f.host).unwrap().2);
    let state: (String, String) = f
        .store
        .conn
        .query_row(
            "SELECT processing,processed_request_id FROM learning_events WHERE event_id=?1",
            [&event],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        state,
        ("processed".into(), "reuse-existing-observation".into())
    );
    assert_eq!(
        f.store
            .conn
            .query_row("SELECT count(*) FROM observations", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    let original: (String, String) = f.store.conn.query_row("SELECT e.text,oe.role FROM learning_events e JOIN observation_events oe USING(event_id) WHERE event_id=?1 AND observation_id=?2", rusqlite::params![help, observation], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!(
        original,
        (
            "Use the signed coefficient, including its minus sign.".into(),
            "assistance".into()
        )
    );
    assert_eq!(
        f.store
            .conn
            .query_row(
                "SELECT processing FROM learning_events WHERE event_id=?1",
                [&help],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "not_applicable"
    );
    let replay = f.store.update(&f.host, request.clone()).unwrap();
    assert_eq!(
        serde_json::to_value(first).unwrap(),
        serde_json::to_value(replay).unwrap()
    );
    request["request_id"] = json!("duplicate-disposition");
    assert_eq!(
        f.store.update(&f.host, request).unwrap_err().code,
        "event_scope_conflict"
    );
}

#[test]
fn existing_observation_support_allows_explicit_normal_disposition_without_processing_other_support()
 {
    let f = Fixture::new();
    let (event, observation, help) = pending_observation_with_help(&f);
    let request = existing_observation_edit(&f, &event, &observation);
    assert_eq!(
        f.store
            .update(&f.host, request.clone())
            .unwrap()
            .update_status,
        "committed"
    );
    assert_eq!(
        f.store
            .conn
            .query_row(
                "SELECT processing FROM learning_events WHERE event_id=?1",
                [&event],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "processed"
    );
    assert_eq!(
        f.store
            .conn
            .query_row(
                "SELECT processing FROM learning_events WHERE event_id=?1",
                [&help],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "not_applicable"
    );
    let mut repeated = request;
    repeated["request_id"] = json!("second-disposition");
    assert_eq!(
        f.store.update(&f.host, repeated).unwrap_err().code,
        "events_already_processed"
    );
}

#[test]
fn existing_observation_completion_still_rejects_unrelated_support_and_recording_off() {
    for variant in ["unrelated", "recording_off"] {
        let f = Fixture::new();
        let (event, observation, _) = pending_observation_with_help(&f);
        let turn = if variant == "unrelated" {
            f.event("different-answer", "A different closing answer", None)
        } else {
            event.clone()
        };
        let control = RawEvent {
            external_event_key: "stop".into(),
            speaker: Speaker::Learner,
            kind: "message".into(),
            text: "Stop".into(),
            metadata: None,
            occurred_at_ms: None,
        };
        f.store.control(&f.host, "stop", "stop", &control).unwrap();
        if variant == "recording_off" {
            f.store
                .control(&f.host, "off", "recording_off", &control)
                .unwrap();
        }
        let mut request = existing_observation_edit(&f, &turn, &observation);
        request["mode"] = json!("complete_captured");
        let error = f.store.update(&f.host, request).unwrap_err();
        assert_eq!(
            error.code,
            if variant == "recording_off" {
                "recording_disabled"
            } else {
                "invalid_variant"
            }
        );
        assert!(!f.store.binding(&f.host).unwrap().1);
        assert_eq!(
            f.store
                .conn
                .query_row("SELECT count(*) FROM learner_memories", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            f.store
                .conn
                .query_row("SELECT count(*) FROM observations", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            f.store
                .conn
                .query_row(
                    "SELECT processing FROM learning_events WHERE event_id=?1",
                    [&event],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            "pending"
        );
    }
}

#[test]
fn cancellation_before_semantics_or_commit_preserves_raw_input_without_semantic_mutations() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    for boundary in ["before_semantic", "before_commit"] {
        let f = Fixture::new();
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let token = crate::request::OperationCancellation::with_flag(cancelled).with_test_hook(
            Arc::new(move |phase| {
                if phase == boundary {
                    flag.store(true, Ordering::SeqCst);
                }
            }),
        );
        let _scope = crate::request::enter_request_cancellation(&token);
        let error = f.store.update(&f.host, json!({"schema":SCHEMA,"action":"update","request_id":"cancelled-start",
            "session":{"operation":"start","client_ref":"session","goal":"Algebra"},
            "reported_events":[{"external_event_key":"pending-raw","speaker":"learner","kind":"message","text":"Please help me identify the signed coefficient."}]})).unwrap_err();
        assert_eq!(error.code, "request_cancelled", "{boundary}: {error}");
        assert_eq!(error.captured_event_ids.len(), 1);
        let raw: (String, String, Option<String>) = f
            .store
            .conn
            .query_row(
                "SELECT text,processing,session_id FROM learning_events WHERE event_id=?1",
                [&error.captured_event_ids[0]],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            raw,
            (
                "Please help me identify the signed coefficient.".into(),
                "pending".into(),
                None
            )
        );
        for table in [
            "learning_sessions",
            "learner_memories",
            "observations",
            "operation_receipts",
            "host_bindings",
        ] {
            assert_eq!(
                f.store
                    .conn
                    .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                        .get::<_, i64>(0))
                    .unwrap(),
                0,
                "{boundary}: {table}"
            );
        }
    }
}

#[test]
fn cancellation_after_commit_returns_and_replays_the_durable_receipt() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let f = Fixture::new();
    f.start();
    let event = f.event("answer", "b is the coefficient, including its sign", None);
    let request = f.learning("commit-before-cancel", &event);
    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = cancelled.clone();
    let token = crate::request::OperationCancellation::with_flag(cancelled).with_test_hook(
        Arc::new(move |phase| {
            if phase == "after_commit" {
                flag.store(true, Ordering::SeqCst);
            }
        }),
    );
    let _scope = crate::request::enter_request_cancellation(&token);
    let first = f.store.update(&f.host, request.clone()).unwrap();
    assert!(token.is_cancelled());
    assert_eq!(first.update_status, "committed");
    let replay = f.store.update(&f.host, request).unwrap();
    assert_eq!(
        serde_json::to_value(first).unwrap(),
        serde_json::to_value(replay).unwrap()
    );
    assert_eq!(
        f.store
            .conn
            .query_row(
                "SELECT processing FROM learning_events WHERE event_id=?1",
                [&event],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "processed"
    );
    for table in ["observations", "learner_memories"] {
        assert_eq!(
            f.store
                .conn
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
    assert!(f.store.binding(&f.host).unwrap().1);
}

#[test]
fn resolved_change_basis_preserves_authored_order_and_never_processes_unrequested_events() {
    let f = Fixture::new();
    let (event, observation, _) = pending_observation_with_help(&f);
    let other = f.event(
        "other-pending",
        "Another answer needing interpretation",
        None,
    );
    let mut request = existing_observation_edit(&f, &event, &observation);
    request["observations"] = json!([{"client_ref":"additional","kind":"learner_work","description":"Two answers remain distinct evidence.",
        "events":[{"event_id":event,"role":"learner_response"},{"event_id":other,"role":"learner_response"}]}]);
    request["memory_edits"][0]["change_basis"] =
        json!([{"new_observation":"additional"},{"event_id":event},{"observation_id":observation}]);
    let saved = f.store.update(&f.host, request).unwrap();
    let basis: String = f
        .store
        .conn
        .query_row(
            "SELECT change_basis_json FROM memory_changes WHERE memory_id=?1",
            [&saved.references["memory"]],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&basis).unwrap(),
        json!([saved.references["additional"], event, observation])
    );
    assert_eq!(
        f.store
            .conn
            .query_row(
                "SELECT processing FROM learning_events WHERE event_id=?1",
                [&other],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "pending"
    );
    // Reusing only the stored observation also retains that pending support;
    // participation authorizes an explicit disposition but never creates one.
    let mut reuse = existing_observation_edit(&f, &other, &saved.references["additional"]);
    reuse["request_id"] = json!("reuse-without-disposition");
    reuse["event_dispositions"] = json!([]);
    f.store.update(&f.host, reuse).unwrap();
    assert_eq!(
        f.store
            .conn
            .query_row(
                "SELECT processing FROM learning_events WHERE event_id=?1",
                [&other],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "pending"
    );
    assert_eq!(
        f.store
            .conn
            .query_row("SELECT count(*) FROM observations", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
}
