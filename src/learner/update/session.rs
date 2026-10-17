use super::*;
use crate::learner::records::{StoredCheckpoint, event_facts};

/// One interpretation of the typed lifecycle request, applied under the same
/// transaction as observations, memories, checkpoint and receipt.
#[derive(Clone, Copy)]
pub(super) enum SessionTransition<'a> {
    None,
    Create(&'a StartSessionEdit),
    Resume(&'a ResumeSessionEdit),
    CheckpointOnly(&'a UseSessionEdit),
    Pause(&'a PauseSessionEdit),
    Complete(&'a CompleteSessionEdit),
}
impl<'a> SessionTransition<'a> {
    pub(super) fn from_request(request: &'a UpdateRequest) -> Self {
        match request.session.as_ref() {
            None => Self::None,
            Some(SessionEdit::Start(edit)) => Self::Create(edit),
            Some(SessionEdit::Resume(edit)) => Self::Resume(edit),
            Some(SessionEdit::Use(edit)) => Self::CheckpointOnly(edit),
            Some(SessionEdit::Pause(edit)) => Self::Pause(edit),
            Some(SessionEdit::Complete(edit)) => Self::Complete(edit),
        }
    }
}

#[derive(Default)]
struct HostBinding {
    session: Option<String>,
    active: bool,
    recording: bool,
}
impl HostBinding {
    fn decode(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            session: row.get(0)?,
            active: row.get(1)?,
            recording: row.get(2)?,
        })
    }
    fn load(conn: &Connection, host: &HostContext) -> Result<Self> {
        Ok(conn
            .query_row(
                "SELECT session_id, learning_active, recording_enabled
             FROM host_bindings
             WHERE host_kind = ?1 AND host_thread_id = ?2",
                params![host.host_kind, host.host_thread_id],
                Self::decode,
            )
            .optional()?
            .unwrap_or(Self {
                recording: true,
                ..Self::default()
            }))
    }
}

/// A pause control stops capture in its own conversation only; the tutor then
/// pauses or completes the lesson from that same, now inactive, conversation.
/// A bare stop of this conversation's own still-active lesson only reduces
/// activity, so it is admitted. Any other edit keeps the inactive refusal and
/// its complete_captured route.
fn bare_stop_of_active_lesson(
    conn: &Connection,
    r: &UpdateRequest,
    transition: SessionTransition<'_>,
    bound: Option<&str>,
) -> Result<bool> {
    let target = match transition {
        SessionTransition::Pause(PauseSessionEdit { session_id, .. })
        | SessionTransition::Complete(CompleteSessionEdit { session_id, .. }) => session_id,
        _ => return Ok(false),
    };
    let bare = r.native_capture.is_none()
        && r.turn_ref.is_none()
        && r.reported_events.is_empty()
        && r.observations.is_empty()
        && r.memory_edits.is_empty()
        && r.checkpoint.is_none()
        && r.event_dispositions.is_empty()
        && r.review_updates.is_empty();
    if !bare || bound != Some(target.as_str()) {
        return Ok(false);
    }
    let status: Option<String> = conn
        .query_row(
            "SELECT status FROM learning_sessions WHERE session_id = ?1",
            [target],
            |row| row.get(0),
        )
        .optional()?;
    Ok(status.as_deref() == Some("active"))
}

/// Semantic admission is independent of raw capture admission. A completion
/// consumes scoped, already recorded learner work without changing permission.
pub(super) fn admit_semantic_update(
    conn: &Connection,
    host: &HostContext,
    r: &UpdateRequest,
    transition: SessionTransition<'_>,
) -> Result<()> {
    let HostBinding {
        session: bound,
        active,
        recording,
    } = HostBinding::load(conn, host)?;
    if !recording {
        return Err(LearnerError::new(
            "recording_disabled",
            "Recording changed before semantic update",
        ));
    }
    if r.mode == UpdateMode::Normal {
        if !active
            && !SessionEdit::accepts_capture_operation(
                r.session.as_ref().map(SessionEdit::operation),
            )
            && !bare_stop_of_active_lesson(conn, r, transition, bound.as_deref())?
        {
            let mut error = LearnerError::new(
                "learning_inactive",
                "Start or resume explicitly, or complete already captured work with mode:complete_captured",
            );
            // A lesson paused or completed from another conversation leaves
            // this one bound but inactive. Say so, and keep the route to any
            // captured work that still needs completing: already-recorded
            // learner turns can finish with complete_captured while inactive.
            let state: Option<(String, i64)> = match &bound {
                Some(id) => conn
                    .query_row(
                        "SELECT status, revision FROM learning_sessions WHERE session_id = ?1",
                        [id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()?,
                None => None,
            };
            if let (Some(id), Some((status, revision))) = (&bound, state)
                && matches!(status.as_str(), "paused" | "completed")
            {
                let pending: i64 = conn.query_row(
                    "SELECT count(*) FROM learning_events
                     WHERE session_id = ?1 AND speaker = 'learner' AND processing = 'pending'",
                    [id],
                    |row| row.get(0),
                )?;
                let redundant_pause = matches!(transition, SessionTransition::Pause(_));
                let opening = if redundant_pause {
                    format!(
                        "This lesson is already {status}, possibly from another conversation; no further pause is needed."
                    )
                } else {
                    format!(
                        "This conversation's lesson is already {status}, possibly from another conversation, so learning is inactive here."
                    )
                };
                let completion = if pending > 0 {
                    format!(
                        " {pending} captured learner turn(s) still await interpretation: read them with next_call, then finish them with mode \"complete_captured\" at session revision {revision} with their event dispositions before saying they are saved."
                    )
                } else {
                    " No captured learner turns await interpretation in this lesson.".to_owned()
                };
                error.recovery = Some(json!({
                    "session_id": id,
                    "session_status": status,
                    "session_revision": revision,
                    "pending_learner_events": pending,
                    "next_call": read::NextCall::pending_events(id),
                    "instruction": format!("{opening} Do not resume it unless the learner asks to continue.{completion}"),
                }));
            }
            return Err(error);
        }
        return Ok(());
    }
    if !r.reported_events.is_empty() || r.native_capture.is_some() {
        return Err(LearnerError::new(
            "invalid_variant",
            "Complete-captured cannot record raw input",
        ));
    }
    let session_id = match transition {
        SessionTransition::None => None,
        SessionTransition::CheckpointOnly(edit) if edit.goal.is_none() => Some(&edit.session_id),
        SessionTransition::Pause(edit) => Some(&edit.session_id),
        SessionTransition::Complete(edit) => Some(&edit.session_id),
        _ => {
            return Err(LearnerError::new(
                "invalid_variant",
                "Complete-captured cannot start, resume, or change a lesson goal",
            ));
        }
    };
    if session_id.is_some_and(|id| bound.as_ref() != Some(id)) {
        return Err(LearnerError::new(
            "event_scope_conflict",
            "Completion must preserve the current host session association",
        ));
    }
    if let Some(task) = r
        .checkpoint
        .as_ref()
        .and_then(|checkpoint| checkpoint.pending_task.as_ref())
    {
        let stored = bound
            .as_ref()
            .map(|id| {
                let text: String = conn.query_row(
                    "SELECT checkpoint_json FROM learning_sessions WHERE session_id = ?1",
                    [id],
                    |row| row.get(0),
                )?;
                StoredCheckpoint::decode(&text, id)
            })
            .transpose()?;
        if stored.as_ref().and_then(StoredCheckpoint::pending_task) != Some(task) {
            return Err(field_error(
                "checkpoint.pending_task",
                "Complete-captured can preserve the existing task or clear it, but cannot issue or change a task",
                host,
            ));
        }
    }
    if r.review_updates
        .iter()
        .any(|edit| matches!(edit, ReviewEdit::Create(_)))
    {
        return Err(field_error(
            "review_updates",
            "Complete-captured cannot create a review suggestion",
            host,
        ));
    }
    let turn = r.turn_ref.as_ref().ok_or_else(|| {
        LearnerError::new(
            "invalid_variant",
            "Complete-captured requires a pending captured learner turn_ref",
        )
    })?;
    let learner_turn: bool = conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM learning_events
            WHERE event_id = ?1 AND speaker = 'learner' AND processing = 'pending'
              AND ((host_kind = ?2 AND host_thread_id = ?3)
                   OR (?4 IS NOT NULL AND session_id = ?4))
        )",
        params![turn, host.host_kind, host.host_thread_id, bound],
        |row| row.get(0),
    )?;
    if !learner_turn {
        return Err(LearnerError::new(
            "event_scope_conflict",
            "Completion turn_ref must identify pending learner work owned by this host or its bound session",
        ));
    }
    if !r
        .event_dispositions
        .iter()
        .any(|disposition| &disposition.event_id == turn)
    {
        return Err(LearnerError::new(
            "invalid_variant",
            "Complete-captured requires an explicit semantic disposition for turn_ref",
        ));
    }
    let scoped = |id: &str| -> Result<()> {
        let valid: bool = conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM learning_events WHERE event_id = ?1
                  AND ((host_kind = ?2 AND host_thread_id = ?3)
                       OR (?4 IS NOT NULL AND session_id = ?4))
            )",
            params![id, host.host_kind, host.host_thread_id, bound],
            |row| row.get(0),
        )?;
        if valid {
            Ok(())
        } else {
            Err(LearnerError::new(
                "event_scope_conflict",
                format!("Completion event {id} is outside the current host/session"),
            ))
        }
    };
    for observation in &r.observations {
        for event in &observation.events {
            scoped(&event.event_id)?;
        }
    }
    for disposition in &r.event_dispositions {
        scoped(&disposition.event_id)?;
    }
    for edit in &r.memory_edits {
        let basis = match edit {
            MemoryEdit::Create(edit) => &edit.change_basis,
            MemoryEdit::Revise(edit) => &edit.change_basis,
            MemoryEdit::Flag(edit) => &edit.change_basis,
            MemoryEdit::Retire(edit) => &edit.change_basis,
        };
        for reference in basis {
            if let ChangeBasisRef::Event(event) = reference {
                scoped(event)?;
            }
        }
    }
    for edit in &r.review_updates {
        if let ReviewEdit::Update(edit) = edit {
            if edit.activity_event_id.as_ref() != Some(turn) {
                return Err(LearnerError::new(
                    "invalid_variant",
                    "Completion review updates require the selected closing learner turn as activity_event_id",
                ));
            }
            scoped(turn)?;
        }
    }
    Ok(())
}

