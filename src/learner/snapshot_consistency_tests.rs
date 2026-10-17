//! Independent contract review cases using the public reader over real SQLite.
use super::*;
use rusqlite::{Connection, params};
use serde_json::{Value, json};
fn fixture() -> Connection {
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch(crate::learner::store::SCHEMA_SQL).unwrap();
    c.execute("INSERT INTO workspace_meta(singleton,workspace_id,storage_family,schema_version,created_at_ms) VALUES(1,'w','learner-model/v2',2,0)",[]).unwrap();
    c
}
fn memory(c: &Connection, id: &str, summary: &str) {
    c.execute("INSERT INTO learner_memories(memory_id,title,scope,summary,state_text,current_basis_json,lifecycle,revision,created_at_ms,updated_at_ms) VALUES(?1,'Algebra','school',?2,'The learner does not yet distinguish coefficients.','[]','active',1,0,0)",params![id,summary]).unwrap();
}
fn run(c: &Connection, mut request: Value) -> Result<Value> {
    request["schema"] = json!(SCHEMA);
    read::test_support::execute(c, "test", "thread", request)
        .map_err(|e| LearnerError::new("context", e))
}
fn support(c: &Connection) {
    memory(c, "m", "Qualified current understanding");
    c.execute("INSERT INTO learning_events(event_id,host_kind,host_thread_id,external_event_key,speaker,kind,origin,text,metadata_json,captured_at_ms,processing) VALUES('e','test','thread','turn','learner','message','host_observed','I cannot identify b; π is not x.','{}',0,'pending')",[]).unwrap();
    c.execute("INSERT INTO observations(observation_id,kind,description,conditions_text,assistance_text,source_refs_json,created_at_ms) VALUES('o','learner_question','Question about coefficient roles','After example','Tutor demonstrated substitution','[]',0)",[]).unwrap();
    c.execute("INSERT INTO observation_events VALUES('o','e','learner_response','[\"I cannot identify b\"]')",[]).unwrap();
    c.execute("INSERT INTO memory_observations VALUES('m','o')", [])
        .unwrap();
}
#[test]
fn event_search_links_current_description() {
    let c = fixture();
    support(&c);
    let p = run(
        &c,
        json!({"action":"observations","kind":"event","literal_terms":["identify"]}),
    )
    .unwrap();
    assert_eq!(
        p["observations"][0]["current_memory_refs"][0]["memory_id"], "m",
        "Original-event recovery must lead back to current interpretation"
    );
}
#[test]
fn support_preserves_roles_and_excerpts() {
    let c = fixture();
    support(&c);
    let p = run(
        &c,
        json!({"action":"observations","kind":"event_refs","observation_id":"o"}),
    )
    .unwrap();
    let serialized = p["support"][0].to_string();
    assert!(
        serialized.contains("learner_response"),
        "Support paging loses observation event role: {serialized}"
    );
    assert!(
        serialized.contains("I cannot identify b"),
        "Support paging loses exact authored excerpt: {serialized}"
    );
}
#[test]
fn utf8_range_and_recording_off_do_not_change_state() {
    let c = fixture();
    support(&c);
    c.execute(
        "INSERT INTO host_bindings VALUES('test','thread',NULL,1,0)",
        [],
    )
    .unwrap();
    let text = "I cannot identify b; π is not x.";
    let start = text.find('π').unwrap();
    let p=run(&c,json!({"action":"read","refs":[{"kind":"event","id":"e","start_byte":start,"end_byte":start+2}]})).unwrap();
    assert_eq!(p["records"][0]["text"], "π");
    assert_eq!(p["records"][0]["partial"], true);
    assert_eq!(p["trace_status"], "disabled");
    assert_eq!(
        c.query_row("SELECT count(*) FROM context_receipts", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert!(run(&c,json!({"action":"read","refs":[{"kind":"event","id":"e","start_byte":start+1,"end_byte":start+2}]})).is_err());
}
#[test]
fn complete_directory_exact_budget() {
    let c = fixture();
    memory(&c, "m", "Current nuanced understanding");
    let roomy = run(&c, json!({"action":"prepare"})).unwrap();
    let mut compact = roomy.clone();
    compact["memories"][0]["state_text"] = Value::Null;
    compact["memories"][0]["detail_status"] = json!("read_required");
    let max = serde_json::to_vec(&compact).unwrap().len();
    let p = run(&c, json!({"action":"prepare","max_bytes":max}));
    assert!(
        p.is_ok(),
        "Whole compact directory should fit with body omitted: max={max}, result={p:?}"
    );
}
#[test]
fn recording_disabled_between_snapshot_and_trace_write() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let path =
        std::env::temp_dir().join(format!("{}.sqlite", store::new_id("trace-race").unwrap()));
    let store = LearnerStore::initialize(&path, "w").unwrap();
    store
        .conn
        .execute(
            "INSERT INTO host_bindings VALUES('test','thread',NULL,1,1)",
            [],
        )
        .unwrap();
    memory(
        &store.conn,
        "m",
        "Does not yet identify coefficients reliably",
    );
    let other = Connection::open(&path).unwrap();
    let changed = Arc::new(AtomicBool::new(false));
    let observed = changed.clone();
    store.conn.authorizer(Some(move |context:AuthContext<'_>| {
  if matches!(context.action,AuthAction::Insert{table_name:"context_receipts"}) {
   observed.store(other.execute("UPDATE host_bindings SET recording_enabled=0 WHERE host_kind='test' AND host_thread_id='thread'",[]).is_ok(),Ordering::SeqCst);
  }
  Authorization::Allow
 })).unwrap();
    let packet = run(&store.conn, json!({"action":"prepare"})).unwrap();
    assert!(changed.load(Ordering::SeqCst));
    assert_eq!(packet["trace_status"], "disabled");
    assert_eq!(
        store
            .conn
            .query_row("SELECT count(*) FROM context_receipts", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        packet["memories"][0]["summary"],
        "Does not yet identify coefficients reliably"
    );
    drop(store);
    let _ = std::fs::remove_file(path);
}
#[test]
fn read_snapshot_survives_concurrent_edit_and_next_read_reports_dirty_index() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let path = std::env::temp_dir().join(format!(
        "{}.sqlite",
        store::new_id("snapshot-race").unwrap()
    ));
    let store = LearnerStore::initialize(&path, "w").unwrap();
    store.conn.execute_batch("PRAGMA journal_mode=WAL").unwrap();
    memory(&store.conn, "m", "Earlier uncertainty");
    search::reindex(&store.conn).unwrap();
    let other = Connection::open(&path).unwrap();
    let changed = Arc::new(AtomicBool::new(false));
    let observed = changed.clone();
    store.conn.authorizer(Some(move |context:AuthContext<'_>| {
  if matches!(context.action,AuthAction::Read{table_name:"learner_memories",..}) && !observed.load(Ordering::SeqCst) {
   let ok=other.execute_batch("BEGIN IMMEDIATE; UPDATE learner_memories SET summary='Current understanding corrected',revision=2 WHERE memory_id='m'; UPDATE workspace_meta SET memory_generation=memory_generation+1; COMMIT;").is_ok();observed.store(ok,Ordering::SeqCst);
  }
  Authorization::Allow
 })).unwrap();
    let first = run(&store.conn, json!({"action":"prepare"})).unwrap();
    assert!(changed.load(Ordering::SeqCst));
    assert_eq!(first["memories"][0]["summary"], "Earlier uncertainty");
    assert_eq!(first["coverage"]["index_current"], true);
    let next = run(
        &store.conn,
        json!({"action":"read","refs":[{"kind":"current_memory","id":"m","expected_revision":1}]}),
    )
    .unwrap();
    assert_eq!(
        next["records"][0]["summary"],
        "Current understanding corrected"
    );
    assert_eq!(next["records"][0]["revision_changed"], true);
    assert_eq!(next["coverage"]["index_current"], false);
    drop(store);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
}
#[test]
fn expensive_literal_scan_cancels_explicitly_and_restores_connection() {
    let c = fixture();
    c.execute_batch("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<50000) INSERT INTO learner_memories(memory_id,title,scope,summary,state_text,current_basis_json,lifecycle,revision,created_at_ms,updated_at_ms) SELECT 'm'||x,'needle','needle','needle','needle','[]','active',1,0,0 FROM n;").unwrap();
    let result = search::search(
        &c,
        &[],
        "",
        &[
            "needle".into(),
            "needle".into(),
            "needle".into(),
            "needle".into(),
        ],
        None,
    )
    .unwrap();
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.starts_with("query_cancelled")),
        "Expensive literal scan must disclose interruption: {:?}",
        result.warnings
    );
    assert_eq!(
        c.query_row("SELECT count(*) FROM learner_memories", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        50000
    );
}
