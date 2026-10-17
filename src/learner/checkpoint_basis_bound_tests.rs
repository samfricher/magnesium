//! Writer-to-reader composition at the supported encoded packet ceiling.
use super::*;
use rusqlite::{params, types::ValueRef};
use serde_json::{Value, json};

const REFERENCES: usize = 16;

struct Fixture {
    store: LearnerStore,
    host: HostContext,
    path: std::path::PathBuf,
    session: String,
    goal: String,
    observation: String,
    proposal: String,
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "magnesium-checkpoint-bound-{}.sqlite",
            store::new_id("test").unwrap()
        ));
        let store = LearnerStore::initialize(&path, "checkpoint-bounds").unwrap();
        let host = HostContext {
            host_kind: "test".into(),
            host_thread_id: "checkpoint-bounds".into(),
        };
        let goal = padded(
            "\u{1}".repeat(2000) + "\"\\\nλ🙂",
            limits::SESSION_GOAL_BYTES,
        );
        let session = store.update(&host, json!({"schema":SCHEMA,"action":"update","request_id":"start","session":{"operation":"start","client_ref":"session","goal":goal}})).unwrap().references["session"].clone();
        let events = store
            .capture(
                &host,
                &[
                    RawEvent {
                        external_event_key: "work".into(),
                        speaker: Speaker::Learner,
                        kind: "message".into(),
                        text: "Original λ² work and a question about its interpretation".into(),
                        metadata: None,
                        occurred_at_ms: Some(42),
                    },
                    RawEvent {
                        external_event_key: "proposal".into(),
                        speaker: Speaker::Tutor,
                        kind: "message".into(),
                        text: "Compare the fixed coefficient with the variable".into(),
                        metadata: Some(EventMetadata {
                            delivery: Some(Delivery::Observed),
                            ..Default::default()
                        }),
                        occurred_at_ms: Some(43),
                    },
                ],
                "host_observed",
                false,
            )
            .unwrap();
        let observation = store.update(&host, json!({"schema":SCHEMA,"action":"update","request_id":"observe","observations":[{"client_ref":"work","kind":"learner_work","description":"Concrete original learner work; interpretation remains provisional","events":[{"event_id":events[0],"role":"learner_response"}]}],"event_dispositions":[{"event_id":events[0],"outcome":"processed"},{"event_id":events[1],"outcome":"no_change"}]})).unwrap().references["work"].clone();
        Self {
            store,
            host,
            path,
            session,
            goal,
            observation,
            proposal: events[1].clone(),
        }
    }

    fn update(&self, mut request: Value) -> UpdateResult {
        request["schema"] = json!(SCHEMA);
        request["action"] = json!("update");
        self.store.update(&self.host, request).unwrap()
    }

    fn record(&self, note: Option<&str>, maximal: bool) -> Value {
        let text = |name: &str, bound| {
            if maximal {
                padded(format!("{name}: \u{1}\"\\\nλ🙂"), bound)
            } else {
                name.to_owned()
            }
        };
        let state = if maximal {
            padded("\u{1}".repeat(6800) + "\"\\\nλ🙂", limits::STATE_BYTES)
        } else {
            "An unchanged provisional description".into()
        };
        json!({"title":text("Title",limits::TITLE_BYTES),"scope":text("Scope",limits::SCOPE_BYTES),"summary":text("Summary",limits::SUMMARY_BYTES),"state_text":state,"retrieval_cues":if maximal {(0..8).map(|n| padded(format!("cue-{n}-λ"),limits::CUE_BYTES)).collect::<Vec<_>>()}else{vec![]},"review_note":note,"current_basis":[{"observation_id":self.observation}]})
    }

    fn memories(&self, notes: &[Option<String>], maximal: bool) -> Vec<String> {
        let mut ids = vec![];
        for (batch, notes) in notes.chunks(limits::MEMORY_EDITS_PER_BATCH).enumerate() {
            let edits = notes.iter().enumerate().map(|(n, note)| json!({"operation":"create","client_ref":format!("memory-{n}"),"record":self.record(note.as_deref(),maximal),"reason":"Preserve observed work and its qualifications","change_basis":[{"observation_id":self.observation}]})).collect::<Vec<_>>();
            let result =
                self.update(json!({"request_id":format!("memories-{batch}"),"memory_edits":edits}));
            ids.extend((0..notes.len()).map(|n| result.references[&format!("memory-{n}")].clone()));
        }
        ids
    }

    fn checkpoint(&self, ids: &[String], revision: i64, session_revision: i64) -> Value {
        let mut checkpoint = json!({
            "current_problem":padded("Problem: \u{1}\"\\\nλ🙂".into(),limits::CHECKPOINT_FIELD_BYTES),
            "working_diagnosis":padded("Provisional diagnosis: \u{1}\"\\\nλ🙂".into(),limits::CHECKPOINT_FIELD_BYTES),
            "next_action":padded("Next teaching step: \u{1}\"\\\nλ🙂".into(),limits::CHECKPOINT_FIELD_BYTES),
            "plan_text":padded("Preserve intended task: \u{1}\"\\\nλ🙂".into(),limits::PLAN_BYTES),
            "memory_refs":ids.iter().map(|id|json!({"memory_id":id,"revision":revision})).collect::<Vec<_>>(),
            "pending_task":{"proposal_event_ref":self.proposal,"conditions":"Conditions: \u{1}\"\\\nλ🙂","observed_delivery_refs":[self.proposal],"help_refs":[],"reference_answer_ref":null}
        });
        let remaining = limits::CHECKPOINT_BYTES - encoded(&checkpoint);
        let conditions = checkpoint["pending_task"]["conditions"]
            .as_str()
            .unwrap()
            .to_owned();
        checkpoint["pending_task"]["conditions"] = json!(conditions + &"x".repeat(remaining));
        assert_eq!(encoded(&checkpoint), limits::CHECKPOINT_BYTES);
        self.update(json!({"request_id":"checkpoint","session":{"operation":"use","session_id":self.session,"expected_revision":session_revision},"checkpoint":checkpoint}));
        let stored: String = self
            .store
            .conn
            .query_row(
                "SELECT checkpoint_json FROM learning_sessions WHERE session_id=?1",
                [&self.session],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored.len(), limits::CHECKPOINT_BYTES);
        serde_json::from_str(&stored).unwrap()
    }

    fn read(&self, action: &str, bound: bool) -> Value {
        let mut request = json!({"schema":SCHEMA,"action":action,"max_bytes":limits::PACKET_BYTES});
        if !bound {
            request["session_id"] = json!(self.session);
        }
        if action == "prepare" {
            request["memory_ids"] = json!([]);
            request["known_revisions"] = json!([]);
        }
        let host = if bound {
            (&*self.host.host_kind, &*self.host.host_thread_id)
        } else {
            ("fresh-host", "fresh-thread")
        };
        let prepared = read::prepare_snapshot_for_tool(
            &self.store.conn,
            host.0,
            host.1,
            request,
            read::Tool::Context,
        )
        .unwrap();
        read::finish_trace(&self.store.conn, prepared, true).unwrap()
    }

    fn follow(&self, call: &Value, bound: bool) -> Value {
        // Execute the serialized arguments unchanged through the advertised tool.
        let typed: read::NextCall = serde_json::from_value(call.clone()).unwrap();
        assert_eq!(serde_json::to_value(typed).unwrap(), *call);
        assert_eq!(call["tool"], "memory");
        let host = if bound {
            (&*self.host.host_kind, &*self.host.host_thread_id)
        } else {
            ("fresh-host", "fresh-thread")
        };
        let prepared = read::prepare_snapshot_for_tool(
            &self.store.conn,
            host.0,
            host.1,
            call["arguments"].clone(),
            read::Tool::Memory,
        )
        .unwrap();
        read::finish_trace(&self.store.conn, prepared, true).unwrap()
    }

    fn receipt_count(&self) -> i64 {
        self.store
            .conn
            .query_row("SELECT count(*) FROM context_receipts", [], |r| r.get(0))
            .unwrap()
    }

    fn assert_packet(&self, packet: &Value, checkpoint: &Value, ids: &[String]) {
        assert!(encoded(packet) <= limits::PACKET_BYTES);
        assert_eq!(packet["session"]["checkpoint"], *checkpoint);
        assert_eq!(packet["session"]["goal"], self.goal);
        assert!(packet["revision_notices"].as_array().unwrap().is_empty());
        let notices = packet["checkpoint_basis"]["memories"].as_array().unwrap();
        assert_eq!(notices.len(), ids.len());
        for (notice, id) in notices.iter().zip(ids) {
            assert_eq!(notice["memory_id"], *id);
            assert!(notice.get("review_note").is_none());
            assert!(notice["has_review_note"].is_boolean());
            assert!(encoded(notice) < 1024, "bounded facts: {}", encoded(notice));
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn encoded(value: &Value) -> usize {
    serde_json::to_vec(value).unwrap().len()
}
fn padded(mut text: String, bytes: usize) -> String {
    assert!(text.len() <= bytes);
    text.push_str(&"x".repeat(bytes - text.len()));
    text
}
fn extreme_note(n: usize) -> String {
    let prefixes = ["\u{1}\"\\\nλ🙂", "🙂λ\n\\\"\u{1}", "\"λ\u{1}\\🙂\n"];
    let prefix = prefixes[n % prefixes.len()];
    let note = prefix.to_owned() + &"\u{1}".repeat(limits::REVIEW_NOTE_BYTES - prefix.len());
    assert_eq!(note.len(), limits::REVIEW_NOTE_BYTES);
    assert!(serde_json::to_vec(&note).unwrap().len() > 6000);
    note
}

/// Include every table and FTS shadow table. The sole permitted read side effect
/// is the optional context receipt, which is asserted independently.
fn canonical_state(conn: &rusqlite::Connection) -> Value {
    let tables = conn.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name!='context_receipts' ORDER BY name").unwrap().query_map([],|r|r.get::<_,String>(0)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
    let mut result = serde_json::Map::new();
    for table in tables {
        let quoted = table.replace('"', "\"\"");
        let mut statement = conn
            .prepare(&format!("SELECT * FROM \"{quoted}\""))
            .unwrap();
        let columns = statement.column_count();
        let mut rows = statement
            .query_map([], |row| {
                let cells = (0..columns)
                    .map(|column| {
                        Ok(match row.get_ref(column)? {
                            ValueRef::Null => Value::Null,
                            ValueRef::Integer(v) => json!(v),
                            ValueRef::Real(v) => json!(v),
                            ValueRef::Text(v) => json!(std::str::from_utf8(v).unwrap()),
                            ValueRef::Blob(v) => json!(v),
                        })
                    })
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(json!(cells))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        rows.sort_by_key(Value::to_string);
        result.insert(table, json!(rows));
    }
    Value::Object(result)
}

#[test]
fn checkpoint_basis_bounds_maximal_writer_join_and_unchanged_full_note_calls() {
    let f = Fixture::new();
    let notes = (0..REFERENCES)
        .map(|n| Some(extreme_note(n)))
        .collect::<Vec<_>>();
    let ids = f.memories(&notes, true);
    let checkpoint = f.checkpoint(&ids, 1, 1);
    let before = canonical_state(&f.store.conn);
    let receipts = f.receipt_count();
    for bound in [false, true] {
        for action in ["status", "prepare"] {
            let packet = f.read(action, bound);
            f.assert_packet(&packet, &checkpoint, &ids);
            assert_eq!(
                packet["checkpoint_basis"]["status"],
                "needs_reconsideration"
            );
            for (n, notice) in packet["checkpoint_basis"]["memories"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
            {
                assert_eq!(notice["used_revision"], 1);
                assert_eq!(notice["current_revision"], 1);
                assert_eq!(notice["status"], "unchanged");
                assert_eq!(notice["has_review_note"], true);
                assert_eq!(
                    notice["next_call"],
                    serde_json::to_value(read::NextCall::current_memory(&ids[n])).unwrap()
                );
                let current = f.follow(&notice["next_call"], bound);
                assert_eq!(current["records"][0]["review_note"], json!(notes[n]));
                assert_eq!(
                    current["records"][0]["state_text"],
                    f.record(notes[n].as_deref(), true)["state_text"]
                );
                assert_eq!(current["records"][0]["detail_status"], "complete");
                assert!(encoded(&current) > limits::PACKET_BYTES * 9 / 10);
            }
        }
        assert_eq!(canonical_state(&f.store.conn), before);
        assert_eq!(f.receipt_count(), receipts + if bound { 34 } else { 0 });
    }
}

#[test]
fn checkpoint_basis_bounds_later_revisions_flags_retirement_and_wide_counters() {
    let f = Fixture::new();
    let ids = f.memories(&vec![None; REFERENCES], true);
    let wide = i64::MAX - 4;
    // Seed only counter widths, not checkpoint/text/admission. Every record and
    // checkpoint is authored by the real writer, as are all later mutations.
    f.store
        .conn
        .execute("UPDATE learner_memories SET revision=?1", [wide])
        .unwrap();
    f.store
        .conn
        .execute(
            "UPDATE learning_sessions SET revision=?1 WHERE session_id=?2",
            params![wide, f.session],
        )
        .unwrap();
    let checkpoint = f.checkpoint(&ids, wide, wide);
    for action in ["status", "prepare"] {
        let packet = f.read(action, true);
        f.assert_packet(&packet, &checkpoint, &ids);
        assert_eq!(packet["checkpoint_basis"]["status"], "unchanged");
    }
    let notes = (0..REFERENCES).map(extreme_note).collect::<Vec<_>>();
    for (batch, memories) in ids.chunks(limits::MEMORY_EDITS_PER_BATCH).enumerate() {
        let edits = memories.iter().enumerate().map(|(n,id)| {
            let index = batch * limits::MEMORY_EDITS_PER_BATCH + n;
            if index.is_multiple_of(2) {
                json!({"operation":"flag","memory_id":id,"expected_revision":wide,"review_note":notes[index],"reason":"Unresolved question after saved checkpoint","change_basis":[{"observation_id":f.observation}]})
            } else {
                json!({"operation":"revise","memory_id":id,"expected_revision":wide,"record":f.record(Some(&notes[index]),true),"change_kind":"reinterpretation","reason":"Reconsider the same original work","change_basis":[{"observation_id":f.observation}]})
            }
        }).collect::<Vec<_>>();
        f.update(json!({"request_id":format!("later-{batch}"),"memory_edits":edits}));
    }
    let replacements = ids[REFERENCES - 4..].to_vec();
    f.update(json!({"request_id":"retire","memory_edits":ids[..limits::MEMORY_EDITS_PER_BATCH].iter().map(|id|json!({"operation":"retire","memory_id":id,"expected_revision":wide+1,"reason":"Retain a tombstone pointing to current replacements","replacement_ids":replacements})).collect::<Vec<_>>()}));
    let before = canonical_state(&f.store.conn);
    let receipts = f.receipt_count();
    for action in ["status", "prepare"] {
        let packet = f.read(action, true);
        f.assert_packet(&packet, &checkpoint, &ids);
        assert_eq!(
            packet["checkpoint_basis"]["status"],
            "needs_reconsideration"
        );
        assert_eq!(packet["session"]["revision"], wide + 1);
        for (n, notice) in packet["checkpoint_basis"]["memories"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            let retired = n < limits::MEMORY_EDITS_PER_BATCH;
            assert_eq!(notice["used_revision"], wide);
            assert_eq!(
                notice["current_revision"],
                wide + if retired { 2 } else { 1 }
            );
            assert_eq!(
                notice["status"],
                if retired { "retired" } else { "changed" }
            );
            assert_eq!(notice["has_review_note"], true);
            assert_eq!(
                notice["replacement_ids"],
                if retired {
                    json!(replacements)
                } else {
                    json!([])
                }
            );
            let current = f.follow(&notice["next_call"], true);
            assert_eq!(current["records"][0]["review_note"], notes[n]);
            assert_eq!(
                current["records"][0]["revision"],
                notice["current_revision"]
            );
            assert_eq!(
                current["records"][0]["lifecycle"],
                if retired { "retired" } else { "active" }
            );
        }
    }
    assert_eq!(canonical_state(&f.store.conn), before);
    assert_eq!(f.receipt_count(), receipts + 34);
}

#[test]
fn checkpoint_basis_bounds_null_empty_and_nonempty_are_exact_not_normalized() {
    let negative = Fixture::new();
    let negative_notes = vec![None, Some(String::new())];
    let negative_ids = negative.memories(&negative_notes, false);
    let negative_checkpoint = negative.checkpoint(&negative_ids, 1, 1);
    let negative_before = canonical_state(&negative.store.conn);
    for action in ["status", "prepare"] {
        let packet = negative.read(action, false);
        negative.assert_packet(&packet, &negative_checkpoint, &negative_ids);
        assert_eq!(packet["checkpoint_basis"]["status"], "unchanged");
        for notice in packet["checkpoint_basis"]["memories"].as_array().unwrap() {
            assert_eq!(notice["has_review_note"], false);
            assert!(notice.get("next_call").is_none());
        }
    }
    assert_eq!(canonical_state(&negative.store.conn), negative_before);
    assert_eq!(negative.receipt_count(), 0);

    let f = Fixture::new();
    let notes = vec![
        None,
        Some(String::new()),
        Some(" \n\t".into()),
        Some("\u{1}\"\\\nλ🙂".into()),
    ];
    let ids = f.memories(&notes, false);
    let checkpoint = f.checkpoint(&ids, 1, 1);
    let before = canonical_state(&f.store.conn);
    let receipts = f.receipt_count();
    for action in ["status", "prepare"] {
        let packet = f.read(action, false);
        f.assert_packet(&packet, &checkpoint, &ids);
        assert_eq!(
            packet["checkpoint_basis"]["status"],
            "needs_reconsideration"
        );
        for (n, notice) in packet["checkpoint_basis"]["memories"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            assert_eq!(notice["status"], "unchanged");
            assert_eq!(notice["has_review_note"], n >= 2);
            let call = if n >= 2 {
                assert_eq!(
                    notice["next_call"],
                    serde_json::to_value(read::NextCall::current_memory(&ids[n])).unwrap()
                );
                notice["next_call"].clone()
            } else {
                assert!(notice.get("next_call").is_none());
                serde_json::to_value(read::NextCall::current_memory(&ids[n])).unwrap()
            };
            let current = f.follow(&call, false);
            assert_eq!(current["records"][0]["review_note"], json!(notes[n]));
        }
    }
    assert_eq!(canonical_state(&f.store.conn), before);
    assert_eq!(f.receipt_count(), receipts);
}

#[test]
fn checkpoint_two_host_connections_compare_and_swap_then_reconcile_without_overwrite() {
    use std::sync::{Arc, Barrier};
    let f = Fixture::new();
    let second_host = HostContext {
        host_kind: f.host.host_kind.clone(),
        host_thread_id: "checkpoint-competitor".into(),
    };
    f.store.update(&second_host,json!({"schema":SCHEMA,"action":"update","request_id":"bind-competitor","session":{"operation":"resume","session_id":f.session,"expected_revision":1}})).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let mut workers = vec![];
    for (n, host) in [f.host.clone(), second_host].into_iter().enumerate() {
        let path = f.path.clone();
        let session = f.session.clone();
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || {
            let store = LearnerStore::open(&path,"checkpoint-bounds").unwrap();
            let prepared = read::prepare_snapshot_for_tool(&store.conn,&host.host_kind,&host.host_thread_id,json!({"schema":SCHEMA,"action":"status","max_bytes":65536}),read::Tool::Context).unwrap();
            let packet = read::finish_trace(&store.conn,prepared,false).unwrap();
            let revision = packet["session"]["revision"].as_i64().unwrap();
            assert_eq!(revision,2);
            let event = store.capture(&host,&[RawEvent { external_event_key: format!("work-{n}"), speaker: Speaker::Learner, kind: "message".into(), text: format!("Original concurrent work {n}: λ²"), metadata: None, occurred_at_ms: Some(100+n as i64) }],"host_observed",false).unwrap().remove(0);
            let payload = json!({"schema":SCHEMA,"action":"update","request_id":format!("checkpoint-cas-{n}"),"reported_events":[{"external_event_key":format!("extra-{n}"),"speaker":"learner","kind":"message","text":format!("Separately captured input {n}")}],"session":{"operation":"use","session_id":session,"expected_revision":revision},"checkpoint":{"current_problem":format!("Problem {n}"),"working_diagnosis":format!("Diagnosis {n}"),"next_action":format!("Intended task {n}"),"plan_text":null,"memory_refs":[],"pending_task":null},"observations":[{"client_ref":"work","kind":"learner_work","description":format!("Concurrent interpretation {n}"),"events":[{"event_id":event,"role":"learner_response"}]}],"memory_edits":[{"operation":"create","client_ref":"memory","record":{"title":format!("Concurrent memory {n}"),"scope":"algebra","summary":"Provisional interpretation","state_text":format!("Work {n} retains its original qualifications"),"current_basis":[{"new_observation":"work"}]},"reason":"Observed work","change_basis":[{"new_observation":"work"}]}],"event_dispositions":[{"event_id":event,"outcome":"processed"}]});
            // Both hosts hold revision 2 before either may attempt publication.
            barrier.wait();
            let result = store.update(&host,payload.clone());
            (n,host,event,payload,result)
        }));
    }
    let results = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        results
            .iter()
            .filter(|(_, _, _, _, result)| result.is_ok())
            .count(),
        1
    );
    let winner = results
        .iter()
        .find(|(_, _, _, _, result)| result.is_ok())
        .unwrap();
    let loser = results
        .iter()
        .find(|(_, _, _, _, result)| result.is_err())
        .unwrap();
    let error = loser.4.as_ref().unwrap_err();
    assert_eq!(error.code, "revision_conflict");
    assert_eq!(error.recovery.as_ref().unwrap()["current_revision"], 3);
    assert_eq!(
        error.recovery.as_ref().unwrap()["next_call"],
        serde_json::to_value(read::NextCall::session_status(&f.session)).unwrap()
    );
    assert_eq!(error.captured_event_ids.len(), 1);
    let count = |table: &str| {
        f.store
            .conn
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap()
    };
    assert_eq!(count("observations"), 2);
    assert_eq!(count("learner_memories"), 1);
    assert_eq!(count("memory_changes"), 1);
    assert_eq!(count("memory_fts"), 1);
    assert_eq!(count("observation_fts"), 2);
    assert_eq!(count("learning_events"), 6);
    for result in [&winner, &loser] {
        let processing: String = f
            .store
            .conn
            .query_row(
                "SELECT processing FROM learning_events WHERE event_id=?1",
                [&result.2],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            processing,
            if result.0 == winner.0 {
                "processed"
            } else {
                "pending"
            }
        );
        let receipt: i64 = f
            .store
            .conn
            .query_row(
                "SELECT count(*) FROM operation_receipts WHERE request_id=?1",
                [result.3["request_id"].as_str().unwrap()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(receipt, if result.0 == winner.0 { 1 } else { 0 });
    }
    let recovery_call = &error.recovery.as_ref().unwrap()["next_call"];
    let typed: read::NextCall = serde_json::from_value(recovery_call.clone()).unwrap();
    assert_eq!(serde_json::to_value(typed).unwrap(), *recovery_call);
    let prepared = read::prepare_snapshot_for_tool(
        &f.store.conn,
        &loser.1.host_kind,
        &loser.1.host_thread_id,
        recovery_call["arguments"].clone(),
        read::Tool::Context,
    )
    .unwrap();
    let current = read::finish_trace(&f.store.conn, prepared, false).unwrap();
    assert_eq!(current["session"]["revision"], 3);
    assert_eq!(current["session"]["checkpoint"], winner.3["checkpoint"]);
    let captured: (String, String) = f
        .store
        .conn
        .query_row(
            "SELECT text,processing FROM learning_events WHERE event_id=?1",
            [&error.captured_event_ids[0]],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        captured,
        (
            format!("Separately captured input {}", loser.0),
            "pending".into()
        )
    );
    let before_replay = canonical_state(&f.store.conn);
    assert!(f.store.replay(&winner.1, &winner.3).unwrap().is_some());
    assert_eq!(canonical_state(&f.store.conn), before_replay);
    let mut retry = loser.3.clone();
    retry["request_id"] = json!("checkpoint-cas-reconcile");
    retry["session"]["expected_revision"] = json!(3);
    let outcome = f.store.update(&loser.1, retry).unwrap();
    assert_eq!(outcome.revisions[&f.session], 4);
    assert_eq!(count("observations"), 3);
    assert_eq!(count("learner_memories"), 2);
    assert_eq!(count("learning_events"), 6);
    let reconciled = f.read("status", true);
    assert_eq!(reconciled["session"]["checkpoint"], loser.3["checkpoint"]);
    assert_eq!(reconciled["session"]["revision"], 4);
}
