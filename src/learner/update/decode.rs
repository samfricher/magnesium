use super::*;
pub(crate) fn field_error(
    path: &str,
    detail: impl std::fmt::Display,
    host: &HostContext,
) -> LearnerError {
    // Unknown JSON keys are request-controlled too. Keep recovery bounded even
    // when a malicious key is much longer than any legitimate protocol path.
    let path = if path.len() > 512 {
        const SUFFIX: &str = " [path truncated]";
        let mut end = 512 - SUFFIX.len();
        while !path.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}{SUFFIX}", &path[..end])
    } else {
        path.to_owned()
    };
    let mut failure = LearnerError::new("invalid_variant", format!("{path}: {detail}"));
    let mut arguments =
        json!({"action":"instructions","stage":"interpret_and_repair","operation":"tool_recipes"});
    if matches!(host.host_kind.as_str(), "codex" | "claude" | "opencode") {
        arguments["host"] = json!(host.host_kind);
    }
    // Tutors reach for an update field when the learner asks to pause; point
    // them at the control action and the workflow that serves its recipe.
    let instruction = if path == "control" {
        arguments["stage"] = json!("consolidate_and_resume");
        "Controls are their own action, not an update field: {\"schema\":\"learner-model/v2\",\"action\":\"control\",\"request_id\":<new>,\"control\":\"pause\"|\"stop\"|\"recording_off\"|\"recording_on\",\"turn_ref\":<the prompt hook's current event>}. It stops this conversation; session.pause then stops the lesson in every conversation."
    } else {
        "Correct the indicated field using the tool contract. Preserve captured_event_ids and the complete intended checkpoint when retrying."
    };
    failure.recovery = Some(
        json!({"field_path":path,"tool":"context","arguments":arguments,"instruction":instruction}),
    );
    failure
}
/// Prefix actual deserializer paths, never Serde's error wording. Operations
/// use this after selecting their typed payload, avoiding Serde tag buffering.
pub(crate) fn decode_at<T: serde::de::DeserializeOwned>(
    payload: &Value,
    prefix: &str,
    host: &HostContext,
) -> Result<T> {
    serde_path_to_error::deserialize(payload).map_err(|error| {
        let nested = error.path().to_string();
        let path = match (prefix.is_empty(), nested.is_empty() || nested == ".") {
            (true, true) => "request".to_owned(),
            (_, true) => prefix.to_owned(),
            (true, false) => nested,
            (false, false) if nested.starts_with('[') => format!("{prefix}{nested}"),
            (false, false) => format!("{prefix}.{nested}"),
        };
        field_error(&path, error.inner(), host)
    })
}
/// Ordinary fields and each selected operation payload are typed once. The
/// generic request is the same protocol shape, not an alternate schema model.
pub(crate) fn decode_update_request(payload: &Value, host: &HostContext) -> Result<UpdateRequest> {
    let envelope: UpdateRequest<Value, Value, Value> = decode_at(payload, "", host)?;
    let session = envelope
        .session
        .as_ref()
        .map(|value| SessionEdit::decode(value, "session", host))
        .transpose()?;
    let memory_edits = envelope
        .memory_edits
        .iter()
        .enumerate()
        .map(|(index, value)| MemoryEdit::decode(value, &format!("memory_edits[{index}]"), host))
        .collect::<Result<_>>()?;
    let review_updates = envelope
        .review_updates
        .iter()
        .enumerate()
        .map(|(index, value)| ReviewEdit::decode(value, &format!("review_updates[{index}]"), host))
        .collect::<Result<_>>()?;
    Ok(UpdateRequest {
        native_capture: envelope.native_capture,
        schema: envelope.schema,
        action: envelope.action,
        mode: envelope.mode,
        request_id: envelope.request_id,
        turn_ref: envelope.turn_ref,
        session,
        reported_events: envelope.reported_events,
        observations: envelope.observations,
        memory_edits,
        checkpoint: envelope.checkpoint,
        event_dispositions: envelope.event_dispositions,
        review_updates,
    })
}
