use super::*;
use chrono::TimeZone;

pub(super) fn apply_observations(
    tx: &Connection,
    host: &HostContext,
    observations: &[ObservationInput],
    session: &Option<String>,
    refs: &mut BTreeMap<String, String>,
    used_events: &mut BTreeSet<String>,
    (now, notes): (i64, &mut Vec<String>),
) -> Result<()> {
    // Attempts this request stored. A paper's answers arrive together, so
    // they never make each other not cold.
    let mut this_request = BTreeSet::new();
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
        if o.attempt.is_some() && !o.attempts.is_empty() {
            return Err(field_error(
                &format!("observations[{observation_index}].attempts"),
                "Give attempt for one answer or attempts for several, not both",
                host,
            ));
        }
        if o.attempts.len() > limits::ATTEMPTS_PER_OBSERVATION {
            return Err(field_error(
                &format!("observations[{observation_index}].attempts"),
                "At most 40 attempts per observation; put the rest in a second observation",
                host,
            ));
        }
        if let Some(index) = o.attempts.iter().position(|a| a.paper.is_none()) {
            return Err(field_error(
                &format!("observations[{observation_index}].attempts[{index}].paper"),
                "attempts is for the answers on a paper, so each needs paper; record a chat answer as attempt, one per observation",
                host,
            ));
        }
        let attempts: Vec<(String, &Attempt)> = match &o.attempt {
            Some(attempt) => vec![(
                format!("observations[{observation_index}].attempt"),
                attempt,
            )],
            None => o
                .attempts
                .iter()
                .enumerate()
                .map(|(i, attempt)| {
                    (
                        format!("observations[{observation_index}].attempts[{i}]"),
                        attempt,
                    )
                })
                .collect(),
        };
        for (path, attempt) in &attempts {
            check_attempt(path, attempt, host)?;
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
        if !attempts.is_empty() {
            let response = o
                .events
                .iter()
                .find(|e| e.role == ObservationRole::LearnerResponse)
                .expect("an attempt was checked for a learner_response event");
            // When: the learner's message, as the host dated or captured it.
            let answered_at: i64 = tx.query_row(
                "SELECT COALESCE(MAX(COALESCE(e.occurred_at_ms,e.captured_at_ms)),?2)
                 FROM learning_events e JOIN observation_events oe ON oe.event_id=e.event_id
                 WHERE oe.observation_id=?1 AND oe.role='learner_response'",
                params![id, now],
                |row| row.get(0),
            )?;
            for (path, attempt) in &attempts {
                if attempt.cold
                    && let Some(earlier) =
                        earlier_today(tx, &attempt.label, answered_at, &this_request)?
                {
                    return Err(field_error(
                        &format!("{path}.cold"),
                        format!(
                            "This label already has an answer earlier today (\"{earlier}\"), so this one is not cold: cold is true only on a later day than the teaching and any earlier try. Send cold false"
                        ),
                        host,
                    ));
                }
                let attempt_id = insert_attempt(
                    tx,
                    (&id, &response.event_id, session.as_deref()),
                    path,
                    attempt,
                    (answered_at, now),
                    notes,
                )?;
                crate::learner::practice::record_attempt(
                    tx,
                    &crate::learner::practice::NewAttempt {
                        attempt_id: &attempt_id,
                        attempt,
                        path,
                        answered_at,
                        now,
                    },
                    host,
                    notes,
                )?;
                this_request.insert(attempt_id);
            }
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

/// Refuse an attempt whose facts contradict each other, before anything is
/// written. The label rule is the quick checks' rule from #26.
fn check_attempt(path: &str, attempt: &Attempt, host: &HostContext) -> Result<()> {
    let label = attempt.label.trim();
    if label.is_empty()
        || label.len() > limits::ATTEMPT_LABEL_BYTES
        || label.chars().any(char::is_control)
    {
        return Err(field_error(
            &format!("{path}.label"),
            "An attempt label is 1–120 bytes without control characters",
            host,
        ));
    }
    if names_a_problem(label) {
        return Err(field_error(
            &format!("{path}.label"),
            "A label names the skill or misunderstanding in a few words, such as \"sign of b in the quadratic formula\", never the problem: no equation and no question number. Put the problem in question. One check per label, so reuse an open check's label exactly when it is the same skill",
            host,
        ));
    }
    if attempt.question.trim().is_empty() {
        return Err(field_error(
            &format!("{path}.question"),
            "Give the question as you set it, or on a paper a line of it",
            host,
        ));
    }
    if attempt.cold && attempt.help != AttemptHelp::None {
        return Err(field_error(
            &format!("{path}.cold"),
            "A helped answer is never cold: cold is true only with help none",
            host,
        ));
    }
    if attempt.counts_for.len() > limits::COUNTS_FOR_PER_ATTEMPT {
        let mut error = field_error(
            &format!("{path}.counts_for"),
            format!("At most {} items", limits::COUNTS_FOR_PER_ATTEMPT),
            host,
        );
        error.code = "field_too_large".into();
        return Err(error);
    }
    if let Some(kind) = &attempt.kind {
        check_kind(&format!("{path}.kind"), kind, host)?;
    }
    if attempt.no_start {
        if attempt.kind.is_none() {
            return Err(field_error(
                &format!("{path}.kind"),
                "no_start needs the kind of problem, so its help level can move: give kind, such as \"discrete expected value\", or the tag when there is a course graph",
                host,
            ));
        }
        if !matches!(attempt.first_try, FirstTry::Declined | FirstTry::Wrong) {
            return Err(field_error(
                &format!("{path}.first_try"),
                "A learner with no starting move has no answer: first_try is declined when they wrote nothing, or wrong",
                host,
            ));
        }
    }
    if let Some(paper) = &attempt.paper {
        let name = paper.name.trim();
        let question = paper.question.trim();
        if name.is_empty()
            || question.is_empty()
            || name.len() > limits::ATTEMPT_LABEL_BYTES
            || question.len() > limits::ATTEMPT_LABEL_BYTES
        {
            return Err(field_error(
                &format!("{path}.paper"),
                "paper.name and paper.question are each 1–120 bytes, such as \"MockA\" and \"3b\"",
                host,
            ));
        }
        if !(paper.out_of.is_finite()
            && paper.marks.is_finite()
            && paper.out_of > 0.0
            && (0.0..=paper.out_of).contains(&paper.marks))
        {
            return Err(field_error(
                &format!("{path}.paper.marks"),
                "marks runs from 0 to out_of, and out_of is above 0",
                host,
            ));
        }
        let mark = if paper.marks == paper.out_of {
            FirstTry::Right
        } else if paper.marks == 0.0 {
            FirstTry::Wrong
        } else {
            FirstTry::Partly
        };
        if attempt.first_try != mark {
            return Err(field_error(
                &format!("{path}.first_try"),
                format!(
                    "{} of {} marks is {}: right is full marks, wrong is none, partly is between",
                    paper.marks,
                    paper.out_of,
                    mark.as_str()
                ),
                host,
            ));
        }
        let lost = paper.marks < paper.out_of;
        match &attempt.gap {
            Some(_) if paper.stretch => {
                return Err(field_error(
                    &format!("{path}.gap"),
                    "A stretch question makes no gap-log row: score it under Stretch items",
                    host,
                ));
            }
            Some(gap) if !lost && gap.gap_type != GapType::S => {
                return Err(field_error(
                    &format!("{path}.gap.type"),
                    "A full-mark answer makes a row only when it was too slow, as type S",
                    host,
                ));
            }
            Some(gap) if gap.what_went_wrong.trim().is_empty() => {
                return Err(field_error(
                    &format!("{path}.gap.what_went_wrong"),
                    "Say what was written, and what the mark scheme needed",
                    host,
                ));
            }
            None if lost && !paper.stretch && attempt.counts_for.is_empty() => {
                return Err(field_error(
                    &format!("{path}.gap"),
                    "This answer lost marks, so it makes a gap-log row: give gap {type: K, S, N, A or R, what_went_wrong}. The runtime numbers and dates the row. For a redo of an existing row, name it in counts_for instead; for a question beyond the plan, set paper.stretch true",
                    host,
                ));
            }
            _ => {}
        }
    } else if attempt.gap.is_some() {
        return Err(field_error(
            &format!("{path}.gap"),
            "gap is for a paper answer, so it needs paper. A chat miss makes its quick check by itself",
            host,
        ));
    }
    Ok(())
}

/// A kind of problem is named as a label is: a few words, never the problem.
pub(crate) fn check_kind(path: &str, kind: &str, host: &HostContext) -> Result<()> {
    let kind = kind.trim();
    if kind.is_empty()
        || kind.len() > limits::ATTEMPT_LABEL_BYTES
        || kind.chars().any(char::is_control)
    {
        return Err(field_error(
            path,
            "A kind of problem is 1–120 bytes without control characters",
            host,
        ));
    }
    if names_a_problem(kind) {
        return Err(field_error(
            path,
            "A kind names the method problems share in a few words, such as \"discrete expected value\", never one problem: no equation and no question number",
            host,
        ));
    }
    Ok(())
}

/// The question of an attempt on the same label answered earlier on the same
/// local day, from an earlier request, if there is one.
fn earlier_today(
    tx: &Connection,
    label: &str,
    answered_at: i64,
    this_request: &BTreeSet<String>,
) -> Result<Option<String>> {
    const DAY_AND_A_BIT_MS: i64 = 26 * 60 * 60 * 1000;
    let day = |ms: i64| {
        chrono::Local
            .timestamp_millis_opt(ms)
            .single()
            .map(|time| time.date_naive())
    };
    let today = day(answered_at);
    let mut statement = tx.prepare(
        "SELECT attempt_id,answered_at_ms,question FROM attempts
         WHERE label_key=?1 AND answered_at_ms BETWEEN ?2 AND ?3
         ORDER BY answered_at_ms,attempt_no",
    )?;
    let rows = statement.query_map(
        params![
            crate::learner::label_key(label),
            answered_at - DAY_AND_A_BIT_MS,
            answered_at
        ],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
            ))
        },
    )?;
    for row in rows {
        let (id, at, question) = row?;
        if !this_request.contains(&id) && today.is_some() && day(at) == today {
            return Ok(Some(question));
        }
    }
    Ok(None)
}

