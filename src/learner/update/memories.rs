use super::*;
use unicode_normalization::UnicodeNormalization;

fn observation_basis(
    conn: &Connection,
    refs: &BTreeMap<String, String>,
    id: &str,
    new: bool,
) -> Result<String> {
    let id = if new {
        refs.get(id).ok_or_else(|| unknown(id))?
    } else {
        id
    };
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM observations WHERE observation_id=?1)",
        [id],
        |r| r.get(0),
    )?;
    if !exists {
        return Err(unknown(id));
    }
    Ok(id.to_owned())
}
fn current_basis(
    conn: &Connection,
    refs: &BTreeMap<String, String>,
    input: &[CurrentBasisRef],
) -> Result<Vec<String>> {
    input
        .iter()
        .map(|item| match item {
            CurrentBasisRef::Observation(reference) => {
                observation_basis(conn, refs, reference, false)
            }
            CurrentBasisRef::NewObservation(reference) => {
                observation_basis(conn, refs, reference, true)
            }
        })
        .collect()
}
/// Stored reference order belongs to the authored change record, while the event
/// set supplies both per-edit completion admission and transaction dispositions.
pub(super) struct ResolvedChangeBasis {
    references: Vec<String>,
    pub(super) participating_event_ids: BTreeSet<String>,
}

