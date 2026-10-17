use super::*;
use crate::learner::{EventMetadata, HostContext, LearnerStore, RawEvent, Speaker, limits};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

struct Fixture {
    store: LearnerStore,
    host: HostContext,
}
impl Fixture {
    fn new() -> Self {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(crate::learner::store::SCHEMA_SQL)
            .unwrap();
        conn.execute("INSERT INTO workspace_meta(singleton,workspace_id,storage_family,schema_version,created_at_ms) VALUES(1,'event-paging','learner-model/v2',2,0)", []).unwrap();
        Self {
            store: LearnerStore {
                conn,
                workspace_id: "event-paging".into(),
            },
            host: HostContext {
                host_kind: "h".into(),
                host_thread_id: "event-paging".into(),
            },
        }
    }
    fn capture(&self, text: &str) -> String {
        self.capture_with_metadata(text, None)
    }
    fn capture_with_metadata(&self, text: &str, metadata: Option<EventMetadata>) -> String {
        let key: i64 = self
            .store
            .conn
            .query_row("SELECT count(*) FROM learning_events", [], |row| row.get(0))
            .unwrap();
        self.store
            .capture(
                &self.host,
                &[RawEvent {
                    external_event_key: format!("large-event-{key}"),
                    speaker: Speaker::Learner,
                    kind: "message".into(),
                    text: text.into(),
                    metadata,
                    occurred_at_ms: Some(42),
                }],
                "host_observed",
                true,
            )
            .unwrap()
            .remove(0)
    }
    fn read(&self, refs: Value, max: usize) -> Result<Value, String> {
        self.next(&json!({"tool":"memory","arguments":{
            "schema":crate::learner::SCHEMA,"action":"read","refs":refs,"max_bytes":max
        }}))
    }
    fn next(&self, call: &Value) -> Result<Value, String> {
        assert_eq!(call["tool"], "memory");
        finish_trace(
            &self.store.conn,
            prepare_snapshot_for_tool(
                &self.store.conn,
                &self.host.host_kind,
                &self.host.host_thread_id,
                call["arguments"].clone(),
                Tool::Memory,
            )?,
            false,
        )
    }

    fn memory(&self, id: &str) {
        self.store.conn.execute("INSERT INTO learner_memories(memory_id,title,scope,summary,state_text,current_basis_json,lifecycle,revision,created_at_ms,updated_at_ms) VALUES(?1,'title','scope','whole summary','whole descriptive state','[]','active',2,0,0)", [id]).unwrap();
    }
}

/// The emitted call must consume refs or advance the first event range. A
/// remaining range retains its original final endpoint and every later ref.
fn assert_progress(before: &[Value], packet: &Value) {
    let mut remaining = before.to_vec();
    for record in packet["records"].as_array().unwrap() {
        let first = &remaining[0];
        match first["kind"].as_str().unwrap() {
            "event" => {
                assert_eq!(record["event_id"], first["id"]);
                assert_eq!(
                    record["start_byte"].as_u64().unwrap(),
                    first["start_byte"].as_u64().unwrap_or(0)
                );
                let final_end = first["end_byte"]
                    .as_u64()
                    .unwrap_or_else(|| record["content_length"].as_u64().unwrap());
                let end = record["end_byte"].as_u64().unwrap();
                assert!(end <= final_end);
                if end < final_end {
                    assert!(end > record["start_byte"].as_u64().unwrap());
                    assert!(record["partial"].as_bool().unwrap());
                    remaining[0] = json!({"kind":"event","id":record["event_id"],
                        "start_byte":end,"end_byte":final_end});
                } else {
                    remaining.remove(0);
                }
            }
            "current_memory" => {
                assert_eq!(record["memory_id"], first["id"]);
                assert_eq!(record["state_text"], "whole descriptive state");
                assert_eq!(record["revision_changed"], first["expected_revision"] != 2);
                remaining.remove(0);
            }
            "observation" => {
                assert_eq!(record["observation_id"], first["id"]);
                remaining.remove(0);
            }
            kind => panic!("unexpected selected reference kind {kind}"),
        }
    }
    assert_ne!(remaining, before, "nonterminal read made no progress");
    if remaining.is_empty() {
        assert!(packet["next_call"].is_null());
    } else {
        assert_eq!(packet["next_call"]["arguments"]["refs"], json!(remaining));
    }
}

