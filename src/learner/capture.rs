use super::*;
use rusqlite::{OptionalExtension, params};
impl LearnerStore {
    pub fn capture(
        &self,
        host: &HostContext,
        events: &[RawEvent],
        origin: &str,
        allow_start: bool,
    ) -> Result<Vec<String>> {
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
        let (session, active): (Option<String>, bool) = tx
            .query_row(
                "SELECT session_id,learning_active FROM host_bindings WHERE host_kind=?1 AND host_thread_id=?2",
                params![host.host_kind, host.host_thread_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .unwrap_or((None, false));
        if !active && !allow_start {
            return Err(LearnerError::new(
                "learning_inactive",
                "Start learning explicitly before capture",
            ));
        }
        if !["host_observed", "model_reported"].contains(&origin) {
            return Err(LearnerError::new("invalid_variant", "origin"));
        }
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
                if typed.intent.is_some() && origin != "host_observed" {
                    return Err(LearnerError::new(
                        "invalid_variant",
                        "metadata.intent is set by the host from how the learner sent the message; leave it out",
                    ));
                }
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
}