pub(super) fn resolve_change_basis(
    conn: &Connection,
    refs: &BTreeMap<String, String>,
    edit: &MemoryEdit,
) -> Result<ResolvedChangeBasis> {
    let input = match edit {
        MemoryEdit::Create(edit) => &edit.change_basis,
        MemoryEdit::Revise(edit) => &edit.change_basis,
        MemoryEdit::Flag(edit) => &edit.change_basis,
        MemoryEdit::Retire(edit) => &edit.change_basis,
    };
    if input.len() > 16 {
        return Err(LearnerError::new("invalid_variant", "reason/change_basis"));
    }
    let mut resolved = ResolvedChangeBasis {
        references: Vec::with_capacity(input.len()),
        participating_event_ids: BTreeSet::new(),
    };
    for item in input {
        let reference = match item {
            ChangeBasisRef::Observation(reference) | ChangeBasisRef::NewObservation(reference) => {
                let id = observation_basis(
                    conn,
                    refs,
                    reference,
                    matches!(item, ChangeBasisRef::NewObservation(_)),
                )?;
                // New observations have already been inserted. Resolve both
                // forms through the same canonical relation, never request text.
                let mut statement = conn
                    .prepare("SELECT event_id FROM observation_events WHERE observation_id = ?1")?;
                for event in statement.query_map([&id], |row| row.get::<_, String>(0))? {
                    resolved.participating_event_ids.insert(event?);
                }
                id
            }
            ChangeBasisRef::Event(reference) => {
                let learner: bool = conn.query_row(
                    "SELECT EXISTS(
                        SELECT 1 FROM learning_events
                        WHERE event_id = ?1 AND speaker = 'learner'
                    )",
                    [reference],
                    |row| row.get(0),
                )?;
                if !learner {
                    return Err(LearnerError::new(
                        "invalid_variant",
                        "Direct change-basis event must be a learner challenge; describe other support in an observation",
                    ));
                }
                resolved.participating_event_ids.insert(reference.clone());
                reference.clone()
            }
        };
        resolved.references.push(reference);
    }
    Ok(resolved)
}
pub(super) fn apply_memory(
    conn: &Connection,
    edit: &MemoryEdit,
    refs: &BTreeMap<String, String>,
    revisions: &mut BTreeMap<String, i64>,
    edited: &mut BTreeSet<String>,
    support: &ResolvedChangeBasis,
    position: (i64, usize),
) -> Result<()> {
    let (now, edit_index) = position;
    let (id, revision, kind, reason) = match edit {
        MemoryEdit::Create(CreateMemoryEdit {
            client_ref, reason, ..
        }) => (refs[client_ref].clone(), 1, "new_observation", reason),
        MemoryEdit::Revise(ReviseMemoryEdit {
            memory_id,
            expected_revision,
            change_kind,
            reason,
            ..
        }) => (
            memory_id.clone(),
            cas(
                conn,
                "learner_memories",
                "memory_id",
                memory_id,
                *expected_revision,
            )?,
            change_kind.as_str(),
            reason,
        ),
        MemoryEdit::Flag(FlagMemoryEdit {
            memory_id,
            expected_revision,
            reason,
            ..
        }) => (
            memory_id.clone(),
            cas(
                conn,
                "learner_memories",
                "memory_id",
                memory_id,
                *expected_revision,
            )?,
            "flag",
            reason,
        ),
        MemoryEdit::Retire(RetireMemoryEdit {
            memory_id,
            expected_revision,
            reason,
            ..
        }) => (
            memory_id.clone(),
            cas(
                conn,
                "learner_memories",
                "memory_id",
                memory_id,
                *expected_revision,
            )?,
            "retire",
            reason,
        ),
    };
    if !edited.insert(id.clone()) {
        return Err(LearnerError::new(
            "invalid_variant",
            "duplicate memory edit",
        ));
    }
    limits::text("memory.reason", reason, 4096)?;
    if reason.trim().is_empty() || kind != "retire" && support.references.is_empty() {
        return Err(LearnerError::new("invalid_variant", "reason/change_basis"));
    }
    let changes = &support.references;
    match edit {
        MemoryEdit::Create(CreateMemoryEdit { record, .. })
        | MemoryEdit::Revise(ReviseMemoryEdit { record, .. }) => {
            for (name, value, max) in [
                ("title", record.title.as_str(), limits::TITLE_BYTES),
                ("scope", record.scope.as_str(), limits::SCOPE_BYTES),
                ("summary", record.summary.as_str(), limits::SUMMARY_BYTES),
                (
                    "state_text",
                    record.state_text.as_str(),
                    limits::STATE_BYTES,
                ),
            ] {
                limits::text(name, value, max)?;
            }
            if let Some(note) = &record.review_note {
                limits::text("review_note", note, limits::REVIEW_NOTE_BYTES)?;
            }
            if record.current_basis.is_empty() || record.current_basis.len() > 8 {
                return Err(LearnerError::new(
                    "invalid_variant",
                    "current_basis requires 1–8 observations",
                ));
            }
            let current = current_basis(conn, refs, &record.current_basis)?;
            if kind == "editorial" {
                for observation in current.iter().chain(changes.iter()) {
                    let is_observation: bool = conn.query_row(
                        "SELECT EXISTS(SELECT 1 FROM observations WHERE observation_id=?1)",
                        [observation],
                        |row| row.get(0),
                    )?;
                    let associated: bool = conn.query_row(
                        "SELECT EXISTS(
                            SELECT 1 FROM memory_observations
                            WHERE memory_id = ?1 AND observation_id = ?2
                        )",
                        params![id, observation],
                        |row| row.get(0),
                    )?;
                    if is_observation && !associated {
                        return Err(LearnerError::new(
                            "invalid_variant",
                            "Editorial revision must reuse existing memory support; use new_observation or reinterpretation to add support",
                        ));
                    }
                }
            }
            if let MemoryEdit::Revise(ReviseMemoryEdit {
                review_resolution, ..
            }) = edit
            {
                if let Some(resolution) = review_resolution {
                    limits::text("review_resolution", resolution, 2048)?;
                }
                let old: Option<String> = conn.query_row(
                    "SELECT review_note FROM learner_memories WHERE memory_id=?1",
                    [&id],
                    |r| r.get(0),
                )?;
                if old.as_deref().is_some_and(|s| !s.is_empty())
                    && record.review_note.as_deref().is_none_or(str::is_empty)
                    && review_resolution
                        .as_deref()
                        .is_none_or(|s| s.trim().is_empty())
                {
                    return Err(LearnerError::new(
                        "invalid_variant",
                        "Clearing review_note requires review_resolution",
                    ));
                }
            }
            let cues = if let Some(cues) = &record.retrieval_cues {
                normalize_labels(cues, limits::CUE_COUNT, limits::CUE_BYTES)?
            } else if revision > 1 {
                crate::learner::records::stored(
                    &conn.query_row(
                        "SELECT retrieval_cues_json FROM learner_memories WHERE memory_id=?1",
                        [&id],
                        |r| r.get::<_, String>(0),
                    )?,
                    "learner_memories.retrieval_cues_json",
                )?
            } else {
                vec![]
            };
            if revision == 1 {
                conn.execute(
                    "INSERT INTO learner_memories(
                        memory_id, title, scope, summary, state_text, retrieval_cues_json,
                        review_note, current_basis_json, lifecycle, revision,
                        created_at_ms, updated_at_ms
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'active', 1, ?9, ?9)",
                    params![
                        id,
                        record.title,
                        record.scope,
                        record.summary,
                        record.state_text,
                        serde_json::to_string(&cues)?,
                        record.review_note,
                        serde_json::to_string(&current)?,
                        now
                    ],
                )?;
            } else {
                conn.execute(
                    "UPDATE learner_memories
                     SET title = ?2, scope = ?3, summary = ?4, state_text = ?5,
                         retrieval_cues_json = ?6, review_note = ?7,
                         current_basis_json = ?8, revision = ?9, updated_at_ms = ?10
                     WHERE memory_id = ?1",
                    params![
                        id,
                        record.title,
                        record.scope,
                        record.summary,
                        record.state_text,
                        serde_json::to_string(&cues)?,
                        record.review_note,
                        serde_json::to_string(&current)?,
                        revision,
                        now
                    ],
                )?;
            }
            for observation in current.iter().chain(changes.iter()) {
                conn.execute(
                    "INSERT OR IGNORE INTO memory_observations
                     SELECT ?1, observation_id FROM observations WHERE observation_id = ?2",
                    params![id, observation],
                )?;
            }
            if let Some(topics) = &record.topics {
                let topics = normalize_labels(topics, limits::TOPIC_COUNT, limits::TOPIC_BYTES)?;
                conn.execute("DELETE FROM memory_topics WHERE memory_id=?1", [&id])?;
                for topic in topics {
                    conn.execute(
                        "INSERT INTO memory_topics VALUES(?1,?2)",
                        params![id, topic],
                    )?;
                }
            }
            conn.execute(
                "UPDATE learner_memories
                 SET last_observed_at_ms = (
                     SELECT max(o.observed_at_ms)
                     FROM observations o JOIN memory_observations mo USING(observation_id)
                     WHERE mo.memory_id = ?1
                 ) WHERE memory_id = ?1",
                [&id],
            )?;
        }
        MemoryEdit::Flag(FlagMemoryEdit { review_note, .. }) => {
            limits::text("review_note", review_note, limits::REVIEW_NOTE_BYTES)?;
            if review_note.trim().is_empty() {
                return Err(LearnerError::new(
                    "invalid_variant",
                    "flag requires nonempty review_note",
                ));
            }
            conn.execute(
                "UPDATE learner_memories
                 SET review_note = ?2, revision = ?3, updated_at_ms = ?4
                 WHERE memory_id = ?1",
                params![id, review_note, revision, now],
            )?;
        }
        MemoryEdit::Retire(RetireMemoryEdit {
            replacement_ids, ..
        }) => {
            let resolved: Vec<_> = replacement_ids
                .iter()
                .map(|i| refs.get(i).unwrap_or(i))
                .collect();
            conn.execute(
                "UPDATE learner_memories
                 SET lifecycle = 'retired', replacement_ids_json = ?2,
                     revision = ?3, updated_at_ms = ?4
                 WHERE memory_id = ?1",
                params![id, serde_json::to_string(&resolved)?, revision, now],
            )?;
        }
    }
    for observation in changes {
        conn.execute(
            "INSERT OR IGNORE INTO memory_observations
                     SELECT ?1, observation_id FROM observations WHERE observation_id = ?2",
            params![id, observation],
        )?;
    }
    let mut snapshot = crate::learner::records::memory_snapshot(conn, &id)?;
    if let MemoryEdit::Revise(ReviseMemoryEdit {
        review_resolution: Some(resolution),
        ..
    }) = edit
    {
        snapshot.review_resolution = Some(resolution.clone());
    }
    conn.execute(
        "INSERT INTO memory_changes(
             memory_id, revision, change_kind, snapshot_json, reason, change_basis_json,
             created_at_ms
         ) VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![
            id,
            revision,
            kind,
            serde_json::to_string(&snapshot)?,
            reason,
            serde_json::to_string(changes)?,
            now
        ],
    )?;
    let mut fields = vec![("reason", serde_json::to_vec(reason)?.len())];
    match edit {
        MemoryEdit::Create(CreateMemoryEdit { record, .. })
        | MemoryEdit::Revise(ReviseMemoryEdit { record, .. }) => {
            fields.extend([
                ("record.title", serde_json::to_vec(&record.title)?.len()),
                ("record.scope", serde_json::to_vec(&record.scope)?.len()),
                ("record.summary", serde_json::to_vec(&record.summary)?.len()),
                (
                    "record.state_text",
                    serde_json::to_vec(&record.state_text)?.len(),
                ),
                (
                    "record.retrieval_cues",
                    serde_json::to_vec(&record.retrieval_cues)?.len(),
                ),
                ("record.topics", serde_json::to_vec(&record.topics)?.len()),
                (
                    "record.review_note",
                    serde_json::to_vec(&record.review_note)?.len(),
                ),
            ]);
            if let MemoryEdit::Revise(ReviseMemoryEdit {
                review_resolution, ..
            }) = edit
            {
                fields.push((
                    "review_resolution",
                    serde_json::to_vec(review_resolution)?.len(),
                ));
            }
        }
        MemoryEdit::Flag(FlagMemoryEdit { review_note, .. }) => {
            fields.push(("review_note", serde_json::to_vec(review_note)?.len()));
        }
        MemoryEdit::Retire(RetireMemoryEdit {
            replacement_ids, ..
        }) => {
            fields.push((
                "replacement_ids",
                serde_json::to_vec(replacement_ids)?.len(),
            ));
        }
    }
    let field = fields
        .into_iter()
        .max_by_key(|(_, bytes)| *bytes)
        .expect("memory fields are nonempty")
        .0;
    crate::learner::read::validate_memory_envelopes(
        conn,
        &id,
        revision,
        &format!("memory_edits[{edit_index}].{field}"),
    )?;
    revisions.insert(id, revision);
    Ok(())
}
fn normalize_labels(labels: &[String], count: usize, max: usize) -> Result<Vec<String>> {
    if labels.len() > count {
        return Err(LearnerError::new(
            "field_too_large",
            "topics/retrieval_cues",
        ));
    }
    let mut values = BTreeSet::new();
    for label in labels {
        limits::text("topics/retrieval_cues", label, max)?;
        let normalized = label
            .nfc()
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        if !normalized.is_empty() {
            values.insert(normalized);
        }
    }
    Ok(values.into_iter().collect())
}
/// JSON view for regression assertions over the canonical typed snapshot.
#[cfg(test)]
pub(crate) fn memory_snapshot(conn: &Connection, id: &str) -> Result<Value> {
    serde_json::to_value(crate::learner::records::memory_snapshot(conn, id)?).map_err(Into::into)
}
