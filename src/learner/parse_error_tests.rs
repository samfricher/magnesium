//! Typed decoder paths must not weaken decoding or lose independent raw capture.
use super::*;
use serde_json::{Value, json};
struct Fixture {
    store: LearnerStore,
    host: HostContext,
    path: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("{}.sqlite", store::new_id("parse-shape").unwrap()));
        let store = LearnerStore::initialize(&path, "parse-workspace").unwrap();
        let host = HostContext {
            host_kind: "codex".into(),
            host_thread_id: "shape-thread".into(),
        };
        store.update(&host,json!({"schema":SCHEMA,"action":"update","request_id":"start","session":{"operation":"start","client_ref":"session","goal":"algebra"}})).unwrap();
        Self { store, host, path }
    }
    fn reject(&self, key: &str, section: &str, value: Value, path: &str) -> LearnerError {
        let mut payload = json!({"schema":SCHEMA,"action":"update","request_id":key,"reported_events":[{"external_event_key":key,"speaker":"learner","kind":"message","text":"I do not understand which letter b represents.","metadata":{}}]});
        payload[section] = value;
        let error = self.store.update(&self.host, payload).unwrap_err();
        assert_eq!(error.code, "invalid_variant");
        assert_eq!(
            error.recovery.as_ref().unwrap()["field_path"],
            path,
            "{error:?}"
        );
        assert!(error.message.contains(path));
        assert_eq!(
            error.recovery.as_ref().unwrap()["arguments"]["operation"],
            "tool_recipes"
        );
        assert_eq!(error.captured_event_ids.len(), 1);
        let pending: (String, String) = self
            .store
            .conn
            .query_row(
                "SELECT processing,text FROM learning_events WHERE event_id=?1",
                [&error.captured_event_ids[0]],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            pending,
            (
                "pending".into(),
                "I do not understand which letter b represents.".into()
            )
        );
        for table in ["observations", "learner_memories", "memory_changes"] {
            assert_eq!(
                self.store
                    .conn
                    .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                        .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
        assert_eq!(
            self.store
                .conn
                .query_row("SELECT count(*) FROM operation_receipts", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        error
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
fn edit() -> Value {
    json!({"operation":"create","client_ref":"memory","record":{"title":"Coefficient labels","scope":"algebra","summary":"Fixed labels uncertain.","state_text":"Substitution strength does not establish fixed-symbol understanding.","current_basis":[{"new_observation":"work"}]},"reason":"Natural question","change_basis":[{"new_observation":"work"}]})
}
#[test]
fn missing_start_reference_identifies_session_field_and_keeps_capture() {
    let f = Fixture::new();
    let e = f.reject(
        "missing-client",
        "session",
        json!({"operation":"start","goal":"algebra"}),
        "session",
    );
    assert!(e.message.contains("client_ref"));
    assert_eq!(
        e.recovery.unwrap()["arguments"],
        json!({"action":"instructions","stage":"interpret_and_repair","operation":"tool_recipes","host":"codex"})
    );
    assert_eq!(
        f.store
            .conn
            .query_row("SELECT count(*) FROM learning_sessions", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}
#[test]
fn observation_aliases_are_rejected_with_exact_indexed_paths_and_real_field_names() {
    let f = Fixture::new();
    for alias in ["event_ids", "summary", "conditions", "assistance"] {
        let mut o = json!({"client_ref":"work","kind":"learner_question","description":"Coefficient question","events":[]});
        o[alias] = if alias == "event_ids" {
            json!(["e-unknown"])
        } else {
            json!("model supplied text")
        };
        let e = f.reject(
            alias,
            "observations",
            json!([o]),
            &format!("observations[0].{alias}"),
        );
        let expected = e.message;
        for required in [
            "description",
            "events",
            "conditions_text",
            "assistance_text",
        ] {
            assert!(expected.contains(required));
        }
    }
}
#[test]
fn flat_memory_create_requires_nested_record_instead_of_translation() {
    let f = Fixture::new();
    let mut flat = edit();
    let record = flat.as_object_mut().unwrap().remove("record").unwrap();
    flat.as_object_mut()
        .unwrap()
        .extend(record.as_object().unwrap().clone());
    let e = f.reject(
        "flat",
        "memory_edits",
        json!([flat]),
        "memory_edits[0].current_basis",
    );
    assert!(e.message.contains("unknown field"));
}
#[test]
fn basis_strings_and_missing_change_basis_have_precise_paths() {
    let f = Fixture::new();
    let mut e = edit();
    e["record"]["current_basis"] = json!(["work"]);
    let error = f.reject(
        "current-string",
        "memory_edits",
        json!([e]),
        "memory_edits[0].record.current_basis[0]",
    );
    assert!(error.message.contains("unknown variant"));
    let mut e = edit();
    e["change_basis"] = json!(["work"]);
    f.reject(
        "change-string",
        "memory_edits",
        json!([e]),
        "memory_edits[0].change_basis[0]",
    );
    let mut e = edit();
    e.as_object_mut().unwrap().remove("change_basis");
    f.reject(
        "change-missing",
        "memory_edits",
        json!([e]),
        "memory_edits[0]",
    );
}
#[test]
fn malformed_event_support_reports_its_nested_field_and_retry_does_not_duplicate_raw() {
    let f = Fixture::new();
    let o = json!([{"client_ref":"work","kind":"learner_question","description":"Coefficient question","events":[{"event_id":"e","role":"learner_response","excerpts":[7]}]}]);
    let first = f.reject(
        "bad-event",
        "observations",
        o.clone(),
        "observations[0].events[0].excerpts[0]",
    );
    let second = f.reject(
        "bad-event",
        "observations",
        o,
        "observations[0].events[0].excerpts[0]",
    );
    assert_eq!(first.captured_event_ids, second.captured_event_ids);
    assert_eq!(
        f.store
            .conn
            .query_row("SELECT count(*) FROM learning_events", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}
