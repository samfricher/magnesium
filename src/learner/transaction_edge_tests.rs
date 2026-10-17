//! Real SQLite writer boundaries for A16/A17. A commit-hook veto proves rollback
//! and fresh-connection absence of a receipt, not ambiguous-I/O durable success.
use super::*;
use rusqlite::{Connection, params, types::ValueRef};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct Fixture {
    store: LearnerStore,
    host: HostContext,
    session: String,
    root: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(store::new_id("transaction-edges").unwrap());
        std::fs::create_dir(&root).unwrap();
        let store = LearnerStore::initialize(&root.join("state.sqlite"), "edges").unwrap();
        let host = HostContext {
            host_kind: "test".into(),
            host_thread_id: "transaction-edges".into(),
        };
        let started = store.update(&host, json!({"schema":SCHEMA,"action":"update","request_id":"start","session":{"operation":"start","client_ref":"session","goal":"Algebra"}})).unwrap();
        Self {
            store,
            host,
            session: started.references["session"].clone(),
            root,
        }
    }
    fn reopen(&self) -> LearnerStore {
        LearnerStore::open(&self.root.join("state.sqlite"), "edges").unwrap()
    }
    fn event(&self, key: &str, time: i64) -> String {
        self.store
            .capture(
                &self.host,
                &[RawEvent {
                    external_event_key: key.into(),
                    speaker: crate::learner::Speaker::Learner,
                    kind: "message".into(),
                    text: format!("Original {key}: x² − 4 = 0; b is not x."),
                    metadata: Some(crate::learner::EventMetadata::default()),
                    occurred_at_ms: Some(time),
                }],
                "host_observed",
                false,
            )
            .unwrap()
            .remove(0)
    }
    fn update(&self, key: &str, edits: Value) -> UpdateResult {
        self.store
            .update(
                &self.host,
                json!({"schema":SCHEMA,"action":"update","request_id":key,"memory_edits":edits}),
            )
            .unwrap()
    }
    fn seed(&self, key: &str, time: i64) -> UpdateResult {
        let event = self.event(key, time);
        self.store
            .update(&self.host, learning(key, &event))
            .unwrap()
    }
    fn read(&self, mut request: Value) -> Value {
        request["schema"] = json!(SCHEMA);
        read::test_support::execute_with_tracing(
            &self.store.conn,
            &self.host.host_kind,
            &self.host.host_thread_id,
            request,
            false,
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Close the live connection before removing this test's private directory.
        let conn = std::mem::replace(&mut self.store.conn, Connection::open_in_memory().unwrap());
        drop(conn);
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn create(name: &str, current: Value, changes: Value) -> Value {
    json!({"operation":"create","client_ref":name,"record":{"title":name,"scope":"Algebra","summary":"Formula use works; coefficient roles remain uncertain.","state_text":"The original work supports formula application, not independent coefficient identification.","topics":["algebra"],"current_basis":current},"reason":"Explicit local reorganization","change_basis":changes})
}
fn retire(id: &str, revision: i64, replacements: Value) -> Value {
    json!({"operation":"retire","memory_id":id,"expected_revision":revision,"reason":"Explicit local reorganization","replacement_ids":replacements})
}
fn learning(key: &str, event: &str) -> Value {
    json!({"schema":SCHEMA,"action":"update","request_id":key,"turn_ref":event,"observations":[{"client_ref":"work","kind":"learner_question","description":"Applies the formula but asks about the coefficient.","conditions_text":"After a worked example","assistance_text":"Tutor demonstrated substitution","events":[{"event_id":event,"role":"learner_response","excerpts":["b is not x"]}]}],"memory_edits":[create("memory", json!([{"new_observation":"work"}]), json!([{"new_observation":"work"}]))],"event_dispositions":[{"event_id":event,"outcome":"processed"}]})
}
fn support(conn: &Connection, memory: &str) -> Vec<String> {
    conn.prepare(
        "SELECT observation_id FROM memory_observations WHERE memory_id=?1 ORDER BY observation_id",
    )
    .unwrap()
    .query_map([memory], |r| r.get(0))
    .unwrap()
    .collect::<rusqlite::Result<_>>()
    .unwrap()
}
fn sorted(ids: &[&str]) -> Vec<String> {
    let mut ids: Vec<_> = ids.iter().map(|id| (*id).to_owned()).collect();
    ids.sort();
    ids
}
/// Compare all canonical semantic rows, generations, receipts and derived index
/// rows. Raw capture is deliberately independent and is checked separately.
fn semantic_state(conn: &Connection) -> Value {
    table_states(
        conn,
        &[
            "workspace_meta",
            "learning_sessions",
            "host_bindings",
            "observations",
            "observation_events",
            "learner_memories",
            "memory_topics",
            "memory_observations",
            "memory_changes",
            "review_items",
            "operation_receipts",
            "memory_fts",
            "observation_fts",
        ],
    )
}
fn table_states(conn: &Connection, tables: &[&str]) -> Value {
    let mut result = serde_json::Map::new();
    for &table in tables {
        // FTS5's index/config shadows are WITHOUT ROWID. Their primary keys
        // occupy the first columns, giving the same deterministic export.
        let order = if table.ends_with("_fts_idx") || table.ends_with("_fts_config") {
            "1,2"
        } else {
            "rowid"
        };
        let mut statement = conn
            .prepare(&format!("SELECT * FROM {table} ORDER BY {order}"))
            .unwrap();
        let columns = statement.column_count();
        let rows: Vec<Value> = statement
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
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        result.insert(table.into(), json!(rows));
    }
    Value::Object(result)
}
fn event_status(conn: &Connection, id: &str) -> (String, Option<String>) {
    conn.query_row(
        "SELECT processing,processed_request_id FROM learning_events WHERE event_id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .unwrap()
}

#[test]
fn semantic_commit_veto_preserves_independent_capture_and_retries_from_fresh_connection() {
    let f = Fixture::new();
    let event = f.event("known-work", 100);
    let mut before = semantic_state(&f.store.conn);
    let event_generation_before: i64 = f
        .store
        .conn
        .query_row("SELECT event_generation FROM workspace_meta", [], |row| {
            row.get(0)
        })
        .unwrap();
    let mut request = learning("commit-veto", &event);
    request["reported_events"] = json!([{"external_event_key":"independent-raw","speaker":"learner","kind":"message","text":"Uninterpreted new answer: x² − 4 = 0"}]);
    request["session"] = json!({"operation":"use","session_id":f.session,"expected_revision":1});
    request["checkpoint"] = json!({"current_problem":"Identify b","working_diagnosis":"Coefficient roles uncertain","next_action":"Contrast b and x","memory_refs":[{"memory_id":"memory","revision":"after_edit"}]});
    request["review_updates"] = json!([{"operation":"create","client_ref":"review","memory_id":"memory","purpose":"Revisit labels","reason":"Work remains uncertain"}]);

    let commits = Arc::new(AtomicUsize::new(0));
    let count = commits.clone();
    f.store
        .conn
        .commit_hook(Some(move || {
            // The first write transaction is raw capture, the second is the
            // complete semantic batch, including its in-transaction receipt.
            count.fetch_add(1, Ordering::SeqCst) == 1
        }))
        .unwrap();
    let rollbacks = Arc::new(AtomicUsize::new(0));
    let count = rollbacks.clone();
    f.store
        .conn
        .rollback_hook(Some(move || {
            count.fetch_add(1, Ordering::SeqCst);
        }))
        .unwrap();
    let error = f.store.update(&f.host, request.clone()).unwrap_err();
    assert_eq!(error.code, "storage_error");
    assert_eq!(commits.load(Ordering::SeqCst), 2);
    assert_eq!(rollbacks.load(Ordering::SeqCst), 1);
    assert!(f.store.conn.is_autocommit());

    let fresh = f.reopen();
    let generation_after_capture: i64 = fresh
        .conn
        .query_row("SELECT event_generation FROM workspace_meta", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(generation_after_capture, event_generation_before + 1);
    let event_generation_column: i64 = fresh
        .conn
        .query_row(
            "SELECT cid FROM pragma_table_info('workspace_meta') WHERE name='event_generation'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    // The independent raw transaction did commit exactly one new event batch.
    // Model that precise metadata advance; all semantic fields, other versions,
    // receipts and index rows must still match their pre-semantic values.
    before["workspace_meta"][0][event_generation_column as usize] = json!(generation_after_capture);
    assert_eq!(semantic_state(&fresh.conn), before);
    assert!(fresh.replay(&f.host, &request).unwrap().is_none());
    let raw: (String, String, String) = fresh.conn.query_row("SELECT event_id,text,origin FROM learning_events WHERE external_event_key='independent-raw'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
    assert_eq!(raw.1, "Uninterpreted new answer: x² − 4 = 0");
    assert_eq!(raw.2, "model_reported");
    assert_eq!(error.captured_event_ids, vec![raw.0.clone(), event.clone()]);
    for id in [&raw.0, &event] {
        assert_eq!(event_status(&fresh.conn, id), ("pending".into(), None));
    }
    let retried = fresh.update(&f.host, request.clone()).unwrap();
    assert_eq!(retried.update_status, "committed");
    assert_eq!(retried.captured_event_ids, error.captured_event_ids);
    assert_eq!(
        event_status(&fresh.conn, &event),
        ("processed".into(), Some("commit-veto".into()))
    );
    assert_eq!(event_status(&fresh.conn, &raw.0), ("pending".into(), None));
    let after = semantic_state(&fresh.conn);
    let replay = fresh.update(&f.host, request).unwrap();
    assert_eq!(
        serde_json::to_value(retried).unwrap(),
        serde_json::to_value(replay).unwrap()
    );
    assert_eq!(semantic_state(&fresh.conn), after);
    assert_eq!(
        fresh
            .conn
            .query_row("SELECT count(*) FROM learning_events", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn sqlite_full_rolls_back_semantics_without_turning_storage_failure_into_index_warning() {
    let f = Fixture::new();
    let event = f.event("full-work", 100);
    let before = semantic_state(&f.store.conn);
    let pages: i64 = f
        .store
        .conn
        .pragma_query_value(None, "page_count", |r| r.get(0))
        .unwrap();
    assert_eq!(
        f.store
            .conn
            .pragma_query_value(None, "freelist_count", |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    f.store
        .conn
        .pragma_update(None, "max_page_count", pages)
        .unwrap();
    let mut request = learning("database-full", &event);
    // Within the real field limit, but necessarily larger than a SQLite page.
    request["memory_edits"][0]["record"]["state_text"] = json!("x".repeat(limits::STATE_BYTES));
    let error = f.store.update(&f.host, request.clone()).unwrap_err();
    assert_eq!(error.code, "storage_error");
    assert!(error.message.contains("full"), "{error}");
    assert_eq!(error.captured_event_ids, vec![event.clone()]);
    assert!(f.store.conn.is_autocommit());
    let fresh = f.reopen();
    assert_eq!(semantic_state(&fresh.conn), before);
    assert_eq!(event_status(&fresh.conn, &event), ("pending".into(), None));
    assert!(fresh.replay(&f.host, &request).unwrap().is_none());
    f.store
        .conn
        .pragma_update(None, "max_page_count", 1_073_741_823)
        .unwrap();
    assert_eq!(
        f.store.update(&f.host, request).unwrap().update_status,
        "committed"
    );
}

#[test]
fn sqlite_corrupt_fts_rolls_back_semantics_and_preserves_independent_raw_capture() {
    let f = Fixture::new();
    let seeded = f.seed("initial-memory", 100);
    let memory = &seeded.references["memory"];
    let pending = f.event("pending-work", 200);
    let rowid: i64 = f
        .store
        .conn
        .query_row(
            "SELECT memory_no FROM learner_memories WHERE memory_id=?1",
            [memory],
            |r| r.get(0),
        )
        .unwrap();
    // FTS5 shadow row 1 holds aggregate document/token sizes. This truncated
    // real record makes ordinary FTS maintenance return SQLITE_CORRUPT_VTAB;
    // no mocked result, authorizer denial, custom VFS or runtime fault hook.
    assert_eq!(
        f.store
            .conn
            .execute("UPDATE memory_fts_data SET block=x'00' WHERE id=1", [],)
            .unwrap(),
        1
    );
    // Reopen to avoid the original connection's FTS aggregate-size cache.
    // Prove the SQLite error class at the exact maintenance boundary, then
    // roll back the probe so it cannot supply any of the semantic assertions.
    let probe = f.reopen();
    let tx = probe.conn.unchecked_transaction().unwrap();
    let error = tx
        .execute("DELETE FROM memory_fts WHERE rowid=?1", [rowid])
        .unwrap_err();
    match error {
        rusqlite::Error::SqliteFailure(code, _) => {
            assert_eq!(code.code, rusqlite::ErrorCode::DatabaseCorrupt);
            assert_eq!(code.extended_code, rusqlite::ffi::SQLITE_CORRUPT_VTAB);
        }
        other => panic!("Expected genuine SQLite FTS corruption: {other}"),
    }
    tx.rollback().unwrap();
    drop(probe);

    let writer = f.reopen();
    let mut before = semantic_state(&writer.conn);
    let generation: i64 = writer
        .conn
        .query_row("SELECT event_generation FROM workspace_meta", [], |r| {
            r.get(0)
        })
        .unwrap();
    let generation_column: i64 = writer
        .conn
        .query_row(
            "SELECT cid FROM pragma_table_info('workspace_meta') WHERE name='event_generation'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    // The only expected protected-state difference is the independently
    // committed raw capture generation, never an index-dirty semantic commit.
    before["workspace_meta"][0][generation_column as usize] = json!(generation + 1);
    let shadow_tables = [
        "memory_fts_data",
        "memory_fts_idx",
        "memory_fts_content",
        "memory_fts_docsize",
        "memory_fts_config",
        "observation_fts_data",
        "observation_fts_idx",
        "observation_fts_content",
        "observation_fts_docsize",
        "observation_fts_config",
    ];
    let shadows_before = table_states(&writer.conn, &shadow_tables);
    let mut request = learning("corrupt-index", &pending);
    request["reported_events"] = json!([{"external_event_key":"corruption-raw","speaker":"learner","kind":"message","text":"Original uncategorized answer: π is fixed; x varies."}]);
    request["session"] = json!({"operation":"use","session_id":f.session,"expected_revision":1});
    request["checkpoint"] = json!({"current_problem":"Identify b","working_diagnosis":"New work needs interpretation","next_action":"Contrast b and x","memory_refs":[{"memory_id":memory,"revision":"after_edit"}]});
    request["review_updates"] = json!([{"operation":"create","client_ref":"review","memory_id":memory,"purpose":"Revisit labels","reason":"Different conditions"}]);
    // One existing projection is the first and only memory maintenance target.
    // A preceding new-memory insert could repair FTS aggregate sizes before
    // the corrupt delete, depending on randomly allocated ID sorting.
    request["memory_edits"] = json!([{"operation":"flag","memory_id":memory,"expected_revision":1,"review_note":"Retain uncertainty","reason":"New response","change_basis":[{"new_observation":"work"}]}]);
    let error = writer.update(&f.host, request.clone()).unwrap_err();
    assert_eq!(error.code, "storage_error");
    assert!(error.message.contains("malformed"), "{error}");
    assert!(writer.conn.is_autocommit());
    let fresh = f.reopen();
    assert_eq!(semantic_state(&fresh.conn), before);
    assert_eq!(table_states(&fresh.conn, &shadow_tables), shadows_before);
    assert!(fresh.replay(&f.host, &request).unwrap().is_none());
    let raw: (String, String, String) = fresh.conn.query_row(
        "SELECT event_id,text,origin FROM learning_events WHERE external_event_key='corruption-raw'", [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ).unwrap();
    assert_eq!(
        raw.1,
        "Original uncategorized answer: π is fixed; x varies."
    );
    assert_eq!(raw.2, "model_reported");
    assert_eq!(
        error.captured_event_ids,
        vec![raw.0.clone(), pending.clone()]
    );
    for id in [&raw.0, &pending] {
        assert_eq!(event_status(&fresh.conn, id), ("pending".into(), None));
    }
    assert_eq!(
        fresh
            .conn
            .query_row("SELECT block FROM memory_fts_data WHERE id=1", [], |r| r
                .get::<_, Vec<u8>>(0),)
            .unwrap(),
        vec![0]
    );
    // A repeated rejected payload deduplicates independent raw input and still
    // fails fatally; neither a success receipt nor extra generation can appear.
    // Each corruption oracle opens a fresh FTS instance rather than relying
    // on connection-local aggregate caches after a failed maintenance attempt.
    let retry = f.reopen();
    let repeated = retry.update(&f.host, request).unwrap_err();
    assert_eq!(repeated.code, "storage_error");
    assert_eq!(repeated.captured_event_ids, error.captured_event_ids);
    assert_eq!(semantic_state(&retry.conn), before);
    assert_eq!(table_states(&retry.conn, &shadow_tables), shadows_before);
    assert_eq!(
        retry
            .conn
            .query_row(
                "SELECT count(*) FROM learning_events WHERE external_event_key='corruption-raw'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
}

#[test]
fn writer_merge_then_split_preserves_authored_support_and_direct_retirement_notices() {
    let f = Fixture::new();
    let initial = f.seed("first-work", 100);
    let a = &initial.references["memory"];
    let historical = &initial.references["work"];
    let revised_event = f.event("later-work", 200);
    let mut revise = learning("revise-first", &revised_event);
    let mut edit = revise["memory_edits"][0].clone();
    edit.as_object_mut().unwrap().remove("client_ref");
    edit["operation"] = json!("revise");
    edit["memory_id"] = json!(a);
    edit["expected_revision"] = json!(1);
    edit["change_kind"] = json!("new_observation");
    revise["memory_edits"][0] = edit;
    let current = f.store.update(&f.host, revise).unwrap();
    let current_a = &current.references["work"];
    let second = f.seed("second-work", 300);
    let b = &second.references["memory"];
    let current_b = &second.references["work"];
    let all_support = sorted(&[historical, current_a, current_b]);
    let changes = json!([{"observation_id":historical},{"observation_id":current_a},{"observation_id":current_b}]);
    let merged = f.update(
        "merge",
        json!([
            retire(a, 2, json!(["combined"])),
            retire(b, 1, json!(["combined"])),
            create(
                "combined",
                json!([{"observation_id":current_b},{"observation_id":current_a}]),
                changes.clone()
            )
        ]),
    );
    let combined = &merged.references["combined"];
    assert_eq!(merged.revisions[a], 3);
    assert_eq!(merged.revisions[b], 2);
    assert_eq!(merged.revisions[combined], 1);
    let snapshot = update::memory_snapshot(&f.store.conn, combined).unwrap();
    assert_eq!(snapshot["current_basis"], json!([current_b, current_a]));
    assert_eq!(snapshot["last_observed_at_ms"], 300);
    assert_eq!(support(&f.store.conn, combined), all_support);
    assert_eq!(support(&f.store.conn, a), sorted(&[historical, current_a]));
    assert_eq!(support(&f.store.conn, b), sorted(&[current_b]));

    let original_a = update::memory_snapshot(&f.store.conn, a).unwrap();
    let original_b = update::memory_snapshot(&f.store.conn, b).unwrap();
    let split = f.update(
        "split",
        json!([
            retire(combined, 1, json!(["formula", "labels"])),
            create(
                "formula",
                json!([{"observation_id":current_a}]),
                changes.clone()
            ),
            create("labels", json!([{"observation_id":current_b}]), changes)
        ]),
    );
    let formula = &split.references["formula"];
    let labels = &split.references["labels"];
    assert_eq!(split.revisions[combined], 2);
    assert_eq!(split.revisions[formula], 1);
    assert_eq!(split.revisions[labels], 1);
    for (id, basis) in [(formula, current_a), (labels, current_b)] {
        assert_eq!(
            update::memory_snapshot(&f.store.conn, id).unwrap()["current_basis"],
            json!([basis])
        );
        assert_eq!(support(&f.store.conn, id), all_support);
    }
    // No recursive rewrite of retired ancestors when their direct replacement
    // is later retired. The writer only applies the requested local changes.
    assert_eq!(
        update::memory_snapshot(&f.store.conn, a).unwrap(),
        original_a
    );
    assert_eq!(
        update::memory_snapshot(&f.store.conn, b).unwrap(),
        original_b
    );
    let read = f.read(json!({"action":"read","refs":[{"kind":"current_memory","id":a,"expected_revision":2},{"kind":"current_memory","id":combined,"expected_revision":1}]}));
    for record in read["records"].as_array().unwrap() {
        assert_eq!(record["lifecycle"], "retired");
        assert_eq!(record["detail_status"], "retired");
        assert_eq!(record["revision_changed"], true);
        assert!(record["summary"].is_null());
        assert!(record["state_text"].is_null());
    }
    assert_eq!(read["records"][0]["replacement_ids"], json!([combined]));
    assert_eq!(
        read["records"][1]["replacement_ids"],
        json!([formula, labels])
    );
    let prepared = f.read(json!({"action":"prepare","known_revisions":[{"memory_id":a,"revision":2},{"memory_id":combined,"revision":1}]}));
    assert_eq!(prepared["revision_notices"][0]["status"], "retired");
    assert_eq!(
        prepared["revision_notices"][0]["replacement_ids"],
        json!([combined])
    );
    assert_eq!(
        prepared["revision_notices"][1]["replacement_ids"],
        json!([formula, labels])
    );
    assert_eq!(prepared["coverage"]["active_memory_count"], 2);
    let active = prepared["memories"].as_array().unwrap();
    assert_eq!(active.len(), 2);
    assert!(
        active.iter().all(|m| m["lifecycle"] == "active"
            && (m["memory_id"] == *formula || m["memory_id"] == *labels))
    );

    let history = f.read(json!({"action":"history","memory_id":a}));
    assert_eq!(history["history"].as_array().unwrap().len(), 3);
    assert!(
        history["history"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["historical"] == true)
    );
    assert_eq!(
        history["history"][2]["snapshot"]["current_basis"],
        json!([historical])
    );
    assert_eq!(
        history["history"][1]["snapshot"]["current_basis"],
        json!([current_a])
    );
    let merged_history = f.read(json!({"action":"history","memory_id":combined}));
    assert_eq!(
        merged_history["history"][1]["change_basis"],
        json!([historical, current_a, current_b])
    );
    assert_eq!(
        merged_history["history"][1]["snapshot"]["current_basis"],
        json!([current_b, current_a])
    );
    let observation =
        f.read(json!({"action":"read","refs":[{"kind":"observation","id":historical}]}));
    assert_eq!(
        observation["records"][0]["conditions_text"],
        "After a worked example"
    );
    assert_eq!(
        observation["records"][0]["assistance_text"],
        "Tutor demonstrated substitution"
    );
    assert_eq!(
        f.store
            .conn
            .query_row("SELECT count(*) FROM observations", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        3
    );
}

#[test]
fn invalid_final_replacements_roll_back_entire_local_reorganization() {
    for invalid in ["missing", "self", "retired-in-same-batch"] {
        let f = Fixture::new();
        let first = f.seed("first", 100);
        let second = f.seed("second", 200);
        let a = &first.references["memory"];
        let b = &second.references["memory"];
        let basis = json!([{"observation_id":first.references["work"]}]);
        let before = semantic_state(&f.store.conn);
        let replacement = match invalid {
            "missing" => "missing-memory",
            "self" => a,
            _ => b,
        };
        let request = json!({"schema":SCHEMA,"action":"update","request_id":"bad-replacement","memory_edits":[
            create("candidate", basis.clone(), basis),
            retire(b, 1, json!([])),
            retire(a, 1, json!([replacement]))
        ]});
        let error = f.store.update(&f.host, request.clone()).unwrap_err();
        assert_eq!(error.code, "unknown_reference", "{invalid}: {error}");
        let fresh = f.reopen();
        assert_eq!(semantic_state(&fresh.conn), before, "{invalid}");
        assert!(fresh.replay(&f.host, &request).unwrap().is_none());
    }
}

#[test]
fn merge_cas_conflict_rolls_back_prior_edits_and_reconciles_once_with_new_key() {
    let f = Fixture::new();
    let first = f.seed("first", 100);
    let second = f.seed("second", 200);
    let a = &first.references["memory"];
    let b = &second.references["memory"];
    let basis = json!([{"observation_id":first.references["work"]},{"observation_id":second.references["work"]}]);
    // A separate writer advances one of the two memories after it was read.
    let other = f.reopen();
    other.update(&f.host, json!({"schema":SCHEMA,"action":"update","request_id":"concurrent-flag","memory_edits":[{"operation":"flag","memory_id":b,"expected_revision":1,"review_note":"Check this interpretation","reason":"Reconsider original support","change_basis":basis}]})).unwrap();
    let before = semantic_state(&f.store.conn);
    let mut request = json!({"schema":SCHEMA,"action":"update","request_id":"stale-merge","memory_edits":[
        create("combined", basis.clone(), basis),
        retire(a, 1, json!(["combined"])),
        retire(b, 1, json!(["combined"]))
    ]});
    let error = f.store.update(&f.host, request.clone()).unwrap_err();
    assert_eq!(error.code, "revision_conflict");
    assert_eq!(error.recovery.as_ref().unwrap()["current_revision"], 2);
    assert_eq!(semantic_state(&other.conn), before);
    assert!(other.replay(&f.host, &request).unwrap().is_none());
    request["request_id"] = json!("reconciled-merge");
    request["memory_edits"][2]["expected_revision"] = json!(2);
    request["memory_edits"][0]["record"]["review_note"] = json!("Check this interpretation");
    let result = f.store.update(&f.host, request.clone()).unwrap();
    assert_eq!(result.revisions[a], 2);
    assert_eq!(result.revisions[b], 3);
    let combined = &result.references["combined"];
    assert_eq!(
        update::memory_snapshot(&other.conn, combined).unwrap()["review_note"],
        "Check this interpretation"
    );
    let after = semantic_state(&other.conn);
    assert_eq!(
        f.store.update(&f.host, request).unwrap().references,
        result.references
    );
    assert_eq!(semantic_state(&other.conn), after);
    assert_eq!(
        other
            .conn
            .query_row(
                "SELECT count(*) FROM operation_receipts WHERE request_id=?1",
                params!["stale-merge"],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
}

#[test]
fn fresh_commit_recovery_requires_a_durable_matching_receipt() {
    let f = Fixture::new();
    let event = f.event("recovery-work", 100);
    let request = learning("recover-receipt", &event);
    let hash = update::payload_hash(&request).unwrap();
    assert!(
        f.store
            .recover_commit_receipt(&f.host, "recover-receipt", &hash)
            .unwrap()
            .is_none()
    );
    let committed = f.store.update(&f.host, request.clone()).unwrap();
    let before = semantic_state(&f.store.conn);
    let recovered = f
        .store
        .recover_commit_receipt(&f.host, "recover-receipt", &hash)
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::to_value(&recovered).unwrap(),
        serde_json::to_value(&committed).unwrap()
    );
    let mut different_payload = request;
    different_payload["memory_edits"][0]["reason"] = json!("A different request with the same key");
    assert!(
        f.store
            .recover_commit_receipt(
                &f.host,
                "recover-receipt",
                &update::payload_hash(&different_payload).unwrap(),
            )
            .unwrap()
            .is_none()
    );
    // A real uncommitted change is visible on the original connection only.
    // Recovery must obtain the writer-issued durable receipt from a fresh one.
    let tx = f.store.conn.unchecked_transaction().unwrap();
    tx.execute(
        "DELETE FROM operation_receipts WHERE request_id='recover-receipt'",
        [],
    )
    .unwrap();
    let recovered = f
        .store
        .recover_commit_receipt(&f.host, "recover-receipt", &hash)
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::to_value(recovered).unwrap(),
        serde_json::to_value(committed).unwrap()
    );
    tx.rollback().unwrap();
    assert_eq!(semantic_state(&f.store.conn), before);
}

#[test]
fn fresh_commit_recovery_rejects_same_payload_receipt_from_another_host() {
    for (kind, thread) in [
        ("other-kind", "transaction-edges"),
        ("test", "other-thread"),
    ] {
        let f = Fixture::new();
        let request = json!({"schema":SCHEMA,"action":"update","request_id":"shared-key"});
        let hash = update::payload_hash(&request).unwrap();
        assert!(
            f.store
                .recover_commit_receipt(&f.host, "shared-key", &hash)
                .unwrap()
                .is_none()
        );
        let other_host = HostContext {
            host_kind: kind.into(),
            host_thread_id: thread.into(),
        };
        let other = f.reopen();
        other.update(&other_host, json!({"schema":SCHEMA,"action":"update","request_id":"other-start","session":{"operation":"start","client_ref":"session","goal":"Separate host"}})).unwrap();
        // This represents B committing after A's failed transaction releases
        // its lock and before A's recovery read. No hardware-I/O ambiguity is
        // simulated, and the receipt itself is produced by the real writer.
        let committed = other.update(&other_host, request.clone()).unwrap();
        let before = semantic_state(&other.conn);
        assert_eq!(committed.host_kind, other_host.host_kind);
        assert_eq!(committed.host_thread_id, other_host.host_thread_id);
        let error = f
            .store
            .recover_commit_receipt(&f.host, "shared-key", &hash)
            .unwrap_err();
        assert_eq!(error.code, "request_key_conflict");
        assert!(error.message.contains("different host conversation"));
        assert_eq!(
            f.store.replay(&f.host, &request).unwrap_err().code,
            error.code
        );
        assert_eq!(semantic_state(&other.conn), before);
        let owner_result = other
            .recover_commit_receipt(&other_host, "shared-key", &hash)
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::to_value(owner_result).unwrap(),
            serde_json::to_value(committed).unwrap()
        );
    }
}
