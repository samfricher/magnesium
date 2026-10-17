use super::*;
use sha2::{Digest, Sha256};

impl LearnerStore {
    /// Read-only successful receipt lookup, before transient proof expiry checks.
    pub fn replay(&self, host: &HostContext, payload: &Value) -> Result<Option<UpdateResult>> {
        limits::text(
            "request",
            &serde_json::to_string(payload)?,
            limits::REQUEST_BYTES,
        )?;
        if let Some(key) = payload.get("request_id").and_then(Value::as_str)
            && let Some(receipt) = read_receipt(&self.conn, key)?
        {
            if receipt.request_sha256 != payload_hash(payload)? {
                return Err(LearnerError::request_key_conflict(key));
            }
            let result: UpdateResult = serde_json::from_str(&receipt.result_json)?;
            validate_receipt_host(&result, host)?;
            return Ok(Some(result));
        }
        Ok(None)
    }
    /// After a failed COMMIT releases its writer lock, another host can commit
    /// the same request key and payload before recovery, so a matching hash
    /// alone does not establish that the receipt belongs to this caller.
    pub(crate) fn recover_commit_receipt(
        &self,
        host: &HostContext,
        request_id: &str,
        hash: &str,
    ) -> Result<Option<UpdateResult>> {
        if let Some(path) = self.conn.path()
            && let Ok(fresh) = LearnerStore::open(std::path::Path::new(path), &self.workspace_id)
            && let Ok(Some(receipt)) = read_receipt(&fresh.conn, request_id)
            && receipt.request_sha256 == hash
        {
            let result: UpdateResult = serde_json::from_str(&receipt.result_json)?;
            validate_receipt_host(&result, host)?;
            return Ok(Some(result));
        }
        Ok(None)
    }
}
pub(super) fn cas(
    conn: &Connection,
    table: &str,
    id_column: &str,
    id: &str,
    expected: i64,
) -> Result<i64> {
    let revision: Option<i64> = conn
        .query_row(
            &format!("SELECT revision FROM {table} WHERE {id_column}=?1"),
            [id],
            |r| r.get(0),
        )
        .optional()?;
    let revision = revision.ok_or_else(|| unknown(id))?;
    if revision != expected {
        let mut error = LearnerError::new(
            "revision_conflict",
            format!("{id}: current revision {revision}; read current record before reconciling"),
        );
        let mut current_review = None;
        let next_call = match table {
            "learner_memories" => read::NextCall::current_memory(id),
            "learning_sessions" => read::NextCall::session_status(id),
            "review_items" => {
                let review = conn.query_row(
                    "SELECT review_id, memory_id, memory_revision, purpose, reason,
                            due_at_ms, status, revision, updated_at_ms
                     FROM review_items WHERE review_id = ?1",
                    [id],
                    CurrentReview::decode,
                )?;
                let call = read::NextCall::reviews_for_memory(&review.memory_id);
                current_review = Some(review);
                call
            }
            _ => {
                return Err(LearnerError::new(
                    "storage_error",
                    "Unsupported revision owner",
                ));
            }
        };
        error.recovery = Some(json!({
            "id": id,
            "current_revision": revision,
            "next_call": next_call,
            "current_review": current_review,
            "instruction": "Read the current record, reconcile once, and retry with its current expected_revision and a fresh request_id. Preserve the intended checkpoint and support."
        }));
        return Err(error);
    }
    Ok(revision + 1)
}

pub(crate) fn payload_hash(payload: &Value) -> Result<String> {
    // serde_json's default map is a recursively sorted BTreeMap; arrays keep order.
    Ok(Sha256::digest(serde_json::to_vec(payload)?)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

pub(super) fn validate_receipt_host(result: &UpdateResult, host: &HostContext) -> Result<()> {
    if result.host_kind != host.host_kind || result.host_thread_id != host.host_thread_id {
        return Err(LearnerError::request_key_conflict(
            "Request receipt belongs to a different host conversation",
        ));
    }
    Ok(())
}

/// Conflict recovery must include terminal reviews too: the suggested-review
/// queue is useful context, but cannot represent a dismissed or completed row.
#[derive(serde::Serialize)]
struct CurrentReview {
    review_id: String,
    memory_id: String,
    memory_revision: i64,
    purpose: String,
    reason: String,
    due_at_ms: Option<i64>,
    status: String,
    revision: i64,
    updated_at_ms: i64,
}
impl CurrentReview {
    fn decode(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            review_id: row.get(0)?,
            memory_id: row.get(1)?,
            memory_revision: row.get(2)?,
            purpose: row.get(3)?,
            reason: row.get(4)?,
            due_at_ms: row.get(5)?,
            status: row.get(6)?,
            revision: row.get(7)?,
            updated_at_ms: row.get(8)?,
        })
    }
}

/// One row decoder for ordinary replay, lock-time replay and uncertain COMMIT
/// recovery. None of these reads establishes success without matching host/hash.
pub(super) struct ReceiptRow {
    pub(super) request_sha256: String,
    pub(super) result_json: String,
}
impl ReceiptRow {
    fn decode(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            request_sha256: row.get(0)?,
            result_json: row.get(1)?,
        })
    }
}
pub(super) fn read_receipt(conn: &Connection, request_id: &str) -> Result<Option<ReceiptRow>> {
    Ok(conn
        .query_row(
            "SELECT request_sha256, result_json
         FROM operation_receipts WHERE request_id = ?1",
            [request_id],
            ReceiptRow::decode,
        )
        .optional()?)
}
