use super::*;
pub(super) fn apply_reviews(
    conn: &Connection,
    edits: &[ReviewEdit],
    refs: &mut BTreeMap<String, String>,
    revisions: &mut BTreeMap<String, i64>,
    used_events: &mut BTreeSet<String>,
    now: i64,
) -> Result<()> {
    let mut seen = BTreeSet::new();
    for edit in edits {
        match edit {
            ReviewEdit::Create(CreateReviewEdit {
                client_ref,
                memory_id,
                purpose,
                reason,
                due_at_ms,
            }) => {
                let id = store::new_id("r")?;
                let memory = refs.get(memory_id).unwrap_or(memory_id);
                let revision: i64 = conn
                    .query_row(
                        "SELECT revision FROM learner_memories WHERE memory_id=?1",
                        [memory],
                        |r| r.get(0),
                    )
                    .optional()?
                    .ok_or_else(|| unknown(memory))?;
                limits::text("review.purpose", purpose, 2048)?;
                limits::text("review.reason", reason, 2048)?;
                conn.execute(
                    "INSERT INTO review_items VALUES(?1,?2,?3,?4,?5,?6,'suggested',1,?7)",
                    params![id, memory, revision, purpose, reason, due_at_ms, now],
                )?;
                insert_ref(refs, client_ref, &id)?;
                revisions.insert(id, 1);
            }
            ReviewEdit::Update(UpdateReviewEdit {
                review_id,
                expected_revision,
                status,
                activity_event_id,
            }) => {
                if !seen.insert(review_id) {
                    return Err(LearnerError::new(
                        "invalid_variant",
                        "duplicate review update",
                    ));
                }
                let revision = cas(
                    conn,
                    "review_items",
                    "review_id",
                    review_id,
                    *expected_revision,
                )?;
                if *status == ReviewStatus::Done {
                    activity_event_id.as_ref().ok_or_else(|| {
                        LearnerError::new("invalid_variant", "done review requires activity")
                    })?;
                }
                if let Some(event) = activity_event_id {
                    let exists: bool = conn.query_row(
                        "SELECT EXISTS(
                        SELECT 1 FROM learning_events
                        WHERE event_id = ?1 AND speaker = 'learner'
                    )",
                        [event],
                        |r| r.get(0),
                    )?;
                    if !exists {
                        return Err(unknown(event));
                    }
                    used_events.insert(event.clone());
                }
                conn.execute(
                    "UPDATE review_items SET status = ?2, revision = ?3, updated_at_ms = ?4
                     WHERE review_id = ?1",
                    params![review_id, status, revision, now],
                )?;
                revisions.insert(review_id.clone(), revision);
            }
        }
    }
    Ok(())
}