pub(super) fn apply_session(
    conn: &Connection,
    host: &HostContext,
    r: &UpdateRequest,
    transition: SessionTransition<'_>,
    refs: &mut BTreeMap<String, String>,
    now: i64,
    captured: &[String],
) -> Result<(Option<String>, bool)> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT session_id FROM host_bindings WHERE host_kind=?1 AND host_thread_id=?2",
            params![host.host_kind, host.host_thread_id],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    let (id, active) = match transition {
        SessionTransition::None => return Ok((existing, false)),
        SessionTransition::Create(StartSessionEdit {
            client_ref, goal, ..
        }) => {
            limits::text("session.goal", goal, limits::SESSION_GOAL_BYTES)?;
            let id = store::new_id("s")?;
            insert_ref(refs, client_ref, &id)?;
            conn.execute(
                "INSERT INTO learning_sessions VALUES(?1,?2,'active','{}',1,?3,?3)",
                params![id, goal, now],
            )?;
            (id, true)
        }
        SessionTransition::CheckpointOnly(UseSessionEdit {
            session_id,
            expected_revision,
            goal,
        }) => {
            let status: String = conn
                .query_row(
                    "SELECT status FROM learning_sessions WHERE session_id=?1",
                    [session_id],
                    |row| row.get(0),
                )
                .optional()?
                .ok_or_else(|| unknown(session_id))?;
            if status != "active" && r.mode == UpdateMode::Normal {
                return Err(LearnerError::new(
                    "invalid_variant",
                    "Use requires an active session; resume explicitly",
                ));
            }
            if existing.as_ref() != Some(session_id) {
                return Err(LearnerError::new(
                    "event_scope_conflict",
                    "Use requires this host's bound session; resume explicitly to change the association",
                ));
            }
            let next = cas(
                conn,
                "learning_sessions",
                "session_id",
                session_id,
                *expected_revision,
            )?;
            if let Some(goal) = goal {
                limits::text("session.goal", goal, limits::SESSION_GOAL_BYTES)?;
            }
            conn.execute(
                "UPDATE learning_sessions
                 SET goal = coalesce(?2, goal), revision = ?3, updated_at_ms = ?4
                 WHERE session_id = ?1",
                params![session_id, goal, next, now],
            )?;
            // A guarded checkpoint update is not an activity transition.
            return Ok((Some(session_id.clone()), false));
        }
        SessionTransition::Resume(ResumeSessionEdit {
            session_id,
            expected_revision,
            ..
        })
        | SessionTransition::Pause(PauseSessionEdit {
            session_id,
            expected_revision,
        })
        | SessionTransition::Complete(CompleteSessionEdit {
            session_id,
            expected_revision,
        }) => {
            if !matches!(transition, SessionTransition::Resume(_))
                && existing.as_ref() != Some(session_id)
            {
                return Err(LearnerError::new(
                    "event_scope_conflict",
                    "Pause/complete requires this host's bound session",
                ));
            }
            let next = cas(
                conn,
                "learning_sessions",
                "session_id",
                session_id,
                *expected_revision,
            )?;
            let status = match transition {
                SessionTransition::Pause(PauseSessionEdit { .. }) => "paused",
                SessionTransition::Complete(CompleteSessionEdit { .. }) => "completed",
                _ => "active",
            };
            conn.execute(
                "UPDATE learning_sessions
                 SET status = ?2, revision = ?3, updated_at_ms = ?4
                 WHERE session_id = ?1",
                params![session_id, status, next, now],
            )?;
            (session_id.clone(), status == "active")
        }
    };
    conn.execute(
        "INSERT INTO host_bindings(host_kind, host_thread_id, session_id, learning_active)
         VALUES(?1, ?2, ?3, ?4)
         ON CONFLICT(host_kind, host_thread_id) DO UPDATE
         SET session_id = excluded.session_id, learning_active = excluded.learning_active",
        params![host.host_kind, host.host_thread_id, id, active],
    )?;
    // Several conversations may share an active lesson (a fork resumes it
    // alongside the original), but a paused or completed lesson stops capture
    // in every conversation still bound to it, not only the one that asked.
    if matches!(
        transition,
        SessionTransition::Pause(_) | SessionTransition::Complete(_)
    ) {
        conn.execute(
            "UPDATE host_bindings SET learning_active = 0
             WHERE session_id = ?1 AND learning_active = 1
               AND NOT (host_kind = ?2 AND host_thread_id = ?3)",
            params![id, host.host_kind, host.host_thread_id],
        )?;
    }
    let selected = match transition {
        SessionTransition::Create(edit) => &edit.selected_unbound_event_ids,
        SessionTransition::Resume(edit) => &edit.selected_unbound_event_ids,
        _ => return Ok((Some(id), false)),
    };
    let changed = attach_session_inputs(conn, host, &id, captured, selected)?;
    Ok((Some(id), changed))
}

