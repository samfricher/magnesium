//! Generated boundary regressions for attribution and encoded whole-record reads.
use super::*;
use rusqlite::params;
use serde_json::{Value, json};

struct Fixture {
    store: LearnerStore,
    host: HostContext,
    path: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "{}.sqlite",
            store::new_id("read-admission").unwrap()
        ));
        let store = LearnerStore::initialize(&path, "encoded-readability").unwrap();
        let host = HostContext {
            host_kind: "test".into(),
            host_thread_id: "encoded".into(),
        };
        store
            .update(
                &host,
                json!({"schema":SCHEMA,"action":"update","request_id":"start",
            "session":{"operation":"start","client_ref":"session","goal":"study"}}),
            )
            .unwrap();
        Self { store, host, path }
    }
    fn event(&self, key: &str, speaker: Speaker, text: &str) -> String {
        self.store
            .capture(
                &self.host,
                &[RawEvent {
                    external_event_key: key.into(),
                    speaker,
                    kind: "message".into(),
                    text: text.into(),
                    metadata: None,
                    occurred_at_ms: Some(42),
                }],
                "host_observed",
                false,
            )
            .unwrap()
            .remove(0)
    }
    fn read(&self, mut request: Value) -> std::result::Result<Value, String> {
        request["schema"] = json!(SCHEMA);
        request["max_bytes"] = json!(limits::PACKET_BYTES);
        read::test_support::execute(
            &self.store.conn,
            &self.host.host_kind,
            &self.host.host_thread_id,
            request,
        )
    }
    fn count(&self, table: &str) -> i64 {
        self.store
            .conn
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    }
    fn payload(&self, key: &str, event: &str, state: &str) -> Value {
        json!({"schema":SCHEMA,"action":"update","request_id":key,"turn_ref":event,
            "observations":[{"client_ref":"observation","kind":"learner_work","description":"Concrete learner work",
                "events":[{"event_id":event,"role":"learner_response"}]}],
            "memory_edits":[{"operation":"create","client_ref":"memory","record":{
                "title":"Title","scope":"Scope","summary":"Summary","state_text":state,
                "current_basis":[{"new_observation":"observation"}]},
                "reason":"New work","change_basis":[{"new_observation":"observation"}]}]})
    }
    fn assert_no_semantics(&self, original_event: &str, original_text: &str) {
        for table in [
            "observations",
            "learner_memories",
            "memory_changes",
            "memory_observations",
            "observation_events",
        ] {
            assert_eq!(self.count(table), 0, "{table}");
        }
        let row: (String, String) = self
            .store
            .conn
            .query_row(
                "SELECT text, processing FROM learning_events WHERE event_id = ?1",
                [original_event],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(row, (original_text.into(), "pending".into()));
        assert_eq!(self.count("operation_receipts"), 1);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[test]
fn observation_attribution_rejects_tutor_solution_as_learner_response_atomically() {
    let fixture = Fixture::new();
    let question = fixture.event("learner", Speaker::Learner, "Please show the solution");
    let solution = fixture.event("tutor", Speaker::Tutor, "The answer is 42");
    let mut request = fixture.payload(
        "bad-attribution",
        &question,
        "Would misattribute the tutor's work",
    );
    request["observations"][0]["events"] = json!([
        {"event_id":question,"role":"task_context"},
        {"event_id":solution,"role":"learner_response"}
    ]);
    request["reported_events"] = json!([{ "external_event_key":"raw-preserved", "speaker":"learner", "kind":"message", "text":"Keep this exact input: λ²" }]);
    let error = fixture.store.update(&fixture.host, request).unwrap_err();
    assert_eq!(error.code, "invalid_variant");
    assert_eq!(
        error.recovery.as_ref().unwrap()["field_path"],
        "observations[0].events[1].role"
    );
    assert!(error.message.contains("learner-authored"));
    assert!(!error.captured_event_ids.is_empty());
    fixture.assert_no_semantics(&question, "Please show the solution");
    assert_eq!(
        fixture
            .store
            .conn
            .query_row(
                "SELECT text FROM learning_events WHERE external_event_key = 'raw-preserved'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "Keep this exact input: λ²"
    );
}

#[test]
fn observation_attribution_requires_relevant_learner_role_without_restricting_other_roles() {
    for kind in ["learner_work", "learner_question", "self_report"] {
        let fixture = Fixture::new();
        let response = fixture.event(
            "learner",
            Speaker::Learner,
            "I quote the book: 'I know this'. Is that right?",
        );
        let help = fixture.event("tutor", Speaker::Tutor, "Here is the task and a hint");
        let mut request = fixture.payload(
            "role",
            &response,
            "A quotation or report is not certified understanding",
        );
        request["observations"][0]["kind"] = json!(kind);
        request["observations"][0]["events"] = json!([
            {"event_id":response,"role":"task_context"}, {"event_id":help,"role":"assistance"}
        ]);
        let error = fixture
            .store
            .update(&fixture.host, request.clone())
            .unwrap_err();
        assert_eq!(
            error.recovery.as_ref().unwrap()["field_path"],
            "observations[0].events"
        );
        fixture.assert_no_semantics(&response, "I quote the book: 'I know this'. Is that right?");
        request["request_id"] = json!("valid-role");
        request["observations"][0]["events"][0]["role"] = json!("learner_response");
        let result = fixture.store.update(&fixture.host, request).unwrap();
        let packet = fixture.read(json!({"action":"read","refs":[{"kind":"observation","id":result.references["observation"]}]})).unwrap();
        assert_eq!(
            packet["records"][0]["event_refs"][0]["origin"],
            "host_observed"
        );
        assert_eq!(packet["records"][0]["event_refs"][1]["speaker"], "tutor");
    }
}

#[test]
fn encoded_records_reject_unreadable_memory_and_preserve_separate_raw_capture() {
    let fixture = Fixture::new();
    let event = fixture.event("learner", Speaker::Learner, "Keep the original λ² ≠ 0");
    let mut request = fixture.payload(
        "escaped-memory",
        &event,
        &"\u{1}".repeat(limits::STATE_BYTES),
    );
    request["reported_events"] = json!([{ "external_event_key":"extra", "speaker":"learner", "kind":"message", "text":"Unabridged raw work" }]);
    let error = fixture.store.update(&fixture.host, request).unwrap_err();
    assert_eq!(error.code, "field_too_large");
    assert_eq!(
        error.recovery.as_ref().unwrap()["field_path"],
        "memory_edits[0].record.state_text"
    );
    assert!(error.message.contains("encoded current memory"));
    assert!(!error.captured_event_ids.is_empty());
    fixture.assert_no_semantics(&event, "Keep the original λ² ≠ 0");
    assert_eq!(fixture.count("learning_events"), 2);
}

#[test]
fn encoded_records_reject_unreadable_observation_and_history_atomically() {
    let fixture = Fixture::new();
    let event = fixture.event("learner", Speaker::Learner, "Raw learner text");
    let mut request = fixture.payload("escaped-observation", &event, "state");
    request["observations"][0]["description"] = json!("\u{1}".repeat(limits::OBSERVATION_BYTES));
    request["observations"][0]["conditions_text"] = json!("\u{1}".repeat(limits::CONDITIONS_BYTES));
    request["observations"][0]["assistance_text"] = json!("\u{1}".repeat(limits::ASSISTANCE_BYTES));
    let error = fixture.store.update(&fixture.host, request).unwrap_err();
    assert_eq!(error.code, "field_too_large");
    assert_eq!(
        error.recovery.as_ref().unwrap()["field_path"],
        "observations[0].description"
    );
    fixture.assert_no_semantics(&event, "Raw learner text");

    let mut request = fixture.payload("escaped-history", &event, &"\u{1}".repeat(9000));
    request["memory_edits"][0]["record"]["retrieval_cues"] = json!(
        (0..limits::CUE_COUNT)
            .map(|index| format!("{}{:02}", "\u{1}".repeat(limits::CUE_BYTES - 2), index))
            .collect::<Vec<_>>()
    );
    let error = fixture.store.update(&fixture.host, request).unwrap_err();
    assert_eq!(error.code, "field_too_large");
    assert!(error.message.contains("historical snapshot"), "{error}");
    fixture.assert_no_semantics(&event, "Raw learner text");
}

#[test]
fn encoded_records_generated_successes_have_complete_max_budget_reads() {
    let mut accepted = 0;
    let mut rejected = 0;
    for pattern in ["x", "\u{1}", "\\", "\"", "λ²≠🙂", "\n\r\t", "a\\\"\u{1}λ"] {
        for bytes in [128, 2048, 8192, 12000, limits::STATE_BYTES] {
            let fixture = Fixture::new();
            let original = pattern.repeat(bytes / pattern.len());
            let event = fixture.event("learner", Speaker::Learner, &original);
            let mut request = fixture.payload("generated", &event, &original);
            let description = pattern.repeat(bytes.min(limits::OBSERVATION_BYTES) / pattern.len());
            let excerpt = pattern.repeat(bytes.min(2048) / pattern.len());
            request["observations"][0]["description"] = json!(description);
            request["observations"][0]["events"][0]["excerpts"] = json!([excerpt]);
            request["memory_edits"][0]["record"]["title"] =
                json!(pattern.repeat(limits::TITLE_BYTES / pattern.len()));
            request["memory_edits"][0]["record"]["scope"] =
                json!(pattern.repeat(limits::SCOPE_BYTES / pattern.len()));
            request["memory_edits"][0]["record"]["summary"] =
                json!(pattern.repeat(limits::SUMMARY_BYTES / pattern.len()));
            let result = match fixture.store.update(&fixture.host, request) {
                Ok(result) => result,
                Err(error) => {
                    assert_eq!(error.code, "field_too_large", "{error}");
                    fixture.assert_no_semantics(&event, &original);
                    rejected += 1;
                    continue;
                }
            };
            accepted += 1;
            let memory_id = &result.references["memory"];
            let observation_id = &result.references["observation"];
            let memory = fixture
                .read(json!({"action":"read","refs":[{"kind":"current_memory","id":memory_id}]}))
                .unwrap();
            assert_eq!(memory["records"][0]["state_text"], original);
            assert_eq!(
                memory["records"][0]["current_basis"],
                json!([{"observation_id":observation_id}])
            );
            let expected = fixture.read(json!({"action":"read","refs":[{"kind":"current_memory","id":memory_id,"expected_revision":i64::MAX}]})).unwrap();
            assert_eq!(expected["records"][0]["state_text"], original);
            assert_eq!(expected["records"][0]["revision_changed"], true);
            let observation = fixture
                .read(json!({"action":"read","refs":[{"kind":"observation","id":observation_id}]}))
                .unwrap();
            assert_eq!(observation["records"][0]["description"], description);
            assert_eq!(
                observation["records"][0]["event_refs"][0]["excerpts"],
                json!([excerpt])
            );
            let history = fixture
                .read(json!({"action":"history","memory_id":memory_id,"limit":1}))
                .unwrap();
            assert_eq!(history["history"][0]["snapshot"]["state_text"], original);
            for packet in [memory, observation, history] {
                assert!(serde_json::to_vec(&packet).unwrap().len() <= limits::PACKET_BYTES);
            }
        }
    }
    assert!(accepted > 20);
    assert!(rejected > 0);
}

#[test]
fn canonical_snapshots_reject_corrupt_json_and_missing_required_history_fields() {
    let fixture = Fixture::new();
    let event = fixture.event("learner", Speaker::Learner, "answer");
    let result = fixture
        .store
        .update(&fixture.host, fixture.payload("valid", &event, "state"))
        .unwrap();
    let memory = &result.references["memory"];
    // Simulate malformed persisted data, including corruption blocked by ordinary SQL writes.
    fixture
        .store
        .conn
        .execute_batch("PRAGMA ignore_check_constraints = ON")
        .unwrap();
    for column in [
        "retrieval_cues_json",
        "current_basis_json",
        "replacement_ids_json",
    ] {
        let previous: String = fixture
            .store
            .conn
            .query_row(
                &format!("SELECT {column} FROM learner_memories WHERE memory_id = ?1"),
                [memory],
                |row| row.get(0),
            )
            .unwrap();
        for broken in ["not json", "null", "{}", "[42]"] {
            fixture
                .store
                .conn
                .execute(
                    &format!("UPDATE learner_memories SET {column} = ?2 WHERE memory_id = ?1"),
                    params![memory, broken],
                )
                .unwrap();
            let error = update::memory_snapshot(&fixture.store.conn, memory).unwrap_err();
            assert_eq!(error.code, "stored_data_invalid");
            assert!(error.message.contains(column));
            let read = fixture
                .read(json!({"action":"read","refs":[{"kind":"current_memory","id":memory}]}))
                .unwrap_err();
            assert!(read.contains("stored_data_invalid"));
        }
        fixture
            .store
            .conn
            .execute(
                &format!("UPDATE learner_memories SET {column} = ?2 WHERE memory_id = ?1"),
                params![memory, previous],
            )
            .unwrap();
    }
    let snapshot = update::memory_snapshot(&fixture.store.conn, memory).unwrap();
    for field in [
        "title",
        "current_basis",
        "topics",
        "state_text",
        "review_note",
        "last_observed_at_ms",
    ] {
        let mut broken = snapshot.clone();
        broken.as_object_mut().unwrap().remove(field);
        fixture
            .store
            .conn
            .execute(
                "UPDATE memory_changes SET snapshot_json = ?2 WHERE memory_id = ?1",
                params![memory, broken.to_string()],
            )
            .unwrap();
        let error = fixture
            .read(json!({"action":"history","memory_id":memory}))
            .unwrap_err();
        assert!(error.contains("stored_data_invalid"), "{error}");
        assert!(error.contains(field), "{error}");
    }
}

#[test]
fn encoded_records_remain_readable_when_future_memory_support_preview_grows() {
    let fixture = Fixture::new();
    let event = fixture.event("learner", Speaker::Learner, "answer");
    let mut request = fixture.payload("large-observation", &event, "unused");
    request["memory_edits"] = json!([]);
    request["observations"][0]["description"] = json!("\u{1}".repeat(8000));
    request["observations"][0]["conditions_text"] = json!("\u{1}".repeat(1536));
    request["observations"][0]["assistance_text"] = json!("\u{1}".repeat(768));
    let result = fixture.store.update(&fixture.host, request).unwrap();
    let observation = &result.references["observation"];
    for batch in 0..5 {
        let edits: Vec<_> = (0..8).map(|index| json!({
            "operation":"create","client_ref":format!("m{index}"),
            "record":{"title":"Associated memory","scope":"scope","summary":"summary","state_text":"complete state",
                "current_basis":[{"observation_id":observation}]},
            "reason":"reuse observation","change_basis":[{"observation_id":observation}]
        })).collect();
        fixture
            .store
            .update(
                &fixture.host,
                json!({"schema":SCHEMA,"action":"update",
            "request_id":format!("associations-{batch}"),"turn_ref":event,"memory_edits":edits}),
            )
            .unwrap();
    }
    let mut small = fixture.payload("small-later-observation", &event, "unused");
    small["memory_edits"] = json!([]);
    fixture.store.update(&fixture.host, small).unwrap();
    let browse = fixture
        .read(json!({"action":"observations","limit":1}))
        .unwrap();
    assert_eq!(
        browse["observations"][0]["observation_id"],
        observation.as_str()
    );
    assert_eq!(
        browse["observations"][0]["description"],
        "\u{1}".repeat(8000)
    );
    assert!(!browse["next_call"].is_null());
    let packet = fixture
        .read(json!({"action":"read","refs":[{"kind":"observation","id":observation}]}))
        .unwrap();
    assert_eq!(packet["records"][0]["description"], "\u{1}".repeat(8000));
    assert!(serde_json::to_vec(&packet).unwrap().len() <= limits::PACKET_BYTES);
    let record = &packet["records"][0];
    assert!(record["current_memory_refs"].as_array().unwrap().len() < 32);
    assert_eq!(record["current_memory_refs_more"], true);
    let mut next = record["current_memory_refs_continuation"].clone();
    let mut ids = std::collections::BTreeSet::new();
    loop {
        assert_eq!(next["tool"], "memory");
        let prepared = read::prepare_snapshot_for_tool(
            &fixture.store.conn,
            &fixture.host.host_kind,
            &fixture.host.host_thread_id,
            next["arguments"].clone(),
            read::Tool::Memory,
        )
        .unwrap();
        let page = read::finish_trace(&fixture.store.conn, prepared, true).unwrap();
        for memory in page["support"].as_array().unwrap() {
            assert!(ids.insert(memory["memory_id"].as_str().unwrap().to_owned()));
        }
        next = page["next_call"].clone();
        if next.is_null() {
            break;
        }
    }
    assert_eq!(ids.len(), 40);
}

#[test]
fn encoded_records_maximum_references_and_history_pages_round_trip() {
    let fixture = Fixture::new();
    let excerpt = "\\\"".repeat(128);
    let events: Vec<_> = (0..16)
        .map(|index| fixture.event(&format!("learner-{index}"), Speaker::Learner, &excerpt))
        .collect();
    let observations: Vec<_> = (0..16)
        .map(|index| {
            json!({
                "client_ref":format!("o{index}"),"kind":"learner_work",
                "description":"\"".repeat(limits::OBSERVATION_BYTES),
                "conditions_text":"\"".repeat(limits::CONDITIONS_BYTES),
                "assistance_text":"\"".repeat(limits::ASSISTANCE_BYTES),
                "events":events.iter().map(|event|json!({"event_id":event,"role":"learner_response",
                    "excerpts":[excerpt,excerpt,excerpt,excerpt]})).collect::<Vec<_>>()
            })
        })
        .collect();
    let mut record = json!({
        "title":"\"".repeat(limits::TITLE_BYTES),"scope":"\"".repeat(limits::SCOPE_BYTES),
        "summary":"\"".repeat(limits::SUMMARY_BYTES),"state_text":"\"".repeat(8192),
        "topics":(0..limits::TOPIC_COUNT).map(|index|format!("{}-{index}", "\"".repeat(120))).collect::<Vec<_>>(),
        "retrieval_cues":(0..limits::CUE_COUNT).map(|index|format!("{}-{index}", "\"".repeat(120))).collect::<Vec<_>>(),
        "review_note":"\"".repeat(limits::REVIEW_NOTE_BYTES),
        "current_basis":(0..8).map(|index|json!({"new_observation":format!("o{index}")})).collect::<Vec<_>>()
    });
    let result = fixture.store.update(&fixture.host, json!({"schema":SCHEMA,"action":"update",
        "request_id":"maximum-refs","turn_ref":events[0],"observations":observations,
        "memory_edits":[{"operation":"create","client_ref":"memory","record":record,
            "reason":"\"".repeat(4096),"change_basis":(0..16).map(|index|json!({"new_observation":format!("o{index}")})).collect::<Vec<_>>() }]
    })).unwrap();
    let memory_id = &result.references["memory"];
    let basis: Vec<_> = (0..8)
        .map(|index| json!({"observation_id":result.references[&format!("o{index}")]}))
        .collect();
    record["current_basis"] = json!(basis);
    for revision in 1..4 {
        fixture.store.update(&fixture.host, json!({"schema":SCHEMA,"action":"update",
            "request_id":format!("revision-{revision}"),"turn_ref":events[0],
            "memory_edits":[{"operation":"revise","memory_id":memory_id,"expected_revision":revision,
                "record":record,"change_kind":"editorial","reason":"\"".repeat(4096),
                "change_basis":(0..16).map(|index|json!({"observation_id":result.references[&format!("o{index}")]})).collect::<Vec<_>>() }]
        })).unwrap();
    }
    let memory = fixture.read(json!({"action":"read","refs":[{"kind":"current_memory","id":memory_id,"expected_revision":i64::MAX}]})).unwrap();
    assert_eq!(memory["records"][0]["current_basis"], json!(basis));
    assert_eq!(memory["records"][0]["state_text"], record["state_text"]);
    for index in 0..16 {
        let observation = fixture.read(json!({"action":"read","refs":[{"kind":"observation","id":result.references[&format!("o{index}")]}]})).unwrap();
        assert_eq!(
            observation["records"][0]["event_refs"]
                .as_array()
                .unwrap()
                .len(),
            16
        );
        assert!(serde_json::to_vec(&observation).unwrap().len() <= limits::PACKET_BYTES);
    }
    let mut page = fixture
        .read(json!({"action":"history","memory_id":memory_id,"limit":1}))
        .unwrap();
    let mut revisions = vec![];
    loop {
        assert!(serde_json::to_vec(&page).unwrap().len() <= limits::PACKET_BYTES);
        for change in page["history"].as_array().unwrap() {
            revisions.push(change["revision"].as_i64().unwrap());
            assert_eq!(change["snapshot"]["state_text"], record["state_text"]);
            assert_eq!(change["change_basis"].as_array().unwrap().len(), 16);
        }
        let next = &page["next_call"];
        if next.is_null() {
            break;
        }
        assert_eq!(next["tool"], "memory");
        let prepared = read::prepare_snapshot_for_tool(
            &fixture.store.conn,
            &fixture.host.host_kind,
            &fixture.host.host_thread_id,
            next["arguments"].clone(),
            read::Tool::Memory,
        )
        .unwrap();
        page = read::finish_trace(&fixture.store.conn, prepared, true).unwrap();
    }
    assert_eq!(revisions, [4, 3, 2, 1]);
}

#[test]
fn encoded_records_preserve_single_readable_legacy_observations() {
    let fixture = Fixture::new();
    let event = fixture.event("learner", Speaker::Learner, "answer");
    let mut request = fixture.payload("legacy-observation", &event, "unused");
    request["memory_edits"] = json!([]);
    let result = fixture.store.update(&fixture.host, request).unwrap();
    let observation = &result.references["observation"];
    // Emulate an already-stored record admitted under the previous decoded-byte
    // bounds. It must not lose its valid explicit whole-record read path.
    fixture.store.conn.execute(
        "UPDATE observations SET description = ?2, conditions_text = ?3 WHERE observation_id = ?1",
        params![observation, "\u{1}".repeat(limits::OBSERVATION_BYTES), "\u{1}".repeat(limits::CONDITIONS_BYTES)],
    ).unwrap();
    let initial = fixture
        .read(json!({"action":"read","refs":[{"kind":"observation","id":observation}]}))
        .unwrap();
    let spare = limits::PACKET_BYTES - serde_json::to_vec(&initial).unwrap().len() - 6;
    let assistance = "\u{1}".repeat(spare / 6);
    assert!(assistance.len() <= limits::ASSISTANCE_BYTES);
    fixture
        .store
        .conn
        .execute(
            "UPDATE observations SET assistance_text = ?2 WHERE observation_id = ?1",
            params![observation, assistance],
        )
        .unwrap();
    let packet = fixture
        .read(json!({"action":"read","refs":[{"kind":"observation","id":observation}]}))
        .unwrap();
    assert_eq!(packet["records"][0]["assistance_text"], assistance);
    assert!(serde_json::to_vec(&packet).unwrap().len() <= limits::PACKET_BYTES);
    // New admission still reserves both forms, including any future support call.
    let mut request = fixture.payload("new-too-large-observation", &event, "unused");
    request["memory_edits"] = json!([]);
    request["observations"][0]["description"] = json!("\u{1}".repeat(limits::OBSERVATION_BYTES));
    request["observations"][0]["conditions_text"] = json!("\u{1}".repeat(limits::CONDITIONS_BYTES));
    request["observations"][0]["assistance_text"] = json!(assistance);
    let error = fixture.store.update(&fixture.host, request).unwrap_err();
    assert_eq!(error.code, "field_too_large");
    assert_eq!(fixture.count("observations"), 1);
}

#[test]
fn encoded_records_identify_expanding_excerpts_without_discarding_raw_work() {
    let fixture = Fixture::new();
    let original = "\u{1}".repeat(limits::EXCERPTS_BYTES);
    let event = fixture.event("learner", Speaker::Learner, &original);
    let mut request = fixture.payload("expanded-excerpts", &event, "small state");
    request["observations"][0]["events"][0]["excerpts"] = json!([original]);
    let error = fixture.store.update(&fixture.host, request).unwrap_err();
    assert_eq!(error.code, "field_too_large");
    assert_eq!(
        error.recovery.as_ref().unwrap()["field_path"],
        "observations[0].events"
    );
    fixture.assert_no_semantics(&event, &original);
}
