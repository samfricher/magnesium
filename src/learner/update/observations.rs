use super::*;

pub(super) fn apply_observations(
    tx: &Connection,
    host: &HostContext,
    observations: &[ObservationInput],
    session: &Option<String>,
    refs: &mut BTreeMap<String, String>,
    used_events: &mut BTreeSet<String>,
    now: i64,
) -> Result<()> {
    for (observation_index, o) in observations.iter().enumerate() {
        limits::text(
            "observation.description",
            &o.description,
            limits::OBSERVATION_BYTES,
        )?;
        limits::text(
            "observation.conditions_text",
            &o.conditions_text,
            limits::CONDITIONS_BYTES,
        )?;
        limits::text(
            "observation.assistance_text",
            &o.assistance_text,
            limits::ASSISTANCE_BYTES,
        )?;
        if o.events.is_empty() || o.events.len() > 16 {
            return Err(LearnerError::new(
                "invalid_variant",
                "observation.events requires 1–16",
            ));
        }
        limits::text(
            "observation.source_refs",
            &serde_json::to_string(&o.source_refs)?,
            limits::SOURCE_REFS_BYTES,
        )?;
        let excerpt_bytes: usize = o
            .events
            .iter()
            .flat_map(|event| event.excerpts.iter())
            .map(String::len)
            .sum();
        if excerpt_bytes > limits::EXCERPTS_BYTES {
            return Err(LearnerError::new(
                "field_too_large",
                "observation.events.excerpts: combined excerpts exceed 16 KiB; reference original event ranges instead",
            ));
        }
        let mut observed = None;
        let mut learner_response = false;
        for (event_index, e) in o.events.iter().enumerate() {
            if e.excerpts.len() > 4 {
                return Err(LearnerError::new(
                    "field_too_large",
                    "observation.events.excerpts: accepts at most 4 strings",
                ));
            }
            let event = crate::learner::records::event_facts(tx, &e.event_id)?;
            if e.role == ObservationRole::LearnerResponse {
                if event.speaker != Speaker::Learner {
                    return Err(field_error(
                        &format!("observations[{observation_index}].events[{event_index}].role"),
                        "learner_response requires a learner-authored event; keep tutor help under its actual supporting role",
                        host,
                    ));
                }
                learner_response = true;
            }
            if event.speaker == Speaker::Learner {
                observed = std::cmp::max(observed, event.occurred_at_ms);
            }
            if !e.excerpts.is_empty() {
                let text: String = tx.query_row(
                    "SELECT text FROM learning_events WHERE event_id=?1",
                    [&e.event_id],
                    |row| row.get(0),
                )?;
                for excerpt in &e.excerpts {
                    if !text
                        .replace("\r\n", "\n")
                        .contains(&excerpt.replace("\r\n", "\n"))
                    {
                        return Err(LearnerError::new("excerpt_mismatch", &e.event_id));
                    }
                }
            }
            used_events.insert(e.event_id.clone());
        }
        if matches!(
            o.kind,
            ObservationKind::LearnerWork
                | ObservationKind::LearnerQuestion
                | ObservationKind::SelfReport
        ) && !learner_response
        {
            return Err(field_error(
                &format!("observations[{observation_index}].events"),
                "Learner work, questions, and self-reports require the relevant learner input under role learner_response",
                host,
            ));
        }
        for source in &o.source_refs {
            if !["original", "excerpt", "model_reported", "unavailable"]
                .contains(&source.available_form.as_str())
            {
                return Err(LearnerError::new(
                    "invalid_variant",
                    "source.available_form",
                ));
            }
            if let Some(path) = &source.path
                && (std::path::Path::new(path).is_absolute()
                    || std::path::Path::new(path)
                        .components()
                        .any(|c| matches!(c, std::path::Component::ParentDir)))
            {
                return Err(LearnerError::new(
                    "invalid_variant",
                    "source.path must be authorized relative path",
                ));
            }
        }
        let id = store::new_id("o")?;
        insert_ref(refs, &o.client_ref, &id)?;
        tx.execute(
            "INSERT INTO observations(
                observation_id, session_id, kind, description, conditions_text,
                assistance_text, source_refs_json, observed_at_ms, created_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id,
                session,
                o.kind,
                o.description,
                o.conditions_text,
                o.assistance_text,
                serde_json::to_string(&o.source_refs)?,
                observed,
                now
            ],
        )?;
        for e in &o.events {
            tx.execute(
                "INSERT INTO observation_events VALUES(?1,?2,?3,?4)",
                params![id, e.event_id, e.role, serde_json::to_string(&e.excerpts)?],
            )?;
        }
        let field = [
            ("description", serde_json::to_vec(&o.description)?.len()),
            (
                "conditions_text",
                serde_json::to_vec(&o.conditions_text)?.len(),
            ),
            (
                "assistance_text",
                serde_json::to_vec(&o.assistance_text)?.len(),
            ),
            ("events", serde_json::to_vec(&o.events)?.len()),
            ("source_refs", serde_json::to_vec(&o.source_refs)?.len()),
        ]
        .into_iter()
        .max_by_key(|(_, bytes)| *bytes)
        .expect("observation fields are nonempty")
        .0;
        crate::learner::read::validate_observation_envelope(
            tx,
            &id,
            &format!("observations[{observation_index}].{field}"),
        )?;
    }
    Ok(())
}