fn attach_session_inputs(
    conn: &Connection,
    host: &HostContext,
    session: &str,
    captured: &[String],
    selected: &[String],
) -> Result<bool> {
    if selected.len() > limits::RAW_EVENTS_PER_BATCH {
        return Err(LearnerError::new(
            "field_too_large",
            "session.selected_unbound_event_ids accepts at most 32 IDs",
        ));
    }
    let mut selected_ids = BTreeSet::new();
    for id in selected {
        limits::text(
            "session.selected_unbound_event_ids",
            id,
            limits::REFERENCE_BYTES,
        )?;
        if !selected_ids.insert(id) {
            return Err(LearnerError::new(
                "invalid_variant",
                "session.selected_unbound_event_ids contains duplicate IDs",
            ));
        }
        let selectable: bool = conn.query_row(
            "SELECT EXISTS(
            SELECT 1 FROM learning_events
            WHERE event_id = ?1 AND session_id IS NULL
              AND speaker = 'learner' AND processing = 'pending'
        )",
            [id],
            |row| row.get(0),
        )?;
        if !selectable {
            return Err(LearnerError::new(
                "event_scope_conflict",
                format!("Selected input {id} is not pending unbound learner work"),
            ));
        }
    }
    for id in captured {
        let owned: bool = conn.query_row(
            "SELECT EXISTS(
            SELECT 1 FROM learning_events
            WHERE event_id = ?1 AND host_kind = ?2 AND host_thread_id = ?3
        )",
            params![id, host.host_kind, host.host_thread_id],
            |row| row.get(0),
        )?;
        if !owned {
            return Err(LearnerError::new(
                "event_scope_conflict",
                format!("Captured input {id} belongs to another host"),
            ));
        }
    }
    // Selection never changes original capture ownership; only the deliberate
    // lesson association changes, in this semantic transaction.
    let mut changed = false;
    for id in captured.iter().chain(selected).collect::<BTreeSet<_>>() {
        changed |= conn.execute(
            "UPDATE learning_events SET session_id=?2 WHERE event_id=?1 AND session_id IS NULL",
            params![id, session],
        )? > 0;
    }
    Ok(changed)
}
pub(super) fn resolve_checkpoint(
    conn: &Connection,
    host: &HostContext,
    c: &Checkpoint,
    refs: &BTreeMap<String, String>,
    revisions: &BTreeMap<String, i64>,
) -> Result<Value> {
    for (name, value) in [
        ("current_problem", &c.current_problem),
        ("working_diagnosis", &c.working_diagnosis),
        ("next_action", &c.next_action),
    ] {
        limits::text(name, value, limits::CHECKPOINT_FIELD_BYTES)?;
    }
    if let Some(plan) = &c.plan_text {
        limits::text("plan_text", plan, limits::PLAN_BYTES)?;
    }
    if c.memory_refs.len() > 16 {
        return Err(LearnerError::new(
            "field_too_large",
            "checkpoint.memory_refs",
        ));
    }
    let mut value = serde_json::to_value(c)?;
    for (i, m) in c.memory_refs.iter().enumerate() {
        let id = refs.get(&m.memory_id).unwrap_or(&m.memory_id);
        let revision = match m.revision {
            MemoryRevision::AfterEdit => *revisions.get(id).ok_or_else(|| unknown(id))?,
            MemoryRevision::Known(revision) => revision,
        };
        let actual: i64 = conn
            .query_row(
                "SELECT revision FROM learner_memories WHERE memory_id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| unknown(id))?;
        if revision > actual || revision < 1 {
            return Err(unknown(id));
        }
        value["memory_refs"][i] = json!({"memory_id":id,"revision":revision});
    }
    if let Some(task) = &c.pending_task {
        if task.help_refs.len() > 16 || task.observed_delivery_refs.len() > 16 {
            return Err(LearnerError::new("field_too_large", "pending_task refs"));
        }
        for event in std::iter::once(&task.proposal_event_ref)
            .chain(task.help_refs.iter())
            .chain(task.reference_answer_ref.iter())
        {
            event_facts(conn, event)?;
        }
        for (index, event) in task.observed_delivery_refs.iter().enumerate() {
            let path = format!("checkpoint.pending_task.observed_delivery_refs[{index}]");
            let facts = event_facts(conn, event).map_err(|error| {
                if error.code == "unknown_reference" {
                    field_error(&path, "Observed delivery requires an existing tutor-authored event explicitly marked delivery:observed; preserve the task proposal, conditions, and other context refs and leave observed_delivery_refs empty when delivery is unestablished", host)
                } else { error }
            })?;
            if !facts.observed_tutor_delivery() {
                return Err(field_error(
                    &path,
                    "Observed delivery requires a tutor-authored event explicitly marked delivery:observed; origin alone does not establish delivery. Preserve the task proposal, conditions, and other context refs and leave observed_delivery_refs empty when delivery is unestablished",
                    host,
                ));
            }
        }
    }
    limits::text(
        "checkpoint",
        &serde_json::to_string(&value)?,
        limits::CHECKPOINT_BYTES,
    )?;
    Ok(value)
}

#[derive(Debug)]
struct SessionState {
    revision: i64,
    status: String,
}
impl SessionState {
    fn decode(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            revision: row.get(0)?,
            status: row.get(1)?,
        })
    }
}