fn assert_largest_prefix(packet: &Value, original: &str, max: usize) {
    if packet["next_call"].is_null() {
        return;
    }
    let record = packet["records"].as_array().unwrap().last().unwrap();
    let end = record["end_byte"].as_u64().unwrap() as usize;
    let refs = packet["next_call"]["arguments"]["refs"].as_array().unwrap();
    if refs[0]["kind"] != "event" || refs[0]["id"] != record["event_id"] {
        return;
    }
    let next_end = end + original[end..].chars().next().unwrap().len_utf8();
    let mut larger = packet.clone();
    let last = larger["records"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap();
    let mut text = last["text"].as_str().unwrap().to_string();
    text.push_str(&original[end..next_end]);
    last["text"] = json!(text);
    last["end_byte"] = json!(next_end);
    let final_end = refs[0]["end_byte"].as_u64().unwrap() as usize;
    if next_end == final_end {
        last["partial"] = json!(last["start_byte"] != 0 || next_end != original.len());
        let mut refs = refs.clone();
        refs.remove(0);
        if refs.is_empty() {
            larger["next_call"] = Value::Null;
        } else {
            larger["next_call"]["arguments"]["refs"] = json!(refs);
        }
    } else {
        larger["next_call"]["arguments"]["refs"][0]["start_byte"] = json!(next_end);
    }
    assert!(
        pages::size(&larger) > max,
        "a larger encoded prefix still fits"
    );
}

#[test]
fn captured_80k_event_recovers_using_only_emitted_calls() {
    let fixture = Fixture::new();
    let text = "x".repeat(80 * 1024);
    let id = fixture.capture(&text);
    let mut packet = fixture
        .read(json!([{"kind":"event","id":id}]), limits::PACKET_BYTES)
        .unwrap();
    let mut reconstructed = String::new();
    let mut chunks = 0;
    loop {
        assert!(pages::size(&packet) <= limits::PACKET_BYTES);
        let record = &packet["records"][0];
        assert_eq!(record["start_byte"], reconstructed.len());
        reconstructed.push_str(record["text"].as_str().unwrap());
        assert_eq!(record["end_byte"], reconstructed.len());
        chunks += 1;
        if packet["next_call"].is_null() {
            break;
        }
        packet = fixture.next(&packet["next_call"]).unwrap();
    }
    assert!(chunks > 1);
    assert_eq!(reconstructed, text);
}

#[test]
fn terminal_event_range_fits_exactly_without_nonterminal_continuation_overhead() {
    let fixture = Fixture::new();
    let text = "é🙂\n".repeat(120);
    let id = fixture.capture(&text);
    let whole = fixture
        .read(json!([{"kind":"event","id":id}]), 65536)
        .unwrap();
    let exact_budget = pages::size(&whole);
    let packet = fixture
        .read(json!([{"kind":"event","id":id}]), exact_budget)
        .unwrap();
    assert_eq!(pages::size(&packet), exact_budget);
    assert_eq!(packet["records"][0]["text"], text);
    assert_eq!(packet["records"][0]["partial"], false);
    assert!(packet["next_call"].is_null());
}

#[test]
fn event_paging_uses_exact_escaped_utf8_bytes_and_preserves_interior_end() {
    let fixture = Fixture::new();
    let text = "\"\\\n\t\r\0é🙂".repeat(6000);
    let id = fixture.capture(&text);
    let start = text.char_indices().nth(13).unwrap().0;
    let end = text.char_indices().nth(43000).unwrap().0;
    let max = 4096;
    let mut refs = json!([{"kind":"event","id":id,"start_byte":start,"end_byte":end}]);
    let mut packet = fixture.read(refs.clone(), max).unwrap();
    let mut reconstructed = String::new();
    loop {
        assert!(pages::size(&packet) <= max);
        assert_progress(refs.as_array().unwrap(), &packet);
        assert_largest_prefix(&packet, &text, max);
        let record = &packet["records"][0];
        assert_eq!(record["content_length"], text.len());
        assert_eq!(record["start_byte"], start + reconstructed.len());
        reconstructed.push_str(record["text"].as_str().unwrap());
        assert_eq!(record["end_byte"], start + reconstructed.len());
        if packet["next_call"].is_null() {
            break;
        }
        assert_eq!(packet["next_call"]["arguments"]["max_bytes"], max);
        assert_eq!(packet["next_call"]["arguments"]["refs"][0]["end_byte"], end);
        refs = packet["next_call"]["arguments"]["refs"].clone();
        packet = fixture.next(&packet["next_call"]).unwrap();
    }
    assert_eq!(reconstructed, text[start..end]);
}

#[test]
fn empty_event_and_empty_interior_range_are_terminal_and_invalid_utf8_is_rejected() {
    let fixture = Fixture::new();
    let empty = fixture.capture("");
    let packet = fixture
        .read(json!([{"kind":"event","id":empty}]), 4096)
        .unwrap();
    assert_eq!(packet["records"][0]["text"], "");
    assert_eq!(packet["records"][0]["start_byte"], 0);
    assert_eq!(packet["records"][0]["end_byte"], 0);
    assert_eq!(packet["records"][0]["partial"], false);
    assert!(packet["next_call"].is_null());
    let id = fixture.capture("aé🙂z");
    let packet = fixture
        .read(
            json!([{"kind":"event","id":id,"start_byte":3,"end_byte":3}]),
            4096,
        )
        .unwrap();
    assert_eq!(packet["records"][0]["text"], "");
    assert_eq!(packet["records"][0]["partial"], true);
    assert!(packet["next_call"].is_null());
    for (start, end) in [(0, 2), (2, 7), (0, 99), (7, 3)] {
        let error = fixture
            .read(
                json!([{"kind":"event","id":id,"start_byte":start,"end_byte":end}]),
                4096,
            )
            .unwrap_err();
        assert!(
            error.starts_with("invalid_variant: event byte range"),
            "{error}"
        );
        assert!(fixture.store.conn.is_autocommit());
    }
}

#[test]
fn event_chunks_precede_later_refs_and_every_continuation_progresses() {
    let fixture = Fixture::new();
    let text = "évidence\n".repeat(2000);
    let id = fixture.capture(&text);
    let second = fixture.capture(&"second ".repeat(1800));
    fixture.memory("first");
    fixture.memory("last");
    fixture.store.conn.execute("INSERT INTO observations(observation_id,kind,description,source_refs_json,created_at_ms) VALUES('o','learner_work','qualified work','[]',0)", []).unwrap();
    let max = 4000;
    let mut refs = json!([
        {"kind":"current_memory","id":"first","expected_revision":2},
        {"kind":"event","id":id},
        {"kind":"current_memory","id":"last","expected_revision":1},
        {"kind":"event","id":second,"end_byte":8400},
        {"kind":"observation","id":"o"}
    ]);
    let before: (i64, i64) = fixture
        .store
        .conn
        .query_row(
            "SELECT event_generation,memory_generation FROM workspace_meta",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let mut packet = fixture.read(refs.clone(), max).unwrap();
    let mut received = Vec::new();
    loop {
        assert_progress(refs.as_array().unwrap(), &packet);
        assert!(pages::size(&packet) <= max);
        received.extend(packet["records"].as_array().unwrap().iter().cloned());
        if packet["next_call"].is_null() {
            break;
        }
        assert_eq!(packet["next_call"]["arguments"]["max_bytes"], max);
        refs = packet["next_call"]["arguments"]["refs"].clone();
        packet = fixture.next(&packet["next_call"]).unwrap();
    }
    let sequence = received
        .iter()
        .map(|record| {
            record["memory_id"]
                .as_str()
                .or_else(|| record["event_id"].as_str())
                .or_else(|| record["observation_id"].as_str())
                .unwrap()
        })
        .fold(Vec::<&str>::new(), |mut ids, id| {
            if ids.last() != Some(&id) {
                ids.push(id);
            }
            ids
        });
    assert_eq!(sequence, ["first", &id, "last", &second, "o"]);
    let original = received
        .iter()
        .filter(|record| record["event_id"] == id)
        .map(|record| record["text"].as_str().unwrap())
        .collect::<String>();
    assert_eq!(original, text);
    let after = fixture
        .store
        .conn
        .query_row(
            "SELECT event_generation,memory_generation FROM workspace_meta",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .unwrap();
    assert_eq!(before, after);
    assert_eq!(
        fixture
            .store
            .conn
            .query_row(
                "SELECT count(*) FROM learning_events WHERE processing='pending'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        2
    );
    assert_eq!(
        fixture
            .store
            .conn
            .query_row("SELECT min(revision) FROM learner_memories", [], |row| row
                .get::<_, i64>(
                0
            ))
            .unwrap(),
        2
    );
}

#[test]
fn paged_events_retain_provenance_metadata_and_support_notices() {
    let fixture = Fixture::new();
    let metadata = EventMetadata {
        tool_name: Some("checker".into()),
        help_refs: vec!["tutor-assistance".into()],
        coverage: Some("partial coverage; context unknown".into()),
        ..EventMetadata::default()
    };
    let id = fixture.capture_with_metadata(&"raw text ".repeat(10000), Some(metadata.clone()));
    fixture.store.conn.execute("INSERT INTO observations(observation_id,kind,description,source_refs_json,created_at_ms) VALUES('o','learner_work','with help','[]',0)", []).unwrap();
    fixture
        .store
        .conn
        .execute(
            "INSERT INTO observation_events VALUES('o',?1,'learner_response','[]')",
            [&id],
        )
        .unwrap();
    for n in 0..36 {
        let memory = format!("m-{n:02}");
        fixture.memory(&memory);
        fixture
            .store
            .conn
            .execute("INSERT INTO memory_observations VALUES(?1,'o')", [&memory])
            .unwrap();
    }
    let full = fixture
        .read(json!([{"kind":"event","id":id}]), 65536)
        .unwrap();
    let record = &full["records"][0];
    assert_eq!(record["current_memory_refs"].as_array().unwrap().len(), 32);
    assert_eq!(record["current_memory_refs_more"], true);
    let page = fixture
        .read(json!([{"kind":"event","id":id}]), 1800)
        .unwrap();
    let record = &page["records"][0];
    assert_eq!(record["event_id"], id);
    assert_eq!(record["speaker"], "learner");
    assert_eq!(record["event_kind"], "message");
    assert_eq!(record["origin"], "host_observed");
    assert_eq!(record["occurred_at_ms"], 42);
    assert!(record["captured_at_ms"].as_i64().unwrap() > 0);
    assert_eq!(record["processing"], "pending");
    assert_eq!(record["metadata"], serde_json::to_value(metadata).unwrap());
    assert_eq!(record["content_length"], 90000);
    assert_eq!(record["current_memory_refs_more"], true);
    let call = &record["current_memory_refs_continuation"];
    assert_eq!(call["arguments"]["kind"], "event_memory_refs");
    assert_eq!(call["arguments"]["event_id"], id);
    let support = fixture.next(call).unwrap();
    assert_eq!(support["support"].as_array().unwrap().len(), 32);
    assert!(!support["next_call"].is_null());
}

#[test]
fn metadata_only_budget_errors_do_not_repeat_an_ineffective_maximum_call() {
    let fixture = Fixture::new();
    let id = fixture.capture_with_metadata(
        "bytes",
        Some(EventMetadata {
            coverage: Some("context ".repeat(1000)),
            ..EventMetadata::default()
        }),
    );
    let error = fixture
        .read(json!([{"kind":"event","id":id}]), 1500)
        .unwrap_err();
    assert!(error.starts_with("budget_too_small"));
    assert!(error.contains("increase max_bytes"));
    assert!(!error.contains("next_call="));
    // Old/corrupt stores can contain metadata beyond today's capture envelope.
    // Never promise that repeating an identical maximum-budget read will work.
    fixture
        .store
        .conn
        .execute(
            "UPDATE learning_events SET metadata_json=?1 WHERE event_id=?2",
            params![json!({"coverage":"\0".repeat(12000)}).to_string(), id],
        )
        .unwrap();
    let error = fixture
        .read(json!([{"kind":"event","id":id}]), 65536)
        .unwrap_err();
    assert!(error.starts_with("budget_too_small"));
    assert!(error.contains("raising max_bytes cannot help"));
    assert!(!error.contains("next_call="));
}

#[test]
fn cancellation_during_page_sizing_and_between_chunks_rolls_back_without_progress() {
    let fixture = Fixture::new();
    let id = fixture.capture(&"text ".repeat(20000));
    let first = fixture
        .read(json!([{"kind":"event","id":id}]), 4096)
        .unwrap();
    let flag = Arc::new(AtomicBool::new(false));
    let hook_flag = flag.clone();
    let cancellation = crate::request::OperationCancellation::with_flag(flag.clone())
        .with_test_hook(Arc::new(move |checkpoint| {
            if checkpoint == "learner_event_page_probe" {
                hook_flag.store(true, Ordering::Relaxed);
            }
        }));
    let scope = crate::request::enter_request_cancellation(&cancellation);
    let error = fixture
        .read(json!([{"kind":"event","id":id}]), 4096)
        .unwrap_err();
    assert!(error.contains("request_cancelled"));
    assert!(fixture.store.conn.is_autocommit());
    drop(scope);
    let cancelled = crate::request::OperationCancellation::with_flag(flag);
    let scope = crate::request::enter_request_cancellation(&cancelled);
    let error = fixture.next(&first["next_call"]).unwrap_err();
    assert!(error.contains("request_cancelled"));
    assert!(fixture.store.conn.is_autocommit());
    drop(scope);
    let next = fixture.next(&first["next_call"]).unwrap();
    assert_eq!(
        next["records"][0]["start_byte"],
        first["records"][0]["end_byte"]
    );
    assert_eq!(
        fixture
            .store
            .conn
            .query_row(
                "SELECT processing FROM learning_events WHERE event_id=?1",
                [&id],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
        "pending"
    );
    assert_eq!(
        fixture
            .store
            .conn
            .query_row("SELECT count(*) FROM context_receipts", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn recorded_event_pages_reserve_trace_bytes_and_recording_off_writes_no_receipts() {
    let fixture = Fixture::new();
    let id = fixture.capture(&"x".repeat(80 * 1024));
    fixture.store.conn.execute("INSERT INTO host_bindings(host_kind,host_thread_id,learning_active,recording_enabled) VALUES('h','event-paging',1,1)", []).unwrap();
    let read = |call: Value| {
        finish_trace(
            &fixture.store.conn,
            prepare_snapshot_for_tool(&fixture.store.conn, "h", "event-paging", call, Tool::Memory)
                .unwrap(),
            true,
        )
        .unwrap()
    };
    let first = read(
        json!({"schema":crate::learner::SCHEMA,"action":"read","refs":[{"kind":"event","id":id}],"max_bytes":65536}),
    );
    assert!(pages::size(&first) <= 65536);
    assert_eq!(first["trace_status"], "recorded");
    assert!(!first["next_call"].is_null());
    fixture
        .store
        .conn
        .execute("UPDATE host_bindings SET recording_enabled=0", [])
        .unwrap();
    let next = read(first["next_call"]["arguments"].clone());
    assert_eq!(next["trace_status"], "disabled");
    assert_eq!(
        fixture
            .store
            .conn
            .query_row("SELECT count(*) FROM context_receipts", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        fixture
            .store
            .conn
            .query_row(
                "SELECT text FROM learning_events WHERE event_id=?1",
                [&id],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
        "x".repeat(80 * 1024)
    );
}
