//! One semantic transaction orchestrates the state-specific writer helpers.
use super::*;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

mod capture;
mod decode;
mod indexes;
mod keys;
mod memories;
mod observations;
mod receipts;
mod reviews;
mod session;

pub(crate) use decode::{decode_at, decode_update_request, field_error};
use indexes::maintain_indexes;
use keys::validate_batch_keys;
#[cfg(test)]
pub(crate) use memories::memory_snapshot;
use memories::{apply_memory, resolve_change_basis};
use observations::apply_observations;
pub(crate) use receipts::payload_hash;
use receipts::{cas, read_receipt, validate_receipt_host};
use reviews::apply_reviews;
use session::{SessionTransition, admit_semantic_update, apply_session, resolve_checkpoint};

impl LearnerStore {
    fn semantic_update(
        &self,
        host: &HostContext,
        r: &UpdateRequest,
        payload: &Value,
        captured: &[String],
        attachment_ids: &[String],
    ) -> Result<UpdateResult> {
        limits::text("request_id", &r.request_id, 256)?;
        if r.request_id.is_empty() {
            return Err(LearnerError::new("field_too_large", "semantic batch"));
        }
        // The outer boundary has already replayed successful receipts before
        // raw capture/typed decode. New batches get one pure key preflight;
        // lock-time receipt lookup below still closes the concurrent retry race.
        validate_batch_keys(r, host)?;
        let hash = payload_hash(payload)?;
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        if let Some(receipt) = read_receipt(&tx, &r.request_id)? {
            if receipt.request_sha256 != hash {
                return Err(LearnerError::request_key_conflict(&r.request_id));
            }
            let result: UpdateResult = serde_json::from_str(&receipt.result_json)?;
            validate_receipt_host(&result, host)?;
            return Ok(result);
        }
        #[cfg(test)]
        crate::request::test_checkpoint("before_semantic");
        check_semantic_cancellation()?;
        // A verified ticket admits one semantic transition. A successful
        // same-key retry returned above; another transition needs new native input.
        if r.native_capture.is_some() {
            let native_id = captured.first().ok_or_else(|| {
                LearnerError::new("invalid_variant", "verified native capture missing")
            })?;
            let attached: Option<String> = tx
                .query_row(
                    "SELECT session_id FROM learning_events WHERE event_id=?1",
                    [native_id],
                    |row| row.get(0),
                )
                .optional()?
                .ok_or_else(|| unknown(native_id))?;
            if let Some(session) = attached {
                let mut error = LearnerError::new(
                    "events_already_processed",
                    "This native input already committed a session transition; replay the original request or use a new native turn",
                );
                error.recovery = Some(
                    json!({"event_id":native_id,"session_id":session,"next_call":read::NextCall::session_status(&session),"instruction":"Read the existing session. A new native capture needs a new verified turn; replay the original request if its outcome was uncertain."}),
                );
                return Err(error);
            }
        }
        let transition = SessionTransition::from_request(r);
        admit_semantic_update(&tx, host, r, transition)?;
        for d in &r.event_dispositions {
            let old: Option<String> = tx
                .query_row(
                    "SELECT processed_request_id FROM learning_events WHERE event_id=?1",
                    [&d.event_id],
                    |row| row.get(0),
                )
                .optional()?
                .ok_or_else(|| unknown(&d.event_id))?;
            if let Some(request) = old {
                return Err(LearnerError::new(
                    "events_already_processed",
                    format!("{}: original request {request}", d.event_id),
                ));
            }
        }
        if r.mode == UpdateMode::Normal
            && let Some(turn) = &r.turn_ref
        {
            let valid: bool = tx.query_row(
                "SELECT EXISTS(
                SELECT 1 FROM learning_events
                WHERE event_id = ?1 AND host_kind = ?2 AND host_thread_id = ?3
                  AND speaker = 'learner'
            )",
                params![turn, host.host_kind, host.host_thread_id],
                |row| row.get(0),
            )?;
            if !valid {
                return Err(unknown(turn));
            }
        }
        check_semantic_cancellation()?;
        let mut refs = BTreeMap::new();
        let mut revisions = BTreeMap::new();
        let now = store::now_ms();
        let (session, mut events_changed) =
            apply_session(&tx, host, r, transition, &mut refs, now, attachment_ids)?;
        session::require_checkpoint_guard(&tx, r, session.as_deref())?;
        if r.session.is_some()
            && let Some(id) = &session
        {
            let revision: i64 = tx.query_row(
                "SELECT revision FROM learning_sessions WHERE session_id=?1",
                [id],
                |row| row.get(0),
            )?;
            revisions.insert(id.clone(), revision);
        }
        let mut used_events = BTreeSet::new();
        apply_observations(
            &tx,
            host,
            &r.observations,
            &session,
            &mut refs,
            &mut used_events,
            now,
        )?;
        // Allocate all new IDs before edits so retirement can refer to same-batch creations.
        for edit in &r.memory_edits {
            if let MemoryEdit::Create(CreateMemoryEdit { client_ref, .. }) = edit {
                insert_ref(&mut refs, client_ref, &store::new_id("m")?)?;
            }
        }
        let mut edited = BTreeSet::new();
        for (edit_index, edit) in r.memory_edits.iter().enumerate() {
            let support = resolve_change_basis(&tx, &refs, edit)?;
            if r.mode == UpdateMode::CompleteCaptured
                && !support
                    .participating_event_ids
                    .contains(r.turn_ref.as_ref().expect("admitted completion turn"))
            {
                return Err(LearnerError::new(
                    "invalid_variant",
                    "Each completion memory edit needs change-basis support from the selected closing learner turn; historical current_basis can remain as context",
                ));
            }
            used_events.extend(support.participating_event_ids.iter().cloned());
            apply_memory(
                &tx,
                edit,
                &refs,
                &mut revisions,
                &mut edited,
                &support,
                (now, edit_index),
            )?;
        }
        for edit in &r.memory_edits {
            if let MemoryEdit::Retire(RetireMemoryEdit {
                memory_id,
                replacement_ids,
                ..
            }) = edit
            {
                if replacement_ids.len() > 4 {
                    return Err(LearnerError::new("field_too_large", "replacement_ids"));
                }
                for replacement in replacement_ids {
                    let id = refs.get(replacement).unwrap_or(replacement);
                    let active: bool = tx.query_row(
                        "SELECT EXISTS(
                        SELECT 1 FROM learner_memories
                        WHERE memory_id = ?1 AND lifecycle = 'active'
                    )",
                        [id],
                        |row| row.get(0),
                    )?;
                    if id == memory_id || !active {
                        return Err(unknown(id));
                    }
                }
            }
        }
        if let Some(checkpoint) = &r.checkpoint {
            let session = session.as_ref().ok_or_else(|| {
                LearnerError::new("invalid_variant", "checkpoint requires session operation")
            })?;
            let value = resolve_checkpoint(&tx, host, checkpoint, &refs, &revisions)?;
            tx.execute(
                "UPDATE learning_sessions SET checkpoint_json=?2 WHERE session_id=?1",
                params![session, serde_json::to_string(&value)?],
            )?;
        }
        apply_reviews(
            &tx,
            &r.review_updates,
            &mut refs,
            &mut revisions,
            &mut used_events,
            now,
        )?;
        if !r.review_updates.is_empty() {
            tx.execute_batch("UPDATE workspace_meta SET review_generation=review_generation+1")?;
        }
        if r.mode == UpdateMode::CompleteCaptured
            && (!r.observations.is_empty()
                || !r.memory_edits.is_empty()
                || !r.review_updates.is_empty())
            && !used_events.contains(r.turn_ref.as_ref().expect("admitted completion turn"))
        {
            return Err(LearnerError::new(
                "invalid_variant",
                "Completion changes must be supported by the selected captured learner turn",
            ));
        }
        for d in &r.event_dispositions {
            if d.outcome == Disposition::Processed && !used_events.contains(&d.event_id) {
                return Err(LearnerError::new(
                    "invalid_variant",
                    "event_dispositions.outcome must be processed or no_change; processed needs observation, change, or review activity support",
                ));
            }
            events_changed |= tx.execute(
                "UPDATE learning_events
                 SET processing = ?2, processed_request_id = ?3
                 WHERE event_id = ?1
                   AND (processing != ?2 OR processed_request_id IS NOT ?3)",
                params![d.event_id, d.outcome, r.request_id],
            )? > 0;
        }
        if events_changed {
            tx.execute_batch("UPDATE workspace_meta SET event_generation=event_generation+1")?;
        }
        let index_status = maintain_indexes(
            &tx,
            &edited,
            &r.observations
                .iter()
                .map(|o| refs[&o.client_ref].clone())
                .collect::<Vec<_>>(),
        )?;
        let result = UpdateResult {
            host_kind: host.host_kind.clone(),
            host_thread_id: host.host_thread_id.clone(),
            schema: SCHEMA.into(),
            workspace_id: self.workspace_id.clone(),
            request_id: r.request_id.clone(),
            captured_event_ids: captured.to_vec(),
            capture_status: "recorded".into(),
            update_status: "committed".into(),
            index_status,
            references: refs,
            revisions,
        };
        // Independently committed capture survives cancellation. This semantic
        // transaction is still safely rollbackable until the receipt/commit
        // boundary; after commit its durable receipt is authoritative.
        #[cfg(test)]
        crate::request::test_checkpoint("before_commit");
        check_semantic_cancellation()?;
        tx.execute(
            "INSERT INTO operation_receipts VALUES(?1,?2,?3,?4)",
            params![r.request_id, hash, serde_json::to_string(&result)?, now],
        )?;
        if let Err(error) = tx.commit() {
            // COMMIT may have an uncertain I/O outcome. Only a fresh usable
            // connection's durable receipt can establish success.
            if let Some(result) = self.recover_commit_receipt(host, &r.request_id, &hash)? {
                return Ok(result);
            }
            return Err(error.into());
        }
        #[cfg(test)]
        crate::request::test_checkpoint("after_commit");
        Ok(result)
    }
}
fn check_semantic_cancellation() -> Result<()> {
    if crate::request::OperationCancellation::none().is_cancelled() {
        return Err(LearnerError::new(
            "request_cancelled",
            "Semantic update cancelled before commit; independently captured input remains pending",
        ));
    }
    Ok(())
}
fn unknown(id: &str) -> LearnerError {
    LearnerError::new("unknown_reference", id)
}
fn insert_ref(refs: &mut BTreeMap<String, String>, name: &str, id: &str) -> Result<()> {
    // The typed preflight owns request admission and field diagnostics. This
    // allocation guard is an internal invariant and never overwrites an ID.
    if refs.contains_key(name) {
        return Err(LearnerError::new(
            "storage_error",
            "Validated client_ref allocated more than once",
        ));
    }
    refs.insert(name.into(), id.into());
    Ok(())
}
