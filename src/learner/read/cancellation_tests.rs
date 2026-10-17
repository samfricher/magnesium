use super::*;
use crate::retrieval::{OperationCancellation, enter_request_cancellation};
use std::sync::{Arc, atomic::AtomicBool};

fn fixture() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(crate::learner::store::SCHEMA_SQL)
        .unwrap();
    conn.execute("INSERT INTO workspace_meta(singleton,workspace_id,storage_family,schema_version,created_at_ms) VALUES(1,'cancellation-tests','learner-model/v2',2,0)", []).unwrap();
    conn
}

#[test]
fn cancelled_learner_read_does_not_start_snapshot() {
    let conn = fixture();
    let cancellation = OperationCancellation::with_flag(Arc::new(AtomicBool::new(true)));
    let _scope = enter_request_cancellation(&cancellation);
    let result = prepare_snapshot(
        &conn,
        "h",
        "t",
        serde_json::json!({"schema":crate::learner::SCHEMA,"action":"browse"}),
    );
    assert!(
        matches!(result, Err(ref error) if error.contains("request_cancelled")),
        "cancelled request must fail before reading"
    );
    assert!(conn.is_autocommit());
}

#[test]
fn cancelled_read_skips_optional_trace() {
    let conn = fixture();
    conn.execute(
        "INSERT INTO learning_sessions VALUES('s','goal','active','{}',1,0,0)",
        [],
    )
    .unwrap();
    conn.execute("INSERT INTO host_bindings(host_kind,host_thread_id,session_id,learning_active,recording_enabled) VALUES('h','t','s',1,1)", []).unwrap();
    let prepared = prepare_snapshot(
        &conn,
        "h",
        "t",
        serde_json::json!({"schema":crate::learner::SCHEMA,"action":"browse"}),
    )
    .unwrap();
    let cancellation = OperationCancellation::with_flag(Arc::new(AtomicBool::new(true)));
    let _scope = enter_request_cancellation(&cancellation);
    let result = finish_trace(&conn, prepared, true).unwrap();
    assert_eq!(result["trace_status"], "disabled");
    assert_eq!(
        conn.query_row("SELECT count(*) FROM context_receipts", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}