pub(super) fn require_checkpoint_guard(
    conn: &Connection,
    request: &UpdateRequest,
    session: Option<&str>,
) -> Result<()> {
    if request.checkpoint.is_none() || request.session.is_some() {
        return Ok(());
    }
    let current = session
        .map(|id| {
            conn.query_row(
                "SELECT revision, status FROM learning_sessions WHERE session_id = ?1",
                [id],
                SessionState::decode,
            )
            .map(|state| {
                (state.status == "active" || request.mode == UpdateMode::CompleteCaptured).then(
                    || {
                        SessionEdit::Use(UseSessionEdit {
                            session_id: id.into(),
                            expected_revision: state.revision,
                            goal: None,
                        })
                    },
                )
            })
        })
        .transpose()?
        .flatten();
    let mut error = LearnerError::new(
        "session_guard_required",
        "Keep the checkpoint and add a guarded session operation with session_id and expected_revision. Read context.status to reconcile the current checkpoint/revision; do not drop it to bypass the guard.",
    );
    error.recovery = Some(json!({
        "field_path":"session",
        "suggested_session":current,
        "next_call":read::NextCall::current_status(session),
        "preserve_fields":["checkpoint","observations","memory_edits","event_dispositions"],
        "instruction":"Read current session state and retry once with a fresh request_id and the intended checkpoint. Complete-captured may use the bound session without activating it; for ordinary updates, explicitly start or resume the intended lesson when no usable session is bound."
    }));
    Err(error)
}
