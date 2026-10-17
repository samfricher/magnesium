//! Pure bounded admission for batch-local definitions and mutation targets.
use super::*;

/// One typed walk establishes unambiguous commands before the semantic writer
/// lock. Support references are deliberately outside these definition/target
/// namespaces: one event or observation may support several descriptions.
pub(super) fn validate_batch_keys(r: &UpdateRequest, host: &HostContext) -> Result<()> {
    if r.observations.len() > limits::OBSERVATIONS_PER_BATCH
        || r.memory_edits.len() > limits::MEMORY_EDITS_PER_BATCH
        || r.review_updates.len() > limits::REVIEWS_PER_BATCH
        || r.event_dispositions.len() > limits::DISPOSITIONS_PER_BATCH
    {
        return Err(LearnerError::new("field_too_large", "semantic batch"));
    }
    let mut local_names = BTreeMap::new();
    let mut memory_targets = BTreeMap::new();
    let mut review_targets = BTreeMap::new();
    let mut disposition_targets = BTreeMap::new();
    if let Some(SessionEdit::Start(edit)) = &r.session {
        local_name(
            &mut local_names,
            &edit.client_ref,
            "session.client_ref".into(),
            host,
        )?;
    }
    for (index, observation) in r.observations.iter().enumerate() {
        local_name(
            &mut local_names,
            &observation.client_ref,
            format!("observations[{index}].client_ref"),
            host,
        )?;
    }
    for (index, edit) in r.memory_edits.iter().enumerate() {
        let id = match edit {
            MemoryEdit::Create(edit) => {
                local_name(
                    &mut local_names,
                    &edit.client_ref,
                    format!("memory_edits[{index}].client_ref"),
                    host,
                )?;
                continue;
            }
            MemoryEdit::Revise(edit) => &edit.memory_id,
            MemoryEdit::Flag(edit) => &edit.memory_id,
            MemoryEdit::Retire(edit) => &edit.memory_id,
        };
        unique(
            &mut memory_targets,
            id,
            format!("memory_edits[{index}].memory_id"),
            host,
        )?;
    }
    for (index, edit) in r.review_updates.iter().enumerate() {
        match edit {
            ReviewEdit::Create(edit) => local_name(
                &mut local_names,
                &edit.client_ref,
                format!("review_updates[{index}].client_ref"),
                host,
            )?,
            ReviewEdit::Update(edit) => unique(
                &mut review_targets,
                &edit.review_id,
                format!("review_updates[{index}].review_id"),
                host,
            )?,
        }
    }
    for (index, disposition) in r.event_dispositions.iter().enumerate() {
        unique(
            &mut disposition_targets,
            &disposition.event_id,
            format!("event_dispositions[{index}].event_id"),
            host,
        )?;
    }
    Ok(())
}

fn unique<'a>(
    seen: &mut BTreeMap<&'a str, String>,
    key: &'a str,
    path: String,
    host: &HostContext,
) -> Result<()> {
    if let Some(first_path) = seen.get(key) {
        return Err(field_error(
            &path,
            format!("Duplicate target; first supplied at {first_path}. Keep one final command."),
            host,
        ));
    }
    seen.insert(key, path);
    Ok(())
}

fn local_name<'a>(
    seen: &mut BTreeMap<&'a str, String>,
    name: &'a str,
    path: String,
    host: &HostContext,
) -> Result<()> {
    if name.len() > limits::REFERENCE_BYTES {
        let mut error = field_error(
            &path,
            format!(
                "Request-local name exceeds {} UTF-8 bytes",
                limits::REFERENCE_BYTES
            ),
            host,
        );
        // Preserve the established size-error classification while returning
        // the same indexed repair guidance as other batch-key diagnostics.
        error.code = "field_too_large".into();
        return Err(error);
    }
    if name.is_empty() {
        return Err(field_error(
            &path,
            "Use one nonempty request-local name of at most 256 UTF-8 bytes",
            host,
        ));
    }
    if store::resembles_persistent_id(name) {
        return Err(field_error(
            &path,
            "client_ref names a new request-local object. Use a local name such as coefficient_memory, not a runtime ID. Keep existing IDs in their reference fields.",
            host,
        ));
    }
    unique(seen, name, path, host)
}
