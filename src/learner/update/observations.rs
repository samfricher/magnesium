use super::*;

pub(super) fn apply_observations(
    tx: &Connection,
    host: &HostContext,
    observations: &[ObservationInput],
    session: &Option<String>,
    refs: &mut BTreeMap<String, String>,
    used_events: &mut BTreeSet<String>,
    (now, notes): (i64, &mut Vec<String>),
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
        if let Some(attempt) = &o.attempt {
            let label = attempt.label.trim();
            if label.is_empty()
                || label.len() > limits::ATTEMPT_LABEL_BYTES
                || label.chars().any(char::is_control)
            {
                return Err(field_error(
                    &format!("observations[{observation_index}].attempt.label"),
                    "An attempt label is 1–120 bytes without control characters",
                    host,
                ));
            }
            if names_a_problem(label) {
                return Err(field_error(
                    &format!("observations[{observation_index}].attempt.label"),
                    "A label names the skill or misunderstanding in a few words, such as \"sign of b in the quadratic formula\", never the problem: no equation and no question number. One check per label, so reuse an open check's label exactly when it is the same skill",
                    host,
                ));
            }
            if !learner_response {
                return Err(field_error(
                    &format!("observations[{observation_index}].events"),
                    "An attempt is the learner's, so cite their message under role learner_response",
                    host,
                ));
            }
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
        let source_refs = labeled_sources(&o.source_refs, observation_index, notes);
        for source in &source_refs {
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
                serde_json::to_string(&source_refs)?,
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
/// A source with a missing or unknown available_form is still saved: the
/// tutor reported it, so it is labeled model_reported and the note says so.
fn labeled_sources(
    sources: &[SourceRef],
    observation_index: usize,
    notes: &mut Vec<String>,
) -> Vec<SourceRef> {
    const FORMS: [&str; 4] = ["original", "excerpt", "model_reported", "unavailable"];
    let mut relabeled = Vec::new();
    let sources = sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            let mut source = source.clone();
            if !FORMS.contains(&source.available_form.as_str()) {
                relabeled.push(index);
                source.available_form = "model_reported".into();
            }
            source
        })
        .collect();
    if !relabeled.is_empty() {
        notes.push(format!(
            "observations[{observation_index}].source_refs {relabeled:?}: available_form was missing or not one of {}, so it was saved as model_reported",
            FORMS.join(", ")
        ));
    }
    sources
}

/// A label that names one problem rather than a skill: it holds an equation,
/// or a question number such as "Q3" or "question 2".
fn names_a_problem(label: &str) -> bool {
    let words = label
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let numbered = |word: &str| word.chars().next().is_some_and(|c| c.is_ascii_digit());
    label.contains('=')
        || words.iter().any(|word| {
            word.strip_prefix('q')
                .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
        })
        || words.windows(2).any(|pair| {
            matches!(
                pair[0].as_str(),
                "q" | "question" | "problem" | "exercise" | "task"
            ) && numbered(&pair[1])
        })
}

#[cfg(test)]
mod label_tests {
    #[test]
    fn a_label_that_names_one_problem_is_refused() {
        for label in [
            "quadratic formula: 2x² − 3x − 2 = 0",
            "Q3 of the diagnostic",
            "question 2",
            "problem 4b",
        ] {
            assert!(super::names_a_problem(label), "{label}");
        }
        for label in [
            "sign of b in the quadratic formula",
            "two-step equations",
            "quotient rule",
            "base 10 logarithms",
            "questions about limits",
        ] {
            assert!(!super::names_a_problem(label), "{label}");
        }
    }
}