/// Store one attempt as its own row, and return its ID.
fn insert_attempt(
    tx: &Connection,
    (observation_id, response_event_id, session): (&str, &str, Option<&str>),
    path: &str,
    attempt: &Attempt,
    (answered_at, now): (i64, i64),
    notes: &mut Vec<String>,
) -> Result<String> {
    let attempt_id = store::new_id("a")?;
    let label = attempt
        .label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let question = limits::shorten(
        &format!("{path}.question"),
        attempt.question.trim(),
        limits::ATTEMPT_QUESTION_BYTES,
        notes,
    );
    let kind = attempt.kind.as_deref().map(crate::learner::help::kind_text);
    tx.execute(
        "INSERT INTO attempts(
            attempt_id, observation_id, response_event_id, session_id, question,
            paper, paper_question, label, label_key, mark, marks, out_of, help,
            confidence, cold, answered_at_ms, seconds, created_at_ms, slow,
            problem_kind, kind_key, no_start
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22)",
        params![
            attempt_id,
            observation_id,
            response_event_id,
            session,
            question,
            attempt.paper.as_ref().map(|p| p.name.trim()),
            attempt.paper.as_ref().map(|p| p.question.trim()),
            label,
            crate::learner::label_key(&label),
            attempt.first_try,
            attempt.paper.as_ref().map(|p| p.marks),
            attempt.paper.as_ref().map(|p| p.out_of),
            attempt.help,
            attempt.confidence,
            attempt.cold,
            answered_at,
            attempt.seconds,
            now,
            attempt.slow,
            kind,
            kind.as_deref().map(crate::learner::label_key),
            attempt.no_start
        ],
    )?;
    if let Some(kind) = &kind {
        crate::learner::help::refresh(tx, &crate::learner::label_key(kind), now)?;
    }
    Ok(attempt_id)
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
