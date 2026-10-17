//! Real runtime commands must have unambiguous batch-local identities.
use super::*;
use rusqlite::{Connection, types::ValueRef};
use serde_json::{Value, json};

struct Fixture {
    store: LearnerStore,
    host: HostContext,
    session: String,
    root: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(store::new_id("batch-keys").unwrap());
        std::fs::create_dir(&root).unwrap();
        let store = LearnerStore::initialize(&root.join("state.sqlite"), "batch-keys").unwrap();
        let host = HostContext {
            host_kind: "codex".into(),
            host_thread_id: "batch-keys".into(),
        };
        let started = store.update(&host, json!({"schema":SCHEMA,"action":"update","request_id":"start","session":{"operation":"start","client_ref":"session","goal":"Coefficient roles"}})).unwrap();
        Self {
            store,
            host,
            session: started.references["session"].clone(),
            root,
        }
    }
    fn update(&self, mut request: Value) -> Result<UpdateResult> {
        request["schema"] = json!(SCHEMA);
        request["action"] = json!("update");
        self.store.update(&self.host, request)
    }
    fn event(&self, key: &str) -> String {
        self.store
            .capture(
                &self.host,
                &[RawEvent {
                    external_event_key: key.into(),
                    speaker: Speaker::Learner,
                    kind: "message".into(),
                    text: "b is fixed; x varies.".into(),
                    metadata: None,
                    occurred_at_ms: Some(42),
                }],
                "host_observed",
                false,
            )
            .unwrap()
            .remove(0)
    }
    fn seed(&self) -> UpdateResult {
        let event = self.event("initial-work");
        let mut request = learning("initial-memory", &event, "memory");
        request["session"] =
            json!({"operation":"use","session_id":self.session,"expected_revision":1});
        request["checkpoint"] = checkpoint("memory", json!("after_edit"));
        request["review_updates"] = json!([review("review", "memory")]);
        self.update(request).unwrap()
    }
    fn rejection(&self, request: Value, path: &str) -> LearnerError {
        self.rejection_with_code(request, path, "invalid_variant")
    }
    fn rejection_with_code(&self, request: Value, path: &str, code: &str) -> LearnerError {
        let error = self.update(request).unwrap_err();
        assert_eq!(error.code, code, "{error:?}");
        assert_eq!(
            error.recovery.as_ref().unwrap()["field_path"],
            path,
            "{error:?}"
        );
        assert!(error.message.contains(path));
        error
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let conn = std::mem::replace(&mut self.store.conn, Connection::open_in_memory().unwrap());
        drop(conn);
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn observation(name: &str, event: &str) -> Value {
    json!({"client_ref":name,"kind":"learner_work","description":"The learner distinguishes fixed b from variable x.","events":[{"event_id":event,"role":"learner_response"}]})
}
fn create(name: &str, basis: Value) -> Value {
    json!({"operation":"create","client_ref":name,"record":{"title":"Coefficient roles","scope":"algebra","summary":"Fixed coefficient identified in this work.","state_text":"The learner distinguishes b from x in this prompted response.","current_basis":basis},"reason":"Observed learner work","change_basis":basis})
}
fn review(name: &str, memory: &str) -> Value {
    json!({"operation":"create","client_ref":name,"memory_id":memory,"purpose":"Revisit coefficient roles","reason":"Check under different conditions"})
}
fn checkpoint(memory: &str, revision: Value) -> Value {
    json!({"current_problem":"Coefficient roles","working_diagnosis":"Identification in prompted work","next_action":"Contrast fixed labels and variables","memory_refs":[{"memory_id":memory,"revision":revision}]})
}
fn learning(key: &str, event: &str, memory: &str) -> Value {
    json!({"request_id":key,"turn_ref":event,"observations":[observation("work",event)],"memory_edits":[create(memory,json!([{"new_observation":"work"}]))],"event_dispositions":[{"event_id":event,"outcome":"processed"}]})
}
fn event_status(conn: &Connection, id: &str) -> (String, Option<String>) {
    conn.query_row(
        "SELECT processing,processed_request_id FROM learning_events WHERE event_id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .unwrap()
}
/// Every semantic canonical/projection row, receipt, control and generation is
/// compared. Only raw capture's event_generation is separated for exact checks.
fn semantic_state(conn: &Connection) -> Value {
    let mut result = serde_json::Map::new();
    for table in [
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
        "context_receipts",
        "memory_fts",
        "observation_fts",
    ] {
        let mut statement = conn
            .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
            .unwrap();
        let columns: Vec<_> = statement
            .column_names()
            .into_iter()
            .map(str::to_owned)
            .collect();
        let rows: Vec<Value> = statement
            .query_map([], |row| {
                let mut cells = serde_json::Map::new();
                for (column, name) in columns.iter().enumerate() {
                    if table == "workspace_meta" && name == "event_generation" {
                        continue;
                    }
                    let value = match row.get_ref(column)? {
                        ValueRef::Null => Value::Null,
                        ValueRef::Integer(v) => json!(v),
                        ValueRef::Real(v) => json!(v),
                        ValueRef::Text(v) => json!(std::str::from_utf8(v).unwrap()),
                        ValueRef::Blob(v) => json!(v),
                    };
                    cells.insert(name.clone(), value);
                }
                Ok(Value::Object(cells))
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        result.insert(table.into(), json!(rows));
    }
    Value::Object(result)
}
fn event_generation(conn: &Connection) -> i64 {
    conn.query_row("SELECT event_generation FROM workspace_meta", [], |r| {
        r.get(0)
    })
    .unwrap()
}

fn duplicate_dispositions(outcomes: [&str; 2]) {
    let f = Fixture::new();
    let event = f.event("pending");
    let before = semantic_state(&f.store.conn);
    let generation = event_generation(&f.store.conn);
    let mut request = learning("duplicate-disposition", &event, "memory");
    request["event_dispositions"] =
        json!([{"event_id":event,"outcome":outcomes[0]},{"event_id":event,"outcome":outcomes[1]}]);
    let error = f.rejection(request, "event_dispositions[1].event_id");
    assert!(error.message.contains("event_dispositions[0].event_id"));
    assert_eq!(semantic_state(&f.store.conn), before);
    assert_eq!(event_generation(&f.store.conn), generation);
    assert_eq!(
        event_status(&f.store.conn, &event),
        ("pending".into(), None)
    );
}

#[test]
fn batch_keys_dispositions_processed_then_no_change_are_rejected() {
    duplicate_dispositions(["processed", "no_change"]);
}
#[test]
fn batch_keys_dispositions_no_change_then_processed_are_rejected() {
    duplicate_dispositions(["no_change", "processed"]);
}
#[test]
fn batch_keys_identical_processed_dispositions_are_rejected() {
    duplicate_dispositions(["processed", "processed"]);
}
#[test]
fn batch_keys_identical_no_change_dispositions_are_rejected() {
    duplicate_dispositions(["no_change", "no_change"]);
}

fn runtime_memory_alias(consumer: &str) {
    let f = Fixture::new();
    let seeded = f.seed();
    let original = &seeded.references["memory"];
    let pending = f.event("new-work");
    let mut request = learning("shadow-existing-memory", &pending, original);
    match consumer {
        "checkpoint" => {
            request["session"] =
                json!({"operation":"use","session_id":f.session,"expected_revision":2});
            request["checkpoint"] = checkpoint(original, json!(1));
        }
        "review" => request["review_updates"] = json!([review("new-review", original)]),
        "retirement" => {
            let another = f.event("other-work");
            let retired = f
                .update(learning("other-memory", &another, "other"))
                .unwrap();
            request["memory_edits"].as_array_mut().unwrap().push(json!({"operation":"retire","memory_id":retired.references["other"],"expected_revision":1,"reason":"Consolidate overlapping observations","replacement_ids":[original]}));
        }
        _ => unreachable!(),
    }
    let before = semantic_state(&f.store.conn);
    let generation = event_generation(&f.store.conn);
    f.rejection(request, "memory_edits[0].client_ref");
    assert_eq!(semantic_state(&f.store.conn), before, "{consumer}");
    assert_eq!(event_generation(&f.store.conn), generation);
    assert_eq!(
        event_status(&f.store.conn, &pending),
        ("pending".into(), None)
    );
    assert_eq!(
        update::memory_snapshot(&f.store.conn, original).unwrap()["revision"],
        1
    );
}

#[test]
fn batch_keys_runtime_memory_alias_cannot_shadow_checkpoint() {
    runtime_memory_alias("checkpoint");
}
#[test]
fn batch_keys_runtime_memory_alias_cannot_shadow_review() {
    runtime_memory_alias("review");
}
#[test]
fn batch_keys_runtime_memory_alias_cannot_shadow_retirement() {
    runtime_memory_alias("retirement");
}

#[test]
fn batch_keys_distinct_explicit_dispositions_leave_unselected_support_pending() {
    let f = Fixture::new();
    let processed = f.event("processed");
    let unchanged = f.event("unchanged");
    let support_only = f.event("support-only");
    let mut request = learning("distinct-dispositions", &processed, "memory");
    request["observations"][0]["events"]
        .as_array_mut()
        .unwrap()
        .push(json!({"event_id":support_only,"role":"task_context"}));
    request["event_dispositions"] = json!([{"event_id":processed,"outcome":"processed"},{"event_id":unchanged,"outcome":"no_change"}]);
    f.update(request).unwrap();
    assert_eq!(
        event_status(&f.store.conn, &processed),
        ("processed".into(), Some("distinct-dispositions".into()))
    );
    assert_eq!(
        event_status(&f.store.conn, &unchanged),
        ("no_change".into(), Some("distinct-dispositions".into()))
    );
    assert_eq!(
        event_status(&f.store.conn, &support_only),
        ("pending".into(), None)
    );
}

#[test]
fn batch_keys_cross_kind_and_same_kind_names_have_indexed_diagnostics() {
    // Definitions are visited in protocol order. Exhaust every cross-kind pair
    // and repeated observation/memory/review definitions with fixed permutations.
    let paths = [
        "session.client_ref",
        "observations[0].client_ref",
        "memory_edits[0].client_ref",
        "review_updates[0].client_ref",
    ];
    let pointers = [
        "/session/client_ref",
        "/observations/0/client_ref",
        "/memory_edits/0/client_ref",
        "/review_updates/0/client_ref",
    ];
    for first in 0..4 {
        for second in first + 1..4 {
            let f = Fixture::new();
            let event = f.event("support");
            let mut request = learning("cross-kind", &event, "memory");
            request["session"] =
                json!({"operation":"start","client_ref":"new-session","goal":"Contrast labels"});
            request["review_updates"] = json!([review("new-review", "memory")]);
            *request.pointer_mut(pointers[first]).unwrap() = json!("same-name");
            *request.pointer_mut(pointers[second]).unwrap() = json!("same-name");
            let before = semantic_state(&f.store.conn);
            let error = f.rejection(request, paths[second]);
            assert!(error.message.contains(paths[first]));
            assert_eq!(semantic_state(&f.store.conn), before);
        }
    }
    for section in ["observations", "memory_edits", "review_updates"] {
        let f = Fixture::new();
        let event = f.event("support");
        let mut request = learning("same-kind", &event, "memory");
        request["review_updates"] = json!([review("new-review", "memory")]);
        let first = request[section][0].clone();
        request[section].as_array_mut().unwrap().push(first);
        let error = f.rejection(request, &format!("{section}[1].client_ref"));
        assert!(error.message.contains(&format!("{section}[0].client_ref")));
    }
}

#[test]
fn batch_keys_all_local_definitions_are_bounded_and_reserve_runtime_shapes() {
    let pointers = [
        "/session/client_ref",
        "/observations/0/client_ref",
        "/memory_edits/0/client_ref",
        "/review_updates/0/client_ref",
    ];
    let paths = [
        "session.client_ref",
        "observations[0].client_ref",
        "memory_edits[0].client_ref",
        "review_updates[0].client_ref",
    ];
    let mut rejected = vec![
        String::new(),
        "x".repeat(limits::REFERENCE_BYTES + 1),
        "é".repeat(limits::REFERENCE_BYTES / 2 + 1),
    ];
    for prefix in ["e", "o", "m", "s", "r"] {
        rejected.push(format!("{prefix}-{}", "a".repeat(32)));
        rejected.push(format!("{prefix}-{}", "ABCDEF0123456789".repeat(2)));
    }
    for (pointer, path) in pointers.into_iter().zip(paths) {
        for name in &rejected {
            let f = Fixture::new();
            let event = f.event("support");
            let mut request = learning("invalid-local", &event, "memory");
            request["session"] =
                json!({"operation":"start","client_ref":"new-session","goal":"Contrast labels"});
            request["review_updates"] = json!([review("new-review", "memory")]);
            *request.pointer_mut(pointer).unwrap() = json!(name);
            let before = semantic_state(&f.store.conn);
            f.rejection_with_code(
                request,
                path,
                if name.len() > limits::REFERENCE_BYTES {
                    "field_too_large"
                } else {
                    "invalid_variant"
                },
            );
            assert_eq!(semantic_state(&f.store.conn), before);
        }
    }
}

#[test]
fn batch_keys_existing_memory_and_review_targets_reject_before_revision_cas() {
    for target in ["memory", "review"] {
        let f = Fixture::new();
        let seed = f.seed();
        let event = f.event("later-work");
        let request = if target == "memory" {
            json!({"request_id":"duplicate-memory","memory_edits":[{"operation":"flag","memory_id":seed.references["memory"],"expected_revision":1,"review_note":"Check labels again","reason":"New work","change_basis":[{"event_id":event}]},{"operation":"retire","memory_id":seed.references["memory"],"expected_revision":1,"reason":"Reorganize"}]})
        } else {
            let edit = json!({"operation":"update","review_id":seed.references["review"],"expected_revision":1,"status":"dismissed"});
            json!({"request_id":"duplicate-review","review_updates":[edit.clone(),edit]})
        };
        let before = semantic_state(&f.store.conn);
        let error = f.rejection(
            request,
            if target == "memory" {
                "memory_edits[1].memory_id"
            } else {
                "review_updates[1].review_id"
            },
        );
        assert!(error.message.contains(if target == "memory" {
            "memory_edits[0].memory_id"
        } else {
            "review_updates[0].review_id"
        }));
        assert_eq!(semantic_state(&f.store.conn), before);
    }
}

#[test]
fn batch_keys_preflight_rejects_before_writer_lock_and_bounds_every_array() {
    let f = Fixture::new();
    let other = Connection::open(f.root.join("state.sqlite")).unwrap();
    other.execute_batch("BEGIN IMMEDIATE").unwrap();
    f.rejection(json!({"request_id":"pre-lock","event_dispositions":[{"event_id":"not-in-store","outcome":"no_change"},{"event_id":"not-in-store","outcome":"no_change"}]}),"event_dispositions[1].event_id");
    for (section, limit, item) in [
        (
            "observations",
            limits::OBSERVATIONS_PER_BATCH,
            observation("work", "unknown"),
        ),
        (
            "memory_edits",
            limits::MEMORY_EDITS_PER_BATCH,
            create("memory", json!([{"new_observation":"work"}])),
        ),
        (
            "review_updates",
            limits::REVIEWS_PER_BATCH,
            review("review", "unknown"),
        ),
        (
            "event_dispositions",
            limits::DISPOSITIONS_PER_BATCH,
            json!({"event_id":"unknown","outcome":"no_change"}),
        ),
    ] {
        let mut request = json!({"request_id":"oversized"});
        request[section] = json!(vec![item; limit + 1]);
        assert_eq!(
            f.update(request).unwrap_err().code,
            "field_too_large",
            "{section}"
        );
    }
    other.execute_batch("ROLLBACK").unwrap();
}

#[test]
fn batch_keys_failed_raw_capture_is_once_pending_then_fresh_key_repairs_and_replays() {
    let f = Fixture::new();
    let seed = f.seed();
    let known = f.event("known-pending");
    let mut request = learning("rejected-batch", &known, "new-memory");
    request["reported_events"] = json!([{"external_event_key":"independent-raw","speaker":"learner","kind":"message","text":"π remains fixed; x varies."}]);
    request["session"] = json!({"operation":"use","session_id":f.session,"expected_revision":2});
    request["checkpoint"] = checkpoint("new-memory", json!("after_edit"));
    request["review_updates"] = json!([review("new-review", "new-memory")]);
    request["memory_edits"].as_array_mut().unwrap().push(json!({"operation":"flag","memory_id":seed.references["memory"],"expected_revision":1,"review_note":"Revisit under new conditions","reason":"New response","change_basis":[{"event_id":known}]}));
    request["event_dispositions"] =
        json!([{"event_id":known,"outcome":"processed"},{"event_id":known,"outcome":"no_change"}]);
    let before = semantic_state(&f.store.conn);
    let generation = event_generation(&f.store.conn);
    let rejected = f.rejection(request.clone(), "event_dispositions[1].event_id");
    assert_eq!(semantic_state(&f.store.conn), before);
    assert_eq!(event_generation(&f.store.conn), generation + 1);
    let raw: (String,String,String,Option<String>) = f.store.conn.query_row("SELECT event_id,text,processing,processed_request_id FROM learning_events WHERE external_event_key='independent-raw'", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap();
    assert_eq!(raw.1, "π remains fixed; x varies.");
    assert_eq!((raw.2, raw.3), ("pending".into(), None));
    assert_eq!(
        rejected.captured_event_ids,
        vec![raw.0.clone(), known.clone()]
    );
    assert_eq!(
        event_status(&f.store.conn, &known),
        ("pending".into(), None)
    );
    let repeated = f.rejection(request.clone(), "event_dispositions[1].event_id");
    assert_eq!(repeated.captured_event_ids, rejected.captured_event_ids);
    assert_eq!(semantic_state(&f.store.conn), before);
    assert_eq!(event_generation(&f.store.conn), generation + 1);
    assert_eq!(
        f.store
            .conn
            .query_row(
                "SELECT count(*) FROM learning_events WHERE external_event_key='independent-raw'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );

    // Correct with a fresh receipt key and explicitly reuse the independently
    // captured original input in its support role; no second raw record.
    request["request_id"] = json!("corrected-batch");
    request["observations"][0]["events"]
        .as_array_mut()
        .unwrap()
        .push(json!({"event_id":raw.0,"role":"task_context"}));
    request["event_dispositions"] =
        json!([{"event_id":known,"outcome":"processed"},{"event_id":raw.0,"outcome":"processed"}]);
    let committed = f.update(request.clone()).unwrap();
    assert_eq!(committed.captured_event_ids, rejected.captured_event_ids);
    assert_eq!(
        event_status(&f.store.conn, &raw.0),
        ("processed".into(), Some("corrected-batch".into()))
    );
    assert_eq!(
        event_status(&f.store.conn, &known),
        ("processed".into(), Some("corrected-batch".into()))
    );
    let control = RawEvent {
        external_event_key: "control-turn".into(),
        speaker: Speaker::Learner,
        kind: "message".into(),
        text: "Stop learning and recording.".into(),
        metadata: None,
        occurred_at_ms: None,
    };
    f.store
        .control(&f.host, "later-stop", "stop", &control)
        .unwrap();
    f.store
        .control(&f.host, "later-off", "recording_off", &control)
        .unwrap();
    let stopped = semantic_state(&f.store.conn);
    let generation = event_generation(&f.store.conn);
    let replay = f.update(request.clone()).unwrap();
    assert_eq!(
        serde_json::to_value(&replay).unwrap(),
        serde_json::to_value(&committed).unwrap()
    );
    assert_eq!(semantic_state(&f.store.conn), stopped);
    assert_eq!(event_generation(&f.store.conn), generation);
    assert_eq!(
        f.store.binding(&f.host).unwrap(),
        (Some(f.session.clone()), false, false)
    );
    assert_eq!(
        f.store
            .conn
            .query_row(
                "SELECT count(*) FROM learning_events WHERE external_event_key='independent-raw'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    let mut changed = request.clone();
    changed["observations"][0]["description"] = json!("Changed payload");
    assert_eq!(f.update(changed).unwrap_err().code, "request_key_conflict");
    let other_host = HostContext {
        host_thread_id: "another-host".into(),
        ..f.host.clone()
    };
    request["schema"] = json!(SCHEMA);
    request["action"] = json!("update");
    assert_eq!(
        f.store.update(&other_host, request).unwrap_err().code,
        "request_key_conflict"
    );
    assert_eq!(semantic_state(&f.store.conn), stopped);
}

#[test]
fn batch_keys_shared_support_different_roles_and_historical_observation_reuse_work() {
    let f = Fixture::new();
    let event = f.event("shared-work");
    let mut request = learning("shared-support", &event, "coefficient_memory");
    request["observations"][0]["events"]
        .as_array_mut()
        .unwrap()
        .push(json!({"event_id":event,"role":"task_context"}));
    request["observations"]
        .as_array_mut()
        .unwrap()
        .push(observation("other_work", &event));
    request["memory_edits"].as_array_mut().unwrap().push(create(
        "variable_memory",
        json!([{"new_observation":"work"}]),
    ));
    let result = f.update(request).unwrap();
    let roles: i64 = f
        .store
        .conn
        .query_row(
            "SELECT count(*) FROM observation_events WHERE observation_id=?1 AND event_id=?2",
            [&result.references["work"], &event],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(roles, 2);
    for memory in ["coefficient_memory", "variable_memory"] {
        assert_eq!(
            f.store
                .conn
                .query_row(
                    "SELECT observation_id FROM memory_observations WHERE memory_id=?1",
                    [&result.references[memory]],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            result.references["work"]
        );
    }
    let historical = f.event("historical-support");
    let observed = f.update(json!({"request_id":"historical-observation","observations":[observation("history",&historical)]})).unwrap();
    assert_eq!(
        event_status(&f.store.conn, &historical),
        ("pending".into(), None)
    );
    let later = f.event("later-work");
    f.update(json!({"request_id":"reuse-observation","memory_edits":[{"operation":"revise","memory_id":result.references["coefficient_memory"],"expected_revision":1,"record":{"title":"Coefficient roles","scope":"algebra","summary":"Retains earlier limited evidence","state_text":"A later learner question warrants qualified wording.","current_basis":[{"observation_id":result.references["work"]},{"observation_id":observed.references["history"]}]},"change_kind":"reinterpretation","reason":"Later learner question","change_basis":[{"observation_id":result.references["work"]},{"event_id":later}]}],"event_dispositions":[{"event_id":later,"outcome":"processed"}]})).unwrap();
    assert_eq!(
        event_status(&f.store.conn, &historical),
        ("pending".into(), None)
    );
    assert_eq!(
        event_status(&f.store.conn, &later),
        ("processed".into(), Some("reuse-observation".into()))
    );
}

#[test]
fn batch_keys_ordinary_names_exact_runtime_references_and_unrelated_text_remain_valid() {
    let f = Fixture::new();
    let seed = f.seed();
    let runtime_like = format!("m-{}", "ABCDEF0123456789".repeat(2));
    let raw = f.update(json!({"request_id":runtime_like,"reported_events":[{"external_event_key":runtime_like,"speaker":"learner","kind":"message","text":runtime_like}]})).unwrap();
    let event = &raw.captured_event_ids[0];
    let names = [
        "answer",
        "coefficient_memory",
        "next_review",
        "m-useful-name",
        "e-0000000000000000000000000000000g",
    ];
    let mut request = learning("ordinary-names", event, names[1]);
    request["observations"][0]["client_ref"] = json!(names[0]);
    request["memory_edits"][0]["record"]["current_basis"] = json!([{"new_observation":names[0]}]);
    request["memory_edits"][0]["change_basis"] = json!([{"new_observation":names[0]}]);
    request["memory_edits"].as_array_mut().unwrap().push(create(
        names[3],
        json!([{"observation_id":seed.references["work"]}]),
    ));
    request["memory_edits"].as_array_mut().unwrap().push(create(
        names[4],
        json!([{"observation_id":seed.references["work"]}]),
    ));
    request["session"] = json!({"operation":"use","session_id":f.session,"expected_revision":2});
    request["checkpoint"] = checkpoint(&seed.references["memory"], json!(1));
    request["review_updates"] = json!([review(names[2], &seed.references["memory"])]);
    let written = f.update(request).unwrap();
    for name in names {
        assert!(written.references.contains_key(name));
    }
    let saved: String = f
        .store
        .conn
        .query_row(
            "SELECT checkpoint_json FROM learning_sessions WHERE session_id=?1",
            [&f.session],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&saved).unwrap()["memory_refs"][0]["memory_id"],
        seed.references["memory"]
    );
    assert_eq!(
        f.store
            .conn
            .query_row(
                "SELECT memory_id FROM review_items WHERE review_id=?1",
                [&written.references["next_review"]],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        seed.references["memory"]
    );
    let boundary = "é".repeat(limits::REFERENCE_BYTES / 2);
    f.update(json!({"request_id":"boundary-name","review_updates":[review(&boundary,&seed.references["memory"])]})).unwrap();
}

#[test]
fn batch_keys_persistent_shape_utility_is_narrow_and_ascii_only() {
    for prefix in ["e", "o", "m", "s", "r"] {
        assert!(store::resembles_persistent_id(&format!(
            "{prefix}-{}",
            "0123456789aBcDeF".repeat(2)
        )));
    }
    for name in [
        "answer",
        "m-answer",
        "m-0000000000000000000000000000000",
        "m-000000000000000000000000000000000",
        "m-0000000000000000000000000000000g",
        "M-00000000000000000000000000000000",
        "x-00000000000000000000000000000000",
        "m-000000000000000000000000000000-0",
        " m-00000000000000000000000000000000",
        "m-00000000000000000000000000000000 ",
        "m-éééééééééééééééé",
    ] {
        assert!(!store::resembles_persistent_id(name), "{name}");
    }
}

#[test]
fn batch_keys_generated_runtime_schema_describes_local_namespace_and_dispositions() {
    let tool = crate::mcp::catalog::get_tool("learning_step").unwrap();
    let schema = Value::Object((*tool.input_schema).clone());
    let update = schema["anyOf"]
        .as_array()
        .unwrap()
        .iter()
        .find(|branch| branch["properties"]["action"]["enum"] == json!(["update"]))
        .unwrap();
    let properties = &update["properties"];
    let create_property = |section: &str| {
        properties[section]["items"]["oneOf"]
            .as_array()
            .unwrap()
            .iter()
            .find(|branch| branch["properties"]["operation"]["const"] == "create")
            .unwrap()["properties"]["client_ref"]
            .clone()
    };
    // The operation schemas may be nullable; inspect each actual typed shape.
    let session = serde_json::to_value(schemars::schema_for!(StartSessionEdit)).unwrap();
    let fields = [
        properties["observations"]["items"]["properties"]["client_ref"].clone(),
        create_property("memory_edits"),
        create_property("review_updates"),
        session["properties"]["client_ref"].clone(),
    ];
    for field in fields {
        let description = field["description"].as_str().unwrap();
        for fact in [
            "Nonempty",
            "256 UTF-8 bytes",
            "unique across all",
            "e/o/m/s/r-32 hex digits",
        ] {
            assert!(description.contains(fact), "{description}");
        }
    }
    assert!(
        properties["event_dispositions"]["items"]["properties"]["event_id"]["description"]
            .as_str()
            .unwrap()
            .contains("at most one disposition")
    );
}
