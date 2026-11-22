use super::*;
pub(crate) fn field_error(
    path: &str,
    detail: impl std::fmt::Display,
    host: &HostContext,
) -> LearnerError {
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
    if crate::adapter::HostKind::parse(&host.host_kind).is_ok() {
        arguments["host"] = json!(host.host_kind);
    }
    failure.recovery = Some(
        json!({"field_path":path,"tool":"context","arguments":arguments,"instruction":"Correct the indicated field using the tool contract. Preserve captured_event_ids and the complete intended checkpoint when retrying."}),
    );
    failure
}
const END_FOR_NOW: &str = "A lesson stays open and keeps everything until it is completed. When the learner is done for now, send {\"schema\":\"learner-model/v2\",\"action\":\"end_sitting\"}; the next message in this conversation goes on with the same lesson. When they have finished it for good, send session.complete.";
pub(crate) fn refuse_removed_fields(payload: &Value, host: &HostContext) -> Result<()> {
    let removed = if payload.get("control").is_some() {
        (
            "control",
            "pause, stop and recording on or off no longer exist",
        )
    } else if payload.get("mode").is_some() {
        (
            "mode",
            "complete_captured no longer exists; send the update without mode, in an open lesson",
        )
    } else if payload
        .pointer("/session/operation")
        .and_then(Value::as_str)
        == Some("pause")
    {
        ("session.operation", "pause no longer exists")
    } else {
        return Ok(());
    };
    let mut error = field_error(removed.0, removed.1, host);
    if let Some(recovery) = error.recovery.as_mut() {
        recovery["arguments"]["stage"] = json!("consolidate_and_resume");
        recovery["instruction"] = json!(END_FOR_NOW);
    }
    Err(error)
}
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
        request_id: envelope.request_id,
        turn_ref: envelope.turn_ref,
        session,
        reported_events: envelope.reported_events,
        observations: envelope.observations,
        memory_edits,
        checkpoint: envelope.checkpoint,
        event_dispositions: envelope.event_dispositions,
        review_updates,
        practice: envelope.practice,
        attempt_corrections: envelope.attempt_corrections,
    })
}
