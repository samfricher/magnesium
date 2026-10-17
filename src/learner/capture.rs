use super::*;
use rusqlite::{OptionalExtension, params};
/// Whether this call changed canonical controls or only returned a historical receipt.
/// This is deliberately outside the stored receipt: retries must not authorize
/// clearing safety state created by a later failed control.
pub(crate) struct ControlOutcome {
    pub receipt: serde_json::Value,
    pub applied: bool,
}
impl LearnerStore {
    /// Origin and allow_start are adapter decisions; tool text never supplies them.
    pub fn capture(
        &self,
        host: &HostContext,
        events: &[RawEvent],
        origin: &str,
        allow_start: bool,
    ) -> Result<Vec<String>> {
        // No raw input means no capture operation. Semantic completion has its
        // own transactional admission and must not depend on teaching activity.
        if events.is_empty() {
            return Ok(vec![]);
        }
        if events.len() > limits::RAW_EVENTS_PER_BATCH {
            return Err(LearnerError::new(
                "field_too_large",
                "reported_events: at most 32 events per capture; continue in another call",
            ));
        }
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let (session, active, recording): (Option<String>,bool,bool) = tx.query_row("SELECT session_id,learning_active,recording_enabled FROM host_bindings WHERE host_kind=?1 AND host_thread_id=?2",params![host.host_kind,host.host_thread_id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional()?.unwrap_or((None,false,true));
        if !recording {
            return Err(LearnerError::new(
                "recording_disabled",
                "Recording is disabled",
            ));
        }
        if !active && !allow_start {
            return Err(LearnerError::new(
                "learning_inactive",
                "Start learning explicitly before capture",
            ));
        }
        if !["host_observed", "model_reported"].contains(&origin) {
            return Err(LearnerError::new("invalid_variant", "origin"));
        }
        // New start/resume input is not evidence for a previously bound
        // session. Bind it only when that semantic session operation commits.
        // Existing external-key retries retain their original association.
        let session = if allow_start { None } else { session };
        let mut ids = vec![];
        let mut inserted = false;
        for event in events {
            limits::text("event.text", &event.text, limits::EVENT_BYTES)?;
            limits::text("event.external_event_key", &event.external_event_key, 512)?;
            limits::text("event.kind", &event.kind, 128)?;
            let metadata = serde_json::to_string(&event.metadata)?;
            limits::text("event.metadata", &metadata, limits::EVENT_METADATA_BYTES)?;
            if let Some(typed) = &event.metadata {
                if typed.origin.as_deref().is_some_and(|value| value != origin) {
                    return Err(LearnerError::new(
                        "invalid_variant",
                        "metadata.origin cannot override captured origin",
                    ));
                }
                if typed.help_refs.len() > 16
                    || typed.source_refs.len() > 16
                    || typed.attachments.len() > 16
                {
                    return Err(LearnerError::new(
                        "field_too_large",
                        "event metadata references",
                    ));
                }
            }
            let existing=tx.query_row("SELECT event_id,speaker,text,metadata_json,occurred_at_ms,origin FROM learning_events WHERE host_kind=?1 AND host_thread_id=?2 AND external_event_key=?3 AND kind=?4",params![host.host_kind,host.host_thread_id,event.external_event_key,event.kind],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,Option<i64>>(4)?,r.get::<_,String>(5)?))).optional()?;
            if let Some(old) = existing {
                if old.1 != event.speaker.as_str()
                    || old.2 != event.text
                    || serde_json::from_str::<Option<EventMetadata>>(&old.3)? != event.metadata
                    || old.4 != event.occurred_at_ms
                    || old.5 != origin
                {
                    return Err(LearnerError::new(
                        "event_key_conflict",
                        &event.external_event_key,
                    ));
                }
                ids.push(old.0);
                continue;
            }
            let id = store::new_id("e")?;
            let processing = if event.speaker == Speaker::Learner {
                "pending"
            } else {
                // Tutor delivery, source material and checker output remain in
                // the original log, without posing as uninterpreted learner work.
                "not_applicable"
            };
            tx.execute("INSERT INTO learning_events(event_id,session_id,host_kind,host_thread_id,external_event_key,speaker,kind,origin,text,metadata_json,occurred_at_ms,captured_at_ms,processing) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",params![id,session,host.host_kind,host.host_thread_id,event.external_event_key,event.speaker,event.kind,origin,event.text,metadata,event.occurred_at_ms,store::now_ms(),processing])?;
            inserted = true;
            ids.push(id);
        }
        if inserted {
            tx.execute_batch("UPDATE workspace_meta SET event_generation=event_generation+1")?;
        }
        tx.commit()?;
        Ok(ids)
    }
    #[cfg(test)]
    pub(crate) fn control(
        &self,
        host: &HostContext,
        request_id: &str,
        action: &str,
        current_learner_event: &RawEvent,
    ) -> Result<serde_json::Value> {
        self.control_with_outcome(host, request_id, action, current_learner_event)
            .map(|outcome| outcome.receipt)
    }
    pub(crate) fn control_with_outcome(
        &self,
        host: &HostContext,
        request_id: &str,
        action: &str,
        current_learner_event: &RawEvent,
    ) -> Result<ControlOutcome> {
        if current_learner_event.speaker != Speaker::Learner {
            return Err(LearnerError::new(
                "invalid_variant",
                "Control requires current learner-origin input",
            ));
        }
        if !["pause", "stop", "recording_off", "recording_on"].contains(&action) {
            return Err(LearnerError::new("invalid_variant", "control.action"));
        }
        limits::text("request_id", request_id, 256)?;
        let request = serde_json::json!({"action":action,"host_kind":host.host_kind,"host_thread_id":host.host_thread_id,"event_key":current_learner_event.external_event_key});
        let hash = super::update::payload_hash(&request)?;
        let tx = self.conn.unchecked_transaction()?;
        if let Some((previous, result)) = tx
            .query_row(
                "SELECT request_sha256,result_json FROM operation_receipts WHERE request_id=?1",
                [request_id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?
        {
            if previous != hash {
                return Err(LearnerError::request_key_conflict(request_id));
            }
            return Ok(ControlOutcome {
                receipt: serde_json::from_str(&result)?,
                applied: false,
            });
        }
        #[cfg(test)]
        crate::request::test_checkpoint("before_control_grant");
        check_control_grant_cancellation(action)?;
        tx.execute(
            "INSERT OR IGNORE INTO host_bindings(host_kind,host_thread_id) VALUES(?1,?2)",
            params![host.host_kind, host.host_thread_id],
        )?;
        match action {
            "pause" | "stop" => {
                // Conversation-scoped, like the deny marker and admission lock
                // that protect it when this write fails. session.pause stops
                // the whole lesson atomically.
                tx.execute("UPDATE host_bindings SET learning_active=0 WHERE host_kind=?1 AND host_thread_id=?2",params![host.host_kind,host.host_thread_id])?;
            }
            _ => {
                tx.execute("UPDATE host_bindings SET recording_enabled=?3 WHERE host_kind=?1 AND host_thread_id=?2",params![host.host_kind,host.host_thread_id,action=="recording_on"])?;
            }
        }
        let result = serde_json::json!({"schema":SCHEMA,"workspace_id":self.workspace_id,"request_id":request_id,"action":action,"durable":true});
        tx.execute(
            "INSERT INTO operation_receipts VALUES(?1,?2,?3,?4)",
            params![
                request_id,
                hash,
                serde_json::to_string(&result)?,
                store::now_ms()
            ],
        )?;
        #[cfg(test)]
        crate::request::test_checkpoint("before_control_commit");
        check_control_grant_cancellation(action)?;
        tx.commit()?;
        Ok(ControlOutcome {
            receipt: result,
            applied: true,
        })
    }
}

// Stop/off are safety transitions and must finish once admitted. Recording-on
// grants capture permission, so cancellation can still abort before commit.
fn check_control_grant_cancellation(action: &str) -> Result<()> {
    if action == "recording_on" && crate::request::OperationCancellation::none().is_cancelled() {
        return Err(LearnerError::new(
            "request_cancelled",
            "Recording-on cancelled before commit; capture permission was not enabled by this request",
        ));
    }
    Ok(())
}
