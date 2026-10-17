//! Direct read helpers for unit regressions; production uses explicit snapshot/trace phases.
use rusqlite::Connection;
use serde_json::Value;

pub(crate) fn execute(
    conn: &Connection,
    host_kind: &str,
    host_thread_id: &str,
    request: Value,
) -> Result<Value, String> {
    execute_with_tracing(conn, host_kind, host_thread_id, request, true)
}

pub(crate) fn execute_with_tracing(
    conn: &Connection,
    host_kind: &str,
    host_thread_id: &str,
    request: Value,
    allow_trace: bool,
) -> Result<Value, String> {
    super::finish_trace(
        conn,
        super::prepare_snapshot(conn, host_kind, host_thread_id, request)?,
        allow_trace,
    )
}
